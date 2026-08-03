//! Runtime configuration and probe result types.
//!
//! `RuntimeConfig` is loaded from the persistence layer (settings table)
//! by the bridge and passed into `AgentRuntime::probe` and `start`.
//! The runtime module never reads the database directly — it receives
//! a fully-populated config from the caller.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Configuration for the Grok ACP runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeConfig {
    /// Explicit path to the grok executable. When `None`, the adapter
    /// searches default locations and PATH.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable_path: Option<PathBuf>,
    /// Minimum required grok version (e.g. "0.2.118").
    pub min_version: String,
    /// Handshake timeout in seconds.
    pub handshake_timeout_secs: u64,
    /// Idle timeout in seconds (0 = never auto-close).
    pub idle_timeout_secs: u64,
    /// Maximum frame size in bytes for JSON-RPC messages (default 4 MiB).
    pub max_frame_bytes: u64,
    /// Maximum JSON nesting depth.
    pub max_depth: u32,
    /// Maximum stderr lines to retain for diagnostics.
    pub max_stderr_lines: u32,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            executable_path: None,
            min_version: "0.2.118".into(),
            handshake_timeout_secs: 30,
            idle_timeout_secs: 300, // 5 minutes (per technical design §7.6)
            max_frame_bytes: 4 * 1024 * 1024, // 4 MiB
            max_depth: 64,
            max_stderr_lines: 200,
        }
    }
}

/// Result of probing the Grok CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeProbeResult {
    /// `true` when grok is installed, meets the minimum version, and
    /// (when checkable) is authenticated.
    pub available: bool,
    /// Absolute path to the discovered executable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable_path: Option<PathBuf>,
    /// Parsed version string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Whether the minimum version requirement is met.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_ok: Option<bool>,
    /// Whether the user appears to be authenticated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authenticated: Option<bool>,
    /// Machine-readable status: "ready", "not_found", "version_too_low",
    /// "not_authenticated", "probe_error".
    pub status: String,
    /// Display-safe message explaining the status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Suggested recovery action for the user.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}

impl RuntimeProbeResult {
    pub fn ready(path: PathBuf, version: String, authenticated: bool) -> Self {
        Self {
            available: true,
            executable_path: Some(path),
            version: Some(version),
            version_ok: Some(true),
            authenticated: Some(authenticated),
            status: "ready".into(),
            message: None,
            action: None,
        }
    }

    pub fn not_found() -> Self {
        Self {
            available: false,
            executable_path: None,
            version: None,
            version_ok: None,
            authenticated: None,
            status: "not_found".into(),
            message: Some("Grok CLI was not found.".into()),
            action: Some(
                "Install Grok Build from the official source and ensure it is on your PATH.".into(),
            ),
        }
    }

    pub fn version_too_low(found: String, required: &str) -> Self {
        Self {
            available: false,
            executable_path: None,
            version: Some(found.clone()),
            version_ok: Some(false),
            authenticated: None,
            status: "version_too_low".into(),
            message: Some(format!(
                "Grok version {} is older than the required {}.",
                found, required
            )),
            action: Some("Update Grok Build to the latest version.".into()),
        }
    }

    pub fn not_authenticated() -> Self {
        Self {
            available: false,
            executable_path: None,
            version: None,
            version_ok: None,
            authenticated: Some(false),
            status: "not_authenticated".into(),
            message: Some("You are not logged in to Grok.".into()),
            action: Some("Run 'grok login' to authenticate.".into()),
        }
    }

    pub fn probe_error(msg: impl Into<String>) -> Self {
        Self {
            available: false,
            executable_path: None,
            version: None,
            version_ok: None,
            authenticated: None,
            status: "probe_error".into(),
            message: Some(msg.into()),
            action: Some("Check the Grok installation and try again.".into()),
        }
    }
}

/// Workspace context for a session start.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceContext {
    /// Working directory for the grok process.
    pub cwd: PathBuf,
}

/// A handle to a running session, returned by `AgentRuntime::start`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeHandle {
    pub session_id: crate::bridge::types::SessionId,
    /// The resolved executable path used for this session.
    pub executable_path: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_safe_limits() {
        let c = RuntimeConfig::default();
        assert_eq!(c.max_frame_bytes, 4 * 1024 * 1024);
        assert!(c.max_depth >= 32);
        assert!(c.max_stderr_lines >= 50);
        assert!(c.handshake_timeout_secs >= 5);
    }

    #[test]
    fn probe_ready() {
        let r = RuntimeProbeResult::ready(PathBuf::from("/usr/bin/grok"), "0.2.118".into(), true);
        assert!(r.available);
        assert_eq!(r.status, "ready");
    }

    #[test]
    fn probe_not_found_has_action() {
        let r = RuntimeProbeResult::not_found();
        assert!(!r.available);
        assert_eq!(r.status, "not_found");
        assert!(r.action.is_some());
    }

    #[test]
    fn probe_version_too_low() {
        let r = RuntimeProbeResult::version_too_low("0.1.0".into(), "0.2.118");
        assert!(!r.available);
        assert_eq!(r.version.unwrap(), "0.1.0");
        assert!(!r.version_ok.unwrap());
    }
}
