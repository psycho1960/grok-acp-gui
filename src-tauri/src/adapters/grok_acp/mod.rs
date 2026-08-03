//! ADP-GROK-ACP: the production ACP adapter.
//!
//! This module owns the translation between raw JSON-RPC 2.0 frames
//! (produced by the codec) and the internal [`AgentEvent`] vocabulary.
//!
//! The adapter does NOT make business decisions (permission gating,
//! plan enforcement, etc.) — it only normalises protocol messages
//! into stable events.

pub mod codec;
pub mod fake;
pub mod interpreter;
pub mod process;
pub mod transport;

pub use codec::{
    encode_notification, encode_request, encode_response_error, encode_response_result, AcpError,
    AcpMessage, AcpNotification, AcpRequest, AcpResponse, CodecError, FrameDecoder,
};
pub use fake::{FakeAcpTransport, FakeScenario};
pub use interpreter::{interpret, AcpSessionContext, InterpretationResult};
pub use process::GrokAcpAdapter;
pub use transport::{AcpTransport, ProcessExit, TransportError, TransportHandle};
