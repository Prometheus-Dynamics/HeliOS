//! Errors of starting, running and stopping recordings.

use crate::storage::StorageError;

#[derive(Debug, thiserror::Error)]
pub enum RecorderError {
    /// A recording is already active (one at a time per device).
    #[error("a recording is already running: {0}")]
    Busy(String),
    #[error("{0}")]
    Storage(#[from] StorageError),
    /// The camera service is unreachable or refused the request.
    #[error("camera service: {0}")]
    Camera(String),
    #[error("recording: {0}")]
    Mcap(String),
    #[error("settings: {0}")]
    Settings(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    Io(String),
}

impl From<std::io::Error> for RecorderError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

impl From<styx::replay::ReplayError> for RecorderError {
    fn from(error: styx::replay::ReplayError) -> Self {
        Self::Mcap(error.to_string())
    }
}
