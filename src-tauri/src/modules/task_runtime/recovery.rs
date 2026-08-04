//! Startup recovery: detect interrupted tasks and produce recovery candidates.
//!
//! On application startup, tasks that were in a "live process implied" state
//! (running, waiting_permission, integrating) are transitioned to `interrupted`
//! because no managed process exists to prove they're still alive.
//!
//! The user is then presented with recovery candidates. Each candidate can be:
//! - **Resumed**: a new attempt is created; the old attempt's events remain
//!   readable but are never replayed.
//! - **Archived**: the task moves to `archived` and no further attempts are made.

use crate::domain::error::DomainError;
use crate::domain::types::{RecoveryCandidate, RecoveryDecision};
use crate::modules::persistence::RepoResult;

/// Perform startup recovery: transition tasks in live-process states to
/// `interrupted`, then return all interrupted tasks as candidates.
pub async fn run_startup_recovery(
    repo: &dyn crate::modules::persistence::Repository,
) -> Result<(Vec<RecoveryCandidate>, u32), DomainError> {
    // Transition live-process tasks to interrupted.
    let count = repo.recover_interrupted_tasks("application restarted")?;

    // Gather recovery candidates.
    let candidates = repo.list_recovery_candidates()?;

    Ok((candidates, count))
}

/// Apply the user's recovery decision for a single task.
pub fn apply_decision(
    repo: &dyn crate::modules::persistence::Repository,
    decision: &RecoveryDecision,
) -> RepoResult<()> {
    repo.apply_recovery_decision(decision)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::RecoveryAction;

    #[test]
    fn recovery_action_serde() {
        assert_eq!(
            serde_json::to_string(&RecoveryAction::Resume).unwrap(),
            "\"resume\""
        );
        assert_eq!(
            serde_json::to_string(&RecoveryAction::Archive).unwrap(),
            "\"archive\""
        );
    }

    #[test]
    fn recovery_decision_roundtrip() {
        let d = RecoveryDecision {
            task_id: crate::bridge::types::TaskId::new("t1"),
            action: RecoveryAction::Resume,
            decided_at: crate::domain::types::utc_now(),
        };
        let json = serde_json::to_string(&d).unwrap();
        let back: RecoveryDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(back.task_id.0, "t1");
        assert_eq!(back.action, RecoveryAction::Resume);
    }
}
