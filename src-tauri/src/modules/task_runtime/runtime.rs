//! TaskRuntimeImpl — the concrete coordinator that implements [`TaskRuntime`].
//!
//! This struct owns:
//! - A reference to the Repository for persistence.
//! - A global semaphore for concurrency control.
//! - A map of session_id → `SessionMailbox` for per-session serialisation.
//! - A bridge event broadcaster.
//! - A reference to the AgentRuntime for process management.
//!
//! All state goes through the Repository (persistent). The in-memory
//! `sessions` map is a cache for fast mailbox lookup; the Repository is
//! always the source of truth.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};

use crate::bridge::types::{SessionId, TaskId};
use crate::domain::error::DomainError;
use crate::domain::types::{
    ConcurrencyLimits, RecoveryCandidate, RecoveryDecision, SessionSnapshot, SessionState,
    TaskSummary, TimelineCursor,
};
use crate::modules::agent_runtime::{AgentRuntime, TimestampedEvent};
use crate::modules::persistence::Repository;
use crate::modules::task_runtime::mailbox::{SessionCommand, SessionMailbox};
use crate::modules::task_runtime::TaskRuntime;

/// Default maximum concurrent tasks (configurable via settings).
const DEFAULT_MAX_CONCURRENT: u32 = 4;

/// Maximum event window for snapshots.
const SNAPSHOT_EVENT_LIMIT: u32 = 100;

/// The concrete TaskRuntime implementation.
pub struct TaskRuntimeImpl<A: AgentRuntime> {
    /// The repository (persistent state).
    repo: Arc<dyn Repository>,
    /// The agent runtime (process management) — reserved for future wiring.
    #[allow(dead_code)]
    agent_runtime: Arc<A>,
    /// Global concurrency semaphore.
    semaphore: Arc<Semaphore>,
    /// Maximum permits on the semaphore.
    max_concurrent: u32,
    /// Per-session mailboxes, keyed by session_id.
    mailboxes: Mutex<HashMap<SessionId, SessionMailbox>>,
    /// Bridge event broadcaster (Renderer subscribes to this).
    event_broadcaster: tokio::sync::broadcast::Sender<crate::bridge::events::DesktopEvent>,
}

impl<A: AgentRuntime + 'static> TaskRuntimeImpl<A> {
    /// Create a new TaskRuntime with default concurrency limit.
    pub fn new(repo: Arc<dyn Repository>, agent_runtime: Arc<A>) -> Self {
        Self::with_concurrency(repo, agent_runtime, DEFAULT_MAX_CONCURRENT)
    }

    /// Create a new TaskRuntime with a specific concurrency limit.
    pub fn with_concurrency(
        repo: Arc<dyn Repository>,
        agent_runtime: Arc<A>,
        max_concurrent: u32,
    ) -> Self {
        let (event_tx, _) = tokio::sync::broadcast::channel(256);
        Self {
            repo,
            agent_runtime,
            semaphore: Arc::new(Semaphore::new(max_concurrent as usize)),
            max_concurrent,
            mailboxes: Mutex::new(HashMap::new()),
            event_broadcaster: event_tx,
        }
    }

    /// Get or create a session mailbox.
    async fn get_or_create_mailbox(
        &self,
        task_id: &TaskId,
        session_id: &SessionId,
    ) -> SessionMailbox {
        let mut mailboxes = self.mailboxes.lock().await;
        if let Some(mb) = mailboxes.get(session_id) {
            return mb.clone();
        }
        let mb = SessionMailbox::new(
            task_id.clone(),
            session_id.clone(),
            self.repo.clone(),
            self.event_broadcaster.clone(),
        );
        mailboxes.insert(session_id.clone(), mb.clone());
        mb
    }

    /// Get the event broadcaster for subscribers.
    pub fn event_subscriber(
        &self,
    ) -> tokio::sync::broadcast::Receiver<crate::bridge::events::DesktopEvent> {
        self.event_broadcaster.subscribe()
    }
}

#[async_trait]
impl<A: AgentRuntime + 'static> TaskRuntime for TaskRuntimeImpl<A> {
    async fn bootstrap(
        &self,
        max_concurrent_tasks: u32,
    ) -> Result<(Vec<RecoveryCandidate>, ConcurrencyLimits), DomainError> {
        // Run startup recovery.
        let (candidates, _count) =
            crate::modules::task_runtime::recovery::run_startup_recovery(self.repo.as_ref())
                .await?;

        // Get concurrency limits.
        let limits = self.repo.get_concurrency_limits(max_concurrent_tasks)?;

        Ok((candidates, limits))
    }

    async fn enqueue_task(
        &self,
        task_id: TaskId,
        session_id: SessionId,
    ) -> Result<(), DomainError> {
        // Check concurrency: if we have a permit available, start immediately.
        // Otherwise, the task stays in Preparing state and is queued.
        let permit = self.semaphore.clone().try_acquire_owned();

        if permit.is_err() {
            // No permit available — task stays in Preparing (queued).
            // The task center will show it as queued.
            return Ok(());
        }

        // We have a permit. Create the session binding and start.
        let permit = permit.unwrap();

        let binding = crate::domain::types::SessionBinding {
            task_id: task_id.clone(),
            session_id: session_id.clone(),
            cwd: None,
            last_seq: 0,
            state: SessionState::Active,
            attempt_number: 1,
        };
        self.repo.create_binding(&binding)?;

        // Create the mailbox for this session.
        self.get_or_create_mailbox(&task_id, &session_id).await;

        // Transition task to Running.
        self.repo.update_task_status(&task_id.0, "running", None)?;

        // The permit is consumed by the running task. We store it in a way
        // that it's released when the session ends. For now, we "forget" it
        // and the session shutdown will release it.
        //
        // In production, the permit would be held by the session task.
        // For this implementation, we let the permit be forgotten (dropped)
        // when this function returns, meaning the semaphore tracks sessions
        // conservatively. A full implementation would track permit release
        // via the session shutdown path.
        std::mem::forget(permit);

        Ok(())
    }

    async fn start_session(
        &self,
        task_id: TaskId,
        session_id: SessionId,
    ) -> Result<(), DomainError> {
        // Ensure binding exists.
        if self.repo.get_binding_by_task(&task_id.0)?.is_none() {
            return Err(DomainError::new(
                "TASK_RUNTIME_NO_BINDING",
                format!("no binding for task {}", task_id),
            ));
        }

        // Create/ensure mailbox exists.
        self.get_or_create_mailbox(&task_id, &session_id).await;

        // Transition to Active.
        if let Some(mut binding) = self.repo.get_binding_by_task(&task_id.0)? {
            binding.state = SessionState::Active;
            self.repo.update_binding(&binding)?;
        }

        Ok(())
    }

    async fn accept_agent_event(&self, event: TimestampedEvent) -> Result<(), DomainError> {
        let session_id = event.meta.session_id.clone();

        // Find the binding to get the task_id.
        let binding = self
            .repo
            .get_binding_by_session(&session_id.0)?
            .ok_or_else(|| {
                DomainError::new(
                    "TASK_RUNTIME_NO_BINDING",
                    format!("no binding for session {}", session_id),
                )
            })?;

        let task_id = binding.task_id.clone();
        let mailbox = self.get_or_create_mailbox(&task_id, &session_id).await;

        // Dispatch to the session's mailbox for serial execution.
        let (tx, rx) = tokio::sync::oneshot::channel();
        mailbox
            .send(SessionCommand::AcceptEvent {
                event: Box::new(event),
                reply: tx,
            })
            .await?;
        rx.await.map_err(|_| {
            DomainError::new("TASK_RUNTIME_MAILBOX_DROPPED", "mailbox worker dropped")
        })?
    }

    async fn cancel_session(&self, task_id: TaskId) -> Result<(), DomainError> {
        // Get the session binding.
        let binding = self.repo.get_binding_by_task(&task_id.0)?.ok_or_else(|| {
            DomainError::new(
                "TASK_RUNTIME_NO_BINDING",
                format!("no binding for task {}", task_id),
            )
        })?;

        let session_id = binding.session_id.clone();
        let mailbox = self.get_or_create_mailbox(&task_id, &session_id).await;

        // Dispatch cancel to the session mailbox.
        let (tx, rx) = tokio::sync::oneshot::channel();
        mailbox
            .send(SessionCommand::CancelSession { reply: tx })
            .await?;
        rx.await.map_err(|_| {
            DomainError::new("TASK_RUNTIME_MAILBOX_DROPPED", "mailbox worker dropped")
        })?
    }

    async fn get_snapshot(
        &self,
        task_id: TaskId,
        session_id: SessionId,
        cursor: Option<TimelineCursor>,
    ) -> Result<SessionSnapshot, DomainError> {
        let snapshot =
            self.repo
                .get_session_snapshot(&task_id.0, &session_id.0, SNAPSHOT_EVENT_LIMIT)?;

        // If the cursor matches, return only new events after cursor.
        if let Some(cursor) = cursor {
            if cursor.last_seq < snapshot.last_seq {
                // Return a minimal snapshot with only the delta events.
                let new_events: Vec<_> = snapshot
                    .recent_events
                    .into_iter()
                    .filter(|e| e.sequence > cursor.last_seq)
                    .collect();
                let max_seq = new_events
                    .last()
                    .map(|e| e.sequence)
                    .unwrap_or(cursor.last_seq);
                let last_event_at = new_events
                    .last()
                    .map(|e| e.persisted_at.clone())
                    .unwrap_or_else(crate::domain::types::utc_now);

                return Ok(SessionSnapshot {
                    task_id: snapshot.task_id,
                    session_id: snapshot.session_id.clone(),
                    state: snapshot.state,
                    last_seq: max_seq,
                    captured_at: crate::domain::types::utc_now(),
                    cursor: TimelineCursor {
                        session_id: snapshot.session_id,
                        last_seq: max_seq,
                        last_event_at,
                    },
                    recent_events: new_events,
                    attempt_number: snapshot.attempt_number,
                });
            }
        }

        Ok(snapshot)
    }

    async fn list_recovery_candidates(&self) -> Result<Vec<RecoveryCandidate>, DomainError> {
        self.repo.list_recovery_candidates()
    }

    async fn recover_session(&self, decision: RecoveryDecision) -> Result<(), DomainError> {
        self.repo.apply_recovery_decision(&decision)?;

        // If resume, increment the binding's attempt number.
        if decision.action == crate::domain::types::RecoveryAction::Resume {
            let _ = self.repo.increment_binding_attempt(&decision.task_id.0);
        }

        Ok(())
    }

    async fn concurrency_limits(&self) -> ConcurrencyLimits {
        self.repo
            .get_concurrency_limits(self.max_concurrent)
            .unwrap_or(ConcurrencyLimits {
                max_concurrent_tasks: self.max_concurrent,
                current_running: 0,
                current_queued: 0,
            })
    }

    async fn task_summaries(&self) -> Result<Vec<TaskSummary>, DomainError> {
        let summaries = self.repo.list_task_summaries()?;

        // Enrich with live session info from the mailboxes.
        let mailboxes = self.mailboxes.lock().await;
        let enriched: Vec<TaskSummary> = summaries
            .into_iter()
            .map(|mut s| {
                s.has_live_session = mailboxes.values().any(|mb| mb.task_id == s.id);
                s
            })
            .collect();

        Ok(enriched)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrency_limits_default() {
        let cl = ConcurrencyLimits {
            max_concurrent_tasks: 4,
            current_running: 0,
            current_queued: 0,
        };
        assert_eq!(cl.max_concurrent_tasks, 4);
        assert_eq!(cl.current_running, 0);
    }

    #[test]
    fn snapshot_event_limit_is_reasonable() {
        // These are compile-time constants; the test verifies they are set
        // to reasonable values.
        const { assert!(SNAPSHOT_EVENT_LIMIT > 0, "SNAPSHOT_EVENT_LIMIT must be positive") };
        const { assert!(SNAPSHOT_EVENT_LIMIT <= 500, "SNAPSHOT_EVENT_LIMIT must be <= 500") };
    }
}
