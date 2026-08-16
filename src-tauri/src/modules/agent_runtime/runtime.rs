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
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::time::{timeout, Duration};

use crate::adapters::filesystem::WorkspaceFilesystem;
use crate::adapters::grok_acp::codec::{
    encode_notification, encode_request, encode_response_error, encode_response_result, AcpError,
    AcpMessage, AcpRequest,
};
use crate::adapters::grok_acp::interpreter::{self, AcpSessionContext, InterpretationResult};
use crate::adapters::grok_acp::transport::{
    AcpTransport, LoginHandle, LoginMethod, LoginProcessState, ProcessExit, TransportError,
    TransportHandle,
};
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
    config::{
        RuntimeConfig, RuntimeHandle, RuntimeLoginMethod, RuntimeLoginResult, RuntimeProbeResult,
        WorkspaceContext,
    },
    AgentRuntime,
};

/// Maximum event channel buffer size per subscriber.
const EVENT_CHANNEL_BUFFER: usize = 1024;

/// Maximum time to wait for a clean process exit before killing.
const KILL_GRACE_SECS: u64 = 5;

/// A mode must be confirmed before its Prompt is allowed to start.
const MODE_CHANGE_TIMEOUT_SECS: u64 = 10;

#[derive(Clone)]
struct ConfigOptionBinding {
    id: String,
    category: String,
    values: Vec<String>,
}

#[derive(Clone)]
enum SessionConfigChange {
    Standard { id: String, value: String },
    LegacyModel { value: String },
}

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
    /// Client `session/set_mode` request ids awaiting the ACP response.
    pending_mode_change_responses: Arc<StdMutex<HashMap<u64, oneshot::Sender<bool>>>>,
    /// Client `session/set_config_option` request ids awaiting ACP confirmation.
    pending_config_change_responses: Arc<StdMutex<HashMap<u64, oneshot::Sender<bool>>>>,
    /// Select options advertised by ACP in the `session/new` response.
    config_options: Vec<ConfigOptionBinding>,
    /// Model profile used to start the Grok process, updated after a
    /// successful legacy `session/set_model` request.
    active_model: Option<String>,
    /// Reasoning supplied by the active local model profile when known.
    active_reasoning: Option<String>,
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
    fn new(
        executable_path: String,
        active_model: Option<String>,
        active_reasoning: Option<String>,
    ) -> Self {
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
            pending_mode_change_responses: Arc::new(StdMutex::new(HashMap::new())),
            pending_config_change_responses: Arc::new(StdMutex::new(HashMap::new())),
            config_options: Vec::new(),
            active_model,
            active_reasoning,
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
    login: Mutex<Option<LoginHandle>>,
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
            login: Mutex::new(None),
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

fn login_result(state: LoginProcessState) -> RuntimeLoginResult {
    match state {
        LoginProcessState::Running => RuntimeLoginResult {
            status: "running".into(),
            exit_code: None,
            message: Some("Grok 登录进程已启动，正在等待官方登录完成。".into()),
            retryable: false,
        },
        LoginProcessState::Succeeded => RuntimeLoginResult {
            status: "succeeded".into(),
            exit_code: Some(0),
            message: Some("Grok login completed. Authentication will be checked again.".into()),
            retryable: false,
        },
        LoginProcessState::Cancelled => RuntimeLoginResult {
            status: "cancelled".into(),
            exit_code: None,
            message: Some("Grok login was cancelled.".into()),
            retryable: true,
        },
        LoginProcessState::TimedOut => RuntimeLoginResult {
            status: "timed_out".into(),
            exit_code: None,
            message: Some("Grok login timed out. Start the login flow again.".into()),
            retryable: true,
        },
        LoginProcessState::Failed { exit_code } => RuntimeLoginResult {
            status: "failed".into(),
            exit_code,
            message: Some("Grok login exited unexpectedly. Try again.".into()),
            retryable: true,
        },
    }
}

fn safe_login_error(error: &TransportError) -> String {
    match error {
        TransportError::NotFound { .. } => {
            "Grok CLI was not found. Install Grok Build and try again.".into()
        }
        TransportError::VersionTooLow { found, required } => {
            format!("Grok {found} is below the required version {required}.")
        }
        TransportError::NotAuthenticated => {
            "Grok is not authenticated. Start the official login flow again.".into()
        }
        TransportError::AuthenticationServiceUnavailable => {
            "无法连接 Grok 认证服务（默认域名 auth.x.ai）。请检查网络或代理是否允许访问该域名，然后点击“立即修复”重试。".into()
        }
        TransportError::SpawnFailed { .. } | TransportError::ProbeError { .. } => {
            "无法启动 Grok 官方登录流程，请稍后重试。".into()
        }
    }
}

#[async_trait]
impl<T: AcpTransport + 'static> AgentRuntime for AgentRuntimeImpl<T> {
    async fn probe(&self, config: &RuntimeConfig) -> RuntimeProbeResult {
        // Delegate to the transport's probe method.
        match self.transport.probe(config).await {
            Ok((path, version)) => {
                let mut result = RuntimeProbeResult::ready(path, version, true);
                // Executable/version probing alone cannot prove that a cached
                // credential is accepted by the service. runtime.refresh uses
                // a structured minimal ACP Turn for that decision.
                result.authenticated = None;
                result
            }
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

    async fn login(
        &self,
        config: &RuntimeConfig,
        method: RuntimeLoginMethod,
    ) -> RuntimeLoginResult {
        let current = self.login_status().await;
        if current.status == "running" {
            return current;
        }

        if let Err(error) = self.transport.probe(config).await {
            return RuntimeLoginResult {
                status: "failed".into(),
                exit_code: None,
                message: Some(safe_login_error(&error)),
                retryable: true,
            };
        }
        let method = match method {
            RuntimeLoginMethod::Oauth => LoginMethod::Oauth,
            RuntimeLoginMethod::DeviceAuth => LoginMethod::DeviceAuth,
        };
        match self
            .transport
            .start_login(method, config.login_timeout_secs)
            .await
        {
            Ok(handle) => {
                *self.login.lock().await = Some(handle);
                RuntimeLoginResult {
                    status: "running".into(),
                    exit_code: None,
                    message: Some("Grok 登录进程已启动，正在等待官方登录完成。".into()),
                    retryable: false,
                }
            }
            Err(error) => RuntimeLoginResult {
                status: "failed".into(),
                exit_code: None,
                message: Some(safe_login_error(&error)),
                retryable: true,
            },
        }
    }

    async fn login_status(&self) -> RuntimeLoginResult {
        let login = self.login.lock().await;
        login
            .as_ref()
            .map(|handle| login_result(handle.status()))
            .unwrap_or_else(RuntimeLoginResult::idle)
    }

    async fn cancel_login(&self) -> RuntimeLoginResult {
        let login = self.login.lock().await;
        let Some(handle) = login.as_ref() else {
            return RuntimeLoginResult::idle();
        };
        if handle.status() == LoginProcessState::Running {
            handle.cancel();
            RuntimeLoginResult {
                status: "running".into(),
                exit_code: None,
                message: Some("Cancelling Grok login…".into()),
                retryable: false,
            }
        } else {
            login_result(handle.status())
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

        // Fail fast with a precise error when the selected model profile needs
        // an env-var API key that this process tree does not have. Without this
        // check the grok child would start fine but fail every turn with a
        // misleading "authentication required" error.
        if let Some(model) = config.model.as_deref() {
            if let Some(message) = super::config::missing_model_env_key(
                model,
                &super::config::configured_models(),
                |key| std::env::var_os(key).is_some_and(|value| !value.is_empty()),
            ) {
                return Err(DomainError::new(
                    crate::domain::error::codes::RUNTIME_MODEL_ENV_MISSING,
                    message,
                ));
            }
        }

        // Create or reset the session slot.
        {
            let active_reasoning = config.model.as_deref().and_then(|model| {
                super::config::configured_models()
                    .into_iter()
                    .find(|profile| profile.id == model)
                    .and_then(|profile| profile.reasoning_effort)
            });
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
                    config.model.clone(),
                    active_reasoning,
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
        let configured_models = super::config::configured_models();
        let selected_model_env_key =
            super::config::selected_model_env_key(config.model.as_deref(), &configured_models);
        match perform_handshake(
            &outbound,
            &mut inbound,
            config.handshake_timeout_secs,
            &workspace_cwd,
            selected_model_env_key,
        )
        .await
        {
            Ok(handshake_info) => {
                self.try_transition(&session_id, RuntimeTransition::HandshakeComplete)
                    .await?;

                let reader_acp_session_id = handshake_info.acp_session_id.clone();
                let mut handshake_messages = handshake_info.pending_messages;
                let (available_mode_ids, visible_modes) =
                    normalize_grok_session_modes(handshake_info.modes);

                // Emit session_ready event.
                let event = TimestampedEvent {
                    meta: EventMeta::new(session_id.clone(), 1),
                    event: AgentEvent::SessionReady(SessionReadyPayload {
                        protocol_version: handshake_info.protocol_version,
                        agent_name: handshake_info.agent_name,
                        agent_version: handshake_info.agent_version,
                        models: vec![],
                        modes: visible_modes,
                    }),
                };
                self.emit_runtime_event(event).await;

                // NOW spawn the inbound reader task for subsequent messages.
                let (
                    interp_ctx,
                    pending_permission_requests,
                    pending_plan_requests,
                    pending_mode_change_responses,
                    pending_config_change_responses,
                ) = {
                    let mut sessions = self.sessions.lock().await;
                    let slot = sessions.get_mut(&session_id).unwrap();
                    slot.acp_session_id = Some(handshake_info.acp_session_id);
                    slot.available_mode_ids = available_mode_ids;
                    slot.config_options = handshake_info.config_options;
                    (
                        slot.interp_ctx.clone(),
                        slot.pending_permission_requests.clone(),
                        slot.pending_plan_requests.clone(),
                        slot.pending_mode_change_responses.clone(),
                        slot.pending_config_change_responses.clone(),
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
                        let msg = match handshake_messages.pop_front() {
                            Some(message) => message,
                            None => tokio::select! {
                                _ = &mut reader_shutdown_rx => break,
                                message = inbound.recv() => match message {
                                    Some(message) => message,
                                    None => break,
                                },
                            },
                        };
                        if let AcpMessage::Response(response) = &msg {
                            if let Some(request_id) = response.id.as_u64() {
                                if let Some(waiter) = pending_mode_change_responses
                                    .lock()
                                    .unwrap()
                                    .remove(&request_id)
                                {
                                    let _ = waiter.send(response.error.is_none());
                                    continue;
                                }
                                if let Some(waiter) = pending_config_change_responses
                                    .lock()
                                    .unwrap()
                                    .remove(&request_id)
                                {
                                    let _ = waiter.send(response.error.is_none());
                                    continue;
                                }
                            }
                        }
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
                    pending_mode_change_responses.lock().unwrap().clear();
                    pending_config_change_responses.lock().unwrap().clear();
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
                    Some(
                        resolve_grok_mode_id(requested_mode, &advertised_modes).ok_or_else(
                            || {
                                DomainError::new(
                                    codes::ACP_REQUEST_FAILED,
                                    "requested session mode is not supported by Grok ACP",
                                )
                            },
                        )?,
                    )
                }
                None => None,
            },
            _ => None,
        };

        let config_to_set = match &request {
            ClientRequest::Prompt(prompt) => {
                let (advertised, active_model, active_reasoning) = self
                    .sessions
                    .lock()
                    .await
                    .get(&session_id)
                    .map(|slot| {
                        (
                            slot.config_options.clone(),
                            slot.active_model.clone(),
                            slot.active_reasoning.clone(),
                        )
                    })
                    .ok_or_else(|| {
                        DomainError::new(codes::DOMAIN_TASK_NOT_FOUND, "session not found")
                    })?;
                let mut changes = Vec::new();

                if let Some(model) = prompt.model.as_deref() {
                    if let Some(option) =
                        advertised.iter().find(|option| option.category == "model")
                    {
                        if !option.values.iter().any(|value| value == model) {
                            return Err(DomainError::new(
                                codes::ACP_REQUEST_FAILED,
                                "requested model value is not advertised by ACP",
                            ));
                        }
                        changes.push(SessionConfigChange::Standard {
                            id: option.id.clone(),
                            value: model.to_string(),
                        });
                    } else if active_model.as_deref() != Some(model) {
                        // Grok builds based on ACP 0.10.x expose the legacy
                        // model request but do not return configOptions.
                        changes.push(SessionConfigChange::LegacyModel {
                            value: model.to_string(),
                        });
                    }
                }

                if let Some(reasoning) = prompt.reasoning.as_deref() {
                    if let Some(option) = advertised
                        .iter()
                        .find(|option| option.category == "thought_level")
                    {
                        if !option.values.iter().any(|value| value == reasoning) {
                            return Err(DomainError::new(
                                codes::ACP_REQUEST_FAILED,
                                "requested thought_level value is not advertised by ACP",
                            ));
                        }
                        changes.push(SessionConfigChange::Standard {
                            id: option.id.clone(),
                            value: reasoning.to_string(),
                        });
                    } else if active_model.as_deref() == prompt.model.as_deref()
                        && active_reasoning
                            .as_deref()
                            .is_some_and(|value| value != reasoning)
                    {
                        return Err(DomainError::new(
                            codes::ACP_REQUEST_FAILED,
                            "this Grok version only supports the reasoning configured by the selected model profile",
                        ));
                    }
                    // With legacy Grok the selected local model profile owns
                    // reasoning. Matching (or unknown) profile values require
                    // no extra request and must not block the first Turn.
                }
                changes
            }
            _ => Vec::new(),
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
        let (request_id, mode_request_id, mode_change_response, config_changes, acp_session_id) = {
            let mut sessions = self.sessions.lock().await;
            let slot = sessions.get_mut(&session_id).unwrap();
            let mode_request_id = mode_to_set.as_ref().map(|_| {
                slot.next_request_id += 1;
                slot.next_request_id
            });
            let mode_change_response = mode_request_id.map(|mode_request_id| {
                let (sender, receiver) = oneshot::channel();
                slot.pending_mode_change_responses
                    .lock()
                    .unwrap()
                    .insert(mode_request_id, sender);
                receiver
            });
            let config_changes = config_to_set
                .iter()
                .map(|change| {
                    slot.next_request_id += 1;
                    let config_request_id = slot.next_request_id;
                    let (sender, receiver) = oneshot::channel();
                    slot.pending_config_change_responses
                        .lock()
                        .unwrap()
                        .insert(config_request_id, sender);
                    (config_request_id, change.clone(), receiver)
                })
                .collect::<Vec<_>>();
            slot.next_request_id += 1;
            let request_id = slot.next_request_id;
            let acp_session_id = slot.acp_session_id.clone().ok_or_else(|| {
                DomainError::new(codes::ACP_HANDSHAKE_FAILED, "ACP session id is missing")
            })?;
            (
                request_id,
                mode_request_id,
                mode_change_response,
                config_changes,
                acp_session_id,
            )
        };

        if let (Some(mode_request_id), Some(mode_id), Some(mode_change_response)) = (
            mode_request_id,
            mode_to_set.as_deref(),
            mode_change_response,
        ) {
            let encoded = encode_request(
                mode_request_id,
                "session/set_mode",
                &serde_json::json!({
                    "sessionId": acp_session_id,
                    "modeId": mode_id,
                }),
            );
            if outbound.send(encoded).await.is_err() {
                if let Some(slot) = self.sessions.lock().await.get(&session_id) {
                    slot.pending_mode_change_responses
                        .lock()
                        .unwrap()
                        .remove(&mode_request_id);
                }
                return Err(DomainError::new(
                    codes::RUNTIME_PROCESS_DIED,
                    "failed to send to process",
                ));
            }

            let mode_change_result = timeout(
                Duration::from_secs(MODE_CHANGE_TIMEOUT_SECS),
                mode_change_response,
            )
            .await;
            if let Some(slot) = self.sessions.lock().await.get(&session_id) {
                slot.pending_mode_change_responses
                    .lock()
                    .unwrap()
                    .remove(&mode_request_id);
            }
            let mode_change_succeeded = mode_change_result
                .map_err(|_| {
                    DomainError::new(
                        codes::ACP_REQUEST_FAILED,
                        "ACP did not confirm the requested session mode",
                    )
                })?
                .map_err(|_| {
                    DomainError::new(
                        codes::RUNTIME_PROCESS_DIED,
                        "ACP closed before confirming the requested session mode",
                    )
                })?;
            if !mode_change_succeeded {
                return Err(DomainError::new(
                    codes::ACP_REQUEST_FAILED,
                    "ACP rejected the requested session mode",
                ));
            }
        }

        for (config_request_id, change, config_change_response) in config_changes {
            let (method, params) = match &change {
                SessionConfigChange::Standard { id, value } => (
                    "session/set_config_option",
                    serde_json::json!({
                        "sessionId": acp_session_id.as_str(),
                        "configId": id,
                        "value": value,
                    }),
                ),
                SessionConfigChange::LegacyModel { value } => (
                    "session/set_model",
                    serde_json::json!({
                        "sessionId": acp_session_id.as_str(),
                        "modelId": value,
                    }),
                ),
            };
            let encoded = encode_request(config_request_id, method, &params);
            if outbound.send(encoded).await.is_err() {
                if let Some(slot) = self.sessions.lock().await.get(&session_id) {
                    slot.pending_config_change_responses
                        .lock()
                        .unwrap()
                        .remove(&config_request_id);
                }
                return Err(DomainError::new(
                    codes::RUNTIME_PROCESS_DIED,
                    "failed to send configuration to process",
                ));
            }

            let config_change_result = timeout(
                Duration::from_secs(MODE_CHANGE_TIMEOUT_SECS),
                config_change_response,
            )
            .await;
            if let Some(slot) = self.sessions.lock().await.get(&session_id) {
                slot.pending_config_change_responses
                    .lock()
                    .unwrap()
                    .remove(&config_request_id);
            }
            let config_change_succeeded = config_change_result
                .map_err(|_| {
                    DomainError::new(
                        codes::ACP_REQUEST_FAILED,
                        "ACP did not confirm the requested session configuration",
                    )
                })?
                .map_err(|_| {
                    DomainError::new(
                        codes::RUNTIME_PROCESS_DIED,
                        "ACP closed before confirming the requested session configuration",
                    )
                })?;
            if !config_change_succeeded {
                return Err(DomainError::new(
                    codes::ACP_REQUEST_FAILED,
                    "ACP rejected the requested session configuration",
                ));
            }

            if let SessionConfigChange::LegacyModel { value } = change {
                let active_reasoning = super::config::configured_models()
                    .into_iter()
                    .find(|profile| profile.id == value)
                    .and_then(|profile| profile.reasoning_effort);
                if let Some(slot) = self.sessions.lock().await.get_mut(&session_id) {
                    slot.active_model = Some(value);
                    slot.active_reasoning = active_reasoning;
                }
            }
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
                let mut prompt = vec![serde_json::json!({
                    "type": "text",
                    "text": p.message,
                })];
                prompt.extend(p.attachments.iter().map(|image| {
                    serde_json::json!({
                        "type": "image",
                        "mimeType": image.mime_type,
                        "data": image.base64_data,
                    })
                }));
                let params = serde_json::json!({
                    "sessionId": acp_session_id,
                    "prompt": prompt,
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
        let mut login = self.login.lock().await;
        if let Some(handle) = login.as_mut() {
            if handle.status() == LoginProcessState::Running {
                handle.cancel();
                let _ = timeout(Duration::from_secs(5), handle.wait_for_change()).await;
            }
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
    config_options: Vec<ConfigOptionBinding>,
    pending_messages: VecDeque<AcpMessage>,
}

const GROK_COMPAT_MODE_IDS: [&str; 3] = ["default", "plan", "ask"];

/// Grok 1.0.4 accepts the legacy session modes but omits `modes` from
/// `session/new`. Keep product modes available while preserving the exact
/// advertised IDs when newer/older builds do publish them.
fn normalize_grok_session_modes(
    advertised: Vec<ModeDescriptor>,
) -> (Vec<String>, Vec<ModeDescriptor>) {
    let wire_ids: Vec<String> = if advertised.is_empty() {
        GROK_COMPAT_MODE_IDS
            .iter()
            .map(|mode| (*mode).to_string())
            .collect()
    } else {
        advertised.iter().map(|mode| mode.id.clone()).collect()
    };
    let supports = |ids: &[&str]| wire_ids.iter().any(|mode| ids.contains(&mode.as_str()));
    let mut visible = Vec::new();
    if supports(&["default", "agent", "auto"]) {
        visible.push(ModeDescriptor {
            id: "agent".into(),
            name: "智能体".into(),
        });
    }
    if supports(&["plan"]) {
        visible.push(ModeDescriptor {
            id: "plan".into(),
            name: "计划".into(),
        });
    }
    if supports(&["ask"]) {
        visible.push(ModeDescriptor {
            id: "ask".into(),
            name: "问答".into(),
        });
    }
    (wire_ids, visible)
}

fn resolve_grok_mode_id(requested: &str, supported: &[String]) -> Option<String> {
    let candidates: &[&str] = match requested {
        "agent" => &["default", "agent", "auto"],
        "plan" => &["plan"],
        "ask" => &["ask"],
        other => &[other],
    };
    candidates
        .iter()
        .find(|candidate| supported.iter().any(|mode| mode == **candidate))
        .map(|mode| (*mode).to_string())
}

#[cfg(test)]
mod mode_compatibility_tests {
    use super::*;

    #[test]
    fn missing_grok_mode_advertisement_uses_verified_legacy_ids() {
        let (wire_ids, visible) = normalize_grok_session_modes(vec![]);
        assert_eq!(wire_ids, vec!["default", "plan", "ask"]);
        assert_eq!(
            visible
                .iter()
                .map(|mode| mode.id.as_str())
                .collect::<Vec<_>>(),
            vec!["agent", "plan", "ask"]
        );
        assert_eq!(
            resolve_grok_mode_id("agent", &wire_ids).as_deref(),
            Some("default")
        );
        assert_eq!(
            resolve_grok_mode_id("plan", &wire_ids).as_deref(),
            Some("plan")
        );
        assert_eq!(
            resolve_grok_mode_id("ask", &wire_ids).as_deref(),
            Some("ask")
        );
    }

    #[test]
    fn advertised_modes_remain_authoritative() {
        let advertised = vec![
            ModeDescriptor {
                id: "auto".into(),
                name: "Auto".into(),
            },
            ModeDescriptor {
                id: "plan".into(),
                name: "Plan".into(),
            },
            ModeDescriptor {
                id: "ask".into(),
                name: "Ask".into(),
            },
        ];
        let (wire_ids, visible) = normalize_grok_session_modes(advertised);
        assert_eq!(
            resolve_grok_mode_id("agent", &wire_ids).as_deref(),
            Some("auto")
        );
        assert_eq!(visible.len(), 3);
        assert_eq!(resolve_grok_mode_id("unsupported", &wire_ids), None);
    }
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
    selected_model_env_key: Option<&str>,
) -> Result<HandshakeInfo, String> {
    let mut pending_messages = VecDeque::new();
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
                        let auth_method = if selected_model_env_key == Some("XAI_API_KEY")
                            && std::env::var_os("XAI_API_KEY").is_some()
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
                            &mut pending_messages,
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
                            &mut pending_messages,
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
                        let config_options = session
                            .get("configOptions")
                            .and_then(serde_json::Value::as_array)
                            .into_iter()
                            .flatten()
                            .filter_map(|option| {
                                if option.get("type").and_then(serde_json::Value::as_str)
                                    != Some("select")
                                {
                                    return None;
                                }
                                let id = option.get("id")?.as_str()?.to_string();
                                let category = option.get("category")?.as_str()?.to_string();
                                let values = option
                                    .get("options")
                                    .and_then(serde_json::Value::as_array)
                                    .into_iter()
                                    .flatten()
                                    .filter_map(|value| {
                                        value
                                            .get("value")
                                            .and_then(serde_json::Value::as_str)
                                            .map(str::to_string)
                                    })
                                    .collect();
                                Some(ConfigOptionBinding {
                                    id,
                                    category,
                                    values,
                                })
                            })
                            .collect();

                        return Ok(HandshakeInfo {
                            protocol_version,
                            agent_name,
                            agent_version,
                            acp_session_id,
                            modes,
                            config_options,
                            pending_messages,
                        });
                    }
                    AcpMessage::Unknown(_) => {
                        // Non-JSON line (e.g. blank line) — skip and continue.
                        continue;
                    }
                    message @ (AcpMessage::Request(_) | AcpMessage::Notification(_)) => {
                        // The agent sent a request/notification before the
                        // initialize response. Preserve it for the reader so
                        // early capability updates are not lost.
                        pending_messages.push_back(message);
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
    pending_messages: &mut VecDeque<AcpMessage>,
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
            Ok(Some(message)) => {
                pending_messages.push_back(message);
                continue;
            }
            Ok(None) => return Err(format!("process closed stdout while waiting for {method}")),
            Err(_) => return Err(format!("{method} timed out")),
        }
    }
}

#[cfg(test)]
mod handshake_message_tests {
    use super::*;
    use crate::adapters::grok_acp::codec::{AcpNotification, AcpResponse};

    #[tokio::test]
    async fn request_response_preserves_notification_received_before_response() {
        let (outbound, mut outbound_rx) = mpsc::channel::<String>(1);
        let (inbound_tx, mut inbound) = mpsc::channel::<AcpMessage>(2);
        let producer = tokio::spawn(async move {
            let request = outbound_rx.recv().await.expect("request frame");
            assert!(request.contains("session/new"));
            inbound_tx
                .send(AcpMessage::Notification(AcpNotification {
                    jsonrpc: "2.0".into(),
                    method: "session/update".into(),
                    params: serde_json::json!({
                        "sessionId": "acp-session",
                        "update": {
                            "sessionUpdate": "available_commands_update",
                            "availableCommands": [{
                                "name": "session",
                                "description": "Manage the current session"
                            }]
                        }
                    }),
                }))
                .await
                .expect("notification");
            inbound_tx
                .send(AcpMessage::Response(AcpResponse {
                    jsonrpc: "2.0".into(),
                    id: serde_json::json!(3),
                    result: Some(serde_json::json!({"sessionId": "acp-session"})),
                    error: None,
                }))
                .await
                .expect("response");
        });
        let mut pending_messages = VecDeque::new();

        let result = request_response(
            &outbound,
            &mut inbound,
            &mut pending_messages,
            3,
            "session/new",
            serde_json::json!({"cwd": "C:/workspace", "mcpServers": []}),
            tokio::time::Instant::now() + Duration::from_secs(1),
        )
        .await
        .expect("session response");
        producer.await.expect("producer");

        assert_eq!(result["sessionId"], "acp-session");
        assert!(matches!(
            pending_messages.front(),
            Some(AcpMessage::Notification(notification))
                if notification.method == "session/update"
        ));
    }
}

#[cfg(test)]
mod login_error_tests {
    use super::*;

    #[test]
    fn renderer_login_errors_do_not_echo_paths_or_process_details() {
        let not_found = safe_login_error(&TransportError::NotFound {
            searched: vec![std::path::PathBuf::from(r"C:\private\profile\grok.exe")],
        });
        let spawn_failed = safe_login_error(&TransportError::SpawnFailed {
            message: "XAI_API_KEY=synthetic-secret command line details".into(),
        });

        assert!(!not_found.contains("private"));
        assert!(!spawn_failed.contains("XAI_API_KEY"));
        assert!(!spawn_failed.contains("synthetic-secret"));
    }

    #[test]
    fn authentication_service_error_is_actionable_without_claiming_browser_opened() {
        let message = safe_login_error(&TransportError::AuthenticationServiceUnavailable);

        assert!(message.contains("auth.x.ai"));
        assert!(message.contains("网络或代理"));
        assert!(!message.contains("浏览器"));
        assert!(!message.contains("复制"));
    }
}
