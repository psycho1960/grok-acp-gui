//! GAG-009 release gate integration tests — Fake ACP subprocess + TaskRuntime
//! full decision chain.
//!
//! These tests start `tests/fake-acp-agent/agent.mjs` as a real child
//! process and drive the production wiring: AgentRuntime (interpreter) →
//! TaskRuntime mailbox → Repository → `resolve_permission`/`resolve_plan` →
//! JSON-RPC response back to the Fake ACP. They cover the release-blocking
//! P1 cases that unit tests cannot:
//!
//! - RG-009-P1-01: an unapproved Plan blocks `allow_once` writes
//!   (`PLAN_NOT_APPROVED`, no state change, and the Fake ACP never receives
//!   an allow-type response) — including `plan_version=None`, `proposed`,
//!   `rejected`, `revision_requested`, and approved-but-version-mismatch.
//! - RG-009-P1-05: a standard ACP v1 `toolCall.rawInput` write request (no
//!   project-private `operation` field) is classified as a write, keeps its
//!   original allow option id, and resolves under an approved Plan.
//! - RG-009-P1-06: Fake ACP Plan approval round-trips the original JSON-RPC
//!   request id and approve option id, persists `approved`, and the task
//!   continues running.
//! - RG-009-P1-07: a permission timeout auto-rejects with the original deny
//!   option id, expires the record, recovers the task, and never sends a
//!   second response.
//!
//! Every test uses its own task id, session id, SQLite file and temporary
//! workspace. A tap transport records every inbound/outbound JSON-RPC line,
//! so "not sent", "sent exactly once", and "echoed verbatim" are observable
//! facts rather than implementation details.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use async_trait::async_trait;
use grok_acp_gui_lib::adapters::grok_acp::transport::{
    AcpTransport, TransportError, TransportHandle,
};
use grok_acp_gui_lib::adapters::grok_acp::{FakeAcpTransport, FakeScenario};
use grok_acp_gui_lib::adapters::sqlite::SqliteRepository;
use grok_acp_gui_lib::bridge::types::{SessionId, TaskId};
use grok_acp_gui_lib::domain::error::codes;
use grok_acp_gui_lib::domain::types::{
    utc_now, Project, ProjectId, Task, TaskStatus, WorkspaceKind,
};
use grok_acp_gui_lib::modules::agent_runtime::config::RuntimeConfig;
use grok_acp_gui_lib::modules::agent_runtime::requests::{ClientRequest, PromptRequest};
use grok_acp_gui_lib::modules::agent_runtime::{AgentRuntime, AgentRuntimeImpl, RuntimeState};
use grok_acp_gui_lib::modules::persistence::Repository;
use grok_acp_gui_lib::modules::task_runtime::permission::{
    OperationCategory, PermissionOptionAction, PermissionRecord, PermissionResolutionRequest,
    PermissionState,
};
use grok_acp_gui_lib::modules::task_runtime::plan::{PlanRecord, PlanResolutionRequest, PlanState};
use grok_acp_gui_lib::modules::task_runtime::{TaskRuntime, TaskRuntimeImpl};

// ---------------------------------------------------------------------------
// Tap transport: records every JSON-RPC line in both directions
// ---------------------------------------------------------------------------

struct TapTransport {
    inner: FakeAcpTransport,
    log: Arc<StdMutex<Vec<String>>>,
}

impl TapTransport {
    fn new(scenario: FakeScenario) -> (Self, Arc<StdMutex<Vec<String>>>) {
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let agent_script = PathBuf::from(manifest_dir)
            .parent()
            .unwrap()
            .join("tests")
            .join("fake-acp-agent")
            .join("agent.mjs");
        let log = Arc::new(StdMutex::new(Vec::new()));
        (
            Self {
                inner: FakeAcpTransport::new(scenario, agent_script),
                log: log.clone(),
            },
            log,
        )
    }
}

#[async_trait]
impl AcpTransport for TapTransport {
    async fn probe(&self, config: &RuntimeConfig) -> Result<(PathBuf, String), TransportError> {
        self.inner.probe(config).await
    }

    async fn spawn(
        &self,
        session_id: SessionId,
        workspace: grok_acp_gui_lib::modules::agent_runtime::WorkspaceContext,
        config: &RuntimeConfig,
    ) -> Result<TransportHandle, TransportError> {
        let mut handle = self.inner.spawn(session_id, workspace, config).await?;

        // Inbound: record every decoded message, then forward to the runtime.
        let log_in = self.log.clone();
        let (in_tx, in_rx) = tokio::sync::mpsc::channel(256);
        let mut inner_inbound = handle.inbound;
        tokio::spawn(async move {
            while let Some(message) = inner_inbound.recv().await {
                log_in.lock().unwrap().push(format!("inbound {message:?}"));
                if in_tx.send(message).await.is_err() {
                    break;
                }
            }
        });
        handle.inbound = in_rx;

        // Outbound: record every encoded line, then forward to the child.
        let log_out = self.log.clone();
        let (out_tx, mut out_rx) = tokio::sync::mpsc::channel(64);
        let inner_outbound = handle.outbound;
        tokio::spawn(async move {
            while let Some(line) = out_rx.recv().await {
                log_out.lock().unwrap().push(format!("outbound {line}"));
                if inner_outbound.send(line).await.is_err() {
                    break;
                }
            }
        });
        handle.outbound = out_tx;

        Ok(handle)
    }

    fn resolved_path(&self) -> Option<PathBuf> {
        self.inner.resolved_path()
    }
}

// ---------------------------------------------------------------------------
// Test environment
// ---------------------------------------------------------------------------

struct Env {
    repo: Arc<SqliteRepository>,
    task_runtime: Arc<TaskRuntimeImpl<AgentRuntimeImpl<TapTransport>>>,
    agent_runtime: Arc<AgentRuntimeImpl<TapTransport>>,
    log: Arc<StdMutex<Vec<String>>>,
    workspace: PathBuf,
    task_id: TaskId,
    session_id: SessionId,
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

/// Boot a full production wiring: real in-memory SQLite, real Fake ACP
/// subprocess, TaskRuntime + AgentRuntime + event forwarder.
async fn setup(scenario: FakeScenario, mode: &str, permission_timeout: Duration) -> Env {
    let workspace =
        std::env::temp_dir().join(format!("gag009-rg-{}-{}", std::process::id(), nanos()));
    fs::create_dir_all(&workspace).expect("create temp workspace");

    let concrete = Arc::new(SqliteRepository::open_in_memory().unwrap());
    let repo: Arc<dyn Repository> = concrete.clone();
    let now = utc_now();
    let path_str = workspace.to_string_lossy().to_string();
    repo.create_project(&Project {
        id: ProjectId::new("p-rg"),
        path: path_str.clone(),
        display_path: "rg".into(),
        repo_root: Some(path_str.clone()),
        trusted_at: Some(now.clone()),
        last_opened_at: now.clone(),
    })
    .unwrap();

    let task_id = TaskId::new(format!("t-rg-{}", nanos()));
    let session_id = SessionId::new(format!("s-rg-{}", nanos()));
    repo.create_task(&Task {
        id: task_id.clone(),
        project_id: ProjectId::new("p-rg"),
        title: "release gate".into(),
        status: TaskStatus::WaitingPermission,
        workspace_kind: WorkspaceKind::Direct,
        mode: Some(mode.into()),
        model: None,
        reasoning: None,
        created_at: now.clone(),
        updated_at: now.clone(),
        interrupt_reason: None,
        interrupted_at: None,
        attempt_count: 1,
    })
    .unwrap();

    let (transport, log) = TapTransport::new(scenario);
    let agent_runtime = AgentRuntimeImpl::new(transport);
    let task_runtime = Arc::new(TaskRuntimeImpl::with_concurrency_and_permission_timeout(
        repo,
        agent_runtime.clone(),
        4,
        permission_timeout,
    ));
    task_runtime.spawn_agent_event_forwarder();

    Env {
        repo: concrete,
        task_runtime,
        agent_runtime,
        log,
        workspace,
        task_id,
        session_id,
    }
}

async fn boot_and_prompt(env: &Env) {
    env.task_runtime
        .enqueue_task(env.task_id.clone(), env.session_id.clone())
        .await
        .expect("enqueue");
    env.task_runtime
        .start_session(env.task_id.clone(), env.session_id.clone())
        .await
        .expect("start session");
    env.agent_runtime
        .send(
            env.session_id.clone(),
            ClientRequest::Prompt(PromptRequest {
                message: "run the plan and commit".into(),
                attachments: vec![],
                mode: Some("code".into()),
                model: None,
                reasoning: None,
            }),
        )
        .await
        .expect("send prompt");
}

async fn wait_for<F, T>(mut condition: F) -> T
where
    F: FnMut() -> Option<T>,
{
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(value) = condition() {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("timed out waiting for condition")
}

fn pending_permission(env: &Env, request_id: &str) -> Option<PermissionRecord> {
    env.repo.get_permission(request_id, &env.session_id.0).ok()
}

fn pending_plan(env: &Env, request_id: &str) -> Option<PlanRecord> {
    env.repo.get_plan(request_id, &env.session_id.0).ok()
}

/// JSON-RPC responses the client sent back to the Fake ACP, i.e. lines with
/// both `id` and `result`.
fn outbound_responses(env: &Env) -> Vec<serde_json::Value> {
    env.log
        .lock()
        .unwrap()
        .iter()
        .filter_map(|line| {
            let payload = line.strip_prefix("outbound ")?;
            let value: serde_json::Value = serde_json::from_str(payload).ok()?;
            (value.get("id").is_some() && value.get("result").is_some()).then_some(value)
        })
        .collect()
}

/// The JSON-RPC id the agent attached to its request of the given method
/// (e.g. `session/request_permission`, `updatePlan`).
fn agent_request_rpc_id(env: &Env, method: &str) -> String {
    env.log
        .lock()
        .unwrap()
        .iter()
        .find_map(|line| {
            if !line.starts_with("inbound ") || !line.contains(&format!("method: \"{method}\"")) {
                return None;
            }
            let id_part = line.split("id: ").nth(1)?;
            let id = id_part.split(',').next()?.trim();
            Some(
                id.trim_start_matches("Number(")
                    .trim_end_matches(')')
                    .to_string(),
            )
        })
        .expect("agent request id must have been captured")
}

fn responses_with_option(env: &Env, option_id: &str) -> Vec<serde_json::Value> {
    outbound_responses(env)
        .into_iter()
        .filter(|value| value["result"]["outcome"]["optionId"].as_str() == Some(option_id))
        .collect()
}

/// Wait until a response carrying `option_id` appears in the outbound log,
/// then give any duplicate a short observation window to surface.
async fn wait_for_response(env: &Env, option_id: &str) {
    wait_for(|| (!responses_with_option(env, option_id).is_empty()).then_some(())).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
}

/// Assert no response carrying `option_id` was ever sent, after an
/// observation window long enough for a (wrong) send to reach the log.
async fn assert_never_sent(env: &Env, option_id: &str) {
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(
        responses_with_option(env, option_id).is_empty(),
        "no {option_id} response may ever reach the Fake ACP"
    );
}

async fn permission_resolve(
    env: &Env,
    permission: &PermissionRecord,
    option_id: &str,
) -> Result<PermissionState, grok_acp_gui_lib::domain::error::DomainError> {
    env.task_runtime
        .resolve_permission(PermissionResolutionRequest {
            task_id: env.task_id.clone(),
            session_id: env.session_id.clone(),
            request_id: permission.request_id.clone(),
            correlation_id: permission.correlation_id.clone(),
            expected_version: permission.plan_version.unwrap_or(0),
            option_id: option_id.into(),
        })
        .await
}

async fn plan_resolve(
    env: &Env,
    plan: &PlanRecord,
    option_id: &str,
) -> Result<PlanState, grok_acp_gui_lib::domain::error::DomainError> {
    env.task_runtime
        .resolve_plan(PlanResolutionRequest {
            task_id: env.task_id.clone(),
            session_id: env.session_id.clone(),
            request_id: plan.request_id.clone(),
            correlation_id: plan.correlation_id.clone(),
            expected_version: plan.version,
            option_id: option_id.into(),
        })
        .await
}

fn task_status(env: &Env) -> TaskStatus {
    env.repo.get_task(&env.task_id.0).unwrap().status
}

async fn shutdown(env: &Env) {
    env.agent_runtime
        .shutdown(env.session_id.clone(), "test complete")
        .await;
    // Only ever delete the workspace we created ourselves under the OS temp dir.
    let workspace = env.workspace.to_string_lossy().to_string();
    if workspace.starts_with(std::env::temp_dir().to_string_lossy().as_ref()) {
        let _ = fs::remove_dir_all(&env.workspace);
    }
}

// ---------------------------------------------------------------------------
// RG-009-P1-01: unapproved Plan blocks allow_once
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p1_01_proposed_plan_blocks_allow_once_without_sending() {
    let env = setup(
        FakeScenario::PlanPermission,
        "plan",
        Duration::from_secs(300),
    )
    .await;
    boot_and_prompt(&env).await;

    // plan-permission scenario: plan arrives first ("plan-2"), then the
    // permission ("perm-3") bound to plan version 1.
    let plan = wait_for(|| pending_plan(&env, "plan-2")).await;
    assert_eq!(plan.state, PlanState::Proposed);
    let permission = wait_for(|| pending_permission(&env, "perm-3")).await;
    assert_eq!(permission.state, PermissionState::Requested);
    assert_eq!(permission.plan_version, Some(1));
    assert_eq!(permission.category, OperationCategory::Write);

    let result = permission_resolve(&env, &permission, "opt-allow-once").await;
    assert!(
        result.is_err(),
        "allow_once under unapproved Plan must fail"
    );
    assert_eq!(result.unwrap_err().code, codes::PLAN_NOT_APPROVED);

    // The Fake ACP must NOT have received an allow-type response, and the
    // agent keeps waiting for one — the task stays waiting_permission.
    assert_never_sent(&env, "opt-allow-once").await;
    assert_eq!(
        pending_permission(&env, "perm-3").unwrap().state,
        PermissionState::Requested
    );
    assert!(matches!(task_status(&env), TaskStatus::WaitingPermission));

    shutdown(&env).await;
}

#[tokio::test]
async fn p1_01_no_plan_blocks_allow_once() {
    // permission-only scenario: no Plan record exists at all → version None.
    let env = setup(FakeScenario::Permission, "plan", Duration::from_secs(300)).await;
    boot_and_prompt(&env).await;

    let permission = wait_for(|| pending_permission(&env, "perm-2")).await;
    assert_eq!(permission.plan_version, None);

    let result = permission_resolve(&env, &permission, "opt-allow-once").await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, codes::PLAN_NOT_APPROVED);
    assert_never_sent(&env, "opt-allow-once").await;
    assert_eq!(
        pending_permission(&env, "perm-2").unwrap().state,
        PermissionState::Requested
    );

    shutdown(&env).await;
}

#[tokio::test]
async fn p1_01_rejected_and_revision_requested_plans_block_allow_once() {
    let env = setup(
        FakeScenario::PlanPermission,
        "plan",
        Duration::from_secs(300),
    )
    .await;
    boot_and_prompt(&env).await;
    let plan = wait_for(|| pending_plan(&env, "plan-2")).await;
    let permission = wait_for(|| pending_permission(&env, "perm-3")).await;

    // Reject the Plan first.
    let rejected = plan_resolve(&env, &plan, "opt-reject").await;
    assert_eq!(rejected.unwrap(), PlanState::Rejected);
    let result = permission_resolve(&env, &permission, "opt-allow-once").await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, codes::PLAN_NOT_APPROVED);
    assert_never_sent(&env, "opt-allow-once").await;
    shutdown(&env).await;

    // Revision request on a fresh environment.
    let env = setup(
        FakeScenario::PlanPermission,
        "plan",
        Duration::from_secs(300),
    )
    .await;
    boot_and_prompt(&env).await;
    let plan = wait_for(|| pending_plan(&env, "plan-2")).await;
    let permission = wait_for(|| pending_permission(&env, "perm-3")).await;
    let revised = plan_resolve(&env, &plan, "opt-revise").await;
    assert_eq!(revised.unwrap(), PlanState::RevisionRequested);
    let result = permission_resolve(&env, &permission, "opt-allow-once").await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, codes::PLAN_NOT_APPROVED);
    assert_never_sent(&env, "opt-allow-once").await;
    shutdown(&env).await;
}

#[tokio::test]
async fn p1_01_approved_plan_with_version_mismatch_blocks_allow_once() {
    let env = setup(
        FakeScenario::PlanPermission,
        "plan",
        Duration::from_secs(300),
    )
    .await;
    boot_and_prompt(&env).await;
    let plan = wait_for(|| pending_plan(&env, "plan-2")).await;
    let permission = wait_for(|| pending_permission(&env, "perm-3")).await;
    assert_eq!(permission.plan_version, Some(1));

    // Approve v1, then a newer Plan v2 arrives before the user decides.
    let approved = plan_resolve(&env, &plan, "opt-approve").await;
    assert_eq!(approved.unwrap(), PlanState::Approved);
    let mut plan_v2 = plan.clone();
    plan_v2.request_id = "plan-4".into();
    plan_v2.version = 2;
    plan_v2.plan_hash = "hash-v2".into();
    plan_v2.state = PlanState::Proposed;
    plan_v2.correlation_id = "corr-plan-v2".into();
    plan_v2.created_at = utc_now();
    plan_v2.updated_at = utc_now();
    env.repo.create_plan(&plan_v2).unwrap();

    // The newer Plan expires the v1-bound permission (fail closed) and the
    // resolution must be rejected with PLAN_NOT_APPROVED.
    assert_eq!(
        pending_permission(&env, "perm-3").unwrap().state,
        PermissionState::Expired
    );
    let result = permission_resolve(&env, &permission, "opt-allow-once").await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, codes::PLAN_NOT_APPROVED);
    assert_never_sent(&env, "opt-allow-once").await;

    shutdown(&env).await;
}

// ---------------------------------------------------------------------------
// RG-009-P1-05: standard ACP v1 rawInput write request is approvable
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p1_05_standard_acp_v1_raw_input_write_approvable() {
    let env = setup(
        FakeScenario::PlanPermission,
        "plan",
        Duration::from_secs(300),
    )
    .await;
    boot_and_prompt(&env).await;
    let plan = wait_for(|| pending_plan(&env, "plan-2")).await;
    let permission = wait_for(|| pending_permission(&env, "perm-3")).await;

    // Classified as a git write purely from `toolCall.rawInput.command`,
    // with the ACP-supplied allow option preserved verbatim.
    assert_eq!(permission.category, OperationCategory::Write);
    assert!(permission
        .options
        .iter()
        .any(|option| { option.option_id == "opt-allow-once" }));

    // Approve the Plan, then resolve the permission with the original option.
    plan_resolve(&env, &plan, "opt-approve").await.unwrap();
    let resolved = permission_resolve(&env, &permission, "opt-allow-once").await;
    assert_eq!(resolved.unwrap(), PermissionState::ApprovedOnce);

    // Exactly one response, echoing the original JSON-RPC request id and the
    // original option id back to the Fake ACP.
    let rpc_id = agent_request_rpc_id(&env, "session/request_permission");
    wait_for_response(&env, "opt-allow-once").await;
    let responses = responses_with_option(&env, "opt-allow-once");
    assert_eq!(responses.len(), 1, "allow_once must be sent exactly once");
    assert_eq!(
        responses[0]["id"].as_u64().map(|value| value.to_string()),
        Some(rpc_id),
        "response must echo the original JSON-RPC request id"
    );
    assert_eq!(
        pending_permission(&env, "perm-3").unwrap().state,
        PermissionState::ApprovedOnce
    );
    // The task left the waiting state; it is running or already idle again
    // (the agent may finish its turn right after the approval is recorded).
    assert!(
        !matches!(task_status(&env), TaskStatus::WaitingPermission),
        "an approved permission must move the task out of waiting_permission"
    );

    shutdown(&env).await;
}

// ---------------------------------------------------------------------------
// RG-009-P1-06: Fake ACP Plan approval round-trip
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p1_06_fake_acp_plan_approve_round_trip_and_task_continues() {
    let env = setup(FakeScenario::Plan, "plan", Duration::from_secs(300)).await;
    boot_and_prompt(&env).await;

    let plan = wait_for(|| pending_plan(&env, "plan-2")).await;
    assert_eq!(plan.state, PlanState::Proposed);

    let approved = plan_resolve(&env, &plan, "opt-approve").await;
    assert_eq!(approved.unwrap(), PlanState::Approved);
    assert_eq!(
        pending_plan(&env, "plan-2").unwrap().state,
        PlanState::Approved
    );

    // The Fake ACP receives exactly one response matching the original
    // updatePlan JSON-RPC id and the original approve option id.
    let rpc_id = agent_request_rpc_id(&env, "updatePlan");
    wait_for_response(&env, "opt-approve").await;
    let responses = responses_with_option(&env, "opt-approve");
    assert_eq!(responses.len(), 1);
    assert_eq!(
        responses[0]["id"].as_u64().map(|value| value.to_string()),
        Some(rpc_id),
        "Plan response must echo the original updatePlan request id"
    );

    // The task exits waiting and the turn continues to a normal completion.
    wait_for(|| {
        (env.agent_runtime.session_state(&env.session_id) == Some(RuntimeState::Ready))
            .then_some(())
    })
    .await;
    assert!(matches!(task_status(&env), TaskStatus::Idle));

    shutdown(&env).await;
}

// ---------------------------------------------------------------------------
// RG-009-P1-07: permission timeout auto-rejects and recovers the task
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p1_07_permission_timeout_auto_rejects_and_recovers() {
    let env = setup(FakeScenario::Permission, "plan", Duration::from_secs(1)).await;
    boot_and_prompt(&env).await;
    let permission = wait_for(|| pending_permission(&env, "perm-2")).await;
    assert_eq!(permission.state, PermissionState::Requested);

    // No user action: the mailbox worker expires the request and sends the
    // ACP deny option back with the original JSON-RPC id.
    wait_for(|| {
        (pending_permission(&env, "perm-2").unwrap().state == PermissionState::Expired)
            .then_some(())
    })
    .await;

    let rpc_id = agent_request_rpc_id(&env, "session/request_permission");
    wait_for_response(&env, "opt-reject").await;
    let responses = responses_with_option(&env, "opt-reject");
    assert_eq!(
        responses.len(),
        1,
        "timeout must send the deny option exactly once"
    );
    assert_eq!(
        responses[0]["id"].as_u64().map(|value| value.to_string()),
        Some(rpc_id),
        "timeout denial must echo the original JSON-RPC request id"
    );

    // Task recovers out of waiting_permission.
    wait_for(|| matches!(task_status(&env), TaskStatus::Idle).then_some(())).await;

    // Resolving after expiry fails closed and never sends a second response.
    let late = permission_resolve(&env, &permission, "opt-allow-once").await;
    assert!(late.is_err());
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(
        responses_with_option(&env, "opt-reject").len(),
        1,
        "expiry handling must be idempotent"
    );

    shutdown(&env).await;
}

// ---------------------------------------------------------------------------
// RG-009-X-03: repeated resolve after decision never sends twice
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p1_02_process_raw_input_fail_closed_in_adapter_to_task_runtime() {
    // B-1: a standard ACP v1 toolCall.rawInput whose command is a non-git
    // Process (e.g. "npm install") must be classified as a Process descriptor
    // by the interpreter, then fail-closed by adapter→TaskRuntime because
    // it carries no cwd (and no PathOptions): validate_within rejects →
    // category = Unknown → allow actions are stripped.
    let env = setup(FakeScenario::ProcessWrite, "code", Duration::from_secs(300)).await;
    boot_and_prompt(&env).await;
    let permission = wait_for(|| pending_permission(&env, "perm-2")).await;

    // Classification: extract_permission_operation sees kind="bash" +
    // rawInput.command="npm install vitest" → executable="npm", argv=[...] →
    // operation_kind="process". But the descriptor carries no cwd/src and
    // validate_within rejects → category is Unknown.
    assert_eq!(
        permission.category,
        OperationCategory::Unknown,
        "Process without cwd / paths must fail-closed to Unknown"
    );

    // Adapter strips allow actions so the only resolvable option is deny.
    let allow_options: Vec<_> = permission
        .options
        .iter()
        .filter(|opt| matches!(opt.action, PermissionOptionAction::AllowOnce))
        .collect();
    let deny_options: Vec<_> = permission
        .options
        .iter()
        .filter(|opt| matches!(opt.action, PermissionOptionAction::Deny))
        .collect();
    assert_eq!(
        allow_options.len(),
        0,
        "Unknown category must strip allow actions"
    );
    assert_eq!(
        deny_options.len(),
        1,
        "deny option must remain as the only resolvable choice"
    );

    // Resolve allow-once: fails closed ("Operation cannot be classified
    // safely; approval is blocked").
    let denied = permission_resolve(&env, &permission, "opt-allow-once").await;
    assert!(denied.is_err(), "allow-once must be rejected on Unknown");
    assert_eq!(denied.unwrap_err().code, "PERMISSION_DENIED");

    // Permission stays pending (allow wasn't applied).
    assert_eq!(
        pending_permission(&env, "perm-2").unwrap().state,
        PermissionState::Requested
    );

    // Resolve deny: succeeds and clears the request.
    let denied2 = permission_resolve(&env, &permission, "opt-reject").await;
    assert_eq!(denied2.unwrap(), PermissionState::Denied);
    assert_eq!(
        pending_permission(&env, "perm-2").unwrap().state,
        PermissionState::Denied
    );

    // Fake ACP must not have received an allow response.
    assert_never_sent(&env, "opt-allow-once").await;

    shutdown(&env).await;
}

#[tokio::test]
async fn p1_04_two_parallel_tasks_plan_permission_acp_responses_isolated() {
    // B-3: two concurrent tasks each receiving a Plan+permission request
    // must have isolated events, DB state, and ACP responses — neither
    // resolve must touch the other's plan/permission.
    let env_a = setup(
        FakeScenario::PlanPermission,
        "plan",
        Duration::from_secs(300),
    )
    .await;
    let env_b = setup(
        FakeScenario::PlanPermission,
        "plan",
        Duration::from_secs(300),
    )
    .await;
    let ea = env_a.task_runtime.clone();
    let eb = env_b.task_runtime.clone();
    let sa = env_a.session_id.clone();
    let sb = env_b.session_id.clone();

    // Boot and prompt both tasks concurrently.
    tokio::join!(
        async {
            boot_and_prompt(&env_a).await;
        },
        async {
            boot_and_prompt(&env_b).await;
        }
    );

    // Each task receives its own plan and permission on its own session.
    let plan_a = wait_for(|| pending_plan(&env_a, "plan-2")).await;
    let plan_b = wait_for(|| pending_plan(&env_b, "plan-2")).await;
    let permission_a = wait_for(|| pending_permission(&env_a, "perm-3")).await;
    let permission_b = wait_for(|| pending_permission(&env_b, "perm-3")).await;

    assert_ne!(
        env_a.session_id, env_b.session_id,
        "test setup must produce two distinct sessions"
    );

    // Resolve plan+permission on env_a — must not touch env_b's plan/permission.
    plan_resolve(&env_a, &plan_a, "opt-approve").await.unwrap();
    permission_resolve(&env_a, &permission_a, "opt-allow-once")
        .await
        .unwrap();

    let plan_b_state = env_b.repo.get_plan("plan-2", &sb.0).unwrap().state;
    let permission_b_state = pending_permission(&env_b, "perm-3").unwrap().state;
    assert_eq!(
        plan_b_state,
        PlanState::Proposed,
        "env_a's plan resolution must not affect env_b's plan"
    );
    assert_eq!(
        permission_b_state,
        PermissionState::Requested,
        "env_a's permission resolution must not affect env_b's permission"
    );

    // Resolve env_b independently.
    plan_resolve(&env_b, &plan_b, "opt-approve").await.unwrap();
    permission_resolve(&env_b, &permission_b, "opt-allow-once")
        .await
        .unwrap();

    // Both tasks now finished their AllowOnce round-trip.
    assert_eq!(
        pending_permission(&env_a, "perm-3").unwrap().state,
        PermissionState::ApprovedOnce
    );
    assert_eq!(
        pending_permission(&env_b, "perm-3").unwrap().state,
        PermissionState::ApprovedOnce
    );

    // After both tasks finished, each env's outbound log contains exactly
    // one opt-allow-once response — the responses are isolated per env
    // (proving the Fake ACP subprocess tap for each env captures only its
    // own subprocess's output).
    wait_for_response(&env_a, "opt-allow-once").await;
    wait_for_response(&env_b, "opt-allow-once").await;
    assert_eq!(
        responses_with_option(&env_a, "opt-allow-once").len(),
        1,
        "env_a must have exactly one opt-allow-once response"
    );
    assert_eq!(
        responses_with_option(&env_b, "opt-allow-once").len(),
        1,
        "env_b must have exactly one opt-allow-once response"
    );

    let _ = (ea, eb, sa);
    shutdown(&env_a).await;
    shutdown(&env_b).await;
}

#[tokio::test]
async fn rg_x_03_duplicate_resolve_after_decision_sends_once() {
    let env = setup(
        FakeScenario::PlanPermission,
        "plan",
        Duration::from_secs(300),
    )
    .await;
    boot_and_prompt(&env).await;
    let plan = wait_for(|| pending_plan(&env, "plan-2")).await;
    let permission = wait_for(|| pending_permission(&env, "perm-3")).await;

    plan_resolve(&env, &plan, "opt-approve").await.unwrap();
    permission_resolve(&env, &permission, "opt-allow-once")
        .await
        .unwrap();

    // Second resolve must fail closed (already resolved), no extra response.
    let duplicate = permission_resolve(&env, &permission, "opt-allow-once").await;
    assert!(duplicate.is_err());
    wait_for_response(&env, "opt-allow-once").await;
    assert_eq!(
        responses_with_option(&env, "opt-allow-once").len(),
        1,
        "duplicate resolution must not send a second ACP response"
    );

    shutdown(&env).await;
}
