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
    /// Create a non-session event (no taskId / sessionId / seq).
    pub fn new(event_type: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            event_type: event_type.into(),
            task_id: None,
            session_id: None,
            seq: None,
            timestamp: super::types::utc_now(),
            payload,
        }
    }

    // Note: session-scoped events must be constructed via `SessionEvent::build`
    // to guarantee that taskId, sessionId, and seq are always present.
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
    pub fn new(
        event_type: impl Into<String>,
        task_id: TaskId,
        session_id: SessionId,
        seq: u64,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            event_type: event_type.into(),
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequestedPayload {
    pub request_id: String,
    pub options: Vec<PermissionOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOption {
    pub id: String,
    pub label: String,
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
        let ev = DesktopEvent::new("runtime.updated", serde_json::json!({"status":"ready"}));
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
