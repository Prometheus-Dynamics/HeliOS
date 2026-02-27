use std::collections::VecDeque;
use std::sync::Mutex;

use axum::{
    Json, Router,
    extract::Query,
    http::{HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::get,
};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::error::ErrorBody;

const MAX_ERRORS: usize = 500;

static ERROR_HISTORY: Lazy<Mutex<VecDeque<ErrorHistoryEntry>>> = Lazy::new(|| Mutex::new(VecDeque::with_capacity(MAX_ERRORS)));

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct ErrorHistoryEntry {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorHistoryResponse {
    pub items: Vec<ErrorHistoryEntry>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ErrorHistoryQuery {
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub since_ms: Option<u64>,
}

pub fn router() -> Router<super::AppState> {
    Router::new().route("/", get(list_history).delete(clear_history)).route("/export", get(export_history))
}

#[utoipa::path(
    get,
    path = "/errors",
    tag = "Errors",
    params(
        ("limit" = Option<usize>, Query, description = "Maximum number of entries"),
        ("since_ms" = Option<u64>, Query, description = "Only include entries since this timestamp (ms)")
    ),
    responses((status = 200, description = "Error history", body = ErrorHistoryResponse))
)]
pub(crate) async fn list_history(Query(query): Query<ErrorHistoryQuery>) -> Json<ErrorHistoryResponse> {
    let items = snapshot_history(query.limit, query.since_ms);
    Json(ErrorHistoryResponse { items })
}

#[utoipa::path(
    get,
    path = "/errors/export",
    tag = "Errors",
    params(
        ("limit" = Option<usize>, Query, description = "Maximum number of entries"),
        ("since_ms" = Option<u64>, Query, description = "Only include entries since this timestamp (ms)")
    ),
    responses((status = 200, description = "Error history export", body = ErrorHistoryResponse))
)]
pub(crate) async fn export_history(Query(query): Query<ErrorHistoryQuery>) -> impl IntoResponse {
    let items = snapshot_history(query.limit, query.since_ms);
    let body = Json(ErrorHistoryResponse { items });
    let mut headers = HeaderMap::new();
    headers.insert("content-disposition", HeaderValue::from_static("attachment; filename=\"helios-error-history.json\""));
    (headers, body)
}

#[utoipa::path(
    delete,
    path = "/errors",
    tag = "Errors",
    responses((status = 204, description = "Cleared error history"))
)]
pub(crate) async fn clear_history() -> StatusCode {
    if let Ok(mut guard) = ERROR_HISTORY.lock() {
        guard.clear();
    }
    StatusCode::NO_CONTENT
}

fn snapshot_history(limit: Option<usize>, since_ms: Option<u64>) -> Vec<ErrorHistoryEntry> {
    let mut items = Vec::new();
    if let Ok(guard) = ERROR_HISTORY.lock() {
        for entry in guard.iter() {
            if let Some(since) = since_ms
                && entry.timestamp_ms < since
            {
                continue;
            }
            items.push(entry.clone());
        }
    }
    if let Some(limit) = limit
        && items.len() > limit
    {
        items.truncate(limit);
    }
    items
}

pub fn record_error_entry(entry: ErrorHistoryEntry) {
    if let Ok(mut guard) = ERROR_HISTORY.lock() {
        guard.push_front(entry);
        while guard.len() > MAX_ERRORS {
            guard.pop_back();
        }
    }
}

pub fn record_error_body(body: &ErrorBody, status: StatusCode, transport: &str) {
    let entry = ErrorHistoryEntry {
        id: uuid::Uuid::new_v4().to_string(),
        status: Some(status.as_u16()),
        code: body.code.clone(),
        error: body.error.clone(),
        details: body.details.clone(),
        timestamp_ms: body.timestamp_ms,
        source: body.source.clone(),
        operation: body.operation.clone(),
        request_id: body.request_id.clone(),
        trace_id: body.trace_id.clone(),
        retryable: body.retryable,
        remediation: body.remediation.clone(),
        reported_by: body.reported_by.clone(),
        transport: Some(transport.to_string()),
    };
    record_error_entry(entry);
}
