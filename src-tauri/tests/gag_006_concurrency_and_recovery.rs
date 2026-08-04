//! GAG-006 §12 integration tests: concurrency, event ordering, dedup,
//! gap detection, recovery, and snapshot consistency.
//!
//! These tests exercise the production `TaskRuntimeImpl` against an
//! in-memory SQLite repository and a no-op AgentRuntime.

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc;

use grok_acp_gui_lib::adapters::sqlite::SqliteRepository;
use grok_acp_gui_lib::bridge::types::{SessionId, TaskId};
use grok_acp_gui_lib::domain::error::codes;
use grok_acp_gui_lib::domain::error::DomainError;
use grok_acp_gui_lib::domain::types::{
    utc_now, Project, ProjectId, RecoveryAction, RecoveryDecision, Task, TaskStatus, WorkspaceKind,
};
use grok_acp_gui_lib::modules::agent_runtime::{
    AgentEvent, AgentRuntime, ClientRequest, EventMeta, RuntimeConfig, RuntimeHandle,
    RuntimeProbeResult, RuntimeState, SendAck, TimestampedEvent, WorkspaceContext,
};
use grok_acp_gui_lib::modules::persistence::Repository;
use grok_acp_gui_lib::modules::task_runtime::{TaskRuntime, TaskRuntimeImpl};

// ---------------------------------------------------------------------------
// Shared test infrastructure
// ---------------------------------------------------------------------------

struct NopAgent;

#[async_trait]
impl AgentRuntime for NopAgent {
    async fn probe(&self, _config: &RuntimeConfig) -> RuntimeProbeResult {
        RuntimeProbeResult::not_found()
    }
    async fn start(
        &self,
        session_id: SessionId,
        _workspace: WorkspaceContext,
        _config: &RuntimeConfig,
    ) -> Result<RuntimeHandle, DomainError> {
        Ok(RuntimeHandle {
            session_id,
            executable_path: "<nop>".into(),
        })
    }
    async fn send(
        &self,
        _session_id: SessionId,
        _request: ClientRequest,
    ) -> Result<SendAck, DomainError> {
        Err(DomainError::new("NOP", "no-op agent"))
    }
    async fn cancel(&self, _session_id: SessionId, _request_id: Option<u64>) {}
    async fn shutdown(&self, _session_id: SessionId, _reason: &str) {}
    fn subscribe(&self) -> mpsc::Receiver<TimestampedEvent> {
        mpsc::channel(1).1
    }
    fn session_state(&self, _session_id: &SessionId) -> Option<RuntimeState> {
        None
    }
}

fn make_project(id: &str, path: &str) -> Project {
    Project {
        id: ProjectId::new(id),
        path: path.into(),
        display_path: path.into(),
        repo_root: Some(path.into()),
        trusted_at: Some(utc_now()),
        last_opened_at: utc_now(),
    }
}

fn make_task(id: &str, project_id: &str, status: TaskStatus) -> Task {
    let now = utc_now();
    Task {
        id: TaskId::new(id),
        project_id: ProjectId::new(project_id),
        title: format!("Task {}", id),
        status,
        workspace_kind: WorkspaceKind::Worktree,
        mode: None,
        model: None,
        reasoning: None,
        created_at: now.clone(),
        updated_at: now,
        interrupt_reason: None,
        interrupted_at: None,
        attempt_count: 1,
    }
}

fn make_event(session_id: &SessionId, seq: u64, kind: AgentEvent) -> TimestampedEvent {
    TimestampedEvent {
        meta: EventMeta::new(session_id.clone(), seq),
        event: kind,
    }
}

fn delta_event(session_id: &SessionId, seq: u64, text: &str) -> TimestampedEvent {
    make_event(
        session_id,
        seq,
        AgentEvent::AssistantDelta(
            grok_acp_gui_lib::modules::agent_runtime::AssistantDeltaPayload { text: text.into() },
        ),
    )
}

// ---------------------------------------------------------------------------
// §12.1 — Event deduplication (idempotent accept)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn event_dedup_idempotent_on_duplicate_sequence() {
    let repo = Arc::new(SqliteRepository::open_in_memory().expect("in-memory repo"));
    repo.create_project(&make_project("p1", "/proj")).unwrap();

    let t = make_task("t1", "p1", TaskStatus::Preparing);
    repo.create_task(&t).unwrap();

    let rt = TaskRuntimeImpl::with_concurrency(repo.clone(), Arc::new(NopAgent), 4);
    let sid = SessionId::new("s1");

    rt.enqueue_task(TaskId::new("t1"), sid.clone())
        .await
        .expect("enqueue");

    // Accept seq 1.
    rt.accept_agent_event(delta_event(&sid, 1, "hello"))
        .await
        .expect("seq 1");

    // Re-accept the same seq 1 — should be rejected as duplicate.
    let result = rt.accept_agent_event(delta_event(&sid, 1, "hello")).await;
    assert!(result.is_err(), "duplicate event must be rejected");
    assert_eq!(
        result.unwrap_err().code,
        codes::EVENT_DUPLICATE,
        "should return EVENT_DUPLICATE"
    );

    // DB should have exactly 1 event.
    let events = repo.get_events_after(&sid.0, 0, 100).expect("get events");
    assert_eq!(events.len(), 1, "only one event persisted (dedup worked)");
}

// ---------------------------------------------------------------------------
// §12.2 — Gap detection (no silent skip)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn event_gap_detected_and_rejected() {
    let repo = Arc::new(SqliteRepository::open_in_memory().expect("in-memory repo"));
    repo.create_project(&make_project("p1", "/proj")).unwrap();

    let t = make_task("t1", "p1", TaskStatus::Preparing);
    repo.create_task(&t).unwrap();

    let rt = TaskRuntimeImpl::with_concurrency(repo.clone(), Arc::new(NopAgent), 4);
    let sid = SessionId::new("s1");

    rt.enqueue_task(TaskId::new("t1"), sid.clone())
        .await
        .expect("enqueue");

    // Accept seq 1.
    rt.accept_agent_event(delta_event(&sid, 1, "first"))
        .await
        .expect("seq 1");

    // Try to accept seq 5 (gap: 2,3,4 missing).
    let result = rt.accept_agent_event(delta_event(&sid, 5, "fifth")).await;
    assert!(result.is_err(), "gap event must be rejected");
    assert_eq!(
        result.unwrap_err().code,
        codes::EVENT_GAP_DETECTED,
        "should return EVENT_GAP_DETECTED"
    );

    // DB should still have only seq 1.
    let events = repo.get_events_after(&sid.0, 0, 100).expect("get events");
    assert_eq!(events.len(), 1, "gap event not persisted");
    assert_eq!(events[0].sequence, 1);
}

// ---------------------------------------------------------------------------
// §12.3 — Per-session ordering under concurrency (10+ sessions)
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_sessions_preserve_per_session_ordering() {
    let repo = Arc::new(SqliteRepository::open_in_memory().expect("in-memory repo"));
    repo.create_project(&make_project("p1", "/proj")).unwrap();

    const SESSION_COUNT: u32 = 12;
    const EVENTS_PER_SESSION: u64 = 5;
    let max_concurrent = SESSION_COUNT; // allow all to run

    let rt = Arc::new(TaskRuntimeImpl::with_concurrency(
        repo.clone(),
        Arc::new(NopAgent),
        max_concurrent,
    ));

    // Create and enqueue all tasks.
    let mut handles = Vec::new();
    for i in 0..SESSION_COUNT {
        let t = make_task(&format!("t{}", i), "p1", TaskStatus::Preparing);
        repo.create_task(&t).unwrap();
    }

    for i in 0..SESSION_COUNT {
        let rt = rt.clone();
        let sid = SessionId::new(format!("s{}", i));
        let task_id = TaskId::new(format!("t{}", i));

        rt.enqueue_task(task_id.clone(), sid.clone())
            .await
            .unwrap_or_else(|_| panic!("enqueue session {}", i));

        let handle = tokio::spawn(async move {
            for seq in 1..=EVENTS_PER_SESSION {
                let ev = delta_event(&sid, seq, &format!("s{}-e{}", i, seq));
                rt.accept_agent_event(ev)
                    .await
                    .unwrap_or_else(|_| panic!("accept s{} seq {}", i, seq));
            }
        });
        handles.push(handle);
    }

    // Wait for all sessions to complete.
    for h in handles {
        h.await.expect("session task completed");
    }

    // Verify: each session has exactly EVENTS_PER_SESSION events, in order.
    for i in 0..SESSION_COUNT {
        let sid = format!("s{}", i);
        let events = repo
            .get_events_after(&sid, 0, 100)
            .unwrap_or_else(|_| panic!("get events for s{}", i));
        assert_eq!(
            events.len(),
            EVENTS_PER_SESSION as usize,
            "session {} should have {} events",
            i,
            EVENTS_PER_SESSION
        );

        // Verify monotonic sequences within the session.
        for (expected_seq, ev) in (1u64..).zip(events.iter()) {
            assert_eq!(
                ev.sequence, expected_seq,
                "session {}: expected seq {}, got {}",
                i, expected_seq, ev.sequence
            );
        }
    }

    // Verify total event count.
    let total = SESSION_COUNT as usize * EVENTS_PER_SESSION as usize;
    // Count events across all sessions by iterating.
    let mut count = 0usize;
    for i in 0..SESSION_COUNT {
        let sid = format!("s{}", i);
        count += repo.get_events_after(&sid, 0, 100).unwrap().len();
    }
    assert_eq!(count, total, "total events across all sessions");
}

// ---------------------------------------------------------------------------
// §12.4 — Recovery decision: resume creates new attempt
// ---------------------------------------------------------------------------

#[tokio::test]
async fn recovery_resume_increments_attempt_and_resets_status() {
    let repo = Arc::new(SqliteRepository::open_in_memory().expect("in-memory repo"));
    repo.create_project(&make_project("p1", "/proj")).unwrap();

    let t = make_task("t1", "p1", TaskStatus::Interrupted);
    repo.create_task(&t).unwrap();

    let rt = TaskRuntimeImpl::with_concurrency(repo.clone(), Arc::new(NopAgent), 4);

    // Apply resume decision.
    let decision = RecoveryDecision {
        task_id: TaskId::new("t1"),
        action: RecoveryAction::Resume,
        decided_at: utc_now(),
    };
    rt.recover_session(decision).await.expect("recover_session");

    let task = repo.get_task("t1").expect("get task");
    assert_eq!(
        task.status,
        TaskStatus::Preparing,
        "resumed task should be Preparing"
    );
    assert_eq!(task.attempt_count, 2, "attempt_count should increment to 2");
    assert!(task.interrupt_reason.is_none(), "interrupt_reason cleared");
}

#[tokio::test]
async fn recovery_archive_moves_to_archived() {
    let repo = Arc::new(SqliteRepository::open_in_memory().expect("in-memory repo"));
    repo.create_project(&make_project("p1", "/proj")).unwrap();

    let t = make_task("t1", "p1", TaskStatus::Interrupted);
    repo.create_task(&t).unwrap();

    let rt = TaskRuntimeImpl::with_concurrency(repo.clone(), Arc::new(NopAgent), 4);

    let decision = RecoveryDecision {
        task_id: TaskId::new("t1"),
        action: RecoveryAction::Archive,
        decided_at: utc_now(),
    };
    rt.recover_session(decision).await.expect("recover_session");

    let task = repo.get_task("t1").expect("get task");
    assert_eq!(task.status, TaskStatus::Archived, "should be archived");
}

// ---------------------------------------------------------------------------
// §12.5 — Snapshot + cursor delta delivery
// ---------------------------------------------------------------------------

#[tokio::test]
async fn snapshot_delta_delivers_only_new_events_after_cursor() {
    let repo = Arc::new(SqliteRepository::open_in_memory().expect("in-memory repo"));
    repo.create_project(&make_project("p1", "/proj")).unwrap();

    let t = make_task("t1", "p1", TaskStatus::Preparing);
    repo.create_task(&t).unwrap();

    let rt = TaskRuntimeImpl::with_concurrency(repo.clone(), Arc::new(NopAgent), 4);
    let sid = SessionId::new("s1");
    let tid = TaskId::new("t1");

    rt.enqueue_task(tid.clone(), sid.clone())
        .await
        .expect("enqueue");

    // Produce 3 events.
    rt.accept_agent_event(delta_event(&sid, 1, "e1"))
        .await
        .expect("seq 1");
    rt.accept_agent_event(delta_event(&sid, 2, "e2"))
        .await
        .expect("seq 2");
    rt.accept_agent_event(delta_event(&sid, 3, "e3"))
        .await
        .expect("seq 3");

    // Get full snapshot (no cursor).
    let snap = rt
        .get_snapshot(tid.clone(), sid.clone(), None)
        .await
        .expect("snapshot");
    assert_eq!(snap.last_seq, 3, "full snapshot has all events");
    assert_eq!(snap.recent_events.len(), 3);

    // Get delta from cursor at seq 1 (should return events 2,3).
    let cursor = grok_acp_gui_lib::domain::types::TimelineCursor {
        session_id: sid.clone(),
        last_seq: 1,
        last_event_at: utc_now(),
    };
    let delta = rt
        .get_snapshot(tid, sid, Some(cursor))
        .await
        .expect("delta snapshot");
    assert_eq!(delta.last_seq, 3, "delta snapshot last_seq is 3");
    assert_eq!(
        delta.recent_events.len(),
        2,
        "delta returns only 2 new events"
    );
    assert_eq!(delta.recent_events[0].sequence, 2);
    assert_eq!(delta.recent_events[1].sequence, 3);
}

// §12.6 is covered by the module-level unit test
// `mailbox::tests::has_side_effects_classification`.

// ---------------------------------------------------------------------------
// §12.7 — Interrupted task recovery candidates listed correctly
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_recovery_candidates_returns_interrupted_only() {
    let repo = Arc::new(SqliteRepository::open_in_memory().expect("in-memory repo"));
    repo.create_project(&make_project("p1", "/proj")).unwrap();

    // Create tasks in various states.
    let running = make_task("t_running", "p1", TaskStatus::Running);
    let interrupted = make_task("t_interrupted", "p1", TaskStatus::Interrupted);
    let merged = make_task("t_merged", "p1", TaskStatus::Merged);
    repo.create_task(&running).unwrap();
    repo.create_task(&interrupted).unwrap();
    repo.create_task(&merged).unwrap();

    let rt = TaskRuntimeImpl::with_concurrency(repo.clone(), Arc::new(NopAgent), 4);

    let candidates = rt
        .list_recovery_candidates()
        .await
        .expect("list candidates");

    // Only the interrupted task should appear.
    assert_eq!(candidates.len(), 1, "only one recovery candidate");
    assert_eq!(candidates[0].task_id.0, "t_interrupted");
    assert_eq!(candidates[0].previous_status, TaskStatus::Interrupted);
}

// ---------------------------------------------------------------------------
// §12.8 — Multiple interleaved sessions do not cross-contaminate events
// ---------------------------------------------------------------------------

#[tokio::test]
async fn interleaved_session_events_do_not_cross_contaminate() {
    let repo = Arc::new(SqliteRepository::open_in_memory().expect("in-memory repo"));
    repo.create_project(&make_project("p1", "/proj")).unwrap();

    for i in 0..3u32 {
        let t = make_task(&format!("t{}", i), "p1", TaskStatus::Preparing);
        repo.create_task(&t).unwrap();
    }

    let rt = TaskRuntimeImpl::with_concurrency(repo.clone(), Arc::new(NopAgent), 4);

    let sid_a = SessionId::new("sa");
    let sid_b = SessionId::new("sb");
    let sid_c = SessionId::new("sc");

    rt.enqueue_task(TaskId::new("t0"), sid_a.clone())
        .await
        .expect("enqueue a");
    rt.enqueue_task(TaskId::new("t1"), sid_b.clone())
        .await
        .expect("enqueue b");
    rt.enqueue_task(TaskId::new("t2"), sid_c.clone())
        .await
        .expect("enqueue c");

    // Interleave events across sessions.
    rt.accept_agent_event(delta_event(&sid_a, 1, "a1"))
        .await
        .expect("a1");
    rt.accept_agent_event(delta_event(&sid_b, 1, "b1"))
        .await
        .expect("b1");
    rt.accept_agent_event(delta_event(&sid_a, 2, "a2"))
        .await
        .expect("a2");
    rt.accept_agent_event(delta_event(&sid_c, 1, "c1"))
        .await
        .expect("c1");
    rt.accept_agent_event(delta_event(&sid_b, 2, "b2"))
        .await
        .expect("b2");
    rt.accept_agent_event(delta_event(&sid_c, 2, "c2"))
        .await
        .expect("c2");
    rt.accept_agent_event(delta_event(&sid_a, 3, "a3"))
        .await
        .expect("a3");

    // Verify each session's events are ordered and independent.
    let events_a = repo.get_events_after("sa", 0, 100).unwrap();
    let events_b = repo.get_events_after("sb", 0, 100).unwrap();
    let events_c = repo.get_events_after("sc", 0, 100).unwrap();

    assert_eq!(events_a.len(), 3);
    assert_eq!(events_b.len(), 2);
    assert_eq!(events_c.len(), 2);

    // Verify sequences within each session.
    for (i, ev) in events_a.iter().enumerate() {
        assert_eq!(ev.sequence, i as u64 + 1);
    }
    for (i, ev) in events_b.iter().enumerate() {
        assert_eq!(ev.sequence, i as u64 + 1);
    }
    for (i, ev) in events_c.iter().enumerate() {
        assert_eq!(ev.sequence, i as u64 + 1);
    }
}
