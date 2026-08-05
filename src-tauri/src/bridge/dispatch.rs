//! Bridge command dispatch and Tauri event channel.
//!
//! The `execute` Tauri command accepts raw JSON and deserialises inside
//! the dispatcher so that unknown / malformed commands produce a stable
//! `BRIDGE_UNSUPPORTED_COMMAND` error instead of a Tauri-level panic.
//!
//! Events are emitted on a single Tauri event channel (`bridge:event`).
//! Session-scoped events are constructed via `SessionEvent` which
//! enforces `taskId`, `sessionId`, and `seq`; non-session events use
//! `DesktopEvent::new`.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use super::error::AppError;
use super::events::DesktopEvent;
use crate::domain;
use crate::modules::agent_runtime::{AgentEvent, RuntimeConfig, TimestampedEvent};
use crate::modules::persistence::Repository;
use crate::modules::task_runtime::TaskRuntime;

/// Wrapper that the `execute` Tauri command returns.
///
/// This is NOT a Rust `Result` — both success and failure are represented
/// as variants so the frontend always receives a valid JSON response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(tag = "success")]
pub enum DesktopResult {
    #[serde(rename = "true")]
    Ok { data: serde_json::Value },
    #[serde(rename = "false")]
    Err { error: AppError },
}

impl DesktopResult {
    pub fn ok(data: impl Serialize) -> Self {
        Self::Ok {
            data: serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
        }
    }

    pub fn err(error: AppError) -> Self {
        Self::Err { error }
    }
}

/// The single Tauri command channel for events.
pub const EVENT_CHANNEL: &str = "bridge:event";

/// Max JSON payload size (1 MiB) before validation rejects.
const MAX_PAYLOAD_BYTES: u64 = 1_048_576;

/// Implementation of the `bootstrap` command (called from lib.rs).
pub fn bootstrap_impl(repo: &dyn Repository, db_init_error: Option<&str>) -> BootstrapSnapshot {
    let product_name = "Grok ACP GUI".to_string();
    let version = env!("CARGO_PKG_VERSION").to_string();
    let platform = std::env::consts::OS.to_string();

    // If the database couldn't even open, surface that immediately.
    if let Some(err) = db_init_error {
        return BootstrapSnapshot {
            product_name,
            version,
            platform,
            ready: false,
            db_error: Some(err.to_string()),
            ..empty_bootstrap()
        };
    }

    // Grok Build model profiles are user-owned configuration. Read them once
    // for the capability snapshot; errors intentionally degrade to no choices.
    let configured_models = crate::modules::agent_runtime::configured_models()
        .into_iter()
        .map(|model| ModelInfo {
            model_id: model.id,
            name: model.name,
            description: None,
            reasoning_effort: model.reasoning_effort,
        })
        .collect();

    // Load domain entities from persistence.
    match repo.bootstrap_snapshot() {
        Ok(domain_snap) => BootstrapSnapshot {
            product_name: domain_snap.product_name,
            version: domain_snap.version,
            platform: domain_snap.platform,
            ready: true,
            db_error: None,
            runtime: RuntimeBootstrapStatus {
                status: "unavailable".into(),
                probe_error: Some("Runtime module not yet wired (GAG-005)".into()),
                version: None,
                authenticated: None,
            },
            capabilities: CapabilitySnapshot {
                models: configured_models,
                modes: vec![],
                slash_commands: vec![],
                model_state: None,
                mode_state: None,
            },
            // Domain entities from persistence
            projects: domain_snap.projects,
            active_tasks: domain_snap.active_tasks,
            bindings: domain_snap.bindings,
            worktrees: domain_snap.worktrees,
            recovery_items: domain_snap.recovery_items,
            settings: domain_snap.settings,
            recovery_performed: domain_snap.recovery_performed,
            tasks_interrupted: domain_snap.tasks_interrupted,
            recovery_candidates: domain_snap.recovery_candidates,
            concurrency: domain_snap.concurrency,
        },
        Err(e) => {
            eprintln!(
                "bootstrap: database query failed ({}): {}",
                e.code, e.message
            );
            BootstrapSnapshot {
                product_name,
                version,
                platform,
                ready: false,
                db_error: Some(format!(
                    "Cannot read application data ({}). {}",
                    e.code, e.message
                )),
                ..empty_bootstrap()
            }
        }
    }
}

/// Returns a BootstrapSnapshot with all optional fields empty/default.
fn empty_bootstrap() -> BootstrapSnapshot {
    BootstrapSnapshot {
        product_name: String::new(),
        version: String::new(),
        platform: String::new(),
        ready: false,
        db_error: None,
        runtime: RuntimeBootstrapStatus {
            status: "unavailable".into(),
            probe_error: None,
            version: None,
            authenticated: None,
        },
        capabilities: CapabilitySnapshot {
            models: vec![],
            modes: vec![],
            slash_commands: vec![],
            model_state: None,
            mode_state: None,
        },
        projects: vec![],
        active_tasks: vec![],
        bindings: vec![],
        worktrees: vec![],
        recovery_items: vec![],
        settings: vec![],
        recovery_performed: false,
        tasks_interrupted: 0,
        recovery_candidates: vec![],
        concurrency: None,
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapSnapshot {
    pub product_name: String,
    pub version: String,
    pub platform: String,
    pub ready: bool,
    /// Non-empty when the database is unavailable or corrupt.
    /// The Renderer should show `UI-ERROR-001` and disable data-dependent features.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub db_error: Option<String>,
    pub runtime: RuntimeBootstrapStatus,
    pub capabilities: CapabilitySnapshot,
    // Domain entities (GAG-004)
    pub projects: Vec<domain::types::Project>,
    pub active_tasks: Vec<domain::types::Task>,
    pub bindings: Vec<domain::types::SessionBinding>,
    pub worktrees: Vec<domain::types::WorktreeRecord>,
    pub recovery_items: Vec<domain::types::RecoveryItem>,
    pub settings: Vec<domain::types::Settings>,
    pub recovery_performed: bool,
    pub tasks_interrupted: u32,
    // GAG-006: Recovery candidates and concurrency
    pub recovery_candidates: Vec<domain::types::RecoveryCandidate>,
    pub concurrency: Option<domain::types::ConcurrencyLimits>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeBootstrapStatus {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probe_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authenticated: Option<bool>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitySnapshot {
    pub models: Vec<ModelInfo>,
    pub modes: Vec<ModeInfo>,
    pub slash_commands: Vec<SlashCommandInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_state: Option<SessionModelState>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode_state: Option<SessionModeState>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionModelState {
    pub current_model_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionModeState {
    pub current_mode_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub model_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeInfo {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlashCommandInfo {
    pub name: String,
    pub description: String,
    pub accepts_input: bool,
}

/// Implementation of the `execute` command (called from lib.rs).
///
/// Accepts raw JSON so that unknown / malformed types produce a stable
/// error.  The dispatch classifies errors:
/// - Unknown `type` -> `BRIDGE_UNSUPPORTED_COMMAND`
/// - Recognised `type` but invalid payload -> `BRIDGE_INVALID_PAYLOAD`
pub async fn execute_impl(
    repo: &dyn Repository,
    runtime: &dyn crate::modules::agent_runtime::AgentRuntime,
    task_runtime: &dyn TaskRuntime,
    raw: serde_json::Value,
) -> DesktopResult {
    // Reject oversized payloads before any deserialization.
    if let Ok(serialized) = serde_json::to_string(&raw) {
        if serialized.len() as u64 > MAX_PAYLOAD_BYTES {
            return DesktopResult::err(AppError::new(
                domain::error::codes::BRIDGE_INVALID_PAYLOAD,
                "Command payload exceeds maximum size (1 MiB)",
            ));
        }
    }

    // Peek at `type` before full deser to classify the error correctly.
    let cmd_type = raw
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    if cmd_type.is_empty() {
        return DesktopResult::err(AppError::new(
            domain::error::codes::BRIDGE_UNSUPPORTED_COMMAND,
            "Command missing required 'type' field",
        ));
    }

    let is_known = super::commands::is_known_command(&cmd_type);

    let cmd: super::commands::DesktopCommand = match serde_json::from_value(raw) {
        Ok(cmd) => cmd,
        Err(e) => {
            let code = if is_known {
                domain::error::codes::BRIDGE_INVALID_PAYLOAD
            } else {
                domain::error::codes::BRIDGE_UNSUPPORTED_COMMAND
            };
            return DesktopResult::err(AppError::new(code, format!("Command error: {}", e)));
        }
    };

    // Validate the parsed command before dispatching.
    if let Err(err) = super::commands::validate(&cmd) {
        return DesktopResult::err(err);
    }

    dispatch(repo, runtime, task_runtime, cmd).await
}

use super::commands::DesktopCommand;

async fn dispatch(
    repo: &dyn Repository,
    runtime: &dyn crate::modules::agent_runtime::AgentRuntime,
    task_runtime: &dyn TaskRuntime,
    cmd: DesktopCommand,
) -> DesktopResult {
    match &cmd {
        DesktopCommand::RuntimeRefresh(_) => runtime_refresh(runtime).await,
        DesktopCommand::RuntimeLogin(_) => not_implemented("runtime.login"),

        DesktopCommand::ProjectOpen(payload) => project_open(repo, payload),
        DesktopCommand::ProjectForget(_) => not_implemented("project.forget"),

        DesktopCommand::TaskCreate(payload) => {
            task_create(repo, runtime, task_runtime, payload).await
        }
        DesktopCommand::TaskOpen(payload) => task_open(repo, task_runtime, payload).await,
        DesktopCommand::TaskArchive(_) => not_implemented("task.archive"),

        DesktopCommand::TurnSend(payload) => turn_send(repo, runtime, task_runtime, payload).await,
        DesktopCommand::TurnCancel(payload) => {
            turn_cancel(repo, runtime, task_runtime, payload).await
        }
        DesktopCommand::SessionConfigure(_) => not_implemented("session.configure"),
        DesktopCommand::SessionResume(payload) => session_resume(repo, task_runtime, payload).await,

        DesktopCommand::PermissionResolve(payload) => {
            permission_resolve(task_runtime, payload).await
        }
        DesktopCommand::PlanResolve(payload) => plan_resolve(task_runtime, payload).await,

        DesktopCommand::ArtifactImport(_) => not_implemented("artifact.import"),
        DesktopCommand::ArtifactSave(_) => not_implemented("artifact.save"),

        DesktopCommand::WorkspaceInspect(payload) => workspace_inspect(payload),
        DesktopCommand::WorktreeAdopt(_) => not_implemented("worktree.adopt"),

        DesktopCommand::ReviewDiff(_) => not_implemented("review.diff"),
        DesktopCommand::ReviewCheckpoint(_) => not_implemented("review.checkpoint"),

        DesktopCommand::IntegrationPreflight(_) => not_implemented("integration.preflight"),
        DesktopCommand::IntegrationExecute(_) => not_implemented("integration.execute"),

        DesktopCommand::WorktreeCleanup(_) => not_implemented("worktree.cleanup"),

        DesktopCommand::RecoveryRestore(_) => not_implemented("recovery.restore"),
        DesktopCommand::RecoveryDelete(_) => not_implemented("recovery.delete"),
    }
}

async fn permission_resolve(
    task_runtime: &dyn TaskRuntime,
    payload: &super::commands::PermissionResolvePayload,
) -> DesktopResult {
    let request = crate::modules::task_runtime::permission::PermissionResolutionRequest {
        task_id: payload.task_id.clone(),
        session_id: payload.session_id.clone(),
        request_id: payload.request_id.clone(),
        correlation_id: payload.correlation_id.clone(),
        expected_version: payload.expected_version,
        option_id: payload.option_id.clone(),
    };
    match task_runtime.resolve_permission(request).await {
        Ok(state) => DesktopResult::ok(serde_json::json!({
            "requestId": payload.request_id,
            "state": state,
        })),
        Err(error) => DesktopResult::err(AppError::new(error.code, error.message)),
    }
}

async fn plan_resolve(
    task_runtime: &dyn TaskRuntime,
    payload: &super::commands::PlanResolvePayload,
) -> DesktopResult {
    let request = crate::modules::task_runtime::plan::PlanResolutionRequest {
        task_id: payload.task_id.clone(),
        session_id: payload.session_id.clone(),
        request_id: payload.request_id.clone(),
        correlation_id: payload.correlation_id.clone(),
        expected_version: payload.expected_version,
        option_id: payload.option_id.clone(),
    };
    match task_runtime.resolve_plan(request).await {
        Ok(state) => DesktopResult::ok(serde_json::json!({
            "requestId": payload.request_id,
            "state": state,
        })),
        Err(error) => DesktopResult::err(AppError::new(error.code, error.message)),
    }
}

/// Wire `runtime.refresh` to the AgentRuntime probe.
async fn runtime_refresh(
    runtime: &dyn crate::modules::agent_runtime::AgentRuntime,
) -> DesktopResult {
    let config = RuntimeConfig::default();
    let result = runtime.probe(&config).await;
    DesktopResult::ok(serde_json::to_value(&result).unwrap_or(serde_json::Value::Null))
}

/// Inspect a workspace path: exists as directory, discover git root if any.
fn workspace_inspect(payload: &super::commands::WorkspaceInspectPayload) -> DesktopResult {
    let path = payload.path.trim();
    if path.is_empty() {
        return DesktopResult::err(
            AppError::new(
                domain::error::codes::BRIDGE_VALIDATION_FAILED,
                "Path is required",
            )
            .with_action("Choose a folder or enter a valid absolute path."),
        );
    }

    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => {
            return DesktopResult::err(
                AppError::new(
                    domain::error::codes::BRIDGE_VALIDATION_FAILED,
                    "Directory does not exist or is not accessible",
                )
                .with_action("Check the path and try again."),
            );
        }
    };
    if !meta.is_dir() {
        return DesktopResult::err(
            AppError::new(
                domain::error::codes::BRIDGE_VALIDATION_FAILED,
                "Path is not a directory",
            )
            .with_action("Select a folder, not a file."),
        );
    }

    let repo_root = find_git_root(std::path::Path::new(path));
    let branch = repo_root
        .as_ref()
        .and_then(|r| read_git_branch(std::path::Path::new(r)))
        .unwrap_or_else(|| "unknown".into());

    DesktopResult::ok(serde_json::json!({
        "repoRoot": repo_root.clone().unwrap_or_else(|| path.to_string()),
        "branch": branch,
        "dirty": false,
        "isGit": repo_root.is_some(),
    }))
}

/// Open (or re-open) a project directory and persist it.
/// Non-git directories are accepted with nonGit=true (Ask/Agent without Worktree).
fn project_open(
    repo: &dyn Repository,
    payload: &super::commands::ProjectOpenPayload,
) -> DesktopResult {
    use crate::domain::types::{utc_now, Project, ProjectId};
    use uuid::Uuid;

    let path = payload.path.trim();
    if path.is_empty() {
        return DesktopResult::err(
            AppError::new(
                domain::error::codes::BRIDGE_VALIDATION_FAILED,
                "Path is required",
            )
            .with_action("Choose a folder or enter a valid absolute path."),
        );
    }

    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => {
            return DesktopResult::err(
                AppError::new(
                    domain::error::codes::BRIDGE_VALIDATION_FAILED,
                    "Directory does not exist or is not accessible",
                )
                .with_action("Check the path and try again."),
            );
        }
    };
    if !meta.is_dir() {
        return DesktopResult::err(
            AppError::new(
                domain::error::codes::BRIDGE_VALIDATION_FAILED,
                "Path is not a directory",
            )
            .with_action("Select a folder, not a file."),
        );
    }

    let canonical = std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string());
    // Strip Windows \\?\ prefix for display
    let normalized = canonical
        .strip_prefix(r"\\?\")
        .unwrap_or(&canonical)
        .to_string();

    let repo_root = find_git_root(std::path::Path::new(&normalized));
    let non_git = repo_root.is_none();
    let now = utc_now();
    let display_path = display_path_for(&normalized);

    // Re-open existing project by path if present
    if let Ok(existing) = repo.list_projects() {
        if let Some(found) = existing
            .iter()
            .find(|p| paths_equal(&p.path, &normalized) || paths_equal(&p.path, path))
        {
            let mut updated = found.clone();
            updated.last_opened_at = now.clone();
            updated.repo_root = repo_root.clone();
            updated.display_path = display_path.clone();
            if updated.trusted_at.is_none() {
                updated.trusted_at = Some(now.clone());
            }
            if let Err(e) = repo.update_project(&updated) {
                return DesktopResult::err(AppError::new(e.code, e.message));
            }
            return DesktopResult::ok(serde_json::json!({
                "projectId": updated.id.0,
                "path": updated.path,
                "displayPath": updated.display_path,
                "repoRoot": updated.repo_root,
                "nonGit": non_git,
            }));
        }
    }

    let project = Project {
        id: ProjectId::new(format!("proj-{}", Uuid::new_v4())),
        path: normalized.clone(),
        display_path,
        repo_root: repo_root.clone(),
        trusted_at: Some(now.clone()),
        last_opened_at: now,
    };

    match repo.create_project(&project) {
        Ok(()) => {}
        Err(e) => {
            // Unique path race — try list again
            if e.code == domain::error::codes::PROJECT_ALREADY_EXISTS {
                if let Ok(list) = repo.list_projects() {
                    if let Some(found) = list.iter().find(|p| paths_equal(&p.path, &normalized)) {
                        return DesktopResult::ok(serde_json::json!({
                            "projectId": found.id.0,
                            "path": found.path,
                            "displayPath": found.display_path,
                            "repoRoot": found.repo_root,
                            "nonGit": non_git,
                        }));
                    }
                }
            }
            return DesktopResult::err(AppError::new(e.code, e.message));
        }
    }

    DesktopResult::ok(serde_json::json!({
        "projectId": project.id.0,
        "path": project.path,
        "displayPath": project.display_path,
        "repoRoot": project.repo_root,
        "nonGit": non_git,
    }))
}

fn find_git_root(start: &std::path::Path) -> Option<String> {
    let mut current = Some(start);
    while let Some(dir) = current {
        let git = dir.join(".git");
        if git.exists() {
            return Some(dir.to_string_lossy().to_string());
        }
        current = dir.parent();
    }
    None
}

fn read_git_branch(repo_root: &std::path::Path) -> Option<String> {
    let head = repo_root.join(".git").join("HEAD");
    let content = std::fs::read_to_string(head).ok()?;
    let content = content.trim();
    if let Some(rest) = content.strip_prefix("ref: refs/heads/") {
        return Some(rest.to_string());
    }
    if content.len() >= 7 {
        return Some(content[..7].to_string());
    }
    Some("HEAD".into())
}

fn display_path_for(path: &str) -> String {
    // Prefer last two path segments for UI.
    let normalized = path.replace('\\', "/");
    let parts: Vec<&str> = normalized.split('/').filter(|s| !s.is_empty()).collect();
    if parts.len() >= 2 {
        format!("{}/{}", parts[parts.len() - 2], parts[parts.len() - 1])
    } else if let Some(last) = parts.last() {
        (*last).to_string()
    } else {
        path.to_string()
    }
}

fn paths_equal(a: &str, b: &str) -> bool {
    let na = a
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase();
    let nb = b
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase();
    na == nb
}

/// Wire `task.create` to the persistence layer.
async fn task_create(
    repo: &dyn Repository,
    runtime: &dyn crate::modules::agent_runtime::AgentRuntime,
    task_runtime: &dyn TaskRuntime,
    payload: &super::commands::TaskCreatePayload,
) -> DesktopResult {
    use crate::domain::types::{utc_now, Task, TaskId, TaskStatus, WorkspaceKind};
    use uuid::Uuid;

    if payload.prompt.trim().is_empty() {
        return DesktopResult::err(
            AppError::new(
                domain::error::codes::BRIDGE_VALIDATION_FAILED,
                "Task prompt is required",
            )
            .with_action("Enter a task goal before creating."),
        );
    }
    if payload.title.trim().is_empty() {
        return DesktopResult::err(
            AppError::new(
                domain::error::codes::BRIDGE_VALIDATION_FAILED,
                "Task title is required",
            )
            .with_action("Provide a title or a non-empty prompt."),
        );
    }

    // Ensure project exists
    if repo.get_project(&payload.project_id.0).is_err() {
        return DesktopResult::err(
            AppError::new(domain::error::codes::PROJECT_NOT_FOUND, "Project not found")
                .with_action("Open a project before creating a task."),
        );
    }

    let workspace_kind = match payload.workspace_strategy.as_deref() {
        Some("direct") => WorkspaceKind::Direct,
        Some("readonly") => WorkspaceKind::Readonly,
        _ => WorkspaceKind::Worktree,
    };

    let now = utc_now();
    let task = Task {
        id: TaskId::new(format!("task-{}", Uuid::new_v4())),
        project_id: payload.project_id.clone(),
        title: payload.title.clone(),
        status: TaskStatus::Preparing,
        workspace_kind,
        mode: payload.mode.clone(),
        model: payload.model.clone(),
        reasoning: payload.reasoning.clone(),
        created_at: now.clone(),
        updated_at: now,
        interrupt_reason: None,
        interrupted_at: None,
        attempt_count: 1,
    };

    match repo.create_task(&task) {
        Ok(()) => {}
        Err(e) => return DesktopResult::err(AppError::new(e.code, e.message)),
    }

    let task_data = serde_json::json!({
        "taskId": task.id.0,
        "task": {
            "id": task.id.0,
            "projectId": task.project_id.0,
            "title": task.title,
            "status": "preparing",
            "createdAt": task.created_at,
        }
    });

    // FR-TASK-001 defines the creation prompt as the first user turn.
    // Keep the task visible even when process startup fails and return a
    // structured error field so the Renderer can offer recovery.
    let initial_turn = super::commands::TurnSendPayload {
        task_id: task.id.clone(),
        message: payload.prompt.clone(),
        attachments: payload.attachments.clone(),
    };
    match turn_send(repo, runtime, task_runtime, &initial_turn).await {
        DesktopResult::Ok { data } => {
            let mut result = task_data;
            result["turn"] = data;
            result["task"]["status"] = serde_json::Value::String("running".into());
            DesktopResult::ok(result)
        }
        DesktopResult::Err { error } => {
            let _ = repo.update_task_status(&task.id.0, "failed", Some(&error.message));
            let mut result = task_data;
            result["task"]["status"] = serde_json::Value::String("failed".into());
            result["startError"] = serde_json::to_value(error).unwrap_or_default();
            DesktopResult::ok(result)
        }
    }
}

async fn ensure_task_session(
    repo: &dyn Repository,
    task_runtime: &dyn TaskRuntime,
    task_id: &crate::bridge::types::TaskId,
) -> Result<crate::bridge::types::SessionId, AppError> {
    use uuid::Uuid;

    // Validate the task before creating any process or binding side effects.
    repo.get_task(&task_id.0)
        .map_err(|error| AppError::new(error.code, error.message))?;

    let session_id = if let Some(binding) = repo
        .get_binding_by_task(&task_id.0)
        .map_err(|error| AppError::new(error.code, error.message))?
    {
        binding.session_id
    } else {
        let session_id =
            crate::bridge::types::SessionId::new(format!("session-{}", Uuid::new_v4()));
        task_runtime
            .enqueue_task(task_id.clone(), session_id.clone())
            .await
            .map_err(|error| AppError::new(error.code, error.message))?;
        session_id
    };

    task_runtime
        .start_session(task_id.clone(), session_id.clone())
        .await
        .map_err(|error| AppError::new(error.code, error.message))?;
    Ok(session_id)
}

async fn turn_send(
    repo: &dyn Repository,
    runtime: &dyn crate::modules::agent_runtime::AgentRuntime,
    task_runtime: &dyn TaskRuntime,
    payload: &super::commands::TurnSendPayload,
) -> DesktopResult {
    use crate::modules::agent_runtime::requests::PromptRequest;
    use crate::modules::agent_runtime::ClientRequest;

    let session_id = match ensure_task_session(repo, task_runtime, &payload.task_id).await {
        Ok(session_id) => session_id,
        Err(error) => return DesktopResult::err(error),
    };
    let task = match repo.get_task(&payload.task_id.0) {
        Ok(task) => task,
        Err(error) => return DesktopResult::err(AppError::new(error.code, error.message)),
    };
    if let Err(error) = repo.update_task_status(&payload.task_id.0, "running", None) {
        return DesktopResult::err(AppError::new(error.code, error.message));
    }

    match runtime
        .send(
            session_id,
            ClientRequest::Prompt(PromptRequest {
                message: payload.message.clone(),
                attachments: payload.attachments.clone().unwrap_or_default(),
                mode: task.mode,
                model: task.model,
                reasoning: task.reasoning,
            }),
        )
        .await
    {
        Ok(ack) => DesktopResult::ok(ack),
        Err(error) => {
            let _ = repo.update_task_status(&payload.task_id.0, "failed", Some(&error.message));
            DesktopResult::err(AppError::new(error.code, error.message))
        }
    }
}

async fn turn_cancel(
    repo: &dyn Repository,
    runtime: &dyn crate::modules::agent_runtime::AgentRuntime,
    task_runtime: &dyn TaskRuntime,
    payload: &super::commands::TurnCancelPayload,
) -> DesktopResult {
    let binding = match repo.get_binding_by_task(&payload.task_id.0) {
        Ok(Some(binding)) => binding,
        Ok(None) => return DesktopResult::ok(serde_json::json!({ "cancelled": false })),
        Err(error) => return DesktopResult::err(AppError::new(error.code, error.message)),
    };
    runtime.cancel(binding.session_id, None).await;
    match task_runtime.cancel_session(payload.task_id.clone()).await {
        Ok(()) => DesktopResult::ok(serde_json::json!({ "cancelled": true })),
        Err(error) => DesktopResult::err(AppError::new(error.code, error.message)),
    }
}

async fn session_resume(
    repo: &dyn Repository,
    task_runtime: &dyn TaskRuntime,
    payload: &super::commands::SessionResumePayload,
) -> DesktopResult {
    let should_increment_attempt = match repo.get_binding_by_task(&payload.task_id.0) {
        Ok(binding) => binding.is_some_and(|binding| {
            binding.state == crate::domain::types::SessionState::Disconnected
        }),
        Err(error) => return DesktopResult::err(AppError::new(error.code, error.message)),
    };
    if should_increment_attempt {
        if let Err(error) = repo.increment_binding_attempt(&payload.task_id.0) {
            return DesktopResult::err(AppError::new(error.code, error.message));
        }
    }
    match ensure_task_session(repo, task_runtime, &payload.task_id).await {
        Ok(session_id) => {
            if let Err(error) = repo.update_task_status(&payload.task_id.0, "idle", None) {
                return DesktopResult::err(AppError::new(error.code, error.message));
            }
            DesktopResult::ok(serde_json::json!({ "sessionId": session_id }))
        }
        Err(error) => DesktopResult::err(error),
    }
}

async fn task_open(
    repo: &dyn Repository,
    task_runtime: &dyn TaskRuntime,
    payload: &super::commands::TaskOpenPayload,
) -> DesktopResult {
    use crate::modules::task_runtime::mailbox::map_stored_events_to_bridge_snapshot;

    let task = match repo.get_task(&payload.task_id.0) {
        Ok(task) => task,
        Err(error) => return DesktopResult::err(AppError::new(error.code, error.message)),
    };
    let status = serde_json::to_value(task.status)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| "interrupted".into());
    let Some(binding) = (match repo.get_binding_by_task(&payload.task_id.0) {
        Ok(binding) => binding,
        Err(error) => return DesktopResult::err(AppError::new(error.code, error.message)),
    }) else {
        return DesktopResult::ok(serde_json::json!({
            "taskId": task.id,
            "title": task.title,
            "status": status,
            "cursor": { "lastSeq": 0, "snapshotSeq": 0 },
            "events": [],
            "attempt": task.attempt_count,
        }));
    };

    let snapshot = match task_runtime
        .get_snapshot(task.id.clone(), binding.session_id.clone(), None)
        .await
    {
        Ok(snapshot) => snapshot,
        Err(error) => return DesktopResult::err(AppError::new(error.code, error.message)),
    };
    // The runtime snapshot window is deliberately bounded, but `task.open`
    // must restore every confirmed logical message. Read the append-only log
    // and compact consecutive streaming chunks before crossing the bridge.
    let stored_events = match repo.get_events_after(&binding.session_id.0, 0, u32::MAX) {
        Ok(events) => events,
        Err(error) => return DesktopResult::err(AppError::new(error.code, error.message)),
    };
    let events = map_stored_events_to_bridge_snapshot(&stored_events);
    DesktopResult::ok(serde_json::json!({
        "taskId": task.id,
        "sessionId": binding.session_id,
        "title": task.title,
        "status": status,
        "cursor": {
            "lastSeq": snapshot.last_seq,
            "snapshotSeq": snapshot.last_seq,
        },
        "events": events,
        "attempt": snapshot.attempt_number,
    }))
}

/// Not-implemented response — every unimplemented command returns a
/// BRIDGE_NOT_IMPLEMENTED error.  This prevents the Renderer from
/// misinterpreting a success response for an unimplemented path.
fn not_implemented(command_name: &str) -> DesktopResult {
    DesktopResult::err(
        AppError::new(
            domain::error::codes::BRIDGE_NOT_IMPLEMENTED,
            format!("Command '{}' is not yet implemented", command_name),
        )
        .with_action("This feature will be available in a future update."),
    )
}

/// Emit a bridge event on the single controlled channel.
///
/// The caller is responsible for setting `task_id`, `session_id`, and
/// `seq` correctly based on whether the event is session-scoped.
pub fn emit(app: &AppHandle, event: DesktopEvent) {
    let _ = app.emit(EVENT_CHANNEL, &event);
}

/// Map an internal `AgentEvent` (from the runtime module) to a
/// `DesktopEvent` (for the bridge channel).  Returns `None` for events
/// that have no bridge representation (e.g. internal diagnostics).
///
/// This is the ONLY transformation point between runtime events and
/// Renderer-visible events.  The Renderer never sees raw ACP messages.
pub fn map_agent_event(event: TimestampedEvent) -> Option<DesktopEvent> {
    let session_id = event.meta.session_id.clone();
    let seq = event.meta.sequence;

    match event.event {
        AgentEvent::UserMessage(p) => {
            let payload = serde_json::json!({
                "role": "user",
                "text": p.text,
            });
            Some(
                super::events::SessionEvent::new(
                    super::events::event_types::MESSAGE_DELTA,
                    super::types::TaskId::new(""),
                    session_id,
                    seq,
                    payload,
                )
                .build(),
            )
        }

        AgentEvent::SessionReady(p) => {
            // Emit a runtime.updated event (non-session) with the
            // session-ready info.  The Renderer uses this to update
            // the runtime status indicator.
            Some(DesktopEvent::non_session(
                super::events::event_types::RUNTIME_UPDATED,
                serde_json::to_value(&p).unwrap_or(serde_json::Value::Null),
            ))
        }

        AgentEvent::AssistantDelta(p) => {
            let payload = serde_json::to_value(&p).unwrap_or(serde_json::Value::Null);
            Some(
                super::events::SessionEvent::new(
                    super::events::event_types::MESSAGE_DELTA,
                    // task_id is not known at the runtime layer; the
                    // session layer (GAG-006) will map session_id → task_id.
                    super::types::TaskId::new(""),
                    session_id,
                    seq,
                    payload,
                )
                .build(),
            )
        }

        AgentEvent::AssistantCompleted(p) => {
            // AssistantCompleted also maps to message.delta with the
            // full text; the Renderer uses it to finalise the message.
            let payload = serde_json::json!({
                "fullText": p.full_text,
                "completed": true,
            });
            Some(
                super::events::SessionEvent::new(
                    super::events::event_types::MESSAGE_DELTA,
                    super::types::TaskId::new(""),
                    session_id,
                    seq,
                    payload,
                )
                .build(),
            )
        }

        AgentEvent::ToolStarted(p) | AgentEvent::ToolUpdated(p) => {
            let payload = super::events::ActivityUpdatedPayload {
                kind: "tool".into(),
                detail: p.title.unwrap_or_default(),
                code: None,
                retryable: None,
            };
            Some(
                super::events::SessionEvent::new(
                    super::events::event_types::ACTIVITY_UPDATED,
                    super::types::TaskId::new(""),
                    session_id,
                    seq,
                    serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null),
                )
                .build(),
            )
        }

        AgentEvent::ToolCompleted(p) => {
            let payload = super::events::ActivityUpdatedPayload {
                kind: format!("tool:{}", p.outcome),
                detail: p.summary.unwrap_or_default(),
                code: None,
                retryable: None,
            };
            Some(
                super::events::SessionEvent::new(
                    super::events::event_types::ACTIVITY_UPDATED,
                    super::types::TaskId::new(""),
                    session_id,
                    seq,
                    serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null),
                )
                .build(),
            )
        }

        AgentEvent::PermissionRequested(p) => {
            let payload = super::events::PermissionRequestedPayload {
                request_id: p.request_id,
                correlation_id: event
                    .meta
                    .correlation_id
                    .as_ref()
                    .map(|value| value.0.clone())
                    .unwrap_or_default(),
                expected_version: None,
                expires_at_epoch_seconds: 0,
                options: p
                    .options
                    .into_iter()
                    .map(|opt| super::events::PermissionOption {
                        option_id: opt.option_id,
                        name: opt.name,
                        kind: match opt.kind.as_deref() {
                            Some("allow_once") => super::events::PermissionOptionKind::AllowOnce,
                            Some("allow_always" | "allow_scope") => {
                                super::events::PermissionOptionKind::AllowAlways
                            }
                            Some("reject_once" | "reject" | "deny") => {
                                super::events::PermissionOptionKind::RejectOnce
                            }
                            Some("reject_always") => {
                                super::events::PermissionOptionKind::RejectAlways
                            }
                            _ => super::events::PermissionOptionKind::Unknown,
                        },
                    })
                    .collect(),
                tool_call: super::events::ToolCallSummary {
                    tool_call_id: p.tool_call.tool_call_id,
                    title: p.tool_call.title,
                    kind: p.tool_call.kind,
                    locations: None,
                },
                operation: serde_json::json!({
                    "category": "unknown",
                    "risk": "Operation context is unavailable on this compatibility path; backend denies it"
                }),
            };
            Some(
                super::events::SessionEvent::new(
                    super::events::event_types::PERMISSION_REQUESTED,
                    super::types::TaskId::new(""),
                    session_id,
                    seq,
                    serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null),
                )
                .build(),
            )
        }

        AgentEvent::PlanProposed(p) => {
            let payload = super::events::PlanUpdatedPayload {
                status: "proposed".into(),
                detail: serde_json::json!({
                    "request_id": p.request_id,
                    "summary": p.summary,
                    "options": p.options,
                }),
            };
            Some(
                super::events::SessionEvent::new(
                    super::events::event_types::PLAN_UPDATED,
                    super::types::TaskId::new(""),
                    session_id,
                    seq,
                    serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null),
                )
                .build(),
            )
        }

        AgentEvent::ArtifactAnnounced(p) => {
            let payload = super::events::ArtifactAvailablePayload {
                task_id: super::types::TaskId::new(""),
                artifact_id: p.artifact_id,
                mime_type: p.mime_type,
                display_name: p.display_name,
            };
            Some(
                super::events::SessionEvent::new(
                    super::events::event_types::ARTIFACT_AVAILABLE,
                    super::types::TaskId::new(""),
                    session_id,
                    seq,
                    serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null),
                )
                .build(),
            )
        }

        AgentEvent::RequestFailed(p) => {
            let payload = super::events::DiagnosticNoticePayload {
                level: "error".into(),
                message: format!("[{}] {}", p.code, p.message),
                source: "agent_runtime".into(),
            };
            Some(
                super::events::SessionEvent::new(
                    super::events::event_types::DIAGNOSTIC_NOTICE,
                    super::types::TaskId::new(""),
                    session_id,
                    seq,
                    serde_json::to_value(&payload).unwrap_or(serde_json::Value::Null),
                )
                .build(),
            )
        }

        AgentEvent::TurnCancelled(_) => Some(
            super::events::SessionEvent::new(
                super::events::event_types::TASK_STATE,
                super::types::TaskId::new(""),
                session_id,
                seq,
                serde_json::json!({
                    "status": "idle",
                    "detail": { "reason": "cancelled" },
                }),
            )
            .build(),
        ),

        AgentEvent::ProcessExited(p) => {
            // Emit a runtime.updated event with the exit info.
            let payload = serde_json::json!({
                "status": "exited",
                "reason": p.reason,
                "code": p.code,
            });
            Some(DesktopEvent::non_session(
                super::events::event_types::RUNTIME_UPDATED,
                payload,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_result_ok_serializes() {
        let r = DesktopResult::ok(serde_json::json!({"acknowledged": "test"}));
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"success\":\"true\""));
        assert!(json.contains("\"acknowledged\":\"test\""));
    }

    #[test]
    fn desktop_result_err_serializes() {
        let r = DesktopResult::err(AppError::new("TEST", "fail"));
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"success\":\"false\""));
        assert!(json.contains("\"error\""));
    }

    #[tokio::test]
    async fn unknown_command_returns_unsupported() {
        let raw = serde_json::json!({"type": "no.such.command", "payload": {}});
        let repo = std::sync::Arc::new(
            crate::adapters::sqlite::SqliteRepository::open_in_memory().expect("in-memory repo"),
        );
        // Use a fake runtime for the test.
        let config = RuntimeConfig::default();
        let adapter = crate::adapters::grok_acp::GrokAcpAdapter::new(config);
        let runtime = crate::modules::agent_runtime::AgentRuntimeImpl::new(adapter);
        let task_runtime =
            crate::modules::task_runtime::TaskRuntimeImpl::new(repo.clone(), runtime.clone());
        let result = execute_impl(repo.as_ref(), runtime.as_ref(), &task_runtime, raw).await;
        match result {
            DesktopResult::Err { error } => {
                assert_eq!(error.code, domain::error::codes::BRIDGE_UNSUPPORTED_COMMAND);
            }
            _ => panic!("expected Err"),
        }
    }

    #[tokio::test]
    async fn oversized_payload_rejected() {
        let big: String = "x".repeat(2_000_000);
        let raw = serde_json::json!({"type": "runtime.refresh", "payload": {}, "big": big});
        let repo = std::sync::Arc::new(
            crate::adapters::sqlite::SqliteRepository::open_in_memory().expect("in-memory repo"),
        );
        let config = RuntimeConfig::default();
        let adapter = crate::adapters::grok_acp::GrokAcpAdapter::new(config);
        let runtime = crate::modules::agent_runtime::AgentRuntimeImpl::new(adapter);
        let task_runtime =
            crate::modules::task_runtime::TaskRuntimeImpl::new(repo.clone(), runtime.clone());
        let result = execute_impl(repo.as_ref(), runtime.as_ref(), &task_runtime, raw).await;
        match result {
            DesktopResult::Err { error } => {
                assert_eq!(error.code, domain::error::codes::BRIDGE_INVALID_PAYLOAD);
            }
            _ => panic!("expected Err"),
        }
    }

    #[tokio::test]
    async fn task_create_persists() {
        let repo = std::sync::Arc::new(
            crate::adapters::sqlite::SqliteRepository::open_in_memory().expect("in-memory repo"),
        );

        // Create a project first (FK constraint).
        use crate::domain::types::{utc_now, Project, ProjectId};
        let project = Project {
            id: ProjectId::new("proj-1"),
            path: std::env::current_dir()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            display_path: "test/project".into(),
            repo_root: Some(
                std::env::current_dir()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            ),
            trusted_at: Some(utc_now()),
            last_opened_at: utc_now(),
        };
        repo.create_project(&project).unwrap();

        let raw = serde_json::json!({
            "type": "task.create",
            "payload": {
                "projectId": "proj-1",
                "title": "Test task",
                "prompt": "Do something",
                "mode": "ask",
                "workspaceStrategy": "direct"
            }
        });

        let fake_agent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("tests/fake-acp-agent/agent.mjs");
        let adapter = crate::adapters::grok_acp::FakeAcpTransport::new(
            crate::adapters::grok_acp::FakeScenario::Normal,
            fake_agent,
        );
        let runtime = crate::modules::agent_runtime::AgentRuntimeImpl::new(adapter);
        let task_runtime = std::sync::Arc::new(crate::modules::task_runtime::TaskRuntimeImpl::new(
            repo.clone(),
            runtime.clone(),
        ));
        task_runtime.spawn_agent_event_forwarder();

        let result =
            execute_impl(repo.as_ref(), runtime.as_ref(), task_runtime.as_ref(), raw).await;
        match result {
            DesktopResult::Ok { data } => {
                let task = &data["task"];
                assert_eq!(task["title"], "Test task");
                assert_eq!(task["status"], "running");
                assert!(!task["id"].as_str().unwrap().is_empty());
            }
            DesktopResult::Err { error } => {
                panic!("unexpected error: {:?}", error);
            }
        }
    }

    #[tokio::test]
    async fn runtime_refresh_returns_probe_result() {
        let repo = std::sync::Arc::new(
            crate::adapters::sqlite::SqliteRepository::open_in_memory().expect("in-memory repo"),
        );
        let raw = serde_json::json!({"type": "runtime.refresh", "payload": {}});

        let config = RuntimeConfig::default();
        let adapter = crate::adapters::grok_acp::GrokAcpAdapter::new(config);
        let runtime = crate::modules::agent_runtime::AgentRuntimeImpl::new(adapter);
        let task_runtime =
            crate::modules::task_runtime::TaskRuntimeImpl::new(repo.clone(), runtime.clone());

        let result = execute_impl(repo.as_ref(), runtime.as_ref(), &task_runtime, raw).await;
        match result {
            DesktopResult::Ok { data } => {
                // The probe will fail (no grok installed in test env),
                // but the result should still be a valid probe result.
                assert!(data.get("available").is_some() || data.get("status").is_some());
            }
            DesktopResult::Err { error } => {
                panic!("runtime.refresh should not error: {:?}", error);
            }
        }
    }
}
