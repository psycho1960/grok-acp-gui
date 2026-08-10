use grok_acp_gui_lib::adapters::sqlite::SqliteRepository;
use grok_acp_gui_lib::domain::types::{
    utc_now, IntegrationAttempt, IntegrationId, IntegrationState, Project, ProjectId,
    RecoveryState, SessionBinding, SessionId, SessionState, Task, TaskId, TaskStatus,
    WorkspaceKind, WorktreeId, WorktreeOwnership, WorktreeRecord, WorktreeState,
};
use grok_acp_gui_lib::modules::artifacts::{ArtifactService, ManagedArtifactService};
use grok_acp_gui_lib::modules::persistence::Repository;
use grok_acp_gui_lib::modules::task_runtime::recovery::{
    ManagedRecoveryService, RecoveryActionKind, RecoveryIssueKind, RecoveryIssueStatus,
    RecoveryService,
};
use grok_acp_gui_lib::modules::workspace::{
    CreateManagedWorktree, ManagedWorkspaceService, WorkspaceService,
};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

fn base_fixture(label: &str, status: TaskStatus) -> (PathBuf, Arc<dyn Repository>) {
    let root = std::env::temp_dir().join(format!("gag-014-{label}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let repo: Arc<dyn Repository> = Arc::new(SqliteRepository::open_in_memory().unwrap());
    let now = utc_now();
    repo.create_project(&Project {
        id: ProjectId::new(format!("project-{label}")),
        path: root.to_string_lossy().into_owned(),
        display_path: label.into(),
        repo_root: None,
        trusted_at: Some(now.clone()),
        last_opened_at: now.clone(),
    })
    .unwrap();
    repo.create_task(&Task {
        id: TaskId::new(format!("task-{label}")),
        project_id: ProjectId::new(format!("project-{label}")),
        title: label.into(),
        status,
        workspace_kind: WorkspaceKind::Direct,
        mode: Some("ask".into()),
        model: None,
        reasoning: None,
        created_at: now.clone(),
        updated_at: now,
        interrupt_reason: Some("fixture interruption".into()),
        interrupted_at: Some(utc_now()),
        attempt_count: 1,
    })
    .unwrap();
    (root, repo)
}

fn recovery_service(root: &Path, repo: Arc<dyn Repository>) -> ManagedRecoveryService {
    let workspace: Arc<dyn WorkspaceService> = Arc::new(ManagedWorkspaceService::new(
        repo.clone(),
        root.join("managed-worktrees"),
        root.join("recovery"),
    ));
    let artifacts: Arc<dyn ArtifactService> = Arc::new(ManagedArtifactService::new());
    ManagedRecoveryService::new(repo, workspace, artifacts)
}

#[test]
fn repeated_scan_deduplicates_identity_and_appends_evidence_revisions() {
    let (root, repo) = base_fixture("revision", TaskStatus::Interrupted);
    let service = recovery_service(&root, repo.clone());
    let first = service.scan("manual").unwrap();
    let second = service.scan("manual").unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1);
    assert_eq!(first[0].issue_id, second[0].issue_id);
    assert_eq!(first[0].revision, 1);
    assert_eq!(second[0].revision, 2);
    let history = service.list_history().unwrap();
    assert_eq!(history.scans.len(), 2);
    assert_eq!(history.issues.len(), 2);
    assert!(history
        .issues
        .iter()
        .all(|issue| issue.kind == RecoveryIssueKind::InterruptedTask));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn retain_plan_uses_revision_and_stale_plan_is_rejected_after_rescan() {
    let (root, repo) = base_fixture("stale", TaskStatus::Interrupted);
    let service = recovery_service(&root, repo);
    let issue = service.scan("manual").unwrap().remove(0);
    let plan = service
        .prepare_action(&issue.issue_id, issue.revision, RecoveryActionKind::Retain)
        .unwrap();
    let rescanned = service.scan("manual").unwrap();
    assert!(rescanned
        .iter()
        .any(|item| item.kind == RecoveryIssueKind::PersistenceMarker));
    let error = service
        .execute_action(&plan.id, &plan.approval_digest)
        .unwrap_err();
    assert_eq!(error.code, "RECOVERY_PLAN_STALE");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn disconnected_session_and_unfinished_integration_have_only_declared_actions() {
    let (root, repo) = base_fixture("orphan", TaskStatus::Idle);
    repo.create_binding(&SessionBinding {
        task_id: TaskId::new("task-orphan"),
        session_id: SessionId::new("session-orphan"),
        cwd: Some(root.to_string_lossy().into_owned()),
        last_seq: 9,
        state: SessionState::Disconnected,
        attempt_number: 2,
    })
    .unwrap();
    let now = utc_now();
    repo.create_integration_attempt(&IntegrationAttempt {
        id: IntegrationId::new("integration-orphan"),
        task_id: TaskId::new("task-orphan"),
        repo_root: root.to_string_lossy().into_owned(),
        repo_identity: "repo-orphan".into(),
        source_ref: "refs/heads/source".into(),
        source_tip_sha: "b".repeat(40),
        source_range: "[]".into(),
        source_dirty: false,
        source_worktree_digest: "source-digest".into(),
        target_ref: "refs/heads/main".into(),
        expected_target_sha: "a".repeat(40),
        commit_message: "fix(GAG-014): fixture".into(),
        validation_commands_json: "[]".into(),
        validation_digest: "validation".into(),
        approval_digest: "approval".into(),
        state: IntegrationState::Staging,
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
    })
    .unwrap();
    let issues = recovery_service(&root, repo).scan("manual").unwrap();
    let session = issues
        .iter()
        .find(|item| item.kind == RecoveryIssueKind::OrphanedSession)
        .unwrap();
    assert!(session
        .safe_actions
        .contains(&RecoveryActionKind::MarkInterrupted));
    assert!(!session
        .safe_actions
        .contains(&RecoveryActionKind::VerifyAndCleanup));
    let integration = issues
        .iter()
        .find(|item| item.kind == RecoveryIssueKind::TemporaryIntegration)
        .unwrap();
    assert!(integration
        .safe_actions
        .contains(&RecoveryActionKind::ContinueIntegration));
    assert!(integration
        .safe_actions
        .contains(&RecoveryActionKind::AbortIntegration));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn worktree_path_outside_managed_root_is_display_and_retain_only() {
    let (root, repo) = base_fixture("outside", TaskStatus::Idle);
    let outside = root.join("outside-worktree");
    std::fs::create_dir_all(&outside).unwrap();
    repo.create_worktree(&WorktreeRecord {
        id: WorktreeId::new("wt-outside"),
        task_id: TaskId::new("task-outside"),
        repo_root: root.to_string_lossy().into_owned(),
        path: outside.to_string_lossy().into_owned(),
        display_path: outside.to_string_lossy().into_owned(),
        branch: "grok/outside".into(),
        base_branch: "main".into(),
        base_commit: "a".repeat(40),
        ownership: WorktreeOwnership::Managed,
        state: WorktreeState::Ready,
        repo_identity: "repo-outside".into(),
        common_git_dir: root.join(".git").to_string_lossy().into_owned(),
        relative_path: "../outside-worktree".into(),
        created_at: utc_now(),
        last_verified_at: utc_now(),
        recovery_bundle_id: None,
        disk_usage_bytes: 0,
        locked: false,
        merged: false,
    })
    .unwrap();
    let issue = recovery_service(&root, repo)
        .scan("manual")
        .unwrap()
        .into_iter()
        .find(|item| item.resource_id == "wt-outside")
        .unwrap();
    assert_eq!(issue.kind, RecoveryIssueKind::WorktreeMismatch);
    assert_eq!(issue.safe_actions, vec![RecoveryActionKind::Retain]);
    assert_eq!(
        issue.canonical_path.as_deref(),
        Some(outside.to_string_lossy().as_ref())
    );
    assert_eq!(
        issue.evidence["diagnosticError"],
        "WORKTREE_OUTSIDE_MANAGED_ROOT"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn temporary_artifact_cleanup_revalidates_digest_and_removes_only_exact_file() {
    let (root, repo) = base_fixture("artifact", TaskStatus::Idle);
    repo.create_binding(&SessionBinding {
        task_id: TaskId::new("task-artifact"),
        session_id: SessionId::new("session-artifact"),
        cwd: Some(root.to_string_lossy().into_owned()),
        last_seq: 0,
        state: SessionState::Idle,
        attempt_number: 1,
    })
    .unwrap();
    let cache = root.join(".grok-acp-gui").join("artifacts").join("aa");
    std::fs::create_dir_all(&cache).unwrap();
    let temporary = cache.join(".import-fixture.tmp");
    let neighbour = cache.join("keep.png");
    std::fs::write(&temporary, b"partial import").unwrap();
    std::fs::write(&neighbour, b"keep").unwrap();
    let service = recovery_service(&root, repo);
    let issue = service
        .scan("manual")
        .unwrap()
        .into_iter()
        .find(|issue| issue.kind == RecoveryIssueKind::ArtifactTemporaryFile)
        .unwrap();
    let plan = service
        .prepare_action(
            &issue.issue_id,
            issue.revision,
            RecoveryActionKind::VerifyAndCleanup,
        )
        .unwrap();
    std::fs::write(&temporary, b"changed after plan").unwrap();
    assert!(service
        .execute_action(&plan.id, &plan.approval_digest)
        .is_err());
    assert!(temporary.exists());
    assert!(neighbour.exists());

    let fresh = service
        .scan("manual")
        .unwrap()
        .into_iter()
        .find(|issue| issue.kind == RecoveryIssueKind::ArtifactTemporaryFile)
        .unwrap();
    let plan = service
        .prepare_action(
            &fresh.issue_id,
            fresh.revision,
            RecoveryActionKind::VerifyAndCleanup,
        )
        .unwrap();
    let resolved = service
        .execute_action(&plan.id, &plan.approval_digest)
        .unwrap();
    assert_eq!(resolved.status, RecoveryIssueStatus::Resolved);
    assert!(!temporary.exists());
    assert!(neighbour.exists());
    let _ = std::fs::remove_dir_all(root);
}

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
    String::from_utf8_lossy(&output.stdout).trim().into()
}

#[test]
fn damaged_bundle_blocks_managed_worktree_cleanup() {
    let root = std::env::temp_dir().join(format!("gag-014-bundle-{}", uuid::Uuid::new_v4()));
    let primary = root.join("primary");
    std::fs::create_dir_all(&primary).unwrap();
    git(&primary, &["init", "-b", "main"]);
    git(&primary, &["config", "user.name", "GAG 014"]);
    git(
        &primary,
        &["config", "user.email", "gag014@example.invalid"],
    );
    std::fs::write(primary.join("base.txt"), "base\n").unwrap();
    git(&primary, &["add", "--", "base.txt"]);
    git(&primary, &["commit", "-m", "fixture"]);

    let repo: Arc<dyn Repository> = Arc::new(SqliteRepository::open_in_memory().unwrap());
    let now = utc_now();
    repo.create_project(&Project {
        id: ProjectId::new("project-bundle"),
        path: primary.to_string_lossy().into_owned(),
        display_path: "bundle".into(),
        repo_root: Some(primary.to_string_lossy().into_owned()),
        trusted_at: Some(now.clone()),
        last_opened_at: now.clone(),
    })
    .unwrap();
    repo.create_task(&Task {
        id: TaskId::new("task-bundle"),
        project_id: ProjectId::new("project-bundle"),
        title: "bundle".into(),
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
    let workspace: Arc<dyn WorkspaceService> = Arc::new(ManagedWorkspaceService::new(
        repo.clone(),
        root.join("managed"),
        root.join("recovery"),
    ));
    let record = workspace
        .create_managed_worktree(CreateManagedWorktree {
            repo_root: primary.clone(),
            task_id: TaskId::new("task-bundle"),
            task_slug: "bundle".into(),
            base_ref: "main".into(),
        })
        .unwrap();
    let worktree = PathBuf::from(&record.path);
    std::fs::write(worktree.join("dirty.txt"), "must survive\n").unwrap();
    let mut closing = record.clone();
    closing.state = WorktreeState::Closing;
    repo.update_worktree(&closing).unwrap();
    let artifacts: Arc<dyn ArtifactService> = Arc::new(ManagedArtifactService::new());
    let service = ManagedRecoveryService::new(repo.clone(), workspace.clone(), artifacts);
    let issue = service
        .scan("manual")
        .unwrap()
        .into_iter()
        .find(|issue| {
            issue.kind == RecoveryIssueKind::WorktreeMismatch && issue.resource_id == record.id.0
        })
        .unwrap();
    assert!(issue.safe_actions.contains(&RecoveryActionKind::Reregister));
    let reregister = service
        .prepare_action(
            &issue.issue_id,
            issue.revision,
            RecoveryActionKind::Reregister,
        )
        .unwrap();
    let reconciled = service
        .execute_action(&reregister.id, &reregister.approval_digest)
        .unwrap();
    assert_eq!(reconciled.status, RecoveryIssueStatus::Resolved);
    let mut closing = repo
        .list_worktrees_by_task("task-bundle")
        .unwrap()
        .remove(0);
    closing.state = WorktreeState::Closing;
    repo.update_worktree(&closing).unwrap();
    let issue = service
        .scan("manual")
        .unwrap()
        .into_iter()
        .find(|issue| {
            issue.kind == RecoveryIssueKind::WorktreeMismatch && issue.resource_id == record.id.0
        })
        .unwrap();
    let plan = service
        .prepare_action(
            &issue.issue_id,
            issue.revision,
            RecoveryActionKind::VerifyAndCleanup,
        )
        .unwrap();
    let bundle = service
        .list_history()
        .unwrap()
        .bundles
        .into_iter()
        .next()
        .unwrap();
    std::fs::write(&bundle.manifest_path, b"tampered").unwrap();
    assert!(service
        .execute_action(&plan.id, &plan.approval_digest)
        .is_err());
    assert!(worktree.exists());
    assert_eq!(
        git(&primary, &["worktree", "list", "--porcelain"])
            .matches("worktree ")
            .count(),
        2
    );

    let _ = Command::new("git")
        .args(["worktree", "remove", "--force", &record.path])
        .current_dir(&primary)
        .output();
    let _ = Command::new("git")
        .args(["branch", "-D", &record.branch])
        .current_dir(&primary)
        .output();
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn verified_bundle_restores_all_dirty_content_and_can_then_be_deleted() {
    let root = std::env::temp_dir().join(format!("gag-014-restore-{}", uuid::Uuid::new_v4()));
    let primary = root.join("primary");
    std::fs::create_dir_all(&primary).unwrap();
    git(&primary, &["init", "-b", "main"]);
    git(&primary, &["config", "user.name", "GAG 014"]);
    git(
        &primary,
        &["config", "user.email", "gag014@example.invalid"],
    );
    std::fs::write(primary.join("base.txt"), "base\n").unwrap();
    git(&primary, &["add", "--", "base.txt"]);
    git(&primary, &["commit", "-m", "fixture"]);

    let repo: Arc<dyn Repository> = Arc::new(SqliteRepository::open_in_memory().unwrap());
    let now = utc_now();
    repo.create_project(&Project {
        id: ProjectId::new("project-restore"),
        path: primary.to_string_lossy().into_owned(),
        display_path: "restore".into(),
        repo_root: Some(primary.to_string_lossy().into_owned()),
        trusted_at: Some(now.clone()),
        last_opened_at: now.clone(),
    })
    .unwrap();
    repo.create_task(&Task {
        id: TaskId::new("task-restore"),
        project_id: ProjectId::new("project-restore"),
        title: "restore".into(),
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
    let workspace: Arc<dyn WorkspaceService> = Arc::new(ManagedWorkspaceService::new(
        repo.clone(),
        root.join("managed"),
        root.join("recovery"),
    ));
    let record = workspace
        .create_managed_worktree(CreateManagedWorktree {
            repo_root: primary.clone(),
            task_id: TaskId::new("task-restore"),
            task_slug: "restore".into(),
            base_ref: "main".into(),
        })
        .unwrap();
    let worktree = PathBuf::from(&record.path);
    std::fs::write(worktree.join("base.txt"), "unstaged change\n").unwrap();
    std::fs::write(worktree.join("staged.txt"), "staged change\n").unwrap();
    git(&worktree, &["add", "--", "staged.txt"]);
    std::fs::write(worktree.join("untracked.txt"), "untracked change\n").unwrap();
    let mut closing = record.clone();
    closing.state = WorktreeState::Closing;
    repo.update_worktree(&closing).unwrap();

    let artifacts: Arc<dyn ArtifactService> = Arc::new(ManagedArtifactService::new());
    let service = ManagedRecoveryService::new(repo.clone(), workspace.clone(), artifacts);
    let worktree_issue = service
        .scan("manual")
        .unwrap()
        .into_iter()
        .find(|issue| {
            issue.kind == RecoveryIssueKind::WorktreeMismatch && issue.resource_id == record.id.0
        })
        .unwrap();
    let cleanup = service
        .prepare_action(
            &worktree_issue.issue_id,
            worktree_issue.revision,
            RecoveryActionKind::VerifyAndCleanup,
        )
        .unwrap();
    let bundle = service.list_history().unwrap().bundles.remove(0);
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&bundle.manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["version"], 2);
    assert_eq!(manifest["applicationVersion"], env!("CARGO_PKG_VERSION"));
    assert_eq!(manifest["taskId"], "task-restore");
    assert_eq!(manifest["worktreeId"], record.id.0);
    assert_eq!(manifest["databaseSummary"]["worktreeId"], record.id.0);
    assert_eq!(
        manifest["databaseSummarySha256"].as_str().unwrap().len(),
        64
    );
    assert!(manifest["git"]["headSha"]
        .as_str()
        .is_some_and(|value| value.len() == 40));
    assert_eq!(manifest["untracked"]["skipped"], serde_json::json!([]));
    assert!(manifest["fileSizes"]["branch.bundle"].as_u64().unwrap() > 0);
    assert!(manifest["files"]["branch.bundle"]
        .as_str()
        .is_some_and(|value| value.len() == 64));
    let removed = service
        .execute_action(&cleanup.id, &cleanup.approval_digest)
        .unwrap();
    assert_eq!(removed.status, RecoveryIssueStatus::Resolved);
    assert!(!worktree.exists());
    assert_eq!(
        repo.get_recovery_item(&bundle.recovery_item_id)
            .unwrap()
            .state,
        RecoveryState::Available
    );

    let bundle_issue = service
        .scan("manual")
        .unwrap()
        .into_iter()
        .find(|issue| {
            issue.kind == RecoveryIssueKind::RecoveryBundle
                && issue.resource_id == bundle.recovery_item_id
        })
        .unwrap();
    let restore = service
        .prepare_action(
            &bundle_issue.issue_id,
            bundle_issue.revision,
            RecoveryActionKind::RestoreBundle,
        )
        .unwrap();
    let restored = service
        .execute_action(&restore.id, &restore.approval_digest)
        .unwrap();
    assert_eq!(restored.status, RecoveryIssueStatus::Resolved);
    assert_eq!(
        std::fs::read_to_string(worktree.join("base.txt"))
            .unwrap()
            .replace("\r\n", "\n"),
        "unstaged change\n"
    );
    assert_eq!(
        std::fs::read_to_string(worktree.join("staged.txt"))
            .unwrap()
            .replace("\r\n", "\n"),
        "staged change\n"
    );
    assert_eq!(
        std::fs::read_to_string(worktree.join("untracked.txt"))
            .unwrap()
            .replace("\r\n", "\n"),
        "untracked change\n"
    );
    assert_eq!(git(&worktree, &["diff", "--name-only"]), "base.txt");
    assert_eq!(
        git(&worktree, &["diff", "--cached", "--name-only"]),
        "staged.txt"
    );
    assert_eq!(
        repo.get_recovery_item(&bundle.recovery_item_id)
            .unwrap()
            .state,
        RecoveryState::Restored
    );

    let restored_bundle_issue = service
        .scan("manual")
        .unwrap()
        .into_iter()
        .find(|issue| {
            issue.kind == RecoveryIssueKind::RecoveryBundle
                && issue.resource_id == bundle.recovery_item_id
        })
        .unwrap();
    let delete = service
        .prepare_action(
            &restored_bundle_issue.issue_id,
            restored_bundle_issue.revision,
            RecoveryActionKind::DeleteBundle,
        )
        .unwrap();
    service
        .execute_action(&delete.id, &delete.approval_digest)
        .unwrap();
    assert!(!Path::new(&bundle.manifest_path).exists());
    assert_eq!(
        repo.get_recovery_item(&bundle.recovery_item_id)
            .unwrap()
            .state,
        RecoveryState::Deleted
    );

    let _ = Command::new("git")
        .args(["worktree", "remove", "--force", &record.path])
        .current_dir(&primary)
        .output();
    let _ = Command::new("git")
        .args(["branch", "-D", &record.branch])
        .current_dir(&primary)
        .output();
    let _ = std::fs::remove_dir_all(root);
}
