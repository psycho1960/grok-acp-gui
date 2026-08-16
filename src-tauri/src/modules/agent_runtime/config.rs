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
    pub reasoning_efforts: Vec<String>,
    /// Name of the environment variable that holds this profile's API key
    /// (`env_key` in the profile). The value is never read or stored — only
    /// the variable NAME is captured so callers can check presence.
    pub env_key: Option<String>,
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

/// Check that a model profile's `env_key` variable is present in the process
/// environment. Returns a display-safe message when the variable is missing.
///
/// Profiles without an `env_key` (native models, inline-`api_key` profiles)
/// and unknown profile ids pass the check — grok resolves those itself.
/// Only the variable NAME is mentioned, never any value.
pub(crate) fn missing_model_env_key(
    model: &str,
    profiles: &[ConfiguredModel],
    env_present: impl Fn(&str) -> bool,
) -> Option<String> {
    let profile = profiles.iter().find(|profile| profile.id == model)?;
    let env_key = profile.env_key.as_deref()?;
    if env_present(env_key) {
        None
    } else {
        Some(format!(
            "模型 '{}' 需要环境变量 '{}'，但当前应用进程未检测到该变量。请设置用户级环境变量，完全退出并重启应用。",
            model, env_key
        ))
    }
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
    let mut current_reasoning_efforts = Vec::new();
    let mut nested_reasoning_effort: Option<String> = None;
    let mut in_reasoning_effort = false;
    let mut current_env_key: Option<String> = None;

    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            if let Some(effort) = nested_reasoning_effort.take() {
                if !current_reasoning_efforts.contains(&effort) {
                    current_reasoning_efforts.push(effort);
                }
            }

            let nested_profile = line
                .strip_prefix("[[model.")
                .and_then(|section| section.strip_suffix(".reasoning_efforts]]"))
                .and_then(parse_toml_key);
            if nested_profile.is_some() && nested_profile.as_deref() == current_profile.as_deref() {
                in_reasoning_effort = true;
                continue;
            }

            push_configured_model(
                &mut profiles,
                current_profile.take(),
                current_model.take(),
                current_reasoning_effort.take(),
                std::mem::take(&mut current_reasoning_efforts),
                current_env_key.take(),
            );
            current_profile = line
                .strip_prefix("[model.")
                .and_then(|section| section.strip_suffix(']'))
                .and_then(parse_toml_key);
            in_reasoning_effort = false;
            continue;
        }

        if current_profile.is_none() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if in_reasoning_effort {
            if key.trim() == "value" {
                nested_reasoning_effort = parse_reasoning_effort(value.trim());
            }
            continue;
        }
        match key.trim() {
            "model" => current_model = parse_toml_string(value.trim()),
            "reasoning_effort" => current_reasoning_effort = parse_reasoning_effort(value.trim()),
            // Capture only the variable NAME. The API key value itself is
            // deliberately never read into this process.
            "env_key" => current_env_key = parse_toml_string(value.trim()),
            _ => {}
        }
    }

    if let Some(effort) = nested_reasoning_effort {
        if !current_reasoning_efforts.contains(&effort) {
            current_reasoning_efforts.push(effort);
        }
    }
    push_configured_model(
        &mut profiles,
        current_profile,
        current_model,
        current_reasoning_effort,
        current_reasoning_efforts,
        current_env_key,
    );
    profiles
}

fn push_configured_model(
    profiles: &mut Vec<ConfiguredModel>,
    profile: Option<String>,
    model_name: Option<String>,
    reasoning_effort: Option<String>,
    reasoning_efforts: Vec<String>,
    env_key: Option<String>,
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
        reasoning_efforts,
        env_key,
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
    /// Interactive login timeout in seconds. The official browser/device
    /// flow allows up to five minutes by default.
    pub login_timeout_secs: u64,
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
            login_timeout_secs: 300,
            idle_timeout_secs: 300, // 5 minutes (per technical design §7.6)
            max_frame_bytes: 4 * 1024 * 1024, // 4 MiB
            max_depth: 64,
            max_stderr_lines: 200,
        }
    }
}

pub(crate) fn selected_model_env_key<'a>(
    model: Option<&str>,
    profiles: &'a [ConfiguredModel],
) -> Option<&'a str> {
    let model = model?;
    profiles
        .iter()
        .find(|profile| profile.id == model)
        .and_then(|profile| profile.env_key.as_deref())
}

/// Renderer-safe state of the separate official Grok login process.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeLoginResult {
    /// idle/running/succeeded/cancelled/timed_out/failed
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Safe, actionable text. Login stdout/stderr never crosses this DTO.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub retryable: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeLoginMethod {
    Oauth,
    DeviceAuth,
}

impl RuntimeLoginResult {
    pub fn idle() -> Self {
        Self {
            status: "idle".into(),
            exit_code: None,
            message: None,
            retryable: true,
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
    /// `true` when grok is installed and meets the minimum version.
    /// Authentication is verified separately through ACP.
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
                    reasoning_efforts: vec![],
                    env_key: None,
                },
                ConfiguredModel {
                    id: "deepseek".into(),
                    name: "deepseek (deepseek-v4-pro)".into(),
                    reasoning_effort: Some("max".into()),
                    reasoning_efforts: vec![],
                    env_key: None,
                },
            ],
        );
    }

    #[test]
    fn parses_reasoning_efforts_declared_by_each_model_profile() {
        let config = r#"
            [model.fast]
            model = "fast"
            reasoning_effort = "high"
            [[model.fast.reasoning_efforts]]
            id = "high"
            value = "high"
            default = true
            [[model.fast.reasoning_efforts]]
            id = "max"
            value = "max"
            default = false

            [model."visual-model"]
            model = "visual-model"
            reasoning_effort = "medium"
            [[model."visual-model".reasoning_efforts]]
            value = "medium"
            [[model."visual-model".reasoning_efforts]]
            value = "high"
        "#;

        let models = parse_configured_models(config);
        assert_eq!(models[0].reasoning_efforts, vec!["high", "max"]);
        assert_eq!(models[1].reasoning_efforts, vec!["medium", "high"]);
    }

    #[test]
    fn parses_env_key_name_but_never_the_value() {
        let config = r#"
            [model.opencode]
            model = "opencode-deepseek-v4-flash"
            env_key = "OPENCODE_API_KEY1"
            api_key = "synthetic-secret-never-captured"
            [model.minimax]
            model = "MiniMax-M3"
            api_key = "synthetic-inline-secret"
        "#;

        let models = parse_configured_models(config);
        assert_eq!(
            models,
            vec![
                ConfiguredModel {
                    id: "opencode".into(),
                    name: "opencode (opencode-deepseek-v4-flash)".into(),
                    reasoning_effort: None,
                    reasoning_efforts: vec![],
                    env_key: Some("OPENCODE_API_KEY1".into()),
                },
                ConfiguredModel {
                    id: "minimax".into(),
                    name: "minimax (MiniMax-M3)".into(),
                    reasoning_effort: None,
                    reasoning_efforts: vec![],
                    // Inline api_key profiles carry no env_key: grok reads the
                    // key from config.toml itself, and we never capture values.
                    env_key: None,
                },
            ],
        );
    }

    #[test]
    fn missing_env_key_reports_only_the_variable_name() {
        let profiles = vec![ConfiguredModel {
            id: "opencode".into(),
            name: "opencode-deepseek-v4-flash".into(),
            reasoning_effort: None,
            reasoning_efforts: vec![],
            env_key: Some("OPENCODE_API_KEY1".into()),
        }];

        let missing = missing_model_env_key("opencode", &profiles, |_| false).expect("missing");
        assert!(missing.contains("OPENCODE_API_KEY1"), "{missing}");
        assert!(
            !missing.contains("sk-"),
            "message must not contain any key value"
        );
        assert!(missing.contains("重启应用"), "{missing}");
    }

    #[test]
    fn env_key_present_passes_the_check() {
        let profiles = vec![ConfiguredModel {
            id: "opencode".into(),
            name: "opencode-deepseek-v4-flash".into(),
            reasoning_effort: None,
            reasoning_efforts: vec![],
            env_key: Some("OPENCODE_API_KEY1".into()),
        }];

        assert_eq!(missing_model_env_key("opencode", &profiles, |_| true), None);
    }

    #[test]
    fn profiles_without_env_key_or_unknown_models_pass_the_check() {
        let profiles = vec![ConfiguredModel {
            id: "native".into(),
            name: "grok-4.5".into(),
            reasoning_effort: None,
            reasoning_efforts: vec![],
            env_key: None,
        }];

        assert_eq!(missing_model_env_key("native", &profiles, |_| false), None);
        assert_eq!(
            missing_model_env_key("unknown-model", &profiles, |_| false),
            None,
            "unknown profiles must pass: grok resolves them itself"
        );
    }
}
