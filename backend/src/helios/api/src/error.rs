//! The one error shape every endpoint returns:
//! `{"error": {"code": "...", "message": "...", "needs": "..."?}}`.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    BadRequest,
    /// The device is secured and the request carries no valid session or token (401).
    Unauthorized,
    /// Authenticated, but the request is refused: a missing CSRF header, or the password
    /// re-confirmation failed (403).
    Forbidden,
    NotFound,
    Conflict,
    Unprocessable,
    PayloadTooLarge,
    /// Too many failed sign-in attempts from this address (429).
    TooManyRequests,
    /// The backend for this endpoint does not exist yet (501).
    NotAvailable,
    /// The backend exists but is not reachable right now, e.g. Orion is down (503).
    BackendUnavailable,
    Internal,
}

impl ErrorCode {
    pub fn status(self) -> StatusCode {
        match self {
            Self::BadRequest => StatusCode::BAD_REQUEST,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Conflict => StatusCode::CONFLICT,
            Self::Unprocessable => StatusCode::UNPROCESSABLE_ENTITY,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::TooManyRequests => StatusCode::TOO_MANY_REQUESTS,
            Self::NotAvailable => StatusCode::NOT_IMPLEMENTED,
            Self::BackendUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
    /// For `not_available`: the missing backend piece, so clients can say what is coming.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub needs: Option<String>,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: &'a ApiError,
}

impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), needs: None }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::BadRequest, message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Unauthorized, message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Forbidden, message)
    }

    pub fn too_many_requests(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::TooManyRequests, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message)
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Conflict, message)
    }

    pub fn unprocessable(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Unprocessable, message)
    }

    pub fn backend(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::BackendUnavailable, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }

    /// 501: the feature has no backend yet. `needs` names what has to land first.
    pub fn not_available(message: impl Into<String>, needs: impl Into<String>) -> Self {
        Self { code: ErrorCode::NotAvailable, message: message.into(), needs: Some(needs.into()) }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut response = (self.code.status(), Json(ErrorBody { error: &self })).into_response();
        if self.code == ErrorCode::Unauthorized {
            response.headers_mut().insert(axum::http::header::WWW_AUTHENTICATE, axum::http::HeaderValue::from_static("Bearer realm=\"helios\""));
        }
        response
    }
}

impl From<std::io::Error> for ApiError {
    fn from(error: std::io::Error) -> Self {
        Self::internal(error.to_string())
    }
}

pub type ApiResult<T> = Result<T, ApiError>;
