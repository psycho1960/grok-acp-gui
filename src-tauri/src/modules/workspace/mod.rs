//! MOD-WORKSPACE public interface for managed Git worktree lifecycle.

use crate::adapters::filesystem::{
    canonicalize_existing_directory, validate_managed_worktree_target,
};
use crate::adapters::git_cli::{GitCli, RepositoryInspection};
use crate::domain::types::{RecoveryId, RecoveryItem, RecoveryState};
use crate::domain::types::{TaskId, WorktreeId, WorktreeOwnership, WorktreeRecord, WorktreeState};
use crate::modules::persistence::Repository;
use crate::modules::task_runtime::permission::{
    ExecutionGuard, OperationDescriptor, OperationKind,
};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceError {
    pub code: &'static str,
    pub message: &'static str,
}

impl std::fmt::Display for WorkspaceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for WorkspaceError {}

#[derive(Debug, Clone)]
pub struct CreateManagedWorktree {
    pub repo_root: PathBuf,
    pub task_id: TaskId,
    pub task_slug: String,
    pub base_ref: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryEvidence {
    pub id: String,
    pub manifest_path: PathBuf,
    pub branch_bundle: PathBuf,
    pub tracked_patch: PathBuf,
    pub untracked_zip: PathBuf,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovalPreparation {
    pub confirmation_token: String,
    pub absolute_path: PathBuf,
    pub dirty: bool,
    pub untracked_files: usize,
    pub force_required: bool,
    pub recovery: Option<RecoveryEvidence>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdoptionPreparation {
    pub confirmation_token: String,
    pub absolute_path: PathBuf,
}

#[derive(Debug, Clone)]
struct PendingRemoval {
    token: String,
    record: WorktreeRecord,
    recovery: Option<RecoveryEvidence>,
    expires_at: std::time::Instant,
    content_digest: String,
}

#[derive(Debug, Clone)]
struct PendingAdoption {
    token: String,
    task_id: TaskId,
    path: PathBuf,
    expires_at: std::time::Instant,
}

pub trait WorkspaceService: Send + Sync {
    fn inspect_repository(&self, path: &Path) -> Result<RepositoryInspection, WorkspaceError>;
    fn create_managed_worktree(
        &self,
        request: CreateManagedWorktree,
    ) -> Result<WorktreeRecord, WorkspaceError>;
    fn list_worktrees(&self, repo_root: &Path) -> Result<Vec<WorktreeRecord>, WorkspaceError>;
    fn prepare_adoption(
        &self,
        task_id: TaskId,
        path: &Path,
    ) -> Result<AdoptionPreparation, WorkspaceError>;
    fn adopt_worktree(
        &self,
        task_id: TaskId,
        path: &Path,
        confirmation_token: &str,
        confirmed_path: &Path,
    ) -> Result<WorktreeRecord, WorkspaceError>;
    fn prepare_removal(&self, task_id: &str) -> Result<RemovalPreparation, WorkspaceError>;
    fn remove_managed_worktree(
        &self,
        task_id: &str,
        confirmation_token: &str,
        confirmed_path: &Path,
    ) -> Result<WorktreeRecord, WorkspaceError>;
    fn inspect_worktree(&self, task_id: &str) -> Result<WorktreeRecord, WorkspaceError>;
    fn reconcile_registry(&self) -> Result<Vec<WorktreeRecord>, WorkspaceError>;
}

pub struct ManagedWorkspaceService {
    repo: Arc<dyn Repository>,
    git: GitCli,
    managed_root: PathBuf,
    #[allow(dead_code)]
    recovery_root: PathBuf,
    lifecycle_lock: Mutex<()>,
    pending_removals: Mutex<HashMap<String, PendingRemoval>>,
    pending_adoptions: Mutex<HashMap<String, PendingAdoption>>,
}

impl ManagedWorkspaceService {
    pub fn new(repo: Arc<dyn Repository>, managed_root: PathBuf, recovery_root: PathBuf) -> Self {
        Self {
            repo,
            git: GitCli::default(),
            managed_root,
            recovery_root,
            lifecycle_lock: Mutex::new(()),
            pending_removals: Mutex::new(HashMap::new()),
            pending_adoptions: Mutex::new(HashMap::new()),
        }
    }

    fn ensure_managed_root(&self) -> Result<PathBuf, WorkspaceError> {
        if !self.managed_root.is_absolute() {
            return Err(workspace_error(
                "WORKTREE_INVALID_ROOT",
                "Managed worktree root must be absolute",
            ));
        }
        let parent = self.managed_root.parent().ok_or_else(|| {
            workspace_error("WORKTREE_INVALID_ROOT", "Managed root has no safe parent")
        })?;
        authorize_managed_fs_write(parent, &[&self.managed_root])?;
        std::fs::create_dir_all(&self.managed_root).map_err(|_| {
            workspace_error(
                "WORKTREE_ROOT_UNAVAILABLE",
                "Managed worktree root is unavailable",
            )
        })?;
        let canonical = canonicalize_existing_directory(&self.managed_root).map_err(|_| {
            workspace_error(
                "WORKTREE_ROOT_UNAVAILABLE",
                "Managed worktree root is unavailable",
            )
        })?;
        if canonical.parent().is_none()
            || std::env::var_os("USERPROFILE")
                .and_then(|home| std::fs::canonicalize(home).ok())
                .is_some_and(|home| same_path_identity(&home, &canonical))
        {
            return Err(workspace_error(
                "WORKTREE_INVALID_ROOT",
                "Managed worktree root is a protected path",
            ));
        }
        Ok(canonical)
    }
}

impl WorkspaceService for ManagedWorkspaceService {
    fn inspect_repository(&self, path: &Path) -> Result<RepositoryInspection, WorkspaceError> {
        self.git.inspect_repository(path).map_err(map_git_error)
    }

    fn create_managed_worktree(
        &self,
        request: CreateManagedWorktree,
    ) -> Result<WorktreeRecord, WorkspaceError> {
        let _lock = self.lifecycle_lock.lock().map_err(|_| {
            workspace_error("WORKTREE_LOCKED", "Worktree lifecycle lock is unavailable")
        })?;
        let task_key = validate_identifier(&request.task_id.0, "task ID")?;
        let task = self
            .repo
            .get_task(&request.task_id.0)
            .map_err(map_repo_error)?;
        let project = self
            .repo
            .get_project(&task.project_id.0)
            .map_err(map_repo_error)?;
        let trusted_repo_root = project.repo_root.ok_or_else(|| {
            workspace_error(
                "WORKTREE_OUTSIDE_REPO",
                "Task project is not a Git repository",
            )
        })?;
        let trusted_repository = self
            .git
            .inspect_repository(Path::new(&trusted_repo_root))
            .map_err(map_git_error)?;
        let supplied_repository = self
            .git
            .inspect_repository(&request.repo_root)
            .map_err(map_git_error)?;
        if !same_path_identity(
            &trusted_repository.common_git_dir,
            &supplied_repository.common_git_dir,
        ) {
            return Err(workspace_error(
                "WORKTREE_REPOSITORY_MISMATCH",
                "Worktree repository does not belong to the task project",
            ));
        }
        let base_ref = trusted_repository
            .branch
            .clone()
            .unwrap_or_else(|| trusted_repository.head.clone());
        validate_git_ref(&base_ref)?;
        let slug = slugify(&task.title);
        let branch = format!("gag/{task_key}-{slug}");
        if branch.len() > 240 {
            return Err(workspace_error(
                "WORKTREE_INVALID_BRANCH",
                "Managed branch name is too long",
            ));
        }

        let repository = trusted_repository;
        let managed_root = self.ensure_managed_root()?;
        if repository.canonical_root == managed_root
            || repository.canonical_root.starts_with(&managed_root)
            || managed_root.starts_with(&repository.canonical_root)
        {
            return Err(workspace_error(
                "WORKTREE_INVALID_ROOT",
                "Managed root must be separate from the repository",
            ));
        }

        let existing = self
            .repo
            .list_worktrees_by_task(&request.task_id.0)
            .map_err(map_repo_error)?;
        if let Some(record) = existing.into_iter().find(|record| {
            record.ownership == WorktreeOwnership::Managed && record.state != WorktreeState::Deleted
        }) {
            let listed = self
                .git
                .list_worktrees(&repository.canonical_root)
                .map_err(map_git_error)?;
            let expected = std::fs::canonicalize(&record.path).map_err(|_| {
                workspace_error("WORKTREE_MISSING", "Registered worktree is missing")
            })?;
            if listed.iter().any(|item| {
                same_path_identity(&item.path, &expected)
                    && item.branch.as_deref() == Some(record.branch.as_str())
            }) {
                return Ok(record);
            }
            return Err(workspace_error(
                "WORKTREE_REGISTRY_MISMATCH",
                "Registered worktree does not match Git metadata",
            ));
        }

        let repo_identity = hex_prefix(&repository.common_git_dir.to_string_lossy(), 16);
        let task_path = task_key.to_ascii_lowercase();
        let parent = managed_root.join(&repo_identity);
        authorize_managed_fs_write(&managed_root, &[&parent])?;
        std::fs::create_dir_all(&parent).map_err(|_| {
            workspace_error(
                "WORKTREE_CREATE_FAILED",
                "Worktree parent could not be created",
            )
        })?;
        let target = validate_managed_worktree_target(&managed_root, &parent.join(task_path))
            .map_err(|_| {
                workspace_error(
                    "WORKTREE_OUTSIDE_MANAGED_ROOT",
                    "Worktree target failed managed-root validation",
                )
            })?;
        if target.exists() {
            let listed = self
                .git
                .list_worktrees(&repository.canonical_root)
                .map_err(map_git_error)?;
            if let Some(item) = listed.iter().find(|item| {
                same_path_identity(&item.path, &target)
                    && item.branch.as_deref() == Some(branch.as_str())
            }) {
                let now = crate::domain::types::utc_now();
                let repaired = WorktreeRecord {
                    id: WorktreeId::new(format!("wt-{}", uuid::Uuid::new_v4())),
                    task_id: request.task_id,
                    repo_root: repository.canonical_root.to_string_lossy().into_owned(),
                    path: target.to_string_lossy().into_owned(),
                    display_path: target
                        .strip_prefix(&managed_root)
                        .unwrap_or(&target)
                        .to_string_lossy()
                        .into_owned(),
                    branch,
                    base_branch: base_ref,
                    base_commit: item.head.clone().unwrap_or_else(|| repository.head.clone()),
                    ownership: WorktreeOwnership::Managed,
                    state: WorktreeState::Ready,
                    repo_identity,
                    common_git_dir: repository.common_git_dir.to_string_lossy().into_owned(),
                    relative_path: target
                        .strip_prefix(&managed_root)
                        .unwrap_or(&target)
                        .to_string_lossy()
                        .into_owned(),
                    created_at: now.clone(),
                    last_verified_at: now,
                    recovery_bundle_id: None,
                    disk_usage_bytes: directory_size(&target).unwrap_or(0),
                    locked: item.locked,
                    merged: false,
                };
                self.repo
                    .create_worktree(&repaired)
                    .map_err(map_repo_error)?;
                return Ok(repaired);
            }
            return Err(workspace_error(
                "WORKTREE_ALREADY_EXISTS",
                "Worktree target exists but does not match the task registration",
            ));
        }
        let base_commit = String::from_utf8(
            self.git
                .capture(
                    &repository.canonical_root,
                    &["rev-parse", "--verify", &format!("{}^{{commit}}", base_ref)],
                )
                .map_err(map_git_error)?,
        )
        .ok()
        .and_then(|value| value.lines().next().map(str::trim).map(str::to_owned))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| workspace_error("WORKTREE_INVALID_BASE", "Base ref is not a commit"))?;

        let target_arg = target.to_string_lossy().into_owned();
        authorize_managed_git(
            &repository.canonical_root,
            &["worktree", "add", "-b", &branch, &target_arg, &base_commit],
            &[&repository.common_git_dir, &target],
        )?;
        self.git
            .run_checked(
                &repository.canonical_root,
                &["worktree", "add", "-b", &branch, &target_arg, &base_commit],
            )
            .map_err(map_git_error)?;

        let verified = self
            .git
            .list_worktrees(&repository.canonical_root)
            .map_err(map_git_error)?
            .into_iter()
            .any(|item| {
                same_path_identity(&item.path, &target)
                    && item.branch.as_deref() == Some(branch.as_str())
            });
        if !verified {
            rollback_created_worktree(&self.git, &repository.canonical_root, &target_arg, &branch);
            return Err(workspace_error(
                "WORKTREE_CREATE_FAILED",
                "Created worktree could not be verified",
            ));
        }

        let record = WorktreeRecord {
            id: WorktreeId::new(format!("wt-{}", uuid::Uuid::new_v4())),
            task_id: request.task_id,
            repo_root: repository.canonical_root.to_string_lossy().into_owned(),
            path: target_arg.clone(),
            display_path: target
                .strip_prefix(&managed_root)
                .unwrap_or(&target)
                .to_string_lossy()
                .into_owned(),
            branch: branch.clone(),
            base_branch: base_ref,
            base_commit,
            ownership: WorktreeOwnership::Managed,
            state: WorktreeState::Ready,
            repo_identity,
            common_git_dir: repository.common_git_dir.to_string_lossy().into_owned(),
            relative_path: target
                .strip_prefix(&managed_root)
                .unwrap_or(&target)
                .to_string_lossy()
                .into_owned(),
            created_at: crate::domain::types::utc_now(),
            last_verified_at: crate::domain::types::utc_now(),
            recovery_bundle_id: None,
            disk_usage_bytes: directory_size(&target).unwrap_or(0),
            locked: false,
            merged: false,
        };
        if self.repo.create_worktree(&record).is_err() {
            rollback_created_worktree(&self.git, &repository.canonical_root, &target_arg, &branch);
            return Err(workspace_error(
                "WORKTREE_REGISTRY_FAILED",
                "Worktree registry update failed",
            ));
        }
        Ok(record)
    }

    fn list_worktrees(&self, repo_root: &Path) -> Result<Vec<WorktreeRecord>, WorkspaceError> {
        let repository = self
            .git
            .inspect_repository(repo_root)
            .map_err(map_git_error)?;
        let listed = self
            .git
            .list_worktrees(&repository.canonical_root)
            .map_err(map_git_error)?;
        let records = self.repo.list_active_worktrees().map_err(map_repo_error)?;
        Ok(records
            .into_iter()
            .filter(|record| {
                Path::new(&record.repo_root) == repository.canonical_root
                    && listed
                        .iter()
                        .any(|item| same_path_identity(&item.path, Path::new(&record.path)))
            })
            .collect())
    }

    fn prepare_adoption(
        &self,
        task_id: TaskId,
        path: &Path,
    ) -> Result<AdoptionPreparation, WorkspaceError> {
        let task = self.repo.get_task(&task_id.0).map_err(map_repo_error)?;
        if !self
            .repo
            .list_worktrees_by_task(&task_id.0)
            .map_err(map_repo_error)?
            .is_empty()
        {
            return Err(workspace_error(
                "WORKTREE_ALREADY_EXISTS",
                "Task already has a worktree registration",
            ));
        }
        let project = self
            .repo
            .get_project(&task.project_id.0)
            .map_err(map_repo_error)?;
        let trusted_root = project.repo_root.ok_or_else(|| {
            workspace_error(
                "WORKTREE_OUTSIDE_REPO",
                "Task project is not a Git repository",
            )
        })?;
        let trusted = self
            .git
            .inspect_repository(Path::new(&trusted_root))
            .map_err(map_git_error)?;
        let candidate = canonicalize_existing_directory(path).map_err(|_| {
            workspace_error("WORKTREE_MISSING", "External worktree is not accessible")
        })?;
        let external = self
            .git
            .inspect_repository(&candidate)
            .map_err(map_git_error)?;
        if !same_path_identity(&trusted.common_git_dir, &external.common_git_dir)
            || same_path_identity(&candidate, &trusted.canonical_root)
        {
            return Err(workspace_error(
                "WORKTREE_REPOSITORY_MISMATCH",
                "External worktree does not belong to the task project",
            ));
        }
        let listed = self
            .git
            .list_worktrees(&trusted.canonical_root)
            .map_err(map_git_error)?;
        if !listed.iter().any(|item| {
            same_path_identity(&item.path, &candidate) && item.branch.is_some() && !item.bare
        }) {
            return Err(workspace_error(
                "WORKTREE_REGISTRY_MISMATCH",
                "External path is not a branch worktree in Git metadata",
            ));
        }
        let token = uuid::Uuid::new_v4().to_string();
        self.pending_adoptions
            .lock()
            .map_err(|_| {
                workspace_error("WORKTREE_LOCKED", "Adoption confirmation is unavailable")
            })?
            .insert(
                task_id.0.clone(),
                PendingAdoption {
                    token: token.clone(),
                    task_id,
                    path: candidate.clone(),
                    expires_at: std::time::Instant::now() + std::time::Duration::from_secs(600),
                },
            );
        Ok(AdoptionPreparation {
            confirmation_token: token,
            absolute_path: candidate,
        })
    }

    fn adopt_worktree(
        &self,
        task_id: TaskId,
        path: &Path,
        confirmation_token: &str,
        confirmed_path: &Path,
    ) -> Result<WorktreeRecord, WorkspaceError> {
        let _lock = self.lifecycle_lock.lock().map_err(|_| {
            workspace_error("WORKTREE_LOCKED", "Worktree lifecycle lock is unavailable")
        })?;
        let pending = self
            .pending_adoptions
            .lock()
            .map_err(|_| {
                workspace_error("WORKTREE_LOCKED", "Adoption confirmation is unavailable")
            })?
            .get(&task_id.0)
            .cloned()
            .ok_or_else(|| {
                workspace_error(
                    "WORKTREE_CONFIRMATION_REQUIRED",
                    "External worktree adoption must be prepared first",
                )
            })?;
        let confirmed = canonicalize_existing_directory(confirmed_path).map_err(|_| {
            workspace_error(
                "WORKTREE_CONFIRMATION_INVALID",
                "Confirmed adoption path is invalid",
            )
        })?;
        if pending.task_id != task_id
            || pending.token != confirmation_token
            || std::time::Instant::now() > pending.expires_at
            || !same_path_identity(&pending.path, &confirmed)
            || !same_path_identity(&pending.path, path)
        {
            return Err(workspace_error(
                "WORKTREE_CONFIRMATION_INVALID",
                "External worktree adoption confirmation does not match",
            ));
        }
        let task = self.repo.get_task(&task_id.0).map_err(map_repo_error)?;
        let project = self
            .repo
            .get_project(&task.project_id.0)
            .map_err(map_repo_error)?;
        if !self
            .repo
            .list_worktrees_by_task(&task_id.0)
            .map_err(map_repo_error)?
            .is_empty()
        {
            return Err(workspace_error(
                "WORKTREE_ALREADY_EXISTS",
                "Task already has a worktree registration",
            ));
        }
        let candidate = std::fs::canonicalize(path).map_err(|_| {
            workspace_error("WORKTREE_MISSING", "External worktree is not accessible")
        })?;
        let repository = self
            .git
            .inspect_repository(&candidate)
            .map_err(map_git_error)?;
        let listed = self.git.list_worktrees(&candidate).map_err(map_git_error)?;
        let item = listed
            .iter()
            .find(|item| same_path_identity(&item.path, &candidate))
            .ok_or_else(|| {
                workspace_error(
                    "WORKTREE_REGISTRY_MISMATCH",
                    "Path is not present in Git worktree metadata",
                )
            })?;
        let branch = item.branch.clone().ok_or_else(|| {
            workspace_error(
                "WORKTREE_INVALID_BRANCH",
                "Detached external worktree cannot be adopted",
            )
        })?;
        let primary = listed
            .iter()
            .find(|other| !same_path_identity(&other.path, &candidate) && !other.bare)
            .ok_or_else(|| {
                workspace_error(
                    "WORKTREE_EXTERNAL_READ_ONLY",
                    "Primary checkout cannot be adopted as an external worktree",
                )
            })?;
        let primary_repository = self
            .git
            .inspect_repository(&primary.path)
            .map_err(map_git_error)?;
        let trusted_root = project.repo_root.ok_or_else(|| {
            workspace_error(
                "WORKTREE_OUTSIDE_REPO",
                "Task project is not a Git repository",
            )
        })?;
        let trusted_repository = self
            .git
            .inspect_repository(Path::new(&trusted_root))
            .map_err(map_git_error)?;
        if !same_path_identity(
            &trusted_repository.common_git_dir,
            &primary_repository.common_git_dir,
        ) {
            return Err(workspace_error(
                "WORKTREE_REPOSITORY_MISMATCH",
                "External worktree does not belong to the task project",
            ));
        }
        let status = self
            .git
            .capture(
                &candidate,
                &["status", "--porcelain=v2", "--untracked-files=all"],
            )
            .map_err(map_git_error)?;
        let now = crate::domain::types::utc_now();
        let record = WorktreeRecord {
            id: WorktreeId::new(format!("wt-{}", uuid::Uuid::new_v4())),
            task_id,
            repo_root: primary_repository
                .canonical_root
                .to_string_lossy()
                .into_owned(),
            path: candidate.to_string_lossy().into_owned(),
            display_path: candidate.to_string_lossy().into_owned(),
            branch,
            base_branch: primary_repository
                .branch
                .unwrap_or_else(|| primary_repository.head.clone()),
            base_commit: item.head.clone().unwrap_or(repository.head),
            ownership: WorktreeOwnership::Adopted,
            state: if status.is_empty() {
                WorktreeState::Ready
            } else {
                WorktreeState::Dirty
            },
            repo_identity: hex_prefix(&primary_repository.common_git_dir.to_string_lossy(), 16),
            common_git_dir: primary_repository
                .common_git_dir
                .to_string_lossy()
                .into_owned(),
            relative_path: String::new(),
            created_at: now.clone(),
            last_verified_at: now,
            recovery_bundle_id: None,
            disk_usage_bytes: directory_size(&candidate).unwrap_or(0),
            locked: item.locked,
            merged: false,
        };
        self.repo.create_worktree(&record).map_err(map_repo_error)?;
        self.pending_adoptions
            .lock()
            .map_err(|_| {
                workspace_error("WORKTREE_LOCKED", "Adoption confirmation is unavailable")
            })?
            .remove(&record.task_id.0);
        Ok(record)
    }

    fn prepare_removal(&self, task_id: &str) -> Result<RemovalPreparation, WorkspaceError> {
        let _lock = self.lifecycle_lock.lock().map_err(|_| {
            workspace_error("WORKTREE_LOCKED", "Worktree lifecycle lock is unavailable")
        })?;
        let record = active_managed_record(self.repo.as_ref(), task_id)?;
        let target = self.prove_registered_target(&record)?;
        let fresh_merged = self
            .git
            .is_ancestor(&target, &record.branch, &record.base_branch)
            .map_err(map_git_error)?;
        self.repo
            .begin_worktree_removal(task_id, &record.id.0)
            .map_err(map_repo_error)?;
        let prepared = (|| {
            let status = self
                .git
                .capture(
                    &target,
                    &["status", "--porcelain=v2", "--untracked-files=all"],
                )
                .map_err(map_git_error)?;
            let untracked = self
                .git
                .capture(
                    &target,
                    &["ls-files", "-z", "--others", "--exclude-standard"],
                )
                .map_err(map_git_error)?;
            let untracked_files = split_nul(&untracked).count();
            let dirty = !status.is_empty();
            let content_digest = workspace_content_digest(&self.git, &target, &untracked)?;
            let recovery = (dirty || !fresh_merged)
                .then(|| self.create_recovery_package(&record, &target, &untracked))
                .transpose()?;
            if let Some(evidence) = recovery.as_ref() {
                verify_recovery_package(evidence)?;
            }
            Ok((untracked_files, dirty, content_digest, recovery))
        })();
        let (untracked_files, dirty, content_digest, recovery) = match prepared {
            Ok(value) => value,
            Err(error) => {
                let _ = self.repo.update_worktree(&record);
                return Err(error);
            }
        };
        let token = uuid::Uuid::new_v4().to_string();
        let mut closing = record.clone();
        closing.state = WorktreeState::Closing;
        closing.last_verified_at = crate::domain::types::utc_now();
        closing.recovery_bundle_id = recovery.as_ref().map(|item| item.id.clone());
        closing.disk_usage_bytes = directory_size(&target).unwrap_or(closing.disk_usage_bytes);
        closing.merged = fresh_merged;
        self.repo
            .update_worktree(&closing)
            .map_err(map_repo_error)?;
        self.pending_removals
            .lock()
            .map_err(|_| workspace_error("WORKTREE_LOCKED", "Removal confirmation is unavailable"))?
            .insert(
                task_id.to_owned(),
                PendingRemoval {
                    token: token.clone(),
                    record: closing,
                    recovery: recovery.clone(),
                    expires_at: std::time::Instant::now() + std::time::Duration::from_secs(600),
                    content_digest,
                },
            );
        Ok(RemovalPreparation {
            confirmation_token: token,
            absolute_path: target,
            dirty,
            untracked_files,
            force_required: dirty || !fresh_merged,
            recovery,
        })
    }

    fn remove_managed_worktree(
        &self,
        task_id: &str,
        confirmation_token: &str,
        confirmed_path: &Path,
    ) -> Result<WorktreeRecord, WorkspaceError> {
        let _lock = self.lifecycle_lock.lock().map_err(|_| {
            workspace_error("WORKTREE_LOCKED", "Worktree lifecycle lock is unavailable")
        })?;
        let pending = self
            .pending_removals
            .lock()
            .map_err(|_| workspace_error("WORKTREE_LOCKED", "Removal confirmation is unavailable"))?
            .get(task_id)
            .cloned()
            .ok_or_else(|| {
                workspace_error(
                    "WORKTREE_CONFIRMATION_REQUIRED",
                    "Removal must be prepared immediately before execution",
                )
            })?;
        ensure_task_not_running(self.repo.as_ref(), task_id)?;
        if pending.token != confirmation_token {
            return Err(workspace_error(
                "WORKTREE_CONFIRMATION_INVALID",
                "Removal confirmation is invalid",
            ));
        }
        if std::time::Instant::now() > pending.expires_at {
            return Err(workspace_error(
                "WORKTREE_CONFIRMATION_EXPIRED",
                "Removal confirmation has expired; prepare again",
            ));
        }
        let target = self.prove_registered_target(&pending.record)?;
        let confirmed = std::fs::canonicalize(confirmed_path).map_err(|_| {
            workspace_error(
                "WORKTREE_CONFIRMATION_INVALID",
                "Confirmed path is not accessible",
            )
        })?;
        if !same_path_identity(&target, &confirmed) {
            return Err(workspace_error(
                "WORKTREE_CONFIRMATION_INVALID",
                "Confirmed path does not match the removal target",
            ));
        }
        let status = self
            .git
            .capture(
                &target,
                &["status", "--porcelain=v2", "--untracked-files=all"],
            )
            .map_err(map_git_error)?;
        let current_untracked = self
            .git
            .capture(
                &target,
                &["ls-files", "-z", "--others", "--exclude-standard"],
            )
            .map_err(map_git_error)?;
        if workspace_content_digest(&self.git, &target, &current_untracked)?
            != pending.content_digest
        {
            return Err(workspace_error(
                "WORKTREE_CONFIRMATION_EXPIRED",
                "Worktree changed after removal preparation; prepare again",
            ));
        }
        let current_merged = self
            .git
            .is_ancestor(&target, &pending.record.branch, &pending.record.base_branch)
            .map_err(map_git_error)?;
        if current_merged != pending.record.merged {
            return Err(workspace_error(
                "WORKTREE_CONFIRMATION_EXPIRED",
                "Base branch ancestry changed after removal preparation; prepare again",
            ));
        }
        if !status.is_empty() {
            let evidence = pending.recovery.as_ref().ok_or_else(|| {
                workspace_error(
                    "WORKTREE_RECOVERY_REQUIRED",
                    "Dirty worktree cannot be removed without recovery evidence",
                )
            })?;
            verify_recovery_package(evidence)?;
        }
        let repo_root = PathBuf::from(&pending.record.repo_root);
        let target_arg = target.to_string_lossy().into_owned();
        ensure_task_not_running(self.repo.as_ref(), task_id)?;
        authorize_managed_git(
            &repo_root,
            &["worktree", "remove", "--force", &target_arg],
            &[Path::new(&pending.record.common_git_dir), &target],
        )?;
        self.git
            .run_checked(&repo_root, &["worktree", "remove", "--force", &target_arg])
            .map_err(map_git_error)?;
        authorize_managed_git(
            &repo_root,
            &["branch", "-D", &pending.record.branch],
            &[Path::new(&pending.record.common_git_dir)],
        )?;
        self.git
            .run_checked(&repo_root, &["branch", "-D", &pending.record.branch])
            .map_err(map_git_error)?;
        let mut removed = pending.record;
        removed.state = WorktreeState::Removed;
        self.repo
            .update_worktree(&removed)
            .map_err(map_repo_error)?;
        self.pending_removals
            .lock()
            .map_err(|_| workspace_error("WORKTREE_LOCKED", "Removal confirmation is unavailable"))?
            .remove(task_id);
        Ok(removed)
    }

    fn inspect_worktree(&self, task_id: &str) -> Result<WorktreeRecord, WorkspaceError> {
        let mut record = active_registered_record(self.repo.as_ref(), task_id)?;
        let target = if record.ownership == WorktreeOwnership::Managed {
            let managed_root = self.ensure_managed_root()?;
            validate_managed_worktree_target(&managed_root, Path::new(&record.path)).map_err(
                |_| {
                    workspace_error(
                        "WORKTREE_OUTSIDE_MANAGED_ROOT",
                        "Registered worktree failed managed-root validation",
                    )
                },
            )?
        } else {
            std::fs::canonicalize(&record.path).unwrap_or_else(|_| PathBuf::from(&record.path))
        };
        if !target.exists() {
            record.state = WorktreeState::Missing;
            record.last_verified_at = crate::domain::types::utc_now();
            record.locked = false;
            self.repo.update_worktree(&record).map_err(map_repo_error)?;
            return Ok(record);
        }
        let repository = self
            .git
            .inspect_repository(Path::new(&record.repo_root))
            .map_err(map_git_error)?;
        let listed = self
            .git
            .list_worktrees(&repository.canonical_root)
            .map_err(map_git_error)?;
        let Some(item) = listed
            .iter()
            .find(|item| same_path_identity(&item.path, &target))
        else {
            record.state = WorktreeState::Orphaned;
            record.last_verified_at = crate::domain::types::utc_now();
            self.repo.update_worktree(&record).map_err(map_repo_error)?;
            return Ok(record);
        };
        if item.branch.as_deref() != Some(record.branch.as_str()) {
            record.state = WorktreeState::Quarantined;
        } else {
            let status = self
                .git
                .capture(
                    &target,
                    &["status", "--porcelain=v2", "--untracked-files=all"],
                )
                .map_err(map_git_error)?;
            let pending_confirmation = self
                .pending_removals
                .lock()
                .map_err(|_| {
                    workspace_error("WORKTREE_LOCKED", "Removal confirmation is unavailable")
                })?
                .get(task_id)
                .is_some_and(|pending| std::time::Instant::now() <= pending.expires_at);
            if record.state != WorktreeState::Archived
                && (record.state != WorktreeState::Closing || !pending_confirmation)
            {
                record.state = if status.is_empty() {
                    WorktreeState::Ready
                } else {
                    WorktreeState::Dirty
                };
            }
        }
        record.locked = item.locked;
        record.merged = self
            .git
            .is_ancestor(&target, &record.branch, &record.base_branch)
            .unwrap_or(false);
        record.disk_usage_bytes = directory_size(&target).unwrap_or(record.disk_usage_bytes);
        record.last_verified_at = crate::domain::types::utc_now();
        self.repo.update_worktree(&record).map_err(map_repo_error)?;
        Ok(record)
    }

    fn reconcile_registry(&self) -> Result<Vec<WorktreeRecord>, WorkspaceError> {
        // Reconciliation is a safe cancellation boundary. It invalidates all
        // in-memory destructive confirmations; closing records are then
        // re-inspected and restored to ready/dirty when the target still
        // exists. This also repairs stale closing rows after process restart.
        self.pending_removals
            .lock()
            .map_err(|_| workspace_error("WORKTREE_LOCKED", "Removal confirmation is unavailable"))?
            .clear();
        let registered = self.repo.list_active_worktrees().map_err(map_repo_error)?;
        let task_ids: Vec<String> = registered
            .iter()
            .filter(|record| {
                matches!(
                    record.ownership,
                    WorktreeOwnership::Managed | WorktreeOwnership::Adopted
                )
            })
            .map(|record| record.task_id.0.clone())
            .collect();
        let mut reconciled = Vec::with_capacity(task_ids.len());
        for task_id in task_ids {
            reconciled.push(self.inspect_worktree(&task_id)?);
        }
        for project in self.repo.list_projects().map_err(map_repo_error)? {
            let Some(repo_root) = project.repo_root else {
                continue;
            };
            let repository = match self.git.inspect_repository(Path::new(&repo_root)) {
                Ok(value) => value,
                Err(_) => continue,
            };
            for item in self
                .git
                .list_worktrees(&repository.canonical_root)
                .map_err(map_git_error)?
            {
                if item.bare
                    || same_path_identity(&item.path, &repository.canonical_root)
                    || registered
                        .iter()
                        .any(|record| same_path_identity(Path::new(&record.path), &item.path))
                {
                    continue;
                }
                let branch = item.branch.clone().unwrap_or_else(|| "HEAD".into());
                let status = self
                    .git
                    .capture(
                        &item.path,
                        &["status", "--porcelain=v2", "--untracked-files=all"],
                    )
                    .map_err(map_git_error)?;
                let now = crate::domain::types::utc_now();
                reconciled.push(WorktreeRecord {
                    id: WorktreeId::new(format!(
                        "external-{}",
                        hex_prefix(&item.path.to_string_lossy(), 16)
                    )),
                    task_id: TaskId::new(format!(
                        "external-{}",
                        hex_prefix(&item.path.to_string_lossy(), 16)
                    )),
                    repo_root: repository.canonical_root.to_string_lossy().into_owned(),
                    path: item.path.to_string_lossy().into_owned(),
                    display_path: item.path.to_string_lossy().into_owned(),
                    branch,
                    base_branch: repository
                        .branch
                        .clone()
                        .unwrap_or_else(|| repository.head.clone()),
                    base_commit: item.head.unwrap_or_else(|| repository.head.clone()),
                    ownership: WorktreeOwnership::External,
                    state: if status.is_empty() {
                        WorktreeState::Ready
                    } else {
                        WorktreeState::Dirty
                    },
                    repo_identity: hex_prefix(&repository.common_git_dir.to_string_lossy(), 16),
                    common_git_dir: repository.common_git_dir.to_string_lossy().into_owned(),
                    relative_path: String::new(),
                    created_at: now.clone(),
                    last_verified_at: now,
                    recovery_bundle_id: None,
                    disk_usage_bytes: directory_size(&item.path).unwrap_or(0),
                    locked: item.locked,
                    merged: false,
                });
            }
        }
        Ok(reconciled)
    }
}

impl ManagedWorkspaceService {
    fn prove_registered_target(&self, record: &WorktreeRecord) -> Result<PathBuf, WorkspaceError> {
        if record.ownership != WorktreeOwnership::Managed {
            return Err(workspace_error(
                "WORKTREE_EXTERNAL_READ_ONLY",
                "External worktrees cannot be removed",
            ));
        }
        let managed_root = self.ensure_managed_root()?;
        let target = validate_managed_worktree_target(&managed_root, Path::new(&record.path))
            .map_err(|_| {
                workspace_error(
                    "WORKTREE_OUTSIDE_MANAGED_ROOT",
                    "Registered worktree failed managed-root validation",
                )
            })?;
        let repository = self
            .git
            .inspect_repository(Path::new(&record.repo_root))
            .map_err(map_git_error)?;
        let expected_identity = hex_prefix(&repository.common_git_dir.to_string_lossy(), 16);
        let expected_relative = target
            .strip_prefix(&managed_root)
            .map_err(|_| {
                workspace_error(
                    "WORKTREE_REGISTRY_MISMATCH",
                    "Worktree relative path does not match the managed root",
                )
            })?
            .to_string_lossy()
            .into_owned();
        let recorded_common = std::fs::canonicalize(&record.common_git_dir).map_err(|_| {
            workspace_error(
                "WORKTREE_REGISTRY_MISMATCH",
                "Recorded common Git directory is not verifiable",
            )
        })?;
        let deterministic_relative = Path::new(&record.repo_identity)
            .join(record.task_id.0.to_ascii_lowercase())
            .to_string_lossy()
            .into_owned();
        if record.repo_identity != expected_identity
            || !same_path_identity(&recorded_common, &repository.common_git_dir)
            || record.relative_path != expected_relative
            || expected_relative != deterministic_relative
        {
            return Err(workspace_error(
                "WORKTREE_REGISTRY_MISMATCH",
                "Persisted worktree identity proof does not match the target",
            ));
        }
        if same_path_identity(&target, &repository.canonical_root)
            || same_path_identity(&target, &managed_root)
        {
            return Err(workspace_error(
                "WORKTREE_PROTECTED_PATH",
                "Protected path cannot be a removal target",
            ));
        }
        let listed = self
            .git
            .list_worktrees(&repository.canonical_root)
            .map_err(map_git_error)?;
        let item = listed
            .iter()
            .find(|item| same_path_identity(&item.path, &target))
            .ok_or_else(|| workspace_error("WORKTREE_MISSING", "Worktree is missing from Git"))?;
        if item.branch.as_deref() != Some(record.branch.as_str()) {
            return Err(workspace_error(
                "WORKTREE_REGISTRY_MISMATCH",
                "Worktree branch does not match the registry",
            ));
        }
        if item.locked {
            return Err(workspace_error("WORKTREE_LOCKED", "Git worktree is locked"));
        }
        Ok(target)
    }

    fn create_recovery_package(
        &self,
        record: &WorktreeRecord,
        worktree: &Path,
        untracked: &[u8],
    ) -> Result<RecoveryEvidence, WorkspaceError> {
        if !self.recovery_root.is_absolute() {
            return Err(workspace_error(
                "WORKTREE_RECOVERY_FAILED",
                "Recovery root must be absolute",
            ));
        }
        let recovery_parent = self.recovery_root.parent().ok_or_else(|| {
            workspace_error(
                "WORKTREE_RECOVERY_FAILED",
                "Recovery root has no safe parent",
            )
        })?;
        authorize_managed_fs_write(recovery_parent, &[&self.recovery_root])?;
        std::fs::create_dir_all(&self.recovery_root).map_err(|_| {
            workspace_error("WORKTREE_RECOVERY_FAILED", "Recovery root is unavailable")
        })?;
        let id = format!("recovery-{}", uuid::Uuid::new_v4());
        let directory = self.recovery_root.join(&id);
        authorize_managed_fs_write(&self.recovery_root, &[&directory])?;
        std::fs::create_dir(&directory).map_err(|_| {
            workspace_error(
                "WORKTREE_RECOVERY_FAILED",
                "Recovery directory could not be created",
            )
        })?;
        let bundle = directory.join("branch.bundle");
        let patch = directory.join("tracked.patch");
        let archive = directory.join("untracked.zip");
        let manifest = directory.join("manifest.json");
        let bundle_arg = bundle.to_string_lossy().into_owned();
        authorize_managed_git(
            worktree,
            &["bundle", "create", &bundle_arg, &record.branch],
            &[&bundle],
        )?;
        if self
            .git
            .run_checked(worktree, &["bundle", "create", &bundle_arg, &record.branch])
            .is_err()
        {
            return Err(workspace_error(
                "WORKTREE_RECOVERY_FAILED",
                "Branch recovery bundle could not be created",
            ));
        }
        if self
            .git
            .run_checked(worktree, &["bundle", "verify", &bundle_arg])
            .is_err()
        {
            return Err(workspace_error(
                "WORKTREE_RECOVERY_FAILED",
                "Branch recovery bundle could not be verified",
            ));
        }
        let mut tracked = self
            .git
            .capture(worktree, &["diff", "--binary", "HEAD"])
            .map_err(map_git_error)?;
        tracked.extend_from_slice(
            &self
                .git
                .capture(worktree, &["diff", "--binary", "--cached", "HEAD"])
                .map_err(map_git_error)?,
        );
        authorize_managed_fs_write(&directory, &[&patch])?;
        std::fs::write(&patch, tracked).map_err(|_| {
            workspace_error(
                "WORKTREE_RECOVERY_FAILED",
                "Tracked recovery patch could not be written",
            )
        })?;
        authorize_managed_fs_write(&directory, &[&archive])?;
        write_stored_zip(worktree, split_nul(untracked), &archive)?;
        let bundle_hash = sha256_file(&bundle)?;
        let patch_hash = sha256_file(&patch)?;
        let archive_hash = sha256_file(&archive)?;
        let expires_at = crate::bridge::types::utc_after_days(7);
        let manifest_value = serde_json::json!({
            "version": 1,
            "taskId": record.task_id.0,
            "repository": record.repo_root,
            "worktree": record.path,
            "branch": record.branch,
            "baseCommit": record.base_commit,
            "createdAt": crate::domain::types::utc_now(),
            "expiresAt": expires_at.clone(),
            "files": {
                "branch.bundle": bundle_hash,
                "tracked.patch": patch_hash,
                "untracked.zip": archive_hash,
            }
        });
        authorize_managed_fs_write(&directory, &[&manifest])?;
        std::fs::write(
            &manifest,
            serde_json::to_vec_pretty(&manifest_value).map_err(|_| {
                workspace_error(
                    "WORKTREE_RECOVERY_FAILED",
                    "Recovery manifest could not be encoded",
                )
            })?,
        )
        .map_err(|_| {
            workspace_error(
                "WORKTREE_RECOVERY_FAILED",
                "Recovery manifest could not be written",
            )
        })?;
        let item = RecoveryItem {
            id: RecoveryId::new(id.clone()),
            task_id: record.task_id.clone(),
            directory: directory.to_string_lossy().into_owned(),
            manifest_path: manifest.to_string_lossy().into_owned(),
            expires_at,
            state: RecoveryState::Available,
        };
        self.repo
            .create_recovery_item(&item)
            .map_err(map_repo_error)?;
        Ok(RecoveryEvidence {
            id,
            manifest_path: manifest,
            branch_bundle: bundle,
            tracked_patch: patch,
            untracked_zip: archive,
        })
    }
}

fn rollback_created_worktree(git: &GitCli, repo: &Path, target: &str, branch: &str) {
    let Ok(repository) = git.inspect_repository(repo) else {
        return;
    };
    let target_path = Path::new(target);
    if authorize_managed_git(
        repo,
        &["worktree", "remove", "--force", target],
        &[&repository.common_git_dir, target_path],
    )
    .is_ok()
    {
        let _ = git.run_checked(repo, &["worktree", "remove", "--force", target]);
    }
    if authorize_managed_git(
        repo,
        &["branch", "-D", branch],
        &[&repository.common_git_dir],
    )
    .is_ok()
    {
        let _ = git.run_checked(repo, &["branch", "-D", branch]);
    }
}

fn authorize_managed_git(
    cwd: &Path,
    args: &[&str],
    write_paths: &[&Path],
) -> Result<(), WorkspaceError> {
    let operation = OperationDescriptor {
        kind: OperationKind::Git,
        executable: Some("git".into()),
        args: args.iter().map(|value| (*value).to_owned()).collect(),
        cwd: cwd.to_string_lossy().into_owned(),
        read_paths: Vec::new(),
        write_paths: write_paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
    };
    ExecutionGuard::authorize_managed(&operation, cwd, write_paths).map_err(|_| {
        workspace_error(
            "WORKTREE_OPERATION_DENIED",
            "Managed Git operation failed ExecutionGuard validation",
        )
    })
}

fn authorize_managed_fs_write(cwd: &Path, write_paths: &[&Path]) -> Result<(), WorkspaceError> {
    let operation = OperationDescriptor {
        kind: OperationKind::FileWrite,
        executable: None,
        args: Vec::new(),
        cwd: cwd.to_string_lossy().into_owned(),
        read_paths: Vec::new(),
        write_paths: write_paths
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect(),
    };
    ExecutionGuard::authorize_managed(&operation, cwd, write_paths).map_err(|_| {
        workspace_error(
            "WORKTREE_OPERATION_DENIED",
            "Managed filesystem operation failed ExecutionGuard validation",
        )
    })
}

fn validate_identifier(value: &str, _name: &str) -> Result<String, WorkspaceError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 96
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(workspace_error(
            "WORKTREE_INVALID_ID",
            "Worktree identifier contains unsupported characters",
        ));
    }
    Ok(value.to_owned())
}

fn validate_git_ref(value: &str) -> Result<(), WorkspaceError> {
    if value.is_empty()
        || value.len() > 240
        || value.starts_with('-')
        || value.starts_with('.')
        || value.ends_with('.')
        || value.ends_with('/')
        || value.contains("..")
        || value.contains("@{")
        || value.contains("//")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/._-".contains(&byte))
    {
        return Err(workspace_error(
            "WORKTREE_INVALID_BASE",
            "Base ref contains unsupported characters",
        ));
    }
    Ok(())
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut separator = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            separator = false;
        } else if !separator && !slug.is_empty() {
            slug.push('-');
            separator = true;
        }
        if slug.len() >= 80 {
            break;
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "task".to_owned()
    } else {
        slug.to_owned()
    }
}

fn hex_prefix(value: &str, length: usize) -> String {
    let normalized = if cfg!(windows) {
        value.to_lowercase()
    } else {
        value.to_owned()
    };
    let digest = Sha256::digest(normalized.as_bytes());
    format!("{digest:x}").chars().take(length).collect()
}

fn same_path_identity(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        fn windows_key(path: &Path) -> String {
            let value = path.to_string_lossy().replace('/', "\\");
            value
                .strip_prefix(r"\\?\UNC\")
                .map(|rest| format!(r"\\{rest}"))
                .or_else(|| value.strip_prefix(r"\\?\").map(str::to_owned))
                .unwrap_or(value)
                .to_lowercase()
        }
        windows_key(left) == windows_key(right)
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn split_nul(bytes: &[u8]) -> impl Iterator<Item = &str> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|item| !item.is_empty())
        .filter_map(|item| std::str::from_utf8(item).ok())
}

fn workspace_content_digest(
    git: &GitCli,
    root: &Path,
    untracked: &[u8],
) -> Result<String, WorkspaceError> {
    let canonical_root = std::fs::canonicalize(root).map_err(|_| {
        workspace_error(
            "WORKTREE_INSPECTION_FAILED",
            "Worktree content could not be verified",
        )
    })?;
    let mut hasher = Sha256::new();
    hasher.update(
        git.capture(root, &["diff", "--binary", "HEAD"])
            .map_err(map_git_error)?,
    );
    hasher.update(
        git.capture(root, &["diff", "--binary", "--cached", "HEAD"])
            .map_err(map_git_error)?,
    );
    hasher.update(untracked);
    for value in split_nul(untracked) {
        let relative = PathBuf::from(value);
        if relative.is_absolute()
            || relative.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
        {
            return Err(workspace_error(
                "WORKTREE_RECOVERY_INVALID",
                "Untracked path failed content verification",
            ));
        }
        let source = root.join(relative);
        let metadata = std::fs::symlink_metadata(&source).map_err(|_| {
            workspace_error(
                "WORKTREE_RECOVERY_INVALID",
                "Untracked file changed during verification",
            )
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(workspace_error(
                "WORKTREE_RECOVERY_INVALID",
                "Untracked entry is not a regular file",
            ));
        }
        let canonical_source = std::fs::canonicalize(&source).map_err(|_| {
            workspace_error(
                "WORKTREE_RECOVERY_INVALID",
                "Untracked file could not be canonicalized",
            )
        })?;
        if !canonical_source.starts_with(&canonical_root) {
            return Err(workspace_error(
                "WORKTREE_RECOVERY_INVALID",
                "Untracked file escaped the worktree",
            ));
        }
        hasher.update(value.as_bytes());
        hasher.update(sha256_file(&canonical_source)?.as_bytes());
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn sha256_file(path: &Path) -> Result<String, WorkspaceError> {
    let mut file = std::fs::File::open(path).map_err(|_| {
        workspace_error(
            "WORKTREE_RECOVERY_INVALID",
            "Recovery evidence is not readable",
        )
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer).map_err(|_| {
            workspace_error(
                "WORKTREE_RECOVERY_INVALID",
                "Recovery evidence could not be verified",
            )
        })?;
        if count == 0 {
            return Ok(format!("{:x}", hasher.finalize()));
        }
        hasher.update(&buffer[..count]);
    }
}

fn verify_recovery_package(evidence: &RecoveryEvidence) -> Result<(), WorkspaceError> {
    for path in [
        &evidence.manifest_path,
        &evidence.branch_bundle,
        &evidence.tracked_patch,
        &evidence.untracked_zip,
    ] {
        let metadata = std::fs::metadata(path).map_err(|_| {
            workspace_error(
                "WORKTREE_RECOVERY_INVALID",
                "Recovery package is incomplete",
            )
        })?;
        if !metadata.is_file() {
            return Err(workspace_error(
                "WORKTREE_RECOVERY_INVALID",
                "Recovery package contains an invalid entry",
            ));
        }
    }
    if std::fs::metadata(&evidence.branch_bundle)
        .map(|value| value.len())
        .unwrap_or(0)
        == 0
        || std::fs::metadata(&evidence.untracked_zip)
            .map(|value| value.len())
            .unwrap_or(0)
            == 0
    {
        return Err(workspace_error(
            "WORKTREE_RECOVERY_INVALID",
            "Recovery package contains empty required evidence",
        ));
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&evidence.manifest_path).map_err(|_| {
            workspace_error(
                "WORKTREE_RECOVERY_INVALID",
                "Recovery manifest is not readable",
            )
        })?)
        .map_err(|_| {
            workspace_error("WORKTREE_RECOVERY_INVALID", "Recovery manifest is invalid")
        })?;
    let files = manifest
        .get("files")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| {
            workspace_error(
                "WORKTREE_RECOVERY_INVALID",
                "Recovery manifest has no file hashes",
            )
        })?;
    for (name, path) in [
        ("branch.bundle", &evidence.branch_bundle),
        ("tracked.patch", &evidence.tracked_patch),
        ("untracked.zip", &evidence.untracked_zip),
    ] {
        let expected = files
            .get(name)
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                workspace_error(
                    "WORKTREE_RECOVERY_INVALID",
                    "Recovery manifest hash is missing",
                )
            })?;
        if sha256_file(path)? != expected {
            return Err(workspace_error(
                "WORKTREE_RECOVERY_INVALID",
                "Recovery package hash verification failed",
            ));
        }
    }
    Ok(())
}

fn write_stored_zip<'a>(
    root: &Path,
    paths: impl Iterator<Item = &'a str>,
    destination: &Path,
) -> Result<(), WorkspaceError> {
    struct Entry {
        name: Vec<u8>,
        crc: u32,
        size: u32,
        offset: u32,
    }
    let canonical_root = std::fs::canonicalize(root).map_err(|_| {
        workspace_error(
            "WORKTREE_RECOVERY_FAILED",
            "Worktree could not be verified for recovery",
        )
    })?;
    let mut writer = std::fs::File::create(destination).map_err(|_| {
        workspace_error(
            "WORKTREE_RECOVERY_FAILED",
            "Untracked recovery archive could not be created",
        )
    })?;
    let mut entries = Vec::new();
    for value in paths {
        let relative = PathBuf::from(value);
        if relative.is_absolute()
            || relative.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
        {
            return Err(workspace_error(
                "WORKTREE_RECOVERY_FAILED",
                "Untracked path failed recovery validation",
            ));
        }
        let source = root.join(&relative);
        let metadata = std::fs::symlink_metadata(&source).map_err(|_| {
            workspace_error(
                "WORKTREE_RECOVERY_FAILED",
                "Untracked recovery source is unavailable",
            )
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(workspace_error(
                "WORKTREE_RECOVERY_FAILED",
                "Untracked recovery source is not a regular file",
            ));
        }
        let canonical_source = std::fs::canonicalize(&source).map_err(|_| {
            workspace_error(
                "WORKTREE_RECOVERY_FAILED",
                "Untracked recovery source could not be verified",
            )
        })?;
        if !canonical_source.starts_with(&canonical_root) {
            return Err(workspace_error(
                "WORKTREE_RECOVERY_FAILED",
                "Untracked recovery source escaped the worktree",
            ));
        }
        let data = std::fs::read(&canonical_source).map_err(|_| {
            workspace_error(
                "WORKTREE_RECOVERY_FAILED",
                "Untracked recovery source could not be read",
            )
        })?;
        let size = u32::try_from(data.len()).map_err(|_| {
            workspace_error(
                "WORKTREE_RECOVERY_FAILED",
                "Untracked recovery source is too large",
            )
        })?;
        let name = relative.to_string_lossy().replace('\\', "/").into_bytes();
        let name_len = u16::try_from(name.len()).map_err(|_| {
            workspace_error(
                "WORKTREE_RECOVERY_FAILED",
                "Untracked recovery name is too long",
            )
        })?;
        let offset = u32::try_from(writer.stream_position().unwrap_or(u64::MAX)).map_err(|_| {
            workspace_error(
                "WORKTREE_RECOVERY_FAILED",
                "Untracked recovery archive is too large",
            )
        })?;
        let crc = crc32fast::hash(&data);
        write_u32(&mut writer, 0x0403_4b50)?;
        write_u16(&mut writer, 20)?;
        write_u16(&mut writer, 0x0800)?;
        write_u16(&mut writer, 0)?;
        write_u16(&mut writer, 0)?;
        write_u16(&mut writer, 0)?;
        write_u32(&mut writer, crc)?;
        write_u32(&mut writer, size)?;
        write_u32(&mut writer, size)?;
        write_u16(&mut writer, name_len)?;
        write_u16(&mut writer, 0)?;
        writer.write_all(&name).map_err(zip_write_error)?;
        writer.write_all(&data).map_err(zip_write_error)?;
        entries.push(Entry {
            name,
            crc,
            size,
            offset,
        });
    }
    let central_offset =
        u32::try_from(writer.stream_position().unwrap_or(u64::MAX)).map_err(|_| {
            workspace_error(
                "WORKTREE_RECOVERY_FAILED",
                "Untracked recovery archive is too large",
            )
        })?;
    for entry in &entries {
        write_u32(&mut writer, 0x0201_4b50)?;
        write_u16(&mut writer, 20)?;
        write_u16(&mut writer, 20)?;
        write_u16(&mut writer, 0x0800)?;
        write_u16(&mut writer, 0)?;
        write_u16(&mut writer, 0)?;
        write_u16(&mut writer, 0)?;
        write_u32(&mut writer, entry.crc)?;
        write_u32(&mut writer, entry.size)?;
        write_u32(&mut writer, entry.size)?;
        write_u16(&mut writer, entry.name.len() as u16)?;
        write_u16(&mut writer, 0)?;
        write_u16(&mut writer, 0)?;
        write_u16(&mut writer, 0)?;
        write_u16(&mut writer, 0)?;
        write_u32(&mut writer, 0)?;
        write_u32(&mut writer, entry.offset)?;
        writer.write_all(&entry.name).map_err(zip_write_error)?;
    }
    let central_end =
        u32::try_from(writer.stream_position().unwrap_or(u64::MAX)).map_err(|_| {
            workspace_error(
                "WORKTREE_RECOVERY_FAILED",
                "Untracked recovery archive is too large",
            )
        })?;
    let count = u16::try_from(entries.len()).map_err(|_| {
        workspace_error(
            "WORKTREE_RECOVERY_FAILED",
            "Too many untracked recovery entries",
        )
    })?;
    write_u32(&mut writer, 0x0605_4b50)?;
    write_u16(&mut writer, 0)?;
    write_u16(&mut writer, 0)?;
    write_u16(&mut writer, count)?;
    write_u16(&mut writer, count)?;
    write_u32(&mut writer, central_end - central_offset)?;
    write_u32(&mut writer, central_offset)?;
    write_u16(&mut writer, 0)?;
    writer.sync_all().map_err(zip_write_error)?;
    Ok(())
}

fn write_u16(writer: &mut impl Write, value: u16) -> Result<(), WorkspaceError> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(zip_write_error)
}

fn write_u32(writer: &mut impl Write, value: u32) -> Result<(), WorkspaceError> {
    writer
        .write_all(&value.to_le_bytes())
        .map_err(zip_write_error)
}

fn zip_write_error(_: std::io::Error) -> WorkspaceError {
    workspace_error(
        "WORKTREE_RECOVERY_FAILED",
        "Untracked recovery archive could not be written",
    )
}

fn active_managed_record(
    repo: &dyn Repository,
    task_id: &str,
) -> Result<WorktreeRecord, WorkspaceError> {
    let records = repo
        .list_worktrees_by_task(task_id)
        .map_err(map_repo_error)?;
    let mut active = records.into_iter().filter(|record| {
        record.ownership == WorktreeOwnership::Managed
            && !matches!(
                record.state,
                WorktreeState::Deleted | WorktreeState::Removed
            )
    });
    let record = active
        .next()
        .ok_or_else(|| workspace_error("WORKTREE_MISSING", "Managed worktree is not registered"))?;
    if active.next().is_some() {
        return Err(workspace_error(
            "WORKTREE_REGISTRY_MISMATCH",
            "Task has multiple active managed worktrees",
        ));
    }
    Ok(record)
}

fn ensure_task_not_running(repo: &dyn Repository, task_id: &str) -> Result<(), WorkspaceError> {
    let task = repo.get_task(task_id).map_err(map_repo_error)?;
    if matches!(
        task.status,
        crate::domain::types::TaskStatus::Running
            | crate::domain::types::TaskStatus::WaitingPermission
            | crate::domain::types::TaskStatus::Integrating
    ) {
        return Err(workspace_error(
            "WORKTREE_TASK_RUNNING",
            "Running task prevents worktree cleanup",
        ));
    }
    Ok(())
}

fn active_registered_record(
    repo: &dyn Repository,
    task_id: &str,
) -> Result<WorktreeRecord, WorkspaceError> {
    let mut records = repo
        .list_worktrees_by_task(task_id)
        .map_err(map_repo_error)?
        .into_iter()
        .filter(|record| {
            !matches!(
                record.state,
                WorktreeState::Deleted | WorktreeState::Removed
            )
        });
    let record = records
        .next()
        .ok_or_else(|| workspace_error("WORKTREE_MISSING", "Worktree is not registered"))?;
    if records.next().is_some() {
        return Err(workspace_error(
            "WORKTREE_REGISTRY_MISMATCH",
            "Task has multiple active worktrees",
        ));
    }
    Ok(record)
}

fn directory_size(root: &Path) -> Result<u64, WorkspaceError> {
    let mut total = 0u64;
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).map_err(|_| {
            workspace_error(
                "WORKTREE_INSPECTION_FAILED",
                "Worktree disk usage could not be inspected",
            )
        })? {
            let entry = entry.map_err(|_| {
                workspace_error(
                    "WORKTREE_INSPECTION_FAILED",
                    "Worktree disk usage could not be inspected",
                )
            })?;
            let metadata = std::fs::symlink_metadata(entry.path()).map_err(|_| {
                workspace_error(
                    "WORKTREE_INSPECTION_FAILED",
                    "Worktree disk usage could not be inspected",
                )
            })?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                pending.push(entry.path());
            } else if metadata.is_file() {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    Ok(total)
}

fn map_git_error(error: crate::adapters::git_cli::GitError) -> WorkspaceError {
    WorkspaceError {
        code: error.code,
        message: error.message,
    }
}

fn map_repo_error(error: crate::domain::error::DomainError) -> WorkspaceError {
    match error.code.as_str() {
        crate::domain::error::codes::WORKTREE_TASK_RUNNING => workspace_error(
            "WORKTREE_TASK_RUNNING",
            "Running task prevents worktree cleanup",
        ),
        crate::domain::error::codes::WORKTREE_NOT_READY => workspace_error(
            "WORKTREE_NOT_READY",
            "Managed worktree is not ready for this operation",
        ),
        _ => workspace_error(
            "WORKTREE_REGISTRY_FAILED",
            "Worktree registry operation failed",
        ),
    }
}

fn workspace_error(code: &'static str, message: &'static str) -> WorkspaceError {
    WorkspaceError { code, message }
}
