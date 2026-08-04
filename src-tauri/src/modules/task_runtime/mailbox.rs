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

        // 5. Update session binding's last_seq.
        if let Some(mut binding) = self.repo.get_binding_by_task(&self.task_id.0)? {
            if binding.session_id == session_id {
                binding.last_seq = seq;
                self.repo.update_binding(&binding)?;
            }
        }

        // 6. Publish to bridge (after persistence).
        let bridge_event = crate::bridge::events::SessionEvent::new(
            stored.event_type.clone(),
            self.task_id.clone(),
            session_id,
            seq,
            stored.payload.clone(),
        )
        .build();
        let _ = self.event_broadcaster.send(bridge_event);

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
}
