use derive_more::From;
use lib_ai::AiError;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, From)]
pub enum Error {
    /// Provided configuration values are invalid.
    InvalidConfig(String),
    /// The service is in an invalid state for the requested operation.
    InvalidState(String),
    /// The requested operation is not currently supported.
    Unsupported(String),
    /// Wrapper around lower-level IO errors.
    #[from]
    Io(std::io::Error),
    /// Wrapper around AI runtime failures.
    #[from]
    Ai(AiError),
    /// Wrapper around JoinError produced by background tasks.
    #[from]
    Join(tokio::task::JoinError),
}

impl core::fmt::Display for Error {
    fn fmt(&self, fmt: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(fmt, "{self:?}")
    }
}

impl std::error::Error for Error {}

impl From<lib_ipc::server::ServerHandshakeError> for Error {
    fn from(error: lib_ipc::server::ServerHandshakeError) -> Self {
        match error {
            lib_ipc::server::ServerHandshakeError::Io(err) => Self::Io(err),
            lib_ipc::server::ServerHandshakeError::BincodeEncode(err) => Self::InvalidState(format!("handshake serialization error: {err}")),
            lib_ipc::server::ServerHandshakeError::BincodeDecode(err) => Self::InvalidState(format!("handshake deserialization error: {err}")),
            lib_ipc::server::ServerHandshakeError::Closed => Self::InvalidState("handshake stream closed".into()),
            lib_ipc::server::ServerHandshakeError::UnexpectedMessage { expected, received } => {
                Self::InvalidState(format!("unexpected handshake message kind (expected {expected:?}, received {received:?})"))
            }
        }
    }
}

impl From<lib_ipc::server::ServerTransportError> for Error {
    fn from(error: lib_ipc::server::ServerTransportError) -> Self {
        match error {
            lib_ipc::server::ServerTransportError::Io(err) => Self::Io(err),
            lib_ipc::server::ServerTransportError::BincodeEncode(err) => Self::InvalidState(format!("transport serialization error: {err}")),
            lib_ipc::server::ServerTransportError::BincodeDecode(err) => Self::InvalidState(format!("transport deserialization error: {err}")),
            lib_ipc::server::ServerTransportError::Tagged(err) => Self::InvalidState(format!("transport tagged payload error: {err}")),
            lib_ipc::server::ServerTransportError::MissingControlPayload => Self::InvalidState("missing control payload for control message".into()),
            lib_ipc::server::ServerTransportError::InvalidEventKind(kind) => Self::InvalidState(format!("invalid outgoing event kind: {kind:?}")),
        }
    }
}

impl From<lib_ipc::server::ServerLoopError<Error>> for Error {
    fn from(error: lib_ipc::server::ServerLoopError<Error>) -> Self {
        match error {
            lib_ipc::server::ServerLoopError::Handshake(err) => err.into(),
            lib_ipc::server::ServerLoopError::Transport(err) => err.into(),
            lib_ipc::server::ServerLoopError::Handler(err) => err,
        }
    }
}

impl From<lib_sensors::Error> for Error {
    fn from(error: lib_sensors::Error) -> Self {
        Self::InvalidState(error.to_string())
    }
}

impl lib_ipc::server::RetryableError for Error {
    fn retryable(&self) -> bool {
        matches!(self, Error::Io(_))
    }
}
