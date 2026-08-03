//! AgentRuntimeImpl — the concrete coordinator that implements
//! [`AgentRuntime`].
//!
//! This struct owns:
//! - A map of session_id → `SessionSlot` (state, transport, context)
//! - A global event broadcaster
//! - A reference to the ACP transport (production or fake)
//!
//! All state transitions go through `state::transition()` and are
//! auditable.  The coordinator enforces:
//! - Single managed process per session
//! - Idempotent `cancel` and `shutdown`
//! - Handshake timeout
//! - Event sequence numbering per session

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{timeout, Duration};

use crate::adapters::grok_acp::codec::{encode_notification, encode_request, AcpMessage};
use crate::adapters::grok_acp::interpreter::{self, AcpSessionContext, InterpretationResult};
use crate::adapters::grok_acp::transport::{AcpTransport, ProcessExit, TransportHandle};
use crate::bridge::types::SessionId;
use crate::domain::error::{codes, DomainError};
use crate::modules::agent_runtime::diagnostics::DiagLog;
use crate::modules::agent_runtime::events::{
    AgentEvent, EventMeta, ProcessExitedPayload, SessionReadyPayload, TimestampedEvent,
};
use crate::modules::agent_runtime::requests::{ClientRequest, SendAck};
use crate::modules::agent_runtime::state::{self, RuntimeState, RuntimeTransition};
use crate::modules::agent_runtime::{
    config::{RuntimeConfig, RuntimeHandle, RuntimeProbeResult, WorkspaceContext},
    AgentRuntime,
};

/// Maximum event channel buffer size per subscriber.
const EVENT_CHANNEL_BUFFER: usize = 1024;

/// Maximum time to wait for a clean process exit before killing.
const KILL_GRACE_SECS: u64 = 5;

/// Internal event sent from a session reader task to the central
/// forwarder. Carries the session_id so the forwarder knows where it
/// came from.
struct SessionInternalEvent {
    #[allow(dead_code)]
    session_id: SessionId,
    event: TimestampedEvent,
}

/// A managed session slot.
struct SessionSlot {
    state: RuntimeState,
    /// Sender for outbound JSON-RPC messages to the child's stdin.
    outbound: Option<mpsc::Sender<String>>,
    /// The process join handle (for shutdown).
    process: Option<tokio::task::JoinHandle<ProcessExit>>,
    /// Per-session interpretation context (kept for diagnostics / seq tracking).
    interp_ctx: AcpSessionContext,
    /// Resolved executable path (for the handle).
    executable_path: String,
}

impl SessionSlot {
    fn new(executable_path: String) -> Self {
        Self {
            state: RuntimeState::Unavailable,
            outbound: None,
            process: None,
            interp_ctx: AcpSessionContext::new(),
            executable_path,
        }
    }
}

/// The concrete AgentRuntime implementation.
///
/// Generic over the transport to allow testing with a Fake ACP adapter.
/// The runtime is always stored inside an `Arc` — spawned tasks capture
/// only the channels they need, never a reference to `self`.
pub struct AgentRuntimeImpl<T: AcpTransport> {
    transport: Arc<T>,
    sessions: Mutex<HashMap<SessionId, SessionSlot>>,
    /// Subscribers receive all session events. Each subscriber has its
    /// own bounded channel.
    event_subscribers: Mutex<Vec<mpsc::Sender<TimestampedEvent>>>,
    /// Central channel that session reader tasks send events to.
    /// A forwarder task distributes these to subscribers.
    internal_event_tx: mpsc::Sender<SessionInternalEvent>,
}

impl<T: AcpTransport + 'static> AgentRuntimeImpl<T> {
    /// Create a new runtime with the given transport.
    pub fn new(transport: T) -> Arc<Self> {
        let transport = Arc::new(transport);
        let (internal_event_tx, internal_event_rx) =
            mpsc::channel::<SessionInternalEvent>(EVENT_CHANNEL_BUFFER);

        let runtime = Arc::new(Self {
            transport,
            sessions: Mutex::new(HashMap::new()),
            event_subscribers: Mutex::new(Vec::new()),
            internal_event_tx,
        });

        // Spawn the central event forwarder.
        runtime.clone().spawn_forwarder(internal_event_rx);

        runtime
    }

    /// Spawn the central event forwarder that distributes events
    /// from session reader tasks to all subscribers.
    fn spawn_forwarder(self: Arc<Self>, rx: mpsc::Receiver<SessionInternalEvent>) {
        tokio::spawn(async move {
            let mut rx = rx;
            while let Some(internal) = rx.recv().await {
                let mut subs = self.event_subscribers.lock().await;
                let mut dead = Vec::new();
                for (i, tx) in subs.iter().enumerate() {
                    if tx.try_send(internal.event.clone()).is_err() {
                        dead.push(i);
                    }
                }
                for i in dead.into_iter().rev() {
                    subs.remove(i);
                }
            }
        });
    }

    /// Attempt a state transition for a session.
    async fn try_transition(
        &self,
        session_id: &SessionId,
        t: RuntimeTransition,
    ) -> Result<RuntimeState, DomainError> {
        let mut sessions = self.sessions.lock().await;
        let slot = sessions
            .get_mut(session_id)
            .ok_or_else(|| DomainError::new(codes::DOMAIN_TASK_NOT_FOUND, "session not found"))?;
        let new_state = state::transition(slot.state, t)?;
        slot.state = new_state;
        Ok(new_state)
    }

    /// Get the current state of a session.
    async fn get_state(&self, session_id: &SessionId) -> Option<RuntimeState> {
        let sessions = self.sessions.lock().await;
        sessions.get(session_id).map(|s| s.state)
    }

    /// Emit a single event from the runtime itself (not from a reader task).
    async fn emit_runtime_event(&self, event: TimestampedEvent) {
        let _ = self
            .internal_event_tx
            .send(SessionInternalEvent {
                session_id: event.meta.session_id.clone(),
                event,
            })
            .await;
    }
}

#[async_trait]
impl<T: AcpTransport + 'static> AgentRuntime for AgentRuntimeImpl<T> {
    async fn probe(&self, config: &RuntimeConfig) -> RuntimeProbeResult {
        // Delegate to the transport's probe method.
        match self.transport.probe(config).await {
            Ok((path, version)) => RuntimeProbeResult::ready(path, version, true),
            Err(super::super::super::adapters::grok_acp::TransportError::NotFound { .. }) => {
                RuntimeProbeResult::not_found()
            }
            Err(super::super::super::adapters::grok_acp::TransportError::VersionTooLow {
                found,
                required,
            }) => RuntimeProbeResult::version_too_low(found, &required),
            Err(super::super::super::adapters::grok_acp::TransportError::NotAuthenticated) => {
                RuntimeProbeResult::not_authenticated()
            }
            Err(e) => RuntimeProbeResult::probe_error(e.to_string()),
        }
    }

    async fn start(
        &self,
        session_id: SessionId,
        workspace: WorkspaceContext,
        config: &RuntimeConfig,
    ) -> Result<RuntimeHandle, DomainError> {
        // Create or reset the session slot.
        {
            let mut sessions = self.sessions.lock().await;
            if let Some(existing) = sessions.get(&session_id) {
                if existing.state.is_live() {
                    return Err(DomainError::new(
                        codes::DOMAIN_ILLEGAL_TRANSITION,
                        format!(
                            "session '{}' is already active (state: {})",
                            session_id, existing.state
                        ),
                    ));
                }
            }
            sessions.insert(
                session_id.clone(),
                SessionSlot::new(
                    self.transport
                        .resolved_path()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default(),
                ),
            );
        }

        // Transition to probing.
        self.try_transition(&session_id, RuntimeTransition::BeginProbe)
            .await?;

        // Ensure the transport has been probed.  If probe() hasn't been
        // called yet (resolved_path is None), call it now.
        if self.transport.resolved_path().is_none() {
            self.transport
                .probe(config)
                .await
                .map_err(|e| DomainError::new(codes::RUNTIME_PROBE_FAILED, e.to_string()))?;
        }

        // Now that probe has resolved the executable path, update the
        // session slot's executable_path for the RuntimeHandle.
        {
            let mut sessions = self.sessions.lock().await;
            if let Some(slot) = sessions.get_mut(&session_id) {
                slot.executable_path = self
                    .transport
                    .resolved_path()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
            }
        }

        // Spawn the transport.
        let TransportHandle {
            outbound,
            mut inbound,
            process,
            stderr,
        } = self
            .transport
            .spawn(session_id.clone(), workspace)
            .await
            .map_err(|e| {
                DomainError::new(
                    codes::RUNTIME_PROBE_FAILED,
                    format!("transport spawn failed: {}", e),
                )
            })?;

        // Transition: probe succeeded → starting → handshaking.
        self.try_transition(&session_id, RuntimeTransition::ProbeSucceeded)
            .await?;
        self.try_transition(&session_id, RuntimeTransition::ProcessSpawned)
            .await?;

        // Store the transport handles in the session slot.
        {
            let mut sessions = self.sessions.lock().await;
            let slot = sessions.get_mut(&session_id).unwrap();
            slot.outbound = Some(outbound.clone());
            slot.process = Some(process);
        }

        // Perform the handshake BEFORE spawning the reader task.
        // This lets us intercept the `initialize` response directly.
        match perform_handshake(&outbound, &mut inbound, config.handshake_timeout_secs).await {
            Ok(handshake_info) => {
                self.try_transition(&session_id, RuntimeTransition::HandshakeComplete)
                    .await?;

                // Emit session_ready event.
                let event = TimestampedEvent {
                    meta: EventMeta::new(session_id.clone(), 0),
                    event: AgentEvent::SessionReady(SessionReadyPayload {
                        protocol_version: handshake_info.protocol_version,
                        agent_name: handshake_info.agent_name,
                        agent_version: handshake_info.agent_version,
                        models: vec![],
                        modes: vec![],
                    }),
                };
                self.emit_runtime_event(event).await;

                // NOW spawn the inbound reader task for subsequent messages.
                let internal_tx = self.internal_event_tx.clone();
                let sid_reader = session_id.clone();
                tokio::spawn(async move {
                    let mut interp_ctx = AcpSessionContext::new();
                    while let Some(msg) = inbound.recv().await {
                        match interpreter::interpret(&msg, &sid_reader, &mut interp_ctx) {
                            InterpretationResult::Events(events) => {
                                for ev in events {
                                    if internal_tx
                                        .send(SessionInternalEvent {
                                            session_id: sid_reader.clone(),
                                            event: ev,
                                        })
                                        .await
                                        .is_err()
                                    {
                                        break;
                                    }
                                }
                            }
                            InterpretationResult::Unknown { method } => {
                                DiagLog::warn(
                                    "agent_runtime:interpreter",
                                    format!("unknown ACP method: {}", method),
                                )
                                .emit();
                            }
                            InterpretationResult::ProtocolError { message } => {
                                DiagLog::error(
                                    "agent_runtime:interpreter",
                                    format!("protocol error: {}", message),
                                )
                                .emit();
                            }
                            InterpretationResult::Ack | InterpretationResult::NoEvent => {}
                        }
                    }
                    // Inbound channel closed — process exited.
                    let exit_event = TimestampedEvent {
                        meta: EventMeta::new(sid_reader.clone(), interp_ctx.next_sequence),
                        event: AgentEvent::ProcessExited(ProcessExitedPayload {
                            code: None,
                            signal: None,
                            reason: "inbound_closed".into(),
                        }),
                    };
                    let _ = internal_tx
                        .send(SessionInternalEvent {
                            session_id: sid_reader,
                            event: exit_event,
                        })
                        .await;
                });

                let exec_path = {
                    let sessions = self.sessions.lock().await;
                    sessions
                        .get(&session_id)
                        .map(|s| s.executable_path.clone())
                        .unwrap_or_default()
                };

                Ok(RuntimeHandle {
                    session_id,
                    executable_path: exec_path,
                })
            }
            Err(e) => {
                // Handshake failed — transition to failed.
                let _ = self
                    .try_transition(&session_id, RuntimeTransition::Fail { reason: e.clone() })
                    .await;

                // Capture stderr diagnostic (bounded, redacted).
                {
                    let stderr = stderr.lock().await;
                    DiagLog::error(
                        "agent_runtime:handshake",
                        format!("handshake failed: {}", e),
                    )
                    .with_context(serde_json::json!({
                        "stderr_snapshot": stderr.snapshot(),
                    }))
                    .emit();
                }

                // Try to send a shutdown message.
                let _ = outbound
                    .send(encode_request(0, "shutdown", &serde_json::json!({})))
                    .await;

                Err(DomainError::new(codes::ACP_HANDSHAKE_FAILED, e))
            }
        }
    }

    async fn send(
        &self,
        session_id: SessionId,
        request: ClientRequest,
    ) -> Result<SendAck, DomainError> {
        let current_state = self.get_state(&session_id).await;
        match current_state {
            Some(RuntimeState::Ready) => {}
            Some(state) => {
                return Err(DomainError::illegal_transition(
                    "Runtime",
                    &state.to_string(),
                    "send",
                ));
            }
            None => {
                return Err(DomainError::new(
                    codes::DOMAIN_TASK_NOT_FOUND,
                    format!("session '{}' not found", session_id),
                ));
            }
        }

        // Transition to Busy.
        self.try_transition(&session_id, RuntimeTransition::TurnStarted)
            .await?;

        // Get the outbound channel.
        let outbound = {
            let sessions = self.sessions.lock().await;
            sessions
                .get(&session_id)
                .and_then(|s| s.outbound.clone())
                .ok_or_else(|| {
                    DomainError::new(codes::RUNTIME_PROCESS_DIED, "no outbound channel")
                })?
        };

        // Allocate a request ID.
        let request_id = {
            let mut sessions = self.sessions.lock().await;
            let slot = sessions.get_mut(&session_id).unwrap();
            slot.interp_ctx.next_sequence += 1;
            slot.interp_ctx.current_request_id = Some(slot.interp_ctx.next_sequence);
            slot.interp_ctx.next_sequence
        };

        let (method, params): (&str, serde_json::Value) = match &request {
            ClientRequest::Prompt(p) => {
                let mut params = serde_json::json!({
                    "message": p.message,
                });
                if let Some(mode) = &p.mode {
                    params["mode"] = serde_json::Value::String(mode.clone());
                }
                if let Some(model) = &p.model {
                    params["model"] = serde_json::Value::String(model.clone());
                }
                ("session/prompt", params)
            }
            ClientRequest::Cancel => {
                return Ok(SendAck { request_id });
            }
            ClientRequest::ResolvePermission(r) => {
                let params = serde_json::json!({
                    "requestId": r.request_id,
                    "optionId": r.option_id,
                });
                ("resolvePermission", params)
            }
            ClientRequest::ResolvePlan(r) => {
                let params = serde_json::json!({
                    "requestId": r.request_id,
                    "optionId": r.option_id,
                });
                ("resolvePlan", params)
            }
        };

        let encoded = encode_request(request_id, method, &params);
        outbound.send(encoded).await.map_err(|_| {
            DomainError::new(codes::RUNTIME_PROCESS_DIED, "failed to send to process")
        })?;

        Ok(SendAck { request_id })
    }

    async fn cancel(&self, session_id: SessionId, _request_id: Option<u64>) {
        let current = self.get_state(&session_id).await;
        match current {
            Some(RuntimeState::Busy) => {
                let outbound = {
                    let sessions = self.sessions.lock().await;
                    sessions.get(&session_id).and_then(|s| s.outbound.clone())
                };
                if let Some(tx) = outbound {
                    let encoded = encode_notification("session/cancel", &serde_json::json!({}));
                    let _ = tx.send(encoded).await;
                }
                let _ = self
                    .try_transition(&session_id, RuntimeTransition::TurnCompleted)
                    .await;
            }
            _ => {
                DiagLog::info(
                    "agent_runtime",
                    format!(
                        "cancel() on session {} in non-busy state — no-op",
                        session_id
                    ),
                )
                .emit();
            }
        }
    }

    async fn shutdown(&self, session_id: SessionId, reason: &str) {
        let current = self.get_state(&session_id).await;
        if current.is_none() {
            return;
        }

        let _ = self
            .try_transition(&session_id, RuntimeTransition::BeginShutdown)
            .await;

        DiagLog::info(
            "agent_runtime",
            format!("shutting down session '{}': {}", session_id, reason),
        )
        .emit();

        // Close the outbound channel (drops the sender → stdin EOF).
        let outbound = {
            let mut sessions = self.sessions.lock().await;
            sessions
                .get_mut(&session_id)
                .and_then(|s| s.outbound.take())
        };
        drop(outbound);

        // Wait for the process to exit (with grace timeout).
        let process = {
            let mut sessions = self.sessions.lock().await;
            sessions.get_mut(&session_id).and_then(|s| s.process.take())
        };

        if let Some(mut handle) = process {
            let grace = Duration::from_secs(KILL_GRACE_SECS);
            match timeout(grace, &mut handle).await {
                Ok(join_result) => {
                    if let Ok(exit) = join_result {
                        DiagLog::info(
                            "agent_runtime",
                            format!(
                                "session '{}' process exited: code={:?} reason={}",
                                session_id, exit.code, exit.reason
                            ),
                        )
                        .emit();
                    }
                }
                Err(_) => {
                    DiagLog::warn(
                        "agent_runtime",
                        format!(
                            "session '{}' did not exit in {}s — killing",
                            session_id, KILL_GRACE_SECS
                        ),
                    )
                    .emit();
                    handle.abort();
                }
            }
        }

        let _ = self
            .try_transition(&session_id, RuntimeTransition::ProcessExited)
            .await;
    }

    fn subscribe(&self) -> mpsc::Receiver<TimestampedEvent> {
        let (tx, rx) = mpsc::channel(EVENT_CHANNEL_BUFFER);
        // try_lock is acceptable here because subscribe() is called
        // rarely and contention is minimal.
        if let Ok(mut subs) = self.event_subscribers.try_lock() {
            subs.push(tx);
        } else {
            // Fallback: log and return an empty channel.
            // The caller will simply not receive events until they retry.
            DiagLog::warn(
                "agent_runtime",
                "subscribe() called during contention — events may be missed",
            )
            .emit();
        }
        rx
    }

    fn session_state(&self, session_id: &SessionId) -> Option<RuntimeState> {
        match self.sessions.try_lock() {
            Ok(sessions) => sessions.get(session_id).map(|s| s.state),
            Err(_) => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Handshake
// ---------------------------------------------------------------------------

/// Information extracted from a successful ACP `initialize` response.
struct HandshakeInfo {
    protocol_version: u32,
    agent_name: String,
    agent_version: String,
}

/// Perform the ACP handshake: send `initialize` and wait for the response.
///
/// Reads the initialize response directly from the inbound channel
/// (before the reader task is spawned).  Times out if no response
/// arrives within `timeout_secs`.
async fn perform_handshake(
    outbound: &mpsc::Sender<String>,
    inbound: &mut mpsc::Receiver<AcpMessage>,
    timeout_secs: u64,
) -> Result<HandshakeInfo, String> {
    let init_params = serde_json::json!({
        "protocolVersion": 1,
        "clientCapabilities": {
            "fs": {},
            "terminal": {}
        }
    });
    let init_request = encode_request(1, "initialize", &init_params);
    outbound
        .send(init_request)
        .await
        .map_err(|e| format!("failed to send initialize: {}", e))?;

    // Wait for the initialize response with a timeout.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(format!("handshake timed out after {}s", timeout_secs));
        }
        match timeout(remaining, inbound.recv()).await {
            Ok(Some(msg)) => {
                match msg {
                    AcpMessage::Response(resp) => {
                        if let Some(err) = resp.error {
                            return Err(format!(
                                "initialize returned error: [{}] {}",
                                err.code, err.message
                            ));
                        }
                        // Extract handshake info from the result.
                        let result = resp.result.unwrap_or(serde_json::Value::Null);
                        let protocol_version = result
                            .get("protocolVersion")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(1) as u32;
                        let agent_name = result
                            .get("agentName")
                            .or_else(|| result.get("name"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let agent_version = result
                            .get("agentVersion")
                            .or_else(|| result.get("version"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        return Ok(HandshakeInfo {
                            protocol_version,
                            agent_name,
                            agent_version,
                        });
                    }
                    AcpMessage::Unknown(_) => {
                        // Non-JSON line (e.g. blank line) — skip and continue.
                        continue;
                    }
                    AcpMessage::Request(_) | AcpMessage::Notification(_) => {
                        // The agent sent a request/notification before the
                        // initialize response.  This is unusual but not
                        // necessarily fatal — continue waiting.
                        continue;
                    }
                }
            }
            Ok(None) => {
                // Channel closed — process exited.
                return Err("process closed stdout before initialize response".into());
            }
            Err(_) => {
                return Err(format!("handshake timed out after {}s", timeout_secs));
            }
        }
    }
}
