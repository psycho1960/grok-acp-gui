//! P0 regression tests: ensure AgentRuntime::probe() is NOT a dead stub.
//!
//! These tests verify that:
//! 1. probe() delegates to the transport's real probe logic
//! 2. start() calls probe() internally if not already probed
//! 3. The probe result is propagated correctly to RuntimeProbeResult
//!
//! They use the FakeAcpTransport (which always succeeds at probe)
//! to verify the wiring without requiring a real grok binary.

use std::path::PathBuf;

use grok_acp_gui_lib::adapters::grok_acp::{FakeAcpTransport, FakeScenario};
use grok_acp_gui_lib::bridge::types::SessionId;
use grok_acp_gui_lib::modules::agent_runtime::{
    config::{RuntimeConfig, WorkspaceContext},
    AgentRuntime, AgentRuntimeImpl,
};

fn fake_agent_path() -> PathBuf {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest_dir)
        .parent()
        .unwrap()
        .join("tests")
        .join("fake-acp-agent")
        .join("agent.mjs")
}

fn make_runtime() -> std::sync::Arc<AgentRuntimeImpl<FakeAcpTransport>> {
    let transport = FakeAcpTransport::new(FakeScenario::Normal, fake_agent_path());
    AgentRuntimeImpl::new(transport)
}

// ---------------------------------------------------------------------------
// P0-1: probe() must NOT be a dead stub
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p0_probe_is_not_dead_stub() {
    let runtime = make_runtime();
    let result = runtime.probe(&RuntimeConfig::default()).await;

    // The fake transport always succeeds at probe — if probe() is
    // properly wired, the result should be available=true.
    assert!(
        result.available,
        "P0 REGRESSION: probe() returned unavailable — dead stub detected. status={}, message={:?}",
        result.status, result.message
    );
    assert_eq!(result.status, "ready");
    assert!(
        result.executable_path.is_some(),
        "probe should set executable_path"
    );
    assert!(result.version.is_some(), "probe should set version");
    assert!(
        result.version_ok == Some(true),
        "probe should set version_ok=true"
    );
}

// ---------------------------------------------------------------------------
// P0-2: start() must call probe() internally if not already probed
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p0_start_calls_probe_internally() {
    let runtime = make_runtime();
    let session_id = SessionId::new("p0-start-probe");

    // Call start() WITHOUT calling probe() first.
    // start() should internally call probe() and succeed.
    let result = runtime
        .start(
            session_id.clone(),
            WorkspaceContext {
                cwd: std::env::temp_dir(),
            },
            &RuntimeConfig::default(),
        )
        .await;

    assert!(
        result.is_ok(),
        "P0 REGRESSION: start() failed because probe was not called internally: {:?}",
        result.err()
    );

    let handle = result.unwrap();
    // After a successful start, the executable_path should be populated.
    assert!(
        !handle.executable_path.is_empty(),
        "P0 REGRESSION: handle.executable_path is empty — probe was not called before spawn"
    );

    // Clean up.
    runtime.shutdown(session_id, "test done").await;
}

// ---------------------------------------------------------------------------
// P0-3: probe() result is cached — calling it twice doesn't re-probe
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p0_probe_result_is_cached() {
    let runtime = make_runtime();

    let result1 = runtime.probe(&RuntimeConfig::default()).await;
    let result2 = runtime.probe(&RuntimeConfig::default()).await;

    assert!(result1.available, "first probe should succeed");
    assert!(result2.available, "second probe should succeed");
    // Both should return the same path.
    assert_eq!(
        result1.executable_path, result2.executable_path,
        "cached probe should return the same path"
    );
}

// ---------------------------------------------------------------------------
// P0-4: probe() before start() makes start() succeed with correct path
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p0_probe_before_start_succeeds() {
    let runtime = make_runtime();
    let session_id = SessionId::new("p0-probe-then-start");

    // Probe first.
    let probe_result = runtime.probe(&RuntimeConfig::default()).await;
    assert!(probe_result.available);

    // Then start — should succeed because probe already set resolved_path.
    let result = runtime
        .start(
            session_id.clone(),
            WorkspaceContext {
                cwd: std::env::temp_dir(),
            },
            &RuntimeConfig::default(),
        )
        .await;

    assert!(result.is_ok(), "start after probe should succeed");

    let handle = result.unwrap();
    // The handle's executable_path should match the probe result.
    assert_eq!(
        handle.executable_path,
        probe_result.executable_path.unwrap().display().to_string(),
        "handle.executable_path should match probe result"
    );

    runtime.shutdown(session_id, "test done").await;
}

#[tokio::test]
async fn probe_distinguishes_missing_and_too_old_grok() {
    let missing = AgentRuntimeImpl::new(FakeAcpTransport::new(
        FakeScenario::NotFound,
        fake_agent_path(),
    ))
    .probe(&RuntimeConfig::default())
    .await;
    assert!(!missing.available);
    assert_eq!(missing.status, "not_found");
    assert_eq!(missing.version, None);

    let too_old = AgentRuntimeImpl::new(FakeAcpTransport::new(
        FakeScenario::VersionTooLow,
        fake_agent_path(),
    ))
    .probe(&RuntimeConfig::default())
    .await;
    assert!(!too_old.available);
    assert_eq!(too_old.status, "version_too_low");
    assert_eq!(too_old.version.as_deref(), Some("0.2.117"));
    assert_eq!(too_old.version_ok, Some(false));
}
