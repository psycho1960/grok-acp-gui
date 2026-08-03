//! DesktopCommand — the single command discriminated union crossing the bridge.
//!
//! Every Renderer action maps to exactly one variant.  The bridge validates
//! the discriminator and payload shape; unrecognised types produce a
//! `BRIDGE_UNSUPPORTED_COMMAND` error without panicking.

use serde::{Deserialize, Serialize};

/// Empty payload for commands that need no parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmptyPayload {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeLoginPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOpenPayload {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectForgetPayload {
    pub project_id: super::types::ProjectId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskCreatePayload {
    pub project_id: super::types::ProjectId,
    pub title: String,
    /// Initial prompt text (FR-TASK-001). Required.
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_strategy: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskOpenPayload {
    pub task_id: super::types::TaskId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskArchivePayload {
    pub task_id: super::types::TaskId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnSendPayload {
    pub task_id: super::types::TaskId,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachments: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnCancelPayload {
    pub task_id: super::types::TaskId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionConfigurePayload {
    pub task_id: super::types::TaskId,
    pub settings: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResumePayload {
    pub task_id: super::types::TaskId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionResolvePayload {
    pub request_id: String,
    pub option_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanResolvePayload {
    pub request_id: String,
    /// ACP option ID, passed verbatim — must not be inferred from labels.
    pub option_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactImportPayload {
    pub task_id: super::types::TaskId,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactSavePayload {
    pub task_id: super::types::TaskId,
    pub artifact_ids: Vec<String>,
    pub target_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInspectPayload {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeAdoptPayload {
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewDiffPayload {
    pub task_id: super::types::TaskId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewCheckpointPayload {
    pub task_id: super::types::TaskId,
    pub message: String,
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationPreflightPayload {
    pub task_id: super::types::TaskId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationExecutePayload {
    pub task_id: super::types::TaskId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeCleanupPayload {
    pub task_id: super::types::TaskId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryRestorePayload {
    pub item_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryDeletePayload {
    pub item_id: String,
}

// ---------------------------------------------------------------------------
// Discriminated union
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum DesktopCommand {
    #[serde(rename = "runtime.refresh")]
    RuntimeRefresh(EmptyPayload),
    #[serde(rename = "runtime.login")]
    RuntimeLogin(RuntimeLoginPayload),

    #[serde(rename = "project.open")]
    ProjectOpen(ProjectOpenPayload),
    #[serde(rename = "project.forget")]
    ProjectForget(ProjectForgetPayload),

    #[serde(rename = "task.create")]
    TaskCreate(TaskCreatePayload),
    #[serde(rename = "task.open")]
    TaskOpen(TaskOpenPayload),
    #[serde(rename = "task.archive")]
    TaskArchive(TaskArchivePayload),

    #[serde(rename = "turn.send")]
    TurnSend(TurnSendPayload),
    #[serde(rename = "turn.cancel")]
    TurnCancel(TurnCancelPayload),
    #[serde(rename = "session.configure")]
    SessionConfigure(SessionConfigurePayload),
    #[serde(rename = "session.resume")]
    SessionResume(SessionResumePayload),

    #[serde(rename = "permission.resolve")]
    PermissionResolve(PermissionResolvePayload),
    #[serde(rename = "plan.resolve")]
    PlanResolve(PlanResolvePayload),

    #[serde(rename = "artifact.import")]
    ArtifactImport(ArtifactImportPayload),
    #[serde(rename = "artifact.save")]
    ArtifactSave(ArtifactSavePayload),

    #[serde(rename = "workspace.inspect")]
    WorkspaceInspect(WorkspaceInspectPayload),
    #[serde(rename = "worktree.adopt")]
    WorktreeAdopt(WorktreeAdoptPayload),

    #[serde(rename = "review.diff")]
    ReviewDiff(ReviewDiffPayload),
    #[serde(rename = "review.checkpoint")]
    ReviewCheckpoint(ReviewCheckpointPayload),

    #[serde(rename = "integration.preflight")]
    IntegrationPreflight(IntegrationPreflightPayload),
    #[serde(rename = "integration.execute")]
    IntegrationExecute(IntegrationExecutePayload),

    #[serde(rename = "worktree.cleanup")]
    WorktreeCleanup(WorktreeCleanupPayload),

    #[serde(rename = "recovery.restore")]
    RecoveryRestore(RecoveryRestorePayload),
    #[serde(rename = "recovery.delete")]
    RecoveryDelete(RecoveryDeletePayload),
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Returns true if `cmd_type` matches a known DesktopCommand variant.
pub fn is_known_command(cmd_type: &str) -> bool {
    matches!(
        cmd_type,
        "runtime.refresh"
            | "runtime.login"
            | "project.open"
            | "project.forget"
            | "task.create"
            | "task.open"
            | "task.archive"
            | "turn.send"
            | "turn.cancel"
            | "session.configure"
            | "session.resume"
            | "permission.resolve"
            | "plan.resolve"
            | "artifact.import"
            | "artifact.save"
            | "workspace.inspect"
            | "worktree.adopt"
            | "review.diff"
            | "review.checkpoint"
            | "integration.preflight"
            | "integration.execute"
            | "worktree.cleanup"
            | "recovery.restore"
            | "recovery.delete"
    )
}

use super::error::AppError;
use crate::domain;

const MAX_TITLE_LENGTH: usize = 500;
const MAX_MESSAGE_LENGTH: usize = 100_000;
const MAX_PATH_LENGTH: usize = 4096;

/// Validate a parsed `DesktopCommand` before dispatching it.
/// Returns `Ok(())` or an `AppError` with `BRIDGE_VALIDATION_FAILED`.
pub fn validate(cmd: &DesktopCommand) -> Result<(), AppError> {
    match cmd {
        DesktopCommand::RuntimeRefresh(_) => Ok(()),

        DesktopCommand::RuntimeLogin(p) => {
            if let Some(ref method) = p.method {
                if method.is_empty() {
                    return Err(validation_err("login method must not be empty"));
                }
            }
            Ok(())
        }

        DesktopCommand::ProjectOpen(p) => {
            validate_non_empty_path(&p.path)?;
            Ok(())
        }

        DesktopCommand::ProjectForget(p) => {
            validate_id_non_empty(&p.project_id.0)?;
            Ok(())
        }

        DesktopCommand::TaskCreate(p) => {
            validate_id_non_empty(&p.project_id.0)?;
            validate_text_len(&p.title, MAX_TITLE_LENGTH, "title")?;
            validate_non_empty_text(&p.prompt, "prompt")?;
            validate_text_len(&p.prompt, MAX_MESSAGE_LENGTH, "prompt")?;
            reject_base64_image(&p.prompt)?;
            validate_enum_opt(
                p.reasoning.as_deref(),
                &["low", "medium", "high"],
                "reasoning",
            )?;
            validate_enum_opt(
                p.workspace_strategy.as_deref(),
                &["worktree", "readonly", "direct"],
                "workspaceStrategy",
            )?;
            Ok(())
        }

        DesktopCommand::TaskOpen(p) => {
            validate_id_non_empty(&p.task_id.0)?;
            Ok(())
        }

        DesktopCommand::TaskArchive(p) => {
            validate_id_non_empty(&p.task_id.0)?;
            Ok(())
        }

        DesktopCommand::TurnSend(p) => {
            validate_id_non_empty(&p.task_id.0)?;
            validate_text_len(&p.message, MAX_MESSAGE_LENGTH, "message")?;
            reject_base64_image(&p.message)?;
            Ok(())
        }

        DesktopCommand::TurnCancel(p) => {
            validate_id_non_empty(&p.task_id.0)?;
            Ok(())
        }

        DesktopCommand::SessionConfigure(p) => {
            validate_id_non_empty(&p.task_id.0)?;
            Ok(())
        }

        DesktopCommand::SessionResume(p) => {
            validate_id_non_empty(&p.task_id.0)?;
            Ok(())
        }

        DesktopCommand::PermissionResolve(p) => {
            validate_id_non_empty(&p.request_id)?;
            validate_id_non_empty(&p.option_id)?;
            Ok(())
        }

        DesktopCommand::PlanResolve(p) => {
            validate_id_non_empty(&p.request_id)?;
            validate_id_non_empty(&p.option_id)?;
            Ok(())
        }

        DesktopCommand::ArtifactImport(p) => {
            validate_id_non_empty(&p.task_id.0)?;
            if p.paths.is_empty() {
                return Err(validation_err("artifact.import requires at least one path"));
            }
            for path in &p.paths {
                validate_non_empty_path(path)?;
            }
            Ok(())
        }

        DesktopCommand::ArtifactSave(p) => {
            validate_id_non_empty(&p.task_id.0)?;
            if p.artifact_ids.is_empty() {
                return Err(validation_err(
                    "artifact.save requires at least one artifact ID",
                ));
            }
            validate_non_empty_path(&p.target_path)?;
            Ok(())
        }

        DesktopCommand::WorkspaceInspect(p) => {
            validate_non_empty_path(&p.path)?;
            Ok(())
        }

        DesktopCommand::WorktreeAdopt(p) => {
            validate_non_empty_path(&p.path)?;
            Ok(())
        }

        DesktopCommand::ReviewDiff(p) => {
            validate_id_non_empty(&p.task_id.0)?;
            Ok(())
        }

        DesktopCommand::ReviewCheckpoint(p) => {
            validate_id_non_empty(&p.task_id.0)?;
            validate_text_len(&p.message, MAX_MESSAGE_LENGTH, "message")?;
            if p.paths.is_empty() {
                return Err(validation_err(
                    "review.checkpoint requires at least one path",
                ));
            }
            Ok(())
        }

        DesktopCommand::IntegrationPreflight(p) => {
            validate_id_non_empty(&p.task_id.0)?;
            Ok(())
        }

        DesktopCommand::IntegrationExecute(p) => {
            validate_id_non_empty(&p.task_id.0)?;
            Ok(())
        }

        DesktopCommand::WorktreeCleanup(p) => {
            validate_id_non_empty(&p.task_id.0)?;
            Ok(())
        }

        DesktopCommand::RecoveryRestore(p) => {
            validate_id_non_empty(&p.item_id)?;
            Ok(())
        }

        DesktopCommand::RecoveryDelete(p) => {
            validate_id_non_empty(&p.item_id)?;
            Ok(())
        }
    }
}

fn validation_err(msg: &str) -> AppError {
    AppError::new(domain::error::codes::BRIDGE_VALIDATION_FAILED, msg)
}

fn validate_id_non_empty(id: &str) -> Result<(), AppError> {
    if id.trim().is_empty() {
        Err(validation_err("ID must not be empty"))
    } else {
        Ok(())
    }
}

fn validate_non_empty_text(text: &str, field: &str) -> Result<(), AppError> {
    if text.trim().is_empty() {
        Err(validation_err(&format!("{} must not be empty", field)))
    } else {
        Ok(())
    }
}

fn validate_text_len(text: &str, max: usize, field: &str) -> Result<(), AppError> {
    if text.len() > max {
        Err(validation_err(&format!(
            "{} exceeds maximum length of {} characters",
            field, max
        )))
    } else {
        Ok(())
    }
}

fn validate_enum_opt(value: Option<&str>, allowed: &[&str], field: &str) -> Result<(), AppError> {
    if let Some(v) = value {
        if !allowed.contains(&v) {
            return Err(validation_err(&format!(
                "{} '{}' is not one of: {}",
                field,
                v,
                allowed.join(", ")
            )));
        }
    }
    Ok(())
}

fn validate_non_empty_path(path: &str) -> Result<(), AppError> {
    if path.trim().is_empty() {
        Err(validation_err("path must not be empty"))
    } else if path.len() > MAX_PATH_LENGTH {
        Err(validation_err("path exceeds maximum length"))
    } else {
        Ok(())
    }
}

fn reject_base64_image(text: &str) -> Result<(), AppError> {
    // Scan for data:image inline base64 (case-insensitive).
    // Uses char-based iteration to avoid UTF-8 byte-boundary panics.
    if text.len() > 22 {
        let prefix: String = text.chars().take(22).collect();
        if prefix.to_lowercase().starts_with("data:image") {
            return Err(validation_err(
                "Base64-encoded images are not allowed in Bridge payloads; use artifact IDs",
            ));
        }
    }
    // Also check later positions in case leading whitespace/text was prepended.
    if text.to_lowercase().contains("data:image/") {
        return Err(validation_err(
            "Base64-encoded images are not allowed in Bridge payloads; use artifact IDs",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_image_at_start_rejected() {
        let text = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUg==";
        assert!(reject_base64_image(text).is_err());
    }

    #[test]
    fn base64_image_after_whitespace_rejected() {
        let text = "  data:image/jpeg;base64,/9j/4AAQ==";
        assert!(reject_base64_image(text).is_err());
    }

    #[test]
    fn chinese_text_no_panic() {
        let text =
            "你好世界！这是一段中文文本，用于测试 UTF-8 边界安全性。data:image 不应该 panic。";
        assert!(reject_base64_image(text).is_ok());
    }

    #[test]
    fn plain_text_passes() {
        assert!(reject_base64_image("Create a login page").is_ok());
    }

    #[test]
    fn base64_in_middle_rejected() {
        let text = "Here is an image: data:image/gif;base64,R0lGODlh... embedded in text";
        assert!(reject_base64_image(text).is_err());
    }

    #[test]
    fn round_trip_runtime_refresh() {
        let cmd = DesktopCommand::RuntimeRefresh(EmptyPayload {});
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains("\"type\":\"runtime.refresh\""));
        assert!(json.contains("\"payload\":{}"));
        let back: DesktopCommand = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, DesktopCommand::RuntimeRefresh(_)));
    }

    #[test]
    fn round_trip_task_create() {
        let cmd = DesktopCommand::TaskCreate(TaskCreatePayload {
            project_id: super::super::types::ProjectId::new("proj-1"),
            title: "Add login".into(),
            prompt: "Create a login page".into(),
            attachments: None,
            mode: Some("code".into()),
            model: None,
            reasoning: None,
            workspace_strategy: None,
        });
        let json = serde_json::to_string(&cmd).unwrap();
        let back: DesktopCommand = serde_json::from_str(&json).unwrap();
        if let DesktopCommand::TaskCreate(p) = back {
            assert_eq!(p.title, "Add login");
            assert_eq!(p.mode.as_deref(), Some("code"));
        } else {
            panic!("wrong variant");
        }
    }

    #[test]
    fn unknown_type_errors() {
        let json = r#"{"type":"not.a.command","payload":{}}"#;
        let result: Result<DesktopCommand, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn generate_fixture_json() {
        // Round-trip test that also serves as TS fixture source.
        let cmds: Vec<DesktopCommand> = vec![
            DesktopCommand::RuntimeRefresh(EmptyPayload {}),
            DesktopCommand::TaskCreate(TaskCreatePayload {
                project_id: super::super::types::ProjectId::new("p1"),
                title: "Test".into(),
                prompt: "Do it".into(),
                attachments: None,
                mode: Some("code".into()),
                model: None,
                reasoning: None,
                workspace_strategy: None,
            }),
            DesktopCommand::PermissionResolve(PermissionResolvePayload {
                request_id: "req-1".into(),
                option_id: "opt-allow-once".into(),
            }),
        ];
        for cmd in &cmds {
            let json = serde_json::to_string(cmd).unwrap();
            let back: DesktopCommand = serde_json::from_str(&json).unwrap();
            // Verify round-trip by re-serializing
            let json2 = serde_json::to_string(&back).unwrap();
            assert_eq!(json, json2);
        }
    }
}
