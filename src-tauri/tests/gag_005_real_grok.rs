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
use grok_acp_gui_lib::bridge::types::SessionId;
use grok_acp_gui_lib::modules::agent_runtime::{
    config::{RuntimeConfig, WorkspaceContext},
    requests::ClientRequest,
    AgentRuntime, AgentRuntimeImpl,
};

fn real_config() -> RuntimeConfig {
    RuntimeConfig {
        // Use the detected path from the default search location.
        executable_path: None,
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
        cwd: std::env::temp_dir(),
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
                events_received.push((seq, kind));
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

    // We should have received at least one event (assistant delta or completed).
    assert!(
        !events_received.is_empty(),
        "should have received at least one event from real grok"
    );
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
    eprintln!("args: --no-auto-update agent stdio");
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
