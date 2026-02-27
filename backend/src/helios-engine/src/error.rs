use std::fmt;

#[derive(Debug, Clone)]
pub enum Error {
    Unimplemented(&'static str),
    InvalidState(&'static str),
    InvalidStateOwned(String),
    NotFound(&'static str),
    Conflict(&'static str),
    Timeout,
}

pub type Result<T> = core::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unimplemented(msg) => write!(f, "unimplemented: {msg}"),
            Error::InvalidState(msg) => write!(f, "invalid state: {msg}"),
            Error::InvalidStateOwned(msg) => write!(f, "invalid state: {msg}"),
            Error::NotFound(msg) => write!(f, "not found: {msg}"),
            Error::Conflict(msg) => write!(f, "conflict: {msg}"),
            Error::Timeout => write!(f, "operation timed out"),
        }
    }
}

impl std::error::Error for Error {}

impl lib_ipc::server::RetryableError for Error {
    fn retryable(&self) -> bool {
        matches!(self, Error::Timeout)
    }
}
