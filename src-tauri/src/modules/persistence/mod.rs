//! MOD-PERSISTENCE: Repository Interface.
//!
//! All data access crosses this trait. No caller ever receives a raw
//! `rusqlite::Connection` — only the domain types returned by these methods.

use crate::domain::error::DomainError;
use crate::domain::types::{
    AttachmentRecord, BootstrapSnapshot, Project, RecoveryItem, SessionBinding, Settings, Task,
    WorktreeRecord,
};

/// Result alias used throughout the persistence layer.
pub type RepoResult<T> = Result<T, DomainError>;

/// The complete Repository Interface required by GAG-004.
///
/// Implementations must use transactions for multi-entity writes and
/// must not leak SQL state (connection handles, raw errors) to callers.
pub trait Repository: Send + Sync {
    // ------------------------------------------------------------------
    // Bootstrap
    // ------------------------------------------------------------------

    /// Load the complete bootstrap snapshot: all active entities plus
    /// application settings. Used by the bridge `bootstrap()` command.
    fn bootstrap_snapshot(&self) -> RepoResult<BootstrapSnapshot>;

    // ------------------------------------------------------------------
    // Projects
    // ------------------------------------------------------------------

    fn create_project(&self, project: &Project) -> RepoResult<()>;
    fn get_project(&self, id: &str) -> RepoResult<Project>;
    fn list_projects(&self) -> RepoResult<Vec<Project>>;
    fn update_project(&self, project: &Project) -> RepoResult<()>;
    fn delete_project(&self, id: &str) -> RepoResult<()>;

    // ------------------------------------------------------------------
    // Tasks
    // ------------------------------------------------------------------

    fn create_task(&self, task: &Task) -> RepoResult<()>;
    fn get_task(&self, id: &str) -> RepoResult<Task>;
    fn list_tasks_by_project(&self, project_id: &str) -> RepoResult<Vec<Task>>;
    fn list_active_tasks(&self) -> RepoResult<Vec<Task>>;
    fn update_task(&self, task: &Task) -> RepoResult<()>;
    /// Transactionally update task status (used by startup recovery).
    fn update_task_status(&self, id: &str, status: &str, reason: Option<&str>) -> RepoResult<()>;

    // ------------------------------------------------------------------
    // Session Bindings
    // ------------------------------------------------------------------

    fn create_binding(&self, binding: &SessionBinding) -> RepoResult<()>;
    fn get_binding_by_task(&self, task_id: &str) -> RepoResult<Option<SessionBinding>>;
    fn get_binding_by_session(&self, session_id: &str) -> RepoResult<Option<SessionBinding>>;
    fn update_binding(&self, binding: &SessionBinding) -> RepoResult<()>;
    fn list_active_bindings(&self) -> RepoResult<Vec<SessionBinding>>;

    // ------------------------------------------------------------------
    // Worktrees
    // ------------------------------------------------------------------

    fn create_worktree(&self, wt: &WorktreeRecord) -> RepoResult<()>;
    fn get_worktree(&self, id: &str) -> RepoResult<WorktreeRecord>;
    fn list_worktrees_by_task(&self, task_id: &str) -> RepoResult<Vec<WorktreeRecord>>;
    fn update_worktree(&self, wt: &WorktreeRecord) -> RepoResult<()>;
    fn delete_worktree(&self, id: &str) -> RepoResult<()>;
    fn list_active_worktrees(&self) -> RepoResult<Vec<WorktreeRecord>>;

    // ------------------------------------------------------------------
    // Attachments
    // ------------------------------------------------------------------

    fn create_attachment(&self, att: &AttachmentRecord) -> RepoResult<()>;
    fn get_attachment(&self, id: &str) -> RepoResult<AttachmentRecord>;
    fn list_attachments_by_task(&self, task_id: &str) -> RepoResult<Vec<AttachmentRecord>>;

    // ------------------------------------------------------------------
    // Recovery Items
    // ------------------------------------------------------------------

    fn create_recovery_item(&self, item: &RecoveryItem) -> RepoResult<()>;
    fn get_recovery_item(&self, id: &str) -> RepoResult<RecoveryItem>;
    fn list_recovery_items(&self) -> RepoResult<Vec<RecoveryItem>>;
    fn update_recovery_item(&self, item: &RecoveryItem) -> RepoResult<()>;
    fn delete_recovery_item(&self, id: &str) -> RepoResult<()>;

    // ------------------------------------------------------------------
    // Settings
    // ------------------------------------------------------------------

    fn get_setting(&self, key: &str) -> RepoResult<Option<Settings>>;
    fn set_setting(&self, setting: &Settings) -> RepoResult<()>;
    fn list_settings(&self) -> RepoResult<Vec<Settings>>;

    // ------------------------------------------------------------------
    // Startup Recovery
    // ------------------------------------------------------------------

    /// Transition all tasks in a "live process implied" state
    /// (running, waiting_permission, integrating) to interrupted,
    /// recording the reason. Returns the count of affected tasks.
    fn recover_interrupted_tasks(&self, reason: &str) -> RepoResult<u32>;
}
