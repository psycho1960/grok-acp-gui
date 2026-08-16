//! SQLite Adapter — implements the Repository Interface using rusqlite.
//!
//! All public methods map domain types to/from SQL rows. Internal errors
//! (lock timeout, corruption, etc.) are translated to `DomainError` with
//! stable codes. Callers never receive a raw `Connection`.
//!
//! # Transaction strategy
//! Multi-entity writes (e.g. creating a Task + Binding together) are
//! handled via explicit `transaction()` calls. Single-row writes use
//! implicit auto-commit. Busy timeout is set to 5 seconds.

pub mod migration;

use crate::domain::error::DomainError;
use crate::domain::types::{
    utc_now, AttachmentId, AttachmentRecord, BootstrapSnapshot, CheckpointRecord,
    ConcurrencyLimits, CorrelationId, IntegrationAttempt, IntegrationId, Project, ProjectId,
    RecoveryAction, RecoveryCandidate, RecoveryDecision, RecoveryId, RecoveryItem, RecoveryState,
    SessionBinding, SessionId, SessionSnapshot, SessionState, Settings, StoredEvent, Task, TaskId,
    TaskStatus, TaskSummary, TimelineCursor, WorkspaceKind, WorktreeId, WorktreeOwnership,
    WorktreeRecord, WorktreeState,
};
use crate::modules::persistence::{RepoResult, Repository};
use crate::modules::task_runtime::permission::{
    ApprovalEvidence, ExecutionContext, OperationCategory, PermissionDecision,
    PermissionOptionAction, PermissionRecord, PermissionState,
};
use crate::modules::task_runtime::plan::{PlanDecision, PlanOptionAction, PlanRecord, PlanState};
use crate::modules::task_runtime::recovery::{
    RecoveryActionKind, RecoveryActionPlan, RecoveryBundleRecord, RecoveryHistory, RecoveryIssue,
    RecoveryIssueKind, RecoveryIssueStatus, RecoveryScan, RecoverySeverity, RecoveryStepResult,
};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::Mutex;

const BUSY_TIMEOUT_MS: u32 = 5000;

/// The SQLite-backed Repository implementation.
pub struct SqliteRepository {
    conn: Mutex<Connection>,
}

impl SqliteRepository {
    /// Open (or create) the database at `path`, run migrations, and
    /// return a ready-to-use `SqliteRepository`.
    pub fn open(path: &Path) -> Result<Self, DomainError> {
        let mut conn =
            Connection::open(path).map_err(|e| db_error("Failed to open database", &e))?;

        conn.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS as u64))
            .map_err(|e| db_error("Failed to set busy timeout", &e))?;

        // Foreign-key enforcement is connection-local and does not mutate a
        // database that may belong to a newer application version.  Defer
        // the persistent WAL journal-mode change until the schema gate and
        // all pending migrations have succeeded.
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(|e| db_error("Failed to enable foreign keys", &e))?;

        migration::run_migrations_transactional(&mut conn)
            .map_err(|e| DomainError::new("DB_MIGRATION_FAILED", e.to_string()))?;

        conn.execute_batch("PRAGMA journal_mode=WAL;")
            .map_err(|e| db_error("Failed to enable WAL mode", &e))?;

        Ok(SqliteRepository {
            conn: Mutex::new(conn),
        })
    }

    /// Open an in-memory database (used as fallback when the on-disk
    /// database cannot be opened or for testing).
    pub fn open_in_memory() -> Result<Self, DomainError> {
        let mut conn = Connection::open_in_memory()
            .map_err(|e| db_error("Failed to open in-memory DB", &e))?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(|e| db_error("Failed to enable foreign keys", &e))?;
        migration::run_migrations_transactional(&mut conn)
            .map_err(|e| DomainError::new("DB_MIGRATION_FAILED", e.to_string()))?;
        Ok(SqliteRepository {
            conn: Mutex::new(conn),
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, DomainError> {
        self.conn
            .lock()
            .map_err(|e| DomainError::new("DB_QUERY_FAILED", format!("Lock poisoned: {}", e)))
    }
}

// ---------------------------------------------------------------------------
// Row mapping helpers
// ---------------------------------------------------------------------------

fn row_to_project(row: &rusqlite::Row) -> rusqlite::Result<Project> {
    Ok(Project {
        id: ProjectId::new(row.get::<_, String>(0)?),
        path: row.get(1)?,
        display_path: row.get(2)?,
        repo_root: row.get(3)?,
        trusted_at: row.get(4)?,
        last_opened_at: row.get(5)?,
    })
}

fn row_to_task(row: &rusqlite::Row) -> rusqlite::Result<Task> {
    Ok(Task {
        id: TaskId::new(row.get::<_, String>(0)?),
        project_id: ProjectId::new(row.get::<_, String>(1)?),
        title: row.get(2)?,
        status: parse_task_status(&row.get::<_, String>(3)?),
        workspace_kind: parse_workspace_kind(&row.get::<_, String>(4)?),
        mode: row.get(5)?,
        model: row.get(6)?,
        reasoning: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        interrupt_reason: row.get(10)?,
        interrupted_at: row.get(11)?,
        attempt_count: row.get::<_, i64>(12)? as u32,
    })
}

fn row_to_binding(row: &rusqlite::Row) -> rusqlite::Result<SessionBinding> {
    Ok(SessionBinding {
        task_id: TaskId::new(row.get::<_, String>(0)?),
        session_id: SessionId::new(row.get::<_, String>(1)?),
        cwd: row.get(2)?,
        last_seq: row.get(3)?,
        state: parse_session_state(&row.get::<_, String>(4)?),
        attempt_number: row.get::<_, i64>(5)? as u32,
    })
}

fn row_to_worktree(row: &rusqlite::Row) -> rusqlite::Result<WorktreeRecord> {
    Ok(WorktreeRecord {
        id: WorktreeId::new(row.get::<_, String>(0)?),
        task_id: TaskId::new(row.get::<_, String>(1)?),
        repo_root: row.get(2)?,
        path: row.get(3)?,
        display_path: row.get(4)?,
        branch: row.get(5)?,
        base_branch: row.get(6)?,
        base_commit: row.get(7)?,
        ownership: parse_worktree_ownership(&row.get::<_, String>(8)?),
        state: parse_worktree_state(&row.get::<_, String>(9)?),
        repo_identity: row.get(10)?,
        common_git_dir: row.get(11)?,
        relative_path: row.get(12)?,
        created_at: row.get(13)?,
        last_verified_at: row.get(14)?,
        recovery_bundle_id: row.get(15)?,
        disk_usage_bytes: row.get::<_, i64>(16)?.max(0) as u64,
        locked: row.get::<_, i64>(17)? != 0,
        merged: row.get::<_, i64>(18)? != 0,
    })
}

fn row_to_integration(row: &rusqlite::Row) -> rusqlite::Result<IntegrationAttempt> {
    let raw_state = row.get::<_, String>(14)?;
    let state = raw_state.parse().map_err(|message: String| {
        rusqlite::Error::FromSqlConversionFailure(
            14,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                message,
            )),
        )
    })?;
    Ok(IntegrationAttempt {
        id: IntegrationId::new(row.get::<_, String>(0)?),
        task_id: TaskId::new(row.get::<_, String>(1)?),
        repo_root: row.get(2)?,
        source_ref: row.get(3)?,
        source_tip_sha: row.get(4)?,
        source_range: row.get(5)?,
        source_dirty: row.get::<_, i64>(6)? != 0,
        source_worktree_digest: row.get(7)?,
        target_ref: row.get(8)?,
        expected_target_sha: row.get(9)?,
        commit_message: row.get(10)?,
        validation_commands_json: row.get(11)?,
        validation_digest: row.get(12)?,
        approval_digest: row.get(13)?,
        state,
        temporary_worktree_id: row.get(15)?,
        temporary_worktree_path: row.get(16)?,
        temporary_branch: row.get(17)?,
        conflict_summary_json: row.get(18)?,
        validation_result_json: row.get(19)?,
        result_commit_sha: row.get(20)?,
        recovery_bundle_path: row.get(21)?,
        cleanup_status: row.get(22)?,
        created_at: row.get(23)?,
        updated_at: row.get(24)?,
        repo_identity: row.get(25)?,
    })
}

fn row_to_attachment(row: &rusqlite::Row) -> rusqlite::Result<AttachmentRecord> {
    Ok(AttachmentRecord {
        id: AttachmentId::new(row.get::<_, String>(0)?),
        task_id: TaskId::new(row.get::<_, String>(1)?),
        sha256: row.get(2)?,
        mime: row.get(3)?,
        bytes: row.get(4)?,
        cache_path: row.get(5)?,
        source_name: row.get(6)?,
        created_at: row.get(7)?,
    })
}

fn row_to_recovery(row: &rusqlite::Row) -> rusqlite::Result<RecoveryItem> {
    Ok(RecoveryItem {
        id: RecoveryId::new(row.get::<_, String>(0)?),
        task_id: TaskId::new(row.get::<_, String>(1)?),
        directory: row.get(2)?,
        manifest_path: row.get(3)?,
        expires_at: row.get(4)?,
        state: parse_recovery_state(&row.get::<_, String>(5)?),
    })
}

fn recovery_row_error(index: usize, message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        index,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            message.into(),
        )),
    )
}

fn row_to_recovery_scan(row: &rusqlite::Row) -> rusqlite::Result<RecoveryScan> {
    Ok(RecoveryScan {
        id: row.get(0)?,
        trigger_kind: row.get(1)?,
        started_at: row.get(2)?,
        completed_at: row.get(3)?,
        issue_count: row.get::<_, i64>(4)?.max(0) as u32,
    })
}

fn row_to_recovery_issue(row: &rusqlite::Row) -> rusqlite::Result<RecoveryIssue> {
    let kind_raw: String = row.get(4)?;
    let severity_raw: String = row.get(5)?;
    let status_raw: String = row.get(6)?;
    let evidence_raw: String = row.get(10)?;
    let actions_raw: String = row.get(13)?;
    let kind = RecoveryIssueKind::parse(&kind_raw)
        .ok_or_else(|| recovery_row_error(4, format!("unknown recovery issue kind: {kind_raw}")))?;
    let severity = RecoverySeverity::parse(&severity_raw).ok_or_else(|| {
        recovery_row_error(5, format!("unknown recovery severity: {severity_raw}"))
    })?;
    let status = RecoveryIssueStatus::parse(&status_raw)
        .ok_or_else(|| recovery_row_error(6, format!("unknown recovery status: {status_raw}")))?;
    let evidence = serde_json::from_str(&evidence_raw)
        .map_err(|error| recovery_row_error(10, format!("invalid recovery evidence: {error}")))?;
    let action_names: Vec<String> = serde_json::from_str(&actions_raw)
        .map_err(|error| recovery_row_error(13, format!("invalid recovery actions: {error}")))?;
    let safe_actions = action_names
        .into_iter()
        .map(|name| {
            RecoveryActionKind::parse(&name)
                .ok_or_else(|| recovery_row_error(13, format!("unknown recovery action: {name}")))
        })
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(RecoveryIssue {
        issue_id: row.get(0)?,
        revision: row.get::<_, i64>(1)?.max(0) as u32,
        scan_id: row.get(2)?,
        stable_key: row.get(3)?,
        kind,
        severity,
        status,
        task_id: row.get::<_, Option<String>>(7)?.map(TaskId::new),
        resource_id: row.get(8)?,
        canonical_path: row.get(9)?,
        evidence,
        impact: row.get(11)?,
        recommended_action: row.get(12)?,
        safe_actions,
        detected_at: row.get(14)?,
    })
}

fn row_to_recovery_action_plan(row: &rusqlite::Row) -> rusqlite::Result<RecoveryActionPlan> {
    let action_raw: String = row.get(3)?;
    let action_kind = RecoveryActionKind::parse(&action_raw)
        .ok_or_else(|| recovery_row_error(3, format!("unknown recovery action: {action_raw}")))?;
    let expected_raw: String = row.get(6)?;
    let steps_raw: String = row.get(7)?;
    let context_raw: String = row.get(8)?;
    Ok(RecoveryActionPlan {
        id: row.get(0)?,
        issue_id: row.get(1)?,
        issue_revision: row.get::<_, i64>(2)?.max(0) as u32,
        action_kind,
        resource_identity: row.get(4)?,
        canonical_path: row.get(5)?,
        expected_state: serde_json::from_str(&expected_raw)
            .map_err(|error| recovery_row_error(6, format!("invalid expected state: {error}")))?,
        steps: serde_json::from_str(&steps_raw)
            .map_err(|error| recovery_row_error(7, format!("invalid recovery steps: {error}")))?,
        internal_context: serde_json::from_str(&context_raw)
            .map_err(|error| recovery_row_error(8, format!("invalid recovery context: {error}")))?,
        destructive_level: row.get(9)?,
        approval_digest: row.get(10)?,
        expires_at_epoch: row.get::<_, i64>(11)?.max(0) as u64,
        created_at: row.get(12)?,
    })
}

fn row_to_recovery_bundle_record(row: &rusqlite::Row) -> rusqlite::Result<RecoveryBundleRecord> {
    Ok(RecoveryBundleRecord {
        id: row.get(0)?,
        issue_id: row.get(1)?,
        issue_revision: row.get::<_, i64>(2)?.max(0) as u32,
        recovery_item_id: row.get(3)?,
        manifest_path: row.get(4)?,
        manifest_sha256: row.get(5)?,
        verified: row.get::<_, i64>(6)? != 0,
        created_at: row.get(7)?,
    })
}

fn row_to_recovery_step_result(row: &rusqlite::Row) -> rusqlite::Result<RecoveryStepResult> {
    Ok(RecoveryStepResult {
        id: row.get(0)?,
        plan_id: row.get(1)?,
        step_index: row.get::<_, i64>(2)?.max(0) as u32,
        step_name: row.get(3)?,
        status: row.get(4)?,
        detail_redacted: row.get(5)?,
        occurred_at: row.get(6)?,
    })
}

fn row_to_setting(row: &rusqlite::Row) -> rusqlite::Result<Settings> {
    let json_str: String = row.get(1)?;
    let json_value: serde_json::Value =
        serde_json::from_str(&json_str).unwrap_or(serde_json::Value::Null);
    Ok(Settings {
        key: row.get(0)?,
        json_value,
    })
}

fn permission_state(value: &str) -> rusqlite::Result<PermissionState> {
    match value {
        "requested" => Ok(PermissionState::Requested),
        "approved_once" => Ok(PermissionState::ApprovedOnce),
        "approved_scope" => Ok(PermissionState::ApprovedScope),
        "denied" => Ok(PermissionState::Denied),
        "expired" => Ok(PermissionState::Expired),
        "cancelled" => Ok(PermissionState::Cancelled),
        "consumed" => Ok(PermissionState::Consumed),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("unknown permission state '{other}'").into(),
        )),
    }
}

fn permission_state_str(value: PermissionState) -> &'static str {
    match value {
        PermissionState::Requested => "requested",
        PermissionState::ApprovedOnce => "approved_once",
        PermissionState::ApprovedScope => "approved_scope",
        PermissionState::Denied => "denied",
        PermissionState::Expired => "expired",
        PermissionState::Cancelled => "cancelled",
        PermissionState::Consumed => "consumed",
    }
}

fn operation_category(value: &str) -> rusqlite::Result<OperationCategory> {
    match value {
        "read_only" => Ok(OperationCategory::ReadOnly),
        "write" => Ok(OperationCategory::Write),
        "destructive" => Ok(OperationCategory::Destructive),
        "unknown" => Ok(OperationCategory::Unknown),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("unknown operation category '{other}'").into(),
        )),
    }
}

fn operation_category_str(value: OperationCategory) -> &'static str {
    match value {
        OperationCategory::ReadOnly => "read_only",
        OperationCategory::Write => "write",
        OperationCategory::Destructive => "destructive",
        OperationCategory::Unknown => "unknown",
    }
}

fn plan_state(value: &str) -> rusqlite::Result<PlanState> {
    match value {
        "draft" => Ok(PlanState::Draft),
        "proposed" => Ok(PlanState::Proposed),
        "approved" => Ok(PlanState::Approved),
        "rejected" => Ok(PlanState::Rejected),
        "revision_requested" => Ok(PlanState::RevisionRequested),
        "superseded" => Ok(PlanState::Superseded),
        "executing" => Ok(PlanState::Executing),
        "completed" => Ok(PlanState::Completed),
        "failed" => Ok(PlanState::Failed),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("unknown plan state '{other}'").into(),
        )),
    }
}

fn plan_state_str(value: PlanState) -> &'static str {
    match value {
        PlanState::Draft => "draft",
        PlanState::Proposed => "proposed",
        PlanState::Approved => "approved",
        PlanState::Rejected => "rejected",
        PlanState::RevisionRequested => "revision_requested",
        PlanState::Superseded => "superseded",
        PlanState::Executing => "executing",
        PlanState::Completed => "completed",
        PlanState::Failed => "failed",
    }
}

fn row_to_permission(row: &rusqlite::Row) -> rusqlite::Result<PermissionRecord> {
    let options_json: String = row.get(9)?;
    Ok(PermissionRecord {
        request_id: row.get(0)?,
        task_id: TaskId::new(row.get::<_, String>(1)?),
        session_id: SessionId::new(row.get::<_, String>(2)?),
        correlation_id: row.get(3)?,
        workspace: row.get(4)?,
        plan_version: row.get::<_, Option<i64>>(5)?.map(|value| value as u64),
        operation_digest: row.get(6)?,
        category: operation_category(&row.get::<_, String>(7)?)?,
        summary_redacted: row.get(8)?,
        options: serde_json::from_str(&options_json).unwrap_or_default(),
        state: permission_state(&row.get::<_, String>(10)?)?,
        expires_at_epoch_seconds: row.get::<_, i64>(11)? as u64,
        decided_option_id: row.get(12)?,
        consumed_at: row.get(13)?,
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
    })
}

fn row_to_plan(row: &rusqlite::Row) -> rusqlite::Result<PlanRecord> {
    let options_json: String = row.get(9)?;
    Ok(PlanRecord {
        request_id: row.get(0)?,
        task_id: TaskId::new(row.get::<_, String>(1)?),
        session_id: SessionId::new(row.get::<_, String>(2)?),
        correlation_id: row.get(3)?,
        workspace: row.get(4)?,
        version: row.get::<_, i64>(5)? as u64,
        plan_hash: row.get(6)?,
        state: plan_state(&row.get::<_, String>(7)?)?,
        summary_redacted: row.get(8)?,
        options: serde_json::from_str(&options_json).unwrap_or_default(),
        decided_option_id: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

// --- GAG-006 row mappers ---

fn row_to_stored_event(row: &rusqlite::Row) -> rusqlite::Result<StoredEvent> {
    let payload_str: String = row.get(5)?;
    let payload: serde_json::Value =
        serde_json::from_str(&payload_str).unwrap_or(serde_json::Value::Null);
    Ok(StoredEvent {
        dedup_key: row.get(0)?,
        session_id: SessionId::new(row.get::<_, String>(1)?),
        task_id: TaskId::new(row.get::<_, String>(2)?),
        sequence: row.get::<_, i64>(3)? as u64,
        event_type: row.get(4)?,
        payload,
        correlation_id: row.get::<_, Option<String>>(6)?.map(CorrelationId::new),
        persisted_at: row.get(7)?,
        has_side_effects: row.get::<_, i64>(8)? != 0,
    })
}

fn row_to_task_summary(row: &rusqlite::Row) -> rusqlite::Result<TaskSummary> {
    Ok(TaskSummary {
        id: TaskId::new(row.get::<_, String>(0)?),
        project_id: ProjectId::new(row.get::<_, String>(1)?),
        title: row.get(2)?,
        status: parse_task_status(&row.get::<_, String>(3)?),
        updated_at: row.get(4)?,
        queue_position: None,    // computed by TaskRuntime
        has_live_session: false, // computed by TaskRuntime
    })
}

/// Query row for recovery candidates — reads from tasks table.
fn row_to_recovery_candidate(row: &rusqlite::Row) -> rusqlite::Result<RecoveryCandidate> {
    let has_session: i64 = row.get::<_, i64>(5)?;
    let attempt_count: i64 = row.get::<_, i64>(7)?;
    Ok(RecoveryCandidate {
        task_id: TaskId::new(row.get::<_, String>(0)?),
        title: row.get(1)?,
        previous_status: parse_task_status(&row.get::<_, String>(2)?),
        interrupted_at: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
        interrupt_reason: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
        has_session: has_session != 0,
        events_available: false, // computed by TaskRuntime
        attempt_count: attempt_count as u32,
    })
}

// ---------------------------------------------------------------------------
// Enum parsers
// ---------------------------------------------------------------------------

fn parse_task_status(s: &str) -> TaskStatus {
    match s {
        "draft" => TaskStatus::Draft,
        "preparing" => TaskStatus::Preparing,
        "running" => TaskStatus::Running,
        "waiting_permission" => TaskStatus::WaitingPermission,
        "idle" => TaskStatus::Idle,
        "failed" => TaskStatus::Failed,
        "ready_for_review" => TaskStatus::ReadyForReview,
        "integrating" => TaskStatus::Integrating,
        "conflicted" => TaskStatus::Conflicted,
        "merged" => TaskStatus::Merged,
        "archived" => TaskStatus::Archived,
        "interrupted" => TaskStatus::Interrupted,
        _ => TaskStatus::Interrupted,
    }
}

fn task_status_to_str(s: TaskStatus) -> &'static str {
    match s {
        TaskStatus::Draft => "draft",
        TaskStatus::Preparing => "preparing",
        TaskStatus::Running => "running",
        TaskStatus::WaitingPermission => "waiting_permission",
        TaskStatus::Idle => "idle",
        TaskStatus::Failed => "failed",
        TaskStatus::ReadyForReview => "ready_for_review",
        TaskStatus::Integrating => "integrating",
        TaskStatus::Conflicted => "conflicted",
        TaskStatus::Merged => "merged",
        TaskStatus::Archived => "archived",
        TaskStatus::Interrupted => "interrupted",
    }
}

fn parse_workspace_kind(s: &str) -> WorkspaceKind {
    match s {
        "readonly" => WorkspaceKind::Readonly,
        "direct" => WorkspaceKind::Direct,
        _ => WorkspaceKind::Worktree,
    }
}

fn workspace_kind_to_str(k: WorkspaceKind) -> &'static str {
    match k {
        WorkspaceKind::Worktree => "worktree",
        WorkspaceKind::Readonly => "readonly",
        WorkspaceKind::Direct => "direct",
    }
}

fn parse_session_state(s: &str) -> SessionState {
    match s {
        "active" => SessionState::Active,
        "idle" => SessionState::Idle,
        "disconnected" => SessionState::Disconnected,
        "closed" => SessionState::Closed,
        _ => SessionState::Idle,
    }
}

fn session_state_to_str(s: SessionState) -> &'static str {
    match s {
        SessionState::Active => "active",
        SessionState::Idle => "idle",
        SessionState::Disconnected => "disconnected",
        SessionState::Closed => "closed",
    }
}

fn parse_worktree_state(s: &str) -> WorktreeState {
    match s {
        "allocating" => WorktreeState::Allocating,
        "ready" => WorktreeState::Ready,
        "active" => WorktreeState::Active,
        "closing" => WorktreeState::Closing,
        "archived" => WorktreeState::Archived,
        "removed" => WorktreeState::Removed,
        "creation_failed" => WorktreeState::CreationFailed,
        "missing" => WorktreeState::Missing,
        "dirty" => WorktreeState::Dirty,
        "orphaned" => WorktreeState::Orphaned,
        "quarantined" => WorktreeState::Quarantined,
        "integrating" => WorktreeState::Integrating,
        "deleted" => WorktreeState::Deleted,
        _ => WorktreeState::Unknown,
    }
}

fn worktree_state_to_str(s: WorktreeState) -> &'static str {
    match s {
        WorktreeState::Allocating => "allocating",
        WorktreeState::Ready => "ready",
        WorktreeState::Active => "active",
        WorktreeState::Closing => "closing",
        WorktreeState::Archived => "archived",
        WorktreeState::Removed => "removed",
        WorktreeState::CreationFailed => "creation_failed",
        WorktreeState::Missing => "missing",
        WorktreeState::Dirty => "dirty",
        WorktreeState::Orphaned => "orphaned",
        WorktreeState::Quarantined => "quarantined",
        WorktreeState::Integrating => "integrating",
        WorktreeState::Deleted => "deleted",
        WorktreeState::Unknown => "unknown",
    }
}

fn parse_worktree_ownership(s: &str) -> WorktreeOwnership {
    match s {
        "external" => WorktreeOwnership::External,
        "adopted" => WorktreeOwnership::Adopted,
        _ => WorktreeOwnership::Managed,
    }
}

fn worktree_ownership_to_str(o: WorktreeOwnership) -> &'static str {
    match o {
        WorktreeOwnership::Managed => "managed",
        WorktreeOwnership::External => "external",
        WorktreeOwnership::Adopted => "adopted",
    }
}

fn parse_recovery_state(s: &str) -> RecoveryState {
    match s {
        "available" => RecoveryState::Available,
        "expired" => RecoveryState::Expired,
        "restoring" => RecoveryState::Restoring,
        "restored" => RecoveryState::Restored,
        "deleted" => RecoveryState::Deleted,
        _ => RecoveryState::Available,
    }
}

fn recovery_state_to_str(s: RecoveryState) -> &'static str {
    match s {
        RecoveryState::Available => "available",
        RecoveryState::Expired => "expired",
        RecoveryState::Restoring => "restoring",
        RecoveryState::Restored => "restored",
        RecoveryState::Deleted => "deleted",
    }
}

// ---------------------------------------------------------------------------
// Error mapping
// ---------------------------------------------------------------------------

fn db_error(context: &str, e: &rusqlite::Error) -> DomainError {
    let code = match e {
        rusqlite::Error::SqliteFailure(err, _) => match err.code {
            rusqlite::ErrorCode::DatabaseBusy => "DB_QUERY_FAILED",
            rusqlite::ErrorCode::DatabaseLocked => "DB_QUERY_FAILED",
            _ => "DB_QUERY_FAILED",
        },
        _ => "DB_QUERY_FAILED",
    };
    DomainError::new(code, format!("{}: {}", context, e))
}

/// Collect a `query_map` row iterator into a `Vec<T>`, surfacing the first
/// row-mapping error as `DB_QUERY_FAILED` instead of silently dropping it.
///
/// The previous `rows.filter_map(|r| r.ok()).collect()` pattern hid
/// corrupted rows (e.g. `InvalidColumnType` on a BLOB stored in a TEXT
/// column) by treating them as "record does not exist". That masqueraded
/// database corruption as data deletion and caused `bootstrap_snapshot()`
/// to return `Ok` with missing entities. This helper restores fail-loud
/// behaviour: any row that cannot be decoded aborts the read.
fn collect_rows<T, I>(iter: I, context: &str) -> RepoResult<Vec<T>>
where
    I: Iterator<Item = Result<T, rusqlite::Error>>,
{
    let mut out = Vec::new();
    for item in iter {
        match item {
            Ok(v) => out.push(v),
            Err(e) => return Err(db_error(context, &e)),
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Repository implementation
// ---------------------------------------------------------------------------

impl Repository for SqliteRepository {
    // ==================================================================
    // Bootstrap
    // ==================================================================

    fn bootstrap_snapshot(&self) -> RepoResult<BootstrapSnapshot> {
        let conn = self.lock()?;

        let projects = {
            let mut stmt = conn
                .prepare("SELECT id, path, display_path, repo_root, trusted_at, last_opened_at FROM projects ORDER BY last_opened_at DESC")
                .map_err(|e| db_error("bootstrap: projects query", &e))?;
            let rows = stmt
                .query_map([], row_to_project)
                .map_err(|e| db_error("bootstrap: projects map", &e))?;
            collect_rows(rows, "bootstrap: projects row")?
        };

        let active_tasks = {
            let mut stmt = conn
                .prepare(
                    "SELECT id, project_id, title, status, workspace_kind, mode, model, reasoning, created_at, updated_at, interrupt_reason, interrupted_at, attempt_count FROM tasks WHERE status NOT IN ('merged', 'archived') ORDER BY updated_at DESC",
                )
                .map_err(|e| db_error("bootstrap: tasks query", &e))?;
            let rows = stmt
                .query_map([], row_to_task)
                .map_err(|e| db_error("bootstrap: tasks map", &e))?;
            collect_rows(rows, "bootstrap: tasks row")?
        };

        let completed_tasks = {
            let mut stmt = conn
                .prepare(
                    "SELECT id, project_id, title, status, workspace_kind, mode, model, reasoning, created_at, updated_at, interrupt_reason, interrupted_at, attempt_count FROM tasks WHERE status IN ('merged', 'archived') ORDER BY updated_at DESC",
                )
                .map_err(|e| db_error("bootstrap: completed tasks query", &e))?;
            let rows = stmt
                .query_map([], row_to_task)
                .map_err(|e| db_error("bootstrap: completed tasks map", &e))?;
            collect_rows(rows, "bootstrap: completed tasks row")?
        };

        let bindings = {
            let mut stmt = conn
                .prepare("SELECT task_id, session_id, cwd, last_seq, state, attempt_number FROM session_bindings")
                .map_err(|e| db_error("bootstrap: bindings query", &e))?;
            let rows = stmt
                .query_map([], row_to_binding)
                .map_err(|e| db_error("bootstrap: bindings map", &e))?;
            collect_rows(rows, "bootstrap: bindings row")?
        };

        let worktrees = {
            let mut stmt = conn
                .prepare(
                    "SELECT id, task_id, repo_root, path, display_path, branch, base_branch, base_commit, ownership, state, repo_identity, common_git_dir, relative_path, created_at, last_verified_at, recovery_bundle_id, disk_usage_bytes, locked, merged FROM worktrees WHERE state NOT IN ('deleted', 'removed')",
                )
                .map_err(|e| db_error("bootstrap: worktrees query", &e))?;
            let rows = stmt
                .query_map([], row_to_worktree)
                .map_err(|e| db_error("bootstrap: worktrees map", &e))?;
            collect_rows(rows, "bootstrap: worktrees row")?
        };

        let recovery_items = {
            let mut stmt = conn
                .prepare(
                    "SELECT id, task_id, directory, manifest_path, expires_at, state FROM recovery_items WHERE state NOT IN ('deleted', 'restored')",
                )
                .map_err(|e| db_error("bootstrap: recovery query", &e))?;
            let rows = stmt
                .query_map([], row_to_recovery)
                .map_err(|e| db_error("bootstrap: recovery map", &e))?;
            collect_rows(rows, "bootstrap: recovery row")?
        };

        let settings = {
            let mut stmt = conn
                .prepare("SELECT key, json_value FROM settings")
                .map_err(|e| db_error("bootstrap: settings query", &e))?;
            let rows = stmt
                .query_map([], row_to_setting)
                .map_err(|e| db_error("bootstrap: settings map", &e))?;
            collect_rows(rows, "bootstrap: settings row")?
        };

        // Recovery performed elsewhere (explicit call to recover_interrupted_tasks).
        Ok(BootstrapSnapshot {
            product_name: "Grok ACP GUI".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            platform: std::env::consts::OS.into(),
            projects,
            active_tasks,
            completed_tasks,
            bindings,
            worktrees,
            recovery_items,
            settings,
            recovery_performed: false,
            tasks_interrupted: 0,
            recovery_candidates: vec![],
            concurrency: None,
        })
    }

    // ==================================================================
    // Projects
    // ==================================================================

    fn create_project(&self, project: &Project) -> RepoResult<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO projects (id, path, display_path, repo_root, trusted_at, last_opened_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                project.id.0,
                project.path,
                project.display_path,
                project.repo_root,
                project.trusted_at,
                project.last_opened_at,
            ],
        )
        .map_err(|e| db_error("create_project", &e))?;
        Ok(())
    }

    fn get_project(&self, id: &str) -> RepoResult<Project> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT id, path, display_path, repo_root, trusted_at, last_opened_at FROM projects WHERE id = ?1",
            params![id],
            row_to_project,
        )
        .map_err(|e| db_error("get_project", &e))
    }

    fn list_projects(&self) -> RepoResult<Vec<Project>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT id, path, display_path, repo_root, trusted_at, last_opened_at FROM projects ORDER BY last_opened_at DESC")
            .map_err(|e| db_error("list_projects", &e))?;
        let rows = stmt
            .query_map([], row_to_project)
            .map_err(|e| db_error("list_projects", &e))?;
        collect_rows(rows, "list_projects row")
    }

    fn update_project(&self, project: &Project) -> RepoResult<()> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE projects SET path = ?1, display_path = ?2, repo_root = ?3, trusted_at = ?4, last_opened_at = ?5 WHERE id = ?6",
            params![
                project.path,
                project.display_path,
                project.repo_root,
                project.trusted_at,
                project.last_opened_at,
                project.id.0,
            ],
        )
        .map_err(|e| db_error("update_project", &e))?;
        Ok(())
    }

    fn delete_project(&self, id: &str) -> RepoResult<()> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM projects WHERE id = ?1", params![id])
            .map_err(|e| db_error("delete_project", &e))?;
        Ok(())
    }

    // ==================================================================
    // Tasks
    // ==================================================================

    fn create_task(&self, task: &Task) -> RepoResult<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO tasks (id, project_id, title, status, workspace_kind, mode, model, reasoning, created_at, updated_at, interrupt_reason, interrupted_at, attempt_count) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                task.id.0,
                task.project_id.0,
                task.title,
                task_status_to_str(task.status),
                workspace_kind_to_str(task.workspace_kind),
                task.mode,
                task.model,
                task.reasoning,
                task.created_at,
                task.updated_at,
                task.interrupt_reason,
                task.interrupted_at,
                task.attempt_count as i64,
            ],
        )
        .map_err(|e| db_error("create_task", &e))?;
        Ok(())
    }

    fn get_task(&self, id: &str) -> RepoResult<Task> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT id, project_id, title, status, workspace_kind, mode, model, reasoning, created_at, updated_at, interrupt_reason, interrupted_at, attempt_count FROM tasks WHERE id = ?1",
            params![id],
            row_to_task,
        )
        .map_err(|e| db_error("get_task", &e))
    }

    fn list_tasks_by_project(&self, project_id: &str) -> RepoResult<Vec<Task>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, title, status, workspace_kind, mode, model, reasoning, created_at, updated_at, interrupt_reason, interrupted_at, attempt_count FROM tasks WHERE project_id = ?1 ORDER BY updated_at DESC",
            )
            .map_err(|e| db_error("list_tasks_by_project", &e))?;
        let rows = stmt
            .query_map(params![project_id], row_to_task)
            .map_err(|e| db_error("list_tasks_by_project", &e))?;
        collect_rows(rows, "list_tasks_by_project row")
    }

    fn list_active_tasks(&self) -> RepoResult<Vec<Task>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, title, status, workspace_kind, mode, model, reasoning, created_at, updated_at, interrupt_reason, interrupted_at, attempt_count FROM tasks WHERE status NOT IN ('merged', 'archived') ORDER BY updated_at DESC",
            )
            .map_err(|e| db_error("list_active_tasks", &e))?;
        let rows = stmt
            .query_map([], row_to_task)
            .map_err(|e| db_error("list_active_tasks", &e))?;
        collect_rows(rows, "list_active_tasks row")
    }

    fn update_task(&self, task: &Task) -> RepoResult<()> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE tasks SET project_id = ?1, title = ?2, status = ?3, workspace_kind = ?4, mode = ?5, model = ?6, reasoning = ?7, updated_at = ?8, interrupt_reason = ?9, interrupted_at = ?10, attempt_count = ?11 WHERE id = ?12",
            params![
                task.project_id.0,
                task.title,
                task_status_to_str(task.status),
                workspace_kind_to_str(task.workspace_kind),
                task.mode,
                task.model,
                task.reasoning,
                task.updated_at,
                task.interrupt_reason,
                task.interrupted_at,
                task.attempt_count as i64,
                task.id.0,
            ],
        )
        .map_err(|e| db_error("update_task", &e))?;
        Ok(())
    }

    fn update_task_configuration(
        &self,
        id: &str,
        workspace_kind: WorkspaceKind,
        mode: Option<&str>,
        model: Option<&str>,
        reasoning: Option<&str>,
        updated_at: &str,
    ) -> RepoResult<()> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE tasks SET workspace_kind = ?1, mode = ?2, model = ?3, reasoning = ?4, updated_at = ?5 WHERE id = ?6",
            params![
                workspace_kind_to_str(workspace_kind),
                mode,
                model,
                reasoning,
                updated_at,
                id,
            ],
        )
        .map_err(|e| db_error("update_task_configuration", &e))?;
        Ok(())
    }

    fn update_task_status(&self, id: &str, status: &str, reason: Option<&str>) -> RepoResult<()> {
        let conn = self.lock()?;
        let now = utc_now();
        conn.execute(
            "UPDATE tasks SET status = ?1, interrupt_reason = ?2, updated_at = ?3 WHERE id = ?4",
            params![status, reason, now, id],
        )
        .map_err(|e| db_error("update_task_status", &e))?;
        Ok(())
    }

    fn begin_task_execution(&self, id: &str) -> RepoResult<()> {
        let mut conn = self.lock()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| db_error("begin_task_execution transaction", &e))?;
        let workspace_kind: String = tx
            .query_row(
                "SELECT workspace_kind FROM tasks WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .map_err(|e| db_error("begin_task_execution task", &e))?;
        if workspace_kind == "worktree" {
            let registered: i64 = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM worktrees WHERE task_id = ?1 AND state NOT IN ('deleted', 'removed'))",
                    params![id],
                    |row| row.get(0),
                )
                .map_err(|e| db_error("begin_task_execution registration", &e))?;
            let launchable: i64 = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM worktrees WHERE task_id = ?1 AND ownership = 'managed' AND state IN ('ready', 'dirty', 'active'))",
                    params![id],
                    |row| row.get(0),
                )
                .map_err(|e| db_error("begin_task_execution worktree", &e))?;
            if registered != 0 && launchable == 0 {
                return Err(DomainError::new(
                    crate::domain::error::codes::WORKTREE_NOT_READY,
                    "managed worktree is not launchable",
                ));
            }
        }
        let now = utc_now();
        tx.execute(
            "UPDATE tasks SET status = 'running', interrupt_reason = NULL, updated_at = ?1 WHERE id = ?2",
            params![now, id],
        )
        .map_err(|e| db_error("begin_task_execution update", &e))?;
        tx.commit()
            .map_err(|e| db_error("begin_task_execution commit", &e))?;
        Ok(())
    }

    // ==================================================================
    // Session Bindings
    // ==================================================================

    fn create_binding(&self, binding: &SessionBinding) -> RepoResult<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO session_bindings (task_id, session_id, cwd, last_seq, state, attempt_number) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                binding.task_id.0,
                binding.session_id.0,
                binding.cwd,
                binding.last_seq,
                session_state_to_str(binding.state),
                binding.attempt_number as i64,
            ],
        )
        .map_err(|e| db_error("create_binding", &e))?;
        Ok(())
    }

    fn get_binding_by_task(&self, task_id: &str) -> RepoResult<Option<SessionBinding>> {
        let conn = self.lock()?;
        let result = conn.query_row(
            "SELECT task_id, session_id, cwd, last_seq, state, attempt_number FROM session_bindings WHERE task_id = ?1",
            params![task_id],
            row_to_binding,
        );
        match result {
            Ok(b) => Ok(Some(b)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(db_error("get_binding_by_task", &e)),
        }
    }

    fn get_binding_by_session(&self, session_id: &str) -> RepoResult<Option<SessionBinding>> {
        let conn = self.lock()?;
        let result = conn.query_row(
            "SELECT task_id, session_id, cwd, last_seq, state, attempt_number FROM session_bindings WHERE session_id = ?1",
            params![session_id],
            row_to_binding,
        );
        match result {
            Ok(b) => Ok(Some(b)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(db_error("get_binding_by_session", &e)),
        }
    }

    fn update_binding(&self, binding: &SessionBinding) -> RepoResult<()> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE session_bindings SET cwd = ?1, last_seq = ?2, state = ?3, attempt_number = ?4 WHERE task_id = ?5",
            params![
                binding.cwd,
                binding.last_seq,
                session_state_to_str(binding.state),
                binding.attempt_number as i64,
                binding.task_id.0,
            ],
        )
        .map_err(|e| db_error("update_binding", &e))?;
        Ok(())
    }

    fn list_active_bindings(&self) -> RepoResult<Vec<SessionBinding>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT task_id, session_id, cwd, last_seq, state, attempt_number FROM session_bindings WHERE state != 'closed'",
            )
            .map_err(|e| db_error("list_active_bindings", &e))?;
        let rows = stmt
            .query_map([], row_to_binding)
            .map_err(|e| db_error("list_active_bindings", &e))?;
        collect_rows(rows, "list_active_bindings row")
    }

    // ==================================================================
    // Worktrees
    // ==================================================================

    fn create_worktree(&self, wt: &WorktreeRecord) -> RepoResult<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO worktrees (id, task_id, repo_root, path, display_path, branch, base_branch, base_commit, ownership, state, repo_identity, common_git_dir, relative_path, created_at, last_verified_at, recovery_bundle_id, disk_usage_bytes, locked, merged) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
            params![
                wt.id.0,
                wt.task_id.0,
                wt.repo_root,
                wt.path,
                wt.display_path,
                wt.branch,
                wt.base_branch,
                wt.base_commit,
                worktree_ownership_to_str(wt.ownership),
                worktree_state_to_str(wt.state),
                wt.repo_identity,
                wt.common_git_dir,
                wt.relative_path,
                wt.created_at,
                wt.last_verified_at,
                wt.recovery_bundle_id,
                wt.disk_usage_bytes as i64,
                wt.locked as i64,
                wt.merged as i64,
            ],
        )
        .map_err(|e| db_error("create_worktree", &e))?;
        Ok(())
    }

    fn get_worktree(&self, id: &str) -> RepoResult<WorktreeRecord> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT id, task_id, repo_root, path, display_path, branch, base_branch, base_commit, ownership, state, repo_identity, common_git_dir, relative_path, created_at, last_verified_at, recovery_bundle_id, disk_usage_bytes, locked, merged FROM worktrees WHERE id = ?1",
            params![id],
            row_to_worktree,
        )
        .map_err(|e| db_error("get_worktree", &e))
    }

    fn list_worktrees_by_task(&self, task_id: &str) -> RepoResult<Vec<WorktreeRecord>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, task_id, repo_root, path, display_path, branch, base_branch, base_commit, ownership, state, repo_identity, common_git_dir, relative_path, created_at, last_verified_at, recovery_bundle_id, disk_usage_bytes, locked, merged FROM worktrees WHERE task_id = ?1",
            )
            .map_err(|e| db_error("list_worktrees_by_task", &e))?;
        let rows = stmt
            .query_map(params![task_id], row_to_worktree)
            .map_err(|e| db_error("list_worktrees_by_task", &e))?;
        collect_rows(rows, "list_worktrees_by_task row")
    }

    fn update_worktree(&self, wt: &WorktreeRecord) -> RepoResult<()> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE worktrees SET task_id = ?1, repo_root = ?2, path = ?3, display_path = ?4, branch = ?5, base_branch = ?6, base_commit = ?7, ownership = ?8, state = ?9, repo_identity = ?10, common_git_dir = ?11, relative_path = ?12, created_at = ?13, last_verified_at = ?14, recovery_bundle_id = ?15, disk_usage_bytes = ?16, locked = ?17, merged = ?18 WHERE id = ?19",
            params![
                wt.task_id.0,
                wt.repo_root,
                wt.path,
                wt.display_path,
                wt.branch,
                wt.base_branch,
                wt.base_commit,
                worktree_ownership_to_str(wt.ownership),
                worktree_state_to_str(wt.state),
                wt.repo_identity,
                wt.common_git_dir,
                wt.relative_path,
                wt.created_at,
                wt.last_verified_at,
                wt.recovery_bundle_id,
                wt.disk_usage_bytes as i64,
                wt.locked as i64,
                wt.merged as i64,
                wt.id.0,
            ],
        )
        .map_err(|e| db_error("update_worktree", &e))?;
        Ok(())
    }

    fn delete_worktree(&self, id: &str) -> RepoResult<()> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM worktrees WHERE id = ?1", params![id])
            .map_err(|e| db_error("delete_worktree", &e))?;
        Ok(())
    }

    fn list_active_worktrees(&self) -> RepoResult<Vec<WorktreeRecord>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, task_id, repo_root, path, display_path, branch, base_branch, base_commit, ownership, state, repo_identity, common_git_dir, relative_path, created_at, last_verified_at, recovery_bundle_id, disk_usage_bytes, locked, merged FROM worktrees WHERE state NOT IN ('deleted', 'removed')",
            )
            .map_err(|e| db_error("list_active_worktrees", &e))?;
        let rows = stmt
            .query_map([], row_to_worktree)
            .map_err(|e| db_error("list_active_worktrees", &e))?;
        collect_rows(rows, "list_active_worktrees row")
    }

    fn begin_worktree_removal(&self, task_id: &str, worktree_id: &str) -> RepoResult<()> {
        let mut conn = self.lock()?;
        let tx = conn
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(|e| db_error("begin_worktree_removal transaction", &e))?;
        let status: String = tx
            .query_row(
                "SELECT status FROM tasks WHERE id = ?1",
                params![task_id],
                |row| row.get(0),
            )
            .map_err(|e| db_error("begin_worktree_removal task", &e))?;
        if matches!(
            status.as_str(),
            "running" | "waiting_permission" | "integrating"
        ) {
            return Err(DomainError::new(
                crate::domain::error::codes::WORKTREE_TASK_RUNNING,
                "running task prevents worktree cleanup",
            ));
        }
        let changed = tx
            .execute(
                "UPDATE worktrees SET state = 'closing', last_verified_at = ?1 WHERE id = ?2 AND task_id = ?3 AND ownership = 'managed' AND state IN ('ready', 'dirty', 'active', 'archived')",
                params![utc_now(), worktree_id, task_id],
            )
            .map_err(|e| db_error("begin_worktree_removal update", &e))?;
        if changed != 1 {
            return Err(DomainError::new(
                crate::domain::error::codes::WORKTREE_NOT_READY,
                "managed worktree cannot enter closing state",
            ));
        }
        tx.commit()
            .map_err(|e| db_error("begin_worktree_removal commit", &e))?;
        Ok(())
    }

    // ==================================================================
    // GAG-012 Checkpoints
    // ==================================================================

    fn create_checkpoint(&self, checkpoint: &CheckpointRecord) -> RepoResult<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO checkpoints (id, task_id, attempt_number, commit_sha, tree_sha, head_before, selection_manifest, selection_hash, message, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                checkpoint.id,
                checkpoint.task_id.0,
                checkpoint.attempt_number as i64,
                checkpoint.commit_sha,
                checkpoint.tree_sha,
                checkpoint.head_before,
                checkpoint.selection_manifest,
                checkpoint.selection_hash,
                checkpoint.message,
                checkpoint.created_at,
            ],
        )
        .map_err(|e| db_error("create_checkpoint", &e))?;
        Ok(())
    }

    fn list_checkpoints_by_task(&self, task_id: &str) -> RepoResult<Vec<CheckpointRecord>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT id, task_id, attempt_number, commit_sha, tree_sha, head_before, selection_manifest, selection_hash, message, created_at FROM checkpoints WHERE task_id = ?1 ORDER BY created_at DESC, id DESC")
            .map_err(|e| db_error("list_checkpoints_by_task", &e))?;
        let rows = stmt
            .query_map(params![task_id], |row| {
                Ok(CheckpointRecord {
                    id: row.get(0)?,
                    task_id: TaskId::new(row.get::<_, String>(1)?),
                    attempt_number: row.get::<_, i64>(2)? as u32,
                    commit_sha: row.get(3)?,
                    tree_sha: row.get(4)?,
                    head_before: row.get(5)?,
                    selection_manifest: row.get(6)?,
                    selection_hash: row.get(7)?,
                    message: row.get(8)?,
                    created_at: row.get(9)?,
                })
            })
            .map_err(|e| db_error("list_checkpoints_by_task", &e))?;
        collect_rows(rows, "list_checkpoints_by_task row")
    }

    // ==================================================================
    // GAG-013 Squash integrations
    // ==================================================================

    fn create_integration_attempt(&self, attempt: &IntegrationAttempt) -> RepoResult<()> {
        let mut conn = self.lock()?;
        let tx = conn
            .transaction()
            .map_err(|e| db_error("create_integration_attempt transaction", &e))?;
        tx.execute(
            "INSERT INTO integration_attempts (id, task_id, repo_root, source_ref, source_tip_sha, source_range, source_dirty, source_worktree_digest, target_ref, expected_target_sha, commit_message, validation_commands_json, validation_digest, approval_digest, state, temporary_worktree_id, temporary_worktree_path, temporary_branch, conflict_summary_json, validation_result_json, result_commit_sha, recovery_bundle_path, cleanup_status, created_at, updated_at, repo_identity) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26)",
            params![attempt.id.0, attempt.task_id.0, attempt.repo_root, attempt.source_ref, attempt.source_tip_sha, attempt.source_range, attempt.source_dirty as i64, attempt.source_worktree_digest, attempt.target_ref, attempt.expected_target_sha, attempt.commit_message, attempt.validation_commands_json, attempt.validation_digest, attempt.approval_digest, attempt.state.as_str(), attempt.temporary_worktree_id, attempt.temporary_worktree_path, attempt.temporary_branch, attempt.conflict_summary_json, attempt.validation_result_json, attempt.result_commit_sha, attempt.recovery_bundle_path, attempt.cleanup_status, attempt.created_at, attempt.updated_at, attempt.repo_identity],
        ).map_err(|e| db_error("create_integration_attempt", &e))?;
        tx.execute("INSERT INTO integration_audit_events (attempt_id, state, detail_json, occurred_at) VALUES (?1, ?2, '{}', ?3)", params![attempt.id.0, attempt.state.as_str(), attempt.updated_at])
            .map_err(|e| db_error("create_integration_audit", &e))?;
        tx.commit()
            .map_err(|e| db_error("create_integration_attempt commit", &e))?;
        Ok(())
    }

    fn get_integration_attempt(&self, id: &str) -> RepoResult<IntegrationAttempt> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT id, task_id, repo_root, source_ref, source_tip_sha, source_range, source_dirty, source_worktree_digest, target_ref, expected_target_sha, commit_message, validation_commands_json, validation_digest, approval_digest, state, temporary_worktree_id, temporary_worktree_path, temporary_branch, conflict_summary_json, validation_result_json, result_commit_sha, recovery_bundle_path, cleanup_status, created_at, updated_at, repo_identity FROM integration_attempts WHERE id = ?1",
            params![id], row_to_integration,
        ).map_err(|e| db_error("get_integration_attempt", &e))
    }

    fn get_active_integration_by_repo(
        &self,
        repo_identity: &str,
        repo_root: &str,
    ) -> RepoResult<Option<IntegrationAttempt>> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT id, task_id, repo_root, source_ref, source_tip_sha, source_range, source_dirty, source_worktree_digest, target_ref, expected_target_sha, commit_message, validation_commands_json, validation_digest, approval_digest, state, temporary_worktree_id, temporary_worktree_path, temporary_branch, conflict_summary_json, validation_result_json, result_commit_sha, recovery_bundle_path, cleanup_status, created_at, updated_at, repo_identity FROM integration_attempts WHERE cleanup_status <> 'completed' AND (repo_identity = ?1 OR repo_root = ?2 OR repo_identity = '') ORDER BY created_at DESC LIMIT 1",
            params![repo_identity, repo_root],
            row_to_integration,
        )
        .optional()
        .map_err(|e| db_error("get_active_integration_by_repo", &e))
    }

    fn get_active_integration_by_task(
        &self,
        task_id: &str,
    ) -> RepoResult<Option<IntegrationAttempt>> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT id, task_id, repo_root, source_ref, source_tip_sha, source_range, source_dirty, source_worktree_digest, target_ref, expected_target_sha, commit_message, validation_commands_json, validation_digest, approval_digest, state, temporary_worktree_id, temporary_worktree_path, temporary_branch, conflict_summary_json, validation_result_json, result_commit_sha, recovery_bundle_path, cleanup_status, created_at, updated_at, repo_identity FROM integration_attempts WHERE task_id = ?1 AND cleanup_status <> 'completed' ORDER BY created_at DESC LIMIT 1",
            params![task_id],
            row_to_integration,
        )
        .optional()
        .map_err(|e| db_error("get_active_integration_by_task", &e))
    }

    fn update_integration_attempt(
        &self,
        attempt: &IntegrationAttempt,
        detail_json: &str,
    ) -> RepoResult<()> {
        let mut conn = self.lock()?;
        let tx = conn
            .transaction()
            .map_err(|e| db_error("update_integration_attempt transaction", &e))?;
        let changed = tx.execute(
            "UPDATE integration_attempts SET state=?1, temporary_worktree_id=?2, temporary_worktree_path=?3, temporary_branch=?4, conflict_summary_json=?5, validation_result_json=?6, result_commit_sha=?7, recovery_bundle_path=?8, cleanup_status=?9, updated_at=?10 WHERE id=?11",
            params![attempt.state.as_str(), attempt.temporary_worktree_id, attempt.temporary_worktree_path, attempt.temporary_branch, attempt.conflict_summary_json, attempt.validation_result_json, attempt.result_commit_sha, attempt.recovery_bundle_path, attempt.cleanup_status, attempt.updated_at, attempt.id.0],
        ).map_err(|e| db_error("update_integration_attempt", &e))?;
        if changed != 1 {
            return Err(DomainError::new(
                "DB_NOT_FOUND",
                "integration attempt not found",
            ));
        }
        tx.execute("INSERT INTO integration_audit_events (attempt_id, state, detail_json, occurred_at) VALUES (?1, ?2, ?3, ?4)", params![attempt.id.0, attempt.state.as_str(), detail_json, attempt.updated_at])
            .map_err(|e| db_error("update_integration_audit", &e))?;
        tx.commit()
            .map_err(|e| db_error("update_integration_attempt commit", &e))?;
        Ok(())
    }

    fn list_active_integrations(&self) -> RepoResult<Vec<IntegrationAttempt>> {
        let conn = self.lock()?;
        let mut statement = conn.prepare(
            "SELECT id, task_id, repo_root, source_ref, source_tip_sha, source_range, source_dirty, source_worktree_digest, target_ref, expected_target_sha, commit_message, validation_commands_json, validation_digest, approval_digest, state, temporary_worktree_id, temporary_worktree_path, temporary_branch, conflict_summary_json, validation_result_json, result_commit_sha, recovery_bundle_path, cleanup_status, created_at, updated_at, repo_identity FROM integration_attempts WHERE cleanup_status <> 'completed' ORDER BY created_at DESC"
        ).map_err(|error| db_error("list_active_integrations", &error))?;
        let rows = statement
            .query_map([], row_to_integration)
            .map_err(|error| db_error("list_active_integrations map", &error))?;
        collect_rows(rows, "list_active_integrations row")
    }

    // ==================================================================
    // Attachments
    // ==================================================================

    fn create_attachment(&self, att: &AttachmentRecord) -> RepoResult<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO attachments (id, task_id, sha256, mime, bytes, cache_path, source_name, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                att.id.0,
                att.task_id.0,
                att.sha256,
                att.mime,
                att.bytes,
                att.cache_path,
                att.source_name,
                att.created_at,
            ],
        )
        .map_err(|e| db_error("create_attachment", &e))?;
        Ok(())
    }

    fn get_attachment(&self, id: &str) -> RepoResult<AttachmentRecord> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT id, task_id, sha256, mime, bytes, cache_path, source_name, created_at FROM attachments WHERE id = ?1",
            params![id],
            row_to_attachment,
        )
        .map_err(|e| db_error("get_attachment", &e))
    }

    fn list_attachments_by_task(&self, task_id: &str) -> RepoResult<Vec<AttachmentRecord>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, task_id, sha256, mime, bytes, cache_path, source_name, created_at FROM attachments WHERE task_id = ?1 ORDER BY created_at",
            )
            .map_err(|e| db_error("list_attachments_by_task", &e))?;
        let rows = stmt
            .query_map(params![task_id], row_to_attachment)
            .map_err(|e| db_error("list_attachments_by_task", &e))?;
        collect_rows(rows, "list_attachments_by_task row")
    }

    // ==================================================================
    // Recovery Items
    // ==================================================================

    fn create_recovery_item(&self, item: &RecoveryItem) -> RepoResult<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO recovery_items (id, task_id, directory, manifest_path, expires_at, state) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                item.id.0,
                item.task_id.0,
                item.directory,
                item.manifest_path,
                item.expires_at,
                recovery_state_to_str(item.state),
            ],
        )
        .map_err(|e| db_error("create_recovery_item", &e))?;
        Ok(())
    }

    fn get_recovery_item(&self, id: &str) -> RepoResult<RecoveryItem> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT id, task_id, directory, manifest_path, expires_at, state FROM recovery_items WHERE id = ?1",
            params![id],
            row_to_recovery,
        )
        .map_err(|e| db_error("get_recovery_item", &e))
    }

    fn list_recovery_items(&self) -> RepoResult<Vec<RecoveryItem>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, task_id, directory, manifest_path, expires_at, state FROM recovery_items ORDER BY expires_at",
            )
            .map_err(|e| db_error("list_recovery_items", &e))?;
        let rows = stmt
            .query_map([], row_to_recovery)
            .map_err(|e| db_error("list_recovery_items", &e))?;
        collect_rows(rows, "list_recovery_items row")
    }

    fn update_recovery_item(&self, item: &RecoveryItem) -> RepoResult<()> {
        let conn = self.lock()?;
        conn.execute(
            "UPDATE recovery_items SET task_id = ?1, directory = ?2, manifest_path = ?3, expires_at = ?4, state = ?5 WHERE id = ?6",
            params![
                item.task_id.0,
                item.directory,
                item.manifest_path,
                item.expires_at,
                recovery_state_to_str(item.state),
                item.id.0,
            ],
        )
        .map_err(|e| db_error("update_recovery_item", &e))?;
        Ok(())
    }

    fn delete_recovery_item(&self, id: &str) -> RepoResult<()> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM recovery_items WHERE id = ?1", params![id])
            .map_err(|e| db_error("delete_recovery_item", &e))?;
        Ok(())
    }

    fn create_recovery_scan(&self, scan: &RecoveryScan) -> RepoResult<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO recovery_scans (id, trigger_kind, started_at, completed_at, issue_count) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![scan.id, scan.trigger_kind, scan.started_at, scan.completed_at, scan.issue_count as i64],
        ).map_err(|error| db_error("create_recovery_scan", &error))?;
        Ok(())
    }

    fn append_recovery_issue(&self, issue: &RecoveryIssue) -> RepoResult<()> {
        if issue.revision == 0 || issue.safe_actions.is_empty() {
            return Err(DomainError::new(
                "DB_QUERY_FAILED",
                "Recovery issue revision and safe actions are required",
            ));
        }
        let evidence = serde_json::to_string(&issue.evidence).map_err(|error| {
            DomainError::new(
                "DB_QUERY_FAILED",
                format!("serialize recovery evidence: {error}"),
            )
        })?;
        let actions: Vec<&str> = issue
            .safe_actions
            .iter()
            .map(|action| action.as_str())
            .collect();
        let actions = serde_json::to_string(&actions).map_err(|error| {
            DomainError::new(
                "DB_QUERY_FAILED",
                format!("serialize recovery actions: {error}"),
            )
        })?;
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO recovery_issues (issue_id, revision, scan_id, stable_key, kind, severity, status, task_id, resource_id, canonical_path, evidence_json, impact, recommended_action, safe_actions_json, detected_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            params![issue.issue_id, issue.revision as i64, issue.scan_id, issue.stable_key, issue.kind.as_str(), issue.severity.as_str(), issue.status.as_str(), issue.task_id.as_ref().map(|id| id.0.as_str()), issue.resource_id, issue.canonical_path, evidence, issue.impact, issue.recommended_action, actions, issue.detected_at],
        ).map_err(|error| db_error("append_recovery_issue", &error))?;
        Ok(())
    }

    fn latest_recovery_issue(&self, stable_key: &str) -> RepoResult<Option<RecoveryIssue>> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT issue_id, revision, scan_id, stable_key, kind, severity, status, task_id, resource_id, canonical_path, evidence_json, impact, recommended_action, safe_actions_json, detected_at FROM recovery_issues WHERE stable_key = ?1 ORDER BY revision DESC LIMIT 1",
            params![stable_key], row_to_recovery_issue,
        ).optional().map_err(|error| db_error("latest_recovery_issue", &error))
    }

    fn get_recovery_issue(
        &self,
        issue_id: &str,
        revision: Option<u32>,
    ) -> RepoResult<RecoveryIssue> {
        let conn = self.lock()?;
        let result = match revision {
            Some(revision) => conn.query_row(
                "SELECT issue_id, revision, scan_id, stable_key, kind, severity, status, task_id, resource_id, canonical_path, evidence_json, impact, recommended_action, safe_actions_json, detected_at FROM recovery_issues WHERE issue_id = ?1 AND revision = ?2",
                params![issue_id, revision as i64], row_to_recovery_issue,
            ),
            None => conn.query_row(
                "SELECT issue_id, revision, scan_id, stable_key, kind, severity, status, task_id, resource_id, canonical_path, evidence_json, impact, recommended_action, safe_actions_json, detected_at FROM recovery_issues WHERE issue_id = ?1 ORDER BY revision DESC LIMIT 1",
                params![issue_id], row_to_recovery_issue,
            ),
        };
        result.map_err(|error| db_error("get_recovery_issue", &error))
    }

    fn create_recovery_action_plan(&self, plan: &RecoveryActionPlan) -> RepoResult<()> {
        let expected = serde_json::to_string(&plan.expected_state).map_err(|error| {
            DomainError::new(
                "DB_QUERY_FAILED",
                format!("serialize recovery expected state: {error}"),
            )
        })?;
        let steps = serde_json::to_string(&plan.steps).map_err(|error| {
            DomainError::new(
                "DB_QUERY_FAILED",
                format!("serialize recovery steps: {error}"),
            )
        })?;
        let context = serde_json::to_string(&plan.internal_context).map_err(|error| {
            DomainError::new(
                "DB_QUERY_FAILED",
                format!("serialize recovery context: {error}"),
            )
        })?;
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO recovery_action_plans (id, issue_id, issue_revision, action_kind, resource_identity, canonical_path, expected_state_json, steps_json, internal_context_json, destructive_level, approval_digest, expires_at_epoch, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![plan.id, plan.issue_id, plan.issue_revision as i64, plan.action_kind.as_str(), plan.resource_identity, plan.canonical_path, expected, steps, context, plan.destructive_level, plan.approval_digest, plan.expires_at_epoch as i64, plan.created_at],
        ).map_err(|error| db_error("create_recovery_action_plan", &error))?;
        Ok(())
    }

    fn get_recovery_action_plan(&self, id: &str) -> RepoResult<RecoveryActionPlan> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT id, issue_id, issue_revision, action_kind, resource_identity, canonical_path, expected_state_json, steps_json, internal_context_json, destructive_level, approval_digest, expires_at_epoch, created_at FROM recovery_action_plans WHERE id = ?1",
            params![id], row_to_recovery_action_plan,
        ).map_err(|error| db_error("get_recovery_action_plan", &error))
    }

    fn append_recovery_step_result(&self, result: &RecoveryStepResult) -> RepoResult<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO recovery_step_results (plan_id, step_index, step_name, status, detail_redacted, occurred_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![result.plan_id, result.step_index as i64, result.step_name, result.status, result.detail_redacted, result.occurred_at],
        ).map_err(|error| db_error("append_recovery_step_result", &error))?;
        Ok(())
    }

    fn create_recovery_bundle_record(&self, bundle: &RecoveryBundleRecord) -> RepoResult<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO recovery_bundles (id, issue_id, issue_revision, recovery_item_id, manifest_path, manifest_sha256, verified, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![bundle.id, bundle.issue_id, bundle.issue_revision as i64, bundle.recovery_item_id, bundle.manifest_path, bundle.manifest_sha256, bundle.verified as i64, bundle.created_at],
        ).map_err(|error| db_error("create_recovery_bundle_record", &error))?;
        Ok(())
    }

    fn get_recovery_bundle_record(&self, id: &str) -> RepoResult<RecoveryBundleRecord> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT id, issue_id, issue_revision, recovery_item_id, manifest_path, manifest_sha256, verified, created_at FROM recovery_bundles WHERE id = ?1",
            params![id], row_to_recovery_bundle_record,
        ).map_err(|error| db_error("get_recovery_bundle_record", &error))
    }

    fn list_recovery_history(&self) -> RepoResult<RecoveryHistory> {
        let conn = self.lock()?;
        let scans = {
            let mut statement = conn.prepare("SELECT id, trigger_kind, started_at, completed_at, issue_count FROM recovery_scans ORDER BY completed_at DESC")
                .map_err(|error| db_error("list_recovery_history scans", &error))?;
            let rows = statement
                .query_map([], row_to_recovery_scan)
                .map_err(|error| db_error("list_recovery_history scans map", &error))?;
            collect_rows(rows, "list_recovery_history scans row")?
        };
        let issues = {
            let mut statement = conn.prepare("SELECT issue_id, revision, scan_id, stable_key, kind, severity, status, task_id, resource_id, canonical_path, evidence_json, impact, recommended_action, safe_actions_json, detected_at FROM recovery_issues ORDER BY detected_at DESC, issue_id, revision DESC")
                .map_err(|error| db_error("list_recovery_history issues", &error))?;
            let rows = statement
                .query_map([], row_to_recovery_issue)
                .map_err(|error| db_error("list_recovery_history issues map", &error))?;
            collect_rows(rows, "list_recovery_history issues row")?
        };
        let plans = {
            let mut statement = conn.prepare("SELECT id, issue_id, issue_revision, action_kind, resource_identity, canonical_path, expected_state_json, steps_json, internal_context_json, destructive_level, approval_digest, expires_at_epoch, created_at FROM recovery_action_plans ORDER BY created_at DESC")
                .map_err(|error| db_error("list_recovery_history plans", &error))?;
            let rows = statement
                .query_map([], row_to_recovery_action_plan)
                .map_err(|error| db_error("list_recovery_history plans map", &error))?;
            collect_rows(rows, "list_recovery_history plans row")?
        };
        let bundles = {
            let mut statement = conn.prepare("SELECT id, issue_id, issue_revision, recovery_item_id, manifest_path, manifest_sha256, verified, created_at FROM recovery_bundles ORDER BY created_at DESC")
                .map_err(|error| db_error("list_recovery_history bundles", &error))?;
            let rows = statement
                .query_map([], row_to_recovery_bundle_record)
                .map_err(|error| db_error("list_recovery_history bundles map", &error))?;
            collect_rows(rows, "list_recovery_history bundles row")?
        };
        let steps = {
            let mut statement = conn.prepare("SELECT id, plan_id, step_index, step_name, status, detail_redacted, occurred_at FROM recovery_step_results ORDER BY id")
                .map_err(|error| db_error("list_recovery_history steps", &error))?;
            let rows = statement
                .query_map([], row_to_recovery_step_result)
                .map_err(|error| db_error("list_recovery_history steps map", &error))?;
            collect_rows(rows, "list_recovery_history steps row")?
        };
        Ok(RecoveryHistory {
            scans,
            issues,
            plans,
            bundles,
            steps,
        })
    }

    // ==================================================================
    // Settings
    // ==================================================================

    fn get_setting(&self, key: &str) -> RepoResult<Option<Settings>> {
        let conn = self.lock()?;
        let result = conn.query_row(
            "SELECT key, json_value FROM settings WHERE key = ?1",
            params![key],
            row_to_setting,
        );
        match result {
            Ok(s) => Ok(Some(s)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(db_error("get_setting", &e)),
        }
    }

    fn set_setting(&self, setting: &Settings) -> RepoResult<()> {
        let conn = self.lock()?;
        let json_str = serde_json::to_string(&setting.json_value)
            .map_err(|e| DomainError::new("DB_QUERY_FAILED", format!("JSON serialize: {}", e)))?;
        conn.execute(
            "INSERT OR REPLACE INTO settings (key, json_value) VALUES (?1, ?2)",
            params![setting.key, json_str],
        )
        .map_err(|e| db_error("set_setting", &e))?;
        Ok(())
    }

    fn list_settings(&self) -> RepoResult<Vec<Settings>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare("SELECT key, json_value FROM settings ORDER BY key")
            .map_err(|e| db_error("list_settings", &e))?;
        let rows = stmt
            .query_map([], row_to_setting)
            .map_err(|e| db_error("list_settings", &e))?;
        collect_rows(rows, "list_settings row")
    }

    // ==================================================================
    // GAG-006: Session Events
    // ==================================================================

    fn append_event(&self, event: &StoredEvent) -> RepoResult<bool> {
        let conn = self.lock()?;
        let attempt_number: i64 = conn
            .query_row(
                "SELECT attempt_number FROM session_bindings WHERE task_id = ?1 AND session_id = ?2",
                params![event.task_id.0, event.session_id.0],
                |row| row.get(0),
            )
            .map_err(|e| db_error("append_event: binding attempt", &e))?;
        let payload_str = serde_json::to_string(&event.payload)
            .map_err(|e| DomainError::new("DB_QUERY_FAILED", format!("JSON serialize: {}", e)))?;
        let result = conn.execute(
            "INSERT OR IGNORE INTO session_events (dedup_key, session_id, task_id, sequence, event_type, payload, correlation_id, persisted_at, has_side_effects, attempt_number) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                event.dedup_key,
                event.session_id.0,
                event.task_id.0,
                event.sequence as i64,
                event.event_type,
                payload_str,
                event.correlation_id.as_ref().map(|c| c.0.as_str()),
                event.persisted_at,
                event.has_side_effects as i64,
                attempt_number,
            ],
        );
        match result {
            Ok(1) => Ok(true),  // inserted
            Ok(_) => Ok(false), // dedup_key existed — idempotent
            Err(e) => Err(db_error("append_event", &e)),
        }
    }

    fn get_events_after(
        &self,
        session_id: &str,
        after_seq: u64,
        limit: u32,
    ) -> RepoResult<Vec<StoredEvent>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT dedup_key, session_id, task_id, sequence, event_type, payload, correlation_id, persisted_at, has_side_effects FROM session_events WHERE session_id = ?1 AND sequence > ?2 ORDER BY sequence ASC LIMIT ?3",
            )
            .map_err(|e| db_error("get_events_after", &e))?;
        let rows = stmt
            .query_map(
                params![session_id, after_seq as i64, limit as i64],
                row_to_stored_event,
            )
            .map_err(|e| db_error("get_events_after: map", &e))?;
        collect_rows(rows, "get_events_after: row")
    }

    fn get_events_for_attempt(
        &self,
        session_id: &str,
        attempt_number: u32,
    ) -> RepoResult<Vec<StoredEvent>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT dedup_key, session_id, task_id, sequence, event_type, payload, correlation_id, persisted_at, has_side_effects FROM session_events WHERE session_id = ?1 AND attempt_number = ?2 ORDER BY sequence ASC",
            )
            .map_err(|e| db_error("get_events_for_attempt", &e))?;
        let rows = stmt
            .query_map(
                params![session_id, attempt_number as i64],
                row_to_stored_event,
            )
            .map_err(|e| db_error("get_events_for_attempt: map", &e))?;
        collect_rows(rows, "get_events_for_attempt: row")
    }

    fn get_max_sequence(&self, session_id: &str) -> RepoResult<Option<u64>> {
        let conn = self.lock()?;
        let result = conn.query_row(
            "SELECT MAX(sequence) FROM session_events WHERE session_id = ?1",
            params![session_id],
            |row| row.get::<_, Option<i64>>(0),
        );
        match result {
            Ok(Some(seq)) => Ok(Some(seq as u64)),
            Ok(None) => Ok(None),
            Err(e) => Err(db_error("get_max_sequence", &e)),
        }
    }

    fn get_session_snapshot(
        &self,
        task_id: &str,
        session_id: &str,
        event_limit: u32,
    ) -> RepoResult<SessionSnapshot> {
        let conn = self.lock()?;

        // Get the binding for this task.
        let binding = conn
            .query_row(
                "SELECT task_id, session_id, cwd, last_seq, state, attempt_number FROM session_bindings WHERE task_id = ?1 AND session_id = ?2",
                params![task_id, session_id],
                row_to_binding,
            )
            .map_err(|e| db_error("get_session_snapshot: binding", &e))?;

        // Get recent events for this session.
        let mut stmt = conn
            .prepare(
                "SELECT dedup_key, session_id, task_id, sequence, event_type, payload, correlation_id, persisted_at, has_side_effects FROM session_events WHERE session_id = ?1 ORDER BY sequence DESC LIMIT ?2",
            )
            .map_err(|e| db_error("get_session_snapshot: events", &e))?;
        let rows = stmt
            .query_map(params![session_id, event_limit as i64], row_to_stored_event)
            .map_err(|e| db_error("get_session_snapshot: map", &e))?;
        let mut events: Vec<StoredEvent> = collect_rows(rows, "get_session_snapshot: row")?;
        // Reverse to get chronological order (query returned DESC).
        events.reverse();

        let max_seq = events
            .last()
            .map(|e| e.sequence)
            .unwrap_or(binding.last_seq);
        let last_event_at = events
            .last()
            .map(|e| e.persisted_at.clone())
            .unwrap_or_else(utc_now);

        Ok(SessionSnapshot {
            task_id: TaskId::new(task_id.to_string()),
            session_id: SessionId::new(session_id.to_string()),
            state: binding.state,
            last_seq: binding.last_seq,
            captured_at: utc_now(),
            cursor: TimelineCursor {
                session_id: SessionId::new(session_id.to_string()),
                last_seq: max_seq,
                last_event_at,
            },
            recent_events: events,
            attempt_number: binding.attempt_number,
        })
    }

    // ==================================================================
    // GAG-006: Recovery & Concurrency
    // ==================================================================

    fn list_tasks_by_statuses(&self, statuses: &[&str]) -> RepoResult<Vec<Task>> {
        let conn = self.lock()?;
        if statuses.is_empty() {
            return Ok(vec![]);
        }
        let placeholders: Vec<String> = statuses
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect();
        let sql = format!(
            "SELECT id, project_id, title, status, workspace_kind, mode, model, reasoning, created_at, updated_at, interrupt_reason, interrupted_at, attempt_count FROM tasks WHERE status IN ({}) ORDER BY updated_at DESC",
            placeholders.join(", ")
        );
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|e| db_error("list_tasks_by_statuses", &e))?;
        let params_vec: Vec<&dyn rusqlite::types::ToSql> = statuses
            .iter()
            .map(|s| s as &dyn rusqlite::types::ToSql)
            .collect();
        let rows = stmt
            .query_map(params_vec.as_slice(), row_to_task)
            .map_err(|e| db_error("list_tasks_by_statuses: map", &e))?;
        collect_rows(rows, "list_tasks_by_statuses: row")
    }

    fn list_task_summaries(&self) -> RepoResult<Vec<TaskSummary>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, title, status, updated_at FROM tasks WHERE status NOT IN ('merged', 'archived') ORDER BY updated_at DESC",
            )
            .map_err(|e| db_error("list_task_summaries", &e))?;
        let rows = stmt
            .query_map([], row_to_task_summary)
            .map_err(|e| db_error("list_task_summaries: map", &e))?;
        collect_rows(rows, "list_task_summaries: row")
    }

    fn increment_binding_attempt(&self, task_id: &str) -> RepoResult<u32> {
        let conn = self.lock()?;
        let new_attempt: i64 = conn
            .query_row(
                "UPDATE session_bindings SET attempt_number = attempt_number + 1 WHERE task_id = ?1 RETURNING attempt_number",
                params![task_id],
                |row| row.get(0),
            )
            .map_err(|e| db_error("increment_binding_attempt", &e))?;
        Ok(new_attempt as u32)
    }

    fn list_recovery_candidates(&self) -> RepoResult<Vec<RecoveryCandidate>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT t.id, t.title, t.status, t.interrupted_at, t.interrupt_reason, COALESCE(sb.task_id IS NOT NULL, 0) as has_session, 0, t.attempt_count FROM tasks t LEFT JOIN session_bindings sb ON t.id = sb.task_id WHERE t.status = 'interrupted' ORDER BY t.updated_at DESC",
            )
            .map_err(|e| db_error("list_recovery_candidates", &e))?;
        let rows = stmt
            .query_map([], row_to_recovery_candidate)
            .map_err(|e| db_error("list_recovery_candidates: map", &e))?;
        let mut candidates = collect_rows(rows, "list_recovery_candidates: row")?;

        // Enrich with events_available info.
        for c in &mut candidates {
            let has_events: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM session_events WHERE task_id = ?1",
                    params![c.task_id.0],
                    |row| row.get(0),
                )
                .unwrap_or(false);
            c.events_available = has_events;
        }

        Ok(candidates)
    }

    fn apply_recovery_decision(&self, decision: &RecoveryDecision) -> RepoResult<()> {
        let conn = self.lock()?;
        let now = utc_now();
        match decision.action {
            RecoveryAction::Resume => {
                conn.execute(
                    "UPDATE tasks SET status = 'preparing', interrupt_reason = NULL, interrupted_at = NULL, updated_at = ?1, attempt_count = attempt_count + 1 WHERE id = ?2 AND status = 'interrupted'",
                    params![now, decision.task_id.0],
                )
                .map_err(|e| db_error("apply_recovery_decision: resume", &e))?;
            }
            RecoveryAction::Archive => {
                conn.execute(
                    "UPDATE tasks SET status = 'archived', updated_at = ?1 WHERE id = ?2 AND status = 'interrupted'",
                    params![now, decision.task_id.0],
                )
                .map_err(|e| db_error("apply_recovery_decision: archive", &e))?;
            }
        }
        Ok(())
    }

    fn get_concurrency_limits(&self, max_concurrent: u32) -> RepoResult<ConcurrencyLimits> {
        let conn = self.lock()?;
        let running: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE status = 'running'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let queued: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM tasks WHERE status = 'preparing'",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        Ok(ConcurrencyLimits {
            max_concurrent_tasks: max_concurrent,
            current_running: running as u32,
            current_queued: queued as u32,
        })
    }

    fn create_plan(&self, plan: &PlanRecord) -> RepoResult<()> {
        let mut conn = self.lock()?;
        let tx = conn
            .transaction()
            .map_err(|e| db_error("create_plan transaction", &e))?;
        tx.execute(
            "UPDATE plans SET state = 'superseded', updated_at = ?1
             WHERE task_id = ?2 AND state IN ('draft','proposed','approved','rejected','revision_requested')",
            params![plan.updated_at, plan.task_id.0],
        )
        .map_err(|e| db_error("create_plan supersede", &e))?;
        tx.execute(
            "UPDATE permission_decisions SET state = 'expired', updated_at = ?1
             WHERE task_id = ?2 AND state IN ('requested','approved_once','approved_scope')",
            params![plan.updated_at, plan.task_id.0],
        )
        .map_err(|e| db_error("create_plan expire approvals", &e))?;
        let options = serde_json::to_string(&plan.options).map_err(|e| {
            DomainError::new("DB_QUERY_FAILED", format!("serialize plan options: {e}"))
        })?;
        tx.execute(
            "INSERT INTO plans (request_id, task_id, session_id, correlation_id, workspace, version, plan_hash, state, summary_redacted, options_json, decided_option_id, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![
                plan.request_id, plan.task_id.0, plan.session_id.0, plan.correlation_id,
                plan.workspace, plan.version as i64, plan.plan_hash, plan_state_str(plan.state),
                plan.summary_redacted, options, plan.decided_option_id, plan.created_at, plan.updated_at
            ],
        )
        .map_err(|e| db_error("create_plan insert", &e))?;
        tx.execute(
            "INSERT INTO approval_audit_events (task_id,session_id,request_id,event_kind,decision,plan_version,correlation_id,occurred_at)
             VALUES (?1,?2,?3,'plan','proposed',?4,?5,?6)",
            params![plan.task_id.0, plan.session_id.0, plan.request_id, plan.version as i64, plan.correlation_id, plan.created_at],
        )
        .map_err(|e| db_error("create_plan audit", &e))?;
        tx.commit()
            .map_err(|e| db_error("create_plan commit", &e))?;
        Ok(())
    }

    fn get_plan(&self, request_id: &str, session_id: &str) -> RepoResult<PlanRecord> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT request_id,task_id,session_id,correlation_id,workspace,version,plan_hash,state,summary_redacted,options_json,decided_option_id,created_at,updated_at FROM plans WHERE request_id=?1 AND session_id=?2",
            params![request_id, session_id],
            row_to_plan,
        )
        .map_err(|e| db_error("get_plan", &e))
    }

    fn latest_plan_version(&self, task_id: &str) -> RepoResult<Option<u64>> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT MAX(version) FROM plans WHERE task_id=?1",
            params![task_id],
            |row| row.get::<_, Option<i64>>(0),
        )
        .map(|value| value.map(|item| item as u64))
        .map_err(|e| db_error("latest_plan_version", &e))
    }

    fn latest_plan(&self, task_id: &str) -> RepoResult<Option<PlanRecord>> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT request_id,task_id,session_id,correlation_id,workspace,version,plan_hash,state,summary_redacted,options_json,decided_option_id,created_at,updated_at FROM plans WHERE task_id=?1 ORDER BY version DESC LIMIT 1",
            params![task_id],
            row_to_plan,
        )
        .optional()
        .map_err(|e| db_error("latest_plan", &e))
    }

    fn decide_plan(&self, decision: &PlanDecision) -> RepoResult<PlanRecord> {
        let mut conn = self.lock()?;
        let tx = conn
            .transaction()
            .map_err(|e| db_error("decide_plan transaction", &e))?;
        let mut plan = tx
            .query_row(
                "SELECT request_id,task_id,session_id,correlation_id,workspace,version,plan_hash,state,summary_redacted,options_json,decided_option_id,created_at,updated_at FROM plans WHERE request_id=?1 AND session_id=?2",
                params![decision.request_id, decision.session_id.0],
                row_to_plan,
            )
            .map_err(|e| db_error("decide_plan load", &e))?;
        if plan.task_id != decision.task_id
            || plan.session_id != decision.session_id
            || plan.correlation_id != decision.correlation_id
            || plan.workspace != decision.workspace
        {
            return Err(DomainError::new(
                crate::domain::error::codes::PERMISSION_CONTEXT_MISMATCH,
                "Plan decision context does not match",
            ));
        }
        if plan.version != decision.expected_version {
            return Err(DomainError::new(
                crate::domain::error::codes::PLAN_VERSION_MISMATCH,
                "Plan version changed; approval is invalid",
            ));
        }
        if plan.state != PlanState::Proposed {
            return Err(DomainError::new(
                crate::domain::error::codes::PERMISSION_ALREADY_RESOLVED,
                "Plan request is no longer pending",
            ));
        }
        let action = plan
            .options
            .iter()
            .find(|option| option.option_id == decision.option_id)
            .map(|option| option.action)
            .unwrap_or(PlanOptionAction::Unknown);
        plan.state = match action {
            PlanOptionAction::Approve => PlanState::Approved,
            PlanOptionAction::RequestRevision => PlanState::RevisionRequested,
            PlanOptionAction::Reject => PlanState::Rejected,
            PlanOptionAction::Unknown => {
                return Err(DomainError::new(
                    crate::domain::error::codes::PERMISSION_DENIED,
                    "Plan option has no explicit safe action",
                ))
            }
        };
        let changed = tx
            .execute(
                "UPDATE plans SET state=?1,decided_option_id=?2,updated_at=?3
             WHERE request_id=?4 AND session_id=?5 AND version=?6 AND state='proposed'",
                params![
                    plan_state_str(plan.state),
                    decision.option_id,
                    decision.decided_at,
                    decision.request_id,
                    decision.session_id.0,
                    decision.expected_version as i64
                ],
            )
            .map_err(|e| db_error("decide_plan update", &e))?;
        if changed != 1 {
            return Err(DomainError::new(
                crate::domain::error::codes::PERMISSION_ALREADY_RESOLVED,
                "Plan was resolved concurrently",
            ));
        }
        tx.execute(
            "INSERT INTO approval_audit_events (task_id,session_id,request_id,event_kind,decision,plan_version,correlation_id,occurred_at)
             VALUES (?1,?2,?3,'plan',?4,?5,?6,?7)",
            params![plan.task_id.0, plan.session_id.0, plan.request_id, plan_state_str(plan.state), plan.version as i64, plan.correlation_id, decision.decided_at],
        ).map_err(|e| db_error("decide_plan audit", &e))?;
        plan.decided_option_id = Some(decision.option_id.clone());
        plan.updated_at = decision.decided_at.clone();
        tx.commit()
            .map_err(|e| db_error("decide_plan commit", &e))?;
        Ok(plan)
    }

    fn revert_plan_decision(&self, request_id: &str, session_id: &str) -> RepoResult<PlanRecord> {
        let conn = self.lock()?;
        let changed = conn
            .execute(
                "UPDATE plans SET state='proposed',decided_option_id=NULL,updated_at=?1
                 WHERE request_id=?2 AND session_id=?3
                   AND state IN ('approved','rejected','revision_requested')",
                params![utc_now(), request_id, session_id],
            )
            .map_err(|e| db_error("revert_plan_decision update", &e))?;
        if changed != 1 {
            return Err(DomainError::new(
                crate::domain::error::codes::PERMISSION_ALREADY_RESOLVED,
                "Plan decision cannot be safely reverted",
            ));
        }
        // Query on the already-held connection: get_plan() re-locks the mutex.
        conn.query_row(
            "SELECT request_id,task_id,session_id,correlation_id,workspace,version,plan_hash,state,summary_redacted,options_json,decided_option_id,created_at,updated_at FROM plans WHERE request_id=?1 AND session_id=?2",
            params![request_id, session_id],
            row_to_plan,
        )
        .map_err(|e| db_error("revert_plan_decision load", &e))
    }

    fn supersede_session_plans(&self, session_id: &str, _reason: &str) -> RepoResult<u32> {
        let conn = self.lock()?;
        let count = conn
            .execute(
                "UPDATE plans SET state='superseded',updated_at=?1 WHERE session_id=?2 AND state='proposed'",
                params![utc_now(), session_id],
            )
            .map_err(|e| db_error("supersede_session_plans", &e))?;
        Ok(count as u32)
    }

    fn create_permission(&self, permission: &PermissionRecord) -> RepoResult<()> {
        let conn = self.lock()?;
        let options = serde_json::to_string(&permission.options).map_err(|e| {
            DomainError::new(
                "DB_QUERY_FAILED",
                format!("serialize permission options: {e}"),
            )
        })?;
        conn.execute(
            "INSERT INTO permission_decisions (request_id,task_id,session_id,correlation_id,workspace,plan_version,operation_digest,category,summary_redacted,options_json,state,expires_at_epoch,decided_option_id,consumed_at,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
            params![permission.request_id, permission.task_id.0, permission.session_id.0, permission.correlation_id,
                permission.workspace, permission.plan_version.map(|value| value as i64), permission.operation_digest,
                operation_category_str(permission.category), permission.summary_redacted, options,
                permission_state_str(permission.state), permission.expires_at_epoch_seconds as i64,
                permission.decided_option_id, permission.consumed_at, permission.created_at, permission.updated_at],
        ).map_err(|e| db_error("create_permission", &e))?;
        Ok(())
    }

    fn get_permission(&self, request_id: &str, session_id: &str) -> RepoResult<PermissionRecord> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT request_id,task_id,session_id,correlation_id,workspace,plan_version,operation_digest,category,summary_redacted,options_json,state,expires_at_epoch,decided_option_id,consumed_at,created_at,updated_at FROM permission_decisions WHERE request_id=?1 AND session_id=?2",
            params![request_id, session_id], row_to_permission,
        ).map_err(|e| db_error("get_permission", &e))
    }

    fn decide_permission(&self, decision: &PermissionDecision) -> RepoResult<PermissionRecord> {
        let mut conn = self.lock()?;
        let tx = conn
            .transaction()
            .map_err(|e| db_error("decide_permission transaction", &e))?;
        let mut permission = tx.query_row(
            "SELECT request_id,task_id,session_id,correlation_id,workspace,plan_version,operation_digest,category,summary_redacted,options_json,state,expires_at_epoch,decided_option_id,consumed_at,created_at,updated_at FROM permission_decisions WHERE request_id=?1 AND session_id=?2",
            params![decision.request_id, decision.session_id.0], row_to_permission,
        ).map_err(|e| db_error("decide_permission load", &e))?;
        if permission.task_id != decision.task_id
            || permission.session_id != decision.session_id
            || permission.correlation_id != decision.correlation_id
            || permission.workspace != decision.workspace
            || permission.plan_version != decision.expected_plan_version
        {
            return Err(DomainError::new(
                crate::domain::error::codes::PERMISSION_CONTEXT_MISMATCH,
                "Permission decision context does not match",
            ));
        }
        if permission.state != PermissionState::Requested {
            return Err(DomainError::new(
                crate::domain::error::codes::PERMISSION_ALREADY_RESOLVED,
                "Permission request is no longer pending",
            ));
        }
        if permission.expires_at_epoch_seconds < decision.decided_at_epoch_seconds {
            tx.execute("UPDATE permission_decisions SET state='expired',updated_at=?1 WHERE request_id=?2 AND session_id=?3 AND state='requested'", params![decision.decided_at, decision.request_id, decision.session_id.0])
                .map_err(|e| db_error("decide_permission expire", &e))?;
            tx.commit()
                .map_err(|e| db_error("decide_permission expire commit", &e))?;
            return Err(DomainError::new(
                crate::domain::error::codes::PERMISSION_EXPIRED,
                "Permission request expired",
            ));
        }
        let action = permission
            .options
            .iter()
            .find(|option| option.option_id == decision.option_id)
            .map(|option| option.action)
            .unwrap_or(PermissionOptionAction::Unknown);
        permission.state = match action {
            PermissionOptionAction::AllowOnce => PermissionState::ApprovedOnce,
            PermissionOptionAction::AllowScope
                if permission.category == OperationCategory::ReadOnly =>
            {
                PermissionState::ApprovedScope
            }
            PermissionOptionAction::AllowScope => {
                return Err(DomainError::new(
                    crate::domain::error::codes::PERMISSION_DENIED,
                    "Persistent approval is restricted to exact read-only operations",
                ))
            }
            PermissionOptionAction::Deny => PermissionState::Denied,
            PermissionOptionAction::Unknown => {
                return Err(DomainError::new(
                    crate::domain::error::codes::PERMISSION_DENIED,
                    "Permission option has no explicit safe action",
                ))
            }
        };
        let changed = tx.execute(
            "UPDATE permission_decisions SET state=?1,decided_option_id=?2,scope_json=?3,updated_at=?4 WHERE request_id=?5 AND session_id=?6 AND state='requested'",
            params![permission_state_str(permission.state), decision.option_id,
                if permission.state == PermissionState::ApprovedScope { Some(format!("{{\"operationDigest\":\"{}\"}}", permission.operation_digest)) } else { None },
                decision.decided_at, decision.request_id, decision.session_id.0],
        ).map_err(|e| db_error("decide_permission update", &e))?;
        if changed != 1 {
            return Err(DomainError::new(
                crate::domain::error::codes::PERMISSION_ALREADY_RESOLVED,
                "Permission was resolved concurrently",
            ));
        }
        tx.execute(
            "INSERT INTO approval_audit_events (task_id,session_id,request_id,event_kind,decision,operation_digest,plan_version,correlation_id,occurred_at)
             VALUES (?1,?2,?3,'permission',?4,?5,?6,?7,?8)",
            params![permission.task_id.0, permission.session_id.0, permission.request_id, permission_state_str(permission.state), permission.operation_digest,
                permission.plan_version.map(|value| value as i64), permission.correlation_id, decision.decided_at],
        ).map_err(|e| db_error("decide_permission audit", &e))?;
        permission.decided_option_id = Some(decision.option_id.clone());
        permission.updated_at = decision.decided_at.clone();
        tx.commit()
            .map_err(|e| db_error("decide_permission commit", &e))?;
        Ok(permission)
    }

    fn revert_permission_decision(
        &self,
        request_id: &str,
        session_id: &str,
    ) -> RepoResult<PermissionRecord> {
        let conn = self.lock()?;
        let changed = conn
            .execute(
                "UPDATE permission_decisions
                 SET state='requested',decided_option_id=NULL,scope_json=NULL,updated_at=?1
                 WHERE request_id=?2 AND session_id=?3
                   AND state IN ('approved_once','approved_scope','denied')",
                params![utc_now(), request_id, session_id],
            )
            .map_err(|e| db_error("revert_permission_decision update", &e))?;
        if changed != 1 {
            return Err(DomainError::new(
                crate::domain::error::codes::PERMISSION_ALREADY_RESOLVED,
                "Permission decision cannot be safely reverted",
            ));
        }
        // Query on the already-held connection: get_permission() re-locks the mutex.
        conn.query_row(
            "SELECT request_id,task_id,session_id,correlation_id,workspace,plan_version,operation_digest,category,summary_redacted,options_json,state,expires_at_epoch,decided_option_id,consumed_at,created_at,updated_at FROM permission_decisions WHERE request_id=?1 AND session_id=?2",
            params![request_id, session_id],
            row_to_permission,
        )
        .map_err(|e| db_error("revert_permission_decision load", &e))
    }

    fn expire_permission(
        &self,
        request_id: &str,
        session_id: &str,
    ) -> RepoResult<Option<PermissionRecord>> {
        let mut conn = self.lock()?;
        let tx = conn
            .transaction()
            .map_err(|e| db_error("expire_permission transaction", &e))?;
        let permission = tx.query_row(
            "SELECT request_id,task_id,session_id,correlation_id,workspace,plan_version,operation_digest,category,summary_redacted,options_json,state,expires_at_epoch,decided_option_id,consumed_at,created_at,updated_at FROM permission_decisions WHERE request_id=?1 AND session_id=?2 AND state='requested'",
            params![request_id, session_id],
            row_to_permission,
        ).optional().map_err(|e| db_error("expire_permission load", &e))?;
        let Some(permission) = permission else {
            return Ok(None);
        };
        tx.execute(
            "UPDATE permission_decisions SET state='expired', updated_at=?1 WHERE request_id=?2 AND session_id=?3 AND state='requested'",
            params![utc_now(), request_id, session_id],
        ).map_err(|e| db_error("expire_permission update", &e))?;
        tx.commit()
            .map_err(|e| db_error("expire_permission commit", &e))?;
        Ok(Some(permission))
    }

    fn expire_session_permissions(&self, session_id: &str, _reason: &str) -> RepoResult<u32> {
        let conn = self.lock()?;
        let count = conn.execute(
            "UPDATE permission_decisions SET state='expired',updated_at=?1 WHERE session_id=?2 AND state IN ('requested','approved_once')",
            params![utc_now(), session_id],
        ).map_err(|e| db_error("expire_session_permissions", &e))?;
        Ok(count as u32)
    }

    fn consume_permission(
        &self,
        context: &ExecutionContext,
        operation_digest: &str,
        now_epoch_seconds: u64,
    ) -> RepoResult<Option<ApprovalEvidence>> {
        let mut conn = self.lock()?;
        let tx = conn
            .transaction()
            .map_err(|e| db_error("consume_permission transaction", &e))?;
        let found = tx
            .query_row(
                "SELECT request_id,state,expires_at_epoch FROM permission_decisions
             WHERE task_id=?1 AND session_id=?2 AND workspace=?3 AND operation_digest=?4
               AND plan_version IS ?5 AND state IN ('approved_once','approved_scope')
             ORDER BY CASE state WHEN 'approved_once' THEN 0 ELSE 1 END, updated_at DESC LIMIT 1",
                params![
                    context.task_id.0,
                    context.session_id.0,
                    context.workspace,
                    operation_digest,
                    context.plan_version.map(|value| value as i64)
                ],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| db_error("consume_permission load", &e))?;
        let Some((request_id, state, expires_at)) = found else {
            return Ok(None);
        };
        if expires_at < now_epoch_seconds as i64 {
            tx.execute(
                "UPDATE permission_decisions SET state='expired',updated_at=?1 WHERE request_id=?2 AND session_id=?3",
                params![utc_now(), request_id, context.session_id.0],
            )
            .map_err(|e| db_error("consume_permission expire", &e))?;
            tx.commit()
                .map_err(|e| db_error("consume_permission expire commit", &e))?;
            return Err(DomainError::new(
                crate::domain::error::codes::PERMISSION_EXPIRED,
                "Approval expired",
            ));
        }
        let consumed_at = utc_now();
        if state == "approved_once" {
            let changed = tx.execute(
                "UPDATE permission_decisions SET state='consumed',consumed_at=?1,updated_at=?1 WHERE request_id=?2 AND session_id=?3 AND state='approved_once'",
                params![consumed_at, request_id, context.session_id.0],
            ).map_err(|e| db_error("consume_permission update", &e))?;
            if changed != 1 {
                return Err(DomainError::new(
                    crate::domain::error::codes::PERMISSION_ALREADY_RESOLVED,
                    "Approval was already consumed",
                ));
            }
        }
        tx.execute(
            "INSERT INTO approval_audit_events (task_id,session_id,request_id,event_kind,decision,operation_digest,plan_version,correlation_id,occurred_at)
             SELECT task_id,session_id,request_id,'permission','consumed',operation_digest,plan_version,correlation_id,?1 FROM permission_decisions WHERE request_id=?2 AND session_id=?3",
            params![consumed_at, request_id, context.session_id.0],
        ).map_err(|e| db_error("consume_permission audit", &e))?;
        tx.commit()
            .map_err(|e| db_error("consume_permission commit", &e))?;
        Ok(Some(ApprovalEvidence {
            permission_id: request_id,
            session_id: context.session_id.clone(),
            workspace: context.workspace.clone(),
            operation_digest: operation_digest.to_string(),
            plan_version: context.plan_version,
            expires_at_epoch_seconds: expires_at as u64,
        }))
    }

    fn recover_interrupted_tasks(&self, reason: &str) -> RepoResult<u32> {
        let mut conn = self.lock()?;
        let tx = conn
            .transaction()
            .map_err(|e| db_error("recover_interrupted_tasks transaction", &e))?;
        let now = utc_now();
        tx.execute(
            "UPDATE permission_decisions SET state='expired',updated_at=?1
             WHERE state IN ('requested','approved_once')",
            params![now],
        )
        .map_err(|e| db_error("recover_interrupted_tasks approvals", &e))?;
        tx.execute(
            "UPDATE plans SET state='failed',updated_at=?1 WHERE state='proposed'",
            params![now],
        )
        .map_err(|e| db_error("recover_interrupted_tasks plans", &e))?;
        let count = tx
            .execute(
                "UPDATE tasks
                 SET status = 'interrupted', interrupt_reason = ?1, updated_at = ?2
                 WHERE status IN ('running', 'waiting_permission', 'integrating')
                    OR (
                        status = 'idle'
                        AND EXISTS (
                            SELECT 1
                            FROM session_bindings sb
                            JOIN session_events se ON se.session_id = sb.session_id
                            WHERE sb.task_id = tasks.id
                              AND sb.state = 'disconnected'
                              AND se.sequence = (
                                  SELECT MAX(latest.sequence)
                                  FROM session_events latest
                                  WHERE latest.session_id = sb.session_id
                              )
                              AND se.event_type = 'process_exited'
                              AND (
                                  json_valid(se.payload) = 0
                                  OR COALESCE(json_extract(se.payload, '$.reason'), 'unknown') != 'clean'
                              )
                        )
                    )",
                params![reason, now],
            )
            .map_err(|e| db_error("recover_interrupted_tasks", &e))?;
        tx.commit()
            .map_err(|e| db_error("recover_interrupted_tasks commit", &e))?;
        Ok(count as u32)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_repo() -> SqliteRepository {
        SqliteRepository::open_in_memory().expect("in-memory repo")
    }

    fn make_project(id: &str, path: &str) -> Project {
        Project {
            id: ProjectId::new(id),
            path: path.into(),
            display_path: path.into(),
            repo_root: Some(path.into()),
            trusted_at: Some(utc_now()),
            last_opened_at: utc_now(),
        }
    }

    fn make_task(id: &str, project_id: &str, status: TaskStatus) -> Task {
        let now = utc_now();
        Task {
            id: TaskId::new(id),
            project_id: ProjectId::new(project_id),
            title: format!("Task {}", id),
            status,
            workspace_kind: WorkspaceKind::Worktree,
            mode: None,
            model: None,
            reasoning: None,
            created_at: now.clone(),
            updated_at: now,
            interrupt_reason: None,
            interrupted_at: None,
            attempt_count: 1,
        }
    }

    #[test]
    fn create_and_get_project() {
        let repo = make_test_repo();
        let p = make_project("p1", "/home/test/project");
        repo.create_project(&p).unwrap();
        let got = repo.get_project("p1").unwrap();
        assert_eq!(got.id, p.id);
        assert_eq!(got.path, "/home/test/project");
    }

    #[test]
    fn create_and_get_task() {
        let repo = make_test_repo();
        let p = make_project("p1", "/home/test");
        repo.create_project(&p).unwrap();
        let t = make_task("t1", "p1", TaskStatus::Preparing);
        repo.create_task(&t).unwrap();
        let got = repo.get_task("t1").unwrap();
        assert_eq!(got.status, TaskStatus::Preparing);
    }

    #[test]
    fn legacy_missing_workspace_kind_defaults_to_worktree() {
        let repo = make_test_repo();
        repo.create_project(&make_project("legacy-project", "/home/legacy"))
            .unwrap();
        repo.create_task(&make_task(
            "legacy-task",
            "legacy-project",
            TaskStatus::Idle,
        ))
        .unwrap();

        repo.conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE tasks SET workspace_kind = '' WHERE id = 'legacy-task'",
                [],
            )
            .expect("simulate a pre-field/unknown workspace value");

        assert_eq!(
            repo.get_task("legacy-task").unwrap().workspace_kind,
            WorkspaceKind::Worktree,
            "legacy records must never degrade to direct"
        );
    }

    #[test]
    fn configuration_update_never_overwrites_runtime_status() {
        let repo = make_test_repo();
        repo.create_project(&make_project("config-project", "/home/config"))
            .unwrap();
        repo.create_task(&make_task(
            "config-task",
            "config-project",
            TaskStatus::Idle,
        ))
        .unwrap();
        repo.update_task_status("config-task", "interrupted", Some("process exited"))
            .unwrap();

        repo.update_task_configuration(
            "config-task",
            WorkspaceKind::Readonly,
            Some("plan"),
            Some("model-a"),
            Some("high"),
            &utc_now(),
        )
        .unwrap();

        let task = repo.get_task("config-task").unwrap();
        assert_eq!(task.status, TaskStatus::Interrupted);
        assert_eq!(task.workspace_kind, WorkspaceKind::Readonly);
        assert_eq!(task.mode.as_deref(), Some("plan"));
    }

    #[test]
    fn unique_project_path_rejected() {
        let repo = make_test_repo();
        let p1 = make_project("p1", "/same/path");
        let p2 = make_project("p2", "/same/path");
        repo.create_project(&p1).unwrap();
        let result = repo.create_project(&p2);
        assert!(result.is_err());
    }

    #[test]
    fn unique_session_id_enforced() {
        let repo = make_test_repo();
        let p = make_project("p1", "/test");
        repo.create_project(&p).unwrap();
        let t = make_task("t1", "p1", TaskStatus::Preparing);
        repo.create_task(&t).unwrap();

        let b1 = SessionBinding {
            task_id: TaskId::new("t1"),
            session_id: SessionId::new("s1"),
            cwd: None,
            last_seq: 0,
            state: SessionState::Active,
            attempt_number: 1,
        };
        repo.create_binding(&b1).unwrap();

        let b2 = SessionBinding {
            task_id: TaskId::new("t1"),
            session_id: SessionId::new("s1"), // duplicate
            cwd: None,
            last_seq: 0,
            state: SessionState::Active,
            attempt_number: 1,
        };
        assert!(repo.create_binding(&b2).is_err());
    }

    #[test]
    fn bootstrap_snapshot_includes_entities() {
        let repo = make_test_repo();
        let p = make_project("p1", "/test");
        repo.create_project(&p).unwrap();
        let t = make_task("t1", "p1", TaskStatus::Running);
        repo.create_task(&t).unwrap();
        let completed = make_task("t2", "p1", TaskStatus::Merged);
        repo.create_task(&completed).unwrap();

        let snap = repo.bootstrap_snapshot().unwrap();
        assert_eq!(snap.projects.len(), 1);
        assert_eq!(snap.active_tasks.len(), 1);
        assert_eq!(snap.completed_tasks.len(), 1);
        assert_eq!(snap.completed_tasks[0].id.0, "t2");
    }

    #[test]
    fn recover_interrupted_tasks() {
        let repo = make_test_repo();
        let p = make_project("p1", "/test");
        repo.create_project(&p).unwrap();

        let t1 = make_task("t1", "p1", TaskStatus::Running);
        let t2 = make_task("t2", "p1", TaskStatus::WaitingPermission);
        let t3 = make_task("t3", "p1", TaskStatus::Integrating);
        let t4 = make_task("t4", "p1", TaskStatus::Preparing);
        let t5 = make_task("t5", "p1", TaskStatus::Archived);

        repo.create_task(&t1).unwrap();
        repo.create_task(&t2).unwrap();
        repo.create_task(&t3).unwrap();
        repo.create_task(&t4).unwrap();
        repo.create_task(&t5).unwrap();

        let count = repo
            .recover_interrupted_tasks("app exited unexpectedly")
            .unwrap();
        assert_eq!(count, 3, "should interrupt 3 live-process tasks");

        let t1_after = repo.get_task("t1").unwrap();
        assert_eq!(t1_after.status, TaskStatus::Interrupted);

        let t4_after = repo.get_task("t4").unwrap();
        assert_eq!(t4_after.status, TaskStatus::Preparing); // unaffected

        let t5_after = repo.get_task("t5").unwrap();
        assert_eq!(t5_after.status, TaskStatus::Archived); // unaffected
    }

    #[test]
    fn recovery_repairs_idle_task_after_non_clean_disconnected_process_only() {
        let repo = make_test_repo();
        repo.create_project(&make_project("p1", "/test")).unwrap();
        for task_id in ["idle-crash", "idle-clean"] {
            repo.create_task(&make_task(task_id, "p1", TaskStatus::Idle))
                .unwrap();
            let session_id = format!("session-{task_id}");
            repo.create_binding(&SessionBinding {
                task_id: TaskId::new(task_id),
                session_id: SessionId::new(&session_id),
                cwd: Some("/test".into()),
                last_seq: 0,
                state: SessionState::Disconnected,
                attempt_number: 1,
            })
            .unwrap();
            repo.append_event(&StoredEvent {
                dedup_key: format!("{session_id}:1"),
                session_id: SessionId::new(&session_id),
                task_id: TaskId::new(task_id),
                sequence: 1,
                event_type: "process_exited".into(),
                payload: serde_json::json!({
                    "kind": "process_exited",
                    "reason": if task_id == "idle-clean" { "clean" } else { "inbound_closed" }
                }),
                correlation_id: None,
                persisted_at: utc_now(),
                has_side_effects: false,
            })
            .unwrap();
        }

        let count = repo
            .recover_interrupted_tasks("application restarted")
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            repo.get_task("idle-crash").unwrap().status,
            TaskStatus::Interrupted
        );
        assert_eq!(
            repo.get_task("idle-clean").unwrap().status,
            TaskStatus::Idle
        );
    }

    #[test]
    fn task_update_preserves_fields() {
        let repo = make_test_repo();
        let p = make_project("p1", "/test");
        repo.create_project(&p).unwrap();

        let mut t = make_task("t1", "p1", TaskStatus::Running);
        repo.create_task(&t).unwrap();

        t.status = TaskStatus::Archived;
        t.updated_at = utc_now();
        repo.update_task(&t).unwrap();

        let got = repo.get_task("t1").unwrap();
        assert_eq!(got.status, TaskStatus::Archived);
        assert_eq!(got.title, "Task t1"); // unchanged
    }

    #[test]
    fn settings_crud() {
        let repo = make_test_repo();
        let s = Settings {
            key: "theme".into(),
            json_value: serde_json::json!({"mode": "dark"}),
        };
        repo.set_setting(&s).unwrap();
        let got = repo.get_setting("theme").unwrap().unwrap();
        assert_eq!(got.key, "theme");
        assert_eq!(got.json_value["mode"], "dark");
    }

    #[test]
    fn foreign_key_cascade_blocked() {
        let repo = make_test_repo();
        // Deleting a project with tasks should fail (RESTRICT).
        let p = make_project("p1", "/test");
        repo.create_project(&p).unwrap();
        let t = make_task("t1", "p1", TaskStatus::Preparing);
        repo.create_task(&t).unwrap();

        let result = repo.delete_project("p1");
        assert!(result.is_err(), "ON DELETE RESTRICT should block");
    }

    // -------------------------------------------------------------------------
    // GAG-004 P1 regression tests — startup recovery & row corruption
    // -------------------------------------------------------------------------

    /// P1: when `recover_interrupted_tasks` fails (e.g. SQLite trigger or
    /// lock prevents the UPDATE), the bootstrap snapshot returned by the
    /// bridge MUST NOT be `ready`. Otherwise the UI would render ShellView
    /// with tasks still showing `Running` despite no live process.
    #[test]
    fn bootstrap_impl_not_ready_when_recovery_fails() {
        let repo = make_test_repo();
        let p = make_project("p1", "/test");
        repo.create_project(&p).unwrap();
        let t = make_task("t1", "p1", TaskStatus::Running);
        repo.create_task(&t).unwrap();

        // Install a trigger that aborts any UPDATE transitioning a task's
        // status to 'interrupted' — simulating a locked / read-only DB.
        {
            let conn = repo.conn.lock().unwrap();
            conn.execute_batch(
                "CREATE TRIGGER block_recovery
                 BEFORE UPDATE OF status ON tasks
                 WHEN NEW.status = 'interrupted'
                 BEGIN
                   SELECT RAISE(ABORT, 'recovery blocked by trigger');
                 END;",
            )
            .expect("trigger installed");
        }

        // Recovery fails because the trigger aborts the UPDATE.
        let recovery_result = repo.recover_interrupted_tasks("app exited");
        assert!(
            recovery_result.is_err(),
            "recover_interrupted_tasks must fail when trigger blocks UPDATE"
        );

        // The task remains Running in the DB — we cannot fix SQLite-level
        // failures, so the bridge must surface a non-ready bootstrap instead.
        let task_after = repo.get_task("t1").unwrap();
        assert_eq!(
            task_after.status,
            TaskStatus::Running,
            "task stays Running when recovery UPDATE was blocked"
        );

        // The startup wiring (lib.rs) sets `db_init_error` when recovery
        // fails; `bootstrap_impl` honours that and returns ready=false.
        // Here we pass that flag directly to verify the bridge behaviour.
        let snap = crate::bridge::dispatch::bootstrap_impl(
            &repo,
            Some("Startup recovery failed (DB_QUERY_FAILED)."),
        );
        assert!(
            !snap.ready,
            "bootstrap must NOT be ready when recovery failed"
        );
        assert!(
            snap.db_error.is_some(),
            "dbError must be surfaced to the UI so ShellView is not rendered"
        );
    }

    /// P1: a row with a BLOB stored in a TEXT column (corrupted data) must
    /// make `bootstrap_snapshot()` return `DB_QUERY_FAILED` instead of
    /// silently dropping the row and returning `Ok` with `projects: []`.
    /// Before the fix, `filter_map(|r| r.ok())` swallowed the
    /// `InvalidColumnType` and masqueraded corruption as data deletion.
    #[test]
    fn bootstrap_snapshot_fails_on_corrupted_blob_row() {
        let repo = make_test_repo();
        let p = make_project("p1", "/test");
        repo.create_project(&p).unwrap();

        // Corrupt `display_path` by writing a BLOB into the TEXT column.
        // SQLite allows dynamic-typed storage; rusqlite's `get::<_, String>`
        // then fails with `InvalidColumnType`.
        {
            let conn = repo.conn.lock().unwrap();
            conn.execute(
                "UPDATE projects SET display_path = X'80' WHERE id = 'p1'",
                [],
            )
            .expect("corruption UPDATE applied");
        }

        let result = repo.bootstrap_snapshot();
        assert!(
            result.is_err(),
            "bootstrap_snapshot must fail when a row cannot be decoded"
        );
        let err = result.unwrap_err();
        assert_eq!(
            err.code,
            crate::domain::error::codes::DB_QUERY_FAILED,
            "error code must be DB_QUERY_FAILED, got: {}",
            err.code
        );
    }
}
