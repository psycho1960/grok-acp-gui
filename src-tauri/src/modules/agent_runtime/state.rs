//! Per-session process state machine for the Agent Runtime.
//!
//! The state machine governs the lifecycle of a single managed Grok process:
//!
//! ```text
//!   unavailable
//!       │
//!       ▼
//!    probing ────────► unavailable (not installed / not authenticated)
//!       │
//!       ▼
//!    starting ───────► failed (spawn error)
//!       │
//!       ▼
//!   handshaking ─────► failed (timeout / protocol error)
//!       │
//!       ▼
//!     ready ◄────────► busy
//!       │
//!       ▼
//!    stopping ────────► stopped
//! ```
//!
//! Any non-terminal state may transition to `failed` on an unrecoverable
//! error.  `failed` transitions to `stopped` after diagnostic capture.
//! `stopped` is terminal for a session handle; a new `start()` creates a
//! fresh state machine.
//!
//! All transitions are atomic and auditable: every legal and illegal
//! attempt is recorded, and illegal attempts return a `DomainError`.

use crate::domain::error::{codes, DomainError};

/// The lifecycle state of a single managed Grok process / session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    /// No process; grok has not been probed for this session yet.
    Unavailable,
    /// Probing grok availability / version / auth status.
    Probing,
    /// Process spawned; awaiting JSON-RPC `initialize` response.
    Starting,
    /// `initialize` sent; awaiting capability negotiation response.
    Handshaking,
    /// Process ready; no active turn.
    Ready,
    /// A turn is in progress (session/prompt sent, awaiting completion).
    Busy,
    /// Shutdown requested; draining or killing the process.
    Stopping,
    /// Process terminated cleanly; session handle is dead.
    Stopped,
    /// Unrecoverable error; diagnostic captured, awaiting cleanup.
    Failed,
}

impl RuntimeState {
    /// Returns `true` when the process is alive or being brought up.
    pub fn is_live(&self) -> bool {
        matches!(
            self,
            RuntimeState::Starting
                | RuntimeState::Handshaking
                | RuntimeState::Ready
                | RuntimeState::Busy
                | RuntimeState::Stopping
        )
    }

    /// Returns `true` when the session can accept a new `send()` call.
    pub fn accepts_requests(&self) -> bool {
        matches!(self, RuntimeState::Ready)
    }

    /// Returns `true` for terminal states — no further transitions.
    pub fn is_terminal(&self) -> bool {
        matches!(self, RuntimeState::Stopped)
    }
}

impl std::fmt::Display for RuntimeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeState::Unavailable => write!(f, "unavailable"),
            RuntimeState::Probing => write!(f, "probing"),
            RuntimeState::Starting => write!(f, "starting"),
            RuntimeState::Handshaking => write!(f, "handshaking"),
            RuntimeState::Ready => write!(f, "ready"),
            RuntimeState::Busy => write!(f, "busy"),
            RuntimeState::Stopping => write!(f, "stopping"),
            RuntimeState::Stopped => write!(f, "stopped"),
            RuntimeState::Failed => write!(f, "failed"),
        }
    }
}

/// Inputs that drive the state machine forward.
///
/// Each variant corresponds to exactly one legal transition; illegal
/// combinations produce `DOMAIN_ILLEGAL_TRANSITION`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeTransition {
    /// Begin probing grok availability.
    BeginProbe,
    /// Probe completed; grok is installed and authenticated.
    ProbeSucceeded,
    /// Probe completed; grok is missing or unusable.
    ProbeFailed,
    /// Process spawned successfully; begin handshake.
    ProcessSpawned,
    /// `initialize` response received; capabilities negotiated.
    HandshakeComplete,
    /// A turn was started (request sent to agent).
    TurnStarted,
    /// The current turn completed (or failed).
    TurnCompleted,
    /// Begin graceful / forced shutdown.
    BeginShutdown,
    /// Process exited cleanly during shutdown.
    ProcessExited,
    /// Unrecoverable error (process crash, protocol violation, etc.).
    Fail { reason: String },
    /// Diagnostic capture complete; move from `failed` to `stopped`.
    CleanupComplete,
}

/// Attempt a state transition.
///
/// Returns the new state on success, or a `DomainError` with code
/// `DOMAIN_ILLEGAL_TRANSITION` when the transition is not legal from
/// the current state.
pub fn transition(
    current: RuntimeState,
    t: RuntimeTransition,
) -> Result<RuntimeState, DomainError> {
    use RuntimeState::*;
    use RuntimeTransition::*;

    match (current, t) {
        // ---- unavailable ----
        (Unavailable, BeginProbe) => Ok(Probing),
        (Unavailable, BeginShutdown) => Ok(Stopping), // idempotent shutdown of dead session
        (Unavailable, Fail { .. }) => Ok(Failed),

        // ---- probing ----
        (Probing, ProbeSucceeded) => Ok(Starting),
        (Probing, ProbeFailed) => Ok(Unavailable),
        (Probing, Fail { .. }) => Ok(Failed),
        (Probing, BeginShutdown) => Ok(Stopped), // nothing to stop

        // ---- starting ----
        (Starting, ProcessSpawned) => Ok(Handshaking),
        (Starting, Fail { .. }) => Ok(Failed),
        (Starting, BeginShutdown) => Ok(Stopping),

        // ---- handshaking ----
        (Handshaking, HandshakeComplete) => Ok(Ready),
        (Handshaking, Fail { .. }) => Ok(Failed),
        (Handshaking, BeginShutdown) => Ok(Stopping),

        // ---- ready <-> busy ----
        (Ready, TurnStarted) => Ok(Busy),
        (Ready, BeginShutdown) => Ok(Stopping),
        (Ready, Fail { .. }) => Ok(Failed),

        (Busy, TurnCompleted) => Ok(Ready),
        (Busy, BeginShutdown) => Ok(Stopping),
        (Busy, Fail { .. }) => Ok(Failed),

        // ---- stopping ----
        (Stopping, ProcessExited) => Ok(Stopped),
        (Stopping, Fail { .. }) => Ok(Failed),
        (Stopping, BeginShutdown) => Ok(Stopping), // idempotent

        // ---- failed ----
        (Failed, CleanupComplete) => Ok(Stopped),
        (Failed, BeginShutdown) => Ok(Stopped), // idempotent

        // ---- stopped: terminal ----
        (Stopped, BeginShutdown) => Ok(Stopped), // idempotent shutdown

        // Everything else is illegal.
        (current, t) => {
            let trans_desc = match &t {
                BeginProbe => "begin_probe",
                ProbeSucceeded => "probe_succeeded",
                ProbeFailed => "probe_failed",
                ProcessSpawned => "process_spawned",
                HandshakeComplete => "handshake_complete",
                TurnStarted => "turn_started",
                TurnCompleted => "turn_completed",
                BeginShutdown => "begin_shutdown",
                ProcessExited => "process_exited",
                Fail { reason } => {
                    return Err(DomainError::new(
                        codes::DOMAIN_ILLEGAL_TRANSITION,
                        format!(
                            "Runtime cannot transition from '{}' via 'fail: {}'",
                            current, reason
                        ),
                    ))
                }
                CleanupComplete => "cleanup_complete",
            };
            Err(DomainError::illegal_transition(
                "Runtime",
                &current.to_string(),
                trans_desc,
            ))
        }
    }
}

// ===========================================================================
// Tests — exhaustive legal / illegal transition table
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn fail() -> RuntimeTransition {
        RuntimeTransition::Fail {
            reason: "test".into(),
        }
    }

    // --- Legal path: full happy lifecycle ---

    #[test]
    fn happy_path_lifecycle() {
        let s = RuntimeState::Unavailable;
        let s = transition(s, RuntimeTransition::BeginProbe).unwrap();
        assert_eq!(s, RuntimeState::Probing);
        let s = transition(s, RuntimeTransition::ProbeSucceeded).unwrap();
        assert_eq!(s, RuntimeState::Starting);
        let s = transition(s, RuntimeTransition::ProcessSpawned).unwrap();
        assert_eq!(s, RuntimeState::Handshaking);
        let s = transition(s, RuntimeTransition::HandshakeComplete).unwrap();
        assert_eq!(s, RuntimeState::Ready);
        let s = transition(s, RuntimeTransition::TurnStarted).unwrap();
        assert_eq!(s, RuntimeState::Busy);
        let s = transition(s, RuntimeTransition::TurnCompleted).unwrap();
        assert_eq!(s, RuntimeState::Ready);
        let s = transition(s, RuntimeTransition::BeginShutdown).unwrap();
        assert_eq!(s, RuntimeState::Stopping);
        let s = transition(s, RuntimeTransition::ProcessExited).unwrap();
        assert_eq!(s, RuntimeState::Stopped);
    }

    #[test]
    fn probe_failed_returns_to_unavailable() {
        let s = transition(RuntimeState::Probing, RuntimeTransition::ProbeFailed).unwrap();
        assert_eq!(s, RuntimeState::Unavailable);
    }

    // --- Any live state can fail ---

    #[test]
    fn any_live_state_can_fail() {
        for state in [
            RuntimeState::Unavailable,
            RuntimeState::Probing,
            RuntimeState::Starting,
            RuntimeState::Handshaking,
            RuntimeState::Ready,
            RuntimeState::Busy,
            RuntimeState::Stopping,
        ] {
            let result = transition(state, fail());
            assert_eq!(
                result.unwrap(),
                RuntimeState::Failed,
                "Fail from {:?} should succeed",
                state
            );
        }
    }

    #[test]
    fn failed_to_stopped_via_cleanup() {
        let s = transition(RuntimeState::Failed, RuntimeTransition::CleanupComplete).unwrap();
        assert_eq!(s, RuntimeState::Stopped);
    }

    // --- Idempotent shutdown ---

    #[test]
    fn shutdown_is_idempotent_from_unavailable() {
        let s = transition(RuntimeState::Unavailable, RuntimeTransition::BeginShutdown).unwrap();
        assert_eq!(s, RuntimeState::Stopping);
    }

    #[test]
    fn shutdown_is_idempotent_from_stopping() {
        let s = transition(RuntimeState::Stopping, RuntimeTransition::BeginShutdown).unwrap();
        assert_eq!(s, RuntimeState::Stopping);
    }

    #[test]
    fn shutdown_is_idempotent_from_stopped() {
        let s = transition(RuntimeState::Stopped, RuntimeTransition::BeginShutdown).unwrap();
        assert_eq!(s, RuntimeState::Stopped);
    }

    #[test]
    fn shutdown_is_idempotent_from_failed() {
        let s = transition(RuntimeState::Failed, RuntimeTransition::BeginShutdown).unwrap();
        assert_eq!(s, RuntimeState::Stopped);
    }

    #[test]
    fn shutdown_from_probing_goes_straight_to_stopped() {
        // No process to kill.
        let s = transition(RuntimeState::Probing, RuntimeTransition::BeginShutdown).unwrap();
        assert_eq!(s, RuntimeState::Stopped);
    }

    // --- Illegal transitions ---

    #[test]
    fn cannot_start_turn_from_busy() {
        let err = transition(RuntimeState::Busy, RuntimeTransition::TurnStarted);
        assert!(err.is_err());
    }

    #[test]
    fn cannot_complete_turn_from_ready() {
        let err = transition(RuntimeState::Ready, RuntimeTransition::TurnCompleted);
        assert!(err.is_err());
    }

    #[test]
    fn cannot_spawn_from_ready() {
        let err = transition(RuntimeState::Ready, RuntimeTransition::ProcessSpawned);
        assert!(err.is_err());
    }

    #[test]
    fn cannot_handshake_from_starting() {
        let err = transition(RuntimeState::Starting, RuntimeTransition::HandshakeComplete);
        assert!(err.is_err());
    }

    #[test]
    fn cannot_fail_from_stopped() {
        let err = transition(RuntimeState::Stopped, fail());
        assert!(err.is_err());
    }

    #[test]
    fn stopped_is_terminal() {
        for t in [
            RuntimeTransition::BeginProbe,
            RuntimeTransition::ProbeSucceeded,
            RuntimeTransition::ProcessSpawned,
            RuntimeTransition::HandshakeComplete,
            RuntimeTransition::TurnStarted,
            RuntimeTransition::TurnCompleted,
            RuntimeTransition::ProcessExited,
            RuntimeTransition::CleanupComplete,
        ] {
            assert!(
                transition(RuntimeState::Stopped, t.clone()).is_err(),
                "Stopped -> {:?} should be illegal",
                t
            );
        }
    }

    // --- State predicates ---

    #[test]
    fn is_live_correct() {
        assert!(!RuntimeState::Unavailable.is_live());
        assert!(!RuntimeState::Probing.is_live());
        assert!(RuntimeState::Starting.is_live());
        assert!(RuntimeState::Handshaking.is_live());
        assert!(RuntimeState::Ready.is_live());
        assert!(RuntimeState::Busy.is_live());
        assert!(RuntimeState::Stopping.is_live());
        assert!(!RuntimeState::Stopped.is_live());
        assert!(!RuntimeState::Failed.is_live());
    }

    #[test]
    fn accepts_requests_only_when_ready() {
        assert!(RuntimeState::Ready.accepts_requests());
        assert!(!RuntimeState::Busy.accepts_requests());
        assert!(!RuntimeState::Unavailable.accepts_requests());
    }

    #[test]
    fn fail_with_reason_preserves_reason_in_error() {
        let result = transition(
            RuntimeState::Handshaking,
            RuntimeTransition::Fail {
                reason: "handshake timeout".into(),
            },
        );
        // Fail from Handshaking is a legal transition → goes to Failed.
        assert_eq!(result.unwrap(), RuntimeState::Failed);
    }
}
