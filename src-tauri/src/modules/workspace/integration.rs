//! GAG-013 isolated squash integration and compare-and-swap publication.

use super::*;
use crate::domain::types::{utc_now, IntegrationAttempt, IntegrationId};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationPlan {
    pub attempt_id: String,
    pub task_id: TaskId,
    pub source_ref: String,
    pub source_tip_sha: String,
    pub source_range: Vec<String>,
    pub source_dirty: bool,
    pub source_worktree_digest: String,
    pub target_ref: String,
    pub expected_target_sha: String,
    pub commit_message: String,
    pub validation_commands: Vec<Vec<String>>,
    pub validation_digest: String,
    pub approval_digest: String,
}

#[derive(Debug, Clone)]
pub struct PrepareSquash {
    pub task_id: TaskId,
    pub commit_message: String,
}

impl ManagedWorkspaceService {
    pub(super) fn prepare_integration(
        &self,
        request: PrepareSquash,
    ) -> Result<IntegrationPlan, WorkspaceError> {
        let _lock = self.lifecycle_lock.lock().map_err(|_| {
            integration_error(
                "INTEGRATION_LOCKED",
                "Repository integration lock is unavailable",
            )
        })?;
        validate_integration_message(&request.commit_message)?;
        let record = active_managed_record(self.repo.as_ref(), &request.task_id.0)?;
        let source_root = self.prove_registered_target(&record)?;
        let source = self
            .git
            .inspect_repository(&source_root)
            .map_err(map_git_error)?;
        ensure_no_in_progress_git_operation(&self.git, &source_root)?;
        let source_untracked = self
            .git
            .capture(
                &source_root,
                &["ls-files", "--others", "--exclude-standard", "-z"],
            )
            .map_err(map_git_error)?;
        let source_worktree_digest =
            workspace_content_digest(&self.git, &source_root, &source_untracked)?;
        if source.branch.as_deref() != Some(record.branch.as_str()) {
            return Err(integration_error(
                "INTEGRATION_SOURCE_CHANGED",
                "Source Worktree branch no longer matches its registration",
            ));
        }
        let target_ref = format!("refs/heads/{}", record.base_branch);
        validate_git_ref(&record.base_branch)?;
        let expected_target_sha = required_utf8_line(
            self.git
                .capture(&source_root, &["rev-parse", "--verify", &target_ref])
                .map_err(map_git_error)?,
        )?;
        let source_tip_sha = source.head;
        if !self
            .git
            .is_ancestor(&source_root, &record.base_commit, &source_tip_sha)
            .map_err(map_git_error)?
        {
            return Err(integration_error(
                "INTEGRATION_SOURCE_CHANGED",
                "Source no longer contains its recorded base",
            ));
        }
        for worktree in self
            .git
            .list_worktrees(&source_root)
            .map_err(map_git_error)?
        {
            if worktree.branch.as_deref() == Some(record.base_branch.as_str()) {
                return Err(integration_error("INTEGRATION_TARGET_CHECKED_OUT", "Target branch is checked out in a linked Worktree; detach or switch it before integration"));
            }
        }
        let merge_base = required_utf8_line(
            self.git
                .capture(
                    &source_root,
                    &["merge-base", &expected_target_sha, &source_tip_sha],
                )
                .map_err(map_git_error)?,
        )?;
        let range_spec = format!("{merge_base}..{source_tip_sha}");
        let source_range = utf8_lines(
            self.git
                .capture(&source_root, &["rev-list", "--reverse", &range_spec])
                .map_err(map_git_error)?,
        )?;
        if source_range.is_empty() {
            return Err(integration_error(
                "INTEGRATION_EMPTY",
                "Source contains no Checkpoint commits to integrate",
            ));
        }
        let checkpoints: HashSet<String> = self
            .repo
            .list_checkpoints_by_task(&request.task_id.0)
            .map_err(map_repo_error)?
            .into_iter()
            .map(|item| item.commit_sha)
            .collect();
        if source_range.iter().any(|sha| !checkpoints.contains(sha)) {
            return Err(integration_error(
                "INTEGRATION_UNTRACKED_COMMIT",
                "Source range contains a commit that is not a recorded Checkpoint",
            ));
        }
        let validation_commands: Vec<Vec<String>> = Vec::new();
        let validation_json = serde_json::to_string(&validation_commands).map_err(|_| {
            integration_error(
                "INTEGRATION_PLAN_INVALID",
                "Validation plan could not be encoded",
            )
        })?;
        let validation_digest = sha256_text(&validation_json);
        let attempt_id = Uuid::new_v4().to_string();
        let source_range_json = serde_json::to_string(&source_range).map_err(|_| {
            integration_error(
                "INTEGRATION_PLAN_INVALID",
                "Source range could not be encoded",
            )
        })?;
        let approval_digest = sha256_text(&format!(
            "{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
            record.repo_identity,
            source_tip_sha,
            source_range_json,
            source.dirty,
            source_worktree_digest,
            target_ref,
            expected_target_sha,
            request.commit_message,
            validation_digest
        ));
        let now = utc_now();
        let attempt = IntegrationAttempt {
            id: IntegrationId::new(attempt_id.clone()),
            task_id: request.task_id.clone(),
            repo_root: record.repo_root,
            source_ref: format!("refs/heads/{}", record.branch),
            source_tip_sha: source_tip_sha.clone(),
            source_range: source_range_json,
            source_dirty: source.dirty,
            source_worktree_digest: source_worktree_digest.clone(),
            target_ref: target_ref.clone(),
            expected_target_sha: expected_target_sha.clone(),
            commit_message: request.commit_message.clone(),
            validation_commands_json: validation_json,
            validation_digest: validation_digest.clone(),
            approval_digest: approval_digest.clone(),
            state: "preflight".into(),
            temporary_worktree_id: None,
            temporary_worktree_path: None,
            temporary_branch: None,
            conflict_summary_json: None,
            validation_result_json: None,
            result_commit_sha: None,
            recovery_bundle_path: None,
            cleanup_status: "not_started".into(),
            created_at: now.clone(),
            updated_at: now,
        };
        self.repo
            .create_integration_attempt(&attempt)
            .map_err(map_repo_error)?;
        Ok(IntegrationPlan {
            attempt_id,
            task_id: request.task_id,
            source_ref: attempt.source_ref,
            source_tip_sha,
            source_range,
            source_dirty: source.dirty,
            source_worktree_digest,
            target_ref,
            expected_target_sha,
            commit_message: request.commit_message,
            validation_commands,
            validation_digest,
            approval_digest,
        })
    }

    pub(super) fn start_integration(
        &self,
        attempt_id: &str,
        approval_digest: &str,
    ) -> Result<IntegrationAttempt, WorkspaceError> {
        let _lock = self.lifecycle_lock.lock().map_err(|_| {
            integration_error(
                "INTEGRATION_LOCKED",
                "Repository integration lock is unavailable",
            )
        })?;
        let mut attempt = self.load_attempt(attempt_id)?;
        require_state(&attempt, &["preflight"])?;
        if !constant_time_equal(&attempt.approval_digest, approval_digest) {
            return Err(integration_error(
                "INTEGRATION_APPROVAL_INVALID",
                "Integration approval does not match the frozen plan",
            ));
        }
        attempt.updated_at = utc_now();
        self.save_attempt(
            &attempt,
            "{\"approval\":\"accepted\",\"source\":\"review_ui\"}",
        )?;
        self.revalidate_frozen_refs(&attempt, true)?;
        self.repo
            .update_task_status(&attempt.task_id.0, "integrating", None)
            .map_err(map_repo_error)?;
        let managed_root = self.ensure_managed_root()?;
        let repo_hash = hex_prefix(&attempt.repo_root, 16);
        let temp_id = format!("integration-{}", &attempt.id.0[..8]);
        let parent = managed_root.join(repo_hash);
        authorize_managed_fs_write(&managed_root, &[&parent])?;
        std::fs::create_dir_all(&parent).map_err(|_| {
            integration_error(
                "INTEGRATION_STAGING_FAILED",
                "Integration parent directory could not be created",
            )
        })?;
        let temp_path = validate_managed_worktree_target(&managed_root, &parent.join(&temp_id))
            .map_err(|_| {
                integration_error(
                    "INTEGRATION_PATH_REJECTED",
                    "Temporary Worktree failed managed-root path proof",
                )
            })?;
        if temp_path.exists() {
            return Err(integration_error(
                "INTEGRATION_PATH_COLLISION",
                "Temporary Worktree path already exists",
            ));
        }
        let temp_branch = format!("gag-integration/{}", attempt.id.0);
        let temp_arg = temp_path.to_string_lossy().into_owned();
        let args = [
            "worktree",
            "add",
            "-b",
            temp_branch.as_str(),
            temp_arg.as_str(),
            attempt.expected_target_sha.as_str(),
        ];
        authorize_managed_git(Path::new(&attempt.repo_root), &args, &[&temp_path])?;
        if let Err(error) = self.git.run_checked(Path::new(&attempt.repo_root), &args) {
            let _ = self.repo.update_task_status(
                &attempt.task_id.0,
                "ready_for_review",
                Some("integration staging failed"),
            );
            return Err(map_git_error(error));
        }
        let verified = self
            .git
            .list_worktrees(Path::new(&attempt.repo_root))
            .map(|items| {
                items.into_iter().any(|item| {
                    same_path_identity(&item.path, &temp_path)
                        && item.branch.as_deref() == Some(temp_branch.as_str())
                        && item.head.as_deref() == Some(attempt.expected_target_sha.as_str())
                })
            })
            .unwrap_or(false);
        if !verified {
            rollback_created_worktree(
                &self.git,
                Path::new(&attempt.repo_root),
                &temp_arg,
                &temp_branch,
            );
            let _ = self.repo.update_task_status(
                &attempt.task_id.0,
                "ready_for_review",
                Some("integration Worktree verification failed"),
            );
            return Err(integration_error(
                "INTEGRATION_STAGING_FAILED",
                "Created integration Worktree failed Git metadata verification",
            ));
        }
        attempt.state = "staging".into();
        attempt.temporary_worktree_id = Some(temp_id);
        attempt.temporary_worktree_path = Some(temp_arg.clone());
        attempt.temporary_branch = Some(temp_branch);
        attempt.updated_at = utc_now();
        if let Err(error) = self.save_attempt(&attempt, "{\"stage\":\"worktree_created\"}") {
            rollback_created_worktree(
                &self.git,
                Path::new(&attempt.repo_root),
                &temp_arg,
                attempt.temporary_branch.as_deref().unwrap(),
            );
            let _ = self.repo.update_task_status(
                &attempt.task_id.0,
                "ready_for_review",
                Some("integration audit persistence failed"),
            );
            return Err(error);
        }
        let temp = Path::new(attempt.temporary_worktree_path.as_deref().unwrap());
        let merge_args = [
            "merge",
            "--squash",
            "--no-commit",
            attempt.source_tip_sha.as_str(),
        ];
        authorize_managed_git(temp, &merge_args, &[temp])?;
        let merge = match self.git.capture_allow_status(temp, &merge_args, &[0, 1]) {
            Ok(output) => output,
            Err(error) => {
                attempt.state = "cleanup_required".into();
                attempt.updated_at = utc_now();
                self.save_attempt(&attempt, "{\"reason\":\"squash_command_failed\"}")?;
                return Err(map_git_error(error));
            }
        };
        if merge.status == 1 {
            let conflicts = split_nul_owned(
                self.git
                    .capture(temp, &["diff", "--name-only", "--diff-filter=U", "-z"])
                    .map_err(map_git_error)?,
            )?;
            if conflicts.is_empty() {
                return Err(integration_error(
                    "INTEGRATION_SQUASH_FAILED",
                    "Squash failed without a structured conflict list",
                ));
            }
            attempt.state = "conflicted".into();
            attempt.conflict_summary_json =
                Some(serde_json::to_string(&conflicts).unwrap_or_else(|_| "[]".into()));
            attempt.updated_at = utc_now();
            self.save_attempt(&attempt, "{\"stage\":\"conflicted\"}")?;
            let _ = self.repo.update_task_status(
                &attempt.task_id.0,
                "conflicted",
                Some("isolated squash conflict"),
            );
            return Ok(attempt);
        }
        let staged = self
            .git
            .capture_allow_status(temp, &["diff", "--cached", "--quiet"], &[0, 1])
            .map_err(map_git_error)?;
        if staged.status == 0 {
            attempt.state = "validation_failed".into();
            attempt.updated_at = utc_now();
            self.save_attempt(&attempt, "{\"reason\":\"empty_squash\"}")?;
            return Err(integration_error(
                "INTEGRATION_EMPTY",
                "Squash produced no staged changes",
            ));
        }
        attempt.state = "validating".into();
        attempt.validation_result_json = Some("[]".into());
        attempt.updated_at = utc_now();
        self.save_attempt(&attempt, "{\"commands\":[]}")?;
        let commit_args = [
            "commit",
            "--no-gpg-sign",
            "-m",
            attempt.commit_message.as_str(),
        ];
        authorize_managed_git(temp, &commit_args, &[temp])?;
        if let Err(error) = self.git.run_checked(temp, &commit_args) {
            attempt.state = "validation_failed".into();
            attempt.validation_result_json = Some("[{\"status\":\"commit_failed\"}]".into());
            attempt.updated_at = utc_now();
            self.save_attempt(&attempt, "{\"reason\":\"commit_failed\"}")?;
            return Err(map_git_error(error));
        }
        let result = required_utf8_line(
            self.git
                .capture(temp, &["rev-parse", "HEAD"])
                .map_err(map_git_error)?,
        )?;
        let parent = required_utf8_line(
            self.git
                .capture(temp, &["rev-parse", "HEAD^"])
                .map_err(map_git_error)?,
        )?;
        if parent != attempt.expected_target_sha {
            attempt.state = "cleanup_required".into();
            attempt.updated_at = utc_now();
            self.save_attempt(&attempt, "{\"reason\":\"parent_mismatch\"}")?;
            return Err(integration_error(
                "INTEGRATION_PARENT_MISMATCH",
                "Integration commit parent is not the frozen target HEAD",
            ));
        }
        attempt.result_commit_sha = Some(result);
        attempt.state = "ready_to_publish".into();
        attempt.updated_at = utc_now();
        self.save_attempt(&attempt, "{\"stage\":\"ready_to_publish\"}")?;
        Ok(attempt)
    }

    pub(super) fn integration_status(
        &self,
        id: &str,
    ) -> Result<IntegrationAttempt, WorkspaceError> {
        self.load_attempt(id)
    }

    pub(super) fn publish_integration_attempt(
        &self,
        id: &str,
        approval_digest: &str,
    ) -> Result<IntegrationAttempt, WorkspaceError> {
        let _lock = self.lifecycle_lock.lock().map_err(|_| {
            integration_error(
                "INTEGRATION_LOCKED",
                "Repository integration lock is unavailable",
            )
        })?;
        let mut attempt = self.load_attempt(id)?;
        require_state(&attempt, &["ready_to_publish"])?;
        if !constant_time_equal(&attempt.approval_digest, approval_digest) {
            return Err(integration_error(
                "INTEGRATION_APPROVAL_INVALID",
                "Integration approval no longer matches the plan",
            ));
        }
        if let Err(error) = self.revalidate_frozen_refs(&attempt, true) {
            attempt.state = "publish_rejected".into();
            attempt.updated_at = utc_now();
            self.save_attempt(&attempt, "{\"reason\":\"frozen_ref_changed\"}")?;
            return Err(error);
        }
        attempt.state = "publishing".into();
        attempt.updated_at = utc_now();
        self.save_attempt(&attempt, "{\"stage\":\"publishing\"}")?;
        let result = attempt.result_commit_sha.clone().ok_or_else(|| {
            integration_error(
                "INTEGRATION_RESULT_MISSING",
                "Integration result commit is missing",
            )
        })?;
        let args = [
            "update-ref",
            attempt.target_ref.as_str(),
            result.as_str(),
            attempt.expected_target_sha.as_str(),
        ];
        authorize_managed_git(
            Path::new(&attempt.repo_root),
            &args,
            &[Path::new(&attempt.repo_root)],
        )?;
        if self
            .git
            .run_checked(Path::new(&attempt.repo_root), &args)
            .is_err()
        {
            attempt.state = "publish_rejected".into();
            attempt.updated_at = utc_now();
            self.save_attempt(&attempt, "{\"reason\":\"cas_failed\"}")?;
            return Err(integration_error(
                "INTEGRATION_TARGET_CHANGED",
                "Target ref changed before atomic publication",
            ));
        }
        attempt.state = "completed".into();
        attempt.updated_at = utc_now();
        self.save_attempt(&attempt, "{\"stage\":\"published\"}")?;
        let _ = self
            .repo
            .update_task_status(&attempt.task_id.0, "merged", None);
        Ok(attempt)
    }

    pub(super) fn abort_integration_attempt(
        &self,
        id: &str,
    ) -> Result<IntegrationAttempt, WorkspaceError> {
        let _lock = self.lifecycle_lock.lock().map_err(|_| {
            integration_error(
                "INTEGRATION_LOCKED",
                "Repository integration lock is unavailable",
            )
        })?;
        let mut attempt = self.load_attempt(id)?;
        if matches!(attempt.state.as_str(), "completed" | "aborted") {
            return Err(integration_error(
                "INTEGRATION_INVALID_STATE",
                "Completed or aborted integration cannot be aborted",
            ));
        }
        if attempt.temporary_worktree_path.is_some() {
            attempt.recovery_bundle_path = Some(
                self.create_integration_recovery(&attempt)?
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        attempt.state = "aborted".into();
        attempt.updated_at = utc_now();
        self.save_attempt(&attempt, "{\"stage\":\"aborted\"}")?;
        let _ = self.repo.update_task_status(
            &attempt.task_id.0,
            "ready_for_review",
            Some("integration aborted"),
        );
        Ok(attempt)
    }

    pub(super) fn cleanup_integration_attempt(
        &self,
        id: &str,
    ) -> Result<IntegrationAttempt, WorkspaceError> {
        let _lock = self.lifecycle_lock.lock().map_err(|_| {
            integration_error(
                "INTEGRATION_LOCKED",
                "Repository integration lock is unavailable",
            )
        })?;
        let mut attempt = self.load_attempt(id)?;
        require_state(
            &attempt,
            &[
                "completed",
                "aborted",
                "conflicted",
                "validation_failed",
                "publish_rejected",
                "cleanup_required",
            ],
        )?;
        let Some(raw_path) = attempt.temporary_worktree_path.clone() else {
            attempt.cleanup_status = "completed".into();
            attempt.updated_at = utc_now();
            self.save_attempt(&attempt, "{\"cleanup\":\"nothing_to_remove\"}")?;
            return Ok(attempt);
        };
        let managed_root = self.ensure_managed_root()?;
        let target = validate_managed_worktree_target(&managed_root, Path::new(&raw_path))
            .map_err(|_| {
                integration_error(
                    "INTEGRATION_CLEANUP_REJECTED",
                    "Temporary Worktree failed managed-root proof",
                )
            })?;
        let listed = self
            .git
            .list_worktrees(Path::new(&attempt.repo_root))
            .map_err(map_git_error)?;
        if !listed.iter().any(|item| {
            same_path_identity(&item.path, &target)
                && item.branch.as_deref() == attempt.temporary_branch.as_deref()
        }) {
            return Err(integration_error(
                "INTEGRATION_CLEANUP_REJECTED",
                "Temporary Worktree ownership proof no longer matches Git metadata",
            ));
        }
        if attempt.state != "completed" && attempt.recovery_bundle_path.is_none() {
            attempt.recovery_bundle_path = Some(
                self.create_integration_recovery(&attempt)?
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        let target_arg = target.to_string_lossy().into_owned();
        let args = ["worktree", "remove", "--force", target_arg.as_str()];
        authorize_managed_git(Path::new(&attempt.repo_root), &args, &[&target])?;
        self.git
            .run_checked(Path::new(&attempt.repo_root), &args)
            .map_err(map_git_error)?;
        if let Some(branch) = attempt.temporary_branch.as_deref() {
            let branch_args = ["branch", "-D", branch];
            authorize_managed_git(
                Path::new(&attempt.repo_root),
                &branch_args,
                &[Path::new(&attempt.repo_root)],
            )?;
            self.git
                .run_checked(Path::new(&attempt.repo_root), &branch_args)
                .map_err(map_git_error)?;
        }
        attempt.cleanup_status = "completed".into();
        attempt.updated_at = utc_now();
        self.save_attempt(&attempt, "{\"cleanup\":\"completed\"}")?;
        Ok(attempt)
    }

    fn load_attempt(&self, id: &str) -> Result<IntegrationAttempt, WorkspaceError> {
        validate_identifier(id, "integration ID")?;
        self.repo
            .get_integration_attempt(id)
            .map_err(map_repo_error)
    }
    fn save_attempt(
        &self,
        attempt: &IntegrationAttempt,
        detail: &str,
    ) -> Result<(), WorkspaceError> {
        self.repo
            .update_integration_attempt(attempt, detail)
            .map_err(map_repo_error)
    }
    fn revalidate_frozen_refs(
        &self,
        attempt: &IntegrationAttempt,
        reject_checked_out: bool,
    ) -> Result<(), WorkspaceError> {
        let root = Path::new(&attempt.repo_root);
        let target = required_utf8_line(
            self.git
                .capture(
                    root,
                    &["rev-parse", "--verify", attempt.target_ref.as_str()],
                )
                .map_err(map_git_error)?,
        )?;
        let source = required_utf8_line(
            self.git
                .capture(
                    root,
                    &["rev-parse", "--verify", attempt.source_ref.as_str()],
                )
                .map_err(map_git_error)?,
        )?;
        if target != attempt.expected_target_sha {
            return Err(integration_error(
                "INTEGRATION_TARGET_CHANGED",
                "Target ref changed after preflight",
            ));
        }
        if source != attempt.source_tip_sha {
            return Err(integration_error(
                "INTEGRATION_SOURCE_CHANGED",
                "Source tip changed after preflight",
            ));
        }
        let record = active_managed_record(self.repo.as_ref(), &attempt.task_id.0)?;
        let source_worktree = self.prove_registered_target(&record)?;
        ensure_no_in_progress_git_operation(&self.git, &source_worktree)?;
        let source_untracked = self
            .git
            .capture(
                &source_worktree,
                &["ls-files", "--others", "--exclude-standard", "-z"],
            )
            .map_err(map_git_error)?;
        if workspace_content_digest(&self.git, &source_worktree, &source_untracked)?
            != attempt.source_worktree_digest
        {
            return Err(integration_error(
                "INTEGRATION_SOURCE_CHANGED",
                "Source Worktree changed after preflight",
            ));
        }
        if reject_checked_out {
            for item in self.git.list_worktrees(root).map_err(map_git_error)? {
                if item
                    .branch
                    .as_deref()
                    .map(|branch| format!("refs/heads/{branch}"))
                    == Some(attempt.target_ref.clone())
                {
                    return Err(integration_error(
                        "INTEGRATION_TARGET_CHECKED_OUT",
                        "Target branch became checked out after preflight",
                    ));
                }
            }
        }
        Ok(())
    }
    fn create_integration_recovery(
        &self,
        attempt: &IntegrationAttempt,
    ) -> Result<PathBuf, WorkspaceError> {
        let raw_path = attempt.temporary_worktree_path.as_deref().ok_or_else(|| {
            integration_error(
                "INTEGRATION_RECOVERY_FAILED",
                "Temporary Worktree path is missing",
            )
        })?;
        let managed_root = self.ensure_managed_root()?;
        let worktree = validate_managed_worktree_target(&managed_root, Path::new(raw_path))
            .map_err(|_| {
                integration_error(
                    "INTEGRATION_RECOVERY_FAILED",
                    "Temporary Worktree failed recovery path proof",
                )
            })?;
        let untracked = self
            .git
            .capture(
                &worktree,
                &["ls-files", "--others", "--exclude-standard", "-z"],
            )
            .map_err(map_git_error)?;
        let source_branch = attempt
            .source_ref
            .strip_prefix("refs/heads/")
            .ok_or_else(|| {
                integration_error(
                    "INTEGRATION_RECOVERY_FAILED",
                    "Frozen source ref is not a local branch",
                )
            })?;
        let record = WorktreeRecord {
            id: WorktreeId::new(format!("integration-{}", attempt.id.0)),
            task_id: attempt.task_id.clone(),
            repo_root: attempt.repo_root.clone(),
            path: raw_path.to_owned(),
            display_path: raw_path.to_owned(),
            branch: source_branch.to_owned(),
            base_branch: attempt.target_ref.clone(),
            base_commit: attempt.expected_target_sha.clone(),
            ownership: WorktreeOwnership::Managed,
            state: WorktreeState::Quarantined,
            repo_identity: String::new(),
            common_git_dir: String::new(),
            relative_path: String::new(),
            created_at: attempt.created_at.clone(),
            last_verified_at: utc_now(),
            recovery_bundle_id: None,
            disk_usage_bytes: 0,
            locked: false,
            merged: false,
        };
        let evidence = self.create_recovery_package(&record, &worktree, &untracked)?;
        verify_recovery_package(&evidence)?;
        evidence
            .manifest_path
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| {
                integration_error(
                    "INTEGRATION_RECOVERY_FAILED",
                    "Recovery manifest has no safe directory",
                )
            })
    }
}

fn require_state(attempt: &IntegrationAttempt, allowed: &[&str]) -> Result<(), WorkspaceError> {
    if allowed.contains(&attempt.state.as_str()) {
        Ok(())
    } else {
        Err(integration_error(
            "INTEGRATION_INVALID_STATE",
            "Integration action is not allowed from its current state",
        ))
    }
}
fn validate_integration_message(value: &str) -> Result<(), WorkspaceError> {
    let value = value.trim();
    if value.len() > 500
        || ![
            "feat", "fix", "docs", "test", "chore", "refactor", "build", "ci",
        ]
        .iter()
        .any(|kind| value.starts_with(&format!("{kind}(")))
        || !value.contains("): ")
    {
        return Err(integration_error(
            "INTEGRATION_INVALID_MESSAGE",
            "Integration commit message must use the repository Conventional Commit format",
        ));
    }
    Ok(())
}
fn required_utf8_line(bytes: Vec<u8>) -> Result<String, WorkspaceError> {
    std::str::from_utf8(&bytes)
        .ok()
        .and_then(|s| s.lines().next())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| {
            integration_error(
                "INTEGRATION_INVALID_GIT_OUTPUT",
                "Git returned incomplete integration metadata",
            )
        })
}
fn utf8_lines(bytes: Vec<u8>) -> Result<Vec<String>, WorkspaceError> {
    let value = std::str::from_utf8(&bytes).map_err(|_| {
        integration_error(
            "INTEGRATION_INVALID_GIT_OUTPUT",
            "Git returned invalid UTF-8 integration metadata",
        )
    })?;
    Ok(value
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect())
}
fn split_nul_owned(bytes: Vec<u8>) -> Result<Vec<String>, WorkspaceError> {
    bytes
        .split(|b| *b == 0)
        .filter(|v| !v.is_empty())
        .map(|v| {
            std::str::from_utf8(v).map(str::to_owned).map_err(|_| {
                integration_error(
                    "INTEGRATION_INVALID_GIT_OUTPUT",
                    "Git returned an invalid conflict path",
                )
            })
        })
        .collect()
}
fn sha256_text(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn ensure_no_in_progress_git_operation(
    git: &GitCli,
    worktree: &Path,
) -> Result<(), WorkspaceError> {
    for marker in [
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "BISECT_LOG",
        "rebase-merge",
        "rebase-apply",
    ] {
        let raw = required_utf8_line(
            git.capture(worktree, &["rev-parse", "--git-path", marker])
                .map_err(map_git_error)?,
        )?;
        let candidate = PathBuf::from(raw);
        let candidate = if candidate.is_absolute() {
            candidate
        } else {
            worktree.join(candidate)
        };
        if candidate.exists() {
            return Err(integration_error(
                "INTEGRATION_GIT_OPERATION_IN_PROGRESS",
                "Source Worktree has an in-progress Git operation",
            ));
        }
    }
    Ok(())
}
fn constant_time_equal(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.as_bytes()
        .iter()
        .zip(right.as_bytes())
        .fold(0u8, |d, (a, b)| d | (a ^ b))
        == 0
}
fn integration_error(code: &'static str, message: &'static str) -> WorkspaceError {
    WorkspaceError { code, message }
}
