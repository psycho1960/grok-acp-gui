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
    utc_now, AttachmentId, AttachmentRecord, BootstrapSnapshot, Project, ProjectId, RecoveryId,
    RecoveryItem, RecoveryState, SessionBinding, SessionId, SessionState, Settings, Task, TaskId,
    TaskStatus, WorkspaceKind, WorktreeId, WorktreeOwnership, WorktreeRecord, WorktreeState,
};
use crate::modules::persistence::{RepoResult, Repository};
use rusqlite::{params, Connection};
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

        // Enable WAL mode and foreign keys.
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA foreign_keys=ON;",
        )
        .map_err(|e| db_error("Failed to set pragmas", &e))?;

        migration::run_migrations_transactional(&mut conn)
            .map_err(|e| DomainError::new("DB_MIGRATION_FAILED", e.to_string()))?;

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
    })
}

fn row_to_binding(row: &rusqlite::Row) -> rusqlite::Result<SessionBinding> {
    Ok(SessionBinding {
        task_id: TaskId::new(row.get::<_, String>(0)?),
        session_id: SessionId::new(row.get::<_, String>(1)?),
        cwd: row.get(2)?,
        last_seq: row.get(3)?,
        state: parse_session_state(&row.get::<_, String>(4)?),
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

fn row_to_setting(row: &rusqlite::Row) -> rusqlite::Result<Settings> {
    let json_str: String = row.get(1)?;
    let json_value: serde_json::Value =
        serde_json::from_str(&json_str).unwrap_or(serde_json::Value::Null);
    Ok(Settings {
        key: row.get(0)?,
        json_value,
    })
}

// ---------------------------------------------------------------------------
// Enum parsers
// ---------------------------------------------------------------------------

fn parse_task_status(s: &str) -> TaskStatus {
    match s {
        "preparing" => TaskStatus::Preparing,
        "running" => TaskStatus::Running,
        "waiting_permission" => TaskStatus::WaitingPermission,
        "integrating" => TaskStatus::Integrating,
        "merged" => TaskStatus::Merged,
        "archived" => TaskStatus::Archived,
        "interrupted" => TaskStatus::Interrupted,
        _ => TaskStatus::Interrupted,
    }
}

fn task_status_to_str(s: TaskStatus) -> &'static str {
    match s {
        TaskStatus::Preparing => "preparing",
        TaskStatus::Running => "running",
        TaskStatus::WaitingPermission => "waiting_permission",
        TaskStatus::Integrating => "integrating",
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
        "ready" => WorktreeState::Ready,
        "dirty" => WorktreeState::Dirty,
        "integrating" => WorktreeState::Integrating,
        "deleted" => WorktreeState::Deleted,
        _ => WorktreeState::Unknown,
    }
}

fn worktree_state_to_str(s: WorktreeState) -> &'static str {
    match s {
        WorktreeState::Ready => "ready",
        WorktreeState::Dirty => "dirty",
        WorktreeState::Integrating => "integrating",
        WorktreeState::Deleted => "deleted",
        WorktreeState::Unknown => "unknown",
    }
}

fn parse_worktree_ownership(s: &str) -> WorktreeOwnership {
    match s {
        "external" => WorktreeOwnership::External,
        _ => WorktreeOwnership::Managed,
    }
}

fn worktree_ownership_to_str(o: WorktreeOwnership) -> &'static str {
    match o {
        WorktreeOwnership::Managed => "managed",
        WorktreeOwnership::External => "external",
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
                    "SELECT id, project_id, title, status, workspace_kind, mode, model, reasoning, created_at, updated_at, interrupt_reason FROM tasks WHERE status NOT IN ('merged', 'archived') ORDER BY updated_at DESC",
                )
                .map_err(|e| db_error("bootstrap: tasks query", &e))?;
            let rows = stmt
                .query_map([], row_to_task)
                .map_err(|e| db_error("bootstrap: tasks map", &e))?;
            collect_rows(rows, "bootstrap: tasks row")?
        };

        let bindings = {
            let mut stmt = conn
                .prepare("SELECT task_id, session_id, cwd, last_seq, state FROM session_bindings")
                .map_err(|e| db_error("bootstrap: bindings query", &e))?;
            let rows = stmt
                .query_map([], row_to_binding)
                .map_err(|e| db_error("bootstrap: bindings map", &e))?;
            collect_rows(rows, "bootstrap: bindings row")?
        };

        let worktrees = {
            let mut stmt = conn
                .prepare(
                    "SELECT id, task_id, repo_root, path, display_path, branch, base_branch, base_commit, ownership, state FROM worktrees WHERE state != 'deleted'",
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
            bindings,
            worktrees,
            recovery_items,
            settings,
            recovery_performed: false,
            tasks_interrupted: 0,
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
            "INSERT INTO tasks (id, project_id, title, status, workspace_kind, mode, model, reasoning, created_at, updated_at, interrupt_reason) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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
            ],
        )
        .map_err(|e| db_error("create_task", &e))?;
        Ok(())
    }

    fn get_task(&self, id: &str) -> RepoResult<Task> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT id, project_id, title, status, workspace_kind, mode, model, reasoning, created_at, updated_at, interrupt_reason FROM tasks WHERE id = ?1",
            params![id],
            row_to_task,
        )
        .map_err(|e| db_error("get_task", &e))
    }

    fn list_tasks_by_project(&self, project_id: &str) -> RepoResult<Vec<Task>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, title, status, workspace_kind, mode, model, reasoning, created_at, updated_at, interrupt_reason FROM tasks WHERE project_id = ?1 ORDER BY updated_at DESC",
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
                "SELECT id, project_id, title, status, workspace_kind, mode, model, reasoning, created_at, updated_at, interrupt_reason FROM tasks WHERE status NOT IN ('merged', 'archived') ORDER BY updated_at DESC",
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
            "UPDATE tasks SET project_id = ?1, title = ?2, status = ?3, workspace_kind = ?4, mode = ?5, model = ?6, reasoning = ?7, updated_at = ?8, interrupt_reason = ?9 WHERE id = ?10",
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
                task.id.0,
            ],
        )
        .map_err(|e| db_error("update_task", &e))?;
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

    // ==================================================================
    // Session Bindings
    // ==================================================================

    fn create_binding(&self, binding: &SessionBinding) -> RepoResult<()> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO session_bindings (task_id, session_id, cwd, last_seq, state) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                binding.task_id.0,
                binding.session_id.0,
                binding.cwd,
                binding.last_seq,
                session_state_to_str(binding.state),
            ],
        )
        .map_err(|e| db_error("create_binding", &e))?;
        Ok(())
    }

    fn get_binding_by_task(&self, task_id: &str) -> RepoResult<Option<SessionBinding>> {
        let conn = self.lock()?;
        let result = conn.query_row(
            "SELECT task_id, session_id, cwd, last_seq, state FROM session_bindings WHERE task_id = ?1",
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
            "SELECT task_id, session_id, cwd, last_seq, state FROM session_bindings WHERE session_id = ?1",
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
            "UPDATE session_bindings SET cwd = ?1, last_seq = ?2, state = ?3 WHERE task_id = ?4",
            params![
                binding.cwd,
                binding.last_seq,
                session_state_to_str(binding.state),
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
                "SELECT task_id, session_id, cwd, last_seq, state FROM session_bindings WHERE state != 'closed'",
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
            "INSERT INTO worktrees (id, task_id, repo_root, path, display_path, branch, base_branch, base_commit, ownership, state) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
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
            ],
        )
        .map_err(|e| db_error("create_worktree", &e))?;
        Ok(())
    }

    fn get_worktree(&self, id: &str) -> RepoResult<WorktreeRecord> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT id, task_id, repo_root, path, display_path, branch, base_branch, base_commit, ownership, state FROM worktrees WHERE id = ?1",
            params![id],
            row_to_worktree,
        )
        .map_err(|e| db_error("get_worktree", &e))
    }

    fn list_worktrees_by_task(&self, task_id: &str) -> RepoResult<Vec<WorktreeRecord>> {
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, task_id, repo_root, path, display_path, branch, base_branch, base_commit, ownership, state FROM worktrees WHERE task_id = ?1",
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
            "UPDATE worktrees SET task_id = ?1, repo_root = ?2, path = ?3, display_path = ?4, branch = ?5, base_branch = ?6, base_commit = ?7, ownership = ?8, state = ?9 WHERE id = ?10",
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
                "SELECT id, task_id, repo_root, path, display_path, branch, base_branch, base_commit, ownership, state FROM worktrees WHERE state != 'deleted'",
            )
            .map_err(|e| db_error("list_active_worktrees", &e))?;
        let rows = stmt
            .query_map([], row_to_worktree)
            .map_err(|e| db_error("list_active_worktrees", &e))?;
        collect_rows(rows, "list_active_worktrees row")
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
    // Startup Recovery
    // ==================================================================

    fn recover_interrupted_tasks(&self, reason: &str) -> RepoResult<u32> {
        let conn = self.lock()?;
        let now = utc_now();
        let count = conn
            .execute(
                "UPDATE tasks SET status = 'interrupted', interrupt_reason = ?1, updated_at = ?2 WHERE status IN ('running', 'waiting_permission', 'integrating')",
                params![reason, now],
            )
            .map_err(|e| db_error("recover_interrupted_tasks", &e))?;
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
        };
        repo.create_binding(&b1).unwrap();

        let b2 = SessionBinding {
            task_id: TaskId::new("t1"),
            session_id: SessionId::new("s1"), // duplicate
            cwd: None,
            last_seq: 0,
            state: SessionState::Active,
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

        let snap = repo.bootstrap_snapshot().unwrap();
        assert_eq!(snap.projects.len(), 1);
        assert_eq!(snap.active_tasks.len(), 1);
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
