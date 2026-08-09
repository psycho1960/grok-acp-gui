use grok_acp_gui_lib::adapters::git_cli::GitCli;
use grok_acp_gui_lib::adapters::grok_acp::{FakeAcpTransport, FakeScenario};
use grok_acp_gui_lib::adapters::sqlite::SqliteRepository;
use grok_acp_gui_lib::bridge::dispatch::{execute_impl_with_services, DesktopResult};
use grok_acp_gui_lib::domain::types::{
    Project, ProjectId, Task, TaskId, TaskStatus, WorkspaceKind,
};
use grok_acp_gui_lib::modules::agent_runtime::{AgentRuntime, AgentRuntimeImpl};
use grok_acp_gui_lib::modules::artifacts::ManagedArtifactService;
use grok_acp_gui_lib::modules::persistence::Repository;
use grok_acp_gui_lib::modules::task_runtime::TaskRuntimeImpl;
use grok_acp_gui_lib::modules::workspace::{
    CreateManagedWorktree, ManagedWorkspaceService, WorkspaceService,
};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

fn git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("git fixture command should start");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn fake_agent_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("tests")
        .join("fake-acp-agent")
        .join("agent.mjs")
}

#[test]
fn repository_identity_and_porcelain_worktrees_are_structured() {
    let fixture = std::env::temp_dir().join(format!("gag-011 repo 空格-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&fixture).unwrap();
    git(&fixture, &["init", "-b", "main"]);
    git(&fixture, &["config", "user.name", "GAG 011"]);
    git(
        &fixture,
        &["config", "user.email", "gag011@example.invalid"],
    );
    std::fs::write(fixture.join("README.md"), "fixture\n").unwrap();
    git(&fixture, &["add", "--", "README.md"]);
    git(&fixture, &["commit", "-m", "fixture"]);

    let linked = fixture
        .parent()
        .unwrap()
        .join(format!("gag-011 linked 中文-{}", uuid::Uuid::new_v4()));
    git(
        &fixture,
        &[
            "worktree",
            "add",
            "-b",
            "gag/GAG-011-fixture",
            linked.to_str().unwrap(),
            "HEAD",
        ],
    );

    let adapter = GitCli::default();
    let repository = adapter.inspect_repository(&fixture).unwrap();
    assert_eq!(repository.branch.as_deref(), Some("main"));
    assert!(!repository.dirty);
    assert_eq!(
        repository.canonical_root,
        std::fs::canonicalize(&fixture).unwrap()
    );
    assert!(repository.common_git_dir.is_absolute());

    let worktrees = adapter.list_worktrees(&fixture).unwrap();
    assert_eq!(worktrees.len(), 2);
    let managed_candidate = worktrees
        .iter()
        .find(|item| item.path == std::fs::canonicalize(&linked).unwrap())
        .expect("linked worktree should be parsed from porcelain output");
    assert_eq!(
        managed_candidate.branch.as_deref(),
        Some("gag/GAG-011-fixture")
    );
    assert!(!managed_candidate.locked);
    assert!(!managed_candidate.prunable);

    git(&fixture, &["worktree", "remove", linked.to_str().unwrap()]);
    std::fs::remove_dir_all(&fixture).unwrap();
}

#[test]
fn managed_worktree_creation_is_idempotent_and_registered() {
    let fixture = std::env::temp_dir().join(format!("gag-011-create-{}", uuid::Uuid::new_v4()));
    let managed_root =
        std::env::temp_dir().join(format!("gag-011-managed-{}", uuid::Uuid::new_v4()));
    let recovery_root =
        std::env::temp_dir().join(format!("gag-011-recovery-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&fixture).unwrap();
    git(&fixture, &["init", "-b", "main"]);
    git(&fixture, &["config", "user.name", "GAG 011"]);
    git(
        &fixture,
        &["config", "user.email", "gag011@example.invalid"],
    );
    std::fs::write(fixture.join("README.md"), "fixture\n").unwrap();
    git(&fixture, &["add", "--", "README.md"]);
    git(&fixture, &["commit", "-m", "fixture"]);

    let repo: Arc<dyn Repository> = Arc::new(SqliteRepository::open_in_memory().unwrap());
    let now = grok_acp_gui_lib::domain::types::utc_now();
    repo.create_project(&Project {
        id: ProjectId::new("project-gag-011"),
        path: fixture.to_string_lossy().into_owned(),
        display_path: "fixture".into(),
        repo_root: Some(fixture.to_string_lossy().into_owned()),
        trusted_at: Some(now.clone()),
        last_opened_at: now.clone(),
    })
    .unwrap();
    repo.create_task(&Task {
        id: TaskId::new("GAG-011-task"),
        project_id: ProjectId::new("project-gag-011"),
        title: "worktree lifecycle".into(),
        status: TaskStatus::Preparing,
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

    let service = ManagedWorkspaceService::new(repo.clone(), managed_root.clone(), recovery_root);
    let unrelated =
        std::env::temp_dir().join(format!("gag-011-unrelated-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&unrelated).unwrap();
    git(&unrelated, &["init", "-b", "main"]);
    git(&unrelated, &["config", "user.name", "GAG 011"]);
    git(
        &unrelated,
        &["config", "user.email", "gag011@example.invalid"],
    );
    std::fs::write(unrelated.join("README.md"), "unrelated\n").unwrap();
    git(&unrelated, &["add", "--", "README.md"]);
    git(&unrelated, &["commit", "-m", "fixture"]);
    assert!(service
        .create_managed_worktree(CreateManagedWorktree {
            repo_root: unrelated.clone(),
            task_id: TaskId::new("GAG-011-task"),
            task_slug: "renderer-controlled-slug".into(),
            base_ref: "main".into(),
        })
        .is_err());
    let request = CreateManagedWorktree {
        repo_root: fixture.clone(),
        task_id: TaskId::new("GAG-011-task"),
        task_slug: "worktree lifecycle".into(),
        base_ref: "main".into(),
    };
    let first = service.create_managed_worktree(request.clone()).unwrap();
    let second = service.create_managed_worktree(request).unwrap();
    assert_eq!(first.id, second.id);
    assert_eq!(first.branch, "gag/GAG-011-task-worktree-lifecycle");
    assert!(std::fs::canonicalize(&first.path)
        .unwrap()
        .starts_with(std::fs::canonicalize(&managed_root).unwrap()));
    assert_eq!(
        repo.list_worktrees_by_task("GAG-011-task").unwrap().len(),
        1
    );
    repo.delete_worktree(&first.id.0).unwrap();
    let repaired = service
        .create_managed_worktree(CreateManagedWorktree {
            repo_root: fixture.clone(),
            task_id: TaskId::new("GAG-011-task"),
            task_slug: "ignored renderer slug".into(),
            base_ref: "untrusted-ref".into(),
        })
        .unwrap();
    assert_ne!(repaired.id, first.id);
    assert_eq!(repaired.path, first.path);
    assert_eq!(
        repo.list_worktrees_by_task("GAG-011-task").unwrap().len(),
        1
    );

    git(&fixture, &["worktree", "remove", "--force", &first.path]);
    git(&fixture, &["branch", "-D", &first.branch]);
    std::fs::remove_dir_all(&fixture).unwrap();
    std::fs::remove_dir_all(&managed_root).unwrap();
    std::fs::remove_dir_all(&unrelated).unwrap();
}

#[test]
fn managed_target_rejects_root_and_same_prefix_escape() {
    use grok_acp_gui_lib::adapters::filesystem::validate_managed_worktree_target;

    let parent = std::env::temp_dir().join(format!("gag-011-containment-{}", uuid::Uuid::new_v4()));
    let managed = parent.join("managed");
    let sibling = parent.join("managed-escape");
    std::fs::create_dir_all(&managed).unwrap();
    std::fs::create_dir_all(&sibling).unwrap();

    assert!(validate_managed_worktree_target(&managed, &managed).is_err());
    assert!(validate_managed_worktree_target(&managed, &sibling).is_err());
    assert!(validate_managed_worktree_target(&managed, &managed.join("repo/task")).is_ok());

    std::fs::remove_dir_all(&parent).unwrap();
}

#[test]
fn dirty_removal_requires_verified_recovery_and_exact_path_confirmation() {
    let fixture = std::env::temp_dir().join(format!("gag-011-remove-{}", uuid::Uuid::new_v4()));
    let managed_root =
        std::env::temp_dir().join(format!("gag-011-managed-{}", uuid::Uuid::new_v4()));
    let recovery_root =
        std::env::temp_dir().join(format!("gag-011-recovery-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&fixture).unwrap();
    git(&fixture, &["init", "-b", "main"]);
    git(&fixture, &["config", "user.name", "GAG 011"]);
    git(
        &fixture,
        &["config", "user.email", "gag011@example.invalid"],
    );
    std::fs::write(fixture.join("tracked.txt"), "base\n").unwrap();
    git(&fixture, &["add", "--", "tracked.txt"]);
    git(&fixture, &["commit", "-m", "fixture"]);

    let repo: Arc<dyn Repository> = Arc::new(SqliteRepository::open_in_memory().unwrap());
    let now = grok_acp_gui_lib::domain::types::utc_now();
    repo.create_project(&Project {
        id: ProjectId::new("project-remove"),
        path: fixture.to_string_lossy().into_owned(),
        display_path: "fixture".into(),
        repo_root: Some(fixture.to_string_lossy().into_owned()),
        trusted_at: Some(now.clone()),
        last_opened_at: now.clone(),
    })
    .unwrap();
    repo.create_task(&Task {
        id: TaskId::new("GAG-011-remove"),
        project_id: ProjectId::new("project-remove"),
        title: "safe removal".into(),
        status: TaskStatus::Preparing,
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
    let service = ManagedWorkspaceService::new(repo.clone(), managed_root.clone(), recovery_root);
    let record = service
        .create_managed_worktree(CreateManagedWorktree {
            repo_root: fixture.clone(),
            task_id: TaskId::new("GAG-011-remove"),
            task_slug: "safe removal".into(),
            base_ref: "main".into(),
        })
        .unwrap();
    let mut tampered = record.clone();
    tampered.relative_path = "wrong/task".into();
    repo.update_worktree(&tampered).unwrap();
    assert!(service.prepare_removal("GAG-011-remove").is_err());
    repo.update_worktree(&record).unwrap();

    repo.begin_task_execution("GAG-011-remove").unwrap();
    assert!(service.prepare_removal("GAG-011-remove").is_err());
    repo.update_task_status("GAG-011-remove", "idle", None)
        .unwrap();
    std::fs::write(Path::new(&record.path).join("tracked.txt"), "changed\n").unwrap();
    std::fs::write(Path::new(&record.path).join("未跟踪.txt"), "recover me\n").unwrap();

    let prepared = service.prepare_removal("GAG-011-remove").unwrap();
    assert!(prepared.dirty);
    assert!(prepared.force_required);
    assert!(repo.begin_task_execution("GAG-011-remove").is_err());
    service.reconcile_registry().unwrap();
    repo.begin_task_execution("GAG-011-remove").unwrap();
    repo.update_task_status("GAG-011-remove", "idle", None)
        .unwrap();
    let prepared = service.prepare_removal("GAG-011-remove").unwrap();
    assert_eq!(prepared.untracked_files, 1);
    let recovery = prepared
        .recovery
        .expect("dirty worktree must have recovery evidence");
    assert!(recovery.manifest_path.is_file());
    assert!(recovery.branch_bundle.is_file());
    assert!(recovery.tracked_patch.is_file());
    assert!(recovery.untracked_zip.is_file());
    assert!(std::fs::metadata(&recovery.branch_bundle).unwrap().len() > 0);
    assert!(std::fs::metadata(&recovery.untracked_zip).unwrap().len() > 0);

    let wrong_path = managed_root.join("wrong");
    assert!(service
        .remove_managed_worktree("GAG-011-remove", &prepared.confirmation_token, &wrong_path,)
        .is_err());
    assert!(Path::new(&record.path).exists());

    let bundle_bytes = std::fs::read(&recovery.branch_bundle).unwrap();
    std::fs::write(&recovery.branch_bundle, []).unwrap();
    assert!(service
        .remove_managed_worktree(
            "GAG-011-remove",
            &prepared.confirmation_token,
            Path::new(&record.path),
        )
        .is_err());
    assert!(Path::new(&record.path).exists());
    std::fs::write(&recovery.branch_bundle, bundle_bytes).unwrap();

    let untracked_path = Path::new(&record.path).join("未跟踪.txt");
    std::fs::write(&untracked_path, "changed after prepare\n").unwrap();
    assert!(service
        .remove_managed_worktree(
            "GAG-011-remove",
            &prepared.confirmation_token,
            Path::new(&record.path),
        )
        .is_err());
    assert!(Path::new(&record.path).exists());
    std::fs::write(&untracked_path, "recover me\n").unwrap();

    let removed = service
        .remove_managed_worktree(
            "GAG-011-remove",
            &prepared.confirmation_token,
            Path::new(&record.path),
        )
        .unwrap();
    assert_eq!(
        removed.state,
        grok_acp_gui_lib::domain::types::WorktreeState::Removed
    );
    assert!(!Path::new(&record.path).exists());
    assert_eq!(
        repo.get_recovery_item(&recovery.id).unwrap().state,
        grok_acp_gui_lib::domain::types::RecoveryState::Available
    );

    std::fs::remove_dir_all(&fixture).unwrap();
    std::fs::remove_dir_all(&managed_root).unwrap();
}

#[test]
fn reconcile_marks_an_externally_removed_worktree_missing() {
    let fixture = std::env::temp_dir().join(format!("gag-011-missing-{}", uuid::Uuid::new_v4()));
    let managed_root =
        std::env::temp_dir().join(format!("gag-011-managed-{}", uuid::Uuid::new_v4()));
    let recovery_root =
        std::env::temp_dir().join(format!("gag-011-recovery-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&fixture).unwrap();
    git(&fixture, &["init", "-b", "main"]);
    git(&fixture, &["config", "user.name", "GAG 011"]);
    git(
        &fixture,
        &["config", "user.email", "gag011@example.invalid"],
    );
    std::fs::write(fixture.join("README.md"), "fixture\n").unwrap();
    git(&fixture, &["add", "--", "README.md"]);
    git(&fixture, &["commit", "-m", "fixture"]);
    let repo: Arc<dyn Repository> = Arc::new(SqliteRepository::open_in_memory().unwrap());
    let now = grok_acp_gui_lib::domain::types::utc_now();
    repo.create_project(&Project {
        id: ProjectId::new("project-missing"),
        path: fixture.to_string_lossy().into_owned(),
        display_path: "fixture".into(),
        repo_root: Some(fixture.to_string_lossy().into_owned()),
        trusted_at: Some(now.clone()),
        last_opened_at: now.clone(),
    })
    .unwrap();
    repo.create_task(&Task {
        id: TaskId::new("GAG-011-missing"),
        project_id: ProjectId::new("project-missing"),
        title: "missing reconciliation".into(),
        status: TaskStatus::Preparing,
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
    let service = ManagedWorkspaceService::new(repo.clone(), managed_root.clone(), recovery_root);
    let record = service
        .create_managed_worktree(CreateManagedWorktree {
            repo_root: fixture.clone(),
            task_id: TaskId::new("GAG-011-missing"),
            task_slug: "missing reconciliation".into(),
            base_ref: "main".into(),
        })
        .unwrap();
    git(&fixture, &["worktree", "remove", "--force", &record.path]);
    let external = std::env::temp_dir().join(format!("gag-011-external-{}", uuid::Uuid::new_v4()));
    git(
        &fixture,
        &[
            "worktree",
            "add",
            "-b",
            "external/gag-011",
            external.to_str().unwrap(),
            "main",
        ],
    );
    let reconciled = service.reconcile_registry().unwrap();
    let missing = reconciled
        .iter()
        .find(|item| item.id == record.id)
        .expect("registered record should remain visible");
    assert_eq!(
        missing.state,
        grok_acp_gui_lib::domain::types::WorktreeState::Missing
    );
    assert_eq!(
        repo.get_worktree(&record.id.0).unwrap().state,
        grok_acp_gui_lib::domain::types::WorktreeState::Missing
    );
    assert!(reconciled.iter().any(|item| {
        item.ownership == grok_acp_gui_lib::domain::types::WorktreeOwnership::External
            && item.path == std::fs::canonicalize(&external).unwrap().to_string_lossy()
    }));

    let now = grok_acp_gui_lib::domain::types::utc_now();
    repo.create_task(&Task {
        id: TaskId::new("GAG-011-adopt"),
        project_id: ProjectId::new("project-missing"),
        title: "adopt external".into(),
        status: TaskStatus::Idle,
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
    let adoption = service
        .prepare_adoption(TaskId::new("GAG-011-adopt"), &external)
        .unwrap();
    assert!(service
        .adopt_worktree(
            TaskId::new("GAG-011-adopt"),
            &external,
            "wrong-token",
            &external,
        )
        .is_err());
    let adopted = service
        .adopt_worktree(
            TaskId::new("GAG-011-adopt"),
            &external,
            &adoption.confirmation_token,
            &adoption.absolute_path,
        )
        .unwrap();
    assert_eq!(
        adopted.ownership,
        grok_acp_gui_lib::domain::types::WorktreeOwnership::Adopted
    );

    git(
        &fixture,
        &["worktree", "remove", "--force", external.to_str().unwrap()],
    );
    git(&fixture, &["branch", "-D", "external/gag-011"]);
    git(&fixture, &["branch", "-D", &record.branch]);
    std::fs::remove_dir_all(&fixture).unwrap();
    std::fs::remove_dir_all(&managed_root).unwrap();
}

#[tokio::test]
async fn production_task_create_builds_worktree_before_starting_acp() {
    let fixture = std::env::temp_dir().join(format!("gag-011-bridge-{}", uuid::Uuid::new_v4()));
    let managed_root =
        std::env::temp_dir().join(format!("gag-011-managed-{}", uuid::Uuid::new_v4()));
    let recovery_root =
        std::env::temp_dir().join(format!("gag-011-recovery-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&fixture).unwrap();
    git(&fixture, &["init", "-b", "main"]);
    git(&fixture, &["config", "user.name", "GAG 011"]);
    git(
        &fixture,
        &["config", "user.email", "gag011@example.invalid"],
    );
    std::fs::write(fixture.join("README.md"), "fixture\n").unwrap();
    git(&fixture, &["add", "--", "README.md"]);
    git(&fixture, &["commit", "-m", "fixture"]);

    let repo = Arc::new(SqliteRepository::open_in_memory().unwrap());
    let now = grok_acp_gui_lib::domain::types::utc_now();
    repo.create_project(&Project {
        id: ProjectId::new("project-bridge"),
        path: fixture.to_string_lossy().into_owned(),
        display_path: "fixture".into(),
        repo_root: Some(fixture.to_string_lossy().into_owned()),
        trusted_at: Some(now.clone()),
        last_opened_at: now,
    })
    .unwrap();
    let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
        FakeScenario::Normal,
        fake_agent_path(),
    ));
    let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
    task_runtime.spawn_agent_event_forwarder();
    let repo_trait: Arc<dyn Repository> = repo.clone();
    let workspace = ManagedWorkspaceService::new(repo_trait, managed_root.clone(), recovery_root);
    let artifacts = ManagedArtifactService::new();

    let result = execute_impl_with_services(
        repo.as_ref(),
        runtime.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        &artifacts,
        &workspace,
        serde_json::json!({
            "type": "task.create",
            "payload": {
                "projectId": "project-bridge",
                "title": "bridge worktree",
                "prompt": "reply briefly",
                "mode": "agent",
                "workspaceStrategy": "worktree"
            }
        }),
    )
    .await;
    let data = match result {
        DesktopResult::Ok { data } => data,
        DesktopResult::Err { error } => panic!("task create failed: {error:?}"),
    };
    assert!(data.get("startError").is_none());
    let task_id = data["taskId"].as_str().unwrap();
    let records = repo.list_worktrees_by_task(task_id).unwrap();
    assert_eq!(records.len(), 1);
    let binding = repo.get_binding_by_task(task_id).unwrap().unwrap();
    assert_eq!(
        std::fs::canonicalize(binding.cwd.unwrap()).unwrap(),
        std::fs::canonicalize(&records[0].path).unwrap()
    );

    runtime.shutdown_all("test cleanup").await;
    git(
        &fixture,
        &["worktree", "remove", "--force", &records[0].path],
    );
    git(&fixture, &["branch", "-D", &records[0].branch]);
    std::fs::remove_dir_all(&fixture).unwrap();
    std::fs::remove_dir_all(&managed_root).unwrap();
}
