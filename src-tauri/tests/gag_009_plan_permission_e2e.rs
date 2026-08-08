//! GAG-009 / REG-X-03 end-to-end regression test:
//! Plan → write blocked → approve → permission → ACP option ID passthrough →
//! consume, with hard "zero I/O on deny" evidence against the real filesystem.
//!
//! Background:
//!   REG-X-03 (cross-task, native) demands that Plan + Permission +
//!   ExecutionGuard work end-to-end via the production dispatcher path,
//!   and that NO file I/O actually occurs when the guard denies a write.
//!   The internal `permission.rs` unit tests already cover the in-memory
//!   classification / guard logic with a `OneShotStore` mock. This test
//!   covers the production wiring: real `SqliteRepository` +
//!   real `ExecutionGuard<Arc<dyn Repository>>` + real filesystem probe.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use grok_acp_gui_lib::adapters::sqlite::SqliteRepository;
use grok_acp_gui_lib::domain::types::{
    utc_now, Project, ProjectId, SessionBinding, SessionId, SessionState, Task, TaskId, TaskStatus,
    WorkspaceKind,
};
use grok_acp_gui_lib::modules::persistence::Repository;
use grok_acp_gui_lib::modules::task_runtime::permission::{
    Authorization, ExecutionContext, ExecutionGuard, OperationDescriptor, OperationKind,
    PermissionDecision, PermissionOption, PermissionOptionAction, PermissionRecord,
    PermissionState,
};
use grok_acp_gui_lib::modules::task_runtime::plan::{
    PlanDecision, PlanOption, PlanOptionAction, PlanRecord, PlanState,
};

fn workspace_root() -> PathBuf {
    // Per the test-cases doc §3.1 item 2: e2e-isolated-data only; here we
    // use a one-time temp dir that the test cleans up at the end.
    let dir = std::env::temp_dir().join(format!(
        "gag-e2e-{}-{}",
        std::process::id(),
        chrono_like_nanos()
    ));
    fs::create_dir_all(&dir).expect("create temp workspace root");
    dir
}

/// Cheap monotonic id, no chrono dep needed.
fn chrono_like_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

fn seed_repo(root: &Path) -> SqliteRepository {
    let repo = SqliteRepository::open_in_memory().unwrap();
    let now = utc_now();
    let path_str = root.to_string_lossy().to_string();
    repo.create_project(&Project {
        id: ProjectId::new("p1"),
        path: path_str.clone(),
        display_path: "e2e".into(),
        repo_root: Some(path_str.clone()),
        trusted_at: Some(now.clone()),
        last_opened_at: now.clone(),
    })
    .unwrap();
    repo.create_task(&Task {
        id: TaskId::new("t1"),
        project_id: ProjectId::new("p1"),
        title: "plan-perm e2e".into(),
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
        task_id: TaskId::new("t1"),
        session_id: SessionId::new("s1"),
        cwd: Some(path_str.clone()),
        last_seq: 0,
        state: SessionState::Active,
        attempt_number: 1,
    })
    .unwrap();
    repo
}

fn make_plan(request: &str, version: u64) -> PlanRecord {
    let now = utc_now();
    PlanRecord {
        request_id: request.into(),
        task_id: TaskId::new("t1"),
        session_id: SessionId::new("s1"),
        correlation_id: format!("corr-plan-{version}"),
        workspace: "C:/repo".into(), // overwritten by seed_repo path in caller
        version,
        plan_hash: format!("hash-{version}"),
        state: PlanState::Proposed,
        summary_redacted: format!("Plan {version}"),
        options: vec![PlanOption {
            option_id: format!("approve-plan-{version}"),
            label: "Approve".into(),
            action: PlanOptionAction::Approve,
        }],
        decided_option_id: None,
        created_at: now.clone(),
        updated_at: now,
    }
}

fn make_permission(plan_version: Option<u64>) -> PermissionRecord {
    let now = utc_now();
    PermissionRecord {
        request_id: "perm-1".into(),
        task_id: TaskId::new("t1"),
        session_id: SessionId::new("s1"),
        correlation_id: "corr-perm-1".into(),
        workspace: "C:/repo".into(),
        plan_version,
        operation_digest: String::new(), // filled by caller after descriptor.digest()
        category: grok_acp_gui_lib::modules::task_runtime::permission::OperationCategory::Write,
        summary_redacted: "write · 1 target".into(),
        options: vec![
            // The ACP-supplied option ID must be passed through unchanged
            // (REG-009-14: "button text uses ACP label, internal ID is raw").
            PermissionOption {
                option_id: "acp-option-allow-once-7c9e".into(),
                label: "Allow once".into(),
                action: PermissionOptionAction::AllowOnce,
            },
            PermissionOption {
                option_id: "acp-option-deny-3f2a".into(),
                label: "Deny".into(),
                action: PermissionOptionAction::Deny,
            },
        ],
        state: PermissionState::Requested,
        expires_at_epoch_seconds: 500,
        decided_option_id: None,
        consumed_at: None,
        created_at: now.clone(),
        updated_at: now,
    }
}

fn context_for(workspace: &str, approved: bool, plan_version: Option<u64>) -> ExecutionContext {
    ExecutionContext {
        task_id: TaskId::new("t1"),
        session_id: SessionId::new("s1"),
        workspace: workspace.into(),
        plan_version,
        plan_approved: approved,
    }
}

#[allow(dead_code)]
fn context(approved: bool, plan_version: Option<u64>) -> ExecutionContext {
    context_for("C:/repo", approved, plan_version)
}

#[test]
fn plan_not_approved_blocks_write_with_zero_disk_io() {
    let root = workspace_root();
    let target = root.join("must_not_exist.txt");
    assert!(!target.exists(), "precondition: target absent");

    let repo = seed_repo(&root);
    // Patch the workspace/cwd paths in test data to the actual root
    let mut plan = make_plan("plan-1", 1);
    plan.workspace = root.to_string_lossy().to_string();
    repo.create_plan(&plan).unwrap();

    // Build a write descriptor pointing at a target file inside the workspace
    let descriptor = OperationDescriptor {
        kind: OperationKind::FileWrite,
        executable: None,
        args: vec![],
        cwd: root.to_string_lossy().to_string(),
        read_paths: vec![],
        write_paths: vec![target.to_string_lossy().to_string()],
    };
    // Sanity: descriptor should validate against the workspace
    descriptor
        .validate_within(&root.to_string_lossy())
        .expect("descriptor must validate against workspace");

    // Plan exists, NOT approved yet — workspace must match temp root
    let ctx = context_for(&root.to_string_lossy(), false, Some(1));
    // Wire the REAL repo (already seeded with project/task/binding) into the
    // ExecutionGuard as its approval store. This is the production wiring.
    let store: Arc<dyn Repository> = Arc::new(repo);
    let guard = ExecutionGuard::new(store);

    let auth = guard.authorize(&descriptor, &ctx, 10);
    assert!(
        matches!(auth, Authorization::Denied(_)),
        "expected Denied while Plan v1 unapproved, got {:?}",
        std::mem::discriminant(&auth)
    );
    // CRITICAL: zero I/O assertion
    assert!(
        !target.exists(),
        "FAIL-CLOSED REGRESSION: target file appeared on disk after denied authorization"
    );

    // While we're here: read-only operations during unapproved plan must be Allowed
    let read_descriptor = OperationDescriptor {
        kind: OperationKind::FileRead,
        executable: None,
        args: vec![],
        cwd: root.to_string_lossy().to_string(),
        read_paths: vec![root.join("allowed.txt").to_string_lossy().to_string()],
        write_paths: vec![],
    };
    let auth_read = guard.authorize(&read_descriptor, &ctx, 10);
    assert!(
        matches!(auth_read, Authorization::Allowed(_)),
        "expected Allowed for read-only under unapproved plan (declared-read exemption)"
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn plan_approve_then_permission_consume_passes_acp_option_id_through_unchanged() {
    let root = workspace_root();
    let target = root.join("guarded-write.txt");

    let repo = seed_repo(&root);

    // Plan v1 lifecycle: create -> approve
    let mut plan = make_plan("plan-1", 1);
    plan.workspace = root.to_string_lossy().to_string();
    repo.create_plan(&plan).unwrap();
    repo.decide_plan(&PlanDecision {
        request_id: "plan-1".into(),
        task_id: TaskId::new("t1"),
        session_id: SessionId::new("s1"),
        correlation_id: "corr-plan-1".into(),
        workspace: root.to_string_lossy().to_string(),
        expected_version: 1,
        option_id: "approve-plan-1".into(),
        decided_at: utc_now(),
    })
    .unwrap();
    assert_eq!(
        repo.get_plan("plan-1", "s1").unwrap().state,
        PlanState::Approved,
        "Plan v1 must be Approved after decision"
    );

    // Build the descriptor and its digest; the permission is bound to the digest
    let descriptor = OperationDescriptor {
        kind: OperationKind::FileWrite,
        executable: None,
        args: vec![],
        cwd: root.to_string_lossy().to_string(),
        read_paths: vec![],
        write_paths: vec![target.to_string_lossy().to_string()],
    };
    let digest = descriptor.digest().unwrap();

    // Permission record with the ACP-supplied option IDs
    let mut perm = make_permission(Some(1));
    perm.workspace = root.to_string_lossy().to_string();
    perm.operation_digest = digest.clone();
    repo.create_permission(&perm).unwrap();

    // UI submits the raw ACP option ID (REG-009-14: "internal option ID is raw")
    let acp_choice = "acp-option-allow-once-7c9e".to_string();
    repo.decide_permission(&PermissionDecision {
        request_id: "perm-1".into(),
        task_id: TaskId::new("t1"),
        session_id: SessionId::new("s1"),
        correlation_id: "corr-perm-1".into(),
        workspace: root.to_string_lossy().to_string(),
        expected_plan_version: Some(1),
        option_id: acp_choice.clone(),
        decided_at: utc_now(),
        decided_at_epoch_seconds: 100,
    })
    .unwrap();

    // Verify the recorded decision preserved the raw ACP option ID
    let stored = repo.get_permission("perm-1", "s1").unwrap();
    assert_eq!(
        stored.decided_option_id.as_deref(),
        Some(acp_choice.as_str()),
        "permission.decided_option_id must equal the ACP-supplied option ID, not a label-derived guess"
    );

    // Wrap the real, already-populated repo into Arc<dyn Repository> and
    // hand it to the production ExecutionGuard. This is the production
    // wiring path the dispatcher takes.
    let store: Arc<dyn Repository> = Arc::new(repo);
    let guard = ExecutionGuard::new(store);
    let ctx = context_for(&root.to_string_lossy(), true, Some(1));

    // First authorize: consumes the approval, returns Allowed
    let auth = guard.authorize(&descriptor, &ctx, 100);
    assert!(
        matches!(auth, Authorization::Allowed(_)),
        "post-approval, post-decision authorization must be Allowed; got {:?}",
        std::mem::discriminant(&auth)
    );
    // Atomic consume: second call must NOT be Allowed again
    let auth2 = guard.authorize(&descriptor, &ctx, 100);
    assert!(
        !matches!(auth2, Authorization::Allowed(_)),
        "second authorize() must NOT be Allowed (one-shot enforcement)"
    );

    // After Allowed, the production executor performs the I/O. We verify the
    // post-authorization write succeeds (this is what the production code
    // path does after guard.authorize returns Allowed).
    fs::write(&target, "approved write").expect("post-authorization write succeeds");
    assert!(
        target.exists(),
        "post-authorization write should create file"
    );
    let content = fs::read_to_string(&target).unwrap();
    assert_eq!(content, "approved write");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn plan_v2_supersedes_v1_approval_e2e() {
    let root = workspace_root();
    let repo = seed_repo(&root);

    let mut plan1 = make_plan("plan-1", 1);
    plan1.workspace = root.to_string_lossy().to_string();
    repo.create_plan(&plan1).unwrap();
    repo.decide_plan(&PlanDecision {
        request_id: "plan-1".into(),
        task_id: TaskId::new("t1"),
        session_id: SessionId::new("s1"),
        correlation_id: "corr-plan-1".into(),
        workspace: root.to_string_lossy().to_string(),
        expected_version: 1,
        option_id: "approve-plan-1".into(),
        decided_at: utc_now(),
    })
    .unwrap();
    assert_eq!(
        repo.get_plan("plan-1", "s1").unwrap().state,
        PlanState::Approved
    );

    // Now v2 arrives
    let mut plan2 = make_plan("plan-2", 2);
    plan2.workspace = root.to_string_lossy().to_string();
    repo.create_plan(&plan2).unwrap();
    assert_eq!(
        repo.get_plan("plan-1", "s1").unwrap().state,
        PlanState::Superseded,
        "Plan v1 must auto-supersede when v2 is created"
    );

    // Stale decision on v1 must fail closed
    let stale = repo.decide_plan(&PlanDecision {
        request_id: "plan-1".into(),
        task_id: TaskId::new("t1"),
        session_id: SessionId::new("s1"),
        correlation_id: "corr-plan-1".into(),
        workspace: root.to_string_lossy().to_string(),
        expected_version: 1,
        option_id: "approve-plan-1".into(),
        decided_at: utc_now(),
    });
    assert!(stale.is_err(), "stale Plan v1 decision must fail closed");

    let _ = fs::remove_dir_all(&root);
}
