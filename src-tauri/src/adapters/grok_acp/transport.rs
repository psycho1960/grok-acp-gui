//! AcpTransport — the seam between the agent_runtime module and the
//! concrete ACP adapter (production grok process or Fake ACP).
//!
//! Both the production [`GrokAcpAdapter`](super::process::GrokAcpAdapter)
//! and the test fake implement this trait, so the runtime coordinator
//! can be tested without a real grok binary.
//!
//! This seam is **real** (both implementations exist); it is not a
//! speculative future abstraction.

use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, watch};

use super::codec::AcpMessage;
use crate::bridge::types::SessionId;
use crate::modules::agent_runtime::config::{RuntimeConfig, WorkspaceContext};
use crate::modules::agent_runtime::diagnostics::StderrBuffer;

/// Outcome of spawning an ACP transport.
pub struct TransportHandle {
    /// Sender for outbound JSON-RPC messages (stdin of the child process).
    pub outbound: mpsc::Sender<String>,
    /// Receiver for inbound decoded messages (stdout of the child process).
    pub inbound: mpsc::Receiver<AcpMessage>,
    /// Join handle for the process — awaited on shutdown.
    pub process: tokio::task::JoinHandle<ProcessExit>,
    /// Shared stderr buffer for diagnostics.
    pub stderr: Arc<tokio::sync::Mutex<StderrBuffer>>,
}

/// How a transport process exited.
#[derive(Debug, Clone)]
pub struct ProcessExit {
    pub code: Option<i32>,
    pub signal: Option<String>,
    /// "clean", "crash", "killed", or "unknown".
    pub reason: String,
}

/// Supported official Grok login flows. These map to separate argv entries;
/// callers never supply arbitrary command text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginMethod {
    Oauth,
    DeviceAuth,
}

/// Terminal/non-terminal state reported by the isolated login process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginProcessState {
    Running,
    Succeeded,
    Cancelled,
    TimedOut,
    Failed { exit_code: Option<i32> },
}

/// Adapter-owned login process control. No child handle, stdout, stderr, or
/// environment value crosses into the Runtime module.
pub struct LoginHandle {
    pub state: watch::Receiver<LoginProcessState>,
    pub cancel: std::sync::Mutex<Option<oneshot::Sender<()>>>,
}

impl LoginHandle {
    pub fn status(&self) -> LoginProcessState {
        self.state.borrow().clone()
    }

    pub fn cancel(&self) {
        if let Some(sender) = self
            .cancel
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            let _ = sender.send(());
        }
    }

    pub async fn wait_for_change(&mut self) -> LoginProcessState {
        let _ = self.state.changed().await;
        self.status()
    }
}

/// The transport seam.  Implementations spawn a child process (or fake)
/// and provide channels for bidirectional JSON-RPC communication.
#[async_trait]
pub trait AcpTransport: Send + Sync {
    /// Probe for the agent executable: search default locations and
    /// PATH, check the version, and cache the resolved path.
    ///
    /// Returns `(resolved_path, version_string)` on success.
    /// Must be called (and succeed) before `spawn()`.
    async fn probe(&self, config: &RuntimeConfig) -> Result<(PathBuf, String), TransportError>;

    /// Spawn the transport for the given session.
    ///
    /// **Precondition**: `probe()` must have been called successfully
    /// at least once.  `spawn()` will return `TransportError::ProbeError`
    /// if the executable path has not been resolved.
    ///
    /// The caller is responsible for the handshake (sending `initialize`
    /// and waiting for the response) after this returns successfully.
    async fn spawn(
        &self,
        session_id: SessionId,
        workspace: WorkspaceContext,
        config: &RuntimeConfig,
    ) -> Result<TransportHandle, TransportError>;

    /// Start the official `grok login` flow as a process separate from ACP.
    /// Implementations must return immediately with a cancellable handle.
    async fn start_login(
        &self,
        _method: LoginMethod,
        _timeout_secs: u64,
    ) -> Result<LoginHandle, TransportError> {
        Err(TransportError::ProbeError {
            message: "login flow is unavailable for this transport".into(),
        })
    }

    /// Returns the resolved executable path, if `probe()` has been
    /// called successfully.  Returns an owned `PathBuf` because
    /// implementations use interior mutability.
    fn resolved_path(&self) -> Option<PathBuf>;
}

/// Errors from the transport layer.
#[derive(Debug, Clone)]
pub enum TransportError {
    /// Executable not found at any searched path.
    NotFound { searched: Vec<PathBuf> },
    /// Executable found but does not meet the minimum version.
    VersionTooLow { found: String, required: String },
    /// Process spawn failed (OS-level error).
    SpawnFailed { message: String },
    /// Not authenticated.
    NotAuthenticated,
    /// Generic probe error.
    ProbeError { message: String },
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransportError::NotFound { searched } => {
                write!(f, "grok not found; searched: {:?}", searched)
            }
            TransportError::VersionTooLow { found, required } => {
                write!(f, "grok version {} < required {}", found, required)
            }
            TransportError::SpawnFailed { message } => {
                write!(f, "spawn failed: {}", message)
            }
            TransportError::NotAuthenticated => write!(f, "not authenticated"),
            TransportError::ProbeError { message } => write!(f, "probe error: {}", message),
        }
    }
}

impl std::error::Error for TransportError {}
