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
        let stored = StoredEvent {
            dedup_key,
            session_id: session_id.clone(),
            task_id: self.task_id.clone(),
            sequence: seq,
            event_type: event.event.kind_str().to_string(),
            payload: serde_json::to_value(&event.event).unwrap_or(serde_json::Value::Null),
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
            crate::modules::agent_runtime::AgentEvent::ProcessExited(exit) => {
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

    async fn handle_cancel(&self) -> Result<(), DomainError> {
        // Transition task from Running/WaitingPermission to Idle or Interrupted.
        // The actual process kill is handled by AgentRuntime; here we update
        // the domain state.
        if let Some(mut binding) = self.repo.get_binding_by_task(&self.task_id.0)? {
            if binding.state == crate::domain::types::SessionState::Active {
                binding.state = crate::domain::types::SessionState::Idle;
                self.repo.update_binding(&binding)?;
            }
        }
        Ok(())
    }
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
