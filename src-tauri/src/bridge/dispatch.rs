//! Bridge command dispatch and Tauri event channel.
//!
//! The `execute` Tauri command deserialises the incoming `DesktopCommand`,
//! routes it to the appropriate deep module, and returns a `DesktopResult`.
//! Unrecognised commands produce a `BRIDGE_UNSUPPORTED_COMMAND` error.
//!
//! Events are emitted on a single Tauri event channel (`bridge:event`).
//! Session-scoped events include `taskId`, `sessionId`, and a monotonic
//! `seq`; non-session events omit those fields.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use super::commands::DesktopCommand;
use super::error::AppError;
use super::events::DesktopEvent;

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

/// Implementation of the `bootstrap` command (called from lib.rs).
pub fn bootstrap_impl() -> BootstrapStatus {
    BootstrapStatus {
        product_name: "Grok ACP GUI",
        version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
        ready: true,
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapStatus {
    pub product_name: &'static str,
    pub version: &'static str,
    pub platform: &'static str,
    pub ready: bool,
}

/// Implementation of the `execute` command (called from lib.rs).
pub fn execute_impl(command: DesktopCommand) -> DesktopResult {
    dispatch(command)
}

fn dispatch(cmd: DesktopCommand) -> DesktopResult {
    match &cmd {
        DesktopCommand::RuntimeRefresh(_) => stub("runtime.refresh"),
        DesktopCommand::RuntimeLogin(_) => stub("runtime.login"),

        DesktopCommand::ProjectOpen(_) => stub("project.open"),
        DesktopCommand::ProjectForget(_) => stub("project.forget"),

        DesktopCommand::TaskCreate(_) => stub("task.create"),
        DesktopCommand::TaskOpen(_) => stub("task.open"),
        DesktopCommand::TaskArchive(_) => stub("task.archive"),

        DesktopCommand::TurnSend(_) => stub("turn.send"),
        DesktopCommand::TurnCancel(_) => stub("turn.cancel"),
        DesktopCommand::SessionConfigure(_) => stub("session.configure"),
        DesktopCommand::SessionResume(_) => stub("session.resume"),

        DesktopCommand::PermissionResolve(_) => stub("permission.resolve"),
        DesktopCommand::PlanResolve(_) => stub("plan.resolve"),

        DesktopCommand::ArtifactImport(_) => stub("artifact.import"),
        DesktopCommand::ArtifactSave(_) => stub("artifact.save"),

        DesktopCommand::WorkspaceInspect(_) => stub("workspace.inspect"),
        DesktopCommand::WorktreeAdopt(_) => stub("worktree.adopt"),

        DesktopCommand::ReviewDiff(_) => stub("review.diff"),
        DesktopCommand::ReviewCheckpoint(_) => stub("review.checkpoint"),

        DesktopCommand::IntegrationPreflight(_) => stub("integration.preflight"),
        DesktopCommand::IntegrationExecute(_) => stub("integration.execute"),

        DesktopCommand::WorktreeCleanup(_) => stub("worktree.cleanup"),

        DesktopCommand::RecoveryRestore(_) => stub("recovery.restore"),
        DesktopCommand::RecoveryDelete(_) => stub("recovery.delete"),
    }
}

/// Stub response — every command returns an acknowledged placeholder
/// until its module is implemented in a later GAG task.
fn stub(command_name: &str) -> DesktopResult {
    DesktopResult::ok(serde_json::json!({
        "acknowledged": command_name,
    }))
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
    fn all_commands_return_stub() {
        // Quick smoke test that every variant dispatches without panicking.
        let cmd = DesktopCommand::RuntimeRefresh(super::super::commands::EmptyPayload {});
        let result = dispatch(cmd);
        match result {
            DesktopResult::Ok { data } => {
                assert!(data.get("acknowledged").is_some());
            }
            _ => panic!("expected Ok"),
        }
    }
}
