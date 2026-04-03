use derive_more::From;
use lib_ipc::client::ClientTransportError;
use lib_ipc::envelope::{TaggedDecodeError, TaggedEnvelope};
use lib_ipc::frame::MessageKind;
use lib_ipc::handshake::client::ClientHandshakeError;
use lib_ipc::types::ProtocolVersion;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, From)]
pub enum Error {
    #[from]
    Io(std::io::Error),
    #[from]
    SerdeJson(serde_json::Error),
    Encode(String),
    Decode(String),
    #[from]
    Tagged(TaggedDecodeError),
    TaggedPayload {
        envelope: TaggedEnvelope,
        error: TaggedDecodeError,
    },

    HandshakeClosed,
    HandshakeRejected {
        reason: String,
    },
    ProtocolMismatch {
        expected: ProtocolVersion,
        received: ProtocolVersion,
    },
    UnexpectedMessage {
        expected: MessageKind,
        received: MessageKind,
    },

    NotImplemented(&'static str),
}

impl core::fmt::Display for Error {
    fn fmt(&self, fmt: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io(err) => write!(fmt, "IO error: {err}"),
            Self::SerdeJson(err) => write!(fmt, "Serde JSON error: {err}"),
            Self::Encode(err) => write!(fmt, "transport encode error: {err}"),
            Self::Decode(err) => write!(fmt, "transport decode error: {err}"),
            Self::Tagged(err) => write!(fmt, "Tagged payload error: {err}"),
            Self::TaggedPayload { envelope, error } => write!(fmt, "Tagged payload error {} ({} bytes): {error}", envelope.kind, envelope.payload.len()),
            Self::HandshakeClosed => write!(fmt, "updater closed the connection during handshake"),
            Self::HandshakeRejected { reason } => write!(fmt, "updater handshake rejected: {reason}"),
            Self::ProtocolMismatch { expected, received } => write!(fmt, "protocol mismatch (expected {expected}, received {received})"),
            Self::UnexpectedMessage { expected, received } => write!(fmt, "unexpected message kind (expected {expected:?}, received {received:?})"),
            Self::NotImplemented(scope) => write!(fmt, "not implemented: {scope}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<ClientHandshakeError> for Error {
    fn from(err: ClientHandshakeError) -> Self {
        match err {
            ClientHandshakeError::Io(err) => Self::Io(err),
            ClientHandshakeError::Encode(err) => Self::Encode(err.to_string()),
            ClientHandshakeError::Decode(err) => Self::Decode(err.to_string()),
            ClientHandshakeError::Closed => Self::HandshakeClosed,
            ClientHandshakeError::UnexpectedMessage { expected, received } => Self::UnexpectedMessage { expected, received },
            ClientHandshakeError::Rejected(reject) => Self::HandshakeRejected { reason: reject.reason },
            ClientHandshakeError::ProtocolMismatch { expected, received } => Self::ProtocolMismatch { expected, received },
        }
    }
}

impl From<ClientTransportError> for Error {
    fn from(err: ClientTransportError) -> Self {
        match err {
            ClientTransportError::Io(err) => Self::Io(err),
            ClientTransportError::Encode(err) => Self::Encode(err.to_string()),
            ClientTransportError::Decode(err) => Self::Decode(err.to_string()),
            ClientTransportError::Tagged(err) => Self::Tagged(err),
            ClientTransportError::TaggedPayload { envelope, error } => Self::TaggedPayload { envelope, error },
            ClientTransportError::UnexpectedMessage { expected, received } => Self::UnexpectedMessage { expected, received },
        }
    }
}
