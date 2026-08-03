//! GAG-005 integration tests: AgentRuntime + Fake ACP agent.
//!
//! These tests spawn the Node.js fake-acp-agent and verify the full
//! runtime lifecycle: probe → start → handshake → send → events → shutdown.

use std::path::PathBuf;
use std::time::Duration;

use grok_acp_gui_lib::adapters::grok_acp::{FakeAcpTransport, FakeScenario};
use grok_acp_gui_lib::bridge::types::SessionId;
use grok_acp_gui_lib::modules::agent_runtime::{
    config::{RuntimeConfig, WorkspaceContext},
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
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), rx.recv()).await {
            Ok(Some(event)) => {
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
    let mut got_permission = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

    while tokio::time::Instant::now() < deadline {
        if let Ok(Some(event)) = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await {
            if let AgentEvent::PermissionRequested(p) = event.event {
                got_permission = true;
                assert!(!p.request_id.is_empty());
                assert!(!p.options.is_empty());
                // Option IDs must be preserved verbatim.
                assert!(p.options.iter().all(|o| o.option_id.starts_with("opt-")));
                break;
            }
        }
    }

    assert!(got_permission, "should have received a permission request");

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

    // The runtime should NOT crash — it should continue processing.
    // Wait a bit and verify the session is still alive.
    tokio::time::sleep(Duration::from_secs(1)).await;

    let state = runtime.session_state(&session_id);
    assert_eq!(
        state,
        Some(grok_acp_gui_lib::modules::agent_runtime::RuntimeState::Busy),
        "session should still be busy (unknown method doesn't crash)"
    );

    // Drain events.
    let _ = rx.try_recv();

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
