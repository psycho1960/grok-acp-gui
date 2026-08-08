use grok_acp_gui_lib::adapters::sqlite::SqliteRepository;
use grok_acp_gui_lib::domain::types::{
    utc_now, Project, ProjectId, SessionBinding, SessionId, SessionState, Task, TaskId, TaskStatus,
    WorkspaceKind,
};
use grok_acp_gui_lib::modules::persistence::Repository;
use grok_acp_gui_lib::modules::task_runtime::permission::{
    ExecutionContext, OperationCategory, PermissionDecision, PermissionOption,
    PermissionOptionAction, PermissionRecord, PermissionState,
};
use grok_acp_gui_lib::modules::task_runtime::plan::{
    PlanDecision, PlanOption, PlanOptionAction, PlanRecord, PlanState,
};

fn repository() -> SqliteRepository {
    let repo = SqliteRepository::open_in_memory().unwrap();
    let now = utc_now();
    repo.create_project(&Project {
        id: ProjectId::new("project-1"),
        path: "C:/repo".into(),
        display_path: "repo".into(),
        repo_root: Some("C:/repo".into()),
        trusted_at: Some(now.clone()),
        last_opened_at: now.clone(),
    })
    .unwrap();
    repo.create_task(&Task {
        id: TaskId::new("task-1"),
        project_id: ProjectId::new("project-1"),
        title: "secure task".into(),
        status: TaskStatus::WaitingPermission,
        workspace_kind: WorkspaceKind::Worktree,
        mode: Some("plan".into()),
        model: None,
        reasoning: None,
        created_at: now.clone(),
        updated_at: now.clone(),
        interrupt_reason: None,
        interrupted_at: None,
        attempt_count: 1,
    })
    .unwrap();
    repo.create_binding(&SessionBinding {
        task_id: TaskId::new("task-1"),
        session_id: SessionId::new("session-1"),
        cwd: Some("C:/repo".into()),
        last_seq: 0,
        state: SessionState::Active,
        attempt_number: 1,
    })
    .unwrap();
    repo
}

fn permission(plan_version: Option<u64>, expires: u64) -> PermissionRecord {
    let now = utc_now();
    PermissionRecord {
        request_id: "permission-1".into(),
        task_id: TaskId::new("task-1"),
        session_id: SessionId::new("session-1"),
        correlation_id: "correlation-1".into(),
        workspace: "C:/repo".into(),
        plan_version,
        operation_digest: "digest-1".into(),
        category: OperationCategory::Write,
        summary_redacted: "write · 1 target".into(),
        options: vec![
            PermissionOption {
                option_id: "allow-1".into(),
                label: "Allow once".into(),
                action: PermissionOptionAction::AllowOnce,
            },
            PermissionOption {
                option_id: "deny-1".into(),
                label: "Deny".into(),
                action: PermissionOptionAction::Deny,
            },
        ],
        state: PermissionState::Requested,
        expires_at_epoch_seconds: expires,
        decided_option_id: None,
        consumed_at: None,
        created_at: now.clone(),
        updated_at: now,
    }
}

#[test]
fn permission_is_bound_to_context_and_consumed_once() {
    let repo = repository();
    repo.create_permission(&permission(None, 500)).unwrap();
    let now = utc_now();
    let mismatch = repo.decide_permission(&PermissionDecision {
        request_id: "permission-1".into(),
        task_id: TaskId::new("task-1"),
        session_id: SessionId::new("other-session"),
        correlation_id: "correlation-1".into(),
        workspace: "C:/repo".into(),
        expected_plan_version: None,
        option_id: "allow-1".into(),
        decided_at: now.clone(),
        decided_at_epoch_seconds: 10,
    });
    assert!(mismatch.is_err());

    repo.decide_permission(&PermissionDecision {
        request_id: "permission-1".into(),
        task_id: TaskId::new("task-1"),
        session_id: SessionId::new("session-1"),
        correlation_id: "correlation-1".into(),
        workspace: "C:/repo".into(),
        expected_plan_version: None,
        option_id: "allow-1".into(),
        decided_at: now,
        decided_at_epoch_seconds: 10,
    })
    .unwrap();
    let context = ExecutionContext {
        task_id: TaskId::new("task-1"),
        session_id: SessionId::new("session-1"),
        workspace: "C:/repo".into(),
        plan_version: None,
        plan_approved: true,
    };
    assert!(repo
        .consume_permission(&context, "digest-1", 20)
        .unwrap()
        .is_some());
    assert!(repo
        .consume_permission(&context, "digest-1", 20)
        .unwrap()
        .is_none());
}

#[test]
fn raw_request_ids_are_isolated_by_session() {
    let repo = repository();
    let now = utc_now();
    repo.create_task(&Task {
        id: TaskId::new("task-2"),
        project_id: ProjectId::new("project-1"),
        title: "second".into(),
        status: TaskStatus::WaitingPermission,
        workspace_kind: WorkspaceKind::Worktree,
        mode: Some("plan".into()),
        model: None,
        reasoning: None,
        created_at: now.clone(),
        updated_at: now,
        interrupt_reason: None,
        interrupted_at: None,
        attempt_count: 1,
    })
    .unwrap();
    repo.create_binding(&SessionBinding {
        task_id: TaskId::new("task-2"),
        session_id: SessionId::new("session-2"),
        cwd: Some("C:/repo".into()),
        last_seq: 0,
        state: SessionState::Active,
        attempt_number: 1,
    })
    .unwrap();
    repo.create_permission(&permission(None, 500)).unwrap();
    let mut second = permission(None, 500);
    second.task_id = TaskId::new("task-2");
    second.session_id = SessionId::new("session-2");
    repo.create_permission(&second).unwrap();
    assert_eq!(
        repo.get_permission("permission-1", "session-1")
            .unwrap()
            .task_id
            .0,
        "task-1"
    );
    assert_eq!(
        repo.get_permission("permission-1", "session-2")
            .unwrap()
            .task_id
            .0,
        "task-2"
    );

    for (task_id, session_id, correlation_id) in [
        ("task-1", "session-1", "correlation-1"),
        ("task-2", "session-2", "correlation-1"),
    ] {
        repo.decide_permission(&PermissionDecision {
            request_id: "permission-1".into(),
            task_id: TaskId::new(task_id),
            session_id: SessionId::new(session_id),
            correlation_id: correlation_id.into(),
            workspace: "C:/repo".into(),
            expected_plan_version: None,
            option_id: "allow-1".into(),
            decided_at: utc_now(),
            decided_at_epoch_seconds: 10,
        })
        .unwrap();
    }

    for (task_id, session_id) in [("task-1", "session-1"), ("task-2", "session-2")] {
        let context = ExecutionContext {
            task_id: TaskId::new(task_id),
            session_id: SessionId::new(session_id),
            workspace: "C:/repo".into(),
            plan_version: None,
            plan_approved: true,
        };
        assert!(
            repo.consume_permission(&context, "digest-1", 20)
                .unwrap()
                .is_some(),
            "same request ID must be consumed independently for {session_id}"
        );
    }
}

#[test]
fn new_plan_version_supersedes_old_plan_and_approvals() {
    let repo = repository();
    let make_plan = |request: &str, version: u64| PlanRecord {
        request_id: request.into(),
        task_id: TaskId::new("task-1"),
        session_id: SessionId::new("session-1"),
        correlation_id: format!("correlation-{version}"),
        workspace: "C:/repo".into(),
        version,
        plan_hash: format!("hash-{version}"),
        state: PlanState::Proposed,
        summary_redacted: format!("Plan {version}"),
        options: vec![PlanOption {
            option_id: format!("approve-{version}"),
            label: "Approve".into(),
            action: PlanOptionAction::Approve,
        }],
        decided_option_id: None,
        created_at: utc_now(),
        updated_at: utc_now(),
    };
    repo.create_plan(&make_plan("plan-1", 1)).unwrap();
    repo.create_permission(&permission(Some(1), 500)).unwrap();
    repo.decide_permission(&PermissionDecision {
        request_id: "permission-1".into(),
        task_id: TaskId::new("task-1"),
        session_id: SessionId::new("session-1"),
        correlation_id: "correlation-1".into(),
        workspace: "C:/repo".into(),
        expected_plan_version: Some(1),
        option_id: "allow-1".into(),
        decided_at: utc_now(),
        decided_at_epoch_seconds: 10,
    })
    .unwrap();

    repo.create_plan(&make_plan("plan-2", 2)).unwrap();
    assert_eq!(
        repo.get_plan("plan-1", "session-1").unwrap().state,
        PlanState::Superseded
    );
    assert_eq!(
        repo.get_permission("permission-1", "session-1")
            .unwrap()
            .state,
        PermissionState::Expired
    );
    let stale = repo.decide_plan(&PlanDecision {
        request_id: "plan-1".into(),
        task_id: TaskId::new("task-1"),
        session_id: SessionId::new("session-1"),
        correlation_id: "correlation-1".into(),
        workspace: "C:/repo".into(),
        expected_version: 1,
        option_id: "approve-1".into(),
        decided_at: utc_now(),
    });
    assert!(stale.is_err());
}

#[test]
fn unknown_and_expired_options_fail_closed() {
    let repo = repository();
    let mut record = permission(None, 5);
    record.options[0].action = PermissionOptionAction::Unknown;
    repo.create_permission(&record).unwrap();
    let decision = PermissionDecision {
        request_id: "permission-1".into(),
        task_id: TaskId::new("task-1"),
        session_id: SessionId::new("session-1"),
        correlation_id: "correlation-1".into(),
        workspace: "C:/repo".into(),
        expected_plan_version: None,
        option_id: "allow-1".into(),
        decided_at: utc_now(),
        decided_at_epoch_seconds: 10,
    };
    assert!(repo.decide_permission(&decision).is_err());
    assert_eq!(
        repo.get_permission("permission-1", "session-1")
            .unwrap()
            .state,
        PermissionState::Expired
    );
}

#[test]
fn startup_recovery_expires_one_shot_approval() {
    let repo = repository();
    repo.create_permission(&permission(None, 500)).unwrap();
    repo.decide_permission(&PermissionDecision {
        request_id: "permission-1".into(),
        task_id: TaskId::new("task-1"),
        session_id: SessionId::new("session-1"),
        correlation_id: "correlation-1".into(),
        workspace: "C:/repo".into(),
        expected_plan_version: None,
        option_id: "allow-1".into(),
        decided_at: utc_now(),
        decided_at_epoch_seconds: 10,
    })
    .unwrap();
    repo.recover_interrupted_tasks("crash").unwrap();
    assert_eq!(
        repo.get_permission("permission-1", "session-1")
            .unwrap()
            .state,
        PermissionState::Expired
    );
}
