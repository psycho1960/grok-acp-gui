//! Fake ACP transport — spawns the Node.js fake-acp-agent for testing.
//!
//! This implements the `AcpTransport` seam so the `AgentRuntimeImpl`
//! can be tested end-to-end without a real Grok binary.
//!
//! The fake agent script is at `tests/fake-acp-agent/agent.mjs`.

use async_trait::async_trait;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStderr, ChildStdin, ChildStdout, Command};
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

/// Which test scenario the fake agent should run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeScenario {
    NotFound,
    VersionTooLow,
    NotAuthenticated,
    TurnAuthRequired,
    Normal,
    Slow,
    Timeout,
    Crash,
    CrashAfterPrompt,
    BadFrame,
    StderrFlood,
    UnknownMethod,
    Permission,
    Plan,
    PlanPermission,
    ProcessWrite,
    ProcessEscape,
    ReadEscape,
    SecretDisplayFields,
    DuplicatePermission,
    DuplicatePlan,
    SetModeError,
    CommandsUpdate,
}

impl FakeScenario {
    fn env_value(&self) -> &'static str {
        match self {
            FakeScenario::NotFound => "not-found",
            FakeScenario::VersionTooLow => "version-too-low",
            FakeScenario::NotAuthenticated => "not-authenticated",
            FakeScenario::TurnAuthRequired => "turn-auth-required",
            FakeScenario::Normal => "normal",
            FakeScenario::Slow => "slow",
            FakeScenario::Timeout => "timeout",
            FakeScenario::Crash => "crash",
            FakeScenario::CrashAfterPrompt => "crash-after-prompt",
            FakeScenario::BadFrame => "bad-frame",
            FakeScenario::StderrFlood => "stderr-flood",
            FakeScenario::UnknownMethod => "unknown-method",
            FakeScenario::Permission => "permission",
            FakeScenario::Plan => "plan",
            FakeScenario::PlanPermission => "plan-permission",
            FakeScenario::ProcessWrite => "process-write",
            FakeScenario::ProcessEscape => "process-escape",
            FakeScenario::ReadEscape => "read-escape",
            FakeScenario::SecretDisplayFields => "secret-display-fields",
            FakeScenario::DuplicatePermission => "duplicate-permission",
            FakeScenario::DuplicatePlan => "duplicate-plan",
            FakeScenario::SetModeError => "set-mode-error",
            FakeScenario::CommandsUpdate => "available-commands",
        }
    }
}

/// A fake ACP transport that spawns the Node.js fake agent.
pub struct FakeAcpTransport {
    scenario: FakeScenario,
    agent_script: PathBuf,
    resolved_path: std::sync::Mutex<Option<PathBuf>>,
}

impl FakeAcpTransport {
    /// Create a new fake transport.  The `agent_script` path should point
    /// to `tests/fake-acp-agent/agent.mjs` relative to the repo root.
    pub fn new(scenario: FakeScenario, agent_script: PathBuf) -> Self {
        Self {
            scenario,
            agent_script,
            resolved_path: std::sync::Mutex::new(None),
        }
    }
}

#[async_trait]
impl AcpTransport for FakeAcpTransport {
    async fn probe(&self, _config: &RuntimeConfig) -> Result<(PathBuf, String), TransportError> {
        match self.scenario {
            FakeScenario::NotFound => {
                return Err(TransportError::NotFound {
                    searched: vec![PathBuf::from("C:/Users/Test User/.grok/bin/grok.exe")],
                })
            }
            FakeScenario::VersionTooLow => {
                return Err(TransportError::VersionTooLow {
                    found: "0.2.117".into(),
                    required: "0.2.118".into(),
                })
            }
            FakeScenario::NotAuthenticated => return Err(TransportError::NotAuthenticated),
            _ => {}
        }
        // The fake transport doesn't need a real grok binary.
        // Find node and report it as the "executable".
        let node = which_node().await?;
        let version = String::from("0.2.118-fake");
        *self.resolved_path.lock().unwrap() = Some(PathBuf::from(&node));
        Ok((PathBuf::from(node), version))
    }

    async fn spawn(
        &self,
        _session_id: SessionId,
        workspace: WorkspaceContext,
        _config: &RuntimeConfig,
    ) -> Result<TransportHandle, TransportError> {
        // Find node executable.
        let node = which_node().await?;

        let mut cmd = Command::new(&node);
        cmd.arg(&self.agent_script);
        cmd.current_dir(&workspace.cwd);

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd.kill_on_drop(true);

        // Inherit only safe env vars (Node.js needs SYSTEMROOT on Windows).
        cmd.env_clear();
        if let Ok(path) = std::env::var("PATH") {
            cmd.env("PATH", path);
        }
        if let Ok(localappdata) = std::env::var("LOCALAPPDATA") {
            cmd.env("LOCALAPPDATA", localappdata);
        }
        if let Ok(systemroot) = std::env::var("SYSTEMROOT") {
            cmd.env("SYSTEMROOT", systemroot);
        }
        if let Ok(temp) = std::env::var("TEMP") {
            cmd.env("TEMP", temp);
        }
        if let Ok(tmp) = std::env::var("TMP") {
            cmd.env("TMP", tmp);
        }
        // Set scenario AFTER env_clear so it's not wiped.
        cmd.env("FAKE_ACP_SCENARIO", self.scenario.env_value());

        #[cfg(target_os = "windows")]
        {
            #[allow(unused_imports)]
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut child = cmd.spawn().map_err(|e| TransportError::SpawnFailed {
            message: format!("failed to spawn node '{}': {}", node, e),
        })?;

        // Store the node path for resolved_path() — we can't mutate self
        // (trait takes &self), so we rely on the caller to have set it.
        // For tests this is acceptable.

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

        // stdin writer
        spawn_stdin_writer(stdin, outbound_rx);

        // stdout reader + decoder
        spawn_stdout_reader(stdout, inbound_tx);

        // stderr reader
        let stderr_buf = StderrBuffer::new(200);
        let stderr_arc = std::sync::Arc::new(tokio::sync::Mutex::new(stderr_buf));
        let stderr_clone = stderr_arc.clone();
        spawn_stderr_reader(stderr, stderr_clone);

        // process monitor
        let process = spawn_process_monitor(child);

        Ok(TransportHandle {
            outbound: outbound_tx,
            inbound: inbound_rx,
            process,
            stderr: stderr_arc,
        })
    }

    async fn start_login(
        &self,
        _method: LoginMethod,
        timeout_secs: u64,
    ) -> Result<LoginHandle, TransportError> {
        let scenario = self.scenario;
        let (state_tx, state_rx) = watch::channel(LoginProcessState::Running);
        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        tauri::async_runtime::spawn(async move {
            let simulated = async move {
                match scenario {
                    FakeScenario::Timeout => std::future::pending().await,
                    FakeScenario::Crash | FakeScenario::CrashAfterPrompt => {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        LoginProcessState::Failed { exit_code: Some(7) }
                    }
                    _ => {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                        LoginProcessState::Succeeded
                    }
                }
            };
            let terminal = tokio::select! {
                result = simulated => result,
                _ = &mut cancel_rx => LoginProcessState::Cancelled,
                _ = tokio::time::sleep(std::time::Duration::from_secs(timeout_secs.max(1))) => LoginProcessState::TimedOut,
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

async fn which_node() -> Result<String, TransportError> {
    let candidates = if cfg!(target_os = "windows") {
        vec!["node.exe", "node"]
    } else {
        vec!["node"]
    };

    for &name in &candidates {
        // Try direct execution.
        if tokio::process::Command::new(name)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .is_ok()
        {
            return Ok(name.into());
        }
    }

    Err(TransportError::NotFound {
        searched: candidates.iter().map(PathBuf::from).collect(),
    })
}

fn spawn_stdin_writer(mut stdin: ChildStdin, mut rx: mpsc::Receiver<String>) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(line) = rx.recv().await {
            let line = format!("{}\n", line);
            if stdin.write_all(line.as_bytes()).await.is_err() {
                break;
            }
            let _ = stdin.flush().await;
        }
        let _ = stdin.shutdown().await;
    })
}

fn spawn_stdout_reader(stdout: ChildStdout, tx: mpsc::Sender<AcpMessage>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stdout);
        let mut decoder = FrameDecoder::new(4 * 1024 * 1024, 64);
        let mut buf = [0u8; 8192];

        loop {
            match reader.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    let results = decoder.feed(&buf[..n]);
                    for result in results {
                        match result {
                            Ok(msg) => {
                                if tx.send(msg).await.is_err() {
                                    return;
                                }
                            }
                            Err(e) => {
                                DiagLog::warn("fake_acp:codec", format!("decode error: {}", e))
                                    .emit();
                            }
                        }
                    }
                }
                Err(e) => {
                    DiagLog::error("fake_acp:stdout", format!("read error: {}", e)).emit();
                    break;
                }
            }
        }
    })
}

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
                Ok(0) => break,
                Ok(_) => {
                    let trimmed = line.trim_end_matches(['\n', '\r']).to_string();
                    buffer.lock().await.push(trimmed);
                }
                Err(e) => {
                    DiagLog::error("fake_acp:stderr", format!("read error: {}", e)).emit();
                    break;
                }
            }
        }
    })
}

fn spawn_process_monitor(mut child: tokio::process::Child) -> JoinHandle<ProcessExit> {
    tokio::spawn(async move {
        let status = match child.wait().await {
            Ok(s) => s,
            Err(_) => {
                return ProcessExit {
                    code: None,
                    signal: None,
                    reason: "unknown".into(),
                };
            }
        };
        let code = status.code();
        let reason = if code == Some(0) {
            "clean".into()
        } else if code.is_some() {
            "crash".into()
        } else {
            "unknown".into()
        };
        ProcessExit {
            code,
            signal: None,
            reason,
        }
    })
}
