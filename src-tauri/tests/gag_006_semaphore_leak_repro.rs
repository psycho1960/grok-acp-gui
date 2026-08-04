//! GAG-006 regression test: semaphore permit lifecycle.
//!
//! Verifies that:
//! 1. enqueue_task returns CONCURRENCY_LIMIT_EXCEEDED when the limit is hit
//!    (instead of silently returning Ok(()))
//! 2. cancel_session releases the permit, allowing a new task to start

use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc;

use grok_acp_gui_lib::adapters::sqlite::SqliteRepository;
use grok_acp_gui_lib::bridge::types::{SessionId, TaskId};
use grok_acp_gui_lib::domain::error::codes;
use grok_acp_gui_lib::domain::error::DomainError;
use grok_acp_gui_lib::domain::types::{
    utc_now, Project, ProjectId, Task, TaskStatus, WorkspaceKind,
};
use grok_acp_gui_lib::modules::agent_runtime::{
    AgentRuntime, ClientRequest, RuntimeConfig, RuntimeHandle, RuntimeProbeResult, RuntimeState,
    SendAck, TimestampedEvent, WorkspaceContext,
};
use grok_acp_gui_lib::modules::persistence::Repository;
use grok_acp_gui_lib::modules::task_runtime::{TaskRuntime, TaskRuntimeImpl};

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

#[tokio::test]
async fn enqueue_respects_concurrency_limit_then_releases_on_cancel() {
    let repo = Arc::new(SqliteRepository::open_in_memory().expect("in-memory repo"));
    repo.create_project(&make_project("p1", "/proj"))
        .expect("create project");

    for i in 0..3u32 {
        let t = make_task(&format!("t{}", i), "p1", TaskStatus::Preparing);
        repo.create_task(&t).expect("create task");
    }

    // max_concurrent = 2
    let rt = TaskRuntimeImpl::with_concurrency(repo.clone(), Arc::new(NopAgent), 2);

    // Phase 1: first 2 tasks acquire permits; 3rd is rejected.
    assert!(
        rt.enqueue_task(TaskId::new("t0"), SessionId::new("s0"))
            .await
            .is_ok(),
        "enqueue 0 should succeed"
    );
    assert!(
        rt.enqueue_task(TaskId::new("t1"), SessionId::new("s1"))
            .await
            .is_ok(),
        "enqueue 1 should succeed"
    );

    let r2 = rt
        .enqueue_task(TaskId::new("t2"), SessionId::new("s2"))
        .await;
    assert!(r2.is_err(), "3rd enqueue should fail with limit exceeded");
    assert_eq!(
        r2.unwrap_err().code,
        codes::CONCURRENCY_LIMIT_EXCEEDED,
        "3rd enqueue should return CONCURRENCY_LIMIT_EXCEEDED"
    );

    let bindings = repo.list_active_bindings().expect("list bindings");
    assert_eq!(bindings.len(), 2, "only 2 sessions should be active");

    // Phase 2: cancel both sessions, releasing permits.
    rt.cancel_session(TaskId::new("t0"))
        .await
        .expect("cancel t0");
    rt.cancel_session(TaskId::new("t1"))
        .await
        .expect("cancel t1");

    // Phase 3: now a new task can acquire a permit.
    let repo_task = make_task("t3", "p1", TaskStatus::Preparing);
    repo.create_task(&repo_task).expect("create t3");

    assert!(
        rt.enqueue_task(TaskId::new("t3"), SessionId::new("s3"))
            .await
            .is_ok(),
        "after cancelling, 4th enqueue should succeed (permit released)"
    );

    let bindings_after = repo.list_active_bindings().expect("list bindings");
    assert_eq!(
        bindings_after.len(),
        3,
        "3 bindings expected after permit release; got {}",
        bindings_after.len()
    );
}
