//! Production ACP transport: spawns the real `grok` CLI process and
//! manages stdin/stdout/stderr pipes.
//!
//! # Security invariants
//! - The command and arguments are passed as a **vector**, never as a
//!   shell string.  No `sh -c` is used.
//! - Known-sensitive environment variables are blocked via `ENV_BLOCKLIST`;
//!   all other parent environment is inherited (blocklist, not allowlist).
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
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::codec::{AcpMessage, FrameDecoder};
use super::transport::{AcpTransport, ProcessExit, TransportError, TransportHandle};
use crate::bridge::types::SessionId;
use crate::modules::agent_runtime::config::{RuntimeConfig, WorkspaceContext};
use crate::modules::agent_runtime::diagnostics::{DiagLog, StderrBuffer};

/// Environment variables that are explicitly BLOCKED from the child process.
/// All other parent environment variables are inherited.
///
/// Design note: a blocklist (denylist) is safer than an allowlist because
/// we cannot predict every variable a modern CLI (like Grok) needs for
/// network, crypto, locale, and platform-specific initialization.
/// Blocking *known-sensitive* keys strikes the right balance between
/// security and reliability.
const ENV_BLOCKLIST: &[&str] = &[
    // Secrets and credentials
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "AZURE_CLIENT_SECRET",
    "DOCKER_PASSWORD",
    "GITHUB_TOKEN",
    "NPM_TOKEN",
    // XAI_API_KEY is intentionally blocked — the adapter adds it
    // explicitly only when present in the parent environment.
    "XAI_API_KEY",
    // Other sensitive vars
    "ACTIONS_RUNTIME_TOKEN",
    "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
];

/// The arguments passed to `grok` to start ACP stdio mode.
const GROK_AGENT_ARGS: &[&str] = &["--no-auto-update", "agent", "stdio"];

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
            .envs(filter_env())
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

        // Apply env blocklist — pass all parent env EXCEPT known-sensitive keys.
        for (k, v) in filter_env() {
            cmd.env(k, v);
        }
        // Explicitly pass XAI_API_KEY if the parent has it (runtime-only, never logged).
        if let Ok(key) = std::env::var("XAI_API_KEY") {
            cmd.env("XAI_API_KEY", key);
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

/// Filter the parent environment: inherit everything EXCEPT blocklisted variables.
/// Returns the filtered set of (key, value) pairs safe for the child process.
fn filter_env() -> Vec<(String, String)> {
    let mut result = Vec::new();
    for (key, val) in std::env::vars() {
        if !ENV_BLOCKLIST.contains(&key.as_str()) {
            result.push((key, val));
        }
    }
    result
}

// Keep old function name as alias for backward compat in tests
#[allow(dead_code)]
fn filter_env_allowlist() -> Vec<(String, String)> {
    let allowlist = &[
        "PATH",
        "USERPROFILE",
        "APPDATA",
        "LOCALAPPDATA",
        "HOME",
        "TEMP",
        "TMP",
        "TMPDIR",
        "LANG",
        "LC_ALL",
        "TERM",
        "SYSTEMROOT",
    ];
    let mut result = Vec::new();
    for &key in allowlist {
        if let Ok(val) = std::env::var(key) {
            result.push((key.into(), val));
        }
    }
    result
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

    #[test]
    fn env_blocklist_includes_secrets() {
        // Sensitive vars MUST be in the blocklist to prevent leaking.
        assert!(ENV_BLOCKLIST.contains(&"XAI_API_KEY"));
        assert!(ENV_BLOCKLIST.contains(&"AWS_SECRET_ACCESS_KEY"));
        assert!(ENV_BLOCKLIST.contains(&"AWS_ACCESS_KEY_ID"));
        assert!(ENV_BLOCKLIST.contains(&"GITHUB_TOKEN"));
    }

    #[test]
    fn env_blocklist_does_not_block_systemroot() {
        // SYSTEMROOT must NOT be in the blocklist — it's needed on Windows.
        assert!(!ENV_BLOCKLIST.contains(&"SYSTEMROOT"));
        assert!(!ENV_BLOCKLIST.contains(&"PATH"));
        assert!(!ENV_BLOCKLIST.contains(&"USERPROFILE"));
        assert!(!ENV_BLOCKLIST.contains(&"HOME"));
    }

    #[test]
    fn filter_env_never_panics() {
        // filter_env reads from std::env — just ensure it doesn't panic.
        let _ = filter_env();
    }

    #[test]
    fn filter_env_excludes_blocklisted() {
        // Verify that blocklisted vars are excluded from filter_env output.
        let filtered = filter_env();
        let keys: Vec<&str> = filtered.iter().map(|(k, _)| k.as_str()).collect();
        for &blocked in ENV_BLOCKLIST {
            assert!(
                !keys.contains(&blocked),
                "blocklisted var '{}' should NOT appear in filter_env output",
                blocked
            );
        }
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
