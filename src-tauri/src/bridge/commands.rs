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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
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
    pub action: PlanAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanAction {
    Approve,
    Reject,
    KeepPlanning,
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

#[cfg(test)]
mod tests {
    use super::*;

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
            mode: Some("code".into()),
            model: None,
            reasoning: None,
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
    fn missing_payload_field_errors() {
        let json = r#"{"type":"task.create","payload":{"projectId":"x"}}"#;
        let result: Result<DesktopCommand, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }
}
