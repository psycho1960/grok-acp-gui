//! GAG-005 integration tests: AgentRuntime + Fake ACP agent.
//!
//! These tests spawn the Node.js fake-acp-agent and verify the full
//! runtime lifecycle: probe → start → handshake → send → events → shutdown.

use std::path::PathBuf;
use std::time::Duration;

use grok_acp_gui_lib::adapters::grok_acp::{FakeAcpTransport, FakeScenario};
use grok_acp_gui_lib::bridge::types::SessionId;
use grok_acp_gui_lib::modules::agent_runtime::{
    config::{RuntimeConfig, RuntimeLoginMethod, WorkspaceContext},
    events::AgentEvent,
    requests::ClientRequest,
    AgentRuntime, AgentRuntimeImpl,
};

/// Locate the fake-acp-agent script relative to the crate root.
fn fake_agent_path() -> PathBuf {
    // CARGO_MANIFEST_DIR is src-tauri/, so the repo root is the parent.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .parent()
        .unwrap()
        .join("tests")
        .join("fake-acp-agent")
        .join("agent.mjs")
}

/// Helper: create a runtime with the fake transport.
fn make_runtime(scenario: FakeScenario) -> std::sync::Arc<AgentRuntimeImpl<FakeAcpTransport>> {
    let transport = FakeAcpTransport::new(scenario, fake_agent_path());
    AgentRuntimeImpl::new(transport)
}

fn default_config() -> RuntimeConfig {
    RuntimeConfig {
        handshake_timeout_secs: 3,
        ..RuntimeConfig::default()
    }
}

fn workspace() -> WorkspaceContext {
    WorkspaceContext {
        cwd: std::env::temp_dir(),
    }
}

// ---------------------------------------------------------------------------
// Happy-path: normal scenario
// ---------------------------------------------------------------------------

#[tokio::test]
async fn fake_acp_normal_lifecycle() {
    let runtime = make_runtime(FakeScenario::Normal);
    let session_id = SessionId::new("test-normal");
    let config = default_config();

    // Start the session (probe + spawn + handshake).
    let handle = runtime
        .start(session_id.clone(), workspace(), &config)
        .await
        .expect("start should succeed");

    assert_eq!(handle.session_id, session_id);
    // executable_path may be empty for the fake transport — that's OK.

    // Subscribe to events BEFORE sending, so we don't miss any.
    let mut rx = runtime.subscribe();

    // Send a prompt.
    let request = ClientRequest::Prompt(
        grok_acp_gui_lib::modules::agent_runtime::requests::PromptRequest {
            message: "Hello".into(),
            attachments: vec![],
            mode: Some("code".into()),
            model: None,
            reasoning: None,
        },
    );
    let ack = runtime
        .send(session_id.clone(), request)
        .await
        .expect("send should succeed");
    assert!(ack.request_id > 0);

    // Wait for events — we should see assistant deltas and completion.
    let mut got_delta = false;
    let mut got_completed = false;
    let mut got_tool = false;
    let mut sequences = Vec::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), rx.recv()).await {
            Ok(Some(event)) => {
                sequences.push(event.meta.sequence);
                match event.event {
                    AgentEvent::AssistantDelta(_) => got_delta = true,
                    AgentEvent::AssistantCompleted(_) => got_completed = true,
                    AgentEvent::ToolStarted(_) | AgentEvent::ToolCompleted(_) => got_tool = true,
                    _ => {}
                }
                if got_delta && got_completed && got_tool {
                    break;
                }
            }
            _ => break,
        }
    }

    assert!(got_delta, "should have received assistant delta");
    assert!(got_completed, "should have received assistant completed");
    assert!(got_tool, "should have received tool lifecycle events");
    assert!(
        sequences.windows(2).all(|pair| pair[0] < pair[1]),
        "event sequences must be strictly increasing: {sequences:?}"
    );
    assert_eq!(
        runtime.session_state(&session_id),
        Some(grok_acp_gui_lib::modules::agent_runtime::RuntimeState::Ready),
        "completed turn must return the runtime to ready"
    );

    // Shutdown.
    runtime.shutdown(session_id.clone(), "test complete").await;

    let state = runtime.session_state(&session_id);
    assert_eq!(
        state,
        Some(grok_acp_gui_lib::modules::agent_runtime::RuntimeState::Stopped)
    );
}

// ---------------------------------------------------------------------------
// Idempotent shutdown
// ---------------------------------------------------------------------------

#[tokio::test]
async fn shutdown_is_idempotent() {
    let runtime = make_runtime(FakeScenario::Normal);
    let session_id = SessionId::new("test-idempotent");
    let config = default_config();

    runtime
        .start(session_id.clone(), workspace(), &config)
        .await
        .expect("start should succeed");

    // First shutdown.
    runtime.shutdown(session_id.clone(), "first").await;
    let state1 = runtime.session_state(&session_id);

    // Second shutdown — should be a no-op, not an error.
    runtime.shutdown(session_id.clone(), "second").await;
    let state2 = runtime.session_state(&session_id);

    assert_eq!(state1, state2);
    assert_eq!(
        state2,
        Some(grok_acp_gui_lib::modules::agent_runtime::RuntimeState::Stopped)
    );
}

#[tokio::test]
async fn shutdown_all_cleans_every_managed_session() {
    let runtime = make_runtime(FakeScenario::Slow);
    let config = default_config();
    let session_a = SessionId::new("test-shutdown-all-a");
    let session_b = SessionId::new("test-shutdown-all-b");
    runtime
        .start(session_a.clone(), workspace(), &config)
        .await
        .expect("A should start");
    runtime
        .start(session_b.clone(), workspace(), &config)
        .await
        .expect("B should start");

    runtime.shutdown_all("application exit").await;

    assert_eq!(
        runtime.session_state(&session_a),
        Some(grok_acp_gui_lib::modules::agent_runtime::RuntimeState::Stopped)
    );
    assert_eq!(
        runtime.session_state(&session_b),
        Some(grok_acp_gui_lib::modules::agent_runtime::RuntimeState::Stopped)
    );
}

// ---------------------------------------------------------------------------
// Cancel when not busy is a no-op
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cancel_when_not_busy_is_noop() {
    let runtime = make_runtime(FakeScenario::Normal);
    let session_id = SessionId::new("test-cancel-noop");
    let config = default_config();

    runtime
        .start(session_id.clone(), workspace(), &config)
        .await
        .expect("start should succeed");

    // Cancel when ready (not busy) — should be a no-op.
    runtime.cancel(session_id.clone(), None).await;

    let state = runtime.session_state(&session_id);
    assert_eq!(
        state,
        Some(grok_acp_gui_lib::modules::agent_runtime::RuntimeState::Ready)
    );

    runtime.shutdown(session_id.clone(), "done").await;
}

#[tokio::test]
async fn cancel_busy_turn_emits_terminal_event_and_suppresses_late_updates() {
    let runtime = make_runtime(FakeScenario::Normal);
    let session_id = SessionId::new("test-cancel-busy");
    runtime
        .start(session_id.clone(), workspace(), &default_config())
        .await
        .expect("start");
    let mut events = runtime.subscribe();
    runtime
        .send(
            session_id.clone(),
            ClientRequest::Prompt(
                grok_acp_gui_lib::modules::agent_runtime::requests::PromptRequest {
                    message: "long turn".into(),
                    attachments: vec![],
                    mode: None,
                    model: None,
                    reasoning: None,
                },
            ),
        )
        .await
        .expect("send");

    tokio::time::sleep(Duration::from_millis(25)).await;
    runtime.cancel(session_id.clone(), None).await;

    let deadline = tokio::time::Instant::now() + Duration::from_millis(250);
    let mut saw_cancelled = false;
    let mut late_turn_update = false;
    while tokio::time::Instant::now() < deadline {
        if let Ok(Some(event)) =
            tokio::time::timeout(Duration::from_millis(30), events.recv()).await
        {
            if saw_cancelled
                && matches!(
                    event.event,
                    AgentEvent::AssistantDelta(_)
                        | AgentEvent::AssistantCompleted(_)
                        | AgentEvent::ToolStarted(_)
                        | AgentEvent::ToolUpdated(_)
                        | AgentEvent::ToolCompleted(_)
                )
            {
                late_turn_update = true;
            }
            if matches!(event.event, AgentEvent::TurnCancelled(_)) {
                saw_cancelled = true;
            }
        }
    }

    assert!(saw_cancelled, "busy cancel must be visible to upper layers");
    assert!(
        !late_turn_update,
        "cancelled turn emitted updates after its terminal event"
    );
    assert_eq!(
        runtime.session_state(&session_id),
        Some(grok_acp_gui_lib::modules::agent_runtime::RuntimeState::Ready)
    );
    runtime.shutdown(session_id, "done").await;
}

// ---------------------------------------------------------------------------
// Crash scenario: process exits during handshake
// ---------------------------------------------------------------------------

#[tokio::test]
async fn crash_scenario_returns_handshake_error() {
    let runtime = make_runtime(FakeScenario::Crash);
    let session_id = SessionId::new("test-crash");
    let config = default_config();

    let result = runtime
        .start(session_id.clone(), workspace(), &config)
        .await;

    // The start should fail because the process crashes during handshake.
    assert!(result.is_err(), "crash scenario should fail to start");

    let err = result.unwrap_err();
    assert!(
        err.code.contains("ACP_HANDSHAKE_FAILED") || err.code.contains("RUNTIME"),
        "error code should be handshake or runtime related, got: {}",
        err.code
    );
}

#[tokio::test]
async fn crash_during_turn_is_structured_and_same_session_can_restart() {
    let runtime = make_runtime(FakeScenario::CrashAfterPrompt);
    let session_id = SessionId::new("test-crash-turn");
    runtime
        .start(session_id.clone(), workspace(), &default_config())
        .await
        .expect("start");
    let mut events = runtime.subscribe();
    runtime
        .send(
            session_id.clone(),
            ClientRequest::Prompt(
                grok_acp_gui_lib::modules::agent_runtime::requests::PromptRequest {
                    message: "crash now".into(),
                    attachments: vec![],
                    mode: None,
                    model: None,
                    reasoning: None,
                },
            ),
        )
        .await
        .expect("send");

    let mut process_exit = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Some(event)) =
            tokio::time::timeout(Duration::from_millis(250), events.recv()).await
        {
            if let AgentEvent::ProcessExited(payload) = event.event {
                process_exit = Some(payload);
                break;
            }
        }
    }
    let process_exit = process_exit.expect("normalized process-exit event");
    assert!(!process_exit.reason.is_empty());
    assert_eq!(
        runtime.session_state(&session_id),
        Some(grok_acp_gui_lib::modules::agent_runtime::RuntimeState::Stopped)
    );

    runtime
        .start(session_id.clone(), workspace(), &default_config())
        .await
        .expect("stopped session must be restartable");
    assert_eq!(
        runtime.session_state(&session_id),
        Some(grok_acp_gui_lib::modules::agent_runtime::RuntimeState::Ready)
    );
    runtime.shutdown(session_id, "done").await;
}

// ---------------------------------------------------------------------------
// Timeout scenario: handshake never completes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn timeout_scenario_fails_handshake() {
    let runtime = make_runtime(FakeScenario::Timeout);
    let session_id = SessionId::new("test-timeout");
    let config = default_config();

    let result = runtime
        .start(session_id.clone(), workspace(), &config)
        .await;

    assert!(
        result.is_err(),
        "timeout scenario should fail to start (handshake timeout)"
    );
}

// ---------------------------------------------------------------------------
// Permission scenario: agent sends requestPermission
// ---------------------------------------------------------------------------

#[tokio::test]
async fn permission_scenario_emits_permission_event() {
    let runtime = make_runtime(FakeScenario::Permission);
    let session_id = SessionId::new("test-permission");
    let config = default_config();

    runtime
        .start(session_id.clone(), workspace(), &config)
        .await
        .expect("start should succeed");

    let mut rx = runtime.subscribe();

    // Send a prompt to trigger the permission request.
    let request = ClientRequest::Prompt(
        grok_acp_gui_lib::modules::agent_runtime::requests::PromptRequest {
            message: "Do something".into(),
            attachments: vec![],
            mode: None,
            model: None,
            reasoning: None,
        },
    );
    let _ = runtime.send(session_id.clone(), request).await;

    // Wait for a permission event.
    let mut pending_permission = None;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

    while tokio::time::Instant::now() < deadline {
        if let Ok(Some(event)) = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await {
            if let AgentEvent::PermissionRequested(p) = event.event {
                assert!(!p.request_id.is_empty());
                assert!(!p.options.is_empty());
                // Option IDs must be preserved verbatim.
                assert!(p.options.iter().all(|o| o.option_id.starts_with("opt-")));
                pending_permission = Some((p.request_id, p.options[0].option_id.clone()));
                break;
            }
        }
    }

    let (request_id, option_id) = pending_permission.expect("permission request");
    let resolved = runtime
        .send(
            session_id.clone(),
            ClientRequest::ResolvePermission(
                grok_acp_gui_lib::modules::agent_runtime::requests::ResolvePermissionRequest {
                    request_id,
                    option_id,
                },
            ),
        )
        .await;
    assert!(
        resolved.is_ok(),
        "resolution must be accepted while Turn is busy"
    );
    assert_eq!(
        runtime.session_state(&session_id),
        Some(grok_acp_gui_lib::modules::agent_runtime::RuntimeState::Busy),
        "resolution must not start or complete a second Turn"
    );

    runtime.shutdown(session_id.clone(), "done").await;
}

// ---------------------------------------------------------------------------
// Unknown method does not crash the runtime
// ---------------------------------------------------------------------------

#[tokio::test]
async fn unknown_method_does_not_crash() {
    let runtime = make_runtime(FakeScenario::UnknownMethod);
    let session_id = SessionId::new("test-unknown");
    let config = default_config();

    runtime
        .start(session_id.clone(), workspace(), &config)
        .await
        .expect("start should succeed");

    let mut rx = runtime.subscribe();

    let request = ClientRequest::Prompt(
        grok_acp_gui_lib::modules::agent_runtime::requests::PromptRequest {
            message: "Hello".into(),
            attachments: vec![],
            mode: None,
            model: None,
            reasoning: None,
        },
    );
    let _ = runtime.send(session_id.clone(), request).await;

    // The runtime should NOT crash — it should ignore the extension,
    // continue streaming, and complete the turn normally.
    let completed = tokio::time::timeout(Duration::from_secs(2), async {
        while let Some(event) = rx.recv().await {
            if matches!(event.event, AgentEvent::AssistantCompleted(_)) {
                return true;
            }
        }
        false
    })
    .await
    .unwrap_or(false);
    assert!(
        completed,
        "known events after an unknown method must continue"
    );

    let state = runtime.session_state(&session_id);
    assert_eq!(
        state,
        Some(grok_acp_gui_lib::modules::agent_runtime::RuntimeState::Ready),
        "a completed turn must return to Ready even after an unknown method"
    );

    runtime.shutdown(session_id.clone(), "done").await;
}

// ---------------------------------------------------------------------------
// Stderr flood does not cause OOM or crash
// ---------------------------------------------------------------------------

#[tokio::test]
async fn stderr_flood_does_not_crash() {
    let runtime = make_runtime(FakeScenario::StderrFlood);
    let session_id = SessionId::new("test-stderr-flood");
    let config = default_config();

    runtime
        .start(session_id.clone(), workspace(), &config)
        .await
        .expect("start should succeed");

    // Wait for the flood.
    tokio::time::sleep(Duration::from_secs(2)).await;

    // The runtime should still be alive.
    let state = runtime.session_state(&session_id);
    assert!(
        state.is_some(),
        "session should still exist after stderr flood"
    );

    runtime.shutdown(session_id.clone(), "done").await;
}

#[tokio::test]
async fn login_success_is_reported_without_process_output() {
    let runtime = make_runtime(FakeScenario::Normal);
    let started = runtime
        .login(&default_config(), RuntimeLoginMethod::Oauth)
        .await;
    assert_eq!(started.status, "running");

    tokio::time::sleep(Duration::from_millis(30)).await;
    let completed = runtime.login_status().await;
    assert_eq!(completed.status, "succeeded");
    assert_eq!(completed.exit_code, Some(0));
    assert!(!serde_json::to_string(&completed)
        .unwrap()
        .to_ascii_lowercase()
        .contains("token"));
}

#[tokio::test]
async fn login_cancel_is_structured_and_idempotent() {
    let runtime = make_runtime(FakeScenario::Timeout);
    let mut config = default_config();
    config.login_timeout_secs = 5;
    runtime.login(&config, RuntimeLoginMethod::Oauth).await;
    assert_eq!(runtime.cancel_login().await.status, "running");
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(runtime.login_status().await.status, "cancelled");
    assert_eq!(runtime.cancel_login().await.status, "cancelled");
}

#[tokio::test]
async fn login_abnormal_exit_and_timeout_are_distinct() {
    let failed_runtime = make_runtime(FakeScenario::Crash);
    failed_runtime
        .login(&default_config(), RuntimeLoginMethod::DeviceAuth)
        .await;
    tokio::time::sleep(Duration::from_millis(30)).await;
    let failed = failed_runtime.login_status().await;
    assert_eq!(failed.status, "failed");
    assert_eq!(failed.exit_code, Some(7));

    let timed_out_runtime = make_runtime(FakeScenario::Timeout);
    let mut config = default_config();
    config.login_timeout_secs = 1;
    timed_out_runtime
        .login(&config, RuntimeLoginMethod::Oauth)
        .await;
    tokio::time::sleep(Duration::from_millis(1_100)).await;
    assert_eq!(timed_out_runtime.login_status().await.status, "timed_out");
}

#[tokio::test]
async fn shutdown_all_cancels_an_active_login_process() {
    let runtime = make_runtime(FakeScenario::Timeout);
    let mut config = default_config();
    config.login_timeout_secs = 30;
    runtime.login(&config, RuntimeLoginMethod::Oauth).await;

    runtime.shutdown_all("application exit").await;
    assert_eq!(runtime.login_status().await.status, "cancelled");
}

#[tokio::test]
async fn acp_starts_in_a_unicode_workspace_with_spaces() {
    let workspace =
        std::env::temp_dir().join(format!("GAG 005A 中文 workspace {}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspace).expect("create unicode workspace");
    let runtime = make_runtime(FakeScenario::Normal);
    let session_id = SessionId::new("unicode-space-workspace");

    let started = runtime
        .start(
            session_id.clone(),
            WorkspaceContext {
                cwd: workspace.clone(),
            },
            &default_config(),
        )
        .await;
    assert!(
        started.is_ok(),
        "unicode/space cwd must be passed as one path: {started:?}"
    );
    runtime.shutdown(session_id, "test complete").await;
    std::fs::remove_dir_all(workspace).expect("remove unicode workspace");
}
