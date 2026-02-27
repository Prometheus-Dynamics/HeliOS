use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use lib_sensors::fan_config::FanConfig;
use serde::Deserialize;
use tracing::debug;
use utoipa::ToSchema;

use super::super::AppState;
use super::super::error::{ApiError, ApiResult, ErrorBody};

#[derive(Debug, Deserialize, ToSchema)]
pub struct FanConfigRequest {
    pub fan: FanConfig,
    #[serde(default)]
    pub requested_by: Option<String>,
}

#[utoipa::path(
    get,
    path = "/device/fan/config",
    tag = "Device",
    responses(
        (status = 200, description = "Fan configuration", body = FanConfig),
        (status = 503, description = "Peripherals IPC unavailable", body = ErrorBody),
        (status = 502, description = "Peripherals error", body = ErrorBody)
    )
)]
pub async fn fan_config(State(state): State<AppState>) -> ApiResult<impl IntoResponse> {
    let Some(sensors) = state.ensure_sensors().await else {
        return Err(ApiError::service_unavailable("peripherals IPC unavailable"));
    };

    match sensors.fan_config().await {
        Ok(Ok(config)) => Ok(Json(config)),
        Ok(Err(reason)) => Err(ApiError::bad_gateway(reason)),
        Err(err) => Err(ApiError::bad_gateway(format!("failed to fetch fan config: {err}"))),
    }
}

#[utoipa::path(
    post,
    path = "/device/fan/config",
    tag = "Device",
    request_body = FanConfigRequest,
    responses(
        (status = 204, description = "Fan configuration updated"),
        (status = 400, description = "Invalid request", body = ErrorBody),
        (status = 503, description = "Peripherals IPC unavailable", body = ErrorBody),
        (status = 502, description = "Peripherals error", body = ErrorBody)
    )
)]
pub async fn update_fan_config(State(state): State<AppState>, Json(payload): Json<FanConfigRequest>) -> ApiResult<StatusCode> {
    let Some(sensors) = state.ensure_sensors().await else {
        return Err(ApiError::service_unavailable("peripherals IPC unavailable"));
    };

    if let Some(requested_by) = payload.requested_by.as_deref() {
        debug!(requested_by, "fan config update requested");
    }

    match sensors.update_fan_config(payload.fan).await {
        Ok(Ok(())) => Ok(StatusCode::NO_CONTENT),
        Ok(Err(reason)) => Err(ApiError::bad_request(reason)),
        Err(err) => Err(ApiError::bad_gateway(format!("failed to update fan config: {err}"))),
    }
}
