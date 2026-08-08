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
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::time::{timeout, Duration};

use crate::adapters::filesystem::WorkspaceFilesystem;
use crate::adapters::grok_acp::codec::{
    encode_notification, encode_request, encode_response_error, encode_response_result, AcpError,
    AcpMessage, AcpRequest,
};
use crate::adapters::grok_acp::interpreter::{self, AcpSessionContext, InterpretationResult};
use crate::adapters::grok_acp::transport::{AcpTransport, ProcessExit, TransportHandle};
use crate::bridge::types::SessionId;
use crate::domain::error::{codes, DomainError};
use crate::modules::agent_runtime::diagnostics::DiagLog;
use crate::modules::agent_runtime::events::{
    AgentEvent, EventMeta, ModeDescriptor, ProcessExitedPayload, SessionReadyPayload,
    TimestampedEvent, TurnCancelledPayload,
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
    /// Stops the inbound reader so it releases its outbound sender clone;
    /// only then can dropping SessionSlot::outbound close child stdin.
    reader_shutdown: Option<oneshot::Sender<()>>,
    reader_task: Option<tokio::task::JoinHandle<()>>,
    /// Per-session interpretation context (kept for diagnostics / seq tracking).
    interp_ctx: Arc<StdMutex<AcpSessionContext>>,
    /// JSON-RPC request ids are independent from event sequence numbers.
    next_request_id: u64,
    /// Session id allocated by the ACP agent via `session/new`.
    acp_session_id: Option<String>,
    /// Mode ids advertised by ACP in the `session/new` response.
    available_mode_ids: Vec<String>,
    /// Agent-to-client JSON-RPC request ids awaiting a permission response.
    /// Keys are the stable request ids exposed to the task runtime; values are
    /// the raw JSON-RPC ids that must be echoed in the response envelope.
    pending_permission_requests: Arc<StdMutex<HashMap<String, serde_json::Value>>>,
    /// Legacy Plan proposals may also arrive as JSON-RPC requests. They use
    /// the same response shape and correlation rule as permission requests.
    pending_plan_requests: Arc<StdMutex<HashMap<String, serde_json::Value>>>,
    /// Resolved executable path (for the handle).
    executable_path: String,
    /// Whether shutdown began while a turn was still in progress. Late
    /// completion frames cannot turn that uncertain shutdown into success.
    shutdown_interrupted_turn: bool,
}

impl SessionSlot {
    fn new(executable_path: String) -> Self {
        Self {
            state: RuntimeState::Unavailable,
            outbound: None,
            process: None,
            reader_shutdown: None,
            reader_task: None,
            interp_ctx: Arc::new(StdMutex::new(AcpSessionContext::from_sequence(2))),
            // 1..=3 are reserved for initialize/authenticate/session-new.
            next_request_id: 3,
            acp_session_id: None,
            available_mode_ids: Vec::new(),
            pending_permission_requests: Arc::new(StdMutex::new(HashMap::new())),
            pending_plan_requests: Arc::new(StdMutex::new(HashMap::new())),
            executable_path,
            shutdown_interrupted_turn: false,
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
    event_subscribers: StdMutex<Vec<mpsc::Sender<TimestampedEvent>>>,
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
            event_subscribers: StdMutex::new(Vec::new()),
            internal_event_tx,
        });

        // Spawn the central event forwarder.
        runtime.clone().spawn_forwarder(internal_event_rx);

        runtime
    }

    /// Spawn the central event forwarder that distributes events
    /// from session reader tasks to all subscribers.
    fn spawn_forwarder(self: Arc<Self>, rx: mpsc::Receiver<SessionInternalEvent>) {
        let runtime = Arc::downgrade(&self);
        tauri::async_runtime::spawn(async move {
            let mut rx = rx;
            while let Some(mut internal) = rx.recv().await {
                let Some(runtime) = runtime.upgrade() else {
                    break;
                };
                if matches!(
                    internal.event.event,
                    AgentEvent::AssistantCompleted(_) | AgentEvent::RequestFailed(_)
                ) {
                    let _ = runtime
                        .try_transition(&internal.session_id, RuntimeTransition::TurnCompleted)
                        .await;
                }
                if matches!(internal.event.event, AgentEvent::ProcessExited(_)) {
                    match runtime.get_state(&internal.session_id).await {
                        Some(RuntimeState::Stopping) => {
                            if let AgentEvent::ProcessExited(exit) = &mut internal.event.event {
                                let interrupted_turn = runtime
                                    .sessions
                                    .lock()
                                    .await
                                    .get(&internal.session_id)
                                    .is_some_and(|slot| slot.shutdown_interrupted_turn);
                                exit.reason = if interrupted_turn {
                                    "shutdown_interrupted"
                                } else {
                                    "clean"
                                }
                                .into();
                            }
                            let _ = runtime
                                .try_transition(
                                    &internal.session_id,
                                    RuntimeTransition::ProcessExited,
                                )
                                .await;
                        }
                        Some(RuntimeState::Stopped) | None => {
                            if let AgentEvent::ProcessExited(exit) = &mut internal.event.event {
                                let interrupted_turn = runtime
                                    .sessions
                                    .lock()
                                    .await
                                    .get(&internal.session_id)
                                    .is_some_and(|slot| slot.shutdown_interrupted_turn);
                                exit.reason = if interrupted_turn {
                                    "shutdown_interrupted"
                                } else {
                                    "clean"
                                }
                                .into();
                            }
                        }
                        Some(_) => {
                            let _ = runtime
                                .try_transition(
                                    &internal.session_id,
                                    RuntimeTransition::Fail {
                                        reason: "managed process exited unexpectedly".into(),
                                    },
                                )
                                .await;
                            let _ = runtime
                                .try_transition(
                                    &internal.session_id,
                                    RuntimeTransition::CleanupComplete,
                                )
                                .await;
                        }
                    }
                }
                let mut subs = runtime
                    .event_subscribers
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let mut dead = Vec::new();
                for (i, tx) in subs.iter().enumerate() {
                    match tx.try_send(internal.event.clone()) {
                        Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => {}
                        Err(mpsc::error::TrySendError::Closed(_)) => dead.push(i),
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
        super::config::validate_model_id(config.model.as_deref()).map_err(|message| {
            DomainError::new(crate::domain::error::codes::RUNTIME_INVALID_MODEL, message)
        })?;

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

        let workspace_cwd = workspace.cwd.clone();
        let workspace_filesystem = WorkspaceFilesystem::new(&workspace_cwd)
            .map_err(|error| DomainError::new(codes::ACP_REQUEST_FAILED, error.safe_message()))?;

        // Spawn the transport.
        let TransportHandle {
            outbound,
            mut inbound,
            process,
            stderr,
        } = self
            .transport
            .spawn(session_id.clone(), workspace, config)
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
            slot.shutdown_interrupted_turn = false;
        }

        // Perform the handshake BEFORE spawning the reader task.
        // This lets us intercept the `initialize` response directly.
        match perform_handshake(
            &outbound,
            &mut inbound,
            config.handshake_timeout_secs,
            &workspace_cwd,
        )
        .await
        {
            Ok(handshake_info) => {
                self.try_transition(&session_id, RuntimeTransition::HandshakeComplete)
                    .await?;

                let reader_acp_session_id = handshake_info.acp_session_id.clone();
                let available_mode_ids = handshake_info
                    .modes
                    .iter()
                    .map(|mode| mode.id.clone())
                    .collect();

                // Emit session_ready event.
                let event = TimestampedEvent {
                    meta: EventMeta::new(session_id.clone(), 1),
                    event: AgentEvent::SessionReady(SessionReadyPayload {
                        protocol_version: handshake_info.protocol_version,
                        agent_name: handshake_info.agent_name,
                        agent_version: handshake_info.agent_version,
                        models: vec![],
                        modes: handshake_info.modes,
                    }),
                };
                self.emit_runtime_event(event).await;

                // NOW spawn the inbound reader task for subsequent messages.
                let (interp_ctx, pending_permission_requests, pending_plan_requests) = {
                    let mut sessions = self.sessions.lock().await;
                    let slot = sessions.get_mut(&session_id).unwrap();
                    slot.acp_session_id = Some(handshake_info.acp_session_id);
                    slot.available_mode_ids = available_mode_ids;
                    (
                        slot.interp_ctx.clone(),
                        slot.pending_permission_requests.clone(),
                        slot.pending_plan_requests.clone(),
                    )
                };
                let internal_tx = self.internal_event_tx.clone();
                let sid_reader = session_id.clone();
                let reader_outbound = outbound.clone();
                let (reader_shutdown_tx, mut reader_shutdown_rx) = oneshot::channel();
                if let Some(slot) = self.sessions.lock().await.get_mut(&session_id) {
                    slot.reader_shutdown = Some(reader_shutdown_tx);
                }
                let reader_task = tokio::spawn(async move {
                    loop {
                        let msg = tokio::select! {
                            _ = &mut reader_shutdown_rx => break,
                            message = inbound.recv() => match message {
                                Some(message) => message,
                                None => break,
                            },
                        };
                        if let AcpMessage::Request(request) = &msg {
                            if let Some(response) = handle_filesystem_request(
                                request,
                                &reader_acp_session_id,
                                &workspace_filesystem,
                            ) {
                                if reader_outbound.send(response).await.is_err() {
                                    break;
                                }
                                continue;
                            }
                            if let Some(request_id) = interpreter::permission_request_id(request) {
                                let duplicate = {
                                    let mut pending = pending_permission_requests.lock().unwrap();
                                    match pending.entry(request_id) {
                                        std::collections::hash_map::Entry::Occupied(_) => true,
                                        std::collections::hash_map::Entry::Vacant(entry) => {
                                            entry.insert(request.id.clone());
                                            false
                                        }
                                    }
                                };
                                if duplicate {
                                    let error = AcpError {
                                        code: crate::adapters::grok_acp::codec::error_codes::INVALID_REQUEST,
                                        message: "duplicate pending permission request id".into(),
                                        data: serde_json::Value::Null,
                                    };
                                    if reader_outbound
                                        .send(encode_response_error(&request.id, &error))
                                        .await
                                        .is_err()
                                    {
                                        break;
                                    }
                                    continue;
                                }
                            } else if let Some(request_id) = interpreter::plan_request_id(request) {
                                let duplicate = {
                                    let mut pending = pending_plan_requests.lock().unwrap();
                                    match pending.entry(request_id) {
                                        std::collections::hash_map::Entry::Occupied(_) => true,
                                        std::collections::hash_map::Entry::Vacant(entry) => {
                                            entry.insert(request.id.clone());
                                            false
                                        }
                                    }
                                };
                                if duplicate {
                                    let error = AcpError {
                                        code: crate::adapters::grok_acp::codec::error_codes::INVALID_REQUEST,
                                        message: "duplicate pending Plan request id".into(),
                                        data: serde_json::Value::Null,
                                    };
                                    if reader_outbound
                                        .send(encode_response_error(&request.id, &error))
                                        .await
                                        .is_err()
                                    {
                                        break;
                                    }
                                    continue;
                                }
                            }
                        }
                        let interpreted = {
                            let mut context = interp_ctx.lock().unwrap();
                            interpreter::interpret(&msg, &sid_reader, &mut context)
                        };
                        match interpreted {
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
                                if let AcpMessage::Request(request) = &msg {
                                    let error = AcpError {
                                        code: crate::adapters::grok_acp::codec::error_codes::METHOD_NOT_FOUND,
                                        message: "Method not supported".into(),
                                        data: serde_json::Value::Null,
                                    };
                                    if reader_outbound
                                        .send(encode_response_error(&request.id, &error))
                                        .await
                                        .is_err()
                                    {
                                        break;
                                    }
                                }
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
                    pending_permission_requests.lock().unwrap().clear();
                    pending_plan_requests.lock().unwrap().clear();
                    // Inbound channel closed — process exited.
                    let exit_sequence = {
                        let mut context = interp_ctx.lock().unwrap();
                        let sequence = context.next_sequence;
                        context.next_sequence += 1;
                        sequence
                    };
                    let exit_event = TimestampedEvent {
                        meta: EventMeta::new(sid_reader.clone(), exit_sequence),
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
                if let Some(slot) = self.sessions.lock().await.get_mut(&session_id) {
                    slot.reader_task = Some(reader_task);
                }

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
        let starts_turn = matches!(request, ClientRequest::Prompt(_));
        let valid_state = matches!(
            (&request, current_state.as_ref()),
            (ClientRequest::Prompt(_), Some(RuntimeState::Ready))
                | (
                    ClientRequest::ResolvePermission(_) | ClientRequest::ResolvePlan(_),
                    Some(RuntimeState::Busy)
                )
                | (ClientRequest::Cancel, Some(RuntimeState::Busy))
        );
        if !valid_state {
            match current_state {
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
        }

        let mode_to_set = match &request {
            ClientRequest::Prompt(prompt) => match prompt.mode.as_deref() {
                Some(requested_mode) => {
                    let advertised_modes = self
                        .sessions
                        .lock()
                        .await
                        .get(&session_id)
                        .map(|slot| slot.available_mode_ids.clone())
                        .ok_or_else(|| {
                            DomainError::new(codes::DOMAIN_TASK_NOT_FOUND, "session not found")
                        })?;
                    if advertised_modes.is_empty() {
                        if requested_mode.eq_ignore_ascii_case("plan") {
                            return Err(DomainError::new(
                                codes::ACP_REQUEST_FAILED,
                                "ACP did not advertise the required plan mode",
                            ));
                        }
                        None
                    } else if advertised_modes.iter().any(|mode| mode == requested_mode) {
                        Some(requested_mode.to_string())
                    } else {
                        return Err(DomainError::new(
                            codes::ACP_REQUEST_FAILED,
                            "requested session mode is not advertised by ACP",
                        ));
                    }
                }
                None => None,
            },
            _ => None,
        };

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
        let (request_id, mode_request_id, acp_session_id) = {
            let mut sessions = self.sessions.lock().await;
            let slot = sessions.get_mut(&session_id).unwrap();
            let mode_request_id = mode_to_set.as_ref().map(|_| {
                slot.next_request_id += 1;
                slot.next_request_id
            });
            slot.next_request_id += 1;
            let request_id = slot.next_request_id;
            let acp_session_id = slot.acp_session_id.clone().ok_or_else(|| {
                DomainError::new(codes::ACP_HANDSHAKE_FAILED, "ACP session id is missing")
            })?;
            (request_id, mode_request_id, acp_session_id)
        };

        if let (Some(mode_request_id), Some(mode_id)) = (mode_request_id, mode_to_set.as_deref()) {
            let encoded = encode_request(
                mode_request_id,
                "session/set_mode",
                &serde_json::json!({
                    "sessionId": acp_session_id,
                    "modeId": mode_id,
                }),
            );
            outbound.send(encoded).await.map_err(|_| {
                DomainError::new(codes::RUNTIME_PROCESS_DIED, "failed to send to process")
            })?;
        }

        if starts_turn {
            self.try_transition(&session_id, RuntimeTransition::TurnStarted)
                .await?;
            let sessions = self.sessions.lock().await;
            let slot = sessions.get(&session_id).ok_or_else(|| {
                DomainError::new(codes::DOMAIN_TASK_NOT_FOUND, "session not found")
            })?;
            let mut context = slot.interp_ctx.lock().unwrap();
            context.current_request_id = Some(request_id);
            context.suppress_turn_updates = false;
            context.clear_error_hint();
        }

        let encoded = match &request {
            ClientRequest::Prompt(p) => {
                let params = serde_json::json!({
                    "sessionId": acp_session_id,
                    "prompt": [{
                        "type": "text",
                        "text": p.message,
                    }],
                });
                encode_request(request_id, "session/prompt", &params)
            }
            ClientRequest::Cancel => {
                return Ok(SendAck { request_id });
            }
            ClientRequest::ResolvePermission(r) => {
                let (pending, rpc_id) = {
                    let sessions = self.sessions.lock().await;
                    let slot = sessions.get(&session_id).ok_or_else(|| {
                        DomainError::new(codes::DOMAIN_TASK_NOT_FOUND, "session not found")
                    })?;
                    let pending = slot.pending_permission_requests.clone();
                    let rpc_id = pending.lock().unwrap().remove(&r.request_id);
                    (pending, rpc_id)
                };
                let rpc_id = rpc_id.ok_or_else(|| {
                    DomainError::new(
                        codes::ACP_REQUEST_FAILED,
                        "permission request is no longer pending at the ACP transport",
                    )
                })?;
                let encoded = encode_response_result(
                    &rpc_id,
                    &serde_json::json!({
                        "outcome": {
                            "outcome": "selected",
                            "optionId": r.option_id,
                        }
                    }),
                );
                // Only a successful write may keep the pending entry removed;
                // on failure restore it so the resolution can be retried.
                if outbound.send(encoded).await.is_err() {
                    pending.lock().unwrap().insert(r.request_id.clone(), rpc_id);
                    return Err(DomainError::new(
                        codes::RUNTIME_PROCESS_DIED,
                        "failed to send to process",
                    ));
                }
                return Ok(SendAck { request_id });
            }
            ClientRequest::ResolvePlan(r) => {
                let (pending, rpc_id) = {
                    let sessions = self.sessions.lock().await;
                    let slot = sessions.get(&session_id).ok_or_else(|| {
                        DomainError::new(codes::DOMAIN_TASK_NOT_FOUND, "session not found")
                    })?;
                    let pending = slot.pending_plan_requests.clone();
                    let rpc_id = pending.lock().unwrap().remove(&r.request_id);
                    (pending, rpc_id)
                };
                let rpc_id = rpc_id.ok_or_else(|| {
                    DomainError::new(
                        codes::ACP_REQUEST_FAILED,
                        "Plan request is no longer pending at the ACP transport",
                    )
                })?;
                let encoded = encode_response_result(
                    &rpc_id,
                    &serde_json::json!({
                        "outcome": {
                            "outcome": "selected",
                            "optionId": r.option_id,
                        }
                    }),
                );
                if outbound.send(encoded).await.is_err() {
                    pending.lock().unwrap().insert(r.request_id.clone(), rpc_id);
                    return Err(DomainError::new(
                        codes::RUNTIME_PROCESS_DIED,
                        "failed to send to process",
                    ));
                }
                return Ok(SendAck { request_id });
            }
        };
        outbound.send(encoded).await.map_err(|_| {
            DomainError::new(codes::RUNTIME_PROCESS_DIED, "failed to send to process")
        })?;

        Ok(SendAck { request_id })
    }

    async fn cancel(&self, session_id: SessionId, _request_id: Option<u64>) {
        let current = self.get_state(&session_id).await;
        match current {
            Some(RuntimeState::Busy) => {
                let (outbound, acp_session_id, pending_permission_rpcs, pending_plan_rpcs) = {
                    let sessions = self.sessions.lock().await;
                    let Some(slot) = sessions.get(&session_id) else {
                        return;
                    };
                    let pending_permission_rpcs =
                        std::mem::take(&mut *slot.pending_permission_requests.lock().unwrap())
                            .into_values()
                            .collect::<Vec<_>>();
                    let pending_plan_rpcs =
                        std::mem::take(&mut *slot.pending_plan_requests.lock().unwrap())
                            .into_values()
                            .collect::<Vec<_>>();
                    (
                        slot.outbound.clone(),
                        slot.acp_session_id.clone(),
                        pending_permission_rpcs,
                        pending_plan_rpcs,
                    )
                };
                if let Some(tx) = outbound {
                    // Agent-to-client requests must always receive a terminal
                    // response before cancelling the outer Prompt; otherwise a
                    // live ACP process can keep the previous turn suspended.
                    for rpc_id in pending_permission_rpcs.into_iter().chain(pending_plan_rpcs) {
                        let encoded = encode_response_result(
                            &rpc_id,
                            &serde_json::json!({ "outcome": { "outcome": "cancelled" } }),
                        );
                        if tx.send(encoded).await.is_err() {
                            break;
                        }
                    }
                    let encoded = encode_notification(
                        "session/cancel",
                        &serde_json::json!({ "sessionId": acp_session_id }),
                    );
                    let _ = tx.send(encoded).await;
                }
                let _ = self
                    .try_transition(&session_id, RuntimeTransition::TurnCompleted)
                    .await;
                let sequence = {
                    let sessions = self.sessions.lock().await;
                    sessions.get(&session_id).map(|slot| {
                        let mut context = slot.interp_ctx.lock().unwrap();
                        context.current_request_id = None;
                        context.accumulated_text.clear();
                        context.suppress_turn_updates = true;
                        let sequence = context.next_sequence;
                        context.next_sequence += 1;
                        sequence
                    })
                };
                if let Some(sequence) = sequence {
                    self.emit_runtime_event(TimestampedEvent {
                        meta: EventMeta::new(session_id.clone(), sequence),
                        event: AgentEvent::TurnCancelled(TurnCancelledPayload::default()),
                    })
                    .await;
                }
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

        if let Some(slot) = self.sessions.lock().await.get_mut(&session_id) {
            slot.shutdown_interrupted_turn = current == Some(RuntimeState::Busy);
            if slot.shutdown_interrupted_turn {
                // Once application shutdown begins, late buffered ACP chunks
                // or the eventual prompt response cannot honestly complete
                // the turn. Preserve already-published deltas and let startup
                // recovery mark the still-running task as interrupted.
                let mut context = slot
                    .interp_ctx
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                context.current_request_id = None;
                context.accumulated_text.clear();
                context.suppress_turn_updates = true;
            }
        }

        let _ = self
            .try_transition(&session_id, RuntimeTransition::BeginShutdown)
            .await;

        DiagLog::info(
            "agent_runtime",
            format!("shutting down session '{}': {}", session_id, reason),
        )
        .emit();

        // Stop the reader first. It owns an outbound sender clone; without
        // this signal, taking SessionSlot::outbound below cannot close stdin
        // and every graceful shutdown waits for the kill timeout.
        let reader_shutdown = {
            let mut sessions = self.sessions.lock().await;
            sessions
                .get_mut(&session_id)
                .and_then(|slot| slot.reader_shutdown.take())
        };
        if let Some(stop_reader) = reader_shutdown {
            let _ = stop_reader.send(());
        }
        let reader_task = {
            let mut sessions = self.sessions.lock().await;
            sessions
                .get_mut(&session_id)
                .and_then(|slot| slot.reader_task.take())
        };
        if let Some(mut reader_task) = reader_task {
            if timeout(Duration::from_secs(1), &mut reader_task)
                .await
                .is_err()
            {
                reader_task.abort();
            }
        }

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

    async fn shutdown_all(&self, reason: &str) {
        let session_ids: Vec<_> = self
            .sessions
            .lock()
            .await
            .iter()
            .filter(|(_, slot)| {
                slot.outbound.is_some() || slot.process.is_some() || slot.reader_task.is_some()
            })
            .map(|(session_id, _)| session_id.clone())
            .collect();
        for session_id in session_ids {
            self.shutdown(session_id, reason).await;
        }
    }

    fn subscribe(&self) -> mpsc::Receiver<TimestampedEvent> {
        let (tx, rx) = mpsc::channel(EVENT_CHANNEL_BUFFER);
        self.event_subscribers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(tx);
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
    acp_session_id: String,
    modes: Vec<ModeDescriptor>,
}

/// Perform the ACP handshake: send `initialize` and wait for the response.
///
/// Reads the initialize response directly from the inbound channel
/// (before the reader task is spawned).  Times out if no response
/// arrives within `timeout_secs`.
fn optional_u32_param(params: &serde_json::Value, name: &str) -> Result<Option<u32>, &'static str> {
    let Some(value) = params.get(name) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    value
        .as_u64()
        .and_then(|value| u32::try_from(value).ok())
        .map(Some)
        .ok_or("Invalid numeric file-read parameter")
}

fn handle_filesystem_request(
    request: &AcpRequest,
    expected_session_id: &str,
    filesystem: &WorkspaceFilesystem,
) -> Option<String> {
    if request.method != "fs/read_text_file" {
        return None;
    }

    let result = (|| {
        let params = request
            .params
            .as_object()
            .ok_or("File-read parameters must be an object")?;
        let session_id = params
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .ok_or("File-read session is missing")?;
        if session_id != expected_session_id {
            return Err("File-read session does not match");
        }
        let path = params
            .get("path")
            .and_then(serde_json::Value::as_str)
            .filter(|path| !path.is_empty())
            .ok_or("File-read path is missing")?;
        let line = optional_u32_param(&request.params, "line")?;
        let limit = optional_u32_param(&request.params, "limit")?;
        filesystem
            .read_text_file(std::path::Path::new(path), line, limit)
            .map(|content| serde_json::json!({ "content": content }))
            .map_err(|error| error.safe_message())
    })();

    Some(match result {
        Ok(result) => encode_response_result(&request.id, &result),
        Err(message) => encode_response_error(
            &request.id,
            &AcpError {
                code: crate::adapters::grok_acp::codec::error_codes::INVALID_PARAMS,
                message: message.into(),
                data: serde_json::Value::Null,
            },
        ),
    })
}

async fn perform_handshake(
    outbound: &mpsc::Sender<String>,
    inbound: &mut mpsc::Receiver<AcpMessage>,
    timeout_secs: u64,
    cwd: &std::path::Path,
) -> Result<HandshakeInfo, String> {
    let init_params = serde_json::json!({
        "protocolVersion": 1,
        "clientCapabilities": {
            "fs": {
                "readTextFile": true,
                "writeTextFile": false
            },
            "terminal": false
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
                        let auth_methods = result
                            .get("authMethods")
                            .and_then(|value| value.as_array())
                            .cloned()
                            .unwrap_or_default();
                        let has_method = |wanted: &str| {
                            auth_methods.iter().any(|method| {
                                method.get("id").and_then(|id| id.as_str()) == Some(wanted)
                            })
                        };
                        let auth_method = if std::env::var_os("XAI_API_KEY").is_some()
                            && has_method("xai.api_key")
                        {
                            "xai.api_key"
                        } else if has_method("cached_token") {
                            "cached_token"
                        } else {
                            return Err(
                                "Grok authentication is required; run `grok login` first".into()
                            );
                        };

                        request_response(
                            outbound,
                            inbound,
                            2,
                            "authenticate",
                            serde_json::json!({
                                "methodId": auth_method,
                                "_meta": { "headless": true },
                            }),
                            deadline,
                        )
                        .await?;
                        let session = request_response(
                            outbound,
                            inbound,
                            3,
                            "session/new",
                            serde_json::json!({
                                "cwd": cwd.to_string_lossy(),
                                "mcpServers": [],
                            }),
                            deadline,
                        )
                        .await?;
                        let acp_session_id = session
                            .get("sessionId")
                            .and_then(|value| value.as_str())
                            .filter(|value| !value.is_empty())
                            .ok_or_else(|| {
                                "session/new response did not include sessionId".to_string()
                            })?
                            .to_string();
                        let modes = session
                            .get("modes")
                            .and_then(|modes| modes.get("availableModes"))
                            .and_then(serde_json::Value::as_array)
                            .into_iter()
                            .flatten()
                            .filter_map(|mode| {
                                let id = mode.get("id")?.as_str()?.to_string();
                                let name = mode
                                    .get("name")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or(&id)
                                    .to_string();
                                Some(ModeDescriptor { id, name })
                            })
                            .collect();

                        return Ok(HandshakeInfo {
                            protocol_version,
                            agent_name,
                            agent_version,
                            acp_session_id,
                            modes,
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

async fn request_response(
    outbound: &mpsc::Sender<String>,
    inbound: &mut mpsc::Receiver<AcpMessage>,
    id: u64,
    method: &str,
    params: serde_json::Value,
    deadline: tokio::time::Instant,
) -> Result<serde_json::Value, String> {
    outbound
        .send(encode_request(id, method, &params))
        .await
        .map_err(|error| format!("failed to send {method}: {error}"))?;

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(format!("{method} timed out"));
        }
        match timeout(remaining, inbound.recv()).await {
            Ok(Some(AcpMessage::Response(response))) if response.id == id => {
                if let Some(error) = response.error {
                    return Err(format!(
                        "{method} returned error: [{}] {}",
                        error.code, error.message
                    ));
                }
                return Ok(response.result.unwrap_or(serde_json::Value::Null));
            }
            Ok(Some(_)) => continue,
            Ok(None) => return Err(format!("process closed stdout while waiting for {method}")),
            Err(_) => return Err(format!("{method} timed out")),
        }
    }
}
