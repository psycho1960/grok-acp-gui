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
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};

use crate::adapters::filesystem::{
    canonicalize_existing_directory, validate_linked_worktree_metadata,
};
use crate::bridge::types::{SessionId, TaskId};
use crate::domain::error::DomainError;
use crate::domain::types::{
    ConcurrencyLimits, RecoveryCandidate, RecoveryDecision, SessionSnapshot, SessionState, Task,
    TaskStatus, TaskSummary, TimelineCursor, WorkspaceKind, WorktreeOwnership, WorktreeState,
};
use crate::modules::agent_runtime::{
    AgentRuntime, RuntimeConfig, RuntimeState, TimestampedEvent, WorkspaceContext,
};
use crate::modules::persistence::Repository;
use crate::modules::task_runtime::mailbox::{SessionCommand, SessionMailbox};
use crate::modules::task_runtime::{SessionConfiguration, SessionConfigurationResult, TaskRuntime};

/// Default maximum concurrent tasks (configurable via settings).
const DEFAULT_MAX_CONCURRENT: u32 = 4;

/// Default permission request timeout (300 seconds).
const DEFAULT_PERMISSION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Maximum event window for snapshots.
const SNAPSHOT_EVENT_LIMIT: u32 = 500;

async fn approval_resolution_for_session(
    locks: &Mutex<HashMap<SessionId, Arc<Mutex<()>>>>,
    session_id: &SessionId,
) -> Arc<Mutex<()>> {
    locks
        .lock()
        .await
        .entry(session_id.clone())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

/// The concrete TaskRuntime implementation.
pub struct TaskRuntimeImpl<A: AgentRuntime> {
    /// The repository (persistent state).
    repo: Arc<dyn Repository>,
    /// The agent runtime (process management).
    agent_runtime: Arc<A>,
    /// Guards the single runtime → task-runtime event pump.
    forwarding_started: AtomicBool,
    /// Global concurrency semaphore.
    semaphore: Arc<Semaphore>,
    /// Maximum permits on the semaphore.
    max_concurrent: u32,
    /// How long a pending permission may stay unresolved before the mailbox
    /// worker auto-rejects it with the ACP deny option.
    permission_timeout: std::time::Duration,
    /// Per-session mailboxes, keyed by session_id.
    mailboxes: Mutex<HashMap<SessionId, SessionMailbox>>,
    /// Per-process generation offset used when a persisted session is
    /// restarted but AgentRuntime begins its local sequence at one again.
    sequence_offsets: Mutex<HashMap<SessionId, u64>>,
    /// Per-session semaphore permits. The permit is released back to the
    /// semaphore when the entry is removed (e.g. on session cancel/completion).
    permits: Mutex<HashMap<SessionId, tokio::sync::OwnedSemaphorePermit>>,
    /// Per-session serialization of ACP resolution submission with the
    /// corresponding database transition. Sessions stay isolated while a
    /// double-click within one session still cannot send an option twice.
    approval_resolutions: Mutex<HashMap<SessionId, Arc<Mutex<()>>>>,
    /// Serializes workspace configuration and session start per task.
    task_operations: Mutex<HashMap<TaskId, Arc<Mutex<()>>>>,
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
        Self::with_concurrency_and_permission_timeout(
            repo,
            agent_runtime,
            max_concurrent,
            DEFAULT_PERMISSION_TIMEOUT,
        )
    }

    /// Create a new TaskRuntime with a specific concurrency limit and
    /// permission timeout. Tests inject short timeouts so the auto-reject
    /// path is exercised without waiting 300 seconds in real time.
    pub fn with_concurrency_and_permission_timeout(
        repo: Arc<dyn Repository>,
        agent_runtime: Arc<A>,
        max_concurrent: u32,
        permission_timeout: std::time::Duration,
    ) -> Self {
        let (event_tx, _) = tokio::sync::broadcast::channel(256);
        Self {
            repo,
            agent_runtime,
            forwarding_started: AtomicBool::new(false),
            semaphore: Arc::new(Semaphore::new(max_concurrent as usize)),
            max_concurrent,
            permission_timeout,
            mailboxes: Mutex::new(HashMap::new()),
            sequence_offsets: Mutex::new(HashMap::new()),
            permits: Mutex::new(HashMap::new()),
            approval_resolutions: Mutex::new(HashMap::new()),
            task_operations: Mutex::new(HashMap::new()),
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
        let approval_resolution = self.approval_resolution_for(session_id).await;
        let mb = SessionMailbox::new(
            task_id.clone(),
            session_id.clone(),
            self.repo.clone(),
            self.event_broadcaster.clone(),
            self.agent_runtime.clone(),
            approval_resolution,
            self.permission_timeout,
        );
        mailboxes.insert(session_id.clone(), mb.clone());
        mb
    }

    async fn approval_resolution_for(&self, session_id: &SessionId) -> Arc<Mutex<()>> {
        approval_resolution_for_session(&self.approval_resolutions, session_id).await
    }

    async fn task_operation_lock(&self, task_id: &TaskId) -> Arc<Mutex<()>> {
        self.task_operations
            .lock()
            .await
            .entry(task_id.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// Get the event broadcaster for subscribers.
    pub fn event_subscriber(
        &self,
    ) -> tokio::sync::broadcast::Receiver<crate::bridge::events::DesktopEvent> {
        self.event_broadcaster.subscribe()
    }

    /// Start the single event pump that persists normalized AgentRuntime
    /// events before publishing task-scoped DesktopEvents.
    pub fn spawn_agent_event_forwarder(self: &Arc<Self>) {
        if self
            .forwarding_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        let mut events = self.agent_runtime.subscribe();
        let runtime = Arc::downgrade(self);
        tauri::async_runtime::spawn(async move {
            while let Some(event) = events.recv().await {
                let Some(runtime) = runtime.upgrade() else {
                    break;
                };
                if let Err(error) = runtime.accept_agent_event(event).await {
                    if error.code != crate::domain::error::codes::EVENT_DUPLICATE {
                        eprintln!("task runtime rejected an agent event ({})", error.code);
                    }
                }
            }
        });
    }
}

fn worktree_not_ready(message: &'static str) -> DomainError {
    DomainError::new(crate::domain::error::codes::WORKTREE_NOT_READY, message)
}

/// Resolve the persisted policy to one canonical directory. Managed-root
/// configuration is owned by GAG-011; until then, the registry relationship,
/// repository identity and a strict non-project path are the available trust
/// boundary. This deliberately rejects nested/project-equivalent paths.
fn resolve_task_workspace(repo: &dyn Repository, task: &Task) -> Result<PathBuf, DomainError> {
    let project = repo.get_project(&task.project_id.0)?;
    let project_path = canonicalize_existing_directory(std::path::Path::new(&project.path))
        .map_err(|_| {
            DomainError::new(
                "TASK_RUNTIME_INVALID_CWD",
                "project workspace is not an existing absolute directory",
            )
        })?;

    if task.workspace_kind != WorkspaceKind::Worktree {
        return Ok(project_path);
    }

    let candidates: Vec<_> = repo
        .list_worktrees_by_task(&task.id.0)?
        .into_iter()
        .filter(|worktree| {
            worktree.ownership == WorktreeOwnership::Managed
                && matches!(worktree.state, WorktreeState::Ready | WorktreeState::Dirty)
        })
        .collect();
    if candidates.len() != 1 {
        return Err(worktree_not_ready(
            "isolated workspace has not been created and verified",
        ));
    }

    let worktree = &candidates[0];
    let trusted_repo_root = project.repo_root.as_deref().unwrap_or(&project.path);
    let project_repo_root =
        canonicalize_existing_directory(std::path::Path::new(trusted_repo_root))
            .map_err(|_| worktree_not_ready("project repository identity is not verifiable"))?;
    let recorded_repo_root =
        canonicalize_existing_directory(std::path::Path::new(&worktree.repo_root))
            .map_err(|_| worktree_not_ready("worktree repository identity is not verifiable"))?;
    let worktree_path = canonicalize_existing_directory(std::path::Path::new(&worktree.path))
        .map_err(|_| {
            worktree_not_ready("isolated workspace path does not exist or is not ready")
        })?;

    if recorded_repo_root != project_repo_root
        || worktree_path == project_path
        || worktree_path.starts_with(&project_path)
        || project_path.starts_with(&worktree_path)
        || worktree_path.starts_with(&project_repo_root)
        || project_repo_root.starts_with(&worktree_path)
    {
        return Err(worktree_not_ready(
            "isolated workspace path is not safely separated from the project checkout",
        ));
    }

    validate_linked_worktree_metadata(&project_repo_root, &worktree_path)
        .map_err(|_| worktree_not_ready("isolated workspace Git registration is not verifiable"))?;

    Ok(worktree_path)
}

fn workspace_available(repo: &dyn Repository, task: &Task) -> bool {
    resolve_task_workspace(repo, task).is_ok()
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
        // Check concurrency: try to acquire a permit.
        let permit = match self.semaphore.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => {
                // No permit available — task stays in Preparing (queued).
                // Return a specific error so the caller can distinguish
                // "queued" from "running".
                return Err(DomainError::new(
                    crate::domain::error::codes::CONCURRENCY_LIMIT_EXCEEDED,
                    format!(
                        "concurrency limit reached ({}/{} running); task {} queued",
                        self.max_concurrent, self.max_concurrent, task_id
                    ),
                ));
            }
        };

        // This transaction races safely with worktree cleanup: either the
        // task becomes running first, or cleanup marks the worktree closing.
        // The unrecorded permit is released automatically if the claim fails.
        self.repo.begin_task_execution(&task_id.0)?;

        // Store the permit so it is released when this session ends.
        {
            let mut permits = self.permits.lock().await;
            permits.insert(session_id.clone(), permit);
        }

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

        Ok(())
    }

    async fn start_session(
        &self,
        task_id: TaskId,
        session_id: SessionId,
    ) -> Result<(), DomainError> {
        let task_lock = self.task_operation_lock(&task_id).await;
        let _task_guard = task_lock.lock().await;

        // Ensure binding exists and belongs to the requested session.
        let Some(mut binding) = self.repo.get_binding_by_task(&task_id.0)? else {
            return Err(DomainError::new(
                "TASK_RUNTIME_NO_BINDING",
                format!("no binding for task {}", task_id),
            ));
        };
        if binding.session_id != session_id {
            return Err(DomainError::new(
                "TASK_RUNTIME_BINDING_MISMATCH",
                format!("task {} is bound to a different session", task_id),
            ));
        }

        self.repo.begin_task_execution(&task_id.0)?;

        // A stopped/idle task may retain a reusable ACP process after its
        // active-task permit was released. Reacquire the slot before the next
        // turn so cancellation recovery does not bypass concurrency limits.
        {
            let mut permits = self.permits.lock().await;
            if !permits.contains_key(&session_id) {
                let permit = self.semaphore.clone().try_acquire_owned().map_err(|_| {
                    DomainError::new(
                        crate::domain::error::codes::CONCURRENCY_LIMIT_EXCEEDED,
                        format!(
                            "concurrency limit reached ({}/{} running); task {} remains idle",
                            self.max_concurrent, self.max_concurrent, task_id
                        ),
                    )
                })?;
                permits.insert(session_id.clone(), permit);
            }
        }

        // Create/ensure mailbox exists.
        self.get_or_create_mailbox(&task_id, &session_id).await;

        // Derive one canonical process cwd from the persisted policy.
        let task = self.repo.get_task(&task_id.0)?;
        let cwd = match resolve_task_workspace(self.repo.as_ref(), &task) {
            Ok(cwd) => cwd,
            Err(error) => {
                binding.state = SessionState::Disconnected;
                self.repo.update_binding(&binding)?;
                self.permits.lock().await.remove(&session_id);
                if matches!(task.status, TaskStatus::Preparing | TaskStatus::Running) {
                    let _ = self.repo.update_task_status(
                        &task_id.0,
                        "idle",
                        Some("workspace is not ready"),
                    );
                }
                return Err(error);
            }
        };

        let runtime_state = self.agent_runtime.session_state(&session_id);
        let previous_cwd = binding
            .cwd
            .as_deref()
            .and_then(|path| canonicalize_existing_directory(std::path::Path::new(path)).ok());
        if matches!(
            runtime_state,
            Some(RuntimeState::Ready | RuntimeState::Busy)
        ) && previous_cwd.as_ref() != Some(&cwd)
        {
            self.agent_runtime
                .shutdown(
                    session_id.clone(),
                    "persisted workspace no longer matches runtime",
                )
                .await;
            binding.cwd = None;
            binding.state = SessionState::Disconnected;
            self.repo.update_binding(&binding)?;
            self.permits.lock().await.remove(&session_id);
            if matches!(task.status, TaskStatus::Preparing | TaskStatus::Running) {
                let _ = self.repo.update_task_status(
                    &task_id.0,
                    "idle",
                    Some("workspace identity changed"),
                );
            }
            return Err(worktree_not_ready(
                "persisted workspace no longer matches the retained session",
            ));
        }

        binding.cwd = Some(cwd.to_string_lossy().into_owned());
        binding.state = SessionState::Active;
        self.repo.update_binding(&binding)?;

        let start_result = match runtime_state {
            Some(RuntimeState::Ready | RuntimeState::Busy) => Ok(()),
            Some(state) if state.is_live() => Err(DomainError::new(
                crate::domain::error::codes::DOMAIN_ILLEGAL_TRANSITION,
                format!("session is not ready ({state})"),
            )),
            _ => {
                let runtime_config = RuntimeConfig {
                    model: task.model.clone(),
                    ..RuntimeConfig::default()
                };
                self.agent_runtime
                    .start(session_id, WorkspaceContext { cwd }, &runtime_config)
                    .await
                    .map(|_| ())
            }
        };
        if let Err(error) = start_result {
            binding.state = SessionState::Disconnected;
            self.repo.update_binding(&binding)?;
            self.permits.lock().await.remove(&binding.session_id);
            if matches!(task.status, TaskStatus::Preparing | TaskStatus::Running) {
                let _ =
                    self.repo
                        .update_task_status(&task_id.0, "idle", Some("session start failed"));
            }
            return Err(error);
        }

        Ok(())
    }

    async fn configure_session(
        &self,
        task_id: TaskId,
        configuration: SessionConfiguration,
    ) -> Result<SessionConfigurationResult, DomainError> {
        let task_lock = self.task_operation_lock(&task_id).await;
        let _task_guard = task_lock.lock().await;

        let mut task = self.repo.get_task(&task_id.0)?;
        let original_workspace_kind = task.workspace_kind;
        let mode_was_supplied = configuration.mode.is_some();

        if let Some(mode) = configuration.mode {
            task.mode = mode;
        }
        if let Some(model) = configuration.model {
            task.model = model;
        }
        if let Some(reasoning) = configuration.reasoning {
            task.reasoning = reasoning;
        }
        match configuration.workspace_strategy {
            Some(kind) => task.workspace_kind = kind,
            None if mode_was_supplied => {
                if let Some(kind) = WorkspaceKind::default_for_known_mode(task.mode.as_deref()) {
                    task.workspace_kind = kind;
                }
            }
            None => {}
        }

        let workspace_changed = task.workspace_kind != original_workspace_kind;
        let binding = self.repo.get_binding_by_task(&task_id.0)?;
        if workspace_changed
            && matches!(
                task.status,
                TaskStatus::Preparing
                    | TaskStatus::Running
                    | TaskStatus::WaitingPermission
                    | TaskStatus::Integrating
            )
        {
            return Err(DomainError::new(
                crate::domain::error::codes::DOMAIN_ILLEGAL_TRANSITION,
                "当前 Turn 仍在运行，不能切换工作区策略；请先停止并确认后重试",
            ));
        }

        // Disconnect the old runtime under the same task lock used by
        // start_session. Persisting this before the task row means a failure
        // cannot expose a new policy while a stale cwd remains reusable.
        if workspace_changed {
            if let Some(mut binding) = binding {
                self.agent_runtime
                    .shutdown(binding.session_id.clone(), "workspace policy changed")
                    .await;
                binding.cwd = None;
                binding.state = SessionState::Disconnected;
                self.repo.update_binding(&binding)?;
            }
        }

        task.updated_at = crate::domain::types::utc_now();
        self.repo.update_task_configuration(
            &task.id.0,
            task.workspace_kind,
            task.mode.as_deref(),
            task.model.as_deref(),
            task.reasoning.as_deref(),
            &task.updated_at,
        )?;
        // Reload the row so a concurrent status/recovery update remains the
        // authoritative value returned to the bridge.
        let task = self.repo.get_task(&task.id.0)?;
        let available = workspace_available(self.repo.as_ref(), &task);
        Ok(SessionConfigurationResult {
            task,
            workspace_available: available,
        })
    }

    async fn workspace_snapshot(
        &self,
        task_id: TaskId,
    ) -> Result<SessionConfigurationResult, DomainError> {
        let task_lock = self.task_operation_lock(&task_id).await;
        let _task_guard = task_lock.lock().await;
        let task = self.repo.get_task(&task_id.0)?;
        let available = workspace_available(self.repo.as_ref(), &task);
        Ok(SessionConfigurationResult {
            task,
            workspace_available: available,
        })
    }

    async fn accept_agent_event(&self, mut event: TimestampedEvent) -> Result<(), DomainError> {
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
        let raw_sequence = event.meta.sequence;
        let offset = {
            let mut offsets = self.sequence_offsets.lock().await;
            if matches!(
                &event.event,
                crate::modules::agent_runtime::AgentEvent::SessionReady(_)
            ) && raw_sequence <= binding.last_seq
            {
                offsets.insert(session_id.clone(), binding.last_seq);
                binding.last_seq
            } else {
                offsets.get(&session_id).copied().unwrap_or(0)
            }
        };
        event.meta.sequence = raw_sequence.saturating_add(offset);
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

        // Close any nested ACP permission/Plan request before making the
        // local task idle. This prevents the agent from retaining a suspended
        // previous turn after the UI has cancelled it.
        self.agent_runtime.cancel(session_id.clone(), None).await;

        // Update task status back to idle so it can be re-enqueued.
        self.repo
            .update_task_status(&task_id.0, "idle", Some("cancelled by user"))?;

        // Update binding state.
        let mut binding = binding;
        binding.state = SessionState::Idle;
        self.repo.update_binding(&binding)?;

        let mailbox = self.get_or_create_mailbox(&task_id, &session_id).await;

        // Dispatch cancel to the session mailbox.
        let (tx, rx) = tokio::sync::oneshot::channel();
        mailbox
            .send(SessionCommand::CancelSession { reply: tx })
            .await?;
        rx.await.map_err(|_| {
            DomainError::new("TASK_RUNTIME_MAILBOX_DROPPED", "mailbox worker dropped")
        })??;

        // The concurrency permit represents an active task slot, not ACP
        // process ownership. Keep the Ready session reusable, but release the
        // slot once cancellation has been durably applied.
        self.permits.lock().await.remove(&session_id);
        Ok(())
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

    async fn resolve_permission(
        &self,
        request: crate::modules::task_runtime::permission::PermissionResolutionRequest,
    ) -> Result<crate::modules::task_runtime::permission::PermissionState, DomainError> {
        use crate::modules::agent_runtime::requests::ResolvePermissionRequest;
        use crate::modules::agent_runtime::ClientRequest;
        use crate::modules::task_runtime::permission::{
            PermissionDecision, PermissionOptionAction, PermissionState,
        };

        let approval_resolution = self.approval_resolution_for(&request.session_id).await;
        let _resolution = approval_resolution.lock().await;
        let pending = self
            .repo
            .get_permission(&request.request_id, &request.session_id.0)?;
        let expected_plan_version =
            (request.expected_version != 0).then_some(request.expected_version);
        if pending.task_id != request.task_id
            || pending.session_id != request.session_id
            || pending.correlation_id != request.correlation_id
            || pending.plan_version != expected_plan_version
        {
            return Err(DomainError::new(
                crate::domain::error::codes::PERMISSION_CONTEXT_MISMATCH,
                "Permission request context or Plan version changed",
            ));
        }
        let action = pending
            .options
            .iter()
            .find(|option| option.option_id == request.option_id)
            .map(|option| option.action)
            .unwrap_or(PermissionOptionAction::Unknown);
        if action == PermissionOptionAction::Unknown {
            return Err(DomainError::new(
                crate::domain::error::codes::PERMISSION_DENIED,
                "Permission option has no explicit ACP action",
            ));
        }
        // The Plan gate comes before every other state check: while the
        // current Plan is not approved for this permission's version, no
        // allow-type option may be forwarded — including on records that a
        // newer Plan already expired. Denial remains available because
        // rejection is always safe.
        if action != PermissionOptionAction::Deny
            && self.repo.get_task(&request.task_id.0)?.mode.as_deref() == Some("plan")
        {
            let active_plan = self.repo.latest_plan(&request.task_id.0)?;
            let approved = active_plan.as_ref().is_some_and(|plan| {
                plan.state == crate::modules::task_runtime::plan::PlanState::Approved
                    && Some(plan.version) == pending.plan_version
            });
            if !approved {
                return Err(DomainError::new(
                    crate::domain::error::codes::PLAN_NOT_APPROVED,
                    "Plan is not approved for this permission request",
                ));
            }
        }
        if pending.state != PermissionState::Requested {
            return Err(DomainError::new(
                crate::domain::error::codes::PERMISSION_ALREADY_RESOLVED,
                "Permission request is no longer pending",
            ));
        }
        // Fail closed: operations that cannot be safely classified (missing
        // cwd, escaped paths, unknown category) are never authorizable. Only
        // denial remains available because rejection is always safe.
        if pending.category == crate::modules::task_runtime::permission::OperationCategory::Unknown
            && action != PermissionOptionAction::Deny
        {
            return Err(DomainError::new(
                crate::domain::error::codes::PERMISSION_DENIED,
                "Operation cannot be classified safely; approval is blocked",
            ));
        }
        if action == PermissionOptionAction::AllowScope
            && pending.category
                != crate::modules::task_runtime::permission::OperationCategory::ReadOnly
        {
            return Err(DomainError::new(
                crate::domain::error::codes::PERMISSION_DENIED,
                "Persistent approval is restricted to exact read-only operations",
            ));
        }
        // Expiry must be checked before the ACP option is sent: an expired
        // request must never reach the agent as an approval. The resolution
        // mutex serializes this check with the send, leaving no window for a
        // concurrent decision to slip in.
        let now_epoch_seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if pending.expires_at_epoch_seconds < now_epoch_seconds {
            return Err(DomainError::new(
                crate::domain::error::codes::PERMISSION_EXPIRED,
                "Permission request expired before it could be resolved",
            ));
        }
        let binding = self
            .repo
            .get_binding_by_task(&request.task_id.0)?
            .ok_or_else(|| {
                DomainError::new(
                    crate::domain::error::codes::PERMISSION_CONTEXT_MISMATCH,
                    "Session binding is missing",
                )
            })?;
        if binding.session_id != request.session_id
            || binding.cwd.as_deref() != Some(&pending.workspace)
        {
            return Err(DomainError::new(
                crate::domain::error::codes::PERMISSION_CONTEXT_MISMATCH,
                "Workspace binding changed",
            ));
        }

        let decided_at = crate::domain::types::utc_now();
        let decided = self.repo.decide_permission(&PermissionDecision {
            request_id: request.request_id.clone(),
            task_id: request.task_id.clone(),
            session_id: request.session_id.clone(),
            correlation_id: request.correlation_id,
            workspace: pending.workspace,
            expected_plan_version,
            option_id: request.option_id.clone(),
            decided_at,
            decided_at_epoch_seconds: now_epoch_seconds,
        })?;
        // A durable decision must exist before the Agent receives an allow
        // response. If SQLite rejects the transaction, fail closed without
        // emitting any ACP approval that could authorize external I/O.
        if let Err(error) = self
            .agent_runtime
            .send(
                request.session_id.clone(),
                ClientRequest::ResolvePermission(ResolvePermissionRequest {
                    request_id: request.request_id.clone(),
                    option_id: request.option_id.clone(),
                }),
            )
            .await
        {
            if self
                .repo
                .revert_permission_decision(&request.request_id, &request.session_id.0)
                .is_err()
            {
                let _ = self.repo.expire_session_permissions(
                    &request.session_id.0,
                    "ACP permission delivery failed",
                );
            }
            return Err(error);
        }
        self.repo
            .update_task_status(&request.task_id.0, "running", None)?;
        Ok(decided.state)
    }

    async fn resolve_plan(
        &self,
        request: crate::modules::task_runtime::plan::PlanResolutionRequest,
    ) -> Result<crate::modules::task_runtime::plan::PlanState, DomainError> {
        use crate::modules::agent_runtime::requests::ResolvePlanRequest;
        use crate::modules::agent_runtime::ClientRequest;
        use crate::modules::task_runtime::plan::{PlanDecision, PlanOptionAction, PlanState};

        let approval_resolution = self.approval_resolution_for(&request.session_id).await;
        let _resolution = approval_resolution.lock().await;
        let pending = self
            .repo
            .get_plan(&request.request_id, &request.session_id.0)?;
        if pending.task_id != request.task_id
            || pending.session_id != request.session_id
            || pending.correlation_id != request.correlation_id
            || pending.version != request.expected_version
        {
            return Err(DomainError::new(
                crate::domain::error::codes::PLAN_VERSION_MISMATCH,
                "Plan context or version changed",
            ));
        }
        if pending.state != PlanState::Proposed {
            return Err(DomainError::new(
                crate::domain::error::codes::PERMISSION_ALREADY_RESOLVED,
                "Plan request is no longer pending",
            ));
        }
        let action = pending
            .options
            .iter()
            .find(|option| option.option_id == request.option_id)
            .map(|option| option.action)
            .unwrap_or(PlanOptionAction::Unknown);
        if action == PlanOptionAction::Unknown {
            return Err(DomainError::new(
                crate::domain::error::codes::PERMISSION_DENIED,
                "Plan option has no explicit ACP action",
            ));
        }
        let binding = self
            .repo
            .get_binding_by_task(&request.task_id.0)?
            .ok_or_else(|| {
                DomainError::new(
                    crate::domain::error::codes::PERMISSION_CONTEXT_MISMATCH,
                    "Session binding is missing",
                )
            })?;
        if binding.session_id != request.session_id
            || binding.cwd.as_deref() != Some(&pending.workspace)
        {
            return Err(DomainError::new(
                crate::domain::error::codes::PERMISSION_CONTEXT_MISMATCH,
                "Workspace binding changed",
            ));
        }

        let decided = self.repo.decide_plan(&PlanDecision {
            request_id: request.request_id.clone(),
            task_id: request.task_id.clone(),
            session_id: request.session_id.clone(),
            correlation_id: request.correlation_id,
            workspace: pending.workspace,
            expected_version: request.expected_version,
            option_id: request.option_id.clone(),
            decided_at: crate::domain::types::utc_now(),
        })?;
        // Persist before responding to the Agent for the same reason as a
        // permission decision: an ACP approval must never outlive its local
        // audit trail when storage fails.
        if let Err(error) = self
            .agent_runtime
            .send(
                request.session_id.clone(),
                ClientRequest::ResolvePlan(ResolvePlanRequest {
                    request_id: request.request_id.clone(),
                    option_id: request.option_id.clone(),
                }),
            )
            .await
        {
            if self
                .repo
                .revert_plan_decision(&request.request_id, &request.session_id.0)
                .is_err()
            {
                let _ = self
                    .repo
                    .supersede_session_plans(&request.session_id.0, "ACP Plan delivery failed");
            }
            return Err(error);
        }
        self.repo.update_task_status(
            &request.task_id.0,
            if decided.state == PlanState::Rejected {
                "idle"
            } else {
                "running"
            },
            None,
        )?;
        Ok(decided.state)
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
        const {
            assert!(
                SNAPSHOT_EVENT_LIMIT > 0,
                "SNAPSHOT_EVENT_LIMIT must be positive"
            )
        };
        const {
            assert!(
                SNAPSHOT_EVENT_LIMIT <= 500,
                "SNAPSHOT_EVENT_LIMIT must be <= 500"
            )
        };
    }

    #[tokio::test]
    async fn approval_resolution_locks_are_isolated_by_session() {
        let locks = Mutex::new(HashMap::new());
        let first = approval_resolution_for_session(&locks, &SessionId::new("session-a")).await;
        let second = approval_resolution_for_session(&locks, &SessionId::new("session-b")).await;

        assert!(
            !Arc::ptr_eq(&first, &second),
            "different sessions must not share an approval-resolution lock"
        );
        let _first_guard = first.lock().await;
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(50), second.lock())
                .await
                .is_ok(),
            "a busy session must not block a different session's resolution"
        );
    }
}
