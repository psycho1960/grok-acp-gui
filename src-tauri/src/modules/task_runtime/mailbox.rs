//! Per-session mailbox: a dedicated mpsc channel that serialises all domain
//! mutations for a single session.
//!
//! Each session has a `SessionMailbox`. The TaskRuntime spawns a worker task
//! that drains the mailbox and applies each `SessionCommand` in order.
//! This guarantees that within a session, all state transitions are
//! strictly ordered — no two commands ever execute concurrently for the
//! same session.
//!
//! Different sessions' mailboxes run independently; only the global
//! semaphore gates the *number* of concurrent sessions.

use crate::bridge::types::{SessionId, TaskId};
use crate::domain::error::DomainError;
use tokio::sync::{mpsc, oneshot};

/// Maximum number of pending commands per session mailbox.
const MAILBOX_CAPACITY: usize = 64;

/// A command dispatched to a session's mailbox for serial execution.
pub enum SessionCommand {
    /// Accept and persist a processed agent event.
    AcceptEvent {
        event: Box<crate::modules::agent_runtime::TimestampedEvent>,
        reply: oneshot::Sender<Result<(), DomainError>>,
    },
    /// Cancel the current session/turn.
    CancelSession {
        reply: oneshot::Sender<Result<(), DomainError>>,
    },
    /// Shut down the mailbox worker (sent when session ends).
    Shutdown,
}

/// Handle for sending commands into a session's mailbox.
#[derive(Clone)]
pub struct SessionMailbox {
    pub task_id: TaskId,
    pub session_id: SessionId,
    sender: mpsc::Sender<SessionCommand>,
}

impl SessionMailbox {
    /// Create a new mailbox and spawn its worker task.
    pub fn new(
        task_id: TaskId,
        session_id: SessionId,
        repo: std::sync::Arc<dyn crate::modules::persistence::Repository>,
        event_broadcaster: tokio::sync::broadcast::Sender<crate::bridge::events::DesktopEvent>,
    ) -> Self {
        let (tx, rx) = mpsc::channel(MAILBOX_CAPACITY);

        let mailbox = Self {
            task_id: task_id.clone(),
            session_id: session_id.clone(),
            sender: tx,
        };

        // Spawn the worker.
        let worker = MailboxWorker {
            task_id,
            session_id,
            repo,
            event_broadcaster,
            rx,
        };
        tokio::spawn(worker.run());

        mailbox
    }

    /// Send a command into the mailbox, awaiting its result.
    pub async fn send(&self, cmd: SessionCommand) -> Result<(), DomainError> {
        self.sender
            .send(cmd)
            .await
            .map_err(|_| DomainError::new("TASK_RUNTIME_MAILBOX_CLOSED", "session mailbox closed"))
    }

    /// Try to send a command without blocking (non-blocking path).
    pub fn try_send(&self, cmd: SessionCommand) -> Result<(), DomainError> {
        self.sender.try_send(cmd).map_err(|e| {
            DomainError::new(
                "TASK_RUNTIME_MAILBOX_FULL",
                format!("session mailbox full or closed: {}", e),
            )
        })
    }
}

/// The worker task that drains a session's mailbox.
struct MailboxWorker {
    task_id: TaskId,
    #[allow(dead_code)]
    session_id: SessionId, // reserved for future reconnection logic
    repo: std::sync::Arc<dyn crate::modules::persistence::Repository>,
    event_broadcaster: tokio::sync::broadcast::Sender<crate::bridge::events::DesktopEvent>,
    rx: mpsc::Receiver<SessionCommand>,
}

impl MailboxWorker {
    async fn run(mut self) {
        while let Some(cmd) = self.rx.recv().await {
            match cmd {
                SessionCommand::AcceptEvent { event, reply } => {
                    let result = self.handle_accept_event(*event).await;
                    let _ = reply.send(result);
                }
                SessionCommand::CancelSession { reply } => {
                    let result = self.handle_cancel().await;
                    let _ = reply.send(result);
                }
                SessionCommand::Shutdown => {
                    break;
                }
            }
        }
    }

    async fn handle_accept_event(
        &self,
        event: crate::modules::agent_runtime::TimestampedEvent,
    ) -> Result<(), DomainError> {
        use crate::domain::error::codes;
        use crate::domain::types::StoredEvent;

        let session_id = event.meta.session_id.clone();
        let seq = event.meta.sequence;

        // 1. Check for gap: expected next sequence is last_seq + 1.
        let last_seq = self.repo.get_max_sequence(&session_id.0)?;
        let expected = last_seq.map(|s| s + 1).unwrap_or(1);
        if seq < expected {
            // Duplicate — idempotent.
            return Err(DomainError::new(
                codes::EVENT_DUPLICATE,
                format!(
                    "duplicate event: seq {} already received (last: {:?})",
                    seq, last_seq
                ),
            ));
        }
        if seq > expected {
            // Gap — cannot skip events.
            return Err(DomainError::new(
                codes::EVENT_GAP_DETECTED,
                format!("event gap detected: expected seq {}, got {}", expected, seq),
            ));
        }

        // 2. Determine if this event has side effects.
        let has_side_effects = has_side_effects_kind(event.event.kind_str());

        // 3. Build StoredEvent.
        let dedup_key = format!("{}:{}", session_id.0, seq);
        let mut stored_payload =
            serde_json::to_value(&event.event).unwrap_or(serde_json::Value::Null);
        self.register_approval_request(&event, &mut stored_payload)?;
        let stored = StoredEvent {
            dedup_key,
            session_id: session_id.clone(),
            task_id: self.task_id.clone(),
            sequence: seq,
            event_type: event.event.kind_str().to_string(),
            payload: stored_payload,
            correlation_id: event.meta.correlation_id.clone(),
            persisted_at: crate::domain::types::utc_now(),
            has_side_effects,
        };

        // 4. Persist event (idempotent via dedup_key UNIQUE constraint).
        let inserted = self.repo.append_event(&stored)?;
        if !inserted {
            // Already exists — idempotent success.
            return Ok(());
        }

        // 5. Update session binding's last_seq and aggregate task state.
        if let Some(mut binding) = self.repo.get_binding_by_task(&self.task_id.0)? {
            if binding.session_id == session_id {
                binding.last_seq = seq;
                match &event.event {
                    crate::modules::agent_runtime::AgentEvent::AssistantCompleted(_)
                    | crate::modules::agent_runtime::AgentEvent::TurnCancelled(_) => {
                        binding.state = crate::domain::types::SessionState::Idle;
                    }
                    crate::modules::agent_runtime::AgentEvent::RequestFailed(_) => {
                        binding.state = crate::domain::types::SessionState::Idle;
                    }
                    crate::modules::agent_runtime::AgentEvent::ProcessExited(_) => {
                        binding.state = crate::domain::types::SessionState::Disconnected;
                    }
                    _ => {}
                }
                self.repo.update_binding(&binding)?;
            }
        }

        match &event.event {
            crate::modules::agent_runtime::AgentEvent::AssistantCompleted(_) => {
                self.repo
                    .update_task_status(&self.task_id.0, "idle", None)?;
            }
            crate::modules::agent_runtime::AgentEvent::TurnCancelled(_) => {
                self.repo
                    .update_task_status(&self.task_id.0, "idle", Some("cancelled by user"))?;
            }
            crate::modules::agent_runtime::AgentEvent::RequestFailed(failure) => {
                self.repo.update_task_status(
                    &self.task_id.0,
                    "failed",
                    Some(&format!("{}: {}", failure.code, failure.message)),
                )?;
            }
            crate::modules::agent_runtime::AgentEvent::PermissionRequested(_) => {
                self.repo
                    .update_task_status(&self.task_id.0, "waiting_permission", None)?;
            }
            crate::modules::agent_runtime::AgentEvent::PlanProposed(_) => {
                self.repo.update_task_status(
                    &self.task_id.0,
                    "waiting_permission",
                    Some("plan"),
                )?;
            }
            crate::modules::agent_runtime::AgentEvent::ProcessExited(exit) => {
                self.repo
                    .expire_session_permissions(&session_id.0, "process exited")?;
                // A managed shutdown is classified by AgentRuntime as clean.
                // A live turn is interrupted even during managed application
                // shutdown; any non-clean exit also invalidates an idle,
                // reusable ACP session.
                let task = self.repo.get_task(&self.task_id.0)?;
                if task.status.implies_live_process() || exit.reason != "clean" {
                    self.repo.update_task_status(
                        &self.task_id.0,
                        "interrupted",
                        Some("Grok process exited unexpectedly"),
                    )?;
                }
            }
            _ => {}
        }

        // 6. Publish to bridge (after persistence).
        if let Some(bridge_event) = map_stored_event_to_bridge(&stored) {
            let _ = self.event_broadcaster.send(bridge_event);
        }

        Ok(())
    }

    fn register_approval_request(
        &self,
        event: &crate::modules::agent_runtime::TimestampedEvent,
        stored_payload: &mut serde_json::Value,
    ) -> Result<(), DomainError> {
        use crate::modules::agent_runtime::AgentEvent;
        use crate::modules::task_runtime::permission::{PermissionRecord, PermissionState};
        use crate::modules::task_runtime::plan::{PlanOption, PlanRecord, PlanState};
        use sha2::{Digest, Sha256};

        if !matches!(
            event.event,
            AgentEvent::PermissionRequested(_) | AgentEvent::PlanProposed(_)
        ) {
            return Ok(());
        }

        let binding = self
            .repo
            .get_binding_by_task(&self.task_id.0)?
            .ok_or_else(|| {
                DomainError::new(
                    crate::domain::error::codes::PERMISSION_DENIED,
                    "Session binding is missing",
                )
            })?;
        if binding.session_id != event.meta.session_id {
            return Err(DomainError::new(
                crate::domain::error::codes::PERMISSION_CONTEXT_MISMATCH,
                "Approval event session does not own this task",
            ));
        }
        let workspace = binding.cwd.unwrap_or_default();
        let correlation_id = event
            .meta
            .correlation_id
            .as_ref()
            .map(|value| value.0.clone())
            .unwrap_or_default();
        if workspace.trim().is_empty() || correlation_id.trim().is_empty() {
            return Err(DomainError::new(
                crate::domain::error::codes::PERMISSION_DENIED,
                "Approval request is missing workspace or correlation context",
            ));
        }
        let now = crate::domain::types::utc_now();

        match &event.event {
            AgentEvent::PermissionRequested(payload) => {
                let plan_version = self.repo.latest_plan_version(&self.task_id.0)?;
                let descriptor = payload.operation.as_ref().and_then(operation_from_agent);
                // The full containment check is the same one ExecutionGuard
                // applies at I/O time. An operation that fails it (unknown
                // category, missing cwd, escaped paths, git -C outside the
                // workspace) is recorded as Unknown and cannot be approved.
                let category = match descriptor.as_ref() {
                    Some(value) if value.validate_within(&workspace).is_ok() => value.category(),
                    _ => crate::modules::task_runtime::permission::OperationCategory::Unknown,
                };
                let digest = descriptor
                    .as_ref()
                    .and_then(|value| value.digest().ok())
                    .unwrap_or_else(|| {
                        let mut hash = Sha256::new();
                        hash.update(b"gag-009-unknown-operation\0");
                        hash.update(payload.request_id.as_bytes());
                        format!("{:x}", hash.finalize())
                    });
                // Unknown operations must fail closed: allow actions are
                // stripped so no option can authorize them. Rejecting remains
                // available because denial is always safe.
                let options = restrict_options_for_category(&payload.options, category);
                let expires_at_epoch_seconds = epoch_seconds().saturating_add(300);
                self.repo.create_permission(&PermissionRecord {
                    request_id: payload.request_id.clone(),
                    task_id: self.task_id.clone(),
                    session_id: event.meta.session_id.clone(),
                    correlation_id: correlation_id.clone(),
                    workspace: workspace.clone(),
                    plan_version,
                    operation_digest: digest,
                    category,
                    summary_redacted: permission_summary(payload, category),
                    options,
                    state: PermissionState::Requested,
                    expires_at_epoch_seconds,
                    decided_option_id: None,
                    consumed_at: None,
                    created_at: now.clone(),
                    updated_at: now.clone(),
                })?;
                *stored_payload = serde_json::json!({
                    "requestId": payload.request_id,
                    "correlationId": correlation_id,
                    "expectedVersion": plan_version,
                    "expiresAtEpochSeconds": expires_at_epoch_seconds,
                    "options": payload.options,
                    "toolCall": {
                        "toolCallId": payload.tool_call.tool_call_id,
                        "title": payload.tool_call.title,
                        "kind": payload.tool_call.kind,
                        "locations": payload.tool_call.locations,
                    },
                    "operation": safe_operation_view(payload.operation.as_ref(), category),
                });
            }
            AgentEvent::PlanProposed(payload) => {
                let version = self.repo.latest_plan_version(&self.task_id.0)?.unwrap_or(0) + 1;
                let mut hash = Sha256::new();
                hash.update(b"gag-009-plan-v1\0");
                hash.update(payload.summary.as_bytes());
                let plan_hash = format!("{:x}", hash.finalize());
                let options: Vec<PlanOption> = payload
                    .options
                    .iter()
                    .map(|option| PlanOption {
                        option_id: option.option_id.clone(),
                        label: option.name.clone(),
                        action: plan_action(option.kind.as_deref()),
                    })
                    .collect();
                self.repo.create_plan(&PlanRecord {
                    request_id: payload.request_id.clone(),
                    task_id: self.task_id.clone(),
                    session_id: event.meta.session_id.clone(),
                    correlation_id: correlation_id.clone(),
                    workspace: workspace.clone(),
                    version,
                    plan_hash,
                    state: PlanState::Proposed,
                    summary_redacted: payload.summary.clone(),
                    options,
                    decided_option_id: None,
                    created_at: now.clone(),
                    updated_at: now,
                })?;
                *stored_payload = serde_json::json!({
                    "status": "proposed",
                    "detail": {
                        "requestId": payload.request_id,
                        "correlationId": correlation_id,
                        "version": version,
                        "summary": payload.summary,
                        "steps": plan_steps(&payload.summary),
                        "options": payload.options,
                    }
                });
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_cancel(&self) -> Result<(), DomainError> {
        // Transition task from Running/WaitingPermission to Idle or Interrupted.
        // The actual process kill is handled by AgentRuntime; here we update
        // the domain state.
        if let Some(mut binding) = self.repo.get_binding_by_task(&self.task_id.0)? {
            self.repo
                .expire_session_permissions(&binding.session_id.0, "turn cancelled")?;
            if binding.state == crate::domain::types::SessionState::Active {
                binding.state = crate::domain::types::SessionState::Idle;
                self.repo.update_binding(&binding)?;
            }
        }
        Ok(())
    }
}

fn epoch_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn permission_action(
    kind: Option<&str>,
) -> crate::modules::task_runtime::permission::PermissionOptionAction {
    use crate::modules::task_runtime::permission::PermissionOptionAction::*;
    match kind.map(|value| value.to_ascii_lowercase()).as_deref() {
        Some("allow_once" | "approve_once") => AllowOnce,
        Some("allow_always" | "allow_scope" | "approve_scope") => AllowScope,
        Some("reject_once" | "reject_always" | "reject" | "deny" | "cancel") => Deny,
        _ => Unknown,
    }
}

/// Fail-closed projection for unclassifiable operations: allow actions are
/// stripped (Unknown), denial stays available because it is always safe.
fn restrict_options_for_category(
    options: &[crate::modules::agent_runtime::events::PermissionOptionDescriptor],
    category: crate::modules::task_runtime::permission::OperationCategory,
) -> Vec<crate::modules::task_runtime::permission::PermissionOption> {
    use crate::modules::task_runtime::permission::PermissionOptionAction;
    use crate::modules::task_runtime::permission::{OperationCategory, PermissionOption};
    options
        .iter()
        .map(|option| {
            let action = permission_action(option.kind.as_deref());
            PermissionOption {
                option_id: option.option_id.clone(),
                label: option.name.clone(),
                action: if category == OperationCategory::Unknown
                    && action != PermissionOptionAction::Deny
                {
                    PermissionOptionAction::Unknown
                } else {
                    action
                },
            }
        })
        .collect()
}

fn plan_action(kind: Option<&str>) -> crate::modules::task_runtime::plan::PlanOptionAction {
    use crate::modules::task_runtime::plan::PlanOptionAction::*;
    match kind.map(|value| value.to_ascii_lowercase()).as_deref() {
        Some("approve" | "allow_once") => Approve,
        Some("continue" | "continue_planning" | "request_revision" | "revision_requested") => {
            RequestRevision
        }
        Some("cancel" | "reject" | "reject_once" | "reject_always") => Reject,
        _ => Unknown,
    }
}

fn operation_from_agent(
    source: &crate::modules::agent_runtime::events::PermissionOperationDescriptor,
) -> Option<crate::modules::task_runtime::permission::OperationDescriptor> {
    use crate::modules::task_runtime::permission::{OperationDescriptor, OperationKind};
    let kind = match source.operation_kind.to_ascii_lowercase().as_str() {
        "process" | "command" | "execute" => OperationKind::Process,
        "git" => OperationKind::Git,
        "file_read" | "read" => OperationKind::FileRead,
        "file_write" | "write" | "edit" => OperationKind::FileWrite,
        "file_delete" | "delete" => OperationKind::FileDelete,
        _ => return None,
    };
    Some(OperationDescriptor {
        kind,
        executable: source.executable.clone(),
        args: source.args.clone(),
        cwd: source.cwd.clone()?,
        read_paths: source.read_paths.clone(),
        write_paths: source.write_paths.clone(),
    })
}

fn permission_summary(
    payload: &crate::modules::agent_runtime::events::PermissionRequestedPayload,
    category: crate::modules::task_runtime::permission::OperationCategory,
) -> String {
    format!(
        "{} · {} · {} target(s)",
        payload.tool_call.title.as_deref().unwrap_or("Tool request"),
        serde_json::to_value(category)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_else(|| "unknown".into()),
        payload
            .operation
            .as_ref()
            .map(|value| value.read_paths.len() + value.write_paths.len())
            .unwrap_or(0)
    )
}

fn safe_arg(arg: &str) -> String {
    let lower = arg.to_ascii_lowercase();
    // Keyword scan catches both `--flag=value` forms and bare values. The
    // list covers credentials commonly embedded in command lines; anything
    // that cannot be proven safe is redacted.
    if [
        "token",
        "secret",
        "password",
        "passwd",
        "authorization",
        "auth",
        "api_key",
        "apikey",
        "api-key",
        "x-api-key",
        "bearer",
        "jwt",
        "cookie",
        "client_secret",
        "client-secret",
        "access_key",
        "access-key",
        "private_key",
        "private-key",
        "credential",
        "session_key",
        "session-key",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        return "[redacted]".into();
    }
    // JWT-style payloads start with a recognizable header.
    if lower.starts_with("eyj") || lower.starts_with("ey0") {
        return "[redacted]".into();
    }
    // High-entropy value without path separators is treated as a credential.
    if arg.chars().count() >= 32
        && !arg.contains('/')
        && !arg.contains('\\')
        && arg.chars().any(|ch| ch.is_ascii_digit())
        && arg.chars().any(|ch| ch.is_ascii_alphabetic())
    {
        return "[redacted]".into();
    }
    if arg.chars().count() > 160 {
        format!("{}…", arg.chars().take(157).collect::<String>())
    } else {
        arg.to_string()
    }
}

fn safe_args(args: &[String]) -> Vec<String> {
    let mut redact_next = false;
    args.iter()
        .map(|arg| {
            if redact_next {
                redact_next = false;
                return "[redacted]".into();
            }
            let lower = arg.to_ascii_lowercase();
            if [
                "--token",
                "--secret",
                "--password",
                "--authorization",
                "--api-key",
                "--apikey",
                "--header",
                "-H",
                "--cookie",
                "--cookie-jar",
                "--access-token",
                "--bearer",
                "--jwt",
            ]
            .iter()
            .any(|flag| lower == *flag)
            {
                redact_next = true;
                return arg.clone();
            }
            safe_arg(arg)
        })
        .collect()
}

fn safe_operation_view(
    operation: Option<&crate::modules::agent_runtime::events::PermissionOperationDescriptor>,
    category: crate::modules::task_runtime::permission::OperationCategory,
) -> serde_json::Value {
    let Some(operation) = operation else {
        return serde_json::json!({
            "category": category,
            "risk": "操作字段缺失，后端将默认拒绝",
        });
    };
    serde_json::json!({
        "category": category,
        "executable": operation.executable,
        "args": safe_args(&operation.args),
        "cwd": operation.cwd,
        "readPaths": operation.read_paths,
        "writePaths": operation.write_paths,
        "risk": match category {
            crate::modules::task_runtime::permission::OperationCategory::ReadOnly => "只读探测，不应修改工作区",
            crate::modules::task_runtime::permission::OperationCategory::Write => "将修改工作区或 Git 状态",
            crate::modules::task_runtime::permission::OperationCategory::Destructive => "可能造成难以恢复的数据变化",
            crate::modules::task_runtime::permission::OperationCategory::Unknown => "无法安全分类，后端将默认拒绝",
        },
    })
}

fn plan_steps(summary: &str) -> Vec<String> {
    summary
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(100)
        .map(|line| {
            line.trim_start_matches(|ch: char| {
                ch.is_ascii_digit() || matches!(ch, '.' | ')' | '-' | '*')
            })
            .trim()
            .to_string()
        })
        .filter(|line| !line.is_empty())
        .collect()
}

/// Classify whether an event kind carries side effects.
/// Events that represent writes, terminal commands, or file mutations
/// must NOT be auto-replayed on recovery.
fn has_side_effects_kind(kind: &str) -> bool {
    matches!(
        kind,
        "tool_started" | "tool_completed" | "permission_requested" | "plan_proposed"
    )
}

/// Map an agent_runtime event kind string to a bridge event type string.
/// Every persisted runtime event is mapped to a safe session event so the
/// bridge-visible sequence remains continuous.
pub fn map_stored_event_to_bridge(
    stored: &crate::domain::types::StoredEvent,
) -> Option<crate::bridge::events::DesktopEvent> {
    use crate::bridge::events::event_types;
    let field = |name: &str| stored.payload.get(name).cloned().unwrap_or_default();
    let (event_type, payload) = match stored.event_type.as_str() {
        "user_message" => (
            event_types::MESSAGE_DELTA,
            serde_json::json!({ "role": "user", "text": field("text") }),
        ),
        "assistant_delta" => (
            event_types::MESSAGE_DELTA,
            serde_json::json!({ "role": "assistant", "text": field("text") }),
        ),
        "assistant_completed" => (
            event_types::TASK_STATE,
            serde_json::json!({
                "taskId": stored.task_id,
                "status": "idle",
                "detail": { "completed": true, "fullText": field("fullText") },
            }),
        ),
        "tool_started" | "tool_updated" => (
            event_types::MESSAGE_DELTA,
            serde_json::json!({
                "toolCall": {
                    "toolCallId": field("toolCallId"),
                    "title": field("title"),
                    "kind": field("kind"),
                    "status": field("status"),
                    "startedAt": field("startedAt"),
                    "inputSummary": field("inputSummary"),
                    "inputRedacted": field("inputRedacted"),
                    "locations": field("locations"),
                }
            }),
        ),
        "tool_completed" => (
            event_types::MESSAGE_DELTA,
            serde_json::json!({
                "toolCall": {
                    "toolCallId": field("toolCallId"),
                    "status": field("outcome"),
                    "outcome": field("outcome"),
                    "resultSummary": field("summary"),
                    "resultRedacted": field("resultRedacted"),
                    "endedAt": field("endedAt"),
                    "durationMs": field("durationMs"),
                }
            }),
        ),
        "permission_requested" => (event_types::PERMISSION_REQUESTED, stored.payload.clone()),
        "plan_proposed" => (event_types::PLAN_UPDATED, stored.payload.clone()),
        "artifact_announced" => (event_types::ARTIFACT_AVAILABLE, stored.payload.clone()),
        "request_failed" => (
            event_types::ACTIVITY_UPDATED,
            serde_json::json!({
                "kind": "error",
                "code": field("code"),
                "retryable": true,
                "detail": format!(
                    "[{}] {}",
                    field("code").as_str().unwrap_or("ACP_REQUEST_FAILED"),
                    field("message").as_str().unwrap_or("Grok request failed")
                ),
            }),
        ),
        "turn_cancelled" => (
            event_types::TASK_STATE,
            serde_json::json!({
                "taskId": stored.task_id,
                "status": "idle",
                "detail": { "reason": "cancelled" }
            }),
        ),
        "process_exited" => (
            event_types::TASK_STATE,
            serde_json::json!({
                "taskId": stored.task_id,
                "status": "interrupted",
                "detail": { "reason": field("reason") },
            }),
        ),
        "session_ready" => (
            event_types::ACTIVITY_UPDATED,
            serde_json::json!({
                "kind": "status",
                "detail": "Grok session ready",
            }),
        ),
        _ => return None,
    };

    let mut event = crate::bridge::events::SessionEvent::new(
        event_type,
        stored.task_id.clone(),
        stored.session_id.clone(),
        stored.sequence,
        payload,
    )
    .build();
    event.timestamp = stored.persisted_at.clone();
    Some(event)
}

/// Build a reconnect snapshot without replaying thousands of tiny streaming
/// chunks over the desktop bridge. Consecutive assistant deltas are one
/// logical visible message segment, so they can be joined while retaining the
/// last persisted sequence as the authoritative cursor position.
pub fn map_stored_events_to_bridge_snapshot(
    stored_events: &[crate::domain::types::StoredEvent],
) -> Vec<crate::bridge::events::DesktopEvent> {
    use crate::domain::types::StoredEvent;

    fn flush_assistant(
        pending: &mut Option<(StoredEvent, String)>,
        events: &mut Vec<crate::bridge::events::DesktopEvent>,
    ) {
        let Some((mut stored, text)) = pending.take() else {
            return;
        };
        stored.payload = serde_json::json!({ "text": text });
        if let Some(event) = map_stored_event_to_bridge(&stored) {
            events.push(event);
        }
    }

    let mut events = Vec::new();
    let mut pending_assistant: Option<(StoredEvent, String)> = None;
    for stored in stored_events {
        if stored.event_type == "assistant_delta" {
            let text = stored
                .payload
                .get("text")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if let Some((pending, combined)) = pending_assistant.as_mut() {
                combined.push_str(text);
                pending.sequence = stored.sequence;
                pending.dedup_key.clone_from(&stored.dedup_key);
                pending.correlation_id.clone_from(&stored.correlation_id);
                pending.persisted_at.clone_from(&stored.persisted_at);
            } else {
                pending_assistant = Some((stored.clone(), text.to_owned()));
            }
            continue;
        }

        flush_assistant(&mut pending_assistant, &mut events);
        if let Some(event) = map_stored_event_to_bridge(stored) {
            events.push(event);
        }
    }
    flush_assistant(&mut pending_assistant, &mut events);
    events
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::task_runtime::permission::OperationCategory;

    #[test]
    fn has_side_effects_classification() {
        assert!(has_side_effects_kind("tool_started"));
        assert!(has_side_effects_kind("tool_completed"));
        assert!(has_side_effects_kind("permission_requested"));
        assert!(has_side_effects_kind("plan_proposed"));
        assert!(!has_side_effects_kind("assistant_delta"));
        assert!(!has_side_effects_kind("session_ready"));
        assert!(!has_side_effects_kind("process_exited"));
    }

    #[test]
    fn unknown_operations_strip_allow_actions_but_keep_denial() {
        use crate::modules::agent_runtime::events::PermissionOptionDescriptor;
        use crate::modules::task_runtime::permission::PermissionOptionAction;
        let options = vec![
            PermissionOptionDescriptor {
                option_id: "allow-1".into(),
                name: "Allow once".into(),
                kind: Some("allow_once".into()),
            },
            PermissionOptionDescriptor {
                option_id: "allow-scope-1".into(),
                name: "Allow always".into(),
                kind: Some("allow_always".into()),
            },
            PermissionOptionDescriptor {
                option_id: "reject-1".into(),
                name: "Reject".into(),
                kind: Some("reject_once".into()),
            },
        ];
        let restricted = restrict_options_for_category(&options, OperationCategory::Unknown);
        assert_eq!(restricted[0].action, PermissionOptionAction::Unknown);
        assert_eq!(restricted[1].action, PermissionOptionAction::Unknown);
        assert_eq!(restricted[2].action, PermissionOptionAction::Deny);
        // A classifiable write keeps its allow-once action.
        let write_restricted = restrict_options_for_category(&options, OperationCategory::Write);
        assert_eq!(
            write_restricted[0].action,
            PermissionOptionAction::AllowOnce
        );
    }

    #[test]
    fn request_failure_bridge_payload_is_structured_and_safe() {
        let stored = crate::domain::types::StoredEvent {
            dedup_key: "session-error:3".into(),
            session_id: crate::domain::types::SessionId::new("session-error"),
            task_id: crate::domain::types::TaskId::new("task-error"),
            sequence: 3,
            event_type: "request_failed".into(),
            payload: serde_json::json!({
                "code": "GROK_USAGE_EXHAUSTED",
                "message": "Grok Build usage balance exhausted",
                "token": "must-never-be-forwarded"
            }),
            correlation_id: None,
            persisted_at: crate::domain::types::utc_now(),
            has_side_effects: false,
        };

        let event = map_stored_event_to_bridge(&stored).expect("bridge event");
        assert_eq!(event.event_type, "activity.updated");
        assert_eq!(event.payload["kind"], "error");
        assert_eq!(event.payload["code"], "GROK_USAGE_EXHAUSTED");
        assert_eq!(event.payload["retryable"], true);
        assert!(!event
            .payload
            .to_string()
            .contains("must-never-be-forwarded"));
    }

    #[test]
    fn permission_operation_view_redacts_secret_arguments() {
        let operation = crate::modules::agent_runtime::events::PermissionOperationDescriptor {
            operation_kind: "process".into(),
            executable: Some("tool.exe".into()),
            args: vec![
                "--token".into(),
                "super-secret-value".into(),
                "--api-key=also-secret".into(),
            ],
            cwd: Some("C:/repo".into()),
            read_paths: vec![],
            write_paths: vec![],
        };
        let view = safe_operation_view(
            Some(&operation),
            crate::modules::task_runtime::permission::OperationCategory::Unknown,
        );
        let serialized = view.to_string();
        assert!(!serialized.contains("super-secret-value"));
        assert!(!serialized.contains("also-secret"));
        assert!(serialized.contains("[redacted]"));
    }

    #[test]
    fn redaction_covers_headers_bearer_jwt_and_high_entropy_values() {
        let operation = crate::modules::agent_runtime::events::PermissionOperationDescriptor {
            operation_kind: "process".into(),
            executable: Some("curl.exe".into()),
            args: vec![
                "-H".into(),
                "X-API-Key: sk-live-secret-key-123456".into(),
                "--header".into(),
                "Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.payload.signature".into(),
                "https://example.com".into(),
            ],
            cwd: Some("C:/repo".into()),
            read_paths: vec![],
            write_paths: vec![],
        };
        let view = safe_operation_view(
            Some(&operation),
            crate::modules::task_runtime::permission::OperationCategory::Unknown,
        );
        let serialized = view.to_string();
        assert!(!serialized.contains("sk-live-secret-key-123456"));
        assert!(!serialized.contains("eyJhbGciOiJIUzI1NiJ9"));
        assert!(!serialized.contains("payload.signature"));
        assert!(serialized.contains("[redacted]"));
        // The URL survives redaction because it is not a credential.
        assert!(serialized.contains("https://example.com"));
    }

    #[test]
    fn snapshot_mapping_compacts_streaming_deltas_without_losing_text_or_terminals() {
        let session_id = crate::domain::types::SessionId::new("session-compact");
        let task_id = crate::domain::types::TaskId::new("task-compact");
        let mut rows = vec![crate::domain::types::StoredEvent {
            dedup_key: "session-compact:1".into(),
            session_id: session_id.clone(),
            task_id: task_id.clone(),
            sequence: 1,
            event_type: "user_message".into(),
            payload: serde_json::json!({ "text": "keep the question" }),
            correlation_id: None,
            persisted_at: crate::domain::types::utc_now(),
            has_side_effects: false,
        }];
        for sequence in 2..=702 {
            rows.push(crate::domain::types::StoredEvent {
                dedup_key: format!("session-compact:{sequence}"),
                session_id: session_id.clone(),
                task_id: task_id.clone(),
                sequence,
                event_type: "assistant_delta".into(),
                payload: serde_json::json!({ "text": format!("chunk-{sequence};") }),
                correlation_id: None,
                persisted_at: crate::domain::types::utc_now(),
                has_side_effects: false,
            });
        }
        rows.push(crate::domain::types::StoredEvent {
            dedup_key: "session-compact:703".into(),
            session_id,
            task_id,
            sequence: 703,
            event_type: "process_exited".into(),
            payload: serde_json::json!({ "reason": "simulated crash" }),
            correlation_id: None,
            persisted_at: crate::domain::types::utc_now(),
            has_side_effects: false,
        });

        let events = map_stored_events_to_bridge_snapshot(&rows);
        assert_eq!(events.len(), 3, "user, compact assistant, process exit");
        assert_eq!(events[0].payload["text"], "keep the question");
        assert_eq!(events[1].seq, Some(702));
        let assistant = events[1].payload["text"].as_str().expect("assistant text");
        assert!(assistant.starts_with("chunk-2;"));
        assert!(assistant.ends_with("chunk-702;"));
        assert_eq!(events[2].event_type, "task.state");
        assert_eq!(events[2].payload["status"], "interrupted");
    }
}
