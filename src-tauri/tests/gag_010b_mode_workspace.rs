//! GAG-010B acceptance tests for persisted mode/workspace policy linkage.
//! No test invokes Git or implements any Worktree lifecycle operation.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use grok_acp_gui_lib::adapters::grok_acp::{FakeAcpTransport, FakeScenario};
use grok_acp_gui_lib::adapters::sqlite::SqliteRepository;
use grok_acp_gui_lib::bridge::dispatch::{execute_impl, DesktopResult};
use grok_acp_gui_lib::bridge::types::{SessionId, TaskId};
use grok_acp_gui_lib::domain::types::{
    utc_now, Project, ProjectId, SessionState, Task, TaskStatus, WorkspaceKind, WorktreeId,
    WorktreeOwnership, WorktreeRecord, WorktreeState,
};
use grok_acp_gui_lib::modules::agent_runtime::{AgentRuntime, AgentRuntimeImpl, RuntimeState};
use grok_acp_gui_lib::modules::persistence::Repository;
use grok_acp_gui_lib::modules::task_runtime::permission::{
    OperationCategory, PermissionOptionAction,
};
use grok_acp_gui_lib::modules::task_runtime::TaskRuntimeImpl;

fn fake_agent_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("tests")
        .join("fake-acp-agent")
        .join("agent.mjs")
}

fn unique_temp_path(label: &str, extension: Option<&str>) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let mut path =
        std::env::temp_dir().join(format!("gag-010b-{label}-{}-{nonce}", std::process::id()));
    if let Some(extension) = extension {
        path.set_extension(extension);
    }
    path
}

fn make_workspace(label: &str) -> PathBuf {
    let path = unique_temp_path(label, None);
    std::fs::create_dir_all(&path).expect("temporary workspace");
    path
}

fn initialize_fake_git_repo(project: &Path) {
    std::fs::create_dir_all(project.join(".git").join("worktrees"))
        .expect("fake common git directory");
}

/// Build only the linked-worktree metadata needed by the read-only identity
/// validator. This does not invoke Git or create a real Worktree lifecycle.
fn register_fake_linked_worktree(project: &Path, worktree: &Path, label: &str) {
    initialize_fake_git_repo(project);
    let registration = project.join(".git").join("worktrees").join(label);
    std::fs::create_dir_all(&registration).expect("fake worktree registration");
    let marker = worktree.join(".git");
    std::fs::write(
        &marker,
        format!("gitdir: {}\n", registration.to_string_lossy()),
    )
    .expect("worktree gitdir marker");
    std::fs::write(registration.join("commondir"), "../..\n").expect("commondir marker");
    std::fs::write(
        registration.join("gitdir"),
        format!("{}\n", marker.to_string_lossy()),
    )
    .expect("worktree back-reference");
}

fn make_repo(project_id: &str, project_path: &Path) -> Arc<SqliteRepository> {
    let repo = Arc::new(SqliteRepository::open_in_memory().expect("in-memory repository"));
    repo.create_project(&Project {
        id: ProjectId::new(project_id),
        path: project_path.to_string_lossy().into_owned(),
        display_path: format!("{project_id}-fixture"),
        repo_root: Some(project_path.to_string_lossy().into_owned()),
        trusted_at: Some(utc_now()),
        last_opened_at: utc_now(),
    })
    .expect("project");
    repo
}

fn insert_task(
    repo: &dyn Repository,
    task_id: &str,
    project_id: &str,
    mode: &str,
    workspace_kind: WorkspaceKind,
) {
    let now = utc_now();
    repo.create_task(&Task {
        id: TaskId::new(task_id),
        project_id: ProjectId::new(project_id),
        title: task_id.into(),
        status: TaskStatus::Idle,
        workspace_kind,
        mode: Some(mode.into()),
        model: None,
        reasoning: None,
        created_at: now.clone(),
        updated_at: now,
        interrupt_reason: None,
        interrupted_at: None,
        attempt_count: 1,
    })
    .expect("task");
}

async fn wait_for_status(repo: &dyn Repository, task_id: &str, expected: TaskStatus) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while tokio::time::Instant::now() < deadline {
        if repo
            .get_task(task_id)
            .is_ok_and(|task| task.status == expected)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("task {task_id} did not reach {expected:?}");
}

fn directory_entries(path: &Path) -> Vec<String> {
    let mut entries: Vec<_> = std::fs::read_dir(path)
        .expect("read workspace")
        .map(|entry| {
            entry
                .expect("directory entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    entries.sort();
    entries
}

#[tokio::test]
async fn task_create_uses_safe_defaults_and_accepts_explicit_overrides() {
    let workspace = make_workspace("defaults");
    let repo = make_repo("project-defaults", &workspace);
    let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
        FakeScenario::Normal,
        fake_agent_path(),
    ));
    let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
    task_runtime.spawn_agent_event_forwarder();

    for (mode, expected) in [
        ("ask", WorkspaceKind::Direct),
        ("agent", WorkspaceKind::Worktree),
        ("plan", WorkspaceKind::Worktree),
    ] {
        let result = execute_impl(
            repo.as_ref(),
            runtime.as_ref(),
            task_runtime.as_ref(),
            serde_json::json!({
                "type": "task.create",
                "payload": {
                    "projectId": "project-defaults",
                    "title": format!("default-{mode}"),
                    "prompt": "verify mapping",
                    "mode": mode
                }
            }),
        )
        .await;
        let data = match result {
            DesktopResult::Ok { data } => data,
            DesktopResult::Err { error } => panic!("create failed: {error:?}"),
        };
        let task_id = data["taskId"].as_str().expect("task id");
        assert_eq!(
            repo.get_task(task_id)
                .expect("persisted task")
                .workspace_kind,
            expected
        );
        if expected == WorkspaceKind::Worktree {
            assert_eq!(data["startError"]["code"], "WORKTREE_NOT_READY");
        }
    }

    let overridden = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "task.create",
            "payload": {
                "projectId": "project-defaults",
                "title": "explicit readonly",
                "prompt": "verify override",
                "mode": "agent",
                "workspaceStrategy": "readonly"
            }
        }),
    )
    .await;
    let data = match overridden {
        DesktopResult::Ok { data } => data,
        DesktopResult::Err { error } => panic!("override failed: {error:?}"),
    };
    assert_eq!(
        repo.get_task(data["taskId"].as_str().expect("task id"))
            .expect("task")
            .workspace_kind,
        WorkspaceKind::Readonly
    );

    let invalid = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "task.create",
            "payload": {
                "projectId": "project-defaults",
                "title": "invalid",
                "prompt": "must reject",
                "mode": "ask",
                "workspaceStrategy": "checkout"
            }
        }),
    )
    .await;
    match invalid {
        DesktopResult::Err { error } => assert_eq!(error.code, "BRIDGE_VALIDATION_FAILED"),
        DesktopResult::Ok { .. } => panic!("invalid workspace strategy was accepted"),
    }

    runtime.shutdown_all("test complete").await;
}

#[tokio::test]
async fn configure_is_atomic_reopens_per_task_and_survives_database_restart() {
    let workspace = make_workspace("persistence");
    let db_path = unique_temp_path("restart", Some("sqlite"));

    {
        let repo = Arc::new(SqliteRepository::open(&db_path).expect("disk repository"));
        repo.create_project(&Project {
            id: ProjectId::new("project-persist"),
            path: workspace.to_string_lossy().into_owned(),
            display_path: "persistence fixture".into(),
            repo_root: Some(workspace.to_string_lossy().into_owned()),
            trusted_at: Some(utc_now()),
            last_opened_at: utc_now(),
        })
        .expect("project");
        insert_task(
            repo.as_ref(),
            "task-a",
            "project-persist",
            "ask",
            WorkspaceKind::Direct,
        );
        insert_task(
            repo.as_ref(),
            "task-b",
            "project-persist",
            "agent",
            WorkspaceKind::Worktree,
        );

        let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
            FakeScenario::Normal,
            fake_agent_path(),
        ));
        let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));

        let configured = execute_impl(
            repo.as_ref(),
            runtime.as_ref(),
            task_runtime.as_ref(),
            serde_json::json!({
                "type": "session.configure",
                "payload": {
                    "taskId": "task-a",
                    "settings": { "mode": "plan", "workspaceStrategy": "readonly" }
                }
            }),
        )
        .await;
        match configured {
            DesktopResult::Ok { data } => {
                assert_eq!(data["mode"], "plan");
                assert_eq!(data["workspaceStrategy"], "readonly");
            }
            DesktopResult::Err { error } => panic!("configure failed: {error:?}"),
        }
        let task_a = repo.get_task("task-a").expect("task a");
        assert_eq!(task_a.mode.as_deref(), Some("plan"));
        assert_eq!(task_a.workspace_kind, WorkspaceKind::Readonly);
        let task_b = repo.get_task("task-b").expect("task b");
        assert_eq!(task_b.mode.as_deref(), Some("agent"));
        assert_eq!(task_b.workspace_kind, WorkspaceKind::Worktree);
    }

    {
        let repo = Arc::new(SqliteRepository::open(&db_path).expect("reopened repository"));
        let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
            FakeScenario::Normal,
            fake_agent_path(),
        ));
        let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
        let reopened = execute_impl(
            repo.as_ref(),
            runtime.as_ref(),
            task_runtime.as_ref(),
            serde_json::json!({ "type": "task.open", "payload": { "taskId": "task-a" } }),
        )
        .await;
        match reopened {
            DesktopResult::Ok { data } => {
                assert_eq!(data["mode"], "plan");
                assert_eq!(data["workspaceStrategy"], "readonly");
            }
            DesktopResult::Err { error } => panic!("reopen failed: {error:?}"),
        }
    }
}

#[tokio::test]
async fn missing_worktree_path_starts_no_acp_process_and_writes_no_workspace_files() {
    let workspace = make_workspace("missing-worktree");
    let repo = make_repo("project-missing", &workspace);
    insert_task(
        repo.as_ref(),
        "task-missing",
        "project-missing",
        "agent",
        WorkspaceKind::Worktree,
    );
    repo.create_worktree(&WorktreeRecord {
        id: WorktreeId::new("worktree-missing"),
        task_id: TaskId::new("task-missing"),
        repo_root: workspace.to_string_lossy().into_owned(),
        path: workspace
            .join("does-not-exist")
            .to_string_lossy()
            .into_owned(),
        display_path: "missing".into(),
        branch: "grok/missing".into(),
        base_branch: "main".into(),
        base_commit: "0000000".into(),
        ownership: WorktreeOwnership::Managed,
        state: WorktreeState::Ready,
        repo_identity: String::new(),
        common_git_dir: String::new(),
        relative_path: String::new(),
        created_at: String::new(),
        last_verified_at: String::new(),
        recovery_bundle_id: None,
        disk_usage_bytes: 0,
        locked: false,
        merged: false,
    })
    .expect("worktree record");

    let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
        FakeScenario::Normal,
        fake_agent_path(),
    ));
    let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
    task_runtime.spawn_agent_event_forwarder();
    let before = directory_entries(&workspace);
    let sent = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "turn.send",
            "payload": { "taskId": "task-missing", "message": "must not start" }
        }),
    )
    .await;
    match sent {
        DesktopResult::Err { error } => assert_eq!(error.code, "WORKTREE_NOT_READY"),
        DesktopResult::Ok { .. } => panic!("missing worktree fell back to project path"),
    }
    assert_eq!(
        directory_entries(&workspace),
        before,
        "workspace files changed"
    );
    let binding = repo
        .get_binding_by_task("task-missing")
        .expect("binding query")
        .expect("binding");
    assert_eq!(runtime.session_state(&binding.session_id), None);
    assert_ne!(
        binding.cwd.as_deref(),
        Some(workspace.to_string_lossy().as_ref())
    );
    assert_eq!(
        repo.get_task("task-missing").expect("task").status,
        TaskStatus::Idle,
        "failed workspace resolution must not leave a phantom running task"
    );

    let opened = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({ "type": "task.open", "payload": { "taskId": "task-missing" } }),
    )
    .await;
    match opened {
        DesktopResult::Ok { data } => {
            assert_eq!(data["workspaceStrategy"], "worktree");
            assert_eq!(data["workspaceAvailable"], false);
        }
        DesktopResult::Err { error } => panic!("open failed: {error:?}"),
    }
}

#[tokio::test]
async fn managed_record_pointing_at_project_checkout_is_rejected() {
    let project = make_workspace("same-as-project");
    let repo = make_repo("project-same", &project);
    insert_task(
        repo.as_ref(),
        "task-same",
        "project-same",
        "agent",
        WorkspaceKind::Worktree,
    );
    repo.create_worktree(&WorktreeRecord {
        id: WorktreeId::new("worktree-same"),
        task_id: TaskId::new("task-same"),
        repo_root: project.to_string_lossy().into_owned(),
        path: project.join(".").to_string_lossy().into_owned(),
        display_path: "forged project checkout".into(),
        branch: "grok/same".into(),
        base_branch: "main".into(),
        base_commit: "0000000".into(),
        ownership: WorktreeOwnership::Managed,
        state: WorktreeState::Ready,
        repo_identity: String::new(),
        common_git_dir: String::new(),
        relative_path: String::new(),
        created_at: String::new(),
        last_verified_at: String::new(),
        recovery_bundle_id: None,
        disk_usage_bytes: 0,
        locked: false,
        merged: false,
    })
    .expect("worktree record");

    let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
        FakeScenario::Normal,
        fake_agent_path(),
    ));
    let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
    task_runtime.spawn_agent_event_forwarder();
    let sent = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "turn.send",
            "payload": { "taskId": "task-same", "message": "must stay isolated" }
        }),
    )
    .await;
    match sent {
        DesktopResult::Err { error } => assert_eq!(error.code, "WORKTREE_NOT_READY"),
        DesktopResult::Ok { .. } => panic!("project checkout was accepted as a managed worktree"),
    }
    assert_eq!(
        repo.get_task("task-same").expect("task").status,
        TaskStatus::Idle
    );
    let binding = repo
        .get_binding_by_task("task-same")
        .expect("binding query")
        .expect("binding");
    assert_eq!(runtime.session_state(&binding.session_id), None);
}

#[tokio::test]
async fn unsafe_worktree_relationships_are_rejected() {
    let mismatch_project = make_workspace("identity-project");
    let mismatch_repo = make_workspace("identity-other-repo");
    let mismatch_worktree = make_workspace("identity-worktree");
    let unregistered_project = make_workspace("unregistered-project");
    let unregistered_worktree = make_workspace("unregistered-worktree");
    initialize_fake_git_repo(&unregistered_project);

    let nested_project = make_workspace("nested-project");
    let nested_worktree = nested_project.join("nested-worktree");
    std::fs::create_dir_all(&nested_worktree).expect("nested workspace");

    let parent_root = make_workspace("parent-root");
    let parent_project = parent_root.join("project");
    std::fs::create_dir_all(&parent_project).expect("parent project");

    let cases = [
        (
            "identity",
            mismatch_project,
            mismatch_repo,
            mismatch_worktree,
        ),
        (
            "unregistered",
            unregistered_project.clone(),
            unregistered_project,
            unregistered_worktree,
        ),
        (
            "nested",
            nested_project.clone(),
            nested_project,
            nested_worktree,
        ),
        (
            "parent",
            parent_project.clone(),
            parent_project,
            parent_root,
        ),
    ];

    for (label, project, recorded_repo_root, worktree_path) in cases {
        let project_id = format!("project-{label}");
        let task_id = format!("task-{label}");
        let worktree_id = format!("worktree-{label}");
        let repo = make_repo(&project_id, &project);
        insert_task(
            repo.as_ref(),
            &task_id,
            &project_id,
            "agent",
            WorkspaceKind::Worktree,
        );
        repo.create_worktree(&WorktreeRecord {
            id: WorktreeId::new(worktree_id),
            task_id: TaskId::new(task_id.clone()),
            repo_root: recorded_repo_root.to_string_lossy().into_owned(),
            path: worktree_path.to_string_lossy().into_owned(),
            display_path: format!("unsafe {label}"),
            branch: format!("grok/{label}"),
            base_branch: "main".into(),
            base_commit: "0000000".into(),
            ownership: WorktreeOwnership::Managed,
            state: WorktreeState::Ready,
            repo_identity: String::new(),
            common_git_dir: String::new(),
            relative_path: String::new(),
            created_at: String::new(),
            last_verified_at: String::new(),
            recovery_bundle_id: None,
            disk_usage_bytes: 0,
            locked: false,
            merged: false,
        })
        .expect("worktree record");
        let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
            FakeScenario::Normal,
            fake_agent_path(),
        ));
        let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
        task_runtime.spawn_agent_event_forwarder();
        let sent = execute_impl(
            repo.as_ref(),
            runtime.as_ref(),
            task_runtime.as_ref(),
            serde_json::json!({
                "type": "turn.send",
                "payload": { "taskId": task_id, "message": "must fail closed" }
            }),
        )
        .await;
        match sent {
            DesktopResult::Err { error } => assert_eq!(
                error.code, "WORKTREE_NOT_READY",
                "unsafe {label} relationship was not rejected"
            ),
            DesktopResult::Ok { .. } => panic!("unsafe {label} relationship was accepted"),
        }
    }
}

#[tokio::test]
async fn workspace_change_rebinds_next_session_and_rejects_an_active_turn() {
    let project = make_workspace("rebind-project");
    let worktree = make_workspace("rebind-worktree");
    register_fake_linked_worktree(&project, &worktree, "rebind");
    let repo = make_repo("project-rebind", &project);
    insert_task(
        repo.as_ref(),
        "task-rebind",
        "project-rebind",
        "agent",
        WorkspaceKind::Worktree,
    );
    repo.create_worktree(&WorktreeRecord {
        id: WorktreeId::new("worktree-rebind"),
        task_id: TaskId::new("task-rebind"),
        repo_root: project.to_string_lossy().into_owned(),
        path: worktree.to_string_lossy().into_owned(),
        display_path: "rebind worktree".into(),
        branch: "grok/rebind".into(),
        base_branch: "main".into(),
        base_commit: "0000000".into(),
        ownership: WorktreeOwnership::Managed,
        state: WorktreeState::Ready,
        repo_identity: String::new(),
        common_git_dir: String::new(),
        relative_path: String::new(),
        created_at: String::new(),
        last_verified_at: String::new(),
        recovery_bundle_id: None,
        disk_usage_bytes: 0,
        locked: false,
        merged: false,
    })
    .expect("worktree record");

    let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
        FakeScenario::Normal,
        fake_agent_path(),
    ));
    let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
    task_runtime.spawn_agent_event_forwarder();
    let first = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "turn.send",
            "payload": { "taskId": "task-rebind", "message": "first" }
        }),
    )
    .await;
    assert!(matches!(first, DesktopResult::Ok { .. }), "{first:?}");
    wait_for_status(repo.as_ref(), "task-rebind", TaskStatus::Idle).await;
    assert_eq!(
        repo.get_binding_by_task("task-rebind")
            .expect("binding")
            .expect("binding")
            .cwd
            .as_deref(),
        Some(
            grok_acp_gui_lib::adapters::filesystem::canonicalize_existing_directory(&worktree)
                .expect("canonical worktree")
                .to_string_lossy()
                .as_ref()
        )
    );

    let configured = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "session.configure",
            "payload": {
                "taskId": "task-rebind",
                "settings": { "mode": "ask", "workspaceStrategy": "direct" }
            }
        }),
    )
    .await;
    assert!(
        matches!(configured, DesktopResult::Ok { .. }),
        "{configured:?}"
    );
    let rebound = repo
        .get_binding_by_task("task-rebind")
        .expect("binding")
        .expect("binding");
    assert_eq!(rebound.cwd, None);
    assert_eq!(
        rebound.state,
        grok_acp_gui_lib::domain::types::SessionState::Disconnected
    );

    let second = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "turn.send",
            "payload": { "taskId": "task-rebind", "message": "second" }
        }),
    )
    .await;
    assert!(matches!(second, DesktopResult::Ok { .. }), "{second:?}");
    assert_eq!(
        repo.get_binding_by_task("task-rebind")
            .expect("binding")
            .expect("binding")
            .cwd
            .as_deref(),
        Some(
            grok_acp_gui_lib::adapters::filesystem::canonicalize_existing_directory(&project)
                .expect("canonical project")
                .to_string_lossy()
                .as_ref()
        )
    );
    runtime.shutdown_all("test complete").await;

    let active_repo = make_repo("project-active", &project);
    insert_task(
        active_repo.as_ref(),
        "task-active",
        "project-active",
        "ask",
        WorkspaceKind::Direct,
    );
    let active_runtime =
        AgentRuntimeImpl::new(FakeAcpTransport::new(FakeScenario::Slow, fake_agent_path()));
    let active_task_runtime = Arc::new(TaskRuntimeImpl::new(
        active_repo.clone(),
        active_runtime.clone(),
    ));
    active_task_runtime.spawn_agent_event_forwarder();
    let running = execute_impl(
        active_repo.as_ref(),
        active_runtime.as_ref(),
        active_task_runtime.as_ref(),
        serde_json::json!({
            "type": "turn.send",
            "payload": { "taskId": "task-active", "message": "stay active" }
        }),
    )
    .await;
    assert!(matches!(running, DesktopResult::Ok { .. }), "{running:?}");
    assert_eq!(
        active_runtime.session_state(&SessionId::new(
            active_repo
                .get_binding_by_task("task-active")
                .expect("binding")
                .expect("binding")
                .session_id
                .0
        )),
        Some(RuntimeState::Busy)
    );
    let rejected = execute_impl(
        active_repo.as_ref(),
        active_runtime.as_ref(),
        active_task_runtime.as_ref(),
        serde_json::json!({
            "type": "session.configure",
            "payload": {
                "taskId": "task-active",
                "settings": { "mode": "agent", "workspaceStrategy": "worktree" }
            }
        }),
    )
    .await;
    match rejected {
        DesktopResult::Err { error } => assert_eq!(error.code, "DOMAIN_ILLEGAL_TRANSITION"),
        DesktopResult::Ok { .. } => panic!("active turn changed cwd without confirmation"),
    }
    assert_eq!(
        active_repo
            .get_task("task-active")
            .expect("task")
            .workspace_kind,
        WorkspaceKind::Direct
    );
    active_runtime.shutdown_all("test complete").await;
}

#[tokio::test]
async fn retained_session_rejects_a_drifted_workspace_path() {
    let project = make_workspace("drift-project");
    let first_worktree = make_workspace("drift-first");
    let second_worktree = make_workspace("drift-second");
    register_fake_linked_worktree(&project, &first_worktree, "drift-first");
    register_fake_linked_worktree(&project, &second_worktree, "drift-second");
    let repo = make_repo("project-drift", &project);
    insert_task(
        repo.as_ref(),
        "task-drift",
        "project-drift",
        "agent",
        WorkspaceKind::Worktree,
    );
    repo.create_worktree(&WorktreeRecord {
        id: WorktreeId::new("worktree-drift"),
        task_id: TaskId::new("task-drift"),
        repo_root: project.to_string_lossy().into_owned(),
        path: first_worktree.to_string_lossy().into_owned(),
        display_path: "drift fixture".into(),
        branch: "grok/drift".into(),
        base_branch: "main".into(),
        base_commit: "0000000".into(),
        ownership: WorktreeOwnership::Managed,
        state: WorktreeState::Ready,
        repo_identity: String::new(),
        common_git_dir: String::new(),
        relative_path: String::new(),
        created_at: String::new(),
        last_verified_at: String::new(),
        recovery_bundle_id: None,
        disk_usage_bytes: 0,
        locked: false,
        merged: false,
    })
    .expect("worktree record");

    let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
        FakeScenario::Normal,
        fake_agent_path(),
    ));
    let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
    task_runtime.spawn_agent_event_forwarder();
    let first = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "turn.send",
            "payload": { "taskId": "task-drift", "message": "first" }
        }),
    )
    .await;
    assert!(matches!(first, DesktopResult::Ok { .. }), "{first:?}");
    wait_for_status(repo.as_ref(), "task-drift", TaskStatus::Idle).await;

    let mut record = repo.get_worktree("worktree-drift").expect("worktree");
    record.path = second_worktree.to_string_lossy().into_owned();
    repo.update_worktree(&record)
        .expect("drift worktree record");

    let second = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "turn.send",
            "payload": { "taskId": "task-drift", "message": "must not reuse old cwd" }
        }),
    )
    .await;
    match second {
        DesktopResult::Err { error } => assert_eq!(error.code, "WORKTREE_NOT_READY"),
        DesktopResult::Ok { .. } => panic!("retained process reused a stale cwd"),
    }
    let binding = repo
        .get_binding_by_task("task-drift")
        .expect("binding query")
        .expect("binding");
    assert_eq!(binding.cwd, None);
    assert_eq!(binding.state, SessionState::Disconnected);
    assert!(!matches!(
        runtime.session_state(&binding.session_id),
        Some(RuntimeState::Ready | RuntimeState::Busy)
    ));
    assert_eq!(
        repo.get_task("task-drift").expect("task").status,
        TaskStatus::Idle
    );
}

#[tokio::test]
async fn readonly_policy_strips_allow_actions_from_write_requests() {
    let workspace = make_workspace("readonly");
    let repo = make_repo("project-readonly", &workspace);
    insert_task(
        repo.as_ref(),
        "task-readonly",
        "project-readonly",
        "ask",
        WorkspaceKind::Readonly,
    );
    let runtime = AgentRuntimeImpl::new(FakeAcpTransport::new(
        FakeScenario::ProcessWrite,
        fake_agent_path(),
    ));
    let task_runtime = Arc::new(TaskRuntimeImpl::new(repo.clone(), runtime.clone()));
    task_runtime.spawn_agent_event_forwarder();
    let mut events = task_runtime.event_subscriber();

    let sent = execute_impl(
        repo.as_ref(),
        runtime.as_ref(),
        task_runtime.as_ref(),
        serde_json::json!({
            "type": "turn.send",
            "payload": { "taskId": "task-readonly", "message": "request a write" }
        }),
    )
    .await;
    assert!(matches!(sent, DesktopResult::Ok { .. }), "{sent:?}");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let (request_id, session_id) = loop {
        assert!(
            tokio::time::Instant::now() < deadline,
            "permission event timeout"
        );
        let event = tokio::time::timeout(Duration::from_secs(1), events.recv())
            .await
            .ok()
            .and_then(Result::ok);
        let Some(event) = event else { continue };
        if event.event_type == "permission.requested" {
            let request_id = event.payload["requestId"]
                .as_str()
                .expect("request id")
                .to_string();
            break (
                request_id,
                event.session_id.expect("permission session id").0,
            );
        }
    };
    let permission = repo
        .get_permission(&request_id, &session_id)
        .expect("permission record");
    assert_eq!(permission.category, OperationCategory::Unknown);
    assert!(permission.options.iter().all(|option| matches!(
        option.action,
        PermissionOptionAction::Deny | PermissionOptionAction::Unknown
    )));
    runtime.shutdown_all("test complete").await;
}
