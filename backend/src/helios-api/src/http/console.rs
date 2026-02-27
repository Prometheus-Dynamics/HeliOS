use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get},
};
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::console_sessions::{CONSOLE_SESSIONS, ConsoleSessionListPayload, ConsoleSessionSummaryPayload};

use super::AppState;
use super::error::{ApiError, ApiResult};

pub fn router() -> Router<AppState> {
    Router::new().route("/sessions", get(list_sessions).post(create_session)).route("/sessions/:session_id", delete(delete_session))
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateConsoleSessionRequest {
    #[serde(default)]
    pub cols: Option<u16>,
    #[serde(default)]
    pub rows: Option<u16>,
}

#[utoipa::path(
    get,
    path = "/console/sessions",
    tag = "Console",
    responses(
        (status = 200, description = "List console sessions", body = ConsoleSessionListPayload),
        (status = 500, description = "Internal error", body = crate::http::error::ErrorBody)
    )
)]
pub async fn list_sessions(_state: State<AppState>) -> ApiResult<impl IntoResponse> {
    let sessions = CONSOLE_SESSIONS.list().await;
    Ok(Json(ConsoleSessionListPayload { sessions }))
}

#[utoipa::path(
    post,
    path = "/console/sessions",
    tag = "Console",
    request_body = CreateConsoleSessionRequest,
    responses(
        (status = 200, description = "Created session", body = ConsoleSessionSummaryPayload),
        (status = 500, description = "Internal error", body = crate::http::error::ErrorBody)
    )
)]
pub async fn create_session(_state: State<AppState>, Json(payload): Json<CreateConsoleSessionRequest>) -> ApiResult<impl IntoResponse> {
    let session = CONSOLE_SESSIONS.create(payload.cols, payload.rows).await.map_err(|err| ApiError::internal(err.to_string()))?;
    Ok(Json(session.summary()))
}

#[utoipa::path(
    delete,
    path = "/console/sessions/{session_id}",
    tag = "Console",
    params(
        ("session_id" = String, Path, description = "Session UUID")
    ),
    responses(
        (status = 204, description = "Session deleted"),
        (status = 404, description = "Session not found", body = crate::http::error::ErrorBody)
    )
)]
pub async fn delete_session(_state: State<AppState>, Path(session_id): Path<Uuid>) -> ApiResult<impl IntoResponse> {
    match CONSOLE_SESSIONS.close(session_id).await {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(crate::console_sessions::ConsoleSessionError::NotFound) => Err(ApiError::not_found("console session not found")),
        Err(err) => Err(ApiError::internal(err.to_string())),
    }
}
