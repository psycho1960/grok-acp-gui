//! MOD-PERSISTENCE: Repository Interface.
//!
//! All data access crosses this trait. No caller ever receives a raw
//! `rusqlite::Connection` — only the domain types returned by these methods.

use crate::domain::error::DomainError;
use crate::domain::types::{
    AttachmentRecord, BootstrapSnapshot, CheckpointRecord, ConcurrencyLimits, IntegrationAttempt,
    Project, RecoveryCandidate, RecoveryDecision, RecoveryItem, SessionBinding, SessionSnapshot,
    Settings, StoredEvent, Task, TaskSummary, WorkspaceKind, WorktreeRecord,
};
use crate::modules::task_runtime::permission::{
    ApprovalEvidence, ExecutionContext, PermissionDecision, PermissionRecord,
};
use crate::modules::task_runtime::plan::{PlanDecision, PlanRecord};

/// Result alias used throughout the persistence layer.
pub type RepoResult<T> = Result<T, DomainError>;

/// The complete Repository Interface required by GAG-004 + GAG-006.
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
    /// Atomically update only user-configurable task fields. Status and
    /// recovery fields are intentionally excluded to avoid lost updates from
    /// concurrent runtime events.
    fn update_task_configuration(
        &self,
        id: &str,
        workspace_kind: WorkspaceKind,
        mode: Option<&str>,
        model: Option<&str>,
        reasoning: Option<&str>,
        updated_at: &str,
    ) -> RepoResult<()>;
    /// Transactionally update task status (used by startup recovery).
    fn update_task_status(&self, id: &str, status: &str, reason: Option<&str>) -> RepoResult<()>;
    /// Atomically marks a task running only when its persisted workspace is
    /// launchable. A Worktree task whose record is closing/archived/missing is
    /// rejected so cleanup and process start cannot cross in flight.
    fn begin_task_execution(&self, id: &str) -> RepoResult<()>;
    /// GAG-006: List tasks in states implying a live process.
    fn list_tasks_by_statuses(&self, statuses: &[&str]) -> RepoResult<Vec<Task>>;
    /// GAG-006: Get lightweight task summaries for task center.
    fn list_task_summaries(&self) -> RepoResult<Vec<TaskSummary>>;

    // ------------------------------------------------------------------
    // Session Bindings
    // ------------------------------------------------------------------

    fn create_binding(&self, binding: &SessionBinding) -> RepoResult<()>;
    fn get_binding_by_task(&self, task_id: &str) -> RepoResult<Option<SessionBinding>>;
    fn get_binding_by_session(&self, session_id: &str) -> RepoResult<Option<SessionBinding>>;
    fn update_binding(&self, binding: &SessionBinding) -> RepoResult<()>;
    fn list_active_bindings(&self) -> RepoResult<Vec<SessionBinding>>;
    /// GAG-006: Increment attempt_number for a session binding.
    fn increment_binding_attempt(&self, task_id: &str) -> RepoResult<u32>;

    // ------------------------------------------------------------------
    // GAG-006: Session Events
    // ------------------------------------------------------------------

    /// Append a session event with deduplication. Returns Ok(true) if
    /// inserted, Ok(false) if the dedup_key already existed (idempotent).
    fn append_event(&self, event: &StoredEvent) -> RepoResult<bool>;

    /// Get events for a session after a given sequence (cursor).
    fn get_events_after(
        &self,
        session_id: &str,
        after_seq: u64,
        limit: u32,
    ) -> RepoResult<Vec<StoredEvent>>;

    /// Get events for a specific attempt within a session.
    fn get_events_for_attempt(
        &self,
        session_id: &str,
        attempt_number: u32,
    ) -> RepoResult<Vec<StoredEvent>>;

    /// Get the highest sequence number for a session.
    fn get_max_sequence(&self, session_id: &str) -> RepoResult<Option<u64>>;

    /// GAG-006: Get session snapshot for Renderer reconnection.
    fn get_session_snapshot(
        &self,
        task_id: &str,
        session_id: &str,
        event_limit: u32,
    ) -> RepoResult<SessionSnapshot>;

    // ------------------------------------------------------------------
    // GAG-006: Recovery Candidates
    // ------------------------------------------------------------------

    /// List tasks that were interrupted and are candidates for recovery.
    fn list_recovery_candidates(&self) -> RepoResult<Vec<RecoveryCandidate>>;

    /// Apply a recovery decision (resume or archive).
    fn apply_recovery_decision(&self, decision: &RecoveryDecision) -> RepoResult<()>;

    /// Get current concurrency limits snapshot.
    fn get_concurrency_limits(&self, max_concurrent: u32) -> RepoResult<ConcurrencyLimits>;

    // ------------------------------------------------------------------
    // Worktrees
    // ------------------------------------------------------------------

    fn create_worktree(&self, wt: &WorktreeRecord) -> RepoResult<()>;
    fn get_worktree(&self, id: &str) -> RepoResult<WorktreeRecord>;
    fn list_worktrees_by_task(&self, task_id: &str) -> RepoResult<Vec<WorktreeRecord>>;
    fn update_worktree(&self, wt: &WorktreeRecord) -> RepoResult<()>;
    fn delete_worktree(&self, id: &str) -> RepoResult<()>;
    fn list_active_worktrees(&self) -> RepoResult<Vec<WorktreeRecord>>;
    /// Atomically moves one managed worktree to closing only when its task is
    /// not in a state that implies a live process.
    fn begin_worktree_removal(&self, task_id: &str, worktree_id: &str) -> RepoResult<()>;

    // ------------------------------------------------------------------
    // GAG-012 Checkpoints (append-only audit index)
    // ------------------------------------------------------------------

    fn create_checkpoint(&self, checkpoint: &CheckpointRecord) -> RepoResult<()>;
    fn list_checkpoints_by_task(&self, task_id: &str) -> RepoResult<Vec<CheckpointRecord>>;

    // ------------------------------------------------------------------
    // GAG-013 Squash integration attempts
    // ------------------------------------------------------------------

    fn create_integration_attempt(&self, attempt: &IntegrationAttempt) -> RepoResult<()>;
    fn get_integration_attempt(&self, id: &str) -> RepoResult<IntegrationAttempt>;
    fn get_active_integration_by_repo(
        &self,
        repo_identity: &str,
        repo_root: &str,
    ) -> RepoResult<Option<IntegrationAttempt>>;
    fn get_active_integration_by_task(
        &self,
        task_id: &str,
    ) -> RepoResult<Option<IntegrationAttempt>>;
    fn update_integration_attempt(
        &self,
        attempt: &IntegrationAttempt,
        detail_json: &str,
    ) -> RepoResult<()>;

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
    // GAG-009: Permission and Plan transactions
    // ------------------------------------------------------------------

    fn create_plan(&self, plan: &PlanRecord) -> RepoResult<()>;
    fn get_plan(&self, request_id: &str, session_id: &str) -> RepoResult<PlanRecord>;
    fn latest_plan_version(&self, task_id: &str) -> RepoResult<Option<u64>>;
    fn latest_plan(&self, task_id: &str) -> RepoResult<Option<PlanRecord>>;
    fn decide_plan(&self, decision: &PlanDecision) -> RepoResult<PlanRecord>;
    /// Reverses a delivered-local-but-not-delivered-to-ACP Plan decision.
    /// The caller uses this only when the ACP response write failed.
    fn revert_plan_decision(&self, request_id: &str, session_id: &str) -> RepoResult<PlanRecord>;
    /// Invalidates every still-proposed Plan for a session after cancellation
    /// or process loss so an old ACP request cannot be approved later.
    fn supersede_session_plans(&self, session_id: &str, reason: &str) -> RepoResult<u32>;

    fn create_permission(&self, permission: &PermissionRecord) -> RepoResult<()>;
    fn get_permission(&self, request_id: &str, session_id: &str) -> RepoResult<PermissionRecord>;
    fn decide_permission(&self, decision: &PermissionDecision) -> RepoResult<PermissionRecord>;
    /// Reverses a delivered-local-but-not-delivered-to-ACP permission decision.
    /// The caller uses this only when the ACP response write failed.
    fn revert_permission_decision(
        &self,
        request_id: &str,
        session_id: &str,
    ) -> RepoResult<PermissionRecord>;
    /// Atomically expires one still-pending request. Used by the backend
    /// timeout worker before it returns the ACP denial option.
    fn expire_permission(
        &self,
        request_id: &str,
        session_id: &str,
    ) -> RepoResult<Option<PermissionRecord>>;
    fn expire_session_permissions(&self, session_id: &str, reason: &str) -> RepoResult<u32>;
    fn consume_permission(
        &self,
        context: &ExecutionContext,
        operation_digest: &str,
        now_epoch_seconds: u64,
    ) -> RepoResult<Option<ApprovalEvidence>>;

    // ------------------------------------------------------------------
    // Startup Recovery
    // ------------------------------------------------------------------

    /// Transition all tasks in a "live process implied" state
    /// (running, waiting_permission, integrating) to interrupted,
    /// recording the reason. Returns the count of affected tasks.
    fn recover_interrupted_tasks(&self, reason: &str) -> RepoResult<u32>;
}
