//! Domain-level error classification.
//!
//! GAG-003 defines the error code taxonomy shared by all modules.
//! Each module uses these constructors so the bridge can produce
//! consistent `AppError` values without leaking internal detail.

/// Well-known error code prefixes.
pub mod codes {
    pub const RUNTIME_PROBE_FAILED: &str = "RUNTIME_PROBE_FAILED";
    pub const RUNTIME_NOT_FOUND: &str = "RUNTIME_NOT_FOUND";
    pub const RUNTIME_LOGIN_FAILED: &str = "RUNTIME_LOGIN_FAILED";
    pub const RUNTIME_PROCESS_DIED: &str = "RUNTIME_PROCESS_DIED";
    pub const RUNTIME_INVALID_MODEL: &str = "RUNTIME_INVALID_MODEL";
    pub const RUNTIME_MODEL_ENV_MISSING: &str = "RUNTIME_MODEL_ENV_MISSING";

    pub const ACP_HANDSHAKE_FAILED: &str = "ACP_HANDSHAKE_FAILED";
    pub const ACP_UNSUPPORTED_CAPABILITY: &str = "ACP_UNSUPPORTED_CAPABILITY";
    pub const ACP_REQUEST_FAILED: &str = "ACP_REQUEST_FAILED";

    pub const PROJECT_NOT_FOUND: &str = "PROJECT_NOT_FOUND";
    pub const PROJECT_ALREADY_EXISTS: &str = "PROJECT_ALREADY_EXISTS";

    pub const GIT_COMMAND_FAILED: &str = "GIT_COMMAND_FAILED";
    pub const GIT_LOCKED: &str = "GIT_LOCKED";

    pub const WORKTREE_ALREADY_EXISTS: &str = "WORKTREE_ALREADY_EXISTS";
    pub const WORKTREE_OUTSIDE_REPO: &str = "WORKTREE_OUTSIDE_REPO";
    pub const WORKTREE_NOT_READY: &str = "WORKTREE_NOT_READY";

    pub const INTEGRATION_CONFLICT: &str = "INTEGRATION_CONFLICT";
    pub const INTEGRATION_DIRTY: &str = "INTEGRATION_DIRTY";

    pub const ARTIFACT_TOO_LARGE: &str = "ARTIFACT_TOO_LARGE";
    pub const ARTIFACT_INVALID_FORMAT: &str = "ARTIFACT_INVALID_FORMAT";
    pub const ARTIFACT_NOT_FOUND: &str = "ARTIFACT_NOT_FOUND";
    pub const ARTIFACT_CACHE_MISSING: &str = "ARTIFACT_CACHE_MISSING";
    pub const ARTIFACT_VISION_FAILED: &str = "ARTIFACT_VISION_FAILED";

    pub const DB_MIGRATION_FAILED: &str = "DB_MIGRATION_FAILED";
    pub const DB_QUERY_FAILED: &str = "DB_QUERY_FAILED";

    pub const BRIDGE_UNSUPPORTED_COMMAND: &str = "BRIDGE_UNSUPPORTED_COMMAND";
    pub const BRIDGE_INVALID_PAYLOAD: &str = "BRIDGE_INVALID_PAYLOAD";
    pub const BRIDGE_VALIDATION_FAILED: &str = "BRIDGE_VALIDATION_FAILED";
    pub const BRIDGE_NOT_IMPLEMENTED: &str = "BRIDGE_NOT_IMPLEMENTED";

    // Domain-level errors (GAG-004+)
    pub const DOMAIN_ILLEGAL_TRANSITION: &str = "DOMAIN_ILLEGAL_TRANSITION";
    pub const DOMAIN_TASK_NOT_FOUND: &str = "DOMAIN_TASK_NOT_FOUND";
    pub const DOMAIN_WORKTREE_NOT_FOUND: &str = "DOMAIN_WORKTREE_NOT_FOUND";

    // GAG-006: Concurrency & recovery errors
    pub const CONCURRENCY_LIMIT_EXCEEDED: &str = "CONCURRENCY_LIMIT_EXCEEDED";
    pub const EVENT_DUPLICATE: &str = "EVENT_DUPLICATE";
    pub const EVENT_GAP_DETECTED: &str = "EVENT_GAP_DETECTED";
    pub const EVENT_REPLAY_BLOCKED: &str = "EVENT_REPLAY_BLOCKED";
    pub const RECOVERY_NO_SESSION: &str = "RECOVERY_NO_SESSION";
    pub const RECOVERY_ALREADY_RESUMED: &str = "RECOVERY_ALREADY_RESUMED";

    // GAG-009: permission, plan, and execution-guard failures.
    pub const PERMISSION_DENIED: &str = "PERMISSION_DENIED";
    pub const PERMISSION_PENDING: &str = "PERMISSION_PENDING";
    pub const PERMISSION_EXPIRED: &str = "PERMISSION_EXPIRED";
    pub const PERMISSION_ALREADY_RESOLVED: &str = "PERMISSION_ALREADY_RESOLVED";
    pub const PERMISSION_CONTEXT_MISMATCH: &str = "PERMISSION_CONTEXT_MISMATCH";
    pub const PLAN_NOT_APPROVED: &str = "PLAN_NOT_APPROVED";
    pub const PLAN_VERSION_MISMATCH: &str = "PLAN_VERSION_MISMATCH";
    pub const OPERATION_UNKNOWN: &str = "OPERATION_UNKNOWN";
}

// ---------------------------------------------------------------------------
// DomainError — pure-domain error, never leaks SQL or paths to bridge
// ---------------------------------------------------------------------------

/// A domain-level error that explains *what* went wrong without exposing
/// database internals, absolute paths, or raw SQL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

impl DomainError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable: false,
        }
    }

    pub fn retryable(mut self) -> Self {
        self.retryable = true;
        self
    }

    /// Illegal state machine transition.
    pub fn illegal_transition(entity: &str, current: &str, attempted: &str) -> Self {
        Self::new(
            codes::DOMAIN_ILLEGAL_TRANSITION,
            format!(
                "{} cannot transition from '{}' via '{}'",
                entity, current, attempted
            ),
        )
    }

    /// Entity not found in the database.
    pub fn not_found(entity: &str, id: &str) -> Self {
        let code = match entity {
            "Task" => codes::DOMAIN_TASK_NOT_FOUND,
            "Worktree" => codes::DOMAIN_WORKTREE_NOT_FOUND,
            _ => codes::PROJECT_NOT_FOUND,
        };
        Self::new(code, format!("{} with id '{}' not found", entity, id))
    }
}

impl std::fmt::Display for DomainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for DomainError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_error_display() {
        let err = DomainError::illegal_transition("Task", "merged", "start");
        let s = err.to_string();
        assert!(s.contains("DOMAIN_ILLEGAL_TRANSITION"));
        assert!(s.contains("merged"));
    }

    #[test]
    fn domain_error_not_found() {
        let err = DomainError::not_found("Task", "task-1");
        assert_eq!(err.code, codes::DOMAIN_TASK_NOT_FOUND);
        assert!(err.message.contains("task-1"));
    }
}
