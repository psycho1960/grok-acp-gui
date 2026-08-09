//! GAG-013 isolated squash integration and compare-and-swap publication.

use super::*;
use crate::domain::types::{utc_now, IntegrationAttempt, IntegrationId, IntegrationState};
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
    pub expected_files: Vec<String>,
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
        if self
            .repo
            .get_active_integration_by_repo(&record.repo_identity, &record.repo_root)
            .map_err(map_repo_error)?
            .is_some()
        {
            return Err(integration_error(
                "INTEGRATION_LOCKED",
                "Repository already has an integration awaiting cleanup",
            ));
        }
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
        let expected_files = split_nul_owned(
            self.git
                .capture(
                    &source_root,
                    &[
                        "diff",
                        "--name-only",
                        "-z",
                        &expected_target_sha,
                        &source_tip_sha,
                    ],
                )
                .map_err(map_git_error)?,
        )?;
        let expected_files_json = serde_json::to_string(&expected_files).map_err(|_| {
            integration_error(
                "INTEGRATION_PLAN_INVALID",
                "Expected file list could not be encoded",
            )
        })?;
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
            "{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
            record.repo_identity,
            source_tip_sha,
            source_range_json,
            source.dirty,
            source_worktree_digest,
            expected_files_json,
            target_ref,
            expected_target_sha,
            request.commit_message,
            validation_digest
        ));
        let now = utc_now();
        let mut attempt = IntegrationAttempt {
            id: IntegrationId::new(attempt_id.clone()),
            task_id: request.task_id.clone(),
            repo_root: record.repo_root,
            repo_identity: record.repo_identity,
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
            state: IntegrationState::Draft,
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
        if let Err(error) = self.repo.create_integration_attempt(&attempt) {
            if self
                .repo
                .get_active_integration_by_repo(&attempt.repo_identity, &attempt.repo_root)
                .map_err(map_repo_error)?
                .is_some()
            {
                return Err(integration_error(
                    "INTEGRATION_LOCKED",
                    "Repository already has an integration awaiting cleanup",
                ));
            }
            return Err(map_repo_error(error));
        }
        attempt.state = IntegrationState::Preflight;
        attempt.updated_at = utc_now();
        self.save_attempt(&attempt, "{\"stage\":\"preflight_complete\"}")?;
        Ok(IntegrationPlan {
            attempt_id,
            task_id: request.task_id,
            source_ref: attempt.source_ref,
            source_tip_sha,
            source_range,
            source_dirty: source.dirty,
            source_worktree_digest,
            expected_files,
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
        if let Err(error) = self.revalidate_frozen_refs(&attempt, true) {
            attempt.state = IntegrationState::PreflightFailed;
            attempt.cleanup_status = "completed".into();
            attempt.updated_at = utc_now();
            self.save_attempt(&attempt, "{\"reason\":\"preflight_changed\"}")?;
            let _ = self.repo.update_task_status(
                &attempt.task_id.0,
                "ready_for_review",
                Some("integration preflight invalidated"),
            );
            return Err(error);
        }
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
        attempt.state = IntegrationState::Staging;
        attempt.temporary_worktree_id = Some(temp_id);
        attempt.temporary_worktree_path = Some(temp_arg.clone());
        attempt.temporary_branch = Some(temp_branch.clone());
        attempt.updated_at = utc_now();
        self.save_attempt(&attempt, "{\"stage\":\"worktree_planned\"}")?;
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
            attempt.state = IntegrationState::CleanupRequired;
            attempt.updated_at = utc_now();
            self.save_attempt(&attempt, "{\"reason\":\"worktree_add_failed\"}")?;
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
            attempt.state = IntegrationState::CleanupRequired;
            attempt.updated_at = utc_now();
            self.save_attempt(&attempt, "{\"reason\":\"worktree_verification_failed\"}")?;
            return Err(integration_error(
                "INTEGRATION_STAGING_FAILED",
                "Created integration Worktree failed Git metadata verification",
            ));
        }
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
                attempt.state = IntegrationState::CleanupRequired;
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
            attempt.state = IntegrationState::Conflicted;
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
            attempt.state = IntegrationState::ValidationFailed;
            attempt.updated_at = utc_now();
            self.save_attempt(&attempt, "{\"reason\":\"empty_squash\"}")?;
            return Err(integration_error(
                "INTEGRATION_EMPTY",
                "Squash produced no staged changes",
            ));
        }
        let mut expected_files = split_nul_owned(
            self.git
                .capture(
                    Path::new(&attempt.repo_root),
                    &[
                        "diff",
                        "--name-only",
                        "-z",
                        attempt.expected_target_sha.as_str(),
                        attempt.source_tip_sha.as_str(),
                    ],
                )
                .map_err(map_git_error)?,
        )?;
        let mut staged_files = split_nul_owned(
            self.git
                .capture(
                    temp,
                    &[
                        "diff",
                        "--cached",
                        "--name-only",
                        "-z",
                        attempt.expected_target_sha.as_str(),
                    ],
                )
                .map_err(map_git_error)?,
        )?;
        expected_files.sort();
        staged_files.sort();
        if staged_files != expected_files {
            attempt.state = IntegrationState::ValidationFailed;
            attempt.validation_result_json = Some("[{\"status\":\"preview_mismatch\"}]".into());
            attempt.updated_at = utc_now();
            self.save_attempt(&attempt, "{\"reason\":\"staged_files_mismatch\"}")?;
            return Err(integration_error(
                "INTEGRATION_PREVIEW_MISMATCH",
                "Staged files no longer match the approved integration preview",
            ));
        }
        let staged_patch = self
            .git
            .capture(
                temp,
                &[
                    "diff",
                    "--cached",
                    "--binary",
                    "--full-index",
                    "--no-ext-diff",
                    attempt.expected_target_sha.as_str(),
                ],
            )
            .map_err(map_git_error)?;
        let staged_patch_digest = sha256_bytes(&staged_patch);
        attempt.state = IntegrationState::Validating;
        attempt.validation_result_json = Some(
            serde_json::json!([{
                "status": "staged_verified",
                "patchDigest": staged_patch_digest,
            }])
            .to_string(),
        );
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
            attempt.state = IntegrationState::ValidationFailed;
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
            attempt.state = IntegrationState::CleanupRequired;
            attempt.updated_at = utc_now();
            self.save_attempt(&attempt, "{\"reason\":\"parent_mismatch\"}")?;
            return Err(integration_error(
                "INTEGRATION_PARENT_MISMATCH",
                "Integration commit parent is not the frozen target HEAD",
            ));
        }
        let committed_patch = self
            .git
            .capture(
                temp,
                &[
                    "diff",
                    "--binary",
                    "--full-index",
                    "--no-ext-diff",
                    attempt.expected_target_sha.as_str(),
                    result.as_str(),
                ],
            )
            .map_err(map_git_error)?;
        if committed_patch != staged_patch {
            attempt.result_commit_sha = Some(result);
            attempt.state = IntegrationState::ValidationFailed;
            attempt.validation_result_json = Some("[{\"status\":\"commit_tree_mismatch\"}]".into());
            attempt.updated_at = utc_now();
            self.save_attempt(&attempt, "{\"reason\":\"commit_tree_mismatch\"}")?;
            return Err(integration_error(
                "INTEGRATION_COMMIT_CHANGED",
                "Commit hooks changed the approved staged integration content",
            ));
        }
        attempt.result_commit_sha = Some(result);
        attempt.state = IntegrationState::ReadyToPublish;
        attempt.updated_at = utc_now();
        self.save_attempt(&attempt, "{\"stage\":\"ready_to_publish\"}")?;
        Ok(attempt)
    }

    pub(super) fn integration_status(
        &self,
        id: &str,
    ) -> Result<IntegrationAttempt, WorkspaceError> {
        let mut attempt = self.load_attempt(id)?;
        if attempt.state == IntegrationState::Validating {
            let _lock = self.lifecycle_lock.lock().map_err(|_| {
                integration_error(
                    "INTEGRATION_LOCKED",
                    "Repository integration lock is unavailable",
                )
            })?;
            attempt = self.load_attempt(id)?;
            attempt = self.reconcile_validating_attempt_locked(attempt)?;
        }
        if attempt.state != IntegrationState::Publishing {
            return Ok(attempt);
        }
        match self.publish_integration_attempt(id, &attempt.approval_digest) {
            Ok(reconciled) => Ok(reconciled),
            Err(_) => self.load_attempt(id),
        }
    }

    pub(super) fn active_integration_for_task(
        &self,
        task_id: &str,
    ) -> Result<Option<IntegrationAttempt>, WorkspaceError> {
        validate_identifier(task_id, "task ID")?;
        let attempt = self
            .repo
            .get_active_integration_by_task(task_id)
            .map_err(map_repo_error)?;
        attempt
            .map(|item| self.integration_status(&item.id.0))
            .transpose()
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
        if attempt.state == IntegrationState::Validating {
            attempt = self.reconcile_validating_attempt_locked(attempt)?;
        }
        require_state(&attempt, &["ready_to_publish", "publishing"])?;
        if !constant_time_equal(&attempt.approval_digest, approval_digest) {
            return Err(integration_error(
                "INTEGRATION_APPROVAL_INVALID",
                "Integration approval no longer matches the plan",
            ));
        }
        let result = attempt.result_commit_sha.clone().ok_or_else(|| {
            integration_error(
                "INTEGRATION_RESULT_MISSING",
                "Integration result commit is missing",
            )
        })?;
        let current_target = required_utf8_line(
            self.git
                .capture(
                    Path::new(&attempt.repo_root),
                    &["rev-parse", "--verify", attempt.target_ref.as_str()],
                )
                .map_err(map_git_error)?,
        )?;
        if attempt.state == IntegrationState::Publishing && current_target == result {
            attempt.state = IntegrationState::Completed;
            attempt.updated_at = utc_now();
            self.save_attempt(&attempt, "{\"stage\":\"published_reconciled\"}")?;
            let _ = self
                .repo
                .update_task_status(&attempt.task_id.0, "merged", None);
            return Ok(attempt);
        }
        if let Err(error) = self.revalidate_frozen_refs(&attempt, true) {
            attempt.state = IntegrationState::PublishRejected;
            attempt.updated_at = utc_now();
            self.save_attempt(&attempt, "{\"reason\":\"frozen_ref_changed\"}")?;
            return Err(error);
        }
        if attempt.state != IntegrationState::Publishing {
            attempt.state = IntegrationState::Publishing;
            attempt.updated_at = utc_now();
            self.save_attempt(&attempt, "{\"stage\":\"publishing\"}")?;
        }
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
            attempt.state = IntegrationState::PublishRejected;
            attempt.updated_at = utc_now();
            self.save_attempt(&attempt, "{\"reason\":\"cas_failed\"}")?;
            return Err(integration_error(
                "INTEGRATION_TARGET_CHANGED",
                "Target ref changed before atomic publication",
            ));
        }
        attempt.state = IntegrationState::Completed;
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
        if attempt.state == IntegrationState::Validating {
            attempt = self.reconcile_validating_attempt_locked(attempt)?;
        }
        if matches!(attempt.state.as_str(), "completed" | "aborted") {
            return Err(integration_error(
                "INTEGRATION_INVALID_STATE",
                "Completed or aborted integration cannot be aborted",
            ));
        }
        if attempt
            .temporary_worktree_path
            .as_deref()
            .is_some_and(|path| Path::new(path).exists())
        {
            attempt.recovery_bundle_path = Some(
                self.create_integration_recovery(&attempt)?
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        attempt.state = IntegrationState::Aborted;
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
                "staging",
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
            .map_err(|_| {
                integration_error(
                    "INTEGRATION_CLEANUP_INSPECTION_FAILED",
                    "Git Worktree metadata could not be inspected during cleanup",
                )
            })?;
        let worktree_present = listed
            .iter()
            .any(|item| same_path_identity(&item.path, &target));
        if let Some(item) = listed
            .iter()
            .find(|item| same_path_identity(&item.path, &target))
        {
            if item.branch.as_deref() != attempt.temporary_branch.as_deref() {
                return Err(integration_error(
                    "INTEGRATION_CLEANUP_REJECTED",
                    "Temporary Worktree ownership proof no longer matches Git metadata",
                ));
            }
        } else if target.exists() {
            return Err(integration_error(
                "INTEGRATION_CLEANUP_REJECTED",
                "Unregistered content remains at the temporary Worktree path",
            ));
        }
        let branch_head = if let Some(branch) = attempt.temporary_branch.as_deref() {
            let branch_ref = format!("refs/heads/{branch}");
            let output = self
                .git
                .capture_allow_status(
                    Path::new(&attempt.repo_root),
                    &["rev-parse", "--verify", "--quiet", branch_ref.as_str()],
                    &[0, 1],
                )
                .map_err(|_| {
                    integration_error(
                        "INTEGRATION_CLEANUP_INSPECTION_FAILED",
                        "Temporary branch metadata could not be inspected during cleanup",
                    )
                })?;
            (output.status == 0)
                .then(|| required_utf8_line(output.stdout))
                .transpose()?
        } else {
            None
        };
        if attempt.result_commit_sha.is_none() {
            if let Some(head) = branch_head.as_deref() {
                if head != attempt.expected_target_sha {
                    let parent = required_utf8_line(
                        self.git
                            .capture(
                                Path::new(&attempt.repo_root),
                                &["rev-parse", &format!("{head}^")],
                            )
                            .map_err(map_git_error)?,
                    )?;
                    if parent != attempt.expected_target_sha {
                        return Err(integration_error(
                            "INTEGRATION_CLEANUP_REJECTED",
                            "Temporary branch moved outside the frozen integration result",
                        ));
                    }
                    attempt.result_commit_sha = Some(head.to_owned());
                    attempt.updated_at = utc_now();
                    self.save_attempt(&attempt, "{\"cleanup\":\"commit_receipt_reconciled\"}")?;
                }
            }
        }
        if !worktree_present && branch_head.is_none() {
            attempt.cleanup_status = "completed".into();
            attempt.updated_at = utc_now();
            self.save_attempt(&attempt, "{\"cleanup\":\"planned_resources_absent\"}")?;
            return Ok(attempt);
        }
        if attempt.state != IntegrationState::Completed && attempt.recovery_bundle_path.is_none() {
            if !worktree_present {
                return Err(integration_error(
                    "INTEGRATION_RECOVERY_FAILED",
                    "Temporary branch remains but its Worktree is unavailable for recovery",
                ));
            }
            attempt.recovery_bundle_path = Some(
                self.create_integration_recovery(&attempt)?
                    .to_string_lossy()
                    .into_owned(),
            );
            attempt.updated_at = utc_now();
            self.save_attempt(&attempt, "{\"cleanup\":\"recovery_verified\"}")?;
        }
        if worktree_present {
            let target_arg = target.to_string_lossy().into_owned();
            let args = ["worktree", "remove", "--force", target_arg.as_str()];
            authorize_managed_git(Path::new(&attempt.repo_root), &args, &[&target])?;
            self.git
                .run_checked(Path::new(&attempt.repo_root), &args)
                .map_err(map_git_error)?;
            attempt.cleanup_status = "worktree_removed".into();
            attempt.updated_at = utc_now();
            self.save_attempt(&attempt, "{\"cleanup\":\"worktree_removed\"}")?;
        }
        if let Some(branch) = attempt.temporary_branch.as_deref() {
            if let Some(head) = branch_head {
                let owned = head == attempt.expected_target_sha
                    || attempt.result_commit_sha.as_deref() == Some(head.as_str());
                if !owned {
                    return Err(integration_error(
                        "INTEGRATION_CLEANUP_REJECTED",
                        "Temporary branch moved outside the frozen integration result",
                    ));
                }
                let branch_ref = format!("refs/heads/{branch}");
                let branch_args = ["update-ref", "-d", branch_ref.as_str(), head.as_str()];
                authorize_managed_git(
                    Path::new(&attempt.repo_root),
                    &branch_args,
                    &[Path::new(&attempt.repo_root)],
                )?;
                self.git
                    .run_checked(Path::new(&attempt.repo_root), &branch_args)
                    .map_err(map_git_error)?;
            }
        }
        attempt.cleanup_status = "branch_removed".into();
        attempt.updated_at = utc_now();
        self.save_attempt(&attempt, "{\"cleanup\":\"branch_removed\"}")?;
        attempt.cleanup_status = "completed".into();
        attempt.updated_at = utc_now();
        self.save_attempt(&attempt, "{\"cleanup\":\"completed\"}")?;
        Ok(attempt)
    }

    pub(super) fn open_integration_worktree_attempt(&self, id: &str) -> Result<(), WorkspaceError> {
        let _lock = self.lifecycle_lock.lock().map_err(|_| {
            integration_error(
                "INTEGRATION_LOCKED",
                "Repository integration lock is unavailable",
            )
        })?;
        let attempt = self.load_attempt(id)?;
        require_state(
            &attempt,
            &[
                "conflicted",
                "validation_failed",
                "cleanup_required",
                "ready_to_publish",
            ],
        )?;
        let raw_path = attempt.temporary_worktree_path.as_deref().ok_or_else(|| {
            integration_error(
                "INTEGRATION_WORKTREE_MISSING",
                "Temporary integration Worktree path is missing",
            )
        })?;
        let managed_root = self.ensure_managed_root()?;
        let target =
            validate_managed_worktree_target(&managed_root, Path::new(raw_path)).map_err(|_| {
                integration_error(
                    "INTEGRATION_PATH_REJECTED",
                    "Temporary Worktree failed managed-root path proof",
                )
            })?;
        let owned = self
            .git
            .list_worktrees(Path::new(&attempt.repo_root))
            .map_err(map_git_error)?
            .into_iter()
            .any(|item| {
                same_path_identity(&item.path, &target)
                    && item.branch.as_deref() == attempt.temporary_branch.as_deref()
            });
        if !owned {
            return Err(integration_error(
                "INTEGRATION_WORKTREE_MISSING",
                "Temporary integration Worktree ownership could not be verified",
            ));
        }
        reveal_managed_directory(&target, &managed_root).map_err(|_| {
            integration_error(
                "INTEGRATION_OPEN_FAILED",
                "Temporary integration Worktree could not be opened",
            )
        })
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

    fn reconcile_validating_attempt_locked(
        &self,
        mut attempt: IntegrationAttempt,
    ) -> Result<IntegrationAttempt, WorkspaceError> {
        if attempt.state != IntegrationState::Validating || attempt.result_commit_sha.is_some() {
            return Ok(attempt);
        }
        let raw_path = attempt.temporary_worktree_path.as_deref().ok_or_else(|| {
            integration_error(
                "INTEGRATION_RESULT_MISSING",
                "Validating integration has no temporary Worktree",
            )
        })?;
        let managed_root = self.ensure_managed_root()?;
        let target =
            validate_managed_worktree_target(&managed_root, Path::new(raw_path)).map_err(|_| {
                integration_error(
                    "INTEGRATION_PATH_REJECTED",
                    "Temporary Worktree failed managed-root path proof",
                )
            })?;
        let owned = self
            .git
            .list_worktrees(Path::new(&attempt.repo_root))
            .map_err(map_git_error)?
            .into_iter()
            .any(|item| {
                same_path_identity(&item.path, &target)
                    && item.branch.as_deref() == attempt.temporary_branch.as_deref()
            });
        if !owned {
            return Err(integration_error(
                "INTEGRATION_WORKTREE_MISSING",
                "Temporary integration Worktree ownership could not be verified",
            ));
        }
        let head = required_utf8_line(
            self.git
                .capture(&target, &["rev-parse", "HEAD"])
                .map_err(map_git_error)?,
        )?;
        if head == attempt.expected_target_sha {
            return Ok(attempt);
        }
        let parent = required_utf8_line(
            self.git
                .capture(&target, &["rev-parse", "HEAD^"])
                .map_err(map_git_error)?,
        )?;
        if parent != attempt.expected_target_sha {
            attempt.state = IntegrationState::CleanupRequired;
            attempt.updated_at = utc_now();
            self.save_attempt(&attempt, "{\"reason\":\"parent_mismatch_reconciled\"}")?;
            return Ok(attempt);
        }
        let expected_patch_digest = attempt
            .validation_result_json
            .as_deref()
            .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
            .and_then(|value| {
                value
                    .get(0)
                    .and_then(|item| item.get("patchDigest"))
                    .and_then(|digest| digest.as_str().map(str::to_owned))
            });
        let committed_patch = self
            .git
            .capture(
                &target,
                &[
                    "diff",
                    "--binary",
                    "--full-index",
                    "--no-ext-diff",
                    attempt.expected_target_sha.as_str(),
                    head.as_str(),
                ],
            )
            .map_err(map_git_error)?;
        if expected_patch_digest.as_deref() != Some(sha256_bytes(&committed_patch).as_str()) {
            attempt.result_commit_sha = Some(head);
            attempt.state = IntegrationState::ValidationFailed;
            attempt.validation_result_json =
                Some("[{\"status\":\"commit_tree_mismatch_reconciled\"}]".into());
            attempt.updated_at = utc_now();
            self.save_attempt(&attempt, "{\"reason\":\"commit_tree_mismatch_reconciled\"}")?;
            return Ok(attempt);
        }
        attempt.result_commit_sha = Some(head);
        attempt.state = IntegrationState::ReadyToPublish;
        attempt.updated_at = utc_now();
        self.save_attempt(&attempt, "{\"stage\":\"commit_receipt_reconciled\"}")?;
        Ok(attempt)
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
        let temporary_branch = attempt.temporary_branch.as_deref().ok_or_else(|| {
            integration_error(
                "INTEGRATION_RECOVERY_FAILED",
                "Temporary integration branch is missing",
            )
        })?;
        let record = WorktreeRecord {
            id: WorktreeId::new(format!("integration-{}", attempt.id.0)),
            task_id: attempt.task_id.clone(),
            repo_root: attempt.repo_root.clone(),
            path: raw_path.to_owned(),
            display_path: raw_path.to_owned(),
            branch: temporary_branch.to_owned(),
            base_branch: attempt
                .target_ref
                .strip_prefix("refs/heads/")
                .unwrap_or(attempt.target_ref.as_str())
                .to_owned(),
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
    sha256_bytes(value.as_bytes())
}
fn sha256_bytes(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
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
