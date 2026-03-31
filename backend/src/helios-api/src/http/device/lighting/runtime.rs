use axum::{Json, extract::State, http::StatusCode};
use helios_peripherals::dto::LightingCommand;
use tracing::debug;

use crate::http::{
    AppState,
    error::{ApiError, ApiResult, ErrorBody},
};

use super::{LightingCommandRequest, LightingRuntimeStatePayload};

#[utoipa::path(
    post,
    path = "/device/lighting",
    tag = "Device",
    request_body = LightingCommandRequest,
    responses(
        (status = 204, description = "Lighting command applied"),
        (status = 400, description = "Bad request", body = ErrorBody),
        (status = 503, description = "Peripherals IPC unavailable", body = ErrorBody),
        (status = 502, description = "Peripherals error", body = ErrorBody),
    )
)]
pub async fn lighting_command(State(state): State<AppState>, Json(body): Json<LightingCommandRequest>) -> ApiResult<StatusCode> {
    let Some(sensors) = state.ensure_sensors().await else {
        return Err(ApiError::service_unavailable("peripherals IPC unavailable"));
    };

    if let Some(requested_by) = body.requested_by.as_deref() {
        debug!(requested_by, brightness = ?body.brightness, "lighting command requested");
    }

    let frame = body.frame.map(|entries| entries.into_iter().map(Into::into).collect::<Vec<_>>());
    let animation = body.animation.map(Into::into);
    let command = LightingCommand { frame, brightness: body.brightness, animation };

    match sensors.lighting_command(command).await {
        Ok(Ok(())) => Ok(StatusCode::NO_CONTENT),
        Ok(Err(reason)) => Err(ApiError::bad_request(reason)),
        Err(err) => Err(ApiError::bad_gateway(format!("failed to send lighting command: {err}"))),
    }
}

#[utoipa::path(
    get,
    path = "/device/lighting/state",
    tag = "Device",
    responses(
        (status = 200, description = "Current lighting runtime state", body = LightingRuntimeStatePayload),
        (status = 503, description = "Peripherals IPC unavailable", body = ErrorBody),
        (status = 502, description = "Peripherals error", body = ErrorBody),
    )
)]
pub async fn lighting_state(State(state): State<AppState>) -> ApiResult<impl axum::response::IntoResponse> {
    let Some(sensors) = state.ensure_sensors().await else {
        return Err(ApiError::service_unavailable("peripherals IPC unavailable"));
    };

    match sensors.lighting_state().await {
        Ok(Ok(current)) => Ok(Json(LightingRuntimeStatePayload::from(current))),
        Ok(Err(reason)) => Err(ApiError::bad_gateway(reason)),
        Err(err) => Err(ApiError::bad_gateway(format!("failed to fetch lighting runtime state: {err}"))),
    }
}
