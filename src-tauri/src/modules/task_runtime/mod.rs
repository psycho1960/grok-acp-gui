//! MOD-TASK-RUNTIME: Task lifecycle, concurrency coordination, session supervision,
//! event ordering, and crash recovery.
//!
//! This is the deep module that sits between the Bridge and AgentRuntime.
//! Every Renderer action involving tasks goes through the [`TaskRuntime`] trait.
//!
//! # Architecture
//! - A **global semaphore** (tokio Semaphore) enforces max concurrent tasks.
//! - Each session has its own **mailbox** — a tokio mpsc channel for serialising
//!   domain mutations within a single session.
//! - Events are **deduplicated**, **gap-checked**, **persisted**, then **published**.
//! - On app restart, `bootstrap()` detects interrupted tasks and produces
//!   RecoveryCandidates for the renderer.

pub mod mailbox;
pub mod permission;
pub mod plan;
pub mod recovery;
pub mod runtime;

use crate::bridge::types::{SessionId, TaskId};
use crate::domain::error::DomainError;
use crate::domain::types::{
    ConcurrencyLimits, RecoveryCandidate, RecoveryDecision, SessionSnapshot, Task, TaskSummary,
    WorkspaceKind,
};
use async_trait::async_trait;

// Re-export the concrete runtime.
pub use runtime::TaskRuntimeImpl;

/// Validated settings patch passed from DesktopBridge into TaskRuntime.
/// Nested options on nullable text fields distinguish omission from clearing.
#[derive(Debug, Clone, Default)]
pub struct SessionConfiguration {
    pub mode: Option<Option<String>>,
    pub model: Option<Option<String>>,
    pub reasoning: Option<Option<String>>,
    pub workspace_strategy: Option<WorkspaceKind>,
}

#[derive(Debug, Clone)]
pub struct SessionConfigurationResult {
    pub task: Task,
    pub workspace_available: bool,
}

/// The public Interface of the Task Runtime module.
///
/// # Concurrency guarantees
/// - Per-session domain mutations are **serialised** through that session's mailbox.
/// - Cross-session operations (listing tasks, concurrency queries) are non-blocking.
/// - `accept_agent_event` is the only path for writing session events to the DB;
///   it validates sequence, dedup key, persists in a transaction, then publishes.
#[async_trait]
pub trait TaskRuntime: Send + Sync {
    /// Bootstrap: perform startup recovery and return full state snapshot.
    async fn bootstrap(
        &self,
        max_concurrent_tasks: u32,
    ) -> Result<(Vec<RecoveryCandidate>, ConcurrencyLimits), DomainError>;

    /// Enqueue a task for execution. If below the concurrency limit, creates
    /// a session binding and transitions the task to Running immediately.
    /// Returns `CONCURRENCY_LIMIT_EXCEEDED` when the limit is reached — the
    /// caller should keep the task in Preparing state and retry later.
    async fn enqueue_task(&self, task_id: TaskId, session_id: SessionId)
        -> Result<(), DomainError>;

    /// Start a session: spawn the agent runtime process and bind it to the task.
    async fn start_session(
        &self,
        task_id: TaskId,
        session_id: SessionId,
    ) -> Result<(), DomainError>;

    /// Persist one validated settings patch as a single task-row update.
    /// Workspace changes are serialized with session start for the same task.
    async fn configure_session(
        &self,
        task_id: TaskId,
        configuration: SessionConfiguration,
    ) -> Result<SessionConfigurationResult, DomainError>;

    /// Return task settings and verified availability from one serialized
    /// policy snapshot.
    async fn workspace_snapshot(
        &self,
        task_id: TaskId,
    ) -> Result<SessionConfigurationResult, DomainError>;

    /// Accept a processed agent event. Validates sequence, deduplicates,
    /// persists in a transaction, updates task/binding state, then publishes.
    async fn accept_agent_event(
        &self,
        event: crate::modules::agent_runtime::TimestampedEvent,
    ) -> Result<(), DomainError>;

    /// Cancel the current session/turn for a task. Idempotent.
    async fn cancel_session(&self, task_id: TaskId) -> Result<(), DomainError>;

    /// Get a session snapshot for Renderer reconnection.
    async fn get_snapshot(
        &self,
        task_id: TaskId,
        session_id: SessionId,
        cursor: Option<crate::domain::types::TimelineCursor>,
    ) -> Result<SessionSnapshot, DomainError>;

    /// List recovery candidates after startup recovery scan.
    async fn list_recovery_candidates(&self) -> Result<Vec<RecoveryCandidate>, DomainError>;

    /// Apply a user recovery decision.
    async fn recover_session(&self, decision: RecoveryDecision) -> Result<(), DomainError>;

    /// Get current concurrency state.
    async fn concurrency_limits(&self) -> ConcurrencyLimits;

    /// Get lightweight task summaries for the task center.
    async fn task_summaries(&self) -> Result<Vec<TaskSummary>, DomainError>;

    /// Atomically resolve a pending permission, then forward the exact ACP
    /// option ID. Context/version mismatches and repeated decisions fail closed.
    async fn resolve_permission(
        &self,
        request: permission::PermissionResolutionRequest,
    ) -> Result<permission::PermissionState, DomainError>;

    /// Atomically resolve the current Plan version and invalidate all older
    /// approvals before forwarding the exact ACP option ID.
    async fn resolve_plan(
        &self,
        request: plan::PlanResolutionRequest,
    ) -> Result<plan::PlanState, DomainError>;
}
