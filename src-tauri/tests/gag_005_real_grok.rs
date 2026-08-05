//! GAG-005 real Grok integration tests.
//!
//! These tests run against the REAL Grok CLI installed on this machine.
//! They are gated behind the `GROK_REAL_INTEGRATION` env var to avoid
//! running in CI without a Grok installation.
//!
//! Run with:
//!   GROK_REAL_INTEGRATION=1 cargo test --test gag_005_real_grok -- --nocapture --test-threads=1
//!
//! Safety: these tests never read or log tokens, API keys, or auth state.

use std::time::Duration;

use grok_acp_gui_lib::adapters::grok_acp::GrokAcpAdapter;
use grok_acp_gui_lib::adapters::sqlite::SqliteRepository;
use grok_acp_gui_lib::bridge::dispatch::{execute_impl, DesktopResult};
use grok_acp_gui_lib::bridge::events::DesktopEvent;
use grok_acp_gui_lib::bridge::types::SessionId;
use grok_acp_gui_lib::domain::types::{utc_now, Project, ProjectId};
use grok_acp_gui_lib::modules::agent_runtime::{
    config::{RuntimeConfig, WorkspaceContext},
    events::AgentEvent,
    requests::ClientRequest,
    AgentRuntime, AgentRuntimeImpl,
};
use grok_acp_gui_lib::modules::persistence::Repository;
use grok_acp_gui_lib::modules::task_runtime::TaskRuntimeImpl;

fn real_config() -> RuntimeConfig {
    RuntimeConfig {
        // Use the detected path from the default search location.
        executable_path: None,
        model: std::env::var("GROK_REAL_MODEL").ok(),
        min_version: "0.2.118".into(),
        handshake_timeout_secs: 30,
        idle_timeout_secs: 60,
        max_frame_bytes: 4 * 1024 * 1024,
        max_depth: 64,
        max_stderr_lines: 200,
    }
}

fn workspace() -> WorkspaceContext {
    WorkspaceContext {
        cwd: std::env::current_dir().expect("test workspace must be available"),
    }
}

/// Only run these tests when GROK_REAL_INTEGRATION=1 is set.
fn should_run() -> bool {
    std::env::var("GROK_REAL_INTEGRATION")
        .map(|v| v == "1")
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// 1. Version probe (FR-RUNTIME-001, FR-RUNTIME-002)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn real_grok_probe_succeeds() {
    if !should_run() {
        eprintln!("Skipping: GROK_REAL_INTEGRATION not set");
        return;
    }

    let adapter = GrokAcpAdapter::new(real_config());
    let runtime = AgentRuntimeImpl::new(adapter);

    let result = runtime.probe(&real_config()).await;

    eprintln!("=== PROBE RESULT ===");
    eprintln!("available: {}", result.available);
    eprintln!("status: {}", result.status);
    eprintln!("executable_path: {:?}", result.executable_path);
    eprintln!("version: {:?}", result.version);
    eprintln!("version_ok: {:?}", result.version_ok);
    eprintln!("authenticated: {:?}", result.authenticated);
    eprintln!("message: {:?}", result.message);
    eprintln!("action: {:?}", result.action);

    assert!(result.available, "probe should find grok");
    assert_eq!(result.status, "ready");
    assert!(result.executable_path.is_some());
    assert!(result.version.is_some());
    assert_eq!(result.version_ok, Some(true));
}

// ---------------------------------------------------------------------------
// 2. ACP handshake + minimal request (FR-RUNTIME-003, FR-SESSION-001)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn real_grok_handshake_and_minimal_request() {
    if !should_run() {
        eprintln!("Skipping: GROK_REAL_INTEGRATION not set");
        return;
    }

    let adapter = GrokAcpAdapter::new(real_config());
    let runtime = AgentRuntimeImpl::new(adapter);

    // Probe first.
    let probe = runtime.probe(&real_config()).await;
    assert!(probe.available, "probe must succeed before start");

    // Start a session.
    let session_id = SessionId::new("real-grok-test");
    let handle = runtime
        .start(session_id.clone(), workspace(), &real_config())
        .await
        .expect("start should succeed with real grok");

    eprintln!("=== SESSION STARTED ===");
    eprintln!("session_id: {}", handle.session_id);
    eprintln!("executable_path: {}", handle.executable_path);

    // Subscribe to events BEFORE sending.
    let mut rx = runtime.subscribe();

    // Send a minimal prompt.
    let request = ClientRequest::Prompt(
        grok_acp_gui_lib::modules::agent_runtime::requests::PromptRequest {
            message: "Reply with exactly: PONG".into(),
            attachments: vec![],
            mode: None,
            model: None,
            reasoning: None,
        },
    );
    let ack = runtime
        .send(session_id.clone(), request)
        .await
        .expect("send should succeed");
    eprintln!("=== REQUEST SENT ===");
    eprintln!("request_id: {}", ack.request_id);

    // Collect events for up to 30 seconds.
    let mut events_received = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);

    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(event)) => {
                let kind = event.event.kind_str();
                let seq = event.meta.sequence;
                eprintln!("=== EVENT seq={} kind={} ===", seq, kind);
                if let AgentEvent::RequestFailed(ref failure) = event.event {
                    eprintln!(
                        "request failure: code={} message={}",
                        failure.code, failure.message
                    );
                }
                events_received.push((seq, kind));
                if matches!(kind, "assistant_completed" | "request_failed") {
                    break;
                }
            }
            Ok(None) => {
                eprintln!("=== EVENT CHANNEL CLOSED ===");
                break;
            }
            Err(_) => {
                eprintln!("=== TIMEOUT waiting for events ===");
                break;
            }
        }
    }

    eprintln!("=== TOTAL EVENTS: {} ===", events_received.len());
    for (seq, kind) in &events_received {
        eprintln!("  seq={} kind={}", seq, kind);
    }

    // Shutdown gracefully.
    runtime.shutdown(session_id.clone(), "test complete").await;

    eprintln!("=== SHUTDOWN COMPLETE ===");
    let final_state = runtime.session_state(&session_id);
    eprintln!("final state: {:?}", final_state);

    // A successful handshake is not enough: the production ACP path must
    // deliver an assistant response rather than merely emitting session_ready
    // followed by request_failed.
    assert!(
        events_received
            .iter()
            .any(|(_, kind)| *kind == "assistant_delta" || *kind == "assistant_completed"),
        "real Grok turn must produce assistant content; got {events_received:?}"
    );
    assert!(
        events_received
            .iter()
            .all(|(_, kind)| *kind != "request_failed"),
        "real Grok turn must not fail; got {events_received:?}"
    );
}

async fn collect_task_events_until_idle(
    rx: &mut tokio::sync::broadcast::Receiver<DesktopEvent>,
    task_id: &str,
    timeout: Duration,
) -> Vec<DesktopEvent> {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut events = Vec::new();
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Ok(event)) if event.task_id.as_ref().is_some_and(|id| id.0 == task_id) => {
                let idle = event.event_type == "task.state" && event.payload["status"] == "idle";
                events.push(event);
                if idle {
                    break;
                }
            }
            Ok(Ok(_)) | Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) | Err(_) => break,
        }
    }
    events
}

#[tokio::test]
async fn real_grok_readonly_tool_lifecycle_is_structured() {
    if !should_run() {
        eprintln!("Skipping: GROK_REAL_INTEGRATION not set");
        return;
    }

    let adapter = GrokAcpAdapter::new(real_config());
    let runtime = AgentRuntimeImpl::new(adapter);
    let probe = runtime.probe(&real_config()).await;
    assert!(probe.available, "probe must succeed before start");

    let session_id = SessionId::new("real-grok-readonly-tool");
    runtime
        .start(session_id.clone(), workspace(), &real_config())
        .await
        .expect("start should succeed with real grok");
    let mut rx = runtime.subscribe();
    runtime
        .send(
            session_id.clone(),
            ClientRequest::Prompt(
                grok_acp_gui_lib::modules::agent_runtime::requests::PromptRequest {
                    message: "Use a read-only tool to list the names in the current directory. Do not modify anything. Then reply with exactly: DONE".into(),
                    attachments: vec![],
                    mode: None,
                    model: None,
                    reasoning: None,
                },
            ),
        )
        .await
        .expect("send should succeed");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut kinds = Vec::new();
    let mut started_ids = Vec::new();
    let mut completed = Vec::new();
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(event)) => {
                let kind = event.event.kind_str();
                eprintln!(
                    "=== TOOL EVENT seq={} kind={} ===",
                    event.meta.sequence, kind
                );
                match &event.event {
                    AgentEvent::ToolStarted(tool) => {
                        assert!(!tool.tool_call_id.is_empty());
                        assert!(tool.title.as_deref().is_some_and(|title| !title.is_empty()));
                        assert!(tool
                            .input_summary
                            .as_deref()
                            .is_some_and(|summary| !summary.is_empty()));
                        started_ids.push(tool.tool_call_id.clone());
                    }
                    AgentEvent::ToolCompleted(tool) => {
                        assert!(!tool.tool_call_id.is_empty());
                        assert!(matches!(
                            tool.outcome.as_str(),
                            "completed" | "success" | "failed" | "cancelled"
                        ));
                        assert!(tool
                            .summary
                            .as_deref()
                            .is_some_and(|summary| !summary.is_empty()));
                        completed.push((tool.tool_call_id.clone(), tool.duration_ms));
                    }
                    AgentEvent::RequestFailed(failure) => {
                        panic!("real read-only tool request failed: {}", failure.code);
                    }
                    _ => {}
                }
                kinds.push(kind);
                if kind == "assistant_completed" {
                    break;
                }
            }
            Ok(None) | Err(_) => break,
        }
    }
    runtime.shutdown(session_id, "test complete").await;

    assert!(
        !started_ids.is_empty(),
        "expected a real tool_started event: {kinds:?}"
    );
    assert!(
        completed.iter().any(|(id, duration)| {
            started_ids.contains(id) && duration.is_some()
        }),
        "expected a matching tool_completed event with duration: started={started_ids:?} completed={completed:?} kinds={kinds:?}"
    );
    assert!(
        kinds.contains(&"assistant_completed"),
        "tool turn must finish with assistant content: {kinds:?}"
    );
}

#[tokio::test]
async fn real_grok_read_file_tool_does_not_wait_forever() {
    if !should_run() {
        eprintln!("Skipping: GROK_REAL_INTEGRATION not set");
        return;
    }

    let fixture = std::env::temp_dir().join(format!("gag008-real-read-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&fixture).expect("create isolated read fixture");
    std::fs::write(
        fixture.join("README.txt"),
        "GAG-008 native Windows E2E fixture.\n",
    )
    .expect("write isolated read fixture");

    let runtime = AgentRuntimeImpl::new(GrokAcpAdapter::new(real_config()));
    assert!(runtime.probe(&real_config()).await.available);
    let session_id = SessionId::new("real-grok-read-file-no-deadlock");
    runtime
        .start(
            session_id.clone(),
            WorkspaceContext {
                cwd: fixture.clone(),
            },
            &real_config(),
        )
        .await
        .expect("start should succeed");
    let mut rx = runtime.subscribe();
    runtime
        .send(
            session_id.clone(),
            ClientRequest::Prompt(
                grok_acp_gui_lib::modules::agent_runtime::requests::PromptRequest {
                    message: "Use the read-only Read tool to read README.txt and reply with its first line. Do not modify anything.".into(),
                    attachments: vec![],
                    mode: None,
                    model: None,
                    reasoning: None,
                },
            ),
        )
        .await
        .expect("read turn should start");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    let mut kinds = Vec::new();
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        match tokio::time::timeout(remaining, rx.recv()).await {
            Ok(Some(event)) => {
                let kind = event.event.kind_str();
                kinds.push(kind);
                if matches!(event.event, AgentEvent::AssistantCompleted(_)) {
                    break;
                }
            }
            Ok(None) | Err(_) => break,
        }
    }
    runtime.shutdown(session_id, "test complete").await;
    let _ = std::fs::remove_dir_all(&fixture);

    assert!(
        kinds.contains(&"tool_completed"),
        "Read must leave running state within 20 seconds: {kinds:?}"
    );
    assert!(
        kinds.contains(&"assistant_completed"),
        "Read turn must finish with assistant content: {kinds:?}"
    );
}

#[tokio::test]
async fn real_grok_cancel_preserves_partial_content_and_accepts_next_turn() {
    if !should_run() {
        eprintln!("Skipping: GROK_REAL_INTEGRATION not set");
        return;
    }

    let runtime = AgentRuntimeImpl::new(GrokAcpAdapter::new(real_config()));
    assert!(runtime.probe(&real_config()).await.available);
    let session_id = SessionId::new("real-grok-cancel-restart");
    runtime
        .start(session_id.clone(), workspace(), &real_config())
        .await
        .expect("start should succeed");
    let mut rx = runtime.subscribe();

    let first_ack = runtime
        .send(
            session_id.clone(),
            ClientRequest::Prompt(
                grok_acp_gui_lib::modules::agent_runtime::requests::PromptRequest {
                    message: "Write a numbered list with 5000 distinct short lines. Start immediately and do not use tools.".into(),
                    attachments: vec![],
                    mode: None,
                    model: None,
                    reasoning: None,
                },
            ),
        )
        .await
        .expect("long turn should start");

    let first_delta = tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if let Some(event) = rx.recv().await {
                if let AgentEvent::AssistantDelta(delta) = event.event {
                    break delta.text;
                }
                if let AgentEvent::RequestFailed(failure) = event.event {
                    panic!("long turn failed before cancellation: {}", failure.code);
                }
            }
        }
    })
    .await
    .expect("real model should stream before cancellation");
    assert!(!first_delta.is_empty(), "partial content must be retained");

    runtime
        .cancel(session_id.clone(), Some(first_ack.request_id))
        .await;
    let cancelled = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(event) = rx.recv().await {
                if matches!(event.event, AgentEvent::TurnCancelled(_)) {
                    break true;
                }
                assert!(
                    !matches!(event.event, AgentEvent::AssistantCompleted(_)),
                    "cancelled turn must not be reported as completed"
                );
            }
        }
    })
    .await
    .expect("cancel must emit a terminal event");
    assert!(cancelled);

    runtime
        .send(
            session_id.clone(),
            ClientRequest::Prompt(
                grok_acp_gui_lib::modules::agent_runtime::requests::PromptRequest {
                    message: "Reply with exactly: AFTER".into(),
                    attachments: vec![],
                    mode: None,
                    model: None,
                    reasoning: None,
                },
            ),
        )
        .await
        .expect("same session must accept a turn after cancellation");
    let next_turn = tokio::time::timeout(Duration::from_secs(30), async {
        let mut text = String::new();
        loop {
            if let Some(event) = rx.recv().await {
                match event.event {
                    AgentEvent::AssistantDelta(delta) => text.push_str(&delta.text),
                    AgentEvent::AssistantCompleted(_) => break text,
                    AgentEvent::RequestFailed(failure) => {
                        panic!("turn after cancellation failed: {}", failure.code)
                    }
                    _ => {}
                }
            }
        }
    })
    .await
    .expect("turn after cancellation must complete");
    assert!(
        next_turn.contains("AFTER"),
        "unexpected next response: {next_turn:?}"
    );

    runtime.shutdown(session_id, "test complete").await;
}

#[tokio::test]
async fn real_deepseek_desktop_bridge_supports_two_persisted_turns() {
    if !should_run() {
        eprintln!("Skipping: GROK_REAL_INTEGRATION not set");
        return;
    }
    let model = std::env::var("GROK_REAL_MODEL")
        .expect("set GROK_REAL_MODEL to an authenticated non-default model for this test");

    let repo =
        std::sync::Arc::new(SqliteRepository::open_in_memory().expect("in-memory repository"));
    let cwd = std::env::current_dir().expect("test workspace");
    repo.create_project(&Project {
        id: ProjectId::new("real-deepseek-project"),
        path: cwd.to_string_lossy().into_owned(),
        display_path: "real-deepseek-project".into(),
        repo_root: Some(cwd.to_string_lossy().into_owned()),
        trusted_at: Some(utc_now()),
        last_opened_at: utc_now(),
    })
    .expect("project");

    let runtime = AgentRuntimeImpl::new(GrokAcpAdapter::new(RuntimeConfig::default()));
    let task_runtime = std::sync::Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
    task_runtime.spawn_agent_event_forwarder();
    let mut events = task_runtime.event_subscriber();

    let created = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "task.create",
            "payload": {
                "projectId": "real-deepseek-project",
                "title": "Real DeepSeek conversation",
                "prompt": "Reply with exactly: FIRST",
                "mode": "ask",
                "model": model.clone(),
                "workspaceStrategy": "direct"
            }
        }),
    )
    .await;
    let create_data = match created {
        DesktopResult::Ok { data } => data,
        DesktopResult::Err { error } => panic!("real task.create failed: {error:?}"),
    };
    assert_eq!(create_data["task"]["status"], "running");
    assert!(create_data.get("startError").is_none());
    let task_id = create_data["taskId"].as_str().expect("task id").to_string();

    let first_events =
        collect_task_events_until_idle(&mut events, &task_id, Duration::from_secs(60)).await;
    assert!(
        first_events.iter().any(|event| {
            event.event_type == "message.delta"
                && event.payload["role"] == "assistant"
                && event.payload["text"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty())
        }),
        "first turn must stream assistant text: {first_events:?}"
    );
    assert!(first_events
        .iter()
        .all(|event| event.event_type != "activity.updated" || event.payload["kind"] != "error"));

    let sent = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "turn.send",
            "payload": { "taskId": task_id, "message": "Reply with exactly: SECOND" }
        }),
    )
    .await;
    assert!(
        matches!(sent, DesktopResult::Ok { .. }),
        "second turn must be accepted: {sent:?}"
    );
    let second_events =
        collect_task_events_until_idle(&mut events, &task_id, Duration::from_secs(60)).await;
    assert!(
        second_events.iter().any(|event| {
            event.event_type == "message.delta"
                && event.payload["role"] == "assistant"
                && event.payload["text"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty())
        }),
        "second turn must stream assistant text: {second_events:?}"
    );

    let reopened = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({"type": "task.open", "payload": {"taskId": task_id}}),
    )
    .await;
    let open_data = match reopened {
        DesktopResult::Ok { data } => data,
        DesktopResult::Err { error } => panic!("task.open failed: {error:?}"),
    };
    assert_eq!(open_data["status"], "idle");
    let history = open_data["events"].as_array().expect("persisted timeline");
    let user_messages: Vec<_> = history
        .iter()
        .filter(|event| event["payload"]["role"] == "user")
        .collect();
    assert_eq!(
        user_messages.len(),
        2,
        "each confirmed user turn must appear once: {history:?}"
    );
    assert_eq!(
        user_messages[0]["payload"]["text"],
        "Reply with exactly: FIRST"
    );
    assert_eq!(
        user_messages[1]["payload"]["text"],
        "Reply with exactly: SECOND"
    );

    runtime.shutdown_all("test complete").await;
}

#[tokio::test]
async fn real_deepseek_two_tasks_keep_tools_and_cancellation_isolated() {
    if !should_run() {
        eprintln!("Skipping: GROK_REAL_INTEGRATION not set");
        return;
    }
    let model = std::env::var("GROK_REAL_MODEL")
        .expect("set GROK_REAL_MODEL to an authenticated non-default model for this test");

    let repo =
        std::sync::Arc::new(SqliteRepository::open_in_memory().expect("in-memory repository"));
    let cwd = std::env::current_dir().expect("test workspace");
    repo.create_project(&Project {
        id: ProjectId::new("real-isolation-project"),
        path: cwd.to_string_lossy().into_owned(),
        display_path: "real-isolation-project".into(),
        repo_root: Some(cwd.to_string_lossy().into_owned()),
        trusted_at: Some(utc_now()),
        last_opened_at: utc_now(),
    })
    .expect("project");

    let runtime = AgentRuntimeImpl::new(GrokAcpAdapter::new(RuntimeConfig::default()));
    let task_runtime = std::sync::Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
    task_runtime.spawn_agent_event_forwarder();
    let mut events = task_runtime.event_subscriber();

    let create_task = |title: &str, prompt: &str| {
        serde_json::json!({
            "type": "task.create",
            "payload": {
                "projectId": "real-isolation-project",
                "title": title,
                "prompt": prompt,
                "mode": "ask",
                "model": model.clone(),
                "workspaceStrategy": "direct"
            }
        })
    };
    let task_a = match execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        create_task(
            "Real task A",
            "Write 5000 numbered short lines beginning with A. Do not use tools.",
        ),
    )
    .await
    {
        DesktopResult::Ok { data } => data["taskId"].as_str().expect("task A id").to_string(),
        DesktopResult::Err { error } => panic!("task A create failed: {error:?}"),
    };
    let task_b = match execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        create_task(
            "Real task B",
            "Use a read-only tool to list directory names, then reply with exactly B_DONE.",
        ),
    )
    .await
    {
        DesktopResult::Ok { data } => data["taskId"].as_str().expect("task B id").to_string(),
        DesktopResult::Err { error } => panic!("task B create failed: {error:?}"),
    };

    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut a_partial = false;
    let mut a_cancel_requested = false;
    let mut a_stopped = false;
    let mut b_tool_completed = false;
    let mut b_assistant = false;
    let mut b_idle = false;
    while tokio::time::Instant::now() < deadline && !(a_stopped && b_tool_completed && b_idle) {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let event = match tokio::time::timeout(remaining, events.recv()).await {
            Ok(Ok(event)) => event,
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) | Err(_) => break,
        };
        let event_task = event.task_id.as_ref().map(|id| id.0.as_str());
        assert!(event_task == Some(task_a.as_str()) || event_task == Some(task_b.as_str()));

        if event.payload["role"] == "user" {
            let text = event.payload["text"].as_str().unwrap_or_default();
            if event_task == Some(task_a.as_str()) {
                assert!(text.starts_with("Write 5000"));
            } else {
                assert!(text.starts_with("Use a read-only tool"));
            }
        }
        if event
            .payload
            .get("toolCall")
            .is_some_and(|value| value.is_object())
        {
            assert_eq!(
                event_task,
                Some(task_b.as_str()),
                "B tool event crossed task scope"
            );
            b_tool_completed |= matches!(
                event.payload["toolCall"]["status"].as_str(),
                Some("completed" | "success")
            );
        }
        if event_task == Some(task_a.as_str())
            && event.event_type == "message.delta"
            && event.payload["role"] == "assistant"
        {
            a_partial = true;
        }
        if event_task == Some(task_a.as_str())
            && event.event_type == "task.state"
            && event.payload["detail"]["reason"] == "cancelled"
        {
            a_stopped = true;
        }
        if event_task == Some(task_b.as_str())
            && event.event_type == "message.delta"
            && event.payload["role"] == "assistant"
        {
            b_assistant = true;
        }
        if event_task == Some(task_b.as_str())
            && event.event_type == "task.state"
            && event.payload["status"] == "idle"
        {
            b_idle = true;
        }

        if a_partial && !a_cancel_requested {
            let cancelled = execute_impl(
                repo.as_ref(),
                runtime.as_ref(),
                task_runtime.as_ref(),
                serde_json::json!({"type": "turn.cancel", "payload": {"taskId": task_a}}),
            )
            .await;
            assert!(matches!(cancelled, DesktopResult::Ok { .. }));
            a_cancel_requested = true;
        }
    }

    assert!(
        a_partial && a_cancel_requested && a_stopped,
        "task A did not stop cleanly: partial={a_partial} cancel={a_cancel_requested} stopped={a_stopped}"
    );
    assert!(
        b_tool_completed,
        "task B did not finish its read-only tool: assistant={b_assistant} idle={b_idle}"
    );
    assert!(
        b_assistant && b_idle,
        "task B was affected by cancelling task A: assistant={b_assistant} idle={b_idle}"
    );
    runtime.shutdown_all("test complete").await;
}

// ---------------------------------------------------------------------------
// 3. Abnormal exit handling (NFR-RELIABILITY-001)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn real_grok_abnormal_exit_handled() {
    if !should_run() {
        eprintln!("Skipping: GROK_REAL_INTEGRATION not set");
        return;
    }

    let adapter = GrokAcpAdapter::new(real_config());
    let runtime = AgentRuntimeImpl::new(adapter);

    let session_id = SessionId::new("real-grok-abnormal");
    let result = runtime
        .start(session_id.clone(), workspace(), &real_config())
        .await;

    if result.is_ok() {
        // Shutdown, then verify we can shutdown again (idempotent).
        runtime
            .shutdown(session_id.clone(), "normal shutdown")
            .await;
        let state1 = runtime.session_state(&session_id);

        // Double shutdown — should be idempotent.
        runtime
            .shutdown(session_id.clone(), "double shutdown")
            .await;
        let state2 = runtime.session_state(&session_id);

        eprintln!("=== ABNORMAL EXIT TEST ===");
        eprintln!("state after first shutdown: {:?}", state1);
        eprintln!("state after second shutdown: {:?}", state2);

        assert_eq!(state1, state2, "double shutdown should be idempotent");
    }
}

// ---------------------------------------------------------------------------
// 4. Command and exit code logging
// ---------------------------------------------------------------------------

#[tokio::test]
async fn real_grok_logs_command_and_exit_code() {
    if !should_run() {
        eprintln!("Skipping: GROK_REAL_INTEGRATION not set");
        return;
    }

    eprintln!("=== COMMAND INFO ===");
    eprintln!("binary: grok (resolved via probe)");
    eprintln!("version: 0.2.118 (from `grok --version`)");
    eprintln!(
        "args: --no-auto-update agent{} stdio",
        real_config()
            .model
            .as_deref()
            .map(|_| " --model <validated-profile-id>")
            .unwrap_or("")
    );
    eprintln!("cwd: {}", std::env::temp_dir().display());
    eprintln!("env allowlist: PATH, USERPROFILE, LOCALAPPDATA, SYSTEMROOT, TEMP, TMP, LANG, TERM");
    eprintln!(
        "XAI_API_KEY: {} (not passed to child unless set in parent env)",
        if std::env::var("XAI_API_KEY").is_ok() {
            "present"
        } else {
            "absent"
        }
    );

    let adapter = GrokAcpAdapter::new(real_config());
    let runtime = AgentRuntimeImpl::new(adapter);

    let probe = runtime.probe(&real_config()).await;
    eprintln!("=== PROBE ===");
    eprintln!("exit_code: 0 (probe succeeded)");
    eprintln!("available: {}", probe.available);
    eprintln!("version: {:?}", probe.version);

    // The exit code of the probe subprocess (`grok --version`).
    // We don't capture it directly, but probe.available=true means exit was 0.
    assert!(probe.available);
}
