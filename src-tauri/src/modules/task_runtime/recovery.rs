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

use crate::bridge::types::TaskId;
use crate::domain::error::DomainError;
use crate::domain::types::{
    utc_now, IntegrationState, RecoveryCandidate, RecoveryDecision, RecoveryState, SessionState,
    WorktreeOwnership, WorktreeState,
};
use crate::modules::artifacts::{ArtifactService, ArtifactTemporaryFile};
use crate::modules::persistence::{RepoResult, Repository};
use crate::modules::workspace::{RemovalPreparation, WorkspaceError, WorkspaceService};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::Arc;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryIssueKind {
    InterruptedTask,
    OrphanedSession,
    WorktreeMismatch,
    TemporaryIntegration,
    ArtifactTemporaryFile,
    PersistenceMarker,
    RecoveryBundle,
}

impl RecoveryIssueKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InterruptedTask => "interrupted_task",
            Self::OrphanedSession => "orphaned_session",
            Self::WorktreeMismatch => "worktree_mismatch",
            Self::TemporaryIntegration => "temporary_integration",
            Self::ArtifactTemporaryFile => "artifact_temporary_file",
            Self::PersistenceMarker => "persistence_marker",
            Self::RecoveryBundle => "recovery_bundle",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "interrupted_task" => Self::InterruptedTask,
            "orphaned_session" => Self::OrphanedSession,
            "worktree_mismatch" => Self::WorktreeMismatch,
            "temporary_integration" => Self::TemporaryIntegration,
            "artifact_temporary_file" => Self::ArtifactTemporaryFile,
            "persistence_marker" => Self::PersistenceMarker,
            "recovery_bundle" => Self::RecoveryBundle,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoverySeverity {
    Immediate,
    Deferred,
    Informational,
}

impl RecoverySeverity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Immediate => "immediate",
            Self::Deferred => "deferred",
            Self::Informational => "informational",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "immediate" => Self::Immediate,
            "deferred" => Self::Deferred,
            "informational" => Self::Informational,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryIssueStatus {
    Detected,
    Assessed,
    Ready,
    Executing,
    Resolved,
    Retained,
    Failed,
    NeedsManualAction,
}

impl RecoveryIssueStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Detected => "detected",
            Self::Assessed => "assessed",
            Self::Ready => "ready",
            Self::Executing => "executing",
            Self::Resolved => "resolved",
            Self::Retained => "retained",
            Self::Failed => "failed",
            Self::NeedsManualAction => "needs_manual_action",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "detected" => Self::Detected,
            "assessed" => Self::Assessed,
            "ready" => Self::Ready,
            "executing" => Self::Executing,
            "resolved" => Self::Resolved,
            "retained" => Self::Retained,
            "failed" => Self::Failed,
            "needs_manual_action" => Self::NeedsManualAction,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryActionKind {
    MarkInterrupted,
    Reregister,
    Retain,
    ShowLocation,
    ResumeSession,
    ContinueIntegration,
    AbortIntegration,
    VerifyAndCleanup,
    RestoreBundle,
    DeleteBundle,
}

impl RecoveryActionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MarkInterrupted => "mark_interrupted",
            Self::Reregister => "reregister",
            Self::Retain => "retain",
            Self::ShowLocation => "show_location",
            Self::ResumeSession => "resume_session",
            Self::ContinueIntegration => "continue_integration",
            Self::AbortIntegration => "abort_integration",
            Self::VerifyAndCleanup => "verify_and_cleanup",
            Self::RestoreBundle => "restore_bundle",
            Self::DeleteBundle => "delete_bundle",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "mark_interrupted" => Self::MarkInterrupted,
            "reregister" => Self::Reregister,
            "retain" => Self::Retain,
            "show_location" => Self::ShowLocation,
            "resume_session" => Self::ResumeSession,
            "continue_integration" => Self::ContinueIntegration,
            "abort_integration" => Self::AbortIntegration,
            "verify_and_cleanup" => Self::VerifyAndCleanup,
            "restore_bundle" => Self::RestoreBundle,
            "delete_bundle" => Self::DeleteBundle,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryScan {
    pub id: String,
    pub trigger_kind: String,
    pub started_at: String,
    pub completed_at: String,
    pub issue_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryIssue {
    pub issue_id: String,
    pub revision: u32,
    pub scan_id: Option<String>,
    pub stable_key: String,
    pub kind: RecoveryIssueKind,
    pub severity: RecoverySeverity,
    pub status: RecoveryIssueStatus,
    pub task_id: Option<TaskId>,
    pub resource_id: String,
    pub canonical_path: Option<String>,
    pub evidence: serde_json::Value,
    pub impact: String,
    pub recommended_action: String,
    pub safe_actions: Vec<RecoveryActionKind>,
    pub detected_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryActionPlan {
    pub id: String,
    pub issue_id: String,
    pub issue_revision: u32,
    pub action_kind: RecoveryActionKind,
    pub resource_identity: String,
    pub canonical_path: Option<String>,
    pub expected_state: serde_json::Value,
    pub steps: Vec<String>,
    #[serde(skip_serializing, default)]
    pub internal_context: serde_json::Value,
    pub destructive_level: String,
    pub approval_digest: String,
    pub expires_at_epoch: u64,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryBundleRecord {
    pub id: String,
    pub issue_id: String,
    pub issue_revision: u32,
    pub recovery_item_id: String,
    pub manifest_path: String,
    pub manifest_sha256: String,
    pub verified: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryStepResult {
    pub id: i64,
    pub plan_id: String,
    pub step_index: u32,
    pub step_name: String,
    pub status: String,
    pub detail_redacted: String,
    pub occurred_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryHistory {
    pub scans: Vec<RecoveryScan>,
    pub issues: Vec<RecoveryIssue>,
    pub plans: Vec<RecoveryActionPlan>,
    pub bundles: Vec<RecoveryBundleRecord>,
    pub steps: Vec<RecoveryStepResult>,
}

pub trait RecoveryService: Send + Sync {
    fn scan(&self, trigger_kind: &str) -> Result<Vec<RecoveryIssue>, DomainError>;
    fn get_issue(
        &self,
        issue_id: &str,
        revision: Option<u32>,
    ) -> Result<RecoveryIssue, DomainError>;
    fn prepare_action(
        &self,
        issue_id: &str,
        revision: u32,
        action: RecoveryActionKind,
    ) -> Result<RecoveryActionPlan, DomainError>;
    fn execute_action(
        &self,
        plan_id: &str,
        approval_digest: &str,
    ) -> Result<RecoveryIssue, DomainError>;
    fn create_bundle(
        &self,
        issue_id: &str,
        revision: u32,
    ) -> Result<RecoveryBundleRecord, DomainError>;
    fn verify_bundle(&self, bundle_id: &str) -> Result<RecoveryBundleRecord, DomainError>;
    fn list_history(&self) -> Result<RecoveryHistory, DomainError>;
}

pub struct ManagedRecoveryService {
    repo: Arc<dyn Repository>,
    workspace: Arc<dyn WorkspaceService>,
    artifacts: Arc<dyn ArtifactService>,
}

impl ManagedRecoveryService {
    pub fn new(
        repo: Arc<dyn Repository>,
        workspace: Arc<dyn WorkspaceService>,
        artifacts: Arc<dyn ArtifactService>,
    ) -> Self {
        Self {
            repo,
            workspace,
            artifacts,
        }
    }

    fn append_status(
        &self,
        issue: &RecoveryIssue,
        status: RecoveryIssueStatus,
    ) -> Result<RecoveryIssue, DomainError> {
        let mut next = issue.clone();
        next.revision = next.revision.saturating_add(1);
        next.scan_id = None;
        next.status = status;
        next.detected_at = utc_now();
        self.repo.append_recovery_issue(&next)?;
        Ok(next)
    }

    fn record_step(
        &self,
        plan: &RecoveryActionPlan,
        index: u32,
        name: &str,
        status: &str,
        detail: &str,
    ) {
        let _ = self.repo.append_recovery_step_result(&RecoveryStepResult {
            id: 0,
            plan_id: plan.id.clone(),
            step_index: index,
            step_name: name.into(),
            status: status.into(),
            detail_redacted: detail.into(),
            occurred_at: utc_now(),
        });
    }

    fn persist_bundle(
        &self,
        issue: &RecoveryIssue,
        preparation: &RemovalPreparation,
    ) -> Result<RecoveryBundleRecord, DomainError> {
        let evidence = preparation.recovery.as_ref().ok_or_else(|| {
            recovery_error(
                "RECOVERY_BUNDLE_NOT_REQUIRED",
                "The current resource does not require a recovery bundle",
            )
        })?;
        let verified = self
            .workspace
            .verify_recovery_bundle(&evidence.id)
            .map_err(map_workspace)?;
        let record = RecoveryBundleRecord {
            id: format!("bundle-record-{}", uuid::Uuid::new_v4()),
            issue_id: issue.issue_id.clone(),
            issue_revision: issue.revision,
            recovery_item_id: evidence.id.clone(),
            manifest_path: verified.manifest_path,
            manifest_sha256: verified.manifest_sha256,
            verified: true,
            created_at: utc_now(),
        };
        self.repo.create_recovery_bundle_record(&record)?;
        Ok(record)
    }
}

impl RecoveryService for ManagedRecoveryService {
    fn scan(&self, trigger_kind: &str) -> Result<Vec<RecoveryIssue>, DomainError> {
        if !matches!(trigger_kind, "startup" | "manual") {
            return Err(recovery_error(
                "RECOVERY_TRIGGER_UNKNOWN",
                "Unknown recovery scan trigger",
            ));
        }
        let started_at = utc_now();
        let scan_id = format!("scan-{}", uuid::Uuid::new_v4());
        let detected_at = utc_now();
        let mut pending = BTreeMap::<String, RecoveryIssue>::new();

        for candidate in self.repo.list_recovery_candidates()? {
            let key = format!("interrupted-task:{}", candidate.task_id.0);
            pending.insert(key.clone(), RecoveryIssue {
                issue_id: stable_issue_id(&key), revision: 1, scan_id: Some(scan_id.clone()), stable_key: key,
                kind: RecoveryIssueKind::InterruptedTask, severity: RecoverySeverity::Immediate,
                status: RecoveryIssueStatus::Detected, task_id: Some(candidate.task_id.clone()),
                resource_id: candidate.task_id.0.clone(), canonical_path: None,
                evidence: serde_json::json!({"previousStatus":candidate.previous_status,"interruptedAt":candidate.interrupted_at,"reason":candidate.interrupt_reason,"hasSession":candidate.has_session,"eventsAvailable":candidate.events_available,"attemptCount":candidate.attempt_count}),
                impact: "The task has no proven live ACP process.".into(), recommended_action: "Resume the persisted session or retain the task for later.".into(),
                safe_actions: vec![RecoveryActionKind::ResumeSession, RecoveryActionKind::MarkInterrupted, RecoveryActionKind::Retain], detected_at: detected_at.clone(),
            });
        }

        let interrupted: HashSet<String> = pending
            .values()
            .filter_map(|issue| issue.task_id.as_ref().map(|id| id.0.clone()))
            .collect();
        for binding in self.repo.list_active_bindings()? {
            if binding.state == SessionState::Disconnected
                && !interrupted.contains(&binding.task_id.0)
            {
                let key = format!("orphaned-session:{}", binding.task_id.0);
                pending.insert(key.clone(), RecoveryIssue {
                    issue_id: stable_issue_id(&key), revision: 1, scan_id: Some(scan_id.clone()), stable_key: key,
                    kind: RecoveryIssueKind::OrphanedSession, severity: RecoverySeverity::Immediate,
                    status: RecoveryIssueStatus::Detected, task_id: Some(binding.task_id.clone()), resource_id: binding.session_id.0,
                    canonical_path: binding.cwd,
                    evidence: serde_json::json!({"state":"disconnected","lastSequence":binding.last_seq,"attemptNumber":binding.attempt_number}),
                    impact: "A persisted session registration has no proven managed process.".into(), recommended_action: "Mark the task interrupted before resuming it.".into(),
                    safe_actions: vec![RecoveryActionKind::MarkInterrupted, RecoveryActionKind::ResumeSession, RecoveryActionKind::Retain], detected_at: detected_at.clone(),
                });
            }
        }

        let registered = self.repo.list_active_worktrees()?;
        let mut diagnosed = Vec::new();
        for original in registered {
            match self.workspace.inspect_worktree(&original.task_id.0) {
                Ok(fresh) => {
                    let observed = if original.state == WorktreeState::Closing {
                        original
                    } else {
                        fresh
                    };
                    diagnosed.push((observed, None));
                }
                Err(error) => diagnosed.push((original, Some(error))),
            }
        }
        if let Ok(discovered) = self.workspace.reconcile_registry() {
            diagnosed.extend(
                discovered
                    .into_iter()
                    .filter(|item| item.ownership == WorktreeOwnership::External)
                    .map(|item| (item, None)),
            );
        }
        for (record, diagnostic_error) in diagnosed {
            let mismatch = diagnostic_error.is_some()
                || record.ownership == WorktreeOwnership::External
                || matches!(
                    record.state,
                    WorktreeState::Closing
                        | WorktreeState::Missing
                        | WorktreeState::Orphaned
                        | WorktreeState::Quarantined
                        | WorktreeState::Unknown
                );
            if !mismatch {
                continue;
            }
            let key = format!("worktree:{}", record.id.0);
            let mut actions = vec![RecoveryActionKind::Retain];
            if diagnostic_error.is_none() {
                actions.push(RecoveryActionKind::ShowLocation);
            }
            if diagnostic_error.is_none()
                && record.ownership == WorktreeOwnership::Managed
                && record.state == WorktreeState::Closing
            {
                actions.push(RecoveryActionKind::Reregister);
                actions.push(RecoveryActionKind::VerifyAndCleanup);
            }
            let diagnostic_code = diagnostic_error.as_ref().map(|error| error.code);
            pending.insert(key.clone(), RecoveryIssue {
                issue_id: stable_issue_id(&key), revision: 1, scan_id: Some(scan_id.clone()), stable_key: key,
                kind: RecoveryIssueKind::WorktreeMismatch,
                severity: if matches!(record.state, WorktreeState::Quarantined | WorktreeState::Unknown) { RecoverySeverity::Immediate } else { RecoverySeverity::Deferred },
                status: RecoveryIssueStatus::Detected, task_id: Some(record.task_id.clone()), resource_id: record.id.0,
                canonical_path: Some(record.path.clone()),
                evidence: serde_json::json!({"ownership":record.ownership,"state":record.state,"repoIdentity":record.repo_identity,"branch":record.branch,"commonGitDir":record.common_git_dir,"lastVerifiedAt":record.last_verified_at,"diagnosticError":diagnostic_code}),
                impact: "Database, Git and filesystem registration are not in a normal aligned state.".into(), recommended_action: "Retain the resource unless the offered verified cleanup plan succeeds.".into(), safe_actions: actions, detected_at: detected_at.clone(),
            });
        }

        for attempt in self.repo.list_active_integrations()? {
            let key = format!("integration:{}", attempt.id.0);
            pending.insert(key.clone(), RecoveryIssue {
                issue_id: stable_issue_id(&key), revision: 1, scan_id: Some(scan_id.clone()), stable_key: key,
                kind: RecoveryIssueKind::TemporaryIntegration,
                severity: if matches!(attempt.state, IntegrationState::Publishing | IntegrationState::Staging | IntegrationState::Validating) { RecoverySeverity::Immediate } else { RecoverySeverity::Deferred },
                status: RecoveryIssueStatus::Detected, task_id: Some(attempt.task_id.clone()), resource_id: attempt.id.0.clone(),
                canonical_path: attempt.temporary_worktree_path.clone(),
                evidence: serde_json::json!({"state":attempt.state,"cleanupStatus":attempt.cleanup_status,"repoIdentity":attempt.repo_identity,"sourceTipSha":attempt.source_tip_sha,"expectedTargetSha":attempt.expected_target_sha,"resultCommitSha":attempt.result_commit_sha,"recoveryBundlePath":attempt.recovery_bundle_path}),
                impact: "An integration attempt has durable resources that are not fully cleaned up.".into(), recommended_action: "Continue only from the frozen attempt, or abort and clean its verified temporary resources.".into(),
                safe_actions: vec![RecoveryActionKind::ContinueIntegration, RecoveryActionKind::AbortIntegration, RecoveryActionKind::ShowLocation, RecoveryActionKind::Retain], detected_at: detected_at.clone(),
            });
        }

        for temporary in self
            .artifacts
            .diagnose_temporary_files(self.repo.as_ref())?
        {
            add_artifact_issue(&mut pending, &scan_id, &detected_at, temporary);
        }

        for item in self
            .repo
            .list_recovery_items()?
            .into_iter()
            .filter(|item| item.state != RecoveryState::Deleted)
        {
            let key = format!("recovery-bundle:{}", item.id.0);
            let safe_actions = match item.state {
                RecoveryState::Available | RecoveryState::Expired => vec![
                    RecoveryActionKind::RestoreBundle,
                    RecoveryActionKind::DeleteBundle,
                    RecoveryActionKind::ShowLocation,
                    RecoveryActionKind::Retain,
                ],
                RecoveryState::Restored => vec![
                    RecoveryActionKind::DeleteBundle,
                    RecoveryActionKind::ShowLocation,
                    RecoveryActionKind::Retain,
                ],
                RecoveryState::Restoring => {
                    vec![RecoveryActionKind::ShowLocation, RecoveryActionKind::Retain]
                }
                RecoveryState::Deleted => Vec::new(),
            };
            pending.insert(key.clone(), RecoveryIssue {
                issue_id: stable_issue_id(&key), revision: 1, scan_id: Some(scan_id.clone()), stable_key: key,
                kind: RecoveryIssueKind::RecoveryBundle,
                severity: match item.state { RecoveryState::Restoring => RecoverySeverity::Immediate, RecoveryState::Expired => RecoverySeverity::Deferred, _ => RecoverySeverity::Informational },
                status: RecoveryIssueStatus::Detected, task_id: Some(item.task_id.clone()), resource_id: item.id.0.clone(),
                canonical_path: Some(item.directory.clone()),
                evidence: serde_json::json!({"state":item.state,"manifestPath":item.manifest_path,"expiresAt":item.expires_at,"contents":["branch.bundle","tracked.patch","staged.patch","untracked.zip","manifest.json"]}),
                impact: "A verified recovery package is retained independently from its original Worktree.".into(),
                recommended_action: "Restore it to its proven managed target, retain it, or delete it through an approved exact-item plan.".into(),
                safe_actions, detected_at: detected_at.clone(),
            });
        }

        let history = self.repo.list_recovery_history()?;
        let mut latest_history = BTreeMap::<String, RecoveryIssue>::new();
        for historical in &history.issues {
            if latest_history
                .get(&historical.issue_id)
                .is_none_or(|current| historical.revision > current.revision)
            {
                latest_history.insert(historical.issue_id.clone(), historical.clone());
            }
        }
        for historical in latest_history.values().filter(|issue| {
            issue.kind != RecoveryIssueKind::PersistenceMarker
                && matches!(
                    issue.status,
                    RecoveryIssueStatus::Ready | RecoveryIssueStatus::Executing
                )
        }) {
            let Some(plan) = history
                .plans
                .iter()
                .filter(|plan| {
                    plan.issue_id == historical.issue_id
                        && plan.issue_revision <= historical.revision
                })
                .max_by_key(|plan| plan.issue_revision)
            else {
                continue;
            };
            let key = format!("persistence-marker:{}", plan.id);
            pending.insert(key.clone(), RecoveryIssue {
                issue_id: stable_issue_id(&key), revision: 1, scan_id: Some(scan_id.clone()), stable_key: key,
                kind: RecoveryIssueKind::PersistenceMarker,
                severity: if historical.status == RecoveryIssueStatus::Executing { RecoverySeverity::Immediate } else { RecoverySeverity::Deferred },
                status: RecoveryIssueStatus::Detected, task_id: historical.task_id.clone(), resource_id: plan.id.clone(),
                canonical_path: historical.canonical_path.clone(),
                evidence: serde_json::json!({"issueId":historical.issue_id,"issueRevision":historical.revision,"issueStatus":historical.status,"action":plan.action_kind,"planExpiresAtEpoch":plan.expires_at_epoch}),
                impact: "A durable recovery action marker did not reach a terminal step result.".into(),
                recommended_action: "Retain the resource and prepare a fresh plan from a new scan.".into(),
                safe_actions: vec![RecoveryActionKind::Retain], detected_at: detected_at.clone(),
            });
        }

        let mut issues: Vec<RecoveryIssue> = pending.into_values().collect();
        for issue in &mut issues {
            issue.revision = self
                .repo
                .latest_recovery_issue(&issue.stable_key)?
                .map_or(1, |previous| previous.revision.saturating_add(1));
        }
        self.repo.create_recovery_scan(&RecoveryScan {
            id: scan_id,
            trigger_kind: trigger_kind.into(),
            started_at,
            completed_at: utc_now(),
            issue_count: issues.len() as u32,
        })?;
        for issue in &issues {
            self.repo.append_recovery_issue(issue)?;
        }
        Ok(issues)
    }

    fn get_issue(
        &self,
        issue_id: &str,
        revision: Option<u32>,
    ) -> Result<RecoveryIssue, DomainError> {
        self.repo.get_recovery_issue(issue_id, revision)
    }

    fn prepare_action(
        &self,
        issue_id: &str,
        revision: u32,
        action: RecoveryActionKind,
    ) -> Result<RecoveryActionPlan, DomainError> {
        let issue = self.repo.get_recovery_issue(issue_id, Some(revision))?;
        let latest = self.repo.get_recovery_issue(issue_id, None)?;
        if latest.revision != revision {
            return Err(recovery_error(
                "RECOVERY_PLAN_STALE",
                "The recovery issue changed; scan and assess it again",
            ));
        }
        if !issue.safe_actions.contains(&action) {
            return Err(recovery_error(
                "RECOVERY_ACTION_REJECTED",
                "The action is not safe for this issue revision",
            ));
        }

        let mut context = serde_json::json!({});
        let mut expected = issue.evidence.clone();
        let mut steps = vec!["revalidate issue revision".into()];
        let destructive = matches!(
            action,
            RecoveryActionKind::VerifyAndCleanup
                | RecoveryActionKind::AbortIntegration
                | RecoveryActionKind::RestoreBundle
                | RecoveryActionKind::DeleteBundle
        );
        if action == RecoveryActionKind::VerifyAndCleanup
            && issue.kind == RecoveryIssueKind::WorktreeMismatch
        {
            let task_id = issue.task_id.as_ref().ok_or_else(|| {
                recovery_error("RECOVERY_OWNERSHIP_UNPROVEN", "The issue has no task owner")
            })?;
            let preparation = self
                .workspace
                .prepare_removal(&task_id.0)
                .map_err(map_workspace)?;
            let bundle = match preparation
                .recovery
                .as_ref()
                .map(|_| self.persist_bundle(&issue, &preparation))
                .transpose()
            {
                Ok(bundle) => bundle,
                Err(error) => {
                    let _ = self.workspace.cancel_removal(&task_id.0);
                    return Err(error);
                }
            };
            if preparation.force_required && bundle.as_ref().is_none_or(|item| !item.verified) {
                let _ = self.workspace.cancel_removal(&task_id.0);
                return Err(recovery_error(
                    "RECOVERY_BUNDLE_REQUIRED",
                    "Verified recovery evidence is required before cleanup",
                ));
            }
            expected = serde_json::json!({"resourceId":issue.resource_id,"canonicalPath":preparation.absolute_path,"forceRequired":preparation.force_required,"recoveryBundleId":bundle.map(|item|item.id)});
            context = serde_json::json!({"confirmationToken":preparation.confirmation_token,"confirmedPath":preparation.absolute_path});
            steps.extend([
                "verify independent recovery bundle".into(),
                "revalidate Git/database/filesystem identity".into(),
                "remove exact managed Worktree".into(),
            ]);
        } else if action == RecoveryActionKind::VerifyAndCleanup
            && issue.kind == RecoveryIssueKind::ArtifactTemporaryFile
        {
            steps.extend([
                "revalidate temporary artifact digest".into(),
                "remove exact temporary file".into(),
            ]);
        } else if action == RecoveryActionKind::AbortIntegration {
            steps.extend([
                "abort frozen integration attempt".into(),
                "verify recovery evidence and clean temporary resources".into(),
            ]);
        } else if matches!(
            action,
            RecoveryActionKind::RestoreBundle | RecoveryActionKind::DeleteBundle
        ) {
            if issue.kind != RecoveryIssueKind::RecoveryBundle {
                return Err(recovery_error(
                    "RECOVERY_ACTION_REJECTED",
                    "Bundle action requires a recovery bundle issue",
                ));
            }
            let verified = self
                .workspace
                .verify_recovery_bundle(&issue.resource_id)
                .map_err(map_workspace)?;
            expected = serde_json::json!({"recoveryItemId":issue.resource_id,"manifestSha256":verified.manifest_sha256,"canonicalPath":issue.canonical_path});
            if action == RecoveryActionKind::RestoreBundle {
                steps.extend([
                    "revalidate bundle and repository identity".into(),
                    "restore branch and exact managed Worktree".into(),
                    "apply tracked/staged/untracked evidence".into(),
                ]);
            } else {
                steps.extend([
                    "revalidate bundle root and manifest hash".into(),
                    "delete only known regular bundle files".into(),
                    "mark recovery item deleted".into(),
                ]);
            }
        }

        let assessed_issue = self.append_status(&issue, RecoveryIssueStatus::Assessed)?;
        let ready_issue = self.append_status(&assessed_issue, RecoveryIssueStatus::Ready)?;
        let id = format!("recovery-plan-{}", uuid::Uuid::new_v4());
        let expires_at_epoch = epoch_now().saturating_add(600);
        let digest_input = serde_json::json!({"id":id,"issueId":issue_id,"revision":ready_issue.revision,"action":action,"resource":issue.resource_id,"path":issue.canonical_path,"expected":expected,"expires":expires_at_epoch});
        let approval_digest = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&digest_input).unwrap_or_default())
        );
        let plan = RecoveryActionPlan {
            id,
            issue_id: issue_id.into(),
            issue_revision: ready_issue.revision,
            action_kind: action,
            resource_identity: issue.resource_id,
            canonical_path: issue.canonical_path,
            expected_state: expected,
            steps,
            internal_context: context,
            destructive_level: if destructive {
                "destructive"
            } else {
                "non_destructive"
            }
            .into(),
            approval_digest,
            expires_at_epoch,
            created_at: utc_now(),
        };
        self.repo.create_recovery_action_plan(&plan)?;
        Ok(plan)
    }

    fn execute_action(
        &self,
        plan_id: &str,
        approval_digest: &str,
    ) -> Result<RecoveryIssue, DomainError> {
        let plan = self.repo.get_recovery_action_plan(plan_id)?;
        if !constant_time_equal(&plan.approval_digest, approval_digest) {
            return Err(recovery_error(
                "RECOVERY_APPROVAL_INVALID",
                "Recovery approval evidence does not match the plan",
            ));
        }
        if epoch_now() > plan.expires_at_epoch {
            if plan.action_kind == RecoveryActionKind::VerifyAndCleanup {
                if let Ok(issue) = self
                    .repo
                    .get_recovery_issue(&plan.issue_id, Some(plan.issue_revision))
                {
                    if let Some(task) = issue.task_id {
                        let _ = self.workspace.cancel_removal(&task.0);
                    }
                }
            }
            return Err(recovery_error(
                "RECOVERY_PLAN_EXPIRED",
                "The recovery plan expired; prepare it again",
            ));
        }
        let issue = self.repo.get_recovery_issue(&plan.issue_id, None)?;
        if issue.revision != plan.issue_revision || issue.status != RecoveryIssueStatus::Ready {
            return Err(recovery_error(
                "RECOVERY_PLAN_STALE",
                "The resource changed after the recovery plan was prepared",
            ));
        }
        let executing = self.append_status(&issue, RecoveryIssueStatus::Executing)?;
        self.record_step(
            &plan,
            0,
            "revalidate issue revision",
            "passed",
            "Issue revision and approval matched",
        );

        let result: Result<(), DomainError> = (|| match plan.action_kind {
            RecoveryActionKind::Retain => Ok(()),
            RecoveryActionKind::MarkInterrupted => {
                let task = executing.task_id.as_ref().ok_or_else(|| {
                    recovery_error("RECOVERY_OWNERSHIP_UNPROVEN", "No task owner is recorded")
                })?;
                self.repo.update_task_status(
                    &task.0,
                    "interrupted",
                    Some("marked by Recovery Center"),
                )
            }
            RecoveryActionKind::ResumeSession => {
                let task = executing.task_id.clone().ok_or_else(|| {
                    recovery_error("RECOVERY_OWNERSHIP_UNPROVEN", "No task owner is recorded")
                })?;
                self.repo.apply_recovery_decision(&RecoveryDecision {
                    task_id: task,
                    action: crate::domain::types::RecoveryAction::Resume,
                    decided_at: utc_now(),
                })
            }
            RecoveryActionKind::ContinueIntegration => self
                .workspace
                .get_integration_status(&executing.resource_id)
                .map_err(map_workspace)
                .map(|_| ()),
            RecoveryActionKind::AbortIntegration => {
                self.workspace
                    .abort_integration(&executing.resource_id)
                    .map_err(map_workspace)?;
                self.workspace
                    .cleanup_integration(&executing.resource_id)
                    .map_err(map_workspace)
                    .map(|_| ())
            }
            RecoveryActionKind::ShowLocation => match executing.kind {
                RecoveryIssueKind::TemporaryIntegration => self
                    .workspace
                    .open_integration_worktree(&executing.resource_id)
                    .map_err(map_workspace),
                RecoveryIssueKind::WorktreeMismatch => {
                    let task = executing.task_id.as_ref().ok_or_else(|| {
                        recovery_error("RECOVERY_OWNERSHIP_UNPROVEN", "No task owner is recorded")
                    })?;
                    self.workspace.open_worktree(&task.0).map_err(map_workspace)
                }
                RecoveryIssueKind::ArtifactTemporaryFile => {
                    let task = executing.task_id.as_ref().ok_or_else(|| {
                        recovery_error("RECOVERY_OWNERSHIP_UNPROVEN", "No task owner is recorded")
                    })?;
                    let digest = executing
                        .evidence
                        .get("sha256")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| {
                            recovery_error(
                                "RECOVERY_STATE_INVALID",
                                "Artifact digest evidence is missing",
                            )
                        })?;
                    self.artifacts.reveal_temporary_file(
                        self.repo.as_ref(),
                        task,
                        &executing.resource_id,
                        digest,
                    )
                }
                RecoveryIssueKind::RecoveryBundle => self
                    .workspace
                    .open_recovery_bundle(&executing.resource_id)
                    .map_err(map_workspace),
                _ => Err(recovery_error(
                    "RECOVERY_ACTION_REJECTED",
                    "This issue has no verified location",
                )),
            },
            RecoveryActionKind::VerifyAndCleanup => {
                if executing.kind == RecoveryIssueKind::ArtifactTemporaryFile {
                    let task = executing.task_id.as_ref().ok_or_else(|| {
                        recovery_error("RECOVERY_OWNERSHIP_UNPROVEN", "No task owner is recorded")
                    })?;
                    let digest = executing
                        .evidence
                        .get("sha256")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| {
                            recovery_error(
                                "RECOVERY_STATE_INVALID",
                                "Artifact digest evidence is missing",
                            )
                        })?;
                    self.artifacts.cleanup_temporary_file(
                        self.repo.as_ref(),
                        task,
                        &executing.resource_id,
                        digest,
                    )
                } else {
                    let task = executing.task_id.as_ref().ok_or_else(|| {
                        recovery_error("RECOVERY_OWNERSHIP_UNPROVEN", "No task owner is recorded")
                    })?;
                    let token = plan
                        .internal_context
                        .get("confirmationToken")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| {
                            recovery_error(
                                "RECOVERY_STATE_INVALID",
                                "Cleanup confirmation evidence is missing",
                            )
                        })?;
                    let path = plan
                        .internal_context
                        .get("confirmedPath")
                        .and_then(serde_json::Value::as_str)
                        .ok_or_else(|| {
                            recovery_error(
                                "RECOVERY_STATE_INVALID",
                                "Cleanup path evidence is missing",
                            )
                        })?;
                    self.workspace
                        .remove_managed_worktree(&task.0, token, Path::new(path))
                        .map_err(map_workspace)
                        .map(|_| ())
                }
            }
            RecoveryActionKind::Reregister => {
                let reconciled = self.workspace.reconcile_registry().map_err(map_workspace)?;
                let record = reconciled
                    .into_iter()
                    .find(|record| record.id.0 == executing.resource_id)
                    .ok_or_else(|| {
                        recovery_error(
                            "RECOVERY_MANUAL_ACTION_REQUIRED",
                            "Reconciliation did not prove the registered Worktree",
                        )
                    })?;
                if matches!(
                    record.state,
                    WorktreeState::Ready | WorktreeState::Dirty | WorktreeState::Active
                ) {
                    Ok(())
                } else {
                    Err(recovery_error(
                        "RECOVERY_MANUAL_ACTION_REQUIRED",
                        "Reconciliation did not restore a usable Worktree state",
                    ))
                }
            }
            RecoveryActionKind::RestoreBundle => {
                let digest = plan
                    .expected_state
                    .get("manifestSha256")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        recovery_error(
                            "RECOVERY_STATE_INVALID",
                            "Bundle manifest evidence is missing",
                        )
                    })?;
                self.workspace
                    .restore_recovery_bundle(&executing.resource_id, digest)
                    .map_err(map_workspace)
                    .map(|_| ())
            }
            RecoveryActionKind::DeleteBundle => {
                let digest = plan
                    .expected_state
                    .get("manifestSha256")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| {
                        recovery_error(
                            "RECOVERY_STATE_INVALID",
                            "Bundle manifest evidence is missing",
                        )
                    })?;
                self.workspace
                    .delete_recovery_bundle(&executing.resource_id, digest)
                    .map_err(map_workspace)
                    .map(|_| ())
            }
        })();

        match result {
            Ok(()) => {
                self.record_step(
                    &plan,
                    (plan.steps.len().saturating_sub(1)) as u32,
                    plan.steps.last().map(String::as_str).unwrap_or("complete"),
                    "passed",
                    "Action completed within the planned resource boundary",
                );
                self.append_status(
                    &executing,
                    if matches!(
                        plan.action_kind,
                        RecoveryActionKind::Retain
                            | RecoveryActionKind::ShowLocation
                            | RecoveryActionKind::ContinueIntegration
                    ) {
                        RecoveryIssueStatus::Retained
                    } else {
                        RecoveryIssueStatus::Resolved
                    },
                )
            }
            Err(error) => {
                self.record_step(
                    &plan,
                    (plan.steps.len().saturating_sub(1)) as u32,
                    plan.steps.last().map(String::as_str).unwrap_or("execute"),
                    "failed",
                    &error.code,
                );
                let _ = self.append_status(
                    &executing,
                    if error.code.contains("STALE")
                        || error.code.contains("MANUAL")
                        || error.code.contains("UNPROVEN")
                    {
                        RecoveryIssueStatus::NeedsManualAction
                    } else {
                        RecoveryIssueStatus::Failed
                    },
                );
                Err(error)
            }
        }
    }

    fn create_bundle(
        &self,
        issue_id: &str,
        revision: u32,
    ) -> Result<RecoveryBundleRecord, DomainError> {
        let issue = self.repo.get_recovery_issue(issue_id, Some(revision))?;
        if self.repo.get_recovery_issue(issue_id, None)?.revision != revision {
            return Err(recovery_error(
                "RECOVERY_PLAN_STALE",
                "The issue changed before bundle creation",
            ));
        }
        let task = issue.task_id.as_ref().ok_or_else(|| {
            recovery_error("RECOVERY_OWNERSHIP_UNPROVEN", "No task owner is recorded")
        })?;
        let preparation = self
            .workspace
            .prepare_removal(&task.0)
            .map_err(map_workspace)?;
        let result = self.persist_bundle(&issue, &preparation);
        let _ = self.workspace.cancel_removal(&task.0);
        result
    }

    fn verify_bundle(&self, bundle_id: &str) -> Result<RecoveryBundleRecord, DomainError> {
        let mut record = self.repo.get_recovery_bundle_record(bundle_id)?;
        let verified = self
            .workspace
            .verify_recovery_bundle(&record.recovery_item_id)
            .map_err(map_workspace)?;
        if !constant_time_equal(&record.manifest_sha256, &verified.manifest_sha256) {
            return Err(recovery_error(
                "RECOVERY_BUNDLE_INVALID",
                "Recovery bundle manifest changed after creation",
            ));
        }
        record.verified = true;
        Ok(record)
    }

    fn list_history(&self) -> Result<RecoveryHistory, DomainError> {
        self.repo.list_recovery_history()
    }
}

fn add_artifact_issue(
    pending: &mut BTreeMap<String, RecoveryIssue>,
    scan_id: &str,
    detected_at: &str,
    temporary: ArtifactTemporaryFile,
) {
    let key = format!("artifact-temp:{}:{}", temporary.task_id.0, temporary.sha256);
    pending.insert(
        key.clone(),
        RecoveryIssue {
            issue_id: stable_issue_id(&key),
            revision: 1,
            scan_id: Some(scan_id.into()),
            stable_key: key,
            kind: RecoveryIssueKind::ArtifactTemporaryFile,
            severity: RecoverySeverity::Deferred,
            status: RecoveryIssueStatus::Detected,
            task_id: Some(temporary.task_id),
            resource_id: temporary.path.clone(),
            canonical_path: Some(temporary.path),
            evidence: serde_json::json!({"bytes":temporary.bytes,"sha256":temporary.sha256}),
            impact: "An interrupted artifact import left an unreferenced managed temporary file."
                .into(),
            recommended_action:
                "Revalidate the file identity, then remove only this temporary file.".into(),
            safe_actions: vec![
                RecoveryActionKind::VerifyAndCleanup,
                RecoveryActionKind::ShowLocation,
                RecoveryActionKind::Retain,
            ],
            detected_at: detected_at.into(),
        },
    );
}

fn stable_issue_id(stable_key: &str) -> String {
    let digest = format!("{:x}", Sha256::digest(stable_key.as_bytes()));
    format!("recovery-issue-{}", &digest[..24])
}

fn epoch_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn constant_time_equal(left: &str, right: &str) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.as_bytes()
        .iter()
        .zip(right.as_bytes())
        .fold(0u8, |diff, (a, b)| diff | (a ^ b))
        == 0
}

fn map_workspace(error: WorkspaceError) -> DomainError {
    DomainError::new(error.code, error.message)
}

fn recovery_error(code: &'static str, message: &'static str) -> DomainError {
    DomainError::new(code, message)
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
