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
pub fn bootstrap_impl() -> BootstrapSnapshot {
    BootstrapSnapshot {
        product_name: "Grok ACP GUI".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        platform: std::env::consts::OS.into(),
        ready: true,
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
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapSnapshot {
    pub product_name: String,
    pub version: String,
    pub platform: String,
    pub ready: bool,
    pub runtime: RuntimeBootstrapStatus,
    pub capabilities: CapabilitySnapshot,
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
/// - Unknown `type` → `BRIDGE_UNSUPPORTED_COMMAND`
/// - Recognised `type` but invalid payload → `BRIDGE_INVALID_PAYLOAD`
pub fn execute_impl(raw: serde_json::Value) -> DesktopResult {
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

    dispatch(cmd)
}

use super::commands::DesktopCommand;

fn dispatch(cmd: DesktopCommand) -> DesktopResult {
    match &cmd {
        DesktopCommand::RuntimeRefresh(_) => not_implemented("runtime.refresh"),
        DesktopCommand::RuntimeLogin(_) => not_implemented("runtime.login"),

        DesktopCommand::ProjectOpen(_) => not_implemented("project.open"),
        DesktopCommand::ProjectForget(_) => not_implemented("project.forget"),

        DesktopCommand::TaskCreate(_) => not_implemented("task.create"),
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

/// Not-implemented response — every command returns a BRIDGE_NOT_IMPLEMENTED
/// error until its module is wired in a later GAG task.  This prevents the
/// Renderer from misinterpreting a success response for an unimplemented path.
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

    #[test]
    fn all_commands_return_not_implemented() {
        let cmd = DesktopCommand::RuntimeRefresh(super::super::commands::EmptyPayload {});
        let result = dispatch(cmd);
        match result {
            DesktopResult::Err { error } => {
                assert_eq!(error.code, domain::error::codes::BRIDGE_NOT_IMPLEMENTED);
            }
            _ => panic!("expected Err"),
        }
    }

    #[test]
    fn unknown_command_returns_unsupported() {
        let raw = serde_json::json!({"type": "no.such.command", "payload": {}});
        let result = execute_impl(raw);
        match result {
            DesktopResult::Err { error } => {
                assert_eq!(error.code, domain::error::codes::BRIDGE_UNSUPPORTED_COMMAND);
            }
            _ => panic!("expected Err"),
        }
    }

    #[test]
    fn oversized_payload_rejected() {
        let big: String = "x".repeat(2_000_000);
        let raw = serde_json::json!({"type": "runtime.refresh", "payload": {}, "big": big});
        let result = execute_impl(raw);
        match result {
            DesktopResult::Err { error } => {
                assert_eq!(error.code, domain::error::codes::BRIDGE_INVALID_PAYLOAD);
            }
            _ => panic!("expected Err"),
        }
    }
}
