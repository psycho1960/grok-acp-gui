//! Client request types — what the upper layer sends to the Agent Runtime.
//!
//! These are the stable, protocol-agnostic requests that `AgentRuntime::send`
//! accepts.  The adapter layer translates them into JSON-RPC messages.

use serde::{Deserialize, Serialize};

use super::events::RequestId;

/// A request the caller wants the agent to process.
///
/// Variants map to ACP operations but do NOT leak JSON-RPC method names
/// or parameter structures.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClientRequest {
    /// Send a user message to the agent (starts a turn).
    Prompt(PromptRequest),
    /// Cancel the current turn (idempotent).
    Cancel,
    /// Resolve a permission request raised by the agent.
    ResolvePermission(ResolvePermissionRequest),
    /// Resolve a plan proposal.
    ResolvePlan(ResolvePlanRequest),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptRequest {
    /// The user's message text.
    pub message: String,
    /// Attachment IDs managed by the artifacts module (optional).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub attachments: Vec<String>,
    /// Mode override (e.g. "code", "ask"). When `None`, uses session default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Model override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Reasoning effort override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvePermissionRequest {
    /// ACP request ID from `PermissionRequestedPayload`.
    pub request_id: String,
    /// ACP option ID — passed verbatim, never inferred from labels.
    pub option_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvePlanRequest {
    /// ACP request ID from `PlanProposedPayload`.
    pub request_id: String,
    /// ACP option ID — passed verbatim.
    pub option_id: String,
}

/// Result of `AgentRuntime::send` — the allocated request ID.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendAck {
    pub request_id: RequestId,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_request_round_trip() {
        let req = ClientRequest::Prompt(PromptRequest {
            message: "fix the bug".into(),
            attachments: vec!["att-1".into()],
            mode: Some("code".into()),
            model: None,
            reasoning: Some("high".into()),
        });
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"kind\":\"prompt\""));
        assert!(json.contains("\"message\":\"fix the bug\""));
        let back: ClientRequest = serde_json::from_str(&json).unwrap();
        match back {
            ClientRequest::Prompt(p) => {
                assert_eq!(p.message, "fix the bug");
                assert_eq!(p.mode.as_deref(), Some("code"));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn cancel_has_no_payload() {
        let req = ClientRequest::Cancel;
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"kind\":\"cancel\""));
    }
}
