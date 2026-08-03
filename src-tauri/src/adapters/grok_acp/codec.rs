//! ACP / JSON-RPC 2.0 protocol codec.
//!
//! This module handles **only** the wire format: framing, size limits,
//! depth limits, and field validation.  It does NOT interpret ACP
//! method semantics — that is the adapter's job.
//!
//! # Framing
//! ACP uses newline-delimited JSON-RPC 2.0 over stdio.  Each frame is a
//! complete JSON object on a single line.  The codec reads bytes from
//! stdout, buffers until a newline, and decodes the line as JSON.
//!
//! # Safety invariants (GAG-005 §11)
//! - Frames exceeding `max_frame_bytes` are rejected as protocol errors.
//! - JSON nesting deeper than `max_depth` is rejected.
//! - Invalid UTF-8 lines are rejected (not retried).
//! - Unknown methods are surfaced as `AcpMessage::UnknownRequest` /
//!   `UnknownNotification` — they must NOT crash the process.
//! - stdout content that is not valid JSON is a protocol error; the
//!   codec does NOT attempt TUI / ANSI text parsing.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::modules::agent_runtime::diagnostics;

/// JSON-RPC 2.0 protocol version string.
pub const JSONRPC_VERSION: &str = "2.0";

/// Default maximum frame size: 4 MiB.
pub const DEFAULT_MAX_FRAME_BYTES: u64 = 4 * 1024 * 1024;

/// Default maximum JSON nesting depth.
pub const DEFAULT_MAX_DEPTH: u32 = 64;

// ---------------------------------------------------------------------------
// Wire types — raw JSON-RPC 2.0 envelopes
// ---------------------------------------------------------------------------

/// A parsed JSON-RPC 2.0 message (request, response, or notification).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcpMessage {
    /// A request with an `id` — expects a response.
    Request(AcpRequest),
    /// A notification (no `id`, no response expected).
    Notification(AcpNotification),
    /// A response to a previously-sent request.
    Response(AcpResponse),
    /// A line that parsed as JSON but does not match any JSON-RPC shape.
    /// Surfaced for auditing; must NOT crash.
    Unknown(Value),
}

/// A JSON-RPC request: has `method`, `params`, and `id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpRequest {
    #[serde(rename = "jsonrpc")]
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub params: Value,
}

/// A JSON-RPC notification: has `method` and `params` but no `id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpNotification {
    #[serde(rename = "jsonrpc")]
    pub jsonrpc: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub params: Value,
}

/// A JSON-RPC response: has `id` and either `result` or `error`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpResponse {
    #[serde(rename = "jsonrpc")]
    pub jsonrpc: String,
    pub id: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<AcpError>,
}

/// JSON-RPC error object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcpError {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
}

/// Standard JSON-RPC error codes.
pub mod error_codes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;
}

// ---------------------------------------------------------------------------
// Codec errors
// ---------------------------------------------------------------------------

/// Errors produced by the codec.  These are protocol-level errors, not
/// application errors — they indicate the stream is corrupt or the peer
/// is misbehaving.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    /// Frame exceeded the configured byte limit.
    FrameTooLarge { actual: u64, limit: u64 },
    /// JSON nesting exceeded the depth limit.
    DepthExceeded { limit: u32 },
    /// Line was not valid UTF-8.
    InvalidUtf8,
    /// Line was not valid JSON.
    InvalidJson(String),
    /// Parsed JSON but missing required JSON-RPC fields.
    MalformedRpc(String),
}

impl std::fmt::Display for CodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CodecError::FrameTooLarge { actual, limit } => {
                write!(f, "frame too large: {} bytes (limit {})", actual, limit)
            }
            CodecError::DepthExceeded { limit } => {
                write!(f, "JSON nesting depth exceeded (limit {})", limit)
            }
            CodecError::InvalidUtf8 => write!(f, "invalid UTF-8 in frame"),
            CodecError::InvalidJson(msg) => write!(f, "invalid JSON: {}", msg),
            CodecError::MalformedRpc(msg) => write!(f, "malformed JSON-RPC: {}", msg),
        }
    }
}

impl std::error::Error for CodecError {}

// ---------------------------------------------------------------------------
// Decoder
// ---------------------------------------------------------------------------

/// A stateful decoder for newline-delimited JSON-RPC 2.0 frames.
pub struct FrameDecoder {
    max_frame_bytes: u64,
    max_depth: u32,
    buffer: Vec<u8>,
}

impl FrameDecoder {
    pub fn new(max_frame_bytes: u64, max_depth: u32) -> Self {
        Self {
            max_frame_bytes,
            max_depth,
            buffer: Vec::with_capacity(4096),
        }
    }

    /// Feed raw bytes from stdout.  Returns a list of decoded messages
    /// and/or errors.  Partial lines are buffered until the next feed.
    ///
    /// On a `FrameTooLarge` error, the current line is discarded and
    /// the decoder resynchronises at the next newline.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Result<AcpMessage, CodecError>> {
        let mut results = Vec::new();
        self.buffer.extend_from_slice(bytes);

        loop {
            // Find the next newline.
            let newline_pos = match self.buffer.iter().position(|&b| b == b'\n') {
                Some(pos) => pos,
                None => {
                    // No complete line yet.  Check if the buffer exceeds
                    // the frame limit (defensive — the stream is likely
                    // a single huge line without a newline).
                    if self.buffer.len() as u64 > self.max_frame_bytes {
                        results.push(Err(CodecError::FrameTooLarge {
                            actual: self.buffer.len() as u64,
                            limit: self.max_frame_bytes,
                        }));
                        self.buffer.clear();
                    }
                    break;
                }
            };

            let line_len = newline_pos;
            let line_bytes = &self.buffer[..line_len];

            // Size check before any parsing.
            if line_len as u64 > self.max_frame_bytes {
                results.push(Err(CodecError::FrameTooLarge {
                    actual: line_len as u64,
                    limit: self.max_frame_bytes,
                }));
            } else {
                results.push(self.decode_line(line_bytes));
            }

            // Remove the consumed line + newline from the buffer.
            self.buffer.drain(..=newline_pos);
        }

        results
    }

    fn decode_line(&self, line: &[u8]) -> Result<AcpMessage, CodecError> {
        // Skip empty lines (some agents emit heartbeats as blank lines).
        if line.iter().all(|&b| b == b'\r' || b == b' ') {
            // Return an Unknown with null so the caller can ignore it.
            return Ok(AcpMessage::Unknown(Value::Null));
        }

        // UTF-8 validation.
        let text = std::str::from_utf8(line).map_err(|_| CodecError::InvalidUtf8)?;

        // Parse JSON.
        let value: Value =
            serde_json::from_str(text).map_err(|e| CodecError::InvalidJson(e.to_string()))?;

        // Depth check.
        if check_depth(&value, self.max_depth).is_err() {
            return Err(CodecError::DepthExceeded {
                limit: self.max_depth,
            });
        }

        // Classify into JSON-RPC message type.
        classify_message(value)
    }
}

/// Check that the JSON value does not nest deeper than `max_depth`.
fn check_depth(value: &Value, max_depth: u32) -> Result<(), ()> {
    fn check(value: &Value, current: u32, max: u32) -> Result<(), ()> {
        if current > max {
            return Err(());
        }
        match value {
            Value::Object(map) => {
                for v in map.values() {
                    check(v, current + 1, max)?;
                }
                Ok(())
            }
            Value::Array(arr) => {
                for v in arr {
                    check(v, current + 1, max)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
    check(value, 0, max_depth)
}

/// Classify a parsed JSON value into a JSON-RPC message type.
fn classify_message(value: Value) -> Result<AcpMessage, CodecError> {
    let obj = match &value {
        Value::Object(o) => o,
        _ => {
            return Ok(AcpMessage::Unknown(value));
        }
    };

    // Check jsonrpc version (if present, must be "2.0").
    if let Some(v) = obj.get("jsonrpc") {
        if v.as_str() != Some(JSONRPC_VERSION) {
            return Err(CodecError::MalformedRpc(format!(
                "unsupported jsonrpc version: {:?}",
                v
            )));
        }
    }

    let has_id = obj.contains_key("id");
    let has_method = obj.contains_key("method");
    let has_result = obj.contains_key("result");
    let has_error = obj.contains_key("error");

    if has_method && has_id {
        // Request
        let req: AcpRequest =
            serde_json::from_value(value).map_err(|e| CodecError::MalformedRpc(e.to_string()))?;
        Ok(AcpMessage::Request(req))
    } else if has_method && !has_id {
        // Notification
        let notif: AcpNotification =
            serde_json::from_value(value).map_err(|e| CodecError::MalformedRpc(e.to_string()))?;
        Ok(AcpMessage::Notification(notif))
    } else if has_id && (has_result || has_error) {
        // Response
        let resp: AcpResponse =
            serde_json::from_value(value).map_err(|e| CodecError::MalformedRpc(e.to_string()))?;
        Ok(AcpMessage::Response(resp))
    } else {
        // Doesn't look like JSON-RPC — surface for auditing.
        Ok(AcpMessage::Unknown(value))
    }
}

// ---------------------------------------------------------------------------
// Encoder — serialise outbound JSON-RPC messages
// ---------------------------------------------------------------------------

/// Encode a JSON-RPC request into a newline-terminated string for stdin.
pub fn encode_request(id: u64, method: &str, params: &Value) -> String {
    let json = serde_json::to_string(params).unwrap_or_else(|_| "null".into());
    format!(
        r#"{{"jsonrpc":"2.0","id":{},"method":{},"params":{}}}"#,
        id,
        escape_json_string(method),
        json
    )
}

/// Encode a JSON-RPC notification (no id).
pub fn encode_notification(method: &str, params: &Value) -> String {
    let json = serde_json::to_string(params).unwrap_or_else(|_| "null".into());
    format!(
        r#"{{"jsonrpc":"2.0","method":{},"params":{}}}"#,
        escape_json_string(method),
        json
    )
}

/// Encode a JSON-RPC response (result).
pub fn encode_response_result(id: &Value, result: &Value) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{},"result":{}}}"#,
        serde_json::to_string(id).unwrap_or_else(|_| "null".into()),
        serde_json::to_string(result).unwrap_or_else(|_| "null".into())
    )
}

/// Encode a JSON-RPC error response.
pub fn encode_response_error(id: &Value, error: &AcpError) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{},"error":{{"code":{},"message":"{}","data":{}}}}}"#,
        serde_json::to_string(id).unwrap_or_else(|_| "null".into()),
        error.code,
        escape_json_string(&error.message),
        serde_json::to_string(&error.data).unwrap_or_else(|_| "null".into())
    )
}

fn escape_json_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

// ---------------------------------------------------------------------------
// Redaction helper for diagnostics
// ---------------------------------------------------------------------------

/// Produce a redacted, display-safe summary of a message for logging.
pub fn redact_message(msg: &AcpMessage) -> Value {
    let raw = match msg {
        AcpMessage::Request(r) => serde_json::to_value(r).ok(),
        AcpMessage::Notification(n) => serde_json::to_value(n).ok(),
        AcpMessage::Response(r) => serde_json::to_value(r).ok(),
        AcpMessage::Unknown(v) => Some(v.clone()),
    };
    let raw = raw.unwrap_or(Value::Null);
    diagnostics::redact(&raw, DEFAULT_MAX_DEPTH)
}

// ---------------------------------------------------------------------------
// Tests — property / boundary / negative
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn decoder() -> FrameDecoder {
        FrameDecoder::new(DEFAULT_MAX_FRAME_BYTES, DEFAULT_MAX_DEPTH)
    }

    // --- Happy path ---

    #[test]
    fn decode_request() {
        let line =
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1}}"#;
        let mut d = decoder();
        let results = d.feed(line.as_bytes());
        assert_eq!(results.len(), 0); // no newline yet

        let results = d.feed(b"\n");
        assert_eq!(results.len(), 1);
        match &results[0] {
            Ok(AcpMessage::Request(r)) => {
                assert_eq!(r.method, "initialize");
                assert_eq!(r.id, Value::from(1));
            }
            other => panic!("expected Request, got {:?}", other),
        }
    }

    #[test]
    fn decode_notification() {
        let line = r#"{"jsonrpc":"2.0","method":"session/update","params":{"type":"delta"}}"#;
        let mut d = decoder();
        let results = d.feed(format!("{}\n", line).as_bytes());
        assert_eq!(results.len(), 1);
        match &results[0] {
            Ok(AcpMessage::Notification(n)) => {
                assert_eq!(n.method, "session/update");
            }
            other => panic!("expected Notification, got {:?}", other),
        }
    }

    #[test]
    fn decode_response_result() {
        let line = r#"{"jsonrpc":"2.0","id":2,"result":{"ok":true}}"#;
        let mut d = decoder();
        let results = d.feed(format!("{}\n", line).as_bytes());
        assert_eq!(results.len(), 1);
        match &results[0] {
            Ok(AcpMessage::Response(r)) => {
                assert_eq!(r.id, Value::from(2));
                assert!(r.result.is_some());
                assert!(r.error.is_none());
            }
            other => panic!("expected Response, got {:?}", other),
        }
    }

    #[test]
    fn decode_response_error() {
        let line = r#"{"jsonrpc":"2.0","id":3,"error":{"code":-32601,"message":"method not found","data":null}}"#;
        let mut d = decoder();
        let results = d.feed(format!("{}\n", line).as_bytes());
        match &results[0] {
            Ok(AcpMessage::Response(r)) => {
                assert_eq!(r.id, Value::from(3));
                let err = r.error.as_ref().unwrap();
                assert_eq!(err.code, -32601);
            }
            other => panic!("expected Response, got {:?}", other),
        }
    }

    // --- Multiple frames in one feed ---

    #[test]
    fn decode_multiple_frames_one_feed() {
        let mut d = decoder();
        let bytes = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"a\",\"params\":{}}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"b\",\"params\":{}}\n";
        let results = d.feed(bytes);
        assert_eq!(results.len(), 2);
        assert!(results[0].is_ok());
        assert!(results[1].is_ok());
    }

    #[test]
    fn partial_line_buffered_across_feeds() {
        let mut d = decoder();
        let r1 = d.feed(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"met");
        assert_eq!(r1.len(), 0); // incomplete
        let r2 = d.feed(b"hod\":\"x\",\"params\":{}}\n");
        assert_eq!(r2.len(), 1);
        assert!(r2[0].is_ok());
    }

    // --- Boundary: empty lines ---

    #[test]
    fn empty_line_yields_unknown_null() {
        let mut d = decoder();
        let results = d.feed(b"\n\n");
        assert_eq!(results.len(), 2);
        for r in &results {
            match r {
                Ok(AcpMessage::Unknown(v)) => assert_eq!(*v, Value::Null),
                other => panic!("expected Unknown(null), got {:?}", other),
            }
        }
    }

    // --- Negative: frame too large ---

    #[test]
    fn frame_too_large_rejected() {
        let mut d = FrameDecoder::new(100, DEFAULT_MAX_DEPTH);
        let big = format!(
            "{{\"id\":1,\"method\":\"x\",\"params\":\"{}\"}}\n",
            "x".repeat(200)
        );
        let results = d.feed(big.as_bytes());
        assert_eq!(results.len(), 1);
        match &results[0] {
            Err(CodecError::FrameTooLarge { .. }) => {}
            other => panic!("expected FrameTooLarge, got {:?}", other),
        }
        // Decoder should resynchronise on next line.
        let ok_line = b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"y\",\"params\":{}}\n";
        let results = d.feed(ok_line);
        assert!(results[0].is_ok());
    }

    // --- Negative: depth exceeded ---

    #[test]
    fn depth_exceeded_rejected() {
        let mut d = FrameDecoder::new(DEFAULT_MAX_FRAME_BYTES, 3);
        // Nesting: outer obj (0) -> a (1) -> b (2) -> c (3) -> d (4) — exceeds limit 3
        let deep = r#"{"a":{"b":{"c":{"d":"deep"}}}}"#;
        let results = d.feed(format!("{}\n", deep).as_bytes());
        match &results[0] {
            Err(CodecError::DepthExceeded { .. }) => {}
            other => panic!("expected DepthExceeded, got {:?}", other),
        }
    }

    // --- Negative: invalid UTF-8 ---

    #[test]
    fn invalid_utf8_rejected() {
        let mut d = decoder();
        let bad = b"\xff\xfe\n";
        let results = d.feed(bad);
        match &results[0] {
            Err(CodecError::InvalidUtf8) => {}
            other => panic!("expected InvalidUtf8, got {:?}", other),
        }
    }

    // --- Negative: invalid JSON ---

    #[test]
    fn invalid_json_rejected() {
        let mut d = decoder();
        let results = d.feed(b"not json at all\n");
        match &results[0] {
            Err(CodecError::InvalidJson(_)) => {}
            other => panic!("expected InvalidJson, got {:?}", other),
        }
    }

    // --- Negative: wrong jsonrpc version ---

    #[test]
    fn wrong_jsonrpc_version_rejected() {
        let mut d = decoder();
        let line = r#"{"jsonrpc":"1.0","id":1,"method":"x","params":{}}"#;
        let results = d.feed(format!("{}\n", line).as_bytes());
        match &results[0] {
            Err(CodecError::MalformedRpc(_)) => {}
            other => panic!("expected MalformedRpc, got {:?}", other),
        }
    }

    // --- Unknown shape is NOT an error (audit only) ---

    #[test]
    fn unknown_json_surfaces_as_unknown() {
        let mut d = decoder();
        let line = r#"{"random":"stuff","no":"rpc fields"}"#;
        let results = d.feed(format!("{}\n", line).as_bytes());
        match &results[0] {
            Ok(AcpMessage::Unknown(_)) => {}
            other => panic!("expected Unknown, got {:?}", other),
        }
    }

    // --- Encoder ---

    #[test]
    fn encode_request_round_trip() {
        let params = serde_json::json!({"protocolVersion": 1});
        let encoded = encode_request(1, "initialize", &params);
        assert!(!encoded.ends_with('\n'));
        // Decode it back
        let mut d = decoder();
        let results = d.feed(format!("{}\n", encoded).as_bytes());
        match &results[0] {
            Ok(AcpMessage::Request(r)) => {
                assert_eq!(r.method, "initialize");
                assert_eq!(r.id, Value::from(1));
            }
            other => panic!("round-trip failed: {:?}", other),
        }
    }

    #[test]
    fn encode_notification_no_id() {
        let encoded = encode_notification("session/update", &serde_json::json!({"type":"delta"}));
        assert!(encoded.contains("\"method\":\"session/update\""));
        assert!(!encoded.contains("\"id\""));
    }

    #[test]
    fn test_encode_response_error() {
        let err = AcpError {
            code: -32601,
            message: "not found".into(),
            data: Value::Null,
        };
        let encoded = super::encode_response_error(&Value::from(5), &err);
        assert!(encoded.contains("\"code\":-32601"));
        assert!(encoded.contains("\"id\":5"));
    }

    // --- Redaction ---

    #[test]
    fn redact_message_scrubs_sensitive_params() {
        let line = r#"{"jsonrpc":"2.0","id":1,"method":"authenticate","params":{"token":"sk-secret123","apiKey":"abc"}}"#;
        let mut d = decoder();
        let results = d.feed(format!("{}\n", line).as_bytes());
        match &results[0] {
            Ok(AcpMessage::Request(r)) => {
                let redacted = redact_message(&AcpMessage::Request(r.clone()));
                let json = serde_json::to_string(&redacted).unwrap();
                assert!(json.contains("<redacted>"));
                assert!(!json.contains("sk-secret123"));
            }
            other => panic!("got {:?}", other),
        }
    }

    // --- Large but valid frame ---

    #[test]
    fn large_valid_frame_accepted() {
        let mut d = FrameDecoder::new(1024 * 1024, DEFAULT_MAX_DEPTH);
        // 10 KB of valid JSON
        let big_text = "x".repeat(10_000);
        let line = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"x","params":{{"text":"{}"}}}}"#,
            big_text
        );
        let results = d.feed(format!("{}\n", line).as_bytes());
        assert!(results[0].is_ok());
    }
}
