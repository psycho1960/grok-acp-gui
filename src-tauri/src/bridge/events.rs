//! DesktopEvent — the single event envelope crossing the bridge.
//!
//! All events share a flat envelope.  Session-scoped events (`message.delta`,
//! `permission.requested`, etc.) carry `taskId`, `sessionId`, and a monotonic
//! `seq`.  Non-session events (`runtime.updated`, `resource.warning`, etc.)
//! omit those fields.

use super::types::{SessionId, TaskId};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopEvent {
    /// Discriminant matching the event category (e.g. `"task.snapshot"`).
    #[serde(rename = "type")]
    pub event_type: String,

    /// Present on session-scoped events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,

    /// Present on session-scoped events.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<SessionId>,

    /// Monotonic sequence number (session-scoped events only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,

    /// ISO-8601 UTC timestamp of event creation.
    pub timestamp: String,

    /// Typed payload whose shape is determined by `event_type`.
    pub payload: serde_json::Value,
}

impl DesktopEvent {
    /// Create a **non-session** event.  Panics in debug if given a
    /// session-scoped event type — those must use `SessionEvent::build`.
    pub fn non_session(event_type: impl Into<String>, payload: serde_json::Value) -> Self {
        let t = event_type.into();
        assert!(
            !is_session_event(&t),
            "BUG: session event '{}' constructed via non_session(). Use SessionEvent::build().",
            t
        );
        Self {
            event_type: t,
            task_id: None,
            session_id: None,
            seq: None,
            timestamp: super::types::utc_now(),
            payload,
        }
    }

    /// Create from a `SessionEvent` — the only path to session-scoped events.
    pub fn from_session(se: SessionEvent) -> Self {
        se.build()
    }
}

fn is_session_event(t: &str) -> bool {
    matches!(
        t,
        event_types::TASK_SNAPSHOT
            | event_types::TASK_STATE
            | event_types::MESSAGE_DELTA
            | event_types::ACTIVITY_UPDATED
            | event_types::PERMISSION_REQUESTED
            | event_types::PLAN_UPDATED
            | event_types::CHANGES_UPDATED
            | event_types::ARTIFACT_AVAILABLE
    )
}

// ---------------------------------------------------------------------------
// SessionEvent — type-level guarantee for session-scoped events
// ---------------------------------------------------------------------------

/// A session-scoped event with **required** `taskId`, `sessionId`, and `seq`.
/// Construct via `SessionEvent::build()`; converts to `DesktopEvent` on
/// serialization.
pub struct SessionEvent {
    pub event_type: String,
    pub task_id: TaskId,
    pub session_id: SessionId,
    pub seq: u64,
    pub payload: serde_json::Value,
}

impl SessionEvent {
    /// Create a new session-scoped event.
    /// # Panics
    /// Panics if `event_type` is not a session event type.
    pub fn new(
        event_type: impl Into<String>,
        task_id: TaskId,
        session_id: SessionId,
        seq: u64,
        payload: serde_json::Value,
    ) -> Self {
        let t = event_type.into();
        assert!(
            is_session_event(&t),
            "BUG: SessionEvent::new called with non-session type '{}'",
            t
        );
        Self {
            event_type: t,
            task_id,
            session_id,
            seq,
            payload,
        }
    }

    /// Convert to the flat `DesktopEvent` envelope for emission.
    pub fn build(self) -> DesktopEvent {
        DesktopEvent {
            event_type: self.event_type,
            task_id: Some(self.task_id),
            session_id: Some(self.session_id),
            seq: Some(self.seq),
            timestamp: super::types::utc_now(),
            payload: self.payload,
        }
    }
}

// ---------------------------------------------------------------------------
// Typed event payload DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeUpdatedPayload {
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSnapshotPayload {
    pub tasks: serde_json::Value, // typed in GAG-004
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatePayload {
    pub task_id: TaskId,
    pub status: String,
    pub detail: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageDeltaPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityUpdatedPayload {
    pub kind: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequestedPayload {
    pub request_id: String,
    pub correlation_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_version: Option<u64>,
    pub expires_at_epoch_seconds: u64,
    pub options: Vec<PermissionOption>,
    /// ACP ToolCallUpdate summary for the UI.
    pub tool_call: ToolCallSummary,
    pub operation: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallSummary {
    pub tool_call_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locations: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionOptionKind {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOption {
    /// ACP optionId — passed verbatim.
    pub option_id: String,
    /// Human-readable label.
    pub name: String,
    /// ACP PermissionOptionKind.
    pub kind: PermissionOptionKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanUpdatedPayload {
    pub status: String,
    pub detail: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangesUpdatedPayload {
    pub task_id: TaskId,
    pub files: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactAvailablePayload {
    pub task_id: TaskId,
    pub artifact_id: String,
    pub mime_type: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceWarningPayload {
    pub message: String,
    pub resource: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticNoticePayload {
    pub level: String,
    pub message: String,
    pub source: String,
}

/// Well-known event type strings kept in one place for Rust/TS consistency.
pub mod event_types {
    pub const RUNTIME_UPDATED: &str = "runtime.updated";
    pub const TASK_SNAPSHOT: &str = "task.snapshot";
    pub const TASK_STATE: &str = "task.state";
    pub const MESSAGE_DELTA: &str = "message.delta";
    pub const ACTIVITY_UPDATED: &str = "activity.updated";
    pub const PERMISSION_REQUESTED: &str = "permission.requested";
    pub const PLAN_UPDATED: &str = "plan.updated";
    pub const CHANGES_UPDATED: &str = "changes.updated";
    pub const ARTIFACT_AVAILABLE: &str = "artifact.available";
    pub const RESOURCE_WARNING: &str = "resource.warning";
    pub const DIAGNOSTIC_NOTICE: &str = "diagnostic.notice";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_session_event_omits_optional_fields() {
        let ev =
            DesktopEvent::non_session("runtime.updated", serde_json::json!({"status":"ready"}));
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("\"type\":\"runtime.updated\""));
        assert!(!json.contains("taskId"));
        assert!(!json.contains("sessionId"));
        assert!(!json.contains("\"seq\""));
    }

    #[test]
    fn session_event_includes_all_fields() {
        let ev = SessionEvent::new(
            "message.delta",
            super::super::types::TaskId::new("t1"),
            super::super::types::SessionId::new("s1"),
            42,
            serde_json::json!({"text": "hello"}),
        )
        .build();
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("\"taskId\":\"t1\""));
        assert!(json.contains("\"sessionId\":\"s1\""));
        assert!(json.contains("\"seq\":42"));
    }
}
