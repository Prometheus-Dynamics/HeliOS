use derive_more::From;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug, From)]
pub enum Error {
    InvalidState(String),
    ArtifactVerificationFailed(String),
    Manifest(String),

    #[cfg(feature = "updater-ipc")]
    #[from]
    UpdaterClient(crate::client::Error),

    #[from]
    Io(std::io::Error),

    #[from]
    Http(reqwest::Error),

    #[from]
    SerdeJson(serde_json::Error),

    #[from]
    Base64(base64::DecodeError),

    #[from]
    Signature(ed25519_dalek::SignatureError),

    /// Placeholder error until the updater workflow is implemented.
    NotImplemented(&'static str),
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
            lib_ipc::server::ServerHandshakeError::Encode(err) => Self::InvalidState(format!("handshake serialization error: {err}")),
            lib_ipc::server::ServerHandshakeError::Decode(err) => Self::InvalidState(format!("handshake deserialization error: {err}")),
            lib_ipc::server::ServerHandshakeError::Closed => Self::InvalidState("handshake stream closed".into()),
            lib_ipc::server::ServerHandshakeError::UnexpectedStream { expected, received } => {
                Self::InvalidState(format!("unexpected handshake stream kind (expected {expected:?}, received {received:?})"))
            }
            lib_ipc::server::ServerHandshakeError::WrongService { expected, received } => {
                Self::InvalidState(format!("unexpected handshake service kind (expected {expected:?}, received {received:?})"))
            }
        }
    }
}

impl From<lib_ipc::server::ServerTransportError> for Error {
    fn from(error: lib_ipc::server::ServerTransportError) -> Self {
        match error {
            lib_ipc::server::ServerTransportError::Io(err) => Self::Io(err),
            lib_ipc::server::ServerTransportError::Encode(err) => Self::InvalidState(format!("transport serialization error: {err}")),
            lib_ipc::server::ServerTransportError::Decode(err) => Self::InvalidState(format!("transport deserialization error: {err}")),
            lib_ipc::server::ServerTransportError::UnexpectedStream { expected, received } => {
                Self::InvalidState(format!("unexpected transport stream kind (expected {expected:?}, received {received:?})"))
            }
            lib_ipc::server::ServerTransportError::WrongService { expected, received } => {
                Self::InvalidState(format!("unexpected transport service kind (expected {expected:?}, received {received:?})"))
            }
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

impl lib_ipc::server::RetryableError for Error {
    fn retryable(&self) -> bool {
        matches!(self, Error::Http(_) | Error::Io(_))
    }
}
