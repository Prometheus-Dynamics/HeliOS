use std::borrow::Cow;
use thiserror::Error;

pub type Result<T> = core::result::Result<T, AiError>;

#[derive(Debug, Error)]
pub enum AiError {
    #[error("unsupported operation: {reason}")]
    Unsupported { reason: Cow<'static, str> },
    #[error("model load failed: {reason}")]
    ModelLoadFailed { reason: String },
    #[error("inference failed: {reason}")]
    InferenceFailed { reason: String },
    #[error("backend is not ready: {reason}")]
    NotReady { reason: String },
    #[error("invalid input: {reason}")]
    InvalidInput { reason: String },
    #[error(transparent)]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

impl AiError {
    pub fn unsupported<T: Into<Cow<'static, str>>>(reason: T) -> Self {
        Self::Unsupported { reason: reason.into() }
    }
}
