use grok_acp_gui_lib::adapters::sqlite::SqliteRepository;
use grok_acp_gui_lib::domain::types::{
    CheckpointRecord, IntegrationId, IntegrationState, Project, ProjectId, Task, TaskId,
    TaskStatus, WorkspaceKind,
};
use grok_acp_gui_lib::modules::persistence::Repository;
use grok_acp_gui_lib::modules::workspace::{
    CheckpointSelection, CreateManagedWorktree, ManagedWorkspaceService, PrepareSquash,
    WorkspaceService,
};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

fn git(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

struct Fixture {
    primary: PathBuf,
    managed: PathBuf,
    recovery: PathBuf,
    task_worktree: PathBuf,
    task_branch: String,
    repo: Arc<dyn Repository>,
    service: ManagedWorkspaceService,
}
impl Fixture {
    fn new() -> Self {
        let suffix = uuid::Uuid::new_v4();
        let primary = std::env::temp_dir().join(format!("gag-013-primary-{suffix}"));
        let managed = std::env::temp_dir().join(format!("gag-013-managed-{suffix}"));
        let recovery = std::env::temp_dir().join(format!("gag-013-recovery-{suffix}"));
        std::fs::create_dir_all(&primary).unwrap();
        git(&primary, &["init", "-b", "main"]);
        git(&primary, &["config", "user.name", "GAG 013"]);
        git(
            &primary,
            &["config", "user.email", "gag013@example.invalid"],
        );
        std::fs::write(primary.join("shared.txt"), "base\n").unwrap();
        git(&primary, &["add", "--", "shared.txt"]);
        git(&primary, &["commit", "-m", "fixture"]);
        let repo: Arc<dyn Repository> = Arc::new(SqliteRepository::open_in_memory().unwrap());
        let now = grok_acp_gui_lib::domain::types::utc_now();
        repo.create_project(&Project {
            id: ProjectId::new("project-gag-013"),
            path: primary.to_string_lossy().into_owned(),
            display_path: "fixture".into(),
            repo_root: Some(primary.to_string_lossy().into_owned()),
            trusted_at: Some(now.clone()),
            last_opened_at: now.clone(),
        })
        .unwrap();
        repo.create_task(&Task {
            id: TaskId::new("GAG-013-task"),
            project_id: ProjectId::new("project-gag-013"),
            title: "squash integration".into(),
            status: TaskStatus::ReadyForReview,
            workspace_kind: WorkspaceKind::Worktree,
            mode: Some("agent".into()),
            model: None,
            reasoning: None,
            created_at: now.clone(),
            updated_at: now,
            interrupt_reason: None,
            interrupted_at: None,
            attempt_count: 0,
        })
        .unwrap();
        let service = ManagedWorkspaceService::new(repo.clone(), managed.clone(), recovery.clone());
        let record = service
            .create_managed_worktree(CreateManagedWorktree {
                repo_root: primary.clone(),
                task_id: TaskId::new("GAG-013-task"),
                task_slug: "ignored".into(),
                base_ref: "main".into(),
            })
            .unwrap();
        Self {
            primary,
            managed,
            recovery,
            task_worktree: PathBuf::from(record.path),
            task_branch: record.branch,
            repo,
            service,
        }
    }
    fn checkpoint(&self, content: &str) {
        std::fs::write(self.task_worktree.join("shared.txt"), content).unwrap();
        let snapshot = self.service.get_worktree_status("GAG-013-task").unwrap();
        let file = snapshot
            .files
            .iter()
            .find(|f| f.path == "shared.txt")
            .unwrap();
        self.service
            .create_checkpoint(
                "GAG-013-task",
                "feat(GAG-013): checkpoint source [GAG-013]",
                &[CheckpointSelection {
                    path: file.path.clone(),
                    fingerprint: file.fingerprint.clone(),
                }],
            )
            .unwrap();
    }
    fn commit_all_and_record_checkpoint(&self, message: &str) {
        let before = git(&self.task_worktree, &["rev-parse", "HEAD"]);
        git(&self.task_worktree, &["add", "-A"]);
        git(&self.task_worktree, &["commit", "-m", message]);
        self.record_current_head(&before, message);
    }
    fn record_current_head(&self, head_before: &str, message: &str) {
        let commit_sha = git(&self.task_worktree, &["rev-parse", "HEAD"]);
        let tree_sha = git(&self.task_worktree, &["rev-parse", "HEAD^{tree}"]);
        self.repo
            .create_checkpoint(&CheckpointRecord {
                id: uuid::Uuid::new_v4().to_string(),
                task_id: TaskId::new("GAG-013-task"),
                attempt_number: 1,
                commit_sha,
                tree_sha,
                head_before: head_before.into(),
                selection_manifest: "[\"binary.dat\",\"renamed.txt\",\"shared.txt\"]".into(),
                selection_hash: "fixture".into(),
                message: message.into(),
                created_at: grok_acp_gui_lib::domain::types::utc_now(),
            })
            .unwrap();
    }
    fn detach_target(&self) {
        git(&self.primary, &["switch", "--detach"]);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let listed = git(&self.primary, &["worktree", "list", "--porcelain"]);
        for line in listed.lines().filter_map(|l| l.strip_prefix("worktree ")) {
            let path = PathBuf::from(line);
            if path != self.primary {
                let _ = Command::new("git")
                    .args(["worktree", "remove", "--force", line])
                    .current_dir(&self.primary)
                    .output();
            }
        }
        let _ = Command::new("git")
            .args(["branch", "-D", &self.task_branch])
            .current_dir(&self.primary)
            .output();
        let _ = std::fs::remove_dir_all(&self.primary);
        let _ = std::fs::remove_dir_all(&self.managed);
        let _ = std::fs::remove_dir_all(&self.recovery);
    }
}

#[test]
fn squash_publish_is_atomic_and_primary_files_and_index_stay_unchanged() {
    let fixture = Fixture::new();
    fixture.checkpoint("source\n");
    std::fs::write(
        fixture.task_worktree.join("unselected.txt"),
        "keep in task Worktree\n",
    )
    .unwrap();
    fixture.detach_target();
    let before_head = git(&fixture.primary, &["rev-parse", "HEAD"]);
    let before_tree = git(&fixture.primary, &["write-tree"]);
    let before_file = std::fs::read(fixture.primary.join("shared.txt")).unwrap();
    let plan = fixture
        .service
        .prepare_squash(PrepareSquash {
            task_id: TaskId::new("GAG-013-task"),
            commit_message: "feat(GAG-013): squash checkpoints".into(),
        })
        .unwrap();
    let staged = fixture
        .service
        .start_squash(&plan.attempt_id, &plan.approval_digest)
        .unwrap();
    assert_eq!(staged.state, "ready_to_publish");
    let published = fixture
        .service
        .publish_integration(&plan.attempt_id, &plan.approval_digest)
        .unwrap();
    assert_eq!(published.state, "completed");
    assert_eq!(
        git(&fixture.primary, &["rev-parse", "refs/heads/main"]),
        published.result_commit_sha.unwrap()
    );
    assert!(git(
        &fixture.primary,
        &["show", "--pretty=format:", "refs/heads/main"]
    )
    .contains("+source"));
    assert!(!git(
        &fixture.primary,
        &["show", "--pretty=format:", "refs/heads/main"]
    )
    .contains("unselected.txt"));
    assert_eq!(
        std::fs::read_to_string(fixture.task_worktree.join("unselected.txt")).unwrap(),
        "keep in task Worktree\n"
    );
    assert_eq!(git(&fixture.primary, &["rev-parse", "HEAD"]), before_head);
    assert_eq!(git(&fixture.primary, &["write-tree"]), before_tree);
    assert_eq!(
        std::fs::read(fixture.primary.join("shared.txt")).unwrap(),
        before_file
    );
    assert_eq!(
        fixture
            .service
            .cleanup_integration(&plan.attempt_id)
            .unwrap()
            .cleanup_status,
        "completed"
    );
}

#[test]
fn target_advance_after_preflight_rejects_without_staging_or_ref_overwrite() {
    let fixture = Fixture::new();
    fixture.checkpoint("source\n");
    fixture.detach_target();
    let plan = fixture
        .service
        .prepare_squash(PrepareSquash {
            task_id: TaskId::new("GAG-013-task"),
            commit_message: "fix(GAG-013): guarded squash".into(),
        })
        .unwrap();
    let old = git(&fixture.primary, &["rev-parse", "refs/heads/main"]);
    git(
        &fixture.primary,
        &[
            "update-ref",
            "refs/heads/main",
            &fixture
                .service
                .inspect_repository(&fixture.task_worktree)
                .unwrap()
                .head,
            &old,
        ],
    );
    let error = fixture
        .service
        .start_squash(&plan.attempt_id, &plan.approval_digest)
        .unwrap_err();
    assert_eq!(error.code, "INTEGRATION_TARGET_CHANGED");
}

#[test]
fn conflict_remains_isolated_and_cleanup_requires_recovery_evidence() {
    let fixture = Fixture::new();
    fixture.checkpoint("source\n");
    std::fs::write(fixture.primary.join("shared.txt"), "target\n").unwrap();
    git(&fixture.primary, &["add", "--", "shared.txt"]);
    git(&fixture.primary, &["commit", "-m", "target change"]);
    fixture.detach_target();
    let target = git(&fixture.primary, &["rev-parse", "refs/heads/main"]);
    let plan = fixture
        .service
        .prepare_squash(PrepareSquash {
            task_id: TaskId::new("GAG-013-task"),
            commit_message: "fix(GAG-013): conflicting squash".into(),
        })
        .unwrap();
    let result = fixture
        .service
        .start_squash(&plan.attempt_id, &plan.approval_digest)
        .unwrap();
    assert_eq!(result.state, "conflicted");
    assert!(result.conflict_summary_json.unwrap().contains("shared.txt"));
    assert_eq!(
        git(&fixture.primary, &["rev-parse", "refs/heads/main"]),
        target
    );
    let cleaned = fixture
        .service
        .cleanup_integration(&plan.attempt_id)
        .unwrap();
    assert_eq!(cleaned.cleanup_status, "completed");
    assert!(cleaned.recovery_bundle_path.is_some());
    assert_eq!(
        git(&fixture.primary, &["rev-parse", "refs/heads/main"]),
        target
    );
}

#[test]
fn multiple_checkpoints_squash_once_and_frozen_source_and_approval_fail_closed() {
    let fixture = Fixture::new();
    fixture.checkpoint("checkpoint one\n");
    fixture.checkpoint("checkpoint two\n");
    fixture.detach_target();
    let plan = fixture
        .service
        .prepare_squash(PrepareSquash {
            task_id: TaskId::new("GAG-013-task"),
            commit_message: "feat(GAG-013): squash two checkpoints".into(),
        })
        .unwrap();
    assert_eq!(plan.source_range.len(), 2);
    let approval_error = fixture
        .service
        .start_squash(&plan.attempt_id, "wrong-digest")
        .unwrap_err();
    assert_eq!(approval_error.code, "INTEGRATION_APPROVAL_INVALID");
    fixture.checkpoint("checkpoint three after approval\n");
    let source_error = fixture
        .service
        .start_squash(&plan.attempt_id, &plan.approval_digest)
        .unwrap_err();
    assert_eq!(source_error.code, "INTEGRATION_SOURCE_CHANGED");

    let fresh = fixture
        .service
        .prepare_squash(PrepareSquash {
            task_id: TaskId::new("GAG-013-task"),
            commit_message: "feat(GAG-013): squash three checkpoints".into(),
        })
        .unwrap();
    let ready = fixture
        .service
        .start_squash(&fresh.attempt_id, &fresh.approval_digest)
        .unwrap();
    assert_eq!(ready.state, "ready_to_publish");
    let result = fixture
        .service
        .publish_integration(&fresh.attempt_id, &fresh.approval_digest)
        .unwrap();
    let result_sha = result.result_commit_sha.unwrap();
    assert_eq!(
        git(
            &fixture.primary,
            &[
                "rev-list",
                "--count",
                &format!("{}..{}", fresh.expected_target_sha, result_sha)
            ]
        ),
        "1"
    );
    fixture
        .service
        .cleanup_integration(&fresh.attempt_id)
        .unwrap();
}

#[test]
fn empty_source_and_checked_out_target_are_rejected_before_side_effects() {
    let fixture = Fixture::new();
    let checked_out = fixture
        .service
        .prepare_squash(PrepareSquash {
            task_id: TaskId::new("GAG-013-task"),
            commit_message: "fix(GAG-013): reject checked out target".into(),
        })
        .unwrap_err();
    assert_eq!(checked_out.code, "INTEGRATION_TARGET_CHECKED_OUT");
    fixture.detach_target();
    let empty = fixture
        .service
        .prepare_squash(PrepareSquash {
            task_id: TaskId::new("GAG-013-task"),
            commit_message: "fix(GAG-013): reject empty source".into(),
        })
        .unwrap_err();
    assert_eq!(empty.code, "INTEGRATION_EMPTY");
}

#[test]
fn dirty_snapshot_change_and_in_progress_git_operation_invalidate_preflight() {
    let fixture = Fixture::new();
    fixture.checkpoint("source\n");
    std::fs::write(
        fixture.task_worktree.join("unselected.txt"),
        "approved dirty state\n",
    )
    .unwrap();
    fixture.detach_target();
    let plan = fixture
        .service
        .prepare_squash(PrepareSquash {
            task_id: TaskId::new("GAG-013-task"),
            commit_message: "fix(GAG-013): bind dirty snapshot".into(),
        })
        .unwrap();
    assert!(plan.source_dirty);
    std::fs::write(
        fixture.task_worktree.join("unselected.txt"),
        "changed after approval\n",
    )
    .unwrap();
    let changed = fixture
        .service
        .start_squash(&plan.attempt_id, &plan.approval_digest)
        .unwrap_err();
    assert_eq!(changed.code, "INTEGRATION_SOURCE_CHANGED");

    std::fs::write(
        fixture.task_worktree.join("unselected.txt"),
        "approved dirty state\n",
    )
    .unwrap();
    let marker = PathBuf::from(git(
        &fixture.task_worktree,
        &["rev-parse", "--git-path", "MERGE_HEAD"],
    ));
    std::fs::write(marker, "0000000000000000000000000000000000000000\n").unwrap();
    let operation = fixture
        .service
        .prepare_squash(PrepareSquash {
            task_id: TaskId::new("GAG-013-task"),
            commit_message: "fix(GAG-013): reject merge in progress".into(),
        })
        .unwrap_err();
    assert_eq!(operation.code, "INTEGRATION_GIT_OPERATION_IN_PROGRESS");
}

#[test]
fn publishing_attempt_reconciles_when_cas_succeeded_before_receipt_persisted() {
    let fixture = Fixture::new();
    fixture.checkpoint("source\n");
    fixture.detach_target();
    let plan = fixture
        .service
        .prepare_squash(PrepareSquash {
            task_id: TaskId::new("GAG-013-task"),
            commit_message: "fix(GAG-013): reconcile published receipt".into(),
        })
        .unwrap();
    let mut attempt = fixture
        .service
        .start_squash(&plan.attempt_id, &plan.approval_digest)
        .unwrap();
    let result = attempt.result_commit_sha.clone().unwrap();
    attempt.state = IntegrationState::Publishing;
    fixture
        .repo
        .update_integration_attempt(&attempt, "{\"fault\":\"after_cas_before_receipt\"}")
        .unwrap();
    git(
        &fixture.primary,
        &[
            "update-ref",
            &plan.target_ref,
            &result,
            &plan.expected_target_sha,
        ],
    );

    let recovered = fixture
        .service
        .publish_integration(&plan.attempt_id, &plan.approval_digest)
        .unwrap();
    assert_eq!(recovered.state, "completed");
    assert_eq!(
        recovered.result_commit_sha.as_deref(),
        Some(result.as_str())
    );
}

#[test]
fn validating_attempt_reconciles_when_commit_succeeded_before_receipt_persisted() {
    let fixture = Fixture::new();
    fixture.checkpoint("source\n");
    fixture.detach_target();
    let plan = fixture
        .service
        .prepare_squash(PrepareSquash {
            task_id: TaskId::new("GAG-013-task"),
            commit_message: "fix(GAG-013): reconcile commit receipt".into(),
        })
        .unwrap();
    let mut attempt = fixture
        .service
        .start_squash(&plan.attempt_id, &plan.approval_digest)
        .unwrap();
    let result = attempt.result_commit_sha.take().unwrap();
    attempt.state = IntegrationState::Validating;
    fixture
        .repo
        .update_integration_attempt(&attempt, "{\"fault\":\"after_commit_before_receipt\"}")
        .unwrap();

    let recovered = fixture
        .service
        .get_active_integration("GAG-013-task")
        .unwrap()
        .unwrap();
    assert_eq!(recovered.state, IntegrationState::ReadyToPublish);
    assert_eq!(
        recovered.result_commit_sha.as_deref(),
        Some(result.as_str())
    );

    fixture.service.abort_integration(&plan.attempt_id).unwrap();
    let cleaned = fixture
        .service
        .cleanup_integration(&plan.attempt_id)
        .unwrap();
    assert_eq!(cleaned.cleanup_status, "completed");
}

#[test]
fn validating_crash_recovery_rejects_commit_hook_content_injection() {
    let fixture = Fixture::new();
    fixture.checkpoint("source\n");
    fixture.detach_target();
    let plan = fixture
        .service
        .prepare_squash(PrepareSquash {
            task_id: TaskId::new("GAG-013-task"),
            commit_message: "fix(GAG-013): recover content receipt".into(),
        })
        .unwrap();
    let mut attempt = fixture
        .service
        .start_squash(&plan.attempt_id, &plan.approval_digest)
        .unwrap();
    let temporary = PathBuf::from(attempt.temporary_worktree_path.as_deref().unwrap());
    git(&temporary, &["reset", "--soft", &plan.expected_target_sha]);
    std::fs::write(temporary.join("injected.txt"), "injected\n").unwrap();
    git(&temporary, &["add", "--", "injected.txt"]);
    git(
        &temporary,
        &[
            "commit",
            "--no-gpg-sign",
            "-m",
            "fix(GAG-013): simulated hook injection",
        ],
    );
    attempt.state = IntegrationState::Validating;
    attempt.result_commit_sha = None;
    fixture
        .repo
        .update_integration_attempt(
            &attempt,
            "{\"fault\":\"after_injected_commit_before_validation\"}",
        )
        .unwrap();

    let recovered = fixture
        .service
        .get_integration_status(&plan.attempt_id)
        .unwrap();
    assert_eq!(recovered.state, IntegrationState::ValidationFailed);
    assert!(recovered.result_commit_sha.is_some());
    assert_eq!(
        git(&fixture.primary, &["rev-parse", &plan.target_ref]),
        plan.expected_target_sha
    );
}

#[test]
fn cleanup_retries_after_worktree_was_removed_before_branch_and_receipt() {
    let fixture = Fixture::new();
    fixture.checkpoint("source\n");
    fixture.detach_target();
    let plan = fixture
        .service
        .prepare_squash(PrepareSquash {
            task_id: TaskId::new("GAG-013-task"),
            commit_message: "fix(GAG-013): retry partial cleanup".into(),
        })
        .unwrap();
    let ready = fixture
        .service
        .start_squash(&plan.attempt_id, &plan.approval_digest)
        .unwrap();
    let aborted = fixture.service.abort_integration(&plan.attempt_id).unwrap();
    let path = aborted.temporary_worktree_path.clone().unwrap();
    let branch = aborted.temporary_branch.clone().unwrap();
    git(&fixture.primary, &["worktree", "remove", "--force", &path]);

    let cleaned = fixture
        .service
        .cleanup_integration(&plan.attempt_id)
        .unwrap();
    assert_eq!(cleaned.cleanup_status, "completed");
    assert!(!Path::new(&path).exists());
    let branch_check = Command::new("git")
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .current_dir(&fixture.primary)
        .status()
        .unwrap();
    assert!(!branch_check.success());
    assert!(ready.result_commit_sha.is_some());
}

#[test]
fn recovery_bundle_contains_the_temporary_integration_result_commit() {
    let fixture = Fixture::new();
    fixture.checkpoint("source\n");
    fixture.detach_target();
    let plan = fixture
        .service
        .prepare_squash(PrepareSquash {
            task_id: TaskId::new("GAG-013-task"),
            commit_message: "fix(GAG-013): preserve integration result".into(),
        })
        .unwrap();
    let ready = fixture
        .service
        .start_squash(&plan.attempt_id, &plan.approval_digest)
        .unwrap();
    let result = ready.result_commit_sha.unwrap();
    let aborted = fixture.service.abort_integration(&plan.attempt_id).unwrap();
    let bundle = PathBuf::from(aborted.recovery_bundle_path.unwrap()).join("branch.bundle");
    let heads = git(
        &fixture.primary,
        &["bundle", "list-heads", bundle.to_str().unwrap()],
    );
    assert!(
        heads.contains(&result),
        "bundle heads did not contain {result}: {heads}"
    );
}

#[test]
fn repository_allows_only_one_uncleaned_integration_attempt() {
    let fixture = Fixture::new();
    fixture.checkpoint("source\n");
    fixture.detach_target();
    fixture
        .service
        .prepare_squash(PrepareSquash {
            task_id: TaskId::new("GAG-013-task"),
            commit_message: "fix(GAG-013): first active integration".into(),
        })
        .unwrap();
    let second = fixture
        .service
        .prepare_squash(PrepareSquash {
            task_id: TaskId::new("GAG-013-task"),
            commit_message: "fix(GAG-013): reject concurrent integration".into(),
        })
        .unwrap_err();
    assert_eq!(second.code, "INTEGRATION_LOCKED");
}

#[test]
fn repository_identity_lease_rejects_a_different_worktree_path() {
    let fixture = Fixture::new();
    fixture.checkpoint("source\n");
    fixture.detach_target();
    let plan = fixture
        .service
        .prepare_squash(PrepareSquash {
            task_id: TaskId::new("GAG-013-task"),
            commit_message: "fix(GAG-013): stable repository lease".into(),
        })
        .unwrap();
    let mut duplicate = fixture
        .repo
        .get_integration_attempt(&plan.attempt_id)
        .unwrap();
    duplicate.id = IntegrationId::new(uuid::Uuid::new_v4().to_string());
    duplicate.repo_root = fixture
        .managed
        .join("linked-alias")
        .to_string_lossy()
        .into_owned();

    assert!(fixture.repo.create_integration_attempt(&duplicate).is_err());
}

#[test]
fn legacy_attempt_without_identity_blocks_new_repository_leases_fail_closed() {
    let fixture = Fixture::new();
    fixture.checkpoint("source\n");
    fixture.detach_target();
    let plan = fixture
        .service
        .prepare_squash(PrepareSquash {
            task_id: TaskId::new("GAG-013-task"),
            commit_message: "fix(GAG-013): legacy lease fallback".into(),
        })
        .unwrap();
    let mut legacy = fixture
        .repo
        .get_integration_attempt(&plan.attempt_id)
        .unwrap();
    legacy.cleanup_status = "completed".into();
    fixture
        .repo
        .update_integration_attempt(&legacy, "{\"cleanup\":\"completed\"}")
        .unwrap();
    legacy.id = IntegrationId::new(uuid::Uuid::new_v4().to_string());
    legacy.repo_identity.clear();
    legacy.repo_root = "C:/legacy-linked-worktree".into();
    legacy.cleanup_status = "not_started".into();
    fixture.repo.create_integration_attempt(&legacy).unwrap();

    let blocked = fixture
        .repo
        .get_active_integration_by_repo("C:/different/common.git", "C:/different/root")
        .unwrap();
    assert_eq!(blocked.unwrap().id, legacy.id);
}

#[test]
fn rename_and_binary_changes_are_previewed_and_squashed() {
    let fixture = Fixture::new();
    std::fs::rename(
        fixture.task_worktree.join("shared.txt"),
        fixture.task_worktree.join("renamed.txt"),
    )
    .unwrap();
    std::fs::write(
        fixture.task_worktree.join("binary.dat"),
        [0_u8, 1, 2, 0, 255, 128],
    )
    .unwrap();
    fixture
        .commit_all_and_record_checkpoint("feat(GAG-013): rename and binary checkpoint [GAG-013]");
    fixture.detach_target();
    let plan = fixture
        .service
        .prepare_squash(PrepareSquash {
            task_id: TaskId::new("GAG-013-task"),
            commit_message: "feat(GAG-013): squash rename and binary".into(),
        })
        .unwrap();
    assert!(plan.expected_files.iter().any(|path| path == "renamed.txt"));
    assert!(plan.expected_files.iter().any(|path| path == "binary.dat"));
    let ready = fixture
        .service
        .start_squash(&plan.attempt_id, &plan.approval_digest)
        .unwrap();
    assert_eq!(ready.state, "ready_to_publish");
    let published = fixture
        .service
        .publish_integration(&plan.attempt_id, &plan.approval_digest)
        .unwrap();
    assert_eq!(published.state, "completed");
    fixture
        .service
        .cleanup_integration(&plan.attempt_id)
        .unwrap();
}

#[test]
fn target_advance_after_staging_rejects_publish_and_preserves_external_head() {
    let fixture = Fixture::new();
    fixture.checkpoint("source\n");
    fixture.detach_target();
    let plan = fixture
        .service
        .prepare_squash(PrepareSquash {
            task_id: TaskId::new("GAG-013-task"),
            commit_message: "fix(GAG-013): reject late target advance".into(),
        })
        .unwrap();
    fixture
        .service
        .start_squash(&plan.attempt_id, &plan.approval_digest)
        .unwrap();
    std::fs::write(fixture.primary.join("external.txt"), "external\n").unwrap();
    git(&fixture.primary, &["add", "--", "external.txt"]);
    git(
        &fixture.primary,
        &["commit", "-m", "external target advance"],
    );
    let external = git(&fixture.primary, &["rev-parse", "HEAD"]);
    git(
        &fixture.primary,
        &[
            "update-ref",
            &plan.target_ref,
            &external,
            &plan.expected_target_sha,
        ],
    );
    let error = fixture
        .service
        .publish_integration(&plan.attempt_id, &plan.approval_digest)
        .unwrap_err();
    assert_eq!(error.code, "INTEGRATION_TARGET_CHANGED");
    assert_eq!(
        git(&fixture.primary, &["rev-parse", &plan.target_ref]),
        external
    );
    assert_eq!(
        fixture
            .service
            .get_integration_status(&plan.attempt_id)
            .unwrap()
            .state,
        "publish_rejected"
    );
}

#[test]
fn commit_failure_is_persisted_without_updating_target() {
    let fixture = Fixture::new();
    fixture.checkpoint("source\n");
    fixture.detach_target();
    let plan = fixture
        .service
        .prepare_squash(PrepareSquash {
            task_id: TaskId::new("GAG-013-task"),
            commit_message: "fix(GAG-013): persist commit failure".into(),
        })
        .unwrap();
    let hook = PathBuf::from(git(
        &fixture.primary,
        &["rev-parse", "--git-path", "hooks/pre-commit"],
    ));
    let hook = if hook.is_absolute() {
        hook
    } else {
        fixture.primary.join(hook)
    };
    std::fs::write(hook, "#!/bin/sh\nexit 1\n").unwrap();
    let error = fixture
        .service
        .start_squash(&plan.attempt_id, &plan.approval_digest)
        .unwrap_err();
    assert_eq!(error.code, "GIT_COMMAND_FAILED");
    let failed = fixture
        .service
        .get_integration_status(&plan.attempt_id)
        .unwrap();
    assert_eq!(failed.state, "validation_failed");
    assert_eq!(
        git(&fixture.primary, &["rev-parse", &plan.target_ref]),
        plan.expected_target_sha
    );
}

#[test]
fn successful_commit_hook_cannot_inject_files_outside_the_approved_preview() {
    let fixture = Fixture::new();
    fixture.checkpoint("source\n");
    fixture.detach_target();
    let plan = fixture
        .service
        .prepare_squash(PrepareSquash {
            task_id: TaskId::new("GAG-013-task"),
            commit_message: "fix(GAG-013): reject hook injection".into(),
        })
        .unwrap();
    let hook = PathBuf::from(git(
        &fixture.primary,
        &["rev-parse", "--git-path", "hooks/pre-commit"],
    ));
    let hook = if hook.is_absolute() {
        hook
    } else {
        fixture.primary.join(hook)
    };
    std::fs::write(
        hook,
        "#!/bin/sh\nprintf 'injected\\n' > injected.txt\ngit add -- injected.txt\nexit 0\n",
    )
    .unwrap();

    let error = fixture
        .service
        .start_squash(&plan.attempt_id, &plan.approval_digest)
        .unwrap_err();
    assert_eq!(error.code, "INTEGRATION_COMMIT_CHANGED");
    let failed = fixture
        .service
        .get_integration_status(&plan.attempt_id)
        .unwrap();
    assert_eq!(failed.state, IntegrationState::ValidationFailed);
    assert!(failed.result_commit_sha.is_some());
    assert_eq!(
        git(&fixture.primary, &["rev-parse", &plan.target_ref]),
        plan.expected_target_sha
    );
}

#[test]
fn cleanup_rejects_a_persisted_path_escape() {
    let fixture = Fixture::new();
    fixture.checkpoint("source\n");
    fixture.detach_target();
    let plan = fixture
        .service
        .prepare_squash(PrepareSquash {
            task_id: TaskId::new("GAG-013-task"),
            commit_message: "fix(GAG-013): reject cleanup path escape".into(),
        })
        .unwrap();
    fixture
        .service
        .start_squash(&plan.attempt_id, &plan.approval_digest)
        .unwrap();
    let mut aborted = fixture.service.abort_integration(&plan.attempt_id).unwrap();
    aborted.temporary_worktree_path = Some(fixture.primary.to_string_lossy().into_owned());
    fixture
        .repo
        .update_integration_attempt(&aborted, "{\"fault\":\"path_escape\"}")
        .unwrap();
    let error = fixture
        .service
        .cleanup_integration(&plan.attempt_id)
        .unwrap_err();
    assert_eq!(error.code, "INTEGRATION_CLEANUP_REJECTED");
}

#[test]
fn cleanup_releases_a_staging_lease_when_planned_resources_were_never_created() {
    let fixture = Fixture::new();
    fixture.checkpoint("source\n");
    fixture.detach_target();
    let plan = fixture
        .service
        .prepare_squash(PrepareSquash {
            task_id: TaskId::new("GAG-013-task"),
            commit_message: "fix(GAG-013): recover pre-create crash".into(),
        })
        .unwrap();
    let mut attempt = fixture
        .repo
        .get_integration_attempt(&plan.attempt_id)
        .unwrap();
    attempt.state = IntegrationState::Staging;
    attempt.temporary_worktree_id = Some("integration-planned".into());
    attempt.temporary_worktree_path = Some(
        fixture
            .managed
            .join("integration-planned")
            .to_string_lossy()
            .into_owned(),
    );
    attempt.temporary_branch = Some("gag-integration/planned".into());
    fixture
        .repo
        .update_integration_attempt(&attempt, "{\"fault\":\"before_worktree_add\"}")
        .unwrap();

    let aborted = fixture.service.abort_integration(&plan.attempt_id).unwrap();
    assert!(aborted.recovery_bundle_path.is_none());
    let cleaned = fixture
        .service
        .cleanup_integration(&plan.attempt_id)
        .unwrap();
    assert_eq!(cleaned.cleanup_status, "completed");
}

#[test]
fn submodule_add_add_conflict_remains_in_the_temporary_worktree() {
    let fixture = Fixture::new();
    let submodule = fixture.recovery.join("submodule-source");
    std::fs::create_dir_all(&submodule).unwrap();
    git(&submodule, &["init", "-b", "main"]);
    git(&submodule, &["config", "user.name", "GAG 013"]);
    git(
        &submodule,
        &["config", "user.email", "gag013@example.invalid"],
    );
    std::fs::write(submodule.join("module.txt"), "module\n").unwrap();
    git(&submodule, &["add", "--", "module.txt"]);
    git(&submodule, &["commit", "-m", "submodule fixture"]);
    let before = git(&fixture.task_worktree, &["rev-parse", "HEAD"]);
    git(
        &fixture.task_worktree,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            submodule.to_str().unwrap(),
            "vendor/sub",
        ],
    );
    git(
        &fixture.task_worktree,
        &["commit", "-m", "feat(GAG-013): add submodule"],
    );
    fixture.record_current_head(&before, "feat(GAG-013): add submodule");

    std::fs::create_dir_all(fixture.primary.join("vendor")).unwrap();
    std::fs::write(fixture.primary.join("vendor/sub"), "regular target file\n").unwrap();
    git(&fixture.primary, &["add", "--", "vendor/sub"]);
    git(&fixture.primary, &["commit", "-m", "target regular file"]);
    fixture.detach_target();
    let target = git(&fixture.primary, &["rev-parse", "refs/heads/main"]);
    let plan = fixture
        .service
        .prepare_squash(PrepareSquash {
            task_id: TaskId::new("GAG-013-task"),
            commit_message: "fix(GAG-013): isolate submodule conflict".into(),
        })
        .unwrap();
    let conflicted = fixture
        .service
        .start_squash(&plan.attempt_id, &plan.approval_digest)
        .unwrap();
    assert_eq!(conflicted.state, "conflicted");
    assert_eq!(
        git(&fixture.primary, &["rev-parse", "refs/heads/main"]),
        target
    );
}

#[test]
fn incompatible_file_modes_conflict_without_touching_the_target_ref() {
    let fixture = Fixture::new();
    let before = git(&fixture.task_worktree, &["rev-parse", "HEAD"]);
    let link_target = fixture.task_worktree.join("link-target.txt");
    std::fs::write(&link_target, "renamed.txt\n").unwrap();
    let blob = git(
        &fixture.task_worktree,
        &["hash-object", "-w", link_target.to_str().unwrap()],
    );
    git(
        &fixture.task_worktree,
        &[
            "update-index",
            "--cacheinfo",
            &format!("120000,{blob},shared.txt"),
        ],
    );
    git(
        &fixture.task_worktree,
        &["commit", "-m", "feat(GAG-013): source symlink mode"],
    );
    fixture.record_current_head(&before, "feat(GAG-013): source symlink mode");

    git(
        &fixture.primary,
        &["update-index", "--chmod=+x", "shared.txt"],
    );
    git(
        &fixture.primary,
        &["commit", "-m", "target executable mode"],
    );
    fixture.detach_target();
    let target = git(&fixture.primary, &["rev-parse", "refs/heads/main"]);
    let plan = fixture
        .service
        .prepare_squash(PrepareSquash {
            task_id: TaskId::new("GAG-013-task"),
            commit_message: "fix(GAG-013): isolate mode conflict".into(),
        })
        .unwrap();
    let conflicted = fixture
        .service
        .start_squash(&plan.attempt_id, &plan.approval_digest)
        .unwrap();
    assert_eq!(conflicted.state, "conflicted");
    assert_eq!(
        git(&fixture.primary, &["rev-parse", "refs/heads/main"]),
        target
    );
}
