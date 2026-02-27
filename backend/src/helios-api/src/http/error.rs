use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use std::fmt;
use std::future::Future;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use utoipa::ToSchema;

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ErrorBody {
    pub code: String,
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    pub timestamp_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retryable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reported_by: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ApiError {
    status: StatusCode,
    code: String,
    message: String,
    details: Option<String>,
    timestamp_ms: Option<u64>,
    source: Option<String>,
    operation: Option<String>,
    request_id: Option<String>,
    trace_id: Option<String>,
    retryable: Option<bool>,
    remediation: Option<String>,
    reported_by: Option<String>,
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.details {
            Some(details) => write!(f, "{}: {} ({})", self.code, self.message, details),
            None => write!(f, "{}: {}", self.code, self.message),
        }
    }
}

impl From<Box<ApiError>> for ApiError {
    fn from(err: Box<ApiError>) -> Self {
        *err
    }
}

#[derive(Debug, Clone)]
pub struct ErrorContext {
    pub request_id: Option<String>,
    pub trace_id: Option<String>,
    pub source: Option<String>,
    pub operation: Option<String>,
    pub reported_by: Option<String>,
    sequence: Arc<AtomicU64>,
}

tokio::task_local! {
    static ERROR_CONTEXT: ErrorContext;
}

pub async fn with_error_context<F, T>(context: ErrorContext, fut: F) -> T
where
    F: Future<Output = T>,
{
    ERROR_CONTEXT.scope(context, fut).await
}

pub fn current_error_context() -> ErrorContext {
    ERROR_CONTEXT.try_with(|ctx| ctx.clone()).unwrap_or_default()
}

impl Default for ErrorContext {
    fn default() -> Self {
        Self { request_id: None, trace_id: None, source: None, operation: None, reported_by: None, sequence: Arc::new(AtomicU64::new(0)) }
    }
}

impl ErrorContext {
    pub fn next_sequence(&self) -> u64 {
        self.sequence.fetch_add(1, Ordering::Relaxed)
    }
}

fn now_timestamp_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

impl ApiError {
    pub fn new(status: StatusCode, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status,
            code: code.into(),
            message: message.into(),
            details: None,
            timestamp_ms: None,
            source: None,
            operation: None,
            request_id: None,
            trace_id: None,
            retryable: None,
            remediation: None,
            reported_by: None,
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "bad_request", message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    pub fn bad_gateway(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_GATEWAY, "bad_gateway", message)
    }

    pub fn service_unavailable(message: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, "service_unavailable", message)
    }

    pub fn payload_too_large(message: impl Into<String>) -> Self {
        Self::new(StatusCode::PAYLOAD_TOO_LARGE, "payload_too_large", message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal", message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut body = ErrorBody {
            code: self.code,
            error: self.message,
            details: self.details,
            timestamp_ms: self.timestamp_ms.unwrap_or_else(now_timestamp_ms),
            source: self.source,
            operation: self.operation,
            request_id: self.request_id,
            trace_id: self.trace_id,
            retryable: self.retryable,
            remediation: self.remediation,
            reported_by: self.reported_by,
        };

        let context = current_error_context();
        if body.request_id.is_none() {
            body.request_id = context.request_id;
        }
        if body.trace_id.is_none() {
            body.trace_id = context.trace_id;
        }
        if body.source.is_none() {
            body.source = context.source;
        }
        if body.operation.is_none() {
            body.operation = context.operation;
        }
        if body.reported_by.is_none() {
            body.reported_by = context.reported_by;
        }

        crate::http::error_history::record_error_body(&body, self.status, "http");
        (self.status, Json(body)).into_response()
    }
}

impl From<std::io::Error> for ApiError {
    fn from(err: std::io::Error) -> Self {
        let status = if err.kind() == std::io::ErrorKind::NotFound { StatusCode::NOT_FOUND } else { StatusCode::INTERNAL_SERVER_ERROR };
        let code = if status == StatusCode::NOT_FOUND { "not_found" } else { "internal" };
        Self::new(status, code, err.to_string())
    }
}

impl From<lib_ipc::client::ClientTransportError> for ApiError {
    fn from(err: lib_ipc::client::ClientTransportError) -> Self {
        Self::bad_gateway(err.to_string())
    }
}

impl From<lib_net::Error> for ApiError {
    fn from(err: lib_net::Error) -> Self {
        use lib_net::Error::*;
        let status = match err {
            InterfaceNotFound(_) => StatusCode::NOT_FOUND,
            UnsupportedIpVersion(_) | FailedToApplyInterfaceSettings(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::BAD_GATEWAY,
        };
        let code = match status {
            StatusCode::NOT_FOUND => "not_found",
            StatusCode::BAD_REQUEST => "bad_request",
            StatusCode::BAD_GATEWAY => "bad_gateway",
            _ => "internal",
        };
        Self::new(status, code, err.to_string())
    }
}

impl ErrorBody {
    pub fn new(code: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            error: error.into(),
            details: None,
            timestamp_ms: now_timestamp_ms(),
            source: None,
            operation: None,
            request_id: None,
            trace_id: None,
            retryable: None,
            remediation: None,
            reported_by: None,
        }
    }
}
