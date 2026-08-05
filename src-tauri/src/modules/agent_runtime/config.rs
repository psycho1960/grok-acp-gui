//! Runtime configuration and probe result types.
//!
//! `RuntimeConfig` is loaded from the persistence layer (settings table)
//! by the bridge and passed into `AgentRuntime::probe` and `start`.
//! The runtime module never reads the database directly — it receives
//! a fully-populated config from the caller.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const MAX_GROK_CONFIG_BYTES: u64 = 1_048_576;

/// A named model profile from Grok Build's local configuration.
///
/// `id` is the `[model.<id>]` profile name accepted by `grok --model`.
/// `name` is display-only and may include the profile's underlying model name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfiguredModel {
    pub id: String,
    pub name: String,
    pub reasoning_effort: Option<String>,
}

/// Read model profiles from Grok Build's local `config.toml`.
///
/// This deliberately reads only `[model.*]` section headers and their
/// `model = "..."` / `reasoning_effort = "..."` values. It never reads auth
/// files, emits config contents, or invokes the Grok TUI. An unreadable,
/// oversized, or malformed config simply yields no choices so the runtime
/// default remains available.
pub fn configured_models() -> Vec<ConfiguredModel> {
    let Some(config_path) = grok_config_path() else {
        return vec![];
    };
    configured_models_from_path(&config_path)
}

fn grok_config_path() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .map(|home| home.join(".grok").join("config.toml"))
}

fn configured_models_from_path(path: &std::path::Path) -> Vec<ConfiguredModel> {
    let Ok(metadata) = std::fs::metadata(path) else {
        return vec![];
    };
    if metadata.len() > MAX_GROK_CONFIG_BYTES {
        return vec![];
    }
    let Ok(contents) = std::fs::read_to_string(path) else {
        return vec![];
    };
    parse_configured_models(&contents)
}

fn parse_configured_models(contents: &str) -> Vec<ConfiguredModel> {
    let mut profiles = Vec::new();
    let mut current_profile: Option<String> = None;
    let mut current_model: Option<String> = None;
    let mut current_reasoning_effort: Option<String> = None;

    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            push_configured_model(
                &mut profiles,
                current_profile.take(),
                current_model.take(),
                current_reasoning_effort.take(),
            );
            current_profile = line
                .strip_prefix("[model.")
                .and_then(|section| section.strip_suffix(']'))
                .and_then(parse_toml_key);
            continue;
        }

        if current_profile.is_none() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "model" => current_model = parse_toml_string(value.trim()),
            "reasoning_effort" => current_reasoning_effort = parse_reasoning_effort(value.trim()),
            _ => {}
        }
    }

    push_configured_model(
        &mut profiles,
        current_profile,
        current_model,
        current_reasoning_effort,
    );
    profiles
}

fn push_configured_model(
    profiles: &mut Vec<ConfiguredModel>,
    profile: Option<String>,
    model_name: Option<String>,
    reasoning_effort: Option<String>,
) {
    let (Some(profile), Some(model_name)) = (profile, model_name) else {
        return;
    };
    if profile.is_empty() || profiles.iter().any(|item| item.id == profile) {
        return;
    }
    let name = if model_name == profile {
        profile.clone()
    } else {
        format!("{} ({})", profile, model_name)
    };
    profiles.push(ConfiguredModel {
        id: profile,
        name,
        reasoning_effort,
    });
}

fn parse_reasoning_effort(value: &str) -> Option<String> {
    let effort = parse_toml_string(value)?;
    matches!(effort.as_str(), "low" | "medium" | "high" | "max").then_some(effort)
}

fn parse_toml_key(value: &str) -> Option<String> {
    let value = value.trim();
    if value.starts_with('"') {
        parse_toml_string(value)
    } else if value.is_empty() || value.chars().any(char::is_whitespace) {
        None
    } else {
        Some(value.to_string())
    }
}

fn parse_toml_string(value: &str) -> Option<String> {
    let quoted = value.strip_prefix('"')?;
    let end = quoted.find('"')?;
    Some(quoted[..end].to_string())
}

/// Configuration for the Grok ACP runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeConfig {
    /// Explicit path to the grok executable. When `None`, the adapter
    /// searches default locations and PATH.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable_path: Option<PathBuf>,
    /// Optional Grok Build model/profile ID passed to `grok agent stdio`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
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
            model: None,
            min_version: "0.2.118".into(),
            handshake_timeout_secs: 30,
            idle_timeout_secs: 300, // 5 minutes (per technical design §7.6)
            max_frame_bytes: 4 * 1024 * 1024, // 4 MiB
            max_depth: 64,
            max_stderr_lines: 200,
        }
    }
}

/// Validate an opaque Grok Build model/profile ID before it reaches argv.
pub(crate) fn validate_model_id(model: Option<&str>) -> Result<(), &'static str> {
    let Some(model) = model else {
        return Ok(());
    };
    let mut chars = model.chars();
    let valid = model.len() <= 128
        && chars.next().is_some_and(|ch| ch.is_ascii_alphanumeric())
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/' | ':'));
    if valid {
        Ok(())
    } else {
        Err("invalid model identifier")
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

    #[test]
    fn parses_only_named_model_profiles() {
        let config = r#"
            [models]
            [model."grok-4.5"]
            model = "grok-4.5"
            reasoning_effort = "high"
            [model.deepseek]
            model = "deepseek-v4-pro"
            reasoning_effort = "max"
            [other]
            model = "must-not-appear"
        "#;

        assert_eq!(
            parse_configured_models(config),
            vec![
                ConfiguredModel {
                    id: "grok-4.5".into(),
                    name: "grok-4.5".into(),
                    reasoning_effort: Some("high".into()),
                },
                ConfiguredModel {
                    id: "deepseek".into(),
                    name: "deepseek (deepseek-v4-pro)".into(),
                    reasoning_effort: Some("max".into()),
                },
            ],
        );
    }
}
