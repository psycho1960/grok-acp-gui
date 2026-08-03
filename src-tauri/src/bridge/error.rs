use serde::{Deserialize, Serialize};

/// Canonical error type crossing the DesktopBridge seam.
///
/// Every error visible to the Renderer is represented as an `AppError`.
/// The bridge never panics on malformed input — it returns an `AppError`
/// with a `BRIDGE_*` code.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    /// Machine-readable error code (e.g. `BRIDGE_UNSUPPORTED_COMMAND`).
    pub code: String,
    /// Human-readable summary. Must be safe to show in the UI.
    pub message: String,
    /// Optional recovery action hint for the user.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// Whether retrying the same operation might succeed.
    pub retryable: bool,
    /// When true the `message` has already been scrubbed of paths,
    /// tokens, and internal identifiers.
    pub details_redacted: bool,
    /// Opaque identifier for correlating logs and support requests.
    pub correlation_id: String,
}

impl AppError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            action: None,
            retryable: false,
            details_redacted: true,
            correlation_id: new_correlation_id(),
        }
    }

    pub fn with_action(mut self, action: impl Into<String>) -> Self {
        self.action = Some(action.into());
        self
    }

    pub fn retryable(mut self) -> Self {
        self.retryable = true;
        self
    }
}

fn new_correlation_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros();
    format!("{:016x}", ts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_serializes_camel_case() {
        let err = AppError::new("BRIDGE_TEST", "something went wrong")
            .with_action("restart the app")
            .retryable();
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"code\""));
        assert!(json.contains("\"message\""));
        assert!(json.contains("\"action\""));
        assert!(json.contains("\"retryable\""));
        assert!(json.contains("\"detailsRedacted\""));
        assert!(json.contains("\"correlationId\""));
    }

    #[test]
    fn error_omits_action_when_none() {
        let err = AppError::new("BRIDGE_TEST", "oops");
        let json = serde_json::to_string(&err).unwrap();
        assert!(!json.contains("\"action\""));
    }
}
