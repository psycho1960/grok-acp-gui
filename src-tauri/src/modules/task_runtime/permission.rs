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
            OperationKind::Process => {
                let category = classify_process(self.executable.as_deref(), &self.args);
                // A destructive process is approvable only when the adapter
                // supplied every target in `write_paths`.  Parsing arbitrary
                // command-line syntax here is unsafe, and omitting those
                // paths would let an out-of-workspace target evade the
                // containment check below.
                if category == OperationCategory::Destructive && self.write_paths.is_empty() {
                    OperationCategory::Unknown
                } else {
                    category
                }
            }
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
        // Git path-type global options (-C, --git-dir, --work-tree) may point
        // outside the workspace; they must satisfy the same containment rule.
        if self.kind == OperationKind::Git {
            for value in git_path_option_values(&self.args)
                .into_iter()
                .chain(git_no_index_operands(&self.args))
            {
                let normalized = if Path::new(&value).is_absolute() {
                    normalize_path(&value)?
                } else {
                    normalize_path(&format!("{}/{}", cwd, value))?
                };
                if !path_is_within(&normalized, &root) {
                    return Err(unknown("git option path escapes its workspace"));
                }
            }
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
        // ACP rawInput may omit structured readPaths. For the narrow
        // read-only process allowlist, inspect literal path operands as well
        // so an absolute or traversal path cannot silently escape the
        // workspace just because the adapter had no platform PathOptions.
        if self.kind == OperationKind::Process && self.category() == OperationCategory::ReadOnly {
            for path in self.args.iter().filter(is_literal_path_operand) {
                let normalized = if Path::new(path).is_absolute() {
                    normalize_path(path)?
                } else {
                    normalize_path(&format!("{}/{}", cwd, path))?
                };
                if !path_is_within(&normalized, &root) {
                    return Err(unknown("raw process path escapes its workspace"));
                }
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

/// A non-option argv token that explicitly names a filesystem location.
/// Bare patterns such as `TODO` stay ordinary search terms; paths and
/// traversal operands are containment-checked against the workspace.
fn is_literal_path_operand(arg: &&String) -> bool {
    let value = arg.as_str();
    !value.starts_with('-')
        && (Path::new(value).is_absolute()
            || value.starts_with("./")
            || value.starts_with(".\\")
            || value.starts_with("../")
            || value.starts_with("..\\")
            || value.contains('/')
            || value.contains('\\'))
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
        // `rg --pre <command>` spawns a subprocess; treat it as unknown.
        "rg" => {
            if args.iter().any(|arg| {
                let value = arg.to_ascii_lowercase();
                value == "--pre" || value.starts_with("--pre=")
            }) {
                return OperationCategory::Unknown;
            }
            OperationCategory::ReadOnly
        }
        // `fd --exec/-x/--exec-batch/-X <command>` spawns subprocesses.
        "fd" => {
            if args.iter().any(|arg| {
                let value = arg.to_ascii_lowercase();
                matches!(value.as_str(), "-x" | "--exec" | "-X" | "--exec-batch")
                    || value.starts_with("--exec=")
                    || value.starts_with("--exec-batch=")
            }) {
                return OperationCategory::Unknown;
            }
            OperationCategory::ReadOnly
        }
        "where" => OperationCategory::ReadOnly,
        "rm" | "rmdir" | "del" | "erase" | "remove-item" => OperationCategory::Destructive,
        _ => OperationCategory::Unknown,
    }
}

/// Collects values of Git path-type global options for containment checks.
fn git_path_option_values(args: &[String]) -> Vec<String> {
    let mut values = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let lower = arg.to_ascii_lowercase();
        if matches!(lower.as_str(), "-c" | "--git-dir" | "--work-tree") {
            if let Some(value) = iter.next() {
                values.push(value.clone());
            }
        } else if let Some(value) = lower.strip_prefix("--git-dir=") {
            values.push(value.to_string());
        } else if let Some(value) = lower.strip_prefix("--work-tree=") {
            values.push(value.to_string());
        }
    }
    values
}

/// `git diff --no-index` compares arbitrary filesystem operands rather than
/// repository paths. They are therefore part of the workspace boundary.
fn git_no_index_operands(args: &[String]) -> Vec<String> {
    let Some(index) = args
        .iter()
        .position(|arg| arg.eq_ignore_ascii_case("--no-index"))
    else {
        return Vec::new();
    };
    args[index + 1..]
        .iter()
        .filter(|arg| arg.as_str() != "--" && !arg.starts_with('-'))
        .cloned()
        .collect()
}

fn classify_git(executable: Option<&str>, args: &[String]) -> OperationCategory {
    if executable_name(executable).as_deref() != Some("git") || args.is_empty() {
        return OperationCategory::Unknown;
    }
    // Any git invocation that writes a file or spawns an external program is
    // never read-only, regardless of the subcommand.
    if args.iter().any(|arg| {
        let value = arg.to_ascii_lowercase();
        value == "--ext-diff"
            || value == "--textconv"
            || value == "--output"
            || value == "-o"
            || value.starts_with("--output=")
    }) {
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

impl ApprovalStore for () {
    fn consume_matching(
        &self,
        _context: &ExecutionContext,
        _operation_digest: &str,
        _now_epoch_seconds: u64,
    ) -> Result<Option<ApprovalEvidence>, DomainError> {
        Ok(None)
    }
}

impl ExecutionGuard<()> {
    /// Authorize a backend-owned managed operation after the caller has
    /// derived every target from persisted state. Unlike ACP approval, this
    /// path accepts no Renderer workspace and requires exact cwd/write-target
    /// equality with the independently proven values supplied by the module.
    pub fn authorize_managed(
        operation: &OperationDescriptor,
        allowed_cwd: &Path,
        allowed_write_paths: &[&Path],
    ) -> Result<(), DomainError> {
        let managed_git = operation.kind == OperationKind::Git
            && operation.executable.as_deref() == Some("git")
            && matches!(
                operation.args.as_slice(),
                [command, subcommand, ..]
                    if (command == "worktree" && matches!(subcommand.as_str(), "add" | "remove"))
                        || (command == "branch" && subcommand == "-D")
                        || (command == "bundle" && subcommand == "create")
                        || (command == "update-ref" && operation.args.len() == 4)
            );
        if (!managed_git
            && matches!(
                operation.category(),
                OperationCategory::Unknown | OperationCategory::ReadOnly
            ))
            || operation.write_paths.is_empty()
        {
            return Err(unknown("managed operation is not an authorized write"));
        }
        let cwd = normalize_path(&operation.cwd)?;
        let allowed_cwd = normalize_path(&allowed_cwd.to_string_lossy())?;
        if cwd != allowed_cwd {
            return Err(unknown("managed operation cwd changed"));
        }
        if operation.write_paths.len() != allowed_write_paths.len() {
            return Err(unknown("managed operation target set changed"));
        }
        for (actual, allowed) in operation.write_paths.iter().zip(allowed_write_paths) {
            if normalize_path(actual)? != normalize_path(&allowed.to_string_lossy())? {
                return Err(unknown("managed operation target changed"));
            }
        }
        Ok(())
    }
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

    #[test]
    fn raw_readonly_process_argument_cannot_escape_workspace() {
        let outside = process("rg.exe", &["secret", "D:/outside"]);
        assert_eq!(outside.category(), OperationCategory::ReadOnly);
        assert!(
            outside.validate_within("C:/repo").is_err(),
            "an explicit rawInput path must not bypass workspace containment"
        );
    }

    #[test]
    fn destructive_process_requires_declared_contained_targets() {
        let outside = process("rm.exe", &["D:/outside/victim.txt"]);
        assert_eq!(outside.category(), OperationCategory::Unknown);
        assert!(outside.validate_within("C:/repo").is_err());

        let mut inside = process("rm.exe", &["victim.txt"]);
        inside.write_paths.push("victim.txt".into());
        assert_eq!(inside.category(), OperationCategory::Destructive);
        assert!(inside.validate_within("C:/repo").is_ok());
    }

    fn process(name: &str, args: &[&str]) -> OperationDescriptor {
        OperationDescriptor {
            kind: OperationKind::Process,
            executable: Some(name.into()),
            args: args.iter().map(|v| (*v).into()).collect(),
            cwd: "C:/repo".into(),
            read_paths: vec![],
            write_paths: vec![],
        }
    }

    #[test]
    fn read_only_tools_cannot_spawn_subprocesses() {
        // `rg --pre <command>` and `fd --exec/-x <command>` execute programs.
        assert_eq!(
            process("rg.exe", &["--pre", "sh -c evil"]).category(),
            OperationCategory::Unknown
        );
        assert_eq!(
            process("rg.exe", &["--pre=python evil.py", "pattern"]).category(),
            OperationCategory::Unknown
        );
        assert_eq!(
            process("fd.exe", &["--exec", "rm"]).category(),
            OperationCategory::Unknown
        );
        assert_eq!(
            process("fd.exe", &["-x", "git", "checkout"]).category(),
            OperationCategory::Unknown
        );
        // Plain read-only invocations stay allowed.
        assert_eq!(
            process("rg.exe", &["--files", "src"]).category(),
            OperationCategory::ReadOnly
        );
        assert_eq!(
            process("fd.exe", &["-e", "rs", "src"]).category(),
            OperationCategory::ReadOnly
        );
    }

    #[test]
    fn git_read_only_commands_cannot_write_or_spawn() {
        assert_eq!(
            git(&["diff", "--output=evil.txt"]).category(),
            OperationCategory::Unknown
        );
        assert_eq!(
            git(&["diff", "--output", "evil.txt"]).category(),
            OperationCategory::Unknown
        );
        assert_eq!(
            git(&["diff", "--ext-diff", "HEAD"]).category(),
            OperationCategory::Unknown
        );
        assert_eq!(
            git(&["log", "--textconv"]).category(),
            OperationCategory::Unknown
        );
        assert_eq!(
            git(&["diff", "HEAD"]).category(),
            OperationCategory::ReadOnly
        );
    }

    #[test]
    fn git_path_options_must_stay_inside_the_workspace() {
        // -C outside the workspace fails containment.
        assert!(git(&["-C", "D:/elsewhere", "status"])
            .validate_within("C:/repo")
            .is_err());
        // --git-dir=value and --work-tree=value forms are covered too.
        assert!(git(&["--git-dir=C:/elsewhere/.git", "status"])
            .validate_within("C:/repo")
            .is_err());
        assert!(git(&["--work-tree", "D:/elsewhere", "status"])
            .validate_within("C:/repo")
            .is_err());
        // In-workspace values are fine.
        assert!(git(&["-C", "C:/repo/sub", "status"])
            .validate_within("C:/repo")
            .is_ok());
        assert!(git(&["--git-dir=C:/repo/.git", "status"])
            .validate_within("C:/repo")
            .is_ok());
    }

    #[test]
    fn git_no_index_operands_must_stay_inside_the_workspace() {
        assert!(
            git(&["diff", "--no-index", "D:/outside/a", "D:/outside/b",])
                .validate_within("C:/repo")
                .is_err()
        );
        assert!(git(&["diff", "--no-index", "C:/repo/a", "C:/repo/b",])
            .validate_within("C:/repo")
            .is_ok());
    }
}
