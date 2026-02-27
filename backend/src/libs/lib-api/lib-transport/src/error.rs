use core::fmt;
use std::time::Duration;

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Handler(String),
    Json(serde_json::Error),
    Connection(String),
    HeartbeatTimeout(HeartbeatTimeout),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Handler(msg) => write!(f, "handler error: {msg}"),
            Error::Json(err) => write!(f, "json serialization error: {err}"),
            Error::Connection(msg) => write!(f, "transport connection error: {msg}"),
            Error::HeartbeatTimeout(timeout) => timeout.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Json(err) => Some(err),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Json(err)
    }
}

impl From<String> for Error {
    fn from(value: String) -> Self {
        Error::Handler(value)
    }
}

impl From<&str> for Error {
    fn from(value: &str) -> Self {
        Error::Handler(value.to_owned())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HeartbeatTimeout {
    pub elapsed: Duration,
    pub threshold: Duration,
}

impl HeartbeatTimeout {
    pub fn new(elapsed: Duration, threshold: Duration) -> Self {
        Self { elapsed, threshold }
    }
}

impl fmt::Display for HeartbeatTimeout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "heartbeat timeout after {:?} (max {:?})", self.elapsed, self.threshold)
    }
}

impl std::error::Error for HeartbeatTimeout {}
