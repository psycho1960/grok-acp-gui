//! ACP message interpreter — translates raw JSON-RPC messages into
//! the internal `AgentEvent` vocabulary.
//!
//! The interpreter is **stateless** except for a per-session context
//! that tracks the current request ID (for correlation) and accumulated
//! assistant text (for `assistant_completed`).
//!
//! Unknown methods are surfaced as `InterpretationResult::Unknown`
//! and must NOT crash the process (GAG-005 §11).

use crate::bridge::types::{CorrelationId, SessionId};
use crate::modules::agent_runtime::events::*;

use super::codec::{AcpMessage, AcpNotification, AcpRequest};

/// Per-session state needed for interpretation.
#[derive(Debug, Clone, Default)]
pub struct AcpSessionContext {
    /// The client request ID currently being processed (for correlation).
    pub current_request_id: Option<u64>,
    /// Accumulated assistant text from deltas.
    pub accumulated_text: String,
    /// Next sequence number to assign.
    pub next_sequence: Sequence,
}

impl AcpSessionContext {
    pub fn new() -> Self {
        Self::default()
    }

    fn next_seq(&mut self) -> Sequence {
        let s = self.next_sequence;
        self.next_sequence += 1;
        s
    }
}

/// The result of interpreting a single ACP message.
#[derive(Debug, Clone)]
pub enum InterpretationResult {
    /// The message produced one or more agent events.
    Events(Vec<TimestampedEvent>),
    /// The message was a response to a client request and carries
    /// no event (e.g. a `session/cancel` ack).
    Ack,
    /// The message was recognised but did not map to any event
    /// (e.g. a heartbeat or capability update with no user-visible
    /// content).  Logged for audit.
    NoEvent,
    /// The message was not recognised.  The raw method name is
    /// preserved for diagnostics but must NOT cause a crash.
    Unknown { method: String },
    /// The message indicates a protocol error (malformed params, etc.).
    ProtocolError { message: String },
}

/// Interpret a single decoded ACP message.
///
/// `session_id` identifies which session the message belongs to.
/// `ctx` holds per-session interpretation state and is mutated in place.
pub fn interpret(
    msg: &AcpMessage,
    session_id: &SessionId,
    ctx: &mut AcpSessionContext,
) -> InterpretationResult {
    match msg {
        AcpMessage::Request(req) => interpret_request(req, session_id, ctx),
        AcpMessage::Notification(notif) => interpret_notification(notif, session_id, ctx),
        AcpMessage::Response(resp) => {
            // Responses to client requests are acks or error carriers.
            if let Some(ref error) = resp.error {
                let request_id = extract_id_as_u64(&resp.id).or(ctx.current_request_id);
                let event = AgentEvent::RequestFailed(RequestFailedPayload {
                    request_id,
                    code: format!("ACP_{}", error.code),
                    message: error.message.clone(),
                });
                let meta = EventMeta::new(session_id.clone(), ctx.next_seq()).with_correlation(
                    CorrelationId::new(format!("req-{}", request_id.unwrap_or(0))),
                );
                InterpretationResult::Events(vec![TimestampedEvent { meta, event }])
            } else {
                InterpretationResult::Ack
            }
        }
        AcpMessage::Unknown(_) => InterpretationResult::NoEvent,
    }
}

fn interpret_request(
    req: &AcpRequest,
    session_id: &SessionId,
    ctx: &mut AcpSessionContext,
) -> InterpretationResult {
    match req.method.as_str() {
        "requestPermission" => {
            let params = &req.params;
            let tool_call = extract_tool_call(params);
            let options = extract_permission_options(params);
            let request_id = extract_string_field(params, "requestId")
                .or_else(|| extract_string_field(params, "id"))
                .unwrap_or_else(|| req.id.as_str().unwrap_or("").to_string());

            let event = AgentEvent::PermissionRequested(PermissionRequestedPayload {
                request_id,
                tool_call,
                options,
            });
            let meta = EventMeta::new(session_id.clone(), ctx.next_seq())
                .with_correlation(CorrelationId::new(format!("perm-{}", req.id)));
            InterpretationResult::Events(vec![TimestampedEvent { meta, event }])
        }

        "updatePlan" => {
            let params = &req.params;
            let request_id = extract_string_field(params, "requestId")
                .or_else(|| extract_string_field(params, "id"))
                .unwrap_or_else(|| req.id.as_str().unwrap_or("").to_string());
            let summary = extract_string_field(params, "summary")
                .or_else(|| extract_string_field(params, "plan"))
                .unwrap_or_default();
            let options = extract_permission_options(params);

            let event = AgentEvent::PlanProposed(PlanProposedPayload {
                request_id,
                summary,
                options,
            });
            let meta = EventMeta::new(session_id.clone(), ctx.next_seq())
                .with_correlation(CorrelationId::new(format!("plan-{}", req.id)));
            InterpretationResult::Events(vec![TimestampedEvent { meta, event }])
        }

        _ => InterpretationResult::Unknown {
            method: req.method.clone(),
        },
    }
}

fn interpret_notification(
    notif: &AcpNotification,
    session_id: &SessionId,
    ctx: &mut AcpSessionContext,
) -> InterpretationResult {
    match notif.method.as_str() {
        "session/update" | "sessionUpdate" => {
            interpret_session_update(&notif.params, session_id, ctx)
        }
        "session/append" => {
            // Some ACP variants use session/append for text deltas.
            let text = extract_string_field(&notif.params, "text")
                .or_else(|| extract_nested_string(&notif.params, &["content", "text"]))
                .unwrap_or_default();
            if text.is_empty() {
                return InterpretationResult::NoEvent;
            }
            ctx.accumulated_text.push_str(&text);
            let event = AgentEvent::AssistantDelta(AssistantDeltaPayload { text });
            let meta = EventMeta::new(session_id.clone(), ctx.next_seq());
            InterpretationResult::Events(vec![TimestampedEvent { meta, event }])
        }
        // ACP may send requestPermission and updatePlan as either requests
        // (with id, expecting response) or notifications (one-way).  The
        // fake agent sends them as notifications; real agents may use either.
        "requestPermission" => {
            let params = &notif.params;
            let tool_call = extract_tool_call(params);
            let options = extract_permission_options(params);
            let request_id = extract_string_field(params, "requestId")
                .or_else(|| extract_string_field(params, "id"))
                .unwrap_or_default();

            let event = AgentEvent::PermissionRequested(PermissionRequestedPayload {
                request_id,
                tool_call,
                options,
            });
            let meta = EventMeta::new(session_id.clone(), ctx.next_seq());
            InterpretationResult::Events(vec![TimestampedEvent { meta, event }])
        }
        "updatePlan" => {
            let params = &notif.params;
            let request_id = extract_string_field(params, "requestId")
                .or_else(|| extract_string_field(params, "id"))
                .unwrap_or_default();
            let summary = extract_string_field(params, "summary")
                .or_else(|| extract_string_field(params, "plan"))
                .unwrap_or_default();
            let options = extract_permission_options(params);

            let event = AgentEvent::PlanProposed(PlanProposedPayload {
                request_id,
                summary,
                options,
            });
            let meta = EventMeta::new(session_id.clone(), ctx.next_seq());
            InterpretationResult::Events(vec![TimestampedEvent { meta, event }])
        }
        _ => InterpretationResult::Unknown {
            method: notif.method.clone(),
        },
    }
}

fn interpret_session_update(
    params: &serde_json::Value,
    session_id: &SessionId,
    ctx: &mut AcpSessionContext,
) -> InterpretationResult {
    // ACP session/update params typically have a "type" or "kind" field.
    let update_type = extract_string_field(params, "type")
        .or_else(|| extract_string_field(params, "kind"))
        .unwrap_or_default();

    match update_type.as_str() {
        "assistantMessage" | "assistant_message" | "text" | "delta" => {
            let text = extract_string_field(params, "content")
                .or_else(|| extract_string_field(params, "text"))
                .or_else(|| extract_nested_string(params, &["content", "text"]))
                .unwrap_or_default();
            if text.is_empty() {
                return InterpretationResult::NoEvent;
            }
            ctx.accumulated_text.push_str(&text);
            let event = AgentEvent::AssistantDelta(AssistantDeltaPayload { text });
            let meta = EventMeta::new(session_id.clone(), ctx.next_seq());
            InterpretationResult::Events(vec![TimestampedEvent { meta, event }])
        }

        "assistantMessageComplete" | "assistant_message_complete" | "message_complete" => {
            let full_text = if ctx.accumulated_text.is_empty() {
                None
            } else {
                let full = std::mem::take(&mut ctx.accumulated_text);
                Some(full)
            };
            let event = AgentEvent::AssistantCompleted(AssistantCompletedPayload { full_text });
            let meta = EventMeta::new(session_id.clone(), ctx.next_seq());
            InterpretationResult::Events(vec![TimestampedEvent { meta, event }])
        }

        "toolCall" | "tool_call" | "toolCallStarted" => {
            let tool_call = extract_tool_call(params);
            let event = AgentEvent::ToolStarted(tool_call);
            let meta = EventMeta::new(session_id.clone(), ctx.next_seq());
            InterpretationResult::Events(vec![TimestampedEvent { meta, event }])
        }

        "toolCallUpdate" | "tool_call_update" => {
            let tool_call = extract_tool_call(params);
            let event = AgentEvent::ToolUpdated(tool_call);
            let meta = EventMeta::new(session_id.clone(), ctx.next_seq());
            InterpretationResult::Events(vec![TimestampedEvent { meta, event }])
        }

        "toolCallComplete" | "tool_call_complete" => {
            let tool_call_id = extract_tool_call_id(params);
            let outcome = extract_string_field(params, "outcome")
                .or_else(|| extract_string_field(params, "status"))
                .unwrap_or_else(|| "unknown".into());
            let summary = extract_string_field(params, "summary")
                .or_else(|| extract_string_field(params, "content"));
            let event = AgentEvent::ToolCompleted(ToolCompletedPayload {
                tool_call_id,
                outcome,
                summary,
            });
            let meta = EventMeta::new(session_id.clone(), ctx.next_seq());
            InterpretationResult::Events(vec![TimestampedEvent { meta, event }])
        }

        "artifact" | "artifactAvailable" => {
            let artifact_id = extract_string_field(params, "artifactId")
                .or_else(|| extract_string_field(params, "id"))
                .unwrap_or_default();
            let mime_type = extract_string_field(params, "mimeType")
                .or_else(|| extract_string_field(params, "mime"))
                .unwrap_or_else(|| "application/octet-stream".into());
            let display_name = extract_string_field(params, "displayName")
                .or_else(|| extract_string_field(params, "name"))
                .unwrap_or_else(|| "artifact".into());
            let event = AgentEvent::ArtifactAnnounced(ArtifactAnnouncedPayload {
                artifact_id,
                mime_type,
                display_name,
            });
            let meta = EventMeta::new(session_id.clone(), ctx.next_seq());
            InterpretationResult::Events(vec![TimestampedEvent { meta, event }])
        }

        _ => {
            // Unknown update type — surface as unknown, do NOT crash.
            InterpretationResult::Unknown {
                method: format!("session/update(type={})", update_type),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Extraction helpers
// ---------------------------------------------------------------------------

fn extract_string_field(params: &serde_json::Value, field: &str) -> Option<String> {
    params
        .get(field)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn extract_nested_string(params: &serde_json::Value, path: &[&str]) -> Option<String> {
    let mut current = params;
    for &key in path {
        current = current.get(key)?;
    }
    current.as_str().map(|s| s.to_string())
}

fn extract_id_as_u64(id: &serde_json::Value) -> Option<u64> {
    id.as_u64()
        .or_else(|| id.as_str().and_then(|s| s.parse().ok()))
}

fn extract_tool_call_id(params: &serde_json::Value) -> String {
    extract_string_field(params, "toolCallId")
        .or_else(|| extract_string_field(params, "tool_call_id"))
        .or_else(|| extract_string_field(params, "id"))
        .unwrap_or_default()
}

fn extract_tool_call(params: &serde_json::Value) -> ToolEventPayload {
    // The tool call info may be at the top level or nested under "toolCall".
    let source = params.get("toolCall").unwrap_or(params);

    ToolEventPayload {
        tool_call_id: extract_tool_call_id(source),
        title: extract_string_field(source, "title")
            .or_else(|| extract_string_field(source, "name")),
        kind: extract_string_field(source, "kind")
            .or_else(|| extract_string_field(source, "toolName"))
            .or_else(|| extract_string_field(source, "type")),
        status: extract_string_field(source, "status"),
    }
}

fn extract_permission_options(params: &serde_json::Value) -> Vec<PermissionOptionDescriptor> {
    let options = params.get("options").or_else(|| params.get("choices"));
    match options {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|opt| {
                let option_id = opt
                    .get("optionId")
                    .or_else(|| opt.get("id"))?
                    .as_str()?
                    .to_string();
                let name = opt
                    .get("name")
                    .or_else(|| opt.get("label"))?
                    .as_str()?
                    .to_string();
                Some(PermissionOptionDescriptor { option_id, name })
            })
            .collect(),
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::grok_acp::codec::{AcpError, AcpResponse};
    use serde_json::json;

    fn ctx() -> AcpSessionContext {
        AcpSessionContext::new()
    }

    fn sid() -> SessionId {
        SessionId::new("s1")
    }

    #[test]
    fn interpret_assistant_delta() {
        let notif = AcpNotification {
            jsonrpc: "2.0".into(),
            method: "session/update".into(),
            params: json!({
                "type": "assistantMessage",
                "content": "Hello "
            }),
        };
        let mut c = ctx();
        let result = interpret(&AcpMessage::Notification(notif), &sid(), &mut c);
        match result {
            InterpretationResult::Events(events) => {
                assert_eq!(events.len(), 1);
                match &events[0].event {
                    AgentEvent::AssistantDelta(d) => assert_eq!(d.text, "Hello "),
                    other => panic!("expected AssistantDelta, got {:?}", other),
                }
            }
            other => panic!("expected Events, got {:?}", other),
        }
        assert_eq!(c.accumulated_text, "Hello ");
    }

    #[test]
    fn interpret_assistant_completed_flushes_accumulated() {
        let mut c = ctx();
        c.accumulated_text = "Hello world".into();

        let notif = AcpNotification {
            jsonrpc: "2.0".into(),
            method: "session/update".into(),
            params: json!({"type": "assistantMessageComplete"}),
        };
        let result = interpret(&AcpMessage::Notification(notif), &sid(), &mut c);
        match result {
            InterpretationResult::Events(events) => match &events[0].event {
                AgentEvent::AssistantCompleted(c) => {
                    assert_eq!(c.full_text.as_deref(), Some("Hello world"));
                }
                other => panic!("expected AssistantCompleted, got {:?}", other),
            },
            other => panic!("expected Events, got {:?}", other),
        }
        // Accumulated text should be flushed.
        assert!(c.accumulated_text.is_empty());
    }

    #[test]
    fn interpret_tool_started() {
        let notif = AcpNotification {
            jsonrpc: "2.0".into(),
            method: "session/update".into(),
            params: json!({
                "type": "toolCall",
                "toolCallId": "tc-1",
                "title": "Edit file",
                "kind": "edit"
            }),
        };
        let mut c = ctx();
        let result = interpret(&AcpMessage::Notification(notif), &sid(), &mut c);
        match result {
            InterpretationResult::Events(events) => match &events[0].event {
                AgentEvent::ToolStarted(t) => {
                    assert_eq!(t.tool_call_id, "tc-1");
                    assert_eq!(t.title.as_deref(), Some("Edit file"));
                    assert_eq!(t.kind.as_deref(), Some("edit"));
                }
                other => panic!("expected ToolStarted, got {:?}", other),
            },
            other => panic!("expected Events, got {:?}", other),
        }
    }

    #[test]
    fn interpret_permission_request_preserves_option_ids() {
        let req = AcpRequest {
            jsonrpc: "2.0".into(),
            id: json!(10),
            method: "requestPermission".into(),
            params: json!({
                "requestId": "perm-abc",
                "toolCall": {
                    "toolCallId": "tc-5",
                    "title": "Run bash",
                    "kind": "bash"
                },
                "options": [
                    {"optionId": "opt-allow-once", "name": "Allow once"},
                    {"optionId": "opt-reject", "name": "Reject"}
                ]
            }),
        };
        let mut c = ctx();
        let result = interpret(&AcpMessage::Request(req), &sid(), &mut c);
        match result {
            InterpretationResult::Events(events) => match &events[0].event {
                AgentEvent::PermissionRequested(p) => {
                    assert_eq!(p.request_id, "perm-abc");
                    assert_eq!(p.options.len(), 2);
                    assert_eq!(p.options[0].option_id, "opt-allow-once");
                    assert_eq!(p.options[1].option_id, "opt-reject");
                }
                other => panic!("expected PermissionRequested, got {:?}", other),
            },
            other => panic!("expected Events, got {:?}", other),
        }
    }

    #[test]
    fn interpret_unknown_method_does_not_crash() {
        let notif = AcpNotification {
            jsonrpc: "2.0".into(),
            method: "some/future/method".into(),
            params: json!({}),
        };
        let mut c = ctx();
        let result = interpret(&AcpMessage::Notification(notif), &sid(), &mut c);
        match result {
            InterpretationResult::Unknown { method } => {
                assert_eq!(method, "some/future/method");
            }
            other => panic!("expected Unknown, got {:?}", other),
        }
    }

    #[test]
    fn interpret_unknown_session_update_type_does_not_crash() {
        let notif = AcpNotification {
            jsonrpc: "2.0".into(),
            method: "session/update".into(),
            params: json!({"type": "someNewUpdateType"}),
        };
        let mut c = ctx();
        let result = interpret(&AcpMessage::Notification(notif), &sid(), &mut c);
        match result {
            InterpretationResult::Unknown { method } => {
                assert!(method.contains("someNewUpdateType"));
            }
            other => panic!("expected Unknown, got {:?}", other),
        }
    }

    #[test]
    fn interpret_response_error_yields_request_failed() {
        let resp = AcpResponse {
            jsonrpc: "2.0".into(),
            id: json!(42),
            result: None,
            error: Some(AcpError {
                code: -32601,
                message: "method not found".into(),
                data: serde_json::Value::Null,
            }),
        };
        let mut c = ctx();
        c.current_request_id = Some(42);
        let result = interpret(&AcpMessage::Response(resp), &sid(), &mut c);
        match result {
            InterpretationResult::Events(events) => match &events[0].event {
                AgentEvent::RequestFailed(f) => {
                    assert_eq!(f.request_id, Some(42));
                    assert!(f.code.contains("32601"));
                }
                other => panic!("expected RequestFailed, got {:?}", other),
            },
            other => panic!("expected Events, got {:?}", other),
        }
    }

    #[test]
    fn interpret_response_success_is_ack() {
        let resp = AcpResponse {
            jsonrpc: "2.0".into(),
            id: json!(1),
            result: Some(json!({"ok": true})),
            error: None,
        };
        let mut c = ctx();
        let result = interpret(&AcpMessage::Response(resp), &sid(), &mut c);
        matches!(result, InterpretationResult::Ack);
    }

    #[test]
    fn sequence_monotonic() {
        let mut c = ctx();
        let s1 = c.next_seq();
        let s2 = c.next_seq();
        let s3 = c.next_seq();
        assert_eq!(s1, 0);
        assert_eq!(s2, 1);
        assert_eq!(s3, 2);
    }

    #[test]
    fn interpret_artifact_announced() {
        let notif = AcpNotification {
            jsonrpc: "2.0".into(),
            method: "session/update".into(),
            params: json!({
                "type": "artifact",
                "artifactId": "art-1",
                "mimeType": "image/png",
                "displayName": "screenshot.png"
            }),
        };
        let mut c = ctx();
        let result = interpret(&AcpMessage::Notification(notif), &sid(), &mut c);
        match result {
            InterpretationResult::Events(events) => match &events[0].event {
                AgentEvent::ArtifactAnnounced(a) => {
                    assert_eq!(a.artifact_id, "art-1");
                    assert_eq!(a.mime_type, "image/png");
                    assert_eq!(a.display_name, "screenshot.png");
                }
                other => panic!("expected ArtifactAnnounced, got {:?}", other),
            },
            other => panic!("expected Events, got {:?}", other),
        }
    }
}
