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

    pub const ACP_HANDSHAKE_FAILED: &str = "ACP_HANDSHAKE_FAILED";
    pub const ACP_UNSUPPORTED_CAPABILITY: &str = "ACP_UNSUPPORTED_CAPABILITY";
    pub const ACP_REQUEST_FAILED: &str = "ACP_REQUEST_FAILED";

    pub const PROJECT_NOT_FOUND: &str = "PROJECT_NOT_FOUND";
    pub const PROJECT_ALREADY_EXISTS: &str = "PROJECT_ALREADY_EXISTS";

    pub const GIT_COMMAND_FAILED: &str = "GIT_COMMAND_FAILED";
    pub const GIT_LOCKED: &str = "GIT_LOCKED";

    pub const WORKTREE_ALREADY_EXISTS: &str = "WORKTREE_ALREADY_EXISTS";
    pub const WORKTREE_OUTSIDE_REPO: &str = "WORKTREE_OUTSIDE_REPO";

    pub const INTEGRATION_CONFLICT: &str = "INTEGRATION_CONFLICT";
    pub const INTEGRATION_DIRTY: &str = "INTEGRATION_DIRTY";

    pub const ARTIFACT_TOO_LARGE: &str = "ARTIFACT_TOO_LARGE";
    pub const ARTIFACT_INVALID_FORMAT: &str = "ARTIFACT_INVALID_FORMAT";

    pub const DB_MIGRATION_FAILED: &str = "DB_MIGRATION_FAILED";
    pub const DB_QUERY_FAILED: &str = "DB_QUERY_FAILED";

    pub const BRIDGE_UNSUPPORTED_COMMAND: &str = "BRIDGE_UNSUPPORTED_COMMAND";
    pub const BRIDGE_INVALID_PAYLOAD: &str = "BRIDGE_INVALID_PAYLOAD";
    pub const BRIDGE_VALIDATION_FAILED: &str = "BRIDGE_VALIDATION_FAILED";
}
