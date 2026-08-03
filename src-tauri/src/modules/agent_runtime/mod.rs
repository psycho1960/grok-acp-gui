//! MOD-AGENT-RUNTIME: the deep module that owns the Grok process lifecycle.
//!
//! Upper layers (the bridge) interact with the agent exclusively through
//! the [`AgentRuntime`] trait.  They never see:
//!
//! - Child process handles
//! - JSON-RPC frames
//! - stdin / stdout pipes
//! - Protocol version negotiation details
//! - stderr content
//!
//! The module internally uses the `grok_acp` adapter (production) and
//! the Fake ACP adapter (tests), both of which implement the same
//! [`AcpTransport`](crate::adapters::grok_acp) seam.

pub mod config;
pub mod diagnostics;
pub mod events;
pub mod requests;
pub mod runtime;
pub mod state;

// Re-export the most commonly used types at the module root.
pub use config::{RuntimeConfig, RuntimeHandle, RuntimeProbeResult, WorkspaceContext};
pub use events::{AgentEvent, EventMeta, Sequence, TimestampedEvent};
pub use requests::{ClientRequest, SendAck};
pub use runtime::AgentRuntimeImpl;
pub use state::{transition, RuntimeState, RuntimeTransition};

use crate::bridge::types::SessionId;
use crate::domain::error::DomainError;
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::sync::mpsc;

/// The public Interface of the Agent Runtime module.
///
/// Every method is async because the underlying transport (JSON-RPC over
/// stdio) is inherently asynchronous.  Callers must NOT assume any method
/// blocks the calling thread.
///
/// # Concurrency
/// - `probe` may be called concurrently with `start` / `send`.
/// - `send`, `cancel`, and `shutdown` for the **same** session are
///   serialised internally; concurrent calls for different sessions are
///   independent.
/// - `subscribe` returns a receiver that receives [`AgentEvent`]s for
///   ALL sessions.  The caller filters by `session_id`.
#[async_trait]
pub trait AgentRuntime: Send + Sync {
    /// Check whether the Grok CLI is installed, meets the minimum version,
    /// and (when checkable) is authenticated.  Does NOT spawn a long-lived
    /// process.
    async fn probe(&self, config: &RuntimeConfig) -> RuntimeProbeResult;

    /// Spawn a managed Grok process for the given session and perform
    /// the ACP handshake.  Returns a handle on success.
    ///
    /// # Errors
    /// - `RUNTIME_PROBE_FAILED` — grok not found or version too low.
    /// - `ACP_HANDSHAKE_FAILED` — process spawned but handshake timed out
    ///   or returned an error.
    /// - `RUNTIME_PROCESS_DIED` — process exited during handshake.
    async fn start(
        &self,
        session_id: SessionId,
        workspace: WorkspaceContext,
        config: &RuntimeConfig,
    ) -> Result<RuntimeHandle, DomainError>;

    /// Send a client request to the agent for the given session.
    /// Returns the allocated request ID.
    ///
    /// # Errors
    /// - `DOMAIN_ILLEGAL_TRANSITION` — session is not in `Ready` state.
    /// - `ACP_REQUEST_FAILED` — the agent rejected the request.
    async fn send(
        &self,
        session_id: SessionId,
        request: ClientRequest,
    ) -> Result<SendAck, DomainError>;

    /// Cancel the current turn for the given session.  Idempotent —
    /// calling cancel when no turn is active is a no-op.
    ///
    /// When `request_id` is provided, cancels a specific request;
    /// otherwise cancels the most recent active request.
    async fn cancel(&self, session_id: SessionId, request_id: Option<u64>);

    /// Shut down the managed process for the given session.  Idempotent —
    /// calling shutdown on an already-stopped session is a no-op.
    ///
    /// The process is first sent a graceful shutdown signal; if it does
    /// not exit within a timeout, it is killed.
    async fn shutdown(&self, session_id: SessionId, reason: &str);

    /// Subscribe to the global event stream.  All sessions' events are
    /// multiplexed on this channel; filter by `meta.session_id`.
    ///
    /// The receiver is bounded; slow consumers will cause events to be
    /// dropped (with a `diagnostic.notice` warning).
    fn subscribe(&self) -> mpsc::Receiver<TimestampedEvent>;

    /// Returns the current state of a session, or `None` if the session
    /// is unknown / never started.
    fn session_state(&self, session_id: &SessionId) -> Option<RuntimeState>;
}

/// Returns the default search paths for the grok executable on this
/// platform, in priority order.  Used by the adapter and for diagnostics.
pub fn default_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // 1. User-configured path (checked by adapter when set in config).

    // 2. Platform default install location.
    if cfg!(target_os = "windows") {
        if let Ok(userprofile) = std::env::var("USERPROFILE") {
            paths.push(
                PathBuf::from(userprofile)
                    .join(".grok")
                    .join("bin")
                    .join("grok.exe"),
            );
        }
        if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
            paths.push(
                PathBuf::from(localappdata)
                    .join("Programs")
                    .join("grok")
                    .join("grok.exe"),
            );
        }
    } else if cfg!(target_os = "macos") {
        paths.push(PathBuf::from("/usr/local/bin/grok"));
        paths.push(PathBuf::from("/opt/homebrew/bin/grok"));
    } else {
        paths.push(PathBuf::from("/usr/bin/grok"));
        paths.push(PathBuf::from("/usr/local/bin/grok"));
    }

    // 3. PATH (adapter falls back to bare "grok" / "grok.exe" which
    //    relies on the OS PATH resolution).
    let exe_name = if cfg!(target_os = "windows") {
        "grok.exe"
    } else {
        "grok"
    };
    paths.push(PathBuf::from(exe_name));

    paths
}
