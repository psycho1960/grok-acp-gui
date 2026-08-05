//! Permission policy and the single execution-boundary guard (GAG-009).
//!
//! Adapters call [`ExecutionGuard::authorize`] immediately before I/O. The
//! Renderer never receives [`ApprovalEvidence`], and an unknown or incomplete
//! descriptor is always denied.

use crate::bridge::types::{SessionId, TaskId};
use crate::domain::error::{codes, DomainError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Component, Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationCategory {
    ReadOnly,
    Write,
    Destructive,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionState {
    Requested,
    ApprovedOnce,
    ApprovedScope,
    Denied,
    Expired,
    Cancelled,
    Consumed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionOptionAction {
    AllowOnce,
    AllowScope,
    Deny,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOption {
    pub option_id: String,
    pub label: String,
    pub action: PermissionOptionAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRecord {
    pub request_id: String,
    pub task_id: TaskId,
    pub session_id: SessionId,
    pub correlation_id: String,
    pub workspace: String,
    pub plan_version: Option<u64>,
    pub operation_digest: String,
    pub category: OperationCategory,
    pub summary_redacted: String,
    pub options: Vec<PermissionOption>,
    pub state: PermissionState,
    pub expires_at_epoch_seconds: u64,
    pub decided_option_id: Option<String>,
    pub consumed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionDecision {
    pub request_id: String,
    pub task_id: TaskId,
    pub session_id: SessionId,
    pub correlation_id: String,
    pub workspace: String,
    pub expected_plan_version: Option<u64>,
    pub option_id: String,
    pub decided_at: String,
    pub decided_at_epoch_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionResolutionRequest {
    pub task_id: TaskId,
    pub session_id: SessionId,
    pub request_id: String,
    pub correlation_id: String,
    /// Zero means the request was created outside Plan mode.
    pub expected_version: u64,
    pub option_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    Process,
    FileRead,
    FileWrite,
    FileDelete,
    Git,
}

/// Structured, shell-free operation details. `args` are executable arguments,
/// never a command string. Every path is included in the approval digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationDescriptor {
    pub kind: OperationKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    pub cwd: String,
    #[serde(default)]
    pub read_paths: Vec<String>,
    #[serde(default)]
    pub write_paths: Vec<String>,
}

impl OperationDescriptor {
    pub fn category(&self) -> OperationCategory {
        match self.kind {
            OperationKind::FileRead => {
                if self.write_paths.is_empty() && !self.read_paths.is_empty() {
                    OperationCategory::ReadOnly
                } else {
                    OperationCategory::Unknown
                }
            }
            OperationKind::FileWrite if self.write_paths.is_empty() => OperationCategory::Unknown,
            OperationKind::FileWrite => OperationCategory::Write,
            OperationKind::FileDelete if self.write_paths.is_empty() => OperationCategory::Unknown,
            OperationKind::FileDelete => OperationCategory::Destructive,
            OperationKind::Git => classify_git(self.executable.as_deref(), &self.args),
            OperationKind::Process => classify_process(self.executable.as_deref(), &self.args),
        }
    }

    pub fn validate_within(&self, workspace: &str) -> Result<(), DomainError> {
        if self.cwd.trim().is_empty()
            || workspace.trim().is_empty()
            || !Path::new(&self.cwd).is_absolute()
            || !Path::new(workspace).is_absolute()
        {
            return Err(unknown("operation cwd or workspace is missing"));
        }
        let root = normalize_path(workspace)?;
        let cwd = normalize_path(&self.cwd)?;
        if !path_is_within(&cwd, &root) {
            return Err(unknown("operation cwd escapes its workspace"));
        }
        for path in self.read_paths.iter().chain(self.write_paths.iter()) {
            let normalized = if Path::new(path).is_absolute() {
                normalize_path(path)?
            } else {
                normalize_path(&format!("{}/{}", cwd, path))?
            };
            if !path_is_within(&normalized, &root) {
                return Err(unknown("operation path escapes its workspace"));
            }
        }
        if self.category() == OperationCategory::Unknown {
            return Err(unknown("operation is not in the declared policy matrix"));
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String, DomainError> {
        let canonical = serde_json::to_vec(self)
            .map_err(|_| unknown("operation descriptor cannot be canonicalized"))?;
        let mut hasher = Sha256::new();
        hasher.update(b"gag-009-operation-v1\0");
        hasher.update(canonical);
        Ok(format!("{:x}", hasher.finalize()))
    }
}

fn executable_name(executable: Option<&str>) -> Option<String> {
    let raw = executable?.trim();
    if raw.is_empty() {
        return None;
    }
    Path::new(raw)
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.trim_end_matches(".exe").to_ascii_lowercase())
}

fn classify_process(executable: Option<&str>, args: &[String]) -> OperationCategory {
    let Some(name) = executable_name(executable) else {
        return OperationCategory::Unknown;
    };
    if args.iter().any(|arg| {
        let value = arg.to_ascii_lowercase();
        value == "-c"
            || value == "-command"
            || value == "/c"
            || value.contains('>')
            || value.contains('|')
            || value.contains("&&")
    }) {
        return OperationCategory::Unknown;
    }
    match name.as_str() {
        "rg" | "fd" | "where" => OperationCategory::ReadOnly,
        "rm" | "rmdir" | "del" | "erase" | "remove-item" => OperationCategory::Destructive,
        _ => OperationCategory::Unknown,
    }
}

fn classify_git(executable: Option<&str>, args: &[String]) -> OperationCategory {
    if executable_name(executable).as_deref() != Some("git") || args.is_empty() {
        return OperationCategory::Unknown;
    }
    let mut command = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "-C" || arg == "--git-dir" || arg == "--work-tree" {
            if iter.next().is_none() {
                return OperationCategory::Unknown;
            }
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        command = Some(arg.to_ascii_lowercase());
        break;
    }
    match command.as_deref() {
        Some(
            "status" | "diff" | "log" | "show" | "rev-parse" | "branch" | "ls-files" | "worktree",
        ) => {
            // `branch` and `worktree` are read-only only for their explicit list forms.
            if command.as_deref() == Some("branch")
                && args
                    .iter()
                    .any(|arg| matches!(arg.as_str(), "-d" | "-D" | "--delete"))
            {
                return OperationCategory::Destructive;
            }
            if command.as_deref() == Some("branch")
                && !args
                    .iter()
                    .any(|arg| arg == "--show-current" || arg == "--list")
            {
                return OperationCategory::Unknown;
            }
            if command.as_deref() == Some("worktree")
                && args
                    .iter()
                    .any(|arg| matches!(arg.as_str(), "remove" | "prune"))
            {
                return OperationCategory::Destructive;
            }
            if command.as_deref() == Some("worktree") && !args.iter().any(|arg| arg == "list") {
                return OperationCategory::Unknown;
            }
            OperationCategory::ReadOnly
        }
        Some("add" | "commit" | "merge" | "checkout" | "switch" | "restore") => {
            OperationCategory::Write
        }
        Some("clean" | "reset") => OperationCategory::Destructive,
        _ => OperationCategory::Unknown,
    }
}

fn normalize_path(raw: &str) -> Result<String, DomainError> {
    let replaced = raw.trim().replace('\\', "/");
    if replaced.is_empty() {
        return Err(unknown("path is missing"));
    }
    let path = Path::new(&replaced);
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => {
                parts.push(prefix.as_os_str().to_string_lossy().to_ascii_lowercase())
            }
            Component::RootDir => {}
            Component::CurDir => {}
            Component::Normal(value) => parts.push(value.to_string_lossy().to_ascii_lowercase()),
            Component::ParentDir => {
                if parts.pop().is_none() {
                    return Err(unknown("path escapes its root"));
                }
            }
        }
    }
    Ok(parts.join("/"))
}

fn path_is_within(candidate: &str, root: &str) -> bool {
    candidate == root
        || candidate
            .strip_prefix(root)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn unknown(message: &str) -> DomainError {
    DomainError::new(codes::OPERATION_UNKNOWN, message)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionContext {
    pub task_id: TaskId,
    pub session_id: SessionId,
    pub workspace: String,
    pub plan_version: Option<u64>,
    pub plan_approved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalEvidence {
    pub permission_id: String,
    pub session_id: SessionId,
    pub workspace: String,
    pub operation_digest: String,
    pub plan_version: Option<u64>,
    pub expires_at_epoch_seconds: u64,
}

pub trait ApprovalStore: Send + Sync {
    /// Atomically validates and consumes one matching approval.
    fn consume_matching(
        &self,
        context: &ExecutionContext,
        operation_digest: &str,
        now_epoch_seconds: u64,
    ) -> Result<Option<ApprovalEvidence>, DomainError>;
}

impl ApprovalStore for std::sync::Arc<dyn crate::modules::persistence::Repository> {
    fn consume_matching(
        &self,
        context: &ExecutionContext,
        operation_digest: &str,
        now_epoch_seconds: u64,
    ) -> Result<Option<ApprovalEvidence>, DomainError> {
        self.consume_permission(context, operation_digest, now_epoch_seconds)
    }
}

pub enum Authorization {
    Allowed(ApprovalEvidence),
    Denied(DomainError),
    Pending,
}

pub struct ExecutionGuard<S: ApprovalStore> {
    store: S,
}

impl<S: ApprovalStore> ExecutionGuard<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    pub fn authorize(
        &self,
        operation: &OperationDescriptor,
        context: &ExecutionContext,
        now_epoch_seconds: u64,
    ) -> Authorization {
        if let Err(error) = operation.validate_within(&context.workspace) {
            return Authorization::Denied(error);
        }
        let category = operation.category();
        if context.plan_version.is_some() && !context.plan_approved {
            if category == OperationCategory::ReadOnly {
                return Authorization::Allowed(ApprovalEvidence {
                    permission_id: "plan-read-only".into(),
                    session_id: context.session_id.clone(),
                    workspace: context.workspace.clone(),
                    operation_digest: operation.digest().unwrap_or_default(),
                    plan_version: context.plan_version,
                    expires_at_epoch_seconds: now_epoch_seconds,
                });
            }
            return Authorization::Denied(DomainError::new(
                codes::PLAN_NOT_APPROVED,
                "Plan is not approved; write and non-read-only operations are blocked",
            ));
        }
        let digest = match operation.digest() {
            Ok(value) => value,
            Err(error) => return Authorization::Denied(error),
        };
        match self
            .store
            .consume_matching(context, &digest, now_epoch_seconds)
        {
            Ok(Some(evidence)) => Authorization::Allowed(evidence),
            Ok(None) => Authorization::Pending,
            Err(error) => Authorization::Denied(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct OneShotStore(Mutex<Option<ApprovalEvidence>>);
    impl ApprovalStore for OneShotStore {
        fn consume_matching(
            &self,
            context: &ExecutionContext,
            digest: &str,
            now: u64,
        ) -> Result<Option<ApprovalEvidence>, DomainError> {
            let mut value = self.0.lock().unwrap();
            let matches = value.as_ref().is_some_and(|e| {
                e.session_id == context.session_id
                    && e.workspace == context.workspace
                    && e.operation_digest == digest
                    && e.plan_version == context.plan_version
                    && e.expires_at_epoch_seconds >= now
            });
            Ok(if matches { value.take() } else { None })
        }
    }

    fn context(approved: bool) -> ExecutionContext {
        ExecutionContext {
            task_id: TaskId::new("task-1"),
            session_id: SessionId::new("session-1"),
            workspace: "C:/repo".into(),
            plan_version: Some(2),
            plan_approved: approved,
        }
    }

    fn git(args: &[&str]) -> OperationDescriptor {
        OperationDescriptor {
            kind: OperationKind::Git,
            executable: Some("git.exe".into()),
            args: args.iter().map(|v| (*v).into()).collect(),
            cwd: "C:/repo".into(),
            read_paths: vec![],
            write_paths: vec![],
        }
    }

    #[test]
    fn classification_is_fail_closed() {
        assert_eq!(
            git(&["status", "--short"]).category(),
            OperationCategory::ReadOnly
        );
        assert_eq!(
            git(&["commit", "-m", "x"]).category(),
            OperationCategory::Write
        );
        assert_eq!(git(&["mystery"]).category(), OperationCategory::Unknown);
        assert_eq!(
            git(&["branch", "topic"]).category(),
            OperationCategory::Unknown
        );
    }

    #[test]
    fn plan_allows_declared_reads_but_blocks_writes() {
        let guard = ExecutionGuard::new(OneShotStore(Mutex::new(None)));
        assert!(matches!(
            guard.authorize(&git(&["status"]), &context(false), 10),
            Authorization::Allowed(_)
        ));
        assert!(matches!(
            guard.authorize(&git(&["commit", "-m", "x"]), &context(false), 10),
            Authorization::Denied(_)
        ));
    }

    #[test]
    fn evidence_is_bound_and_consumed_once() {
        let op = git(&["commit", "-m", "x"]);
        let digest = op.digest().unwrap();
        let evidence = ApprovalEvidence {
            permission_id: "permission-1".into(),
            session_id: SessionId::new("session-1"),
            workspace: "C:/repo".into(),
            operation_digest: digest,
            plan_version: Some(2),
            expires_at_epoch_seconds: 50,
        };
        let guard = ExecutionGuard::new(OneShotStore(Mutex::new(Some(evidence))));
        assert!(matches!(
            guard.authorize(&op, &context(true), 10),
            Authorization::Allowed(_)
        ));
        assert!(matches!(
            guard.authorize(&op, &context(true), 10),
            Authorization::Pending
        ));
    }

    #[test]
    fn digest_changes_with_argument_and_paths_cannot_escape() {
        assert_ne!(
            git(&["show", "a"]).digest().unwrap(),
            git(&["show", "b"]).digest().unwrap()
        );
        let mut op = git(&["status"]);
        op.read_paths.push("../secret".into());
        assert!(op.validate_within("C:/repo").is_err());
    }
}
