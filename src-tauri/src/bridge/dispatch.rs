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
                models: vec![],
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

    dispatch(repo, runtime, cmd).await
}

use super::commands::DesktopCommand;

async fn dispatch(
    repo: &dyn Repository,
    runtime: &dyn crate::modules::agent_runtime::AgentRuntime,
    cmd: DesktopCommand,
) -> DesktopResult {
    match &cmd {
        DesktopCommand::RuntimeRefresh(_) => runtime_refresh(runtime).await,
        DesktopCommand::RuntimeLogin(_) => not_implemented("runtime.login"),

        DesktopCommand::ProjectOpen(_) => not_implemented("project.open"),
        DesktopCommand::ProjectForget(_) => not_implemented("project.forget"),

        DesktopCommand::TaskCreate(payload) => task_create(repo, payload),
        DesktopCommand::TaskOpen(_) => not_implemented("task.open"),
        DesktopCommand::TaskArchive(_) => not_implemented("task.archive"),

        DesktopCommand::TurnSend(_) => not_implemented("turn.send"),
        DesktopCommand::TurnCancel(_) => not_implemented("turn.cancel"),
        DesktopCommand::SessionConfigure(_) => not_implemented("session.configure"),
        DesktopCommand::SessionResume(_) => not_implemented("session.resume"),

        DesktopCommand::PermissionResolve(_) => not_implemented("permission.resolve"),
        DesktopCommand::PlanResolve(_) => not_implemented("plan.resolve"),

        DesktopCommand::ArtifactImport(_) => not_implemented("artifact.import"),
        DesktopCommand::ArtifactSave(_) => not_implemented("artifact.save"),

        DesktopCommand::WorkspaceInspect(_) => not_implemented("workspace.inspect"),
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

/// Wire `runtime.refresh` to the AgentRuntime probe.
async fn runtime_refresh(
    runtime: &dyn crate::modules::agent_runtime::AgentRuntime,
) -> DesktopResult {
    let config = RuntimeConfig::default();
    let result = runtime.probe(&config).await;
    DesktopResult::ok(serde_json::to_value(&result).unwrap_or(serde_json::Value::Null))
}

/// Wire `task.create` to the persistence layer.
fn task_create(
    repo: &dyn Repository,
    payload: &super::commands::TaskCreatePayload,
) -> DesktopResult {
    use crate::domain::types::{utc_now, Task, TaskId, TaskStatus, WorkspaceKind};
    use uuid::Uuid;

    let now = utc_now();
    let task = Task {
        id: TaskId::new(format!("task-{}", Uuid::new_v4())),
        project_id: payload.project_id.clone(),
        title: payload.title.clone(),
        status: TaskStatus::Preparing,
        workspace_kind: WorkspaceKind::Worktree,
        mode: payload.mode.clone(),
        model: payload.model.clone(),
        reasoning: payload.reasoning.clone(),
        created_at: now.clone(),
        updated_at: now,
        interrupt_reason: None,
    };

    match repo.create_task(&task) {
        Ok(()) => {}
        Err(e) => return DesktopResult::err(AppError::new(e.code, e.message)),
    }

    DesktopResult::ok(serde_json::json!({
        "task": {
            "id": task.id.0,
            "projectId": task.project_id.0,
            "title": task.title,
            "status": "preparing",
            "createdAt": task.created_at,
        }
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
                options: p
                    .options
                    .into_iter()
                    .map(|opt| super::events::PermissionOption {
                        option_id: opt.option_id,
                        name: opt.name,
                        kind: super::events::PermissionOptionKind::AllowOnce, // placeholder
                    })
                    .collect(),
                tool_call: super::events::ToolCallSummary {
                    tool_call_id: p.tool_call.tool_call_id,
                    title: p.tool_call.title,
                    kind: p.tool_call.kind,
                    locations: None,
                },
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
        let repo =
            crate::adapters::sqlite::SqliteRepository::open_in_memory().expect("in-memory repo");
        // Use a fake runtime for the test.
        let config = RuntimeConfig::default();
        let adapter = crate::adapters::grok_acp::GrokAcpAdapter::new(config);
        let runtime = crate::modules::agent_runtime::AgentRuntimeImpl::new(adapter);
        let result = execute_impl(&repo, runtime.as_ref(), raw).await;
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
        let repo =
            crate::adapters::sqlite::SqliteRepository::open_in_memory().expect("in-memory repo");
        let config = RuntimeConfig::default();
        let adapter = crate::adapters::grok_acp::GrokAcpAdapter::new(config);
        let runtime = crate::modules::agent_runtime::AgentRuntimeImpl::new(adapter);
        let result = execute_impl(&repo, runtime.as_ref(), raw).await;
        match result {
            DesktopResult::Err { error } => {
                assert_eq!(error.code, domain::error::codes::BRIDGE_INVALID_PAYLOAD);
            }
            _ => panic!("expected Err"),
        }
    }

    #[tokio::test]
    async fn task_create_persists() {
        let repo =
            crate::adapters::sqlite::SqliteRepository::open_in_memory().expect("in-memory repo");

        // Create a project first (FK constraint).
        use crate::domain::types::{utc_now, Project, ProjectId};
        let project = Project {
            id: ProjectId::new("proj-1"),
            path: "/test/project".into(),
            display_path: "/test/project".into(),
            repo_root: Some("/test/project".into()),
            trusted_at: Some(utc_now()),
            last_opened_at: utc_now(),
        };
        repo.create_project(&project).unwrap();

        let raw = serde_json::json!({
            "type": "task.create",
            "payload": {
                "projectId": "proj-1",
                "title": "Test task",
                "prompt": "Do something"
            }
        });

        let config = RuntimeConfig::default();
        let adapter = crate::adapters::grok_acp::GrokAcpAdapter::new(config);
        let runtime = crate::modules::agent_runtime::AgentRuntimeImpl::new(adapter);

        let result = execute_impl(&repo, runtime.as_ref(), raw).await;
        match result {
            DesktopResult::Ok { data } => {
                let task = &data["task"];
                assert_eq!(task["title"], "Test task");
                assert_eq!(task["status"], "preparing");
                assert!(!task["id"].as_str().unwrap().is_empty());
            }
            DesktopResult::Err { error } => {
                panic!("unexpected error: {:?}", error);
            }
        }
    }

    #[tokio::test]
    async fn runtime_refresh_returns_probe_result() {
        let repo =
            crate::adapters::sqlite::SqliteRepository::open_in_memory().expect("in-memory repo");
        let raw = serde_json::json!({"type": "runtime.refresh", "payload": {}});

        let config = RuntimeConfig::default();
        let adapter = crate::adapters::grok_acp::GrokAcpAdapter::new(config);
        let runtime = crate::modules::agent_runtime::AgentRuntimeImpl::new(adapter);

        let result = execute_impl(&repo, runtime.as_ref(), raw).await;
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
