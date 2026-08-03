//! Domain types — the canonical Rust representation of every entity
//! persisted in SQLite and referenced by modules.
//!
//! These types are pure data; state-transition logic lives in `state.rs`.
//! Bridge DTOs are mapped to/from these types in the `bridge` crate.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Re-export bridge ID newtypes — domain owns the "what these IDs mean"
// ---------------------------------------------------------------------------

pub use crate::bridge::types::{utc_now, CorrelationId, DisplayPath, ProjectId, SessionId, TaskId};

// ---------------------------------------------------------------------------
// Additional ID newtypes for domain-owned entities
// ---------------------------------------------------------------------------

macro_rules! id_newtype {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(raw: impl Into<String>) -> Self {
                Self(raw.into())
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

id_newtype!(WorktreeId);
id_newtype!(AttachmentId);
id_newtype!(RecoveryId);

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// Lifecycle status of a Task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// Initial state after creation; not yet bound to a Workspace.
    Preparing,
    /// Bound to a valid Workspace and actively executing.
    Running,
    /// Awaiting user permission resolution.
    WaitingPermission,
    /// Integration in progress (squash merge into target).
    Integrating,
    /// Successfully merged and completed.
    Merged,
    /// User-archived; retained for reference.
    Archived,
    /// Process died or app exited while in a non-terminal state.
    Interrupted,
}

impl TaskStatus {
    /// Returns `true` if this is a terminal state from which the task
    /// cannot resume.
    pub fn is_terminal(&self) -> bool {
        matches!(self, TaskStatus::Merged | TaskStatus::Archived)
    }

    /// Returns `true` if this status implies the owning process may
    /// not be alive — used during startup recovery.
    pub fn implies_live_process(&self) -> bool {
        matches!(
            self,
            TaskStatus::Running | TaskStatus::WaitingPermission | TaskStatus::Integrating
        )
    }
}

/// Where a task performs file-level work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceKind {
    /// Isolated Git worktree (default).
    Worktree,
    /// Read-only view of a repo; no file modifications.
    Readonly,
    /// Operates directly on the checkout (admin-only).
    Direct,
}

/// Ownership classification for a Worktree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeOwnership {
    /// Created and managed by this application.
    Managed,
    /// Externally created; read-only unless explicitly adopted.
    External,
}

/// Operational state of a Worktree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeState {
    /// Worktree is clean and ready.
    Ready,
    /// Worktree has uncommitted changes.
    Dirty,
    /// Integration is in progress on this worktree.
    Integrating,
    /// Worktree has been deleted (pruned).
    Deleted,
    /// Unknown or unresolvable state.
    Unknown,
}

/// State of an ACP session binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    /// Session is active and processing.
    Active,
    /// Session is idle (no active turn).
    Idle,
    /// Session process exited; requires resume.
    Disconnected,
    /// Session is closed and will not be resumed.
    Closed,
}

/// Lifecycle state of a recovery item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryState {
    /// Recovery package is available for restore.
    Available,
    /// Recovery package has passed its expiry.
    Expired,
    /// Restore operation has been initiated.
    Restoring,
    /// Package has been successfully restored.
    Restored,
    /// Package has been explicitly deleted.
    Deleted,
}

// ---------------------------------------------------------------------------
// Domain entities
// ---------------------------------------------------------------------------

/// A project directory recognized by the application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: ProjectId,
    /// Normalized absolute path (used internally for validation).
    pub path: String,
    /// Display-safe path shown in the UI.
    pub display_path: String,
    /// The git repository root, if discovered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    /// When the user first trusted this project.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trusted_at: Option<String>,
    /// Timestamp of the last open.
    pub last_opened_at: String,
}

/// A unit of work managed by the application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: TaskId,
    pub project_id: ProjectId,
    pub title: String,
    pub status: TaskStatus,
    pub workspace_kind: WorkspaceKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// Human-readable reason when status is Interrupted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interrupt_reason: Option<String>,
}

/// Links a Task to an ACP session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBinding {
    pub task_id: TaskId,
    pub session_id: SessionId,
    /// Working directory of the session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Last acknowledged sequence number.
    pub last_seq: u64,
    /// Current session state.
    pub state: SessionState,
}

/// A Git worktree tracked by the application.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeRecord {
    pub id: WorktreeId,
    pub task_id: TaskId,
    pub repo_root: String,
    /// Filesystem path to the worktree.
    pub path: String,
    /// Display-safe path for the UI.
    pub display_path: String,
    /// The branch name for this worktree.
    pub branch: String,
    /// The base branch from which this worktree was created.
    pub base_branch: String,
    /// The commit at which this worktree was created.
    pub base_commit: String,
    pub ownership: WorktreeOwnership,
    pub state: WorktreeState,
}

/// Metadata record for an imported artifact (image, file).
///
/// The actual bytes live on the filesystem in a managed cache directory;
/// this record stores the metadata, hash, and cache location.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentRecord {
    pub id: AttachmentId,
    pub task_id: TaskId,
    /// SHA-256 hex digest of the artifact bytes.
    pub sha256: String,
    /// MIME type (e.g. "image/png").
    pub mime: String,
    /// Byte count.
    pub bytes: u64,
    /// Path within the managed cache directory.
    pub cache_path: String,
    /// Original filename from the user.
    pub source_name: String,
    pub created_at: String,
}

/// A recovery package that can restore worktree state after
/// forced cleanup or crash.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryItem {
    pub id: RecoveryId,
    pub task_id: TaskId,
    /// Directory containing the recovery package.
    pub directory: String,
    /// Path to the manifest.json within the package.
    pub manifest_path: String,
    /// ISO-8601 UTC when the package expires.
    pub expires_at: String,
    pub state: RecoveryState,
}

/// Key-value settings row.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub key: String,
    pub json_value: serde_json::Value,
}

// ---------------------------------------------------------------------------
// Bootstrap snapshot — the aggregate returned on app startup
// ---------------------------------------------------------------------------

/// The complete application state snapshot returned by `bootstrap()`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapSnapshot {
    pub product_name: String,
    pub version: String,
    pub platform: String,
    /// All known projects.
    pub projects: Vec<Project>,
    /// All active (non-archived, non-merged) tasks with their bindings.
    pub active_tasks: Vec<Task>,
    /// Bindings for active tasks.
    pub bindings: Vec<SessionBinding>,
    /// Worktree records for active tasks.
    pub worktrees: Vec<WorktreeRecord>,
    /// Available recovery items.
    pub recovery_items: Vec<RecoveryItem>,
    /// Application settings.
    pub settings: Vec<Settings>,
    /// Whether startup recovery was performed.
    pub recovery_performed: bool,
    /// Number of tasks that were transitioned to Interrupted.
    pub tasks_interrupted: u32,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_status_terminal() {
        assert!(TaskStatus::Merged.is_terminal());
        assert!(TaskStatus::Archived.is_terminal());
        assert!(!TaskStatus::Running.is_terminal());
        assert!(!TaskStatus::Interrupted.is_terminal());
    }

    #[test]
    fn task_status_live_process() {
        assert!(TaskStatus::Running.implies_live_process());
        assert!(TaskStatus::WaitingPermission.implies_live_process());
        assert!(TaskStatus::Integrating.implies_live_process());
        assert!(!TaskStatus::Preparing.implies_live_process());
        assert!(!TaskStatus::Interrupted.implies_live_process());
    }

    #[test]
    fn bootstrap_snapshot_serde() {
        let snap = BootstrapSnapshot {
            product_name: "Grok ACP GUI".into(),
            version: "0.1.16".into(),
            platform: "windows".into(),
            projects: vec![],
            active_tasks: vec![],
            bindings: vec![],
            worktrees: vec![],
            recovery_items: vec![],
            settings: vec![],
            recovery_performed: false,
            tasks_interrupted: 0,
        };
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("productName"));
        assert!(json.contains("recoveryPerformed"));
    }

    #[test]
    fn id_newtype_display() {
        let id = WorktreeId::new("wt-1");
        assert_eq!(id.to_string(), "wt-1");
    }

    #[test]
    fn serde_roundtrip_task_status() {
        let status = TaskStatus::WaitingPermission;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"waiting_permission\"");
        let back: TaskStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, TaskStatus::WaitingPermission);
    }

    #[test]
    fn serde_roundtrip_workspace_kind() {
        let kind = WorkspaceKind::Worktree;
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, "\"worktree\"");
        let back: WorkspaceKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, WorkspaceKind::Worktree);
    }

    #[test]
    fn serde_roundtrip_project() {
        let p = Project {
            id: ProjectId::new("p1"),
            path: "C:\\Users\\test\\project".into(),
            display_path: "~/project".into(),
            repo_root: Some("C:\\Users\\test\\project".into()),
            trusted_at: Some(utc_now()),
            last_opened_at: utc_now(),
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: Project = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, p.id);
        assert_eq!(back.display_path, "~/project");
    }
}
