use grok_acp_gui_lib::adapters::sqlite::SqliteRepository;
use grok_acp_gui_lib::domain::types::{
    Project, ProjectId, Task, TaskId, TaskStatus, WorkspaceKind,
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
        let service = ManagedWorkspaceService::new(repo, managed.clone(), recovery.clone());
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
