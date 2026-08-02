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

    /// Create a session-scoped event with the required fields.
    pub fn session(
        event_type: impl Into<String>,
        task_id: TaskId,
        session_id: SessionId,
        seq: u64,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            event_type: event_type.into(),
            task_id: Some(task_id),
            session_id: Some(session_id),
            seq: Some(seq),
            timestamp: super::types::utc_now(),
            payload,
        }
    }
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
        let ev = DesktopEvent::session(
            "message.delta",
            super::super::types::TaskId::new("t1"),
            super::super::types::SessionId::new("s1"),
            42,
            serde_json::json!({"text": "hello"}),
        );
        let json = serde_json::to_string(&ev).unwrap();
        assert!(json.contains("\"taskId\":\"t1\""));
        assert!(json.contains("\"sessionId\":\"s1\""));
        assert!(json.contains("\"seq\":42"));
    }
}
