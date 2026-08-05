//! Production-path acceptance tests for GAG-008.
//!
//! Public seam: DesktopBridge dispatcher backed by real SQLite,
//! TaskRuntime, AgentRuntime, and the process-based Fake ACP agent.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use grok_acp_gui_lib::adapters::grok_acp::{FakeAcpTransport, FakeScenario};
use grok_acp_gui_lib::adapters::sqlite::SqliteRepository;
use grok_acp_gui_lib::bridge::dispatch::{execute_impl, DesktopResult};
use grok_acp_gui_lib::domain::types::{utc_now, Project, ProjectId, TaskStatus};
use grok_acp_gui_lib::modules::agent_runtime::{AgentRuntime, AgentRuntimeImpl};
use grok_acp_gui_lib::modules::persistence::Repository;
use grok_acp_gui_lib::modules::task_runtime::{TaskRuntime, TaskRuntimeImpl};

fn fake_agent_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("tests")
        .join("fake-acp-agent")
        .join("agent.mjs")
}

fn temporary_database(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "gag-008-{label}-{}-{nonce}.sqlite",
        std::process::id()
    ))
}

async fn cleanup_temporary_database(path: &std::path::Path) {
    let temp_root = std::env::temp_dir();
    assert!(
        path.starts_with(&temp_root),
        "refuse to clean non-temp path"
    );
    for candidate in [
        path.to_path_buf(),
        PathBuf::from(format!("{}-wal", path.display())),
        PathBuf::from(format!("{}-shm", path.display())),
    ] {
        let mut last_error = None;
        for _ in 0..50 {
            match std::fs::remove_file(&candidate) {
                Ok(()) => {
                    last_error = None;
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    last_error = None;
                    break;
                }
                Err(error) => {
                    last_error = Some(error);
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }
        }
        if let Some(error) = last_error {
            panic!(
                "temporary database cleanup failed for {}: {error}",
                candidate.display()
            );
        }
    }
}

async fn wait_for_task_status(repo: &SqliteRepository, task_id: &str, expected: TaskStatus) {
    // Generous deadline: CI runners spawn Node + the fake ACP agent per test,
    // and a loaded runner can take well over 5s to complete a turn.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while tokio::time::Instant::now() < deadline {
        if repo.get_task(task_id).expect("task query").status == expected {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("task {task_id} did not reach {expected:?}");
}

#[tokio::test]
async fn missing_managed_worktree_never_falls_back_to_the_project_checkout() {
    let repo = Arc::new(SqliteRepository::open_in_memory().expect("in-memory repository"));
    let cwd = std::env::current_dir().expect("workspace");
    repo.create_project(&Project {
        id: ProjectId::new("project-worktree-gate"),
        path: cwd.to_string_lossy().into_owned(),
        display_path: "worktree-gate-fixture".into(),
        repo_root: Some(cwd.to_string_lossy().into_owned()),
        trusted_at: Some(utc_now()),
        last_opened_at: utc_now(),
    })
    .expect("project");
    let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
        FakeScenario::Normal,
        fake_agent_path(),
    ));
    let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
    task_runtime.spawn_agent_event_forwarder();

    let result = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "task.create",
            "payload": {
                "projectId": "project-worktree-gate",
                "title": "Must stay isolated",
                "prompt": "inspect the project",
                "mode": "agent",
                "workspaceStrategy": "worktree"
            }
        }),
    )
    .await;

    let data = match result {
        DesktopResult::Ok { data } => data,
        DesktopResult::Err { error } => panic!("task draft should be preserved: {error:?}"),
    };
    assert_eq!(data["task"]["status"], "failed");
    assert_eq!(data["startError"]["code"], "WORKTREE_NOT_READY");
    let task_id = data["taskId"].as_str().expect("task id");
    let binding = repo
        .get_binding_by_task(task_id)
        .expect("binding query")
        .expect("preserved binding");
    assert_ne!(
        binding.cwd.as_deref(),
        Some(cwd.to_string_lossy().as_ref()),
        "an isolated task must never execute in the original checkout"
    );
}

#[tokio::test]
async fn task_model_is_validated_before_the_acp_process_starts() {
    let repo = Arc::new(SqliteRepository::open_in_memory().expect("in-memory repository"));
    let cwd = std::env::current_dir().expect("workspace");
    repo.create_project(&Project {
        id: ProjectId::new("project-model-gate"),
        path: cwd.to_string_lossy().into_owned(),
        display_path: "model-gate-fixture".into(),
        repo_root: Some(cwd.to_string_lossy().into_owned()),
        trusted_at: Some(utc_now()),
        last_opened_at: utc_now(),
    })
    .expect("project");
    let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
        FakeScenario::Normal,
        fake_agent_path(),
    ));
    let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
    task_runtime.spawn_agent_event_forwarder();

    let result = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "task.create",
            "payload": {
                "projectId": "project-model-gate",
                "title": "Reject option injection",
                "prompt": "hello",
                "mode": "ask",
                "model": "--always-approve",
                "workspaceStrategy": "direct"
            }
        }),
    )
    .await;

    let data = match result {
        DesktopResult::Ok { data } => data,
        DesktopResult::Err { error } => panic!("task draft should be preserved: {error:?}"),
    };
    assert_eq!(data["task"]["status"], "failed");
    assert_eq!(data["startError"]["code"], "RUNTIME_INVALID_MODEL");
    assert!(!data.to_string().contains("--always-approve"));
}

#[tokio::test]
async fn create_send_stream_persist_and_reopen_real_production_path() {
    let repo = Arc::new(SqliteRepository::open_in_memory().expect("in-memory repository"));
    let cwd = std::env::current_dir().expect("workspace");
    repo.create_project(&Project {
        id: ProjectId::new("project-bridge"),
        path: cwd.to_string_lossy().into_owned(),
        display_path: "bridge-fixture".into(),
        repo_root: Some(cwd.to_string_lossy().into_owned()),
        trusted_at: Some(utc_now()),
        last_opened_at: utc_now(),
    })
    .expect("project");

    let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
        FakeScenario::Normal,
        fake_agent_path(),
    ));
    let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
    task_runtime.spawn_agent_event_forwarder();
    let mut events = task_runtime.event_subscriber();

    let created = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "task.create",
            "payload": {
                "projectId": "project-bridge",
                "title": "Production bridge turn",
                "prompt": "Hello",
                "mode": "ask",
                "workspaceStrategy": "direct"
            }
        }),
    )
    .await;
    let task_id = match created {
        DesktopResult::Ok { data } => data["taskId"].as_str().unwrap().to_string(),
        DesktopResult::Err { error } => panic!("create failed: {error:?}"),
    };

    // Generous deadline: CI runners spawn Node + the fake ACP agent per test.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut streamed = String::new();
    let mut idle = false;
    while tokio::time::Instant::now() < deadline && !idle {
        if let Ok(Ok(event)) = tokio::time::timeout(Duration::from_secs(1), events.recv()).await {
            assert_eq!(
                event.task_id.as_ref().map(|id| id.0.as_str()),
                Some(task_id.as_str())
            );
            if event.event_type == "message.delta" {
                streamed.push_str(
                    event
                        .payload
                        .get("text")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                );
            }
            if event.event_type == "task.state" {
                idle = event.payload.get("status").and_then(|v| v.as_str()) == Some("idle");
            }
        }
    }
    assert!(
        streamed.contains("Hello from fake ACP agent!"),
        "streamed={streamed:?}"
    );
    assert!(idle, "turn must return to idle");

    let reopened = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "task.open",
            "payload": { "taskId": task_id }
        }),
    )
    .await;
    let data = match reopened {
        DesktopResult::Ok { data } => data,
        DesktopResult::Err { error } => panic!("open failed: {error:?}"),
    };
    let history = data["events"].as_array().expect("timeline events");
    assert_eq!(
        history
            .iter()
            .filter(|event| event["payload"]["role"] == "user")
            .count(),
        1,
        "confirmed user message must be restored exactly once: {history:?}"
    );
    assert!(history.iter().any(|event| event["payload"]["text"]
        .as_str()
        .is_some_and(|text| text.contains("Hello"))));
    let tool_events: Vec<_> = history
        .iter()
        .filter(|event| event["payload"].get("toolCall").is_some())
        .collect();
    assert_eq!(
        tool_events.len(),
        2,
        "one tool lifecycle must merge by id: {tool_events:?}"
    );
    assert_eq!(
        tool_events[0]["payload"]["toolCall"]["toolCallId"],
        tool_events[1]["payload"]["toolCall"]["toolCallId"]
    );
    assert_eq!(tool_events[1]["payload"]["toolCall"]["status"], "completed");
    assert!(tool_events[1]["payload"]["toolCall"]["durationMs"].is_number());
    let visible_history = serde_json::to_string(history).unwrap();
    assert!(!visible_history.contains("rawInput"));
    assert!(!visible_history.contains("rawOutput"));
    assert!(!visible_history.contains("fixture.txt"));
}

#[tokio::test]
async fn two_tasks_are_isolated_and_stopping_one_does_not_stop_the_other() {
    let repo = Arc::new(SqliteRepository::open_in_memory().expect("in-memory repository"));
    let cwd = std::env::current_dir().expect("workspace");
    repo.create_project(&Project {
        id: ProjectId::new("project-isolation"),
        path: cwd.to_string_lossy().into_owned(),
        display_path: "isolation-fixture".into(),
        repo_root: Some(cwd.to_string_lossy().into_owned()),
        trusted_at: Some(utc_now()),
        last_opened_at: utc_now(),
    })
    .expect("project");
    let runtime =
        AgentRuntimeImpl::new(FakeAcpTransport::new(FakeScenario::Slow, fake_agent_path()));
    let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
    task_runtime.spawn_agent_event_forwarder();
    let mut events = task_runtime.event_subscriber();

    let create = |title: &str, prompt: &str| {
        serde_json::json!({
            "type": "task.create",
            "payload": {
                "projectId": "project-isolation",
                "title": title,
                "prompt": prompt,
                "mode": "ask",
                "workspaceStrategy": "direct"
            }
        })
    };
    let task_a = match execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        create("A", "question only for A"),
    )
    .await
    {
        DesktopResult::Ok { data } => data["taskId"].as_str().unwrap().to_string(),
        DesktopResult::Err { error } => panic!("A create failed: {error:?}"),
    };
    let task_b = match execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        create("B", "question only for B"),
    )
    .await
    {
        DesktopResult::Ok { data } => data["taskId"].as_str().unwrap().to_string(),
        DesktopResult::Err { error } => panic!("B create failed: {error:?}"),
    };

    let cancelled = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({ "type": "turn.cancel", "payload": { "taskId": task_a } }),
    )
    .await;
    assert!(matches!(cancelled, DesktopResult::Ok { .. }));

    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let mut a_stopped = false;
    let mut b_idle = false;
    while tokio::time::Instant::now() < deadline && !(a_stopped && b_idle) {
        if let Ok(Ok(event)) = tokio::time::timeout(Duration::from_millis(500), events.recv()).await
        {
            let event_task = event.task_id.as_ref().map(|id| id.0.as_str());
            assert!(event_task == Some(task_a.as_str()) || event_task == Some(task_b.as_str()));
            if event.payload.get("role").and_then(|value| value.as_str()) == Some("user") {
                let text = event
                    .payload
                    .get("text")
                    .and_then(|value| value.as_str())
                    .unwrap();
                if event_task == Some(task_a.as_str()) {
                    assert_eq!(text, "question only for A");
                } else {
                    assert_eq!(text, "question only for B");
                }
            }
            if event.event_type == "task.state" && event_task == Some(task_a.as_str()) {
                a_stopped = event.payload["detail"]["reason"] == "cancelled";
            }
            if event.event_type == "task.state" && event_task == Some(task_b.as_str()) {
                b_idle = event.payload["status"] == "idle";
            }
        }
    }
    assert!(a_stopped, "A must expose its own stopped state");
    assert!(b_idle, "B must complete even while A is stopped");

    let resend = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "turn.send",
            "payload": { "taskId": task_a, "message": "second question for A" }
        }),
    )
    .await;
    assert!(
        matches!(resend, DesktopResult::Ok { .. }),
        "a stopped task must accept another turn: {resend:?}"
    );
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let mut got_second_user = false;
    let mut a_idle = false;
    while tokio::time::Instant::now() < deadline && !(got_second_user && a_idle) {
        if let Ok(Ok(event)) = tokio::time::timeout(Duration::from_millis(500), events.recv()).await
        {
            if event.task_id.as_ref().map(|id| id.0.as_str()) != Some(task_a.as_str()) {
                continue;
            }
            got_second_user |=
                event.payload["role"] == "user" && event.payload["text"] == "second question for A";
            a_idle |= event.event_type == "task.state" && event.payload["status"] == "idle";
        }
    }
    assert!(got_second_user, "the restarted turn must stay scoped to A");
    assert!(a_idle, "A must return to idle after its restarted turn");
}

#[tokio::test]
async fn interrupted_session_resume_continues_sequence_and_attempt_history() {
    let repo = Arc::new(SqliteRepository::open_in_memory().expect("in-memory repository"));
    let cwd = std::env::current_dir().expect("workspace");
    repo.create_project(&Project {
        id: ProjectId::new("project-resume"),
        path: cwd.to_string_lossy().into_owned(),
        display_path: "resume-fixture".into(),
        repo_root: Some(cwd.to_string_lossy().into_owned()),
        trusted_at: Some(utc_now()),
        last_opened_at: utc_now(),
    })
    .expect("project");
    let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
        FakeScenario::CrashAfterPrompt,
        fake_agent_path(),
    ));
    let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
    task_runtime.spawn_agent_event_forwarder();
    let mut events = task_runtime.event_subscriber();
    let task_id = match execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "task.create",
            "payload": {
                "projectId": "project-resume",
                "title": "Resume",
                "prompt": "crash once",
                "workspaceStrategy": "direct"
            }
        }),
    )
    .await
    {
        DesktopResult::Ok { data } => data["taskId"].as_str().unwrap().to_string(),
        DesktopResult::Err { error } => panic!("create failed: {error:?}"),
    };

    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    let mut interrupted = false;
    while tokio::time::Instant::now() < deadline {
        if let Ok(Ok(event)) = tokio::time::timeout(Duration::from_millis(250), events.recv()).await
        {
            if event.event_type == "task.state" && event.payload["status"] == "interrupted" {
                interrupted = true;
                break;
            }
        }
    }
    assert!(
        interrupted,
        "process crash must become a task-scoped interrupted event"
    );
    let binding_before = repo
        .get_binding_by_task(&task_id)
        .expect("binding query")
        .expect("binding");
    let seq_before = binding_before.last_seq;
    assert!(seq_before >= 2);

    let resumed = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({ "type": "session.resume", "payload": { "taskId": task_id } }),
    )
    .await;
    assert!(
        matches!(resumed, DesktopResult::Ok { .. }),
        "resume={resumed:?}"
    );

    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while tokio::time::Instant::now() < deadline {
        let binding = repo
            .get_binding_by_task(&task_id)
            .expect("binding query")
            .expect("binding");
        if binding.last_seq > seq_before {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let binding_after = repo
        .get_binding_by_task(&task_id)
        .expect("binding query")
        .expect("binding");
    assert!(
        binding_after.last_seq > seq_before,
        "sequence did not continue"
    );
    assert_eq!(binding_after.attempt_number, 2);
    assert!(
        !repo
            .get_events_for_attempt(&binding_after.session_id.0, 2)
            .expect("attempt events")
            .is_empty(),
        "resumed process events must be recorded under attempt 2"
    );
}

#[tokio::test]
async fn idle_process_exit_is_persisted_as_interrupted_and_reopens_with_history() {
    let repo = Arc::new(SqliteRepository::open_in_memory().expect("in-memory repository"));
    let cwd = std::env::current_dir().expect("workspace");
    repo.create_project(&Project {
        id: ProjectId::new("project-idle-crash"),
        path: cwd.to_string_lossy().into_owned(),
        display_path: "idle-crash-fixture".into(),
        repo_root: Some(cwd.to_string_lossy().into_owned()),
        trusted_at: Some(utc_now()),
        last_opened_at: utc_now(),
    })
    .expect("project");
    let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
        FakeScenario::Normal,
        fake_agent_path(),
    ));
    let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
    task_runtime.spawn_agent_event_forwarder();
    let task_id = match execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "task.create",
            "payload": {
                "projectId": "project-idle-crash",
                "title": "Idle crash",
                "prompt": "persist this history",
                "mode": "ask",
                "workspaceStrategy": "direct"
            }
        }),
    )
    .await
    {
        DesktopResult::Ok { data } => data["taskId"].as_str().unwrap().to_string(),
        DesktopResult::Err { error } => panic!("create failed: {error:?}"),
    };
    wait_for_task_status(repo.as_ref(), &task_id, TaskStatus::Idle).await;
    let binding = repo
        .get_binding_by_task(&task_id)
        .expect("binding query")
        .expect("binding");

    for index in 1..=600 {
        task_runtime
            .accept_agent_event(grok_acp_gui_lib::modules::agent_runtime::TimestampedEvent {
                meta: grok_acp_gui_lib::modules::agent_runtime::EventMeta::new(
                    binding.session_id.clone(),
                    binding.last_seq + index,
                ),
                event: grok_acp_gui_lib::modules::agent_runtime::AgentEvent::AssistantDelta(
                    grok_acp_gui_lib::modules::agent_runtime::AssistantDeltaPayload {
                        text: format!("bulk-{index};"),
                    },
                ),
            })
            .await
            .expect("persist bulk assistant delta");
    }
    let binding = repo
        .get_binding_by_task(&task_id)
        .expect("binding query after bulk history")
        .expect("binding after bulk history");

    task_runtime
        .accept_agent_event(grok_acp_gui_lib::modules::agent_runtime::TimestampedEvent {
            meta: grok_acp_gui_lib::modules::agent_runtime::EventMeta::new(
                binding.session_id.clone(),
                binding.last_seq + 1,
            ),
            event: grok_acp_gui_lib::modules::agent_runtime::AgentEvent::ProcessExited(
                grok_acp_gui_lib::modules::agent_runtime::events::ProcessExitedPayload {
                    code: Some(23),
                    signal: None,
                    reason: "simulated idle crash".into(),
                },
            ),
        })
        .await
        .expect("persist process exit");

    assert_eq!(
        repo.get_task(&task_id).expect("task after crash").status,
        TaskStatus::Interrupted
    );
    let reopened = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({ "type": "task.open", "payload": { "taskId": task_id } }),
    )
    .await;
    let data = match reopened {
        DesktopResult::Ok { data } => data,
        DesktopResult::Err { error } => panic!("open failed: {error:?}"),
    };
    assert_eq!(data["status"], "interrupted");
    let history = data["events"].as_array().expect("history");
    assert!(history.len() < 50, "streaming history was not compacted");
    assert!(history
        .iter()
        .any(|event| event["payload"]["role"] == "user"));
    let restored_assistant: String = history
        .iter()
        .filter(|event| event["payload"]["role"] == "assistant")
        .filter_map(|event| event["payload"]["text"].as_str())
        .collect();
    assert!(restored_assistant.contains("bulk-1;"));
    assert!(restored_assistant.contains("bulk-600;"));
    assert!(history.iter().any(|event| {
        event["type"] == "task.state" && event["payload"]["status"] == "interrupted"
    }));

    runtime.shutdown_all("test complete").await;
}

#[tokio::test]
async fn confirmed_history_survives_a_real_database_close_and_reopen() {
    let db_path = temporary_database("history-reopen");
    let cwd = std::env::current_dir().expect("workspace");
    let task_id;

    {
        let repo = Arc::new(SqliteRepository::open(&db_path).expect("disk repository"));
        repo.create_project(&Project {
            id: ProjectId::new("project-disk-reopen"),
            path: cwd.to_string_lossy().into_owned(),
            display_path: "disk-reopen-fixture".into(),
            repo_root: Some(cwd.to_string_lossy().into_owned()),
            trusted_at: Some(utc_now()),
            last_opened_at: utc_now(),
        })
        .expect("project");
        let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
            FakeScenario::Normal,
            fake_agent_path(),
        ));
        let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
        task_runtime.spawn_agent_event_forwarder();

        task_id = match execute_impl(
            repo.as_ref(),
            runtime.as_ref(),
            task_runtime.as_ref(),
            serde_json::json!({
                "type": "task.create",
                "payload": {
                    "projectId": "project-disk-reopen",
                    "title": "Persist across process",
                    "prompt": "durable history",
                    "mode": "ask",
                    "workspaceStrategy": "direct"
                }
            }),
        )
        .await
        {
            DesktopResult::Ok { data } => data["taskId"].as_str().unwrap().to_string(),
            DesktopResult::Err { error } => panic!("create failed: {error:?}"),
        };
        wait_for_task_status(repo.as_ref(), &task_id, TaskStatus::Idle).await;
        runtime.shutdown_all("test application exit").await;
    }

    {
        let repo = Arc::new(SqliteRepository::open(&db_path).expect("reopened repository"));
        assert_eq!(
            repo.get_task(&task_id).expect("restored task").status,
            TaskStatus::Idle
        );
        let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
            FakeScenario::Normal,
            fake_agent_path(),
        ));
        let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
        let reopened = execute_impl(
            repo.as_ref(),
            runtime.as_ref(),
            task_runtime.as_ref(),
            serde_json::json!({ "type": "task.open", "payload": { "taskId": task_id } }),
        )
        .await;
        let data = match reopened {
            DesktopResult::Ok { data } => data,
            DesktopResult::Err { error } => panic!("open failed: {error:?}"),
        };
        let history = data["events"].as_array().expect("persisted history");
        assert_eq!(data["status"], "idle");
        assert_eq!(
            history
                .iter()
                .filter(|event| event["payload"]["role"] == "user")
                .count(),
            1
        );
        let restored_assistant: String = history
            .iter()
            .filter(|event| event["payload"]["role"] == "assistant")
            .filter_map(|event| event["payload"]["text"].as_str())
            .collect();
        assert!(
            restored_assistant.contains("Hello from fake ACP agent!"),
            "restored assistant deltas were {restored_assistant:?}"
        );
    }

    cleanup_temporary_database(&db_path).await;
}

#[tokio::test]
async fn an_inflight_turn_reopens_as_interrupted_and_keeps_partial_history() {
    let db_path = temporary_database("inflight-reopen");
    let cwd = std::env::current_dir().expect("workspace");
    let task_id;

    {
        let repo = Arc::new(SqliteRepository::open(&db_path).expect("disk repository"));
        repo.create_project(&Project {
            id: ProjectId::new("project-inflight-reopen"),
            path: cwd.to_string_lossy().into_owned(),
            display_path: "inflight-reopen-fixture".into(),
            repo_root: Some(cwd.to_string_lossy().into_owned()),
            trusted_at: Some(utc_now()),
            last_opened_at: utc_now(),
        })
        .expect("project");
        let runtime =
            AgentRuntimeImpl::new(FakeAcpTransport::new(FakeScenario::Slow, fake_agent_path()));
        let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
        task_runtime.spawn_agent_event_forwarder();

        task_id = match execute_impl(
            repo.as_ref(),
            runtime.as_ref(),
            task_runtime.as_ref(),
            serde_json::json!({
                "type": "task.create",
                "payload": {
                    "projectId": "project-inflight-reopen",
                    "title": "Interrupted restart",
                    "prompt": "keep this partial request",
                    "mode": "ask",
                    "workspaceStrategy": "direct"
                }
            }),
        )
        .await
        {
            DesktopResult::Ok { data } => data["taskId"].as_str().unwrap().to_string(),
            DesktopResult::Err { error } => panic!("create failed: {error:?}"),
        };
        wait_for_task_status(repo.as_ref(), &task_id, TaskStatus::Running).await;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while repo
            .get_binding_by_task(&task_id)
            .expect("binding query")
            .is_some_and(|binding| binding.last_seq < 2)
            && tokio::time::Instant::now() < deadline
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        runtime.shutdown_all("simulated application exit").await;
    }

    {
        let repo = Arc::new(SqliteRepository::open(&db_path).expect("reopened repository"));
        repo.recover_interrupted_tasks("application restarted")
            .expect("startup recovery");
        assert_eq!(
            repo.get_task(&task_id).expect("restored task").status,
            TaskStatus::Interrupted
        );
        let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
            FakeScenario::Normal,
            fake_agent_path(),
        ));
        let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
        let reopened = execute_impl(
            repo.as_ref(),
            runtime.as_ref(),
            task_runtime.as_ref(),
            serde_json::json!({ "type": "task.open", "payload": { "taskId": task_id } }),
        )
        .await;
        let data = match reopened {
            DesktopResult::Ok { data } => data,
            DesktopResult::Err { error } => panic!("open failed: {error:?}"),
        };
        let history = data["events"].as_array().expect("partial history");
        assert_eq!(data["status"], "interrupted");
        assert_eq!(
            history
                .iter()
                .filter(|event| event["payload"]["role"] == "user")
                .count(),
            1
        );
        assert!(!history.iter().any(|event| {
            event["type"] == "task.state" && event["payload"]["detail"]["completed"] == true
        }));
    }

    cleanup_temporary_database(&db_path).await;
}
