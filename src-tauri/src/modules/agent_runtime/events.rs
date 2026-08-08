//! Internal `AgentEvent` types — the stable event vocabulary produced by
//! the Agent Runtime.
//!
//! These events are the **only** information that crosses from the
//! `agent_runtime` module upward to the bridge.  They carry no raw
//! JSON-RPC frames, process handles, or stdout bytes.
//!
//! Every event carries:
//! - `session_id` — which session produced it
//! - `sequence` — per-session monotonic counter
//! - `occurred_at` — ISO-8601 UTC timestamp
//! - `correlation_id` — links the event to the client request that
//!   caused it (when applicable)

use crate::bridge::types::{utc_now, CorrelationId, SessionId};
use serde::{Deserialize, Serialize};

/// Per-session monotonic sequence counter.
pub type Sequence = u64;

/// A unique identifier for a client request sent to the agent.
pub type RequestId = u64;

// ---------------------------------------------------------------------------
// AgentEvent — the discriminated union of all runtime events
// ---------------------------------------------------------------------------

/// A normalized event emitted by the Agent Runtime.
///
/// Variants correspond 1:1 to the event vocabulary required by GAG-005 §9.
/// The bridge layer maps these to `DesktopEvent` payloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentEvent {
    /// User message confirmed by the ACP session stream.
    UserMessage(UserMessagePayload),
    /// Session is ready to accept requests (handshake completed).
    SessionReady(SessionReadyPayload),
    /// A streaming text delta from the assistant.
    AssistantDelta(AssistantDeltaPayload),
    /// The assistant message is complete.
    AssistantCompleted(AssistantCompletedPayload),
    /// A tool call has started.
    ToolStarted(ToolEventPayload),
    /// A tool call has been updated (progress, partial output).
    ToolUpdated(ToolEventPayload),
    /// A tool call has completed.
    ToolCompleted(ToolCompletedPayload),
    /// The agent proposed a plan for user approval.
    PlanProposed(PlanProposedPayload),
    /// The agent requested a permission decision.
    PermissionRequested(PermissionRequestedPayload),
    /// An artifact (image, file result) was announced.
    ArtifactAnnounced(ArtifactAnnouncedPayload),
    /// The ACP session published its available slash commands.
    CommandsUpdated(CommandsUpdatedPayload),
    /// A request failed.
    RequestFailed(RequestFailedPayload),
    /// The current turn was cancelled by the user.
    TurnCancelled(TurnCancelledPayload),
    /// The managed process exited (crash or clean shutdown).
    ProcessExited(ProcessExitedPayload),
}

impl AgentEvent {
    /// Returns the event kind as a static string for logging / dispatch.
    pub fn kind_str(&self) -> &'static str {
        match self {
            AgentEvent::UserMessage(_) => "user_message",
            AgentEvent::SessionReady(_) => "session_ready",
            AgentEvent::AssistantDelta(_) => "assistant_delta",
            AgentEvent::AssistantCompleted(_) => "assistant_completed",
            AgentEvent::ToolStarted(_) => "tool_started",
            AgentEvent::ToolUpdated(_) => "tool_updated",
            AgentEvent::ToolCompleted(_) => "tool_completed",
            AgentEvent::PlanProposed(_) => "plan_proposed",
            AgentEvent::PermissionRequested(_) => "permission_requested",
            AgentEvent::ArtifactAnnounced(_) => "artifact_announced",
            AgentEvent::CommandsUpdated(_) => "commands_updated",
            AgentEvent::RequestFailed(_) => "request_failed",
            AgentEvent::TurnCancelled(_) => "turn_cancelled",
            AgentEvent::ProcessExited(_) => "process_exited",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserMessagePayload {
    pub text: String,
}

/// Common metadata attached to every `AgentEvent`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventMeta {
    pub session_id: SessionId,
    pub sequence: Sequence,
    pub occurred_at: String,
    /// Links to the client request that caused this event, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<CorrelationId>,
}

impl EventMeta {
    pub fn new(session_id: SessionId, sequence: Sequence) -> Self {
        Self {
            session_id,
            sequence,
            occurred_at: utc_now(),
            correlation_id: None,
        }
    }

    pub fn with_correlation(mut self, cid: CorrelationId) -> Self {
        self.correlation_id = Some(cid);
        self
    }
}

/// A full event: metadata + typed payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimestampedEvent {
    #[serde(flatten)]
    pub meta: EventMeta,
    #[serde(flatten)]
    pub event: AgentEvent,
}

// ---------------------------------------------------------------------------
// Payload types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionReadyPayload {
    /// Negotiated ACP protocol version.
    pub protocol_version: u32,
    /// Agent name reported during handshake.
    pub agent_name: String,
    /// Agent version reported during handshake.
    pub agent_version: String,
    /// Available models (if reported).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub models: Vec<ModelDescriptor>,
    /// Available modes (if reported).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub modes: Vec<ModeDescriptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDescriptor {
    pub model_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeDescriptor {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantDeltaPayload {
    /// Incremental text content.
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssistantCompletedPayload {
    /// Full assistant message text (concatenation of all deltas).
    /// May be omitted when only deltas were streamed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolEventPayload {
    pub tool_call_id: String,
    /// Tool name / kind (e.g. "bash", "edit", "read").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// ACP tool-call kind.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Human-readable status / progress summary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_summary: Option<String>,
    pub input_redacted: bool,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub locations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCompletedPayload {
    pub tool_call_id: String,
    /// "success" or "error".
    pub outcome: String,
    /// Display-safe summary of the result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    pub result_redacted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TurnCancelledPayload {}

/// ACP option ID is passed verbatim — never inferred from labels.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequestedPayload {
    /// ACP request ID for this permission prompt.
    pub request_id: String,
    /// ACP tool-call summary.
    pub tool_call: ToolEventPayload,
    /// Options presented by the agent, with original option IDs.
    pub options: Vec<PermissionOptionDescriptor>,
    /// Structured internal operation details for the local execution guard.
    /// This value is never copied wholesale into a DesktopEvent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<PermissionOperationDescriptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOptionDescriptor {
    /// ACP option ID — passed verbatim to the agent on resolution.
    pub option_id: String,
    /// Human-readable label.
    pub name: String,
    /// Explicit ACP semantic discriminator. Missing/unknown values stay
    /// unknown; callers must never infer semantics from `name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOperationDescriptor {
    pub operation_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default)]
    pub read_paths: Vec<String>,
    #[serde(default)]
    pub write_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanProposedPayload {
    /// ACP request ID for this plan prompt.
    pub request_id: String,
    /// Display-safe plan summary.
    pub summary: String,
    /// Options presented by the agent.
    pub options: Vec<PermissionOptionDescriptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactAnnouncedPayload {
    pub artifact_id: String,
    pub mime_type: String,
    /// Display name (sanitised).
    pub display_name: String,
    /// ACP may identify a generated file by a workspace-relative path. This
    /// stays inside the runtime boundary and is never serialised to the UI.
    #[serde(default, skip_serializing)]
    pub relative_path: Option<String>,
}

/// A slash command the ACP agent can execute (Grok Build quick commands).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableCommandDescriptor {
    /// Command name (e.g. "init", "plan", "share").
    pub name: String,
    /// Human-readable description, redacted for display.
    pub description: String,
    /// Whether the command accepts trailing text input.
    pub accepts_input: bool,
}

/// The ACP session published or changed its available commands.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CommandsUpdatedPayload {
    pub commands: Vec<AvailableCommandDescriptor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestFailedPayload {
    /// The request that failed, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<RequestId>,
    /// Machine-readable error code (RUNTIME_* / ACP_*).
    pub code: String,
    /// Display-safe message.
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessExitedPayload {
    /// Exit code if the process exited normally.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<i32>,
    /// Signal if the process was killed by a signal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<String>,
    /// "clean", "crash", "killed", or "unknown".
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_meta_has_timestamp() {
        let meta = EventMeta::new(SessionId::new("s1"), 1);
        assert_eq!(meta.session_id.0, "s1");
        assert_eq!(meta.sequence, 1);
        assert!(!meta.occurred_at.is_empty());
        assert!(meta.correlation_id.is_none());
    }

    #[test]
    fn event_meta_with_correlation() {
        let meta =
            EventMeta::new(SessionId::new("s1"), 5).with_correlation(CorrelationId::new("cid-1"));
        assert_eq!(meta.correlation_id.unwrap().0, "cid-1");
    }

    #[test]
    fn kind_str_matches_expected() {
        let cases: Vec<(AgentEvent, &str)> = vec![
            (
                AgentEvent::SessionReady(SessionReadyPayload {
                    protocol_version: 1,
                    agent_name: "grok".into(),
                    agent_version: "0.2.118".into(),
                    models: vec![],
                    modes: vec![],
                }),
                "session_ready",
            ),
            (
                AgentEvent::AssistantDelta(AssistantDeltaPayload { text: "hi".into() }),
                "assistant_delta",
            ),
            (
                AgentEvent::ProcessExited(ProcessExitedPayload {
                    code: Some(0),
                    signal: None,
                    reason: "clean".into(),
                }),
                "process_exited",
            ),
        ];
        for (ev, expected) in cases {
            assert_eq!(ev.kind_str(), expected);
        }
    }

    #[test]
    fn timestamped_event_serializes_flat() {
        let te = TimestampedEvent {
            meta: EventMeta::new(SessionId::new("s1"), 1),
            event: AgentEvent::AssistantDelta(AssistantDeltaPayload {
                text: "hello".into(),
            }),
        };
        let json = serde_json::to_string(&te).unwrap();
        assert!(json.contains("\"sessionId\":\"s1\""));
        assert!(json.contains("\"sequence\":1"));
        assert!(json.contains("\"kind\":\"assistant_delta\""));
        assert!(json.contains("\"text\":\"hello\""));
    }

    #[test]
    fn permission_option_carries_original_option_id() {
        let opt = PermissionOptionDescriptor {
            option_id: "opt-abc-123".into(),
            name: "Allow once".into(),
            kind: Some("allow_once".into()),
        };
        let json = serde_json::to_string(&opt).unwrap();
        assert!(json.contains("\"optionId\":\"opt-abc-123\""));
    }
}
