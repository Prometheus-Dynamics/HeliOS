use std::fmt;

#[derive(Debug, Clone)]
pub enum Error {
    Unimplemented(&'static str),
    InvalidState(&'static str),
    InvalidStateOwned(String),
    RetryableInvalidStateOwned(String),
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
            Error::RetryableInvalidStateOwned(msg) => write!(f, "invalid state: {msg}"),
            Error::NotFound(msg) => write!(f, "not found: {msg}"),
            Error::Conflict(msg) => write!(f, "conflict: {msg}"),
            Error::Timeout => write!(f, "operation timed out"),
        }
    }
}

impl std::error::Error for Error {}

impl Error {
    pub fn retryable(&self) -> bool {
        matches!(self, Error::Timeout | Error::RetryableInvalidStateOwned(_))
    }
}

impl lib_ipc::server::RetryableError for Error {
    fn retryable(&self) -> bool {
        self.retryable()
    }
}

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn retryable_error_variants_are_marked_retryable() {
        assert!(Error::Timeout.retryable());
        assert!(Error::RetryableInvalidStateOwned("capture warming up".to_string()).retryable());
    }

    #[test]
    fn non_retryable_error_variants_remain_non_retryable() {
        assert!(!Error::InvalidState("broken").retryable());
        assert!(!Error::InvalidStateOwned("broken".to_string()).retryable());
        assert!(!Error::Conflict("busy").retryable());
    }
}
