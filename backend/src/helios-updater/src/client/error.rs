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
    #[from]
    BincodeEncode(bincode::error::EncodeError),
    #[from]
    BincodeDecode(bincode::error::DecodeError),
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
            Self::BincodeEncode(err) => write!(fmt, "Bincode encode error: {err}"),
            Self::BincodeDecode(err) => write!(fmt, "Bincode decode error: {err}"),
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
            ClientHandshakeError::BincodeEncode(err) => Self::BincodeEncode(err),
            ClientHandshakeError::BincodeDecode(err) => Self::BincodeDecode(err),
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
            ClientTransportError::BincodeEncode(err) => Self::BincodeEncode(err),
            ClientTransportError::BincodeDecode(err) => Self::BincodeDecode(err),
            ClientTransportError::Tagged(err) => Self::Tagged(err),
            ClientTransportError::TaggedPayload { envelope, error } => Self::TaggedPayload { envelope, error },
            ClientTransportError::UnexpectedMessage { expected, received } => Self::UnexpectedMessage { expected, received },
        }
    }
}
