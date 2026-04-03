use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use uuid::Uuid;

use super::super::error::{ApiError, ApiResult};

#[utoipa::path(
    get,
    path = "/device/resource-guard",
    tag = "Device",
    operation_id = "resource_guard_status",
    responses((status = 200, description = "Resource guard status", body = crate::resource_guard::ResourceGuardStatus))
)]
pub async fn status(State(state): State<crate::http::AppState>) -> ApiResult<impl axum::response::IntoResponse> {
    Ok((StatusCode::OK, Json(state.services.runtime.resource_guard().snapshot())))
}

#[utoipa::path(
    post,
    path = "/device/resource-guard/restore/{stream_id}",
    tag = "Device",
    params(("stream_id" = Uuid, Path, description = "Stream ID")),
    responses((status = 200, description = "Resource guard stream restore action", body = crate::resource_guard::ResourceGuardAction))
)]
pub async fn restore(State(state): State<crate::http::AppState>, Path(stream_id): Path<Uuid>) -> ApiResult<impl axum::response::IntoResponse> {
    match state.services.runtime.resource_guard().restore_stream(stream_id).await {
        Ok(action) => Ok((StatusCode::OK, Json(action))),
        Err(message) if message.contains("not currently degraded") => Err(ApiError::not_found(message)),
        Err(message) if message.contains("disabled") => Err(ApiError::service_unavailable(message)),
        Err(message) if message.contains("unavailable") || message.contains("closed") || message.contains("timed out") => Err(ApiError::service_unavailable(message)),
        Err(message) if message.contains("engine rejected") || message.contains("failed restoring") => Err(ApiError::bad_gateway(message)),
        Err(message) => Err(ApiError::bad_request(message)),
    }
}
