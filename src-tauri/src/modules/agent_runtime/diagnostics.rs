//! Structured diagnostics and sensitive-field redaction.
//!
//! GAG-005 §11 requires: "日志不得包含 Token、API Key、环境变量全值、
//! 用户图片内容或未脱敏命令环境" and "stderr 不进入协议解码器；必须限量缓存".
//!
//! This module provides:
//! - `redact()` — scrub known-sensitive keys from a JSON value
//! - `DiagLog` — structured log entry with severity, source, and redacted fields
//! - `StderrBuffer` — bounded ring buffer for stderr lines

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::VecDeque;

/// Keys whose values must be redacted from any diagnostic output.
const SENSITIVE_KEY_PATTERNS: &[&str] = &[
    "token",
    "api_key",
    "apikey",
    "authorization",
    "auth",
    "password",
    "passwd",
    "secret",
    "cookie",
    "xai_api_key",
    "session_state", // ACP sessionState may carry auth context
    "credential",
    "private_key",
];

/// Redact sensitive values from a JSON value, returning a safe copy.
///
/// - Object keys matching `SENSITIVE_KEY_PATTERNS` (case-insensitive,
///   substring match) have their values replaced with `"<redacted>"`.
/// - Strings that look like Bearer tokens or long hex/base64 are masked.
/// - Arrays and nested objects are traversed recursively up to a depth limit.
pub fn redact(value: &Value, max_depth: u32) -> Value {
    redact_inner(value, 0, max_depth)
}

fn redact_inner(value: &Value, depth: u32, max_depth: u32) -> Value {
    if depth > max_depth {
        return Value::String("<truncated:depth>".into());
    }
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                if is_sensitive_key(k) {
                    out.insert(k.clone(), Value::String("<redacted>".into()));
                } else {
                    out.insert(k.clone(), redact_inner(v, depth + 1, max_depth));
                }
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(
            arr.iter()
                .map(|v| redact_inner(v, depth + 1, max_depth))
                .collect(),
        ),
        Value::String(s) => {
            if looks_like_secret(s) {
                Value::String("<redacted>".into())
            } else {
                value.clone()
            }
        }
        _ => value.clone(),
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let lower = key.to_lowercase();
    SENSITIVE_KEY_PATTERNS.iter().any(|p| lower.contains(p))
}

/// Heuristic: a string that looks like a bearer token, long hex, or
/// base64-encoded secret.
fn looks_like_secret(s: &str) -> bool {
    if s.len() < 16 {
        return false;
    }
    let lower = s.to_lowercase();
    if lower.starts_with("bearer ") {
        return true;
    }
    // Long strings of only hex or base64 chars (40+ = likely sha1/api key)
    if s.len() >= 40 && s.chars().all(|c| c.is_ascii_hexdigit()) {
        return true;
    }
    // Base64-looking: [A-Za-z0-9+/=]{40,}
    if s.len() >= 40
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
    {
        return true;
    }
    false
}

// ---------------------------------------------------------------------------
// DiagLog — structured log entry
// ---------------------------------------------------------------------------

/// A structured diagnostic entry, safe for logging and (when needed)
/// for forwarding to the Renderer via `diagnostic.notice`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagLog {
    pub level: DiagLevel,
    /// Source module / component (e.g. "agent_runtime", "grok_acp").
    pub source: String,
    pub message: String,
    /// Additional redacted context.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<Value>,
    /// Correlation ID if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl DiagLog {
    pub fn info(source: &str, message: impl Into<String>) -> Self {
        Self {
            level: DiagLevel::Info,
            source: source.into(),
            message: message.into(),
            context: None,
            correlation_id: None,
        }
    }

    pub fn warn(source: &str, message: impl Into<String>) -> Self {
        Self {
            level: DiagLevel::Warn,
            source: source.into(),
            message: message.into(),
            context: None,
            correlation_id: None,
        }
    }

    pub fn error(source: &str, message: impl Into<String>) -> Self {
        Self {
            level: DiagLevel::Error,
            source: source.into(),
            message: message.into(),
            context: None,
            correlation_id: None,
        }
    }

    pub fn with_context(mut self, ctx: Value) -> Self {
        self.context = Some(ctx);
        self
    }

    /// Emit to stderr in a structured single-line format.
    /// This is the ONLY logging path for the runtime module.
    pub fn emit(&self) {
        let json = serde_json::to_string(self).unwrap_or_else(|_| "{}".into());
        eprintln!("[runtime:diag] {}", json);
    }
}

// ---------------------------------------------------------------------------
// StderrBuffer — bounded ring buffer for child stderr
// ---------------------------------------------------------------------------

/// A bounded buffer that retains the last N lines of child-process stderr.
///
/// This prevents unbounded memory growth when a misbehaving agent
/// floods stderr.  The buffer is NOT fed into the protocol decoder —
/// it exists solely for diagnostic capture.
#[derive(Debug)]
pub struct StderrBuffer {
    lines: VecDeque<String>,
    max_lines: usize,
    total_seen: u64,
    truncated: bool,
}

impl StderrBuffer {
    pub fn new(max_lines: usize) -> Self {
        Self {
            lines: VecDeque::with_capacity(max_lines),
            max_lines,
            total_seen: 0,
            truncated: false,
        }
    }

    /// Push a line into the buffer.  When the buffer is full, the oldest
    /// line is evicted and `truncated` is set to `true`.
    pub fn push(&mut self, line: String) {
        self.total_seen += 1;
        if self.lines.len() >= self.max_lines {
            self.lines.pop_front();
            self.truncated = true;
        }
        // Redact before storing so the buffer is always safe to emit.
        let redacted = redact_line(&line);
        self.lines.push_back(redacted);
    }

    /// Returns all retained lines as a single newline-joined string.
    pub fn snapshot(&self) -> String {
        let mut out = String::new();
        if self.truncated {
            out.push_str(&format!(
                "[... {} earlier lines truncated ...]\n",
                self.total_seen.saturating_sub(self.lines.len() as u64)
            ));
        }
        for line in &self.lines {
            out.push_str(line);
            out.push('\n');
        }
        out
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn total_seen(&self) -> u64 {
        self.total_seen
    }

    pub fn was_truncated(&self) -> bool {
        self.truncated
    }
}

/// Redact sensitive patterns from a single stderr line.
/// This is a lighter-weight scan than full JSON redaction.
fn redact_line(line: &str) -> String {
    // Mask anything that looks like "key=value" where key is sensitive.
    let lower = line.to_lowercase();
    for pattern in SENSITIVE_KEY_PATTERNS {
        let needle = format!("{}=", pattern);
        if lower.contains(&needle) {
            // Replace the value after the = until whitespace or end.
            let idx = lower.find(&needle).unwrap();
            let start = idx + needle.len();
            let end = line[start..]
                .find(|c: char| c.is_whitespace())
                .map(|e| start + e)
                .unwrap_or(line.len());
            let mut result = line.to_string();
            result.replace_range(start..end, "<redacted>");
            return result;
        }
    }
    // Also redact Bearer tokens in stderr.
    if line.to_lowercase().contains("bearer ") {
        return line.to_lowercase().replace(
            |c: char| c.is_alphanumeric() || c == '-' || c == '_' || c == '.',
            "",
        );
    }
    line.to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn redact_replaces_sensitive_keys() {
        let v = json!({
            "token": "abc123",
            "api_key": "sk-xxxxx",
            "normal": "keep me",
            "nested": {
                "authorization": "Bearer xyz",
                "count": 42
            }
        });
        let r = redact(&v, 10);
        assert_eq!(r["token"], "<redacted>");
        assert_eq!(r["api_key"], "<redacted>");
        assert_eq!(r["normal"], "keep me");
        assert_eq!(r["nested"]["authorization"], "<redacted>");
        assert_eq!(r["nested"]["count"], 42);
    }

    #[test]
    fn redact_handles_arrays() {
        let v = json!([{"token": "a"}, {"safe": "b"}]);
        let r = redact(&v, 10);
        assert_eq!(r[0]["token"], "<redacted>");
        assert_eq!(r[1]["safe"], "b");
    }

    #[test]
    fn redact_depth_limit() {
        let v = json!({"a": {"b": {"c": {"d": "deep"}}}});
        let r = redact(&v, 2);
        // At depth 2, the value is truncated
        assert!(r["a"]["b"]["c"] == "<truncated:depth>");
    }

    #[test]
    fn looks_like_secret_detects_hex() {
        assert!(looks_like_secret(
            "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"
        ));
        assert!(looks_like_secret(
            "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"
        ));
        assert!(!looks_like_secret("short"));
        assert!(!looks_like_secret("a normal sentence with spaces"));
    }

    #[test]
    fn stderr_buffer_evicts_old_lines() {
        let mut buf = StderrBuffer::new(3);
        buf.push("line1".into());
        buf.push("line2".into());
        buf.push("line3".into());
        buf.push("line4".into());
        assert_eq!(buf.line_count(), 3);
        assert!(buf.was_truncated());
        assert_eq!(buf.total_seen(), 4);
        let snap = buf.snapshot();
        assert!(snap.contains("line4"));
        assert!(!snap.contains("line1"));
        assert!(snap.contains("truncated"));
    }

    #[test]
    fn stderr_buffer_redacts_sensitive_values() {
        let mut buf = StderrBuffer::new(10);
        buf.push("token=secret_abc123".into());
        buf.push("normal log line".into());
        let snap = buf.snapshot();
        assert!(snap.contains("<redacted>"));
        assert!(!snap.contains("secret_abc123"));
        assert!(snap.contains("normal log line"));
    }

    #[test]
    fn diag_log_serializes() {
        let log = DiagLog::warn("agent_runtime", "handshake slow")
            .with_context(json!({"elapsed_ms": 25000}));
        let json = serde_json::to_string(&log).unwrap();
        assert!(json.contains("\"level\":\"warn\""));
        assert!(json.contains("\"source\":\"agent_runtime\""));
        assert!(json.contains("handshake slow"));
        assert!(json.contains("elapsed_ms"));
    }

    #[test]
    fn redact_preserves_display_safe_message() {
        let v = json!({
            "message": "Connection refused",
            "error": "ECONNREFUSED",
            "token": "sk-abc123def456"
        });
        let r = redact(&v, 10);
        assert_eq!(r["message"], "Connection refused");
        assert_eq!(r["error"], "ECONNREFUSED");
        assert_eq!(r["token"], "<redacted>");
    }

    #[test]
    fn redact_handles_case_insensitive_keys() {
        let v = json!({
            "API_KEY": "secret",
            "AuthToken": "secret",
            "XAI_API_KEY": "secret"
        });
        let r = redact(&v, 10);
        assert_eq!(r["API_KEY"], "<redacted>");
        assert_eq!(r["AuthToken"], "<redacted>");
        assert_eq!(r["XAI_API_KEY"], "<redacted>");
    }
}
