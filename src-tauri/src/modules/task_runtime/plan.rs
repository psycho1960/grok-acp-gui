//! Version-bound Plan state machine (GAG-009).

use crate::bridge::types::{SessionId, TaskId};
use crate::domain::error::DomainError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanState {
    Draft,
    Proposed,
    Approved,
    Rejected,
    RevisionRequested,
    Superseded,
    Executing,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanTransition {
    Propose,
    Approve,
    Reject,
    RequestRevision,
    Supersede,
    StartExecution,
    Complete,
    Fail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanOptionAction {
    Approve,
    RequestRevision,
    Reject,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanOption {
    pub option_id: String,
    pub label: String,
    pub action: PlanOptionAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanRecord {
    pub request_id: String,
    pub task_id: TaskId,
    pub session_id: SessionId,
    pub correlation_id: String,
    pub workspace: String,
    pub version: u64,
    pub plan_hash: String,
    pub state: PlanState,
    pub summary_redacted: String,
    pub options: Vec<PlanOption>,
    pub decided_option_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanDecision {
    pub request_id: String,
    pub task_id: TaskId,
    pub session_id: SessionId,
    pub correlation_id: String,
    pub workspace: String,
    pub expected_version: u64,
    pub option_id: String,
    pub decided_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanResolutionRequest {
    pub task_id: TaskId,
    pub session_id: SessionId,
    pub request_id: String,
    pub correlation_id: String,
    pub expected_version: u64,
    pub option_id: String,
}

impl PlanState {
    pub fn transition(self, action: PlanTransition) -> Result<Self, DomainError> {
        use PlanState::*;
        use PlanTransition::*;
        match (self, action) {
            (Draft, Propose) => Ok(Proposed),
            (Proposed, Approve) => Ok(Approved),
            (Proposed, Reject) => Ok(Rejected),
            (Proposed, RequestRevision) => Ok(RevisionRequested),
            (Draft | Proposed | Approved | Rejected | RevisionRequested, Supersede) => {
                Ok(Superseded)
            }
            (Approved, StartExecution) => Ok(Executing),
            (Executing, Complete) => Ok(Completed),
            (Executing, Fail) => Ok(Failed),
            _ => Err(DomainError::illegal_transition(
                "Plan",
                &format!("{:?}", self).to_ascii_lowercase(),
                &format!("{:?}", action).to_ascii_lowercase(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_only_applies_to_proposed_plan() {
        assert_eq!(
            PlanState::Proposed
                .transition(PlanTransition::Approve)
                .unwrap(),
            PlanState::Approved
        );
        assert!(PlanState::Draft
            .transition(PlanTransition::Approve)
            .is_err());
        assert!(PlanState::Superseded
            .transition(PlanTransition::Approve)
            .is_err());
    }

    #[test]
    fn revision_and_execution_paths_are_explicit() {
        assert_eq!(
            PlanState::Proposed
                .transition(PlanTransition::RequestRevision)
                .unwrap(),
            PlanState::RevisionRequested
        );
        assert_eq!(
            PlanState::Approved
                .transition(PlanTransition::StartExecution)
                .unwrap(),
            PlanState::Executing
        );
    }
}
