//! Production ACP transport: spawns the real `grok` CLI process and
//! manages stdin/stdout/stderr pipes.
//!
//! # Security invariants
//! - The command and arguments are passed as a **vector**, never as a
//!   shell string.  No `sh -c` is used.
//! - The parent environment is cleared and rebuilt from `BASE_ENV_ALLOWLIST`.
//!   Only the selected model profile's `env_key` may be added dynamically.
//! - stderr is read on a separate task and stored in a bounded
//!   `StderrBuffer`; it never enters the protocol decoder.
//! - stdout is decoded by `FrameDecoder`; non-JSON content is a
//!   protocol error, never TUI text.

use async_trait::async_trait;
#[cfg(target_os = "windows")]
#[allow(unused_imports)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Mutex;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;

use super::codec::{AcpMessage, FrameDecoder};
use super::transport::{
    AcpTransport, LoginHandle, LoginMethod, LoginProcessState, ProcessExit, TransportError,
    TransportHandle,
};
use crate::bridge::types::SessionId;
use crate::modules::agent_runtime::config::{RuntimeConfig, WorkspaceContext};
use crate::modules::agent_runtime::diagnostics::{DiagLog, StderrBuffer};

/// Minimum environment required for Grok to start on supported desktop
/// platforms. Values are inherited at runtime but are never logged or sent to
/// the Renderer. API keys are intentionally absent from this static list.
const BASE_ENV_ALLOWLIST: &[&str] = &[
    // Executable lookup and Windows process startup.
    "PATH",
    "PATHEXT",
    "SYSTEMROOT",
    "WINDIR",
    "COMSPEC",
    // User-scoped Grok configuration and credential location.
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "HOME",
    "APPDATA",
    "LOCALAPPDATA",
    "PROGRAMDATA",
    "GROK_HOME",
    // Temporary files and locale.
    "TEMP",
    "TMP",
    "TMPDIR",
    "LANG",
    "LC_ALL",
    "TERM",
    // Explicit network and trust-store configuration.
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "CURL_CA_BUNDLE",
    "REQUESTS_CA_BUNDLE",
    // Non-secret Grok endpoint / enterprise OIDC configuration.
    "GROK_CLI_CHAT_PROXY_BASE_URL",
    "GROK_OIDC_ISSUER",
    "GROK_OIDC_CLIENT_ID",
];

/// The arguments passed to `grok` to start ACP stdio mode.
const GROK_AGENT_ARGS: &[&str] = &["--no-auto-update", "agent", "stdio"];

fn build_login_args(method: LoginMethod) -> [&'static str; 2] {
    match method {
        LoginMethod::Oauth => ["login", "--oauth"],
        LoginMethod::DeviceAuth => ["login", "--device-auth"],
    }
}

fn build_agent_args(model: Option<&str>) -> Result<Vec<&str>, TransportError> {
    let mut args = GROK_AGENT_ARGS[..2].to_vec();
    if let Some(model) = model {
        crate::modules::agent_runtime::config::validate_model_id(Some(model)).map_err(
            |message| TransportError::ProbeError {
                message: message.into(),
            },
        )?;
        args.extend(["--model", model]);
    }
    args.push(GROK_AGENT_ARGS[2]);
    Ok(args)
}

/// Production grok ACP adapter.
pub struct GrokAcpAdapter {
    config: RuntimeConfig,
    /// Interior-mutable cache of the resolved executable path.
    /// Set by `probe()`, read by `spawn()` and `resolved_path()`.
    resolved_path: Mutex<Option<PathBuf>>,
}

impl GrokAcpAdapter {
    pub fn new(config: RuntimeConfig) -> Self {
        Self {
            config,
            resolved_path: Mutex::new(None),
        }
    }

    /// Run `grok --version` and parse the output.
    async fn detect_version(&self, exe: &Path) -> Result<String, TransportError> {
        let output = tokio::process::Command::new(exe)
            .arg("--version")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_clear()
            .envs(filter_env(None))
            .output()
            .await
            .map_err(|e| TransportError::ProbeError {
                message: format!("failed to execute --version: {}", e),
            })?;

        if !output.status.success() {
            return Err(TransportError::ProbeError {
                message: format!("--version exited with status {}", output.status),
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_version(&stdout).ok_or_else(|| TransportError::ProbeError {
            message: format!("could not parse version from: {}", stdout.trim()),
        })
    }

    /// Search for the grok executable using the given config's path.
    fn find_executable_with_config(
        &self,
        config: &RuntimeConfig,
    ) -> Result<PathBuf, TransportError> {
        let mut searched = Vec::new();

        // 1. Explicit config path.
        if let Some(ref p) = config.executable_path {
            if p.is_file() {
                return Ok(p.clone());
            }
            searched.push(p.clone());
        }

        // 2. Default search paths.
        for p in crate::modules::agent_runtime::default_search_paths() {
            if p.file_name().is_some() && p.parent().is_none() {
                continue;
            }
            if p.is_file() {
                return Ok(p);
            }
            searched.push(p);
        }

        // 3. PATH lookup.
        let exe_name = if cfg!(target_os = "windows") {
            "grok.exe"
        } else {
            "grok"
        };
        if let Ok(path_env) = std::env::var("PATH") {
            for dir in path_env.split(if cfg!(target_os = "windows") {
                ';'
            } else {
                ':'
            }) {
                let candidate = PathBuf::from(dir).join(exe_name);
                if candidate.is_file() {
                    return Ok(candidate);
                }
                searched.push(candidate);
            }
        }

        Err(TransportError::NotFound { searched })
    }
}

#[async_trait]
impl AcpTransport for GrokAcpAdapter {
    async fn probe(&self, config: &RuntimeConfig) -> Result<(PathBuf, String), TransportError> {
        // Use the config passed to probe() if it has an explicit path,
        // otherwise fall back to the adapter's own config.
        let search_config = if config.executable_path.is_some() {
            config
        } else {
            &self.config
        };

        // Search for the executable.
        let candidate = if let Some(ref p) = search_config.executable_path {
            if p.is_file() {
                Ok(p.clone())
            } else {
                Err(TransportError::NotFound {
                    searched: vec![p.clone()],
                })
            }
        } else {
            // Use the adapter's find_executable which searches defaults + PATH.
            self.find_executable_with_config(search_config)
        }?;

        // Detect version.
        let version = self.detect_version(&candidate).await?;

        // Check version against the requirement.
        if !version_gte(&version, &search_config.min_version) {
            return Err(TransportError::VersionTooLow {
                found: version,
                required: search_config.min_version.clone(),
            });
        }

        // Cache the resolved path.
        *self.resolved_path.lock().unwrap() = Some(candidate.clone());

        Ok((candidate, version))
    }

    async fn spawn(
        &self,
        _session_id: SessionId,
        workspace: WorkspaceContext,
        config: &RuntimeConfig,
    ) -> Result<TransportHandle, TransportError> {
        let exe = self.resolved_path.lock().unwrap().clone().ok_or_else(|| {
            TransportError::ProbeError {
                message: "spawn() called before successful probe()".into(),
            }
        })?;

        // Build the command with argument vector — NO shell, NO string concat.
        let mut cmd = Command::new(&exe);
        cmd.args(build_agent_args(config.model.as_deref())?);

        // Set cwd — validated by the caller (workspace module).
        cmd.current_dir(&workspace.cwd);

        // Pipe all three stdio streams.
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd.kill_on_drop(true);

        // Rebuild a minimal environment after clearing the inherited parent.
        // A model API key crosses this boundary only when the currently
        // selected Grok profile explicitly names that exact `env_key`.
        cmd.env_clear();
        let configured_models = crate::modules::agent_runtime::configured_models();
        let selected_env_key = crate::modules::agent_runtime::config::selected_model_env_key(
            config.model.as_deref(),
            &configured_models,
        );
        for (k, v) in filter_env(selected_env_key) {
            cmd.env(k, v);
        }

        // On Windows, do NOT use a console window for the child.
        #[cfg(target_os = "windows")]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = cmd.spawn().map_err(|e| TransportError::SpawnFailed {
            message: format!("failed to spawn '{}': {}", exe.display(), e),
        })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| TransportError::SpawnFailed {
                message: "child stdin not captured".into(),
            })?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| TransportError::SpawnFailed {
                message: "child stdout not captured".into(),
            })?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| TransportError::SpawnFailed {
                message: "child stderr not captured".into(),
            })?;

        let (outbound_tx, outbound_rx) = mpsc::channel::<String>(64);
        let (inbound_tx, inbound_rx) = mpsc::channel::<AcpMessage>(256);

        // Spawn stdin writer task.
        spawn_stdin_writer(stdin, outbound_rx);

        // Spawn stdout reader + decoder task.
        spawn_stdout_reader(
            stdout,
            inbound_tx,
            self.config.max_frame_bytes,
            self.config.max_depth,
        );

        // Spawn stderr reader task with bounded buffer.
        let stderr_buf = StderrBuffer::new(self.config.max_stderr_lines as usize);
        let stderr_mutex = std::sync::Arc::new(tokio::sync::Mutex::new(stderr_buf));
        let stderr_clone = stderr_mutex.clone();
        spawn_stderr_reader(stderr, stderr_clone);

        // Spawn process monitor task.
        let process = spawn_process_monitor(child);

        Ok(TransportHandle {
            outbound: outbound_tx,
            inbound: inbound_rx,
            process,
            stderr: stderr_mutex,
        })
    }

    async fn start_login(
        &self,
        method: LoginMethod,
        timeout_secs: u64,
    ) -> Result<LoginHandle, TransportError> {
        let exe = self.resolved_path.lock().unwrap().clone().ok_or_else(|| {
            TransportError::ProbeError {
                message: "login called before successful executable probe".into(),
            }
        })?;
        let mut command = Command::new(&exe);
        command.args(build_login_args(method));
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .env_clear()
            .envs(filter_env(None));

        #[cfg(target_os = "windows")]
        {
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = command
            .spawn()
            .map_err(|error| TransportError::SpawnFailed {
                message: format!("failed to start Grok login process: {}", error),
            })?;
        let (state_tx, state_rx) = watch::channel(LoginProcessState::Running);
        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        tauri::async_runtime::spawn(async move {
            let terminal = tokio::select! {
                status = child.wait() => match status {
                    Ok(status) if status.success() => LoginProcessState::Succeeded,
                    Ok(status) => LoginProcessState::Failed { exit_code: status.code() },
                    Err(_) => LoginProcessState::Failed { exit_code: None },
                },
                _ = &mut cancel_rx => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    LoginProcessState::Cancelled
                },
                _ = tokio::time::sleep(std::time::Duration::from_secs(timeout_secs.max(1))) => {
                    let _ = child.kill().await;
                    let _ = child.wait().await;
                    LoginProcessState::TimedOut
                },
            };
            let _ = state_tx.send(terminal);
        });

        Ok(LoginHandle {
            state: state_rx,
            cancel: std::sync::Mutex::new(Some(cancel_tx)),
        })
    }

    fn resolved_path(&self) -> Option<PathBuf> {
        self.resolved_path.lock().unwrap().clone()
    }
}

// ---------------------------------------------------------------------------
// Task spawners
// ---------------------------------------------------------------------------

/// Read JSON-RPC strings from the outbound channel and write them to
/// the child's stdin, newline-terminated.
fn spawn_stdin_writer(mut stdin: ChildStdin, mut rx: mpsc::Receiver<String>) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(line) = rx.recv().await {
            let line = format!("{}\n", line);
            if stdin.write_all(line.as_bytes()).await.is_err() {
                break;
            }
            let _ = stdin.flush().await;
        }
        // Close stdin to signal the child that no more input is coming.
        let _ = stdin.shutdown().await;
    })
}

/// Read raw bytes from the child's stdout, decode JSON-RPC frames, and
/// send parsed messages to the inbound channel.
fn spawn_stdout_reader(
    stdout: ChildStdout,
    tx: mpsc::Sender<AcpMessage>,
    max_frame_bytes: u64,
    max_depth: u32,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        let mut decoder = FrameDecoder::new(max_frame_bytes, max_depth);
        let mut buf = [0u8; 8192];

        loop {
            match reader.read(&mut buf).await {
                Ok(0) => break, // EOF
                Ok(n) => {
                    let results = decoder.feed(&buf[..n]);
                    for result in results {
                        match result {
                            Ok(msg) => {
                                if tx.send(msg).await.is_err() {
                                    return; // channel closed
                                }
                            }
                            Err(e) => {
                                DiagLog::warn(
                                    "grok_acp:codec",
                                    format!("frame decode error: {}", e),
                                )
                                .emit();
                                // The decoder resynchronises at the next newline;
                                // we continue reading.
                            }
                        }
                    }
                }
                Err(e) => {
                    DiagLog::error("grok_acp:stdout", format!("read error: {}", e)).emit();
                    break;
                }
            }
        }
    })
}

/// Read stderr line by line, redact, and store in the bounded buffer.
fn spawn_stderr_reader(
    stderr: ChildStderr,
    buffer: std::sync::Arc<tokio::sync::Mutex<StderrBuffer>>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break, // EOF
                Ok(_) => {
                    let trimmed = line.trim_end_matches(['\n', '\r']).to_string();
                    buffer.lock().await.push(trimmed);
                }
                Err(e) => {
                    DiagLog::error("grok_acp:stderr", format!("read error: {}", e)).emit();
                    break;
                }
            }
        }
    })
}

/// Monitor the child process for exit and return the exit info.
fn spawn_process_monitor(mut child: Child) -> JoinHandle<ProcessExit> {
    tokio::spawn(async move {
        // Wait for the process to exit.
        let status = match child.wait().await {
            Ok(s) => s,
            Err(e) => {
                DiagLog::error("grok_acp:process", format!("wait error: {}", e)).emit();
                return ProcessExit {
                    code: None,
                    signal: None,
                    reason: "unknown".into(),
                };
            }
        };

        let code = status.code();
        let signal = signal_name(&status);
        let reason = if code == Some(0) {
            "clean".into()
        } else if signal.is_some() {
            "killed".into()
        } else if code.is_some() {
            "crash".into()
        } else {
            "unknown".into()
        };

        ProcessExit {
            code,
            signal,
            reason,
        }
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Select child environment variables from a supplied parent snapshot.
/// Keeping this pure makes the security boundary directly testable without
/// mutating the test runner's global process environment.
fn select_child_env_from(
    parent: impl IntoIterator<Item = (String, String)>,
    selected_model_env_key: Option<&str>,
) -> Vec<(String, String)> {
    let selected_model_env_key = selected_model_env_key.filter(|key| valid_env_key(key));
    parent
        .into_iter()
        .filter(|(key, _)| {
            BASE_ENV_ALLOWLIST
                .iter()
                .any(|allowed| env_key_eq(key, allowed))
                || selected_model_env_key.is_some_and(|selected| env_key_eq(key, selected))
        })
        .collect()
}

fn filter_env(selected_model_env_key: Option<&str>) -> Vec<(String, String)> {
    select_child_env_from(std::env::vars(), selected_model_env_key)
}

fn valid_env_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 128
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn env_key_eq(left: &str, right: &str) -> bool {
    if cfg!(target_os = "windows") {
        left.eq_ignore_ascii_case(right)
    } else {
        left == right
    }
}

/// Parse a version string like "grok 0.2.118" or "0.2.118" into "0.2.118".
fn parse_version(s: &str) -> Option<String> {
    // Find the first sequence of digits and dots.
    let trimmed = s.trim();
    for token in trimmed.split_whitespace() {
        if token
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
        {
            return Some(token.trim_end_matches(',').to_string());
        }
    }
    None
}

/// Compare two semantic version strings: returns true if `a >= b`.
fn version_gte(a: &str, b: &str) -> bool {
    let pa: Vec<u64> = a.split('.').filter_map(|s| s.parse().ok()).collect();
    let pb: Vec<u64> = b.split('.').filter_map(|s| s.parse().ok()).collect();
    for i in 0..pa.len().max(pb.len()) {
        let va = pa.get(i).copied().unwrap_or(0);
        let vb = pb.get(i).copied().unwrap_or(0);
        if va > vb {
            return true;
        }
        if va < vb {
            return false;
        }
    }
    true
}

/// Extract a human-readable signal name from a process ExitStatus.
fn signal_name(status: &std::process::ExitStatus) -> Option<String> {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status.signal().map(|s| format!("signal {}", s))
    }
    #[cfg(not(unix))]
    {
        let _ = status;
        None
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_extracts_numeric_token() {
        assert_eq!(parse_version("grok 0.2.118"), Some("0.2.118".into()));
        assert_eq!(parse_version("0.2.118"), Some("0.2.118".into()));
        assert_eq!(
            parse_version("grok version 1.0.0, build abc"),
            Some("1.0.0".into())
        );
        assert_eq!(parse_version("no version here"), None);
    }

    #[test]
    fn version_gte_basic() {
        assert!(version_gte("0.2.118", "0.2.118"));
        assert!(version_gte("0.3.0", "0.2.118"));
        assert!(version_gte("1.0.0", "0.2.118"));
        assert!(!version_gte("0.1.0", "0.2.118"));
        assert!(!version_gte("0.2.117", "0.2.118"));
    }

    #[test]
    fn version_gte_handles_short_versions() {
        assert!(version_gte("1", "0.2.118"));
        assert!(!version_gte("0", "0.2.118"));
        // 0.2 -> [0, 2] vs [0, 2, 118] -> compares 0==0, 2==2, then 0 < 118 -> false
        assert!(!version_gte("0.2", "0.2.118"));
    }

    fn parent_env_fixture() -> Vec<(String, String)> {
        [
            ("PATH", "C:\\Windows\\System32"),
            ("SYSTEMROOT", "C:\\Windows"),
            ("USERPROFILE", "C:\\Users\\测试 User"),
            ("HTTPS_PROXY", "http://proxy.invalid"),
            ("SSL_CERT_FILE", "C:\\证书\\root.pem"),
            ("XAI_API_KEY", "xai-secret"),
            ("OPENAI_API_KEY", "openai-secret"),
            ("AWS_SECRET_ACCESS_KEY", "aws-secret"),
            ("UNRELATED_SETTING", "must-not-cross-boundary"),
        ]
        .into_iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
    }

    #[test]
    fn child_environment_is_a_minimal_allowlist() {
        let filtered = select_child_env_from(parent_env_fixture(), None);
        let keys: Vec<&str> = filtered.iter().map(|(key, _)| key.as_str()).collect();

        assert!(keys.contains(&"PATH"));
        assert!(keys.contains(&"SYSTEMROOT"));
        assert!(keys.contains(&"USERPROFILE"));
        assert!(keys.contains(&"HTTPS_PROXY"));
        assert!(keys.contains(&"SSL_CERT_FILE"));
        assert!(!keys.contains(&"XAI_API_KEY"));
        assert!(!keys.contains(&"OPENAI_API_KEY"));
        assert!(!keys.contains(&"AWS_SECRET_ACCESS_KEY"));
        assert!(!keys.contains(&"UNRELATED_SETTING"));
    }

    #[test]
    fn only_the_selected_model_env_key_is_added() {
        let filtered = select_child_env_from(parent_env_fixture(), Some("OPENAI_API_KEY"));
        let keys: Vec<&str> = filtered.iter().map(|(key, _)| key.as_str()).collect();

        assert!(keys.contains(&"OPENAI_API_KEY"));
        assert!(!keys.contains(&"XAI_API_KEY"));
        assert!(!keys.contains(&"AWS_SECRET_ACCESS_KEY"));
    }

    #[test]
    fn grok_agent_args_no_shell() {
        // The args must be a vector, not a shell string.
        assert_eq!(GROK_AGENT_ARGS, &["--no-auto-update", "agent", "stdio"]);
        // Ensure no shell metacharacters.
        for arg in GROK_AGENT_ARGS {
            assert!(!arg.contains(['|', ';', '&']));
        }
    }

    #[test]
    fn grok_login_args_are_fixed_and_never_use_a_shell_string() {
        assert_eq!(build_login_args(LoginMethod::Oauth), ["login", "--oauth"]);
        assert_eq!(
            build_login_args(LoginMethod::DeviceAuth),
            ["login", "--device-auth"]
        );
    }

    #[test]
    fn explicit_model_is_passed_as_a_separate_agent_argument() {
        let args = build_agent_args(Some("deepseek-v4-pro")).expect("valid model id");

        assert_eq!(
            args,
            vec![
                "--no-auto-update",
                "agent",
                "--model",
                "deepseek-v4-pro",
                "stdio",
            ]
        );
    }

    #[test]
    fn option_like_model_id_is_rejected_before_process_spawn() {
        let error = build_agent_args(Some("--always-approve"))
            .expect_err("an option-like model id must fail closed");

        assert!(matches!(error, TransportError::ProbeError { .. }));
        assert!(!error.to_string().contains("--always-approve"));
    }
}
