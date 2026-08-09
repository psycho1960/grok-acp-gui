//! State transition functions with invariant enforcement.
//!
//! Every transition is a pure function: `(current_state, input) → Result<new_state, DomainError>`.
//! Illegal transitions return a descriptive domain error; the caller decides
//! how to map that to an `AppError` for the bridge.

use super::error::DomainError;
use super::types::{RecoveryState, SessionState, TaskStatus, WorktreeState};

// ===========================================================================
// Task status transitions
// ===========================================================================

/// Allowed inputs for a Task state transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskTransition {
    /// Bind to a workspace and start executing.
    Start,
    /// Agent requested permission; task enters waiting state.
    AwaitPermission,
    /// Permission resolved; task resumes execution.
    Resume,
    /// Start integration (squash merge).
    BeginIntegration,
    /// Integration completed successfully.
    CompleteIntegration,
    /// User archives the task.
    Archive,
    /// Process died or app exited; mark for recovery.
    Interrupt { reason: String },
}

/// Attempt a Task status transition.  Returns the new status or a domain error.
///
/// # Invariants enforced
/// - `Start` only valid from `Preparing`.
/// - `AwaitPermission` only valid from `Running`.
/// - `Resume` only valid from `WaitingPermission`.
/// - `BeginIntegration` only valid from `Running`.
/// - `CompleteIntegration` only valid from `Integrating`.
/// - `Merged` cannot transition to anything (terminal).
/// - `Archive` valid from any non-terminal except `Archived`.
/// - `Interrupt` valid from `Running`, `WaitingPermission`, `Integrating`.
pub fn transition_task(
    current: TaskStatus,
    transition: TaskTransition,
) -> Result<TaskStatus, DomainError> {
    use TaskStatus::*;
    use TaskTransition::*;

    let trans_desc = format!("{:?}", transition);

    match (current, transition) {
        // Preparing → Running (valid workspace required by caller)
        (Preparing, Start) => Ok(Running),

        // Running → WaitingPermission
        (Running, AwaitPermission) => Ok(WaitingPermission),

        // WaitingPermission → Running
        (WaitingPermission, Resume) => Ok(Running),

        // Running → Integrating
        (Running, BeginIntegration) => Ok(Integrating),

        // Integrating → Merged
        (Integrating, CompleteIntegration) => Ok(Merged),

        // Any non-terminal → Archived (except Archived itself)
        (Archived, Archive) => Err(DomainError::illegal_transition(
            "Task",
            "archived",
            "archive (already archived)",
        )),
        (Merged, Archive) => Err(DomainError::illegal_transition(
            "Task",
            "merged",
            "archive (already merged)",
        )),
        (s, Archive) if !s.is_terminal() => Ok(Archived),

        // Interrupt: only from live-process states
        (Running, Interrupt { .. }) => Ok(Interrupted),
        (WaitingPermission, Interrupt { .. }) => Ok(Interrupted),
        (Integrating, Interrupt { .. }) => Ok(Interrupted),

        // Merged is terminal — no transitions
        (Merged, _) => Err(DomainError::illegal_transition(
            "Task",
            "merged",
            &trans_desc,
        )),

        // Illegal combinations
        (current, _) => Err(DomainError::illegal_transition(
            "Task",
            &format!("{:?}", current).to_lowercase(),
            &trans_desc,
        )),
    }
}

// ===========================================================================
// Worktree state transitions
// ===========================================================================

/// Allowed inputs for a Worktree state transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeTransition {
    /// Worktree was just created and verified.
    Initialize,
    /// Uncommitted changes detected.
    MarkDirty,
    /// Changes committed; worktree is clean.
    MarkClean,
    /// Integration started on this worktree.
    BeginIntegration,
    /// Integration completed.
    CompleteIntegration,
    /// Worktree explicitly deleted (pruned).
    Delete,
    /// State cannot be determined.
    MarkUnknown,
}

/// Attempt a Worktree state transition.
///
/// # Invariants
/// - Ownership is tracked separately from state; a "clean" external worktree
///   does not automatically become Managed.
/// - `Deleted` is terminal.
pub fn transition_worktree(
    current: WorktreeState,
    transition: WorktreeTransition,
) -> Result<WorktreeState, DomainError> {
    use WorktreeState::*;
    use WorktreeTransition::*;

    match (current, transition) {
        // Deleted is terminal — handle it first for all transitions
        (Deleted | Removed, _) => Err(DomainError::illegal_transition(
            "Worktree",
            "deleted",
            &format!("{:?}", transition),
        )),

        // Initialize: valid from any non-Deleted state
        (_, Initialize) => Ok(Ready),

        // MarkUnknown: valid from any non-Deleted state
        (_, MarkUnknown) => Ok(Unknown),

        // Delete: valid from any non-Deleted state
        (_, Delete) => Ok(Deleted),

        // Ready ↔ Dirty cycle
        (Ready, MarkDirty) => Ok(Dirty),
        (Dirty, MarkClean) => Ok(Ready),

        // Ready ↔ Integrating cycle
        (Ready, BeginIntegration) => Ok(Integrating),
        (Integrating, CompleteIntegration) => Ok(Ready),

        // Invalid state for the given transition
        (current, t) => Err(DomainError::illegal_transition(
            "Worktree",
            &format!("{:?}", current).to_lowercase(),
            &format!("{:?}", t),
        )),
    }
}

// ===========================================================================
// Session state transitions
// ===========================================================================

/// Allowed inputs for a Session state transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionTransition {
    /// Session is actively processing a turn.
    Activate,
    /// Turn completed; session idle.
    Idle,
    /// Process exited unexpectedly.
    Disconnect,
    /// Explicitly close the session.
    Close,
}

/// Attempt a Session state transition.
pub fn transition_session(
    current: SessionState,
    transition: SessionTransition,
) -> Result<SessionState, DomainError> {
    use SessionState::*;

    match (current, transition) {
        // Activate: valid from Idle, Disconnected, or Active (idempotent)
        (Active, SessionTransition::Activate) => Ok(Active),
        (Idle, SessionTransition::Activate) => Ok(Active),
        (Disconnected, SessionTransition::Activate) => Ok(Active),

        // Idle: only valid from Active
        (Active, SessionTransition::Idle) => Ok(Idle),

        // Disconnect: valid from any non-Closed
        (Closed, SessionTransition::Disconnect) => Err(DomainError::illegal_transition(
            "Session",
            "closed",
            "disconnect",
        )),
        (_, SessionTransition::Disconnect) => Ok(Disconnected),

        // Close: valid from any non-Closed
        (Closed, SessionTransition::Close) => Err(DomainError::illegal_transition(
            "Session",
            "closed",
            "close (already closed)",
        )),
        (_, SessionTransition::Close) => Ok(Closed),

        // Closed is terminal
        (Closed, _) => Err(DomainError::illegal_transition(
            "Session",
            "closed",
            &format!("{:?}", transition),
        )),

        // Unhandled combos
        (current, t) => Err(DomainError::illegal_transition(
            "Session",
            &format!("{:?}", current).to_lowercase(),
            &format!("{:?}", t),
        )),
    }
}

// ===========================================================================
// Recovery state transitions
// ===========================================================================

/// Allowed inputs for a Recovery state transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryTransition {
    /// Mark the recovery package as expired.
    Expire,
    /// Begin restoring from the package.
    BeginRestore,
    /// Restore completed successfully.
    CompleteRestore,
    /// Explicitly delete the package.
    Delete,
}

/// Attempt a Recovery state transition.
///
/// # Invariants
/// - `Expired` does NOT mean deleted — the package still exists.
/// - Only `Available` or `Expired` can transition to `Restoring`.
/// - `Deleted` is terminal and requires explicit successful deletion by caller.
pub fn transition_recovery(
    current: RecoveryState,
    transition: RecoveryTransition,
) -> Result<RecoveryState, DomainError> {
    use RecoveryState::*;
    use RecoveryTransition::*;

    match (current, transition) {
        // Available → Expired
        (Available, Expire) => Ok(Expired),

        // Available or Expired → Restoring
        (Available, BeginRestore) => Ok(Restoring),
        (Expired, BeginRestore) => Ok(Restoring),

        // Restoring → Restored
        (Restoring, CompleteRestore) => Ok(Restored),

        // Any non-deleted → Deleted
        (Deleted, Delete) => Err(DomainError::illegal_transition(
            "Recovery",
            "deleted",
            "delete (already deleted)",
        )),
        (_, Delete) => Ok(Deleted),

        // Deleted is terminal
        (Deleted, _) => Err(DomainError::illegal_transition(
            "Recovery",
            "deleted",
            &format!("{:?}", transition),
        )),

        // Other illegal combos
        (current, _) => Err(DomainError::illegal_transition(
            "Recovery",
            &format!("{:?}", current).to_lowercase(),
            &format!("{:?}", transition),
        )),
    }
}

// ===========================================================================
// Tests — full transition tables
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Task transition table tests
    // ------------------------------------------------------------------

    #[test]
    fn task_preparing_to_running() {
        let result = transition_task(TaskStatus::Preparing, TaskTransition::Start);
        assert_eq!(result, Ok(TaskStatus::Running));
    }

    #[test]
    fn task_running_to_waiting_permission() {
        let result = transition_task(TaskStatus::Running, TaskTransition::AwaitPermission);
        assert_eq!(result, Ok(TaskStatus::WaitingPermission));
    }

    #[test]
    fn task_waiting_to_running() {
        let result = transition_task(TaskStatus::WaitingPermission, TaskTransition::Resume);
        assert_eq!(result, Ok(TaskStatus::Running));
    }

    #[test]
    fn task_running_to_integrating() {
        let result = transition_task(TaskStatus::Running, TaskTransition::BeginIntegration);
        assert_eq!(result, Ok(TaskStatus::Integrating));
    }

    #[test]
    fn task_integrating_to_merged() {
        let result = transition_task(TaskStatus::Integrating, TaskTransition::CompleteIntegration);
        assert_eq!(result, Ok(TaskStatus::Merged));
    }

    #[test]
    fn task_merged_is_terminal() {
        // Cannot go back to running
        assert!(transition_task(TaskStatus::Merged, TaskTransition::Start).is_err());
        assert!(transition_task(TaskStatus::Merged, TaskTransition::Resume).is_err());
        assert!(transition_task(TaskStatus::Merged, TaskTransition::Archive).is_err());
    }

    #[test]
    fn task_running_to_archived() {
        let result = transition_task(TaskStatus::Running, TaskTransition::Archive);
        assert_eq!(result, Ok(TaskStatus::Archived));
    }

    #[test]
    fn task_interrupted_from_running() {
        let result = transition_task(
            TaskStatus::Running,
            TaskTransition::Interrupt {
                reason: "app exited".into(),
            },
        );
        assert_eq!(result, Ok(TaskStatus::Interrupted));
    }

    #[test]
    fn task_interrupted_from_waiting_permission() {
        let result = transition_task(
            TaskStatus::WaitingPermission,
            TaskTransition::Interrupt {
                reason: "crash".into(),
            },
        );
        assert_eq!(result, Ok(TaskStatus::Interrupted));
    }

    #[test]
    fn task_interrupted_from_integrating() {
        let result = transition_task(
            TaskStatus::Integrating,
            TaskTransition::Interrupt {
                reason: "power loss".into(),
            },
        );
        assert_eq!(result, Ok(TaskStatus::Interrupted));
    }

    #[test]
    fn task_preparing_cannot_integrate() {
        assert!(transition_task(TaskStatus::Preparing, TaskTransition::BeginIntegration).is_err());
    }

    #[test]
    fn task_illegal_from_preparing() {
        assert!(transition_task(TaskStatus::Preparing, TaskTransition::Resume).is_err());
        assert!(
            transition_task(TaskStatus::Preparing, TaskTransition::CompleteIntegration).is_err()
        );
    }

    #[test]
    fn task_all_legal_transitions_exhaustive() {
        // Verify every legal transition from the matrix.
        let legal = vec![
            (
                TaskStatus::Preparing,
                TaskTransition::Start,
                TaskStatus::Running,
            ),
            (
                TaskStatus::Running,
                TaskTransition::AwaitPermission,
                TaskStatus::WaitingPermission,
            ),
            (
                TaskStatus::WaitingPermission,
                TaskTransition::Resume,
                TaskStatus::Running,
            ),
            (
                TaskStatus::Running,
                TaskTransition::BeginIntegration,
                TaskStatus::Integrating,
            ),
            (
                TaskStatus::Integrating,
                TaskTransition::CompleteIntegration,
                TaskStatus::Merged,
            ),
            (
                TaskStatus::Running,
                TaskTransition::Archive,
                TaskStatus::Archived,
            ),
            (
                TaskStatus::Preparing,
                TaskTransition::Archive,
                TaskStatus::Archived,
            ),
            (
                TaskStatus::Running,
                TaskTransition::Interrupt {
                    reason: "test".into(),
                },
                TaskStatus::Interrupted,
            ),
            (
                TaskStatus::WaitingPermission,
                TaskTransition::Interrupt {
                    reason: "test".into(),
                },
                TaskStatus::Interrupted,
            ),
            (
                TaskStatus::Integrating,
                TaskTransition::Interrupt {
                    reason: "test".into(),
                },
                TaskStatus::Interrupted,
            ),
        ];

        for (current, transition, expected) in legal {
            let result = transition_task(current, transition);
            assert_eq!(
                result,
                Ok(expected),
                "Transition {:?} → ? should yield {:?}",
                current,
                expected
            );
        }
    }

    // ------------------------------------------------------------------
    // Worktree transition table tests
    // ------------------------------------------------------------------

    #[test]
    fn worktree_initialize() {
        assert_eq!(
            transition_worktree(WorktreeState::Unknown, WorktreeTransition::Initialize),
            Ok(WorktreeState::Ready)
        );
    }

    #[test]
    fn worktree_dirty_cycle() {
        let r = transition_worktree(WorktreeState::Ready, WorktreeTransition::MarkDirty);
        assert_eq!(r, Ok(WorktreeState::Dirty));
        let r = transition_worktree(WorktreeState::Dirty, WorktreeTransition::MarkClean);
        assert_eq!(r, Ok(WorktreeState::Ready));
    }

    #[test]
    fn worktree_integration_cycle() {
        let r = transition_worktree(WorktreeState::Ready, WorktreeTransition::BeginIntegration);
        assert_eq!(r, Ok(WorktreeState::Integrating));
        let r = transition_worktree(
            WorktreeState::Integrating,
            WorktreeTransition::CompleteIntegration,
        );
        assert_eq!(r, Ok(WorktreeState::Ready));
    }

    #[test]
    fn worktree_deleted_is_terminal() {
        assert!(
            transition_worktree(WorktreeState::Deleted, WorktreeTransition::Initialize).is_err()
        );
        assert!(transition_worktree(WorktreeState::Deleted, WorktreeTransition::Delete).is_err());
    }

    #[test]
    fn worktree_delete_from_any() {
        for state in &[
            WorktreeState::Ready,
            WorktreeState::Dirty,
            WorktreeState::Integrating,
            WorktreeState::Unknown,
        ] {
            assert_eq!(
                transition_worktree(*state, WorktreeTransition::Delete),
                Ok(WorktreeState::Deleted)
            );
        }
    }

    // ------------------------------------------------------------------
    // Session transition table tests
    // ------------------------------------------------------------------

    #[test]
    fn session_lifecycle() {
        assert_eq!(
            transition_session(SessionState::Idle, SessionTransition::Activate),
            Ok(SessionState::Active)
        );
        assert_eq!(
            transition_session(SessionState::Active, SessionTransition::Idle),
            Ok(SessionState::Idle)
        );
        assert_eq!(
            transition_session(SessionState::Active, SessionTransition::Disconnect),
            Ok(SessionState::Disconnected)
        );
        assert_eq!(
            transition_session(SessionState::Disconnected, SessionTransition::Activate),
            Ok(SessionState::Active)
        );
        assert_eq!(
            transition_session(SessionState::Active, SessionTransition::Close),
            Ok(SessionState::Closed)
        );
        assert!(transition_session(SessionState::Closed, SessionTransition::Activate).is_err());
    }

    // ------------------------------------------------------------------
    // Recovery transition table tests
    // ------------------------------------------------------------------

    #[test]
    fn recovery_expire() {
        assert_eq!(
            transition_recovery(RecoveryState::Available, RecoveryTransition::Expire),
            Ok(RecoveryState::Expired)
        );
    }

    #[test]
    fn recovery_restore_from_available() {
        assert_eq!(
            transition_recovery(RecoveryState::Available, RecoveryTransition::BeginRestore),
            Ok(RecoveryState::Restoring)
        );
    }

    #[test]
    fn recovery_restore_from_expired() {
        // Expired is not deleted — restore is still valid
        assert_eq!(
            transition_recovery(RecoveryState::Expired, RecoveryTransition::BeginRestore),
            Ok(RecoveryState::Restoring)
        );
    }

    #[test]
    fn recovery_complete_restore() {
        assert_eq!(
            transition_recovery(
                RecoveryState::Restoring,
                RecoveryTransition::CompleteRestore
            ),
            Ok(RecoveryState::Restored)
        );
    }

    #[test]
    fn recovery_deleted_is_terminal() {
        assert!(
            transition_recovery(RecoveryState::Deleted, RecoveryTransition::BeginRestore).is_err()
        );
        assert!(transition_recovery(RecoveryState::Deleted, RecoveryTransition::Delete).is_err());
    }

    #[test]
    fn recovery_delete_from_any() {
        for state in &[
            RecoveryState::Available,
            RecoveryState::Expired,
            RecoveryState::Restoring,
            RecoveryState::Restored,
        ] {
            assert_eq!(
                transition_recovery(*state, RecoveryTransition::Delete),
                Ok(RecoveryState::Deleted)
            );
        }
    }

    #[test]
    fn recovery_restored_cannot_restore_again() {
        assert!(
            transition_recovery(RecoveryState::Restored, RecoveryTransition::BeginRestore).is_err()
        );
    }
}
