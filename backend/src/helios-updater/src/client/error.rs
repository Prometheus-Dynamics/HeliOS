use derive_more::From;
use lib_ipc::client::ClientHandshakeError;
use lib_ipc::client::ClientTransportError;
use lib_ipc::types::ProtocolVersion;
use lib_ipc::wire::{ServiceKind, StreamKind};

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, From)]
pub enum Error {
    #[from]
    Io(std::io::Error),
    #[from]
    SerdeJson(serde_json::Error),
    Encode(String),
    Decode(String),

    HandshakeClosed,
    HandshakeRejected {
        reason: String,
    },
    ProtocolMismatch {
        expected: ProtocolVersion,
        received: ProtocolVersion,
    },
    UnexpectedStream {
        expected: StreamKind,
        received: StreamKind,
    },
    WrongService {
        expected: ServiceKind,
        received: ServiceKind,
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
            Self::HandshakeClosed => write!(fmt, "updater closed the connection during handshake"),
            Self::HandshakeRejected { reason } => write!(fmt, "updater handshake rejected: {reason}"),
            Self::ProtocolMismatch { expected, received } => write!(fmt, "protocol mismatch (expected {expected}, received {received})"),
            Self::UnexpectedStream { expected, received } => write!(fmt, "unexpected stream kind (expected {expected:?}, received {received:?})"),
            Self::WrongService { expected, received } => write!(fmt, "unexpected service kind (expected {expected:?}, received {received:?})"),
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
            ClientHandshakeError::UnexpectedStream { expected, received } => Self::UnexpectedStream { expected, received },
            ClientHandshakeError::WrongService { expected, received } => Self::WrongService { expected, received },
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
            ClientTransportError::UnexpectedStream { expected, received } => Self::UnexpectedStream { expected, received },
            ClientTransportError::WrongService { expected, received } => Self::WrongService { expected, received },
        }
    }
}
