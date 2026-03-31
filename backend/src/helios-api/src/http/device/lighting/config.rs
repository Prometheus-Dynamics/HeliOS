use axum::{Json, http::StatusCode, response::IntoResponse};
use lib_sensors::led_config::{self, LedConfig};
use serde::Serialize;
use tokio::fs;
use tracing::debug;

use crate::http::{
    error::{ApiError, ApiResult, ErrorBody},
    persisted_files,
};

use super::LightingConfigRequest;

const LEGACY_LED_SETTINGS_PATH: &str = "/etc/helios/leds.toml";

#[derive(Debug, Serialize)]
struct LightingConfigDoc {
    leds: LedConfig,
}

#[utoipa::path(
    get,
    path = "/device/lighting/config",
    tag = "Device",
    responses((status = 200, description = "Lighting configuration", body = LedConfig))
)]
pub async fn lighting_config() -> ApiResult<impl IntoResponse> {
    let config = load_lighting_config().await;
    Ok(Json(config))
}

#[utoipa::path(
    post,
    path = "/device/lighting/config",
    tag = "Device",
    request_body = LightingConfigRequest,
    responses(
        (status = 204, description = "Lighting configuration updated"),
        (status = 400, description = "Invalid request", body = ErrorBody)
    )
)]
pub async fn update_lighting_config(Json(payload): Json<LightingConfigRequest>) -> ApiResult<StatusCode> {
    if let Some(requested_by) = payload.requested_by.as_deref() {
        debug!(requested_by, "lighting config update requested");
    }

    if payload.lighting.count == 0 {
        return Err(ApiError::bad_request("led count must be at least 1"));
    }
    if payload.lighting.color_order.trim().is_empty() {
        return Err(ApiError::bad_request("color order is required"));
    }
    if payload.lighting.protocol.trim().is_empty() {
        return Err(ApiError::bad_request("protocol is required"));
    }

    persist_lighting_config(&payload.lighting).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/device/lighting/config/reset",
    tag = "Device",
    responses((status = 200, description = "Lighting configuration reset", body = LedConfig))
)]
pub async fn reset_lighting_config() -> ApiResult<impl IntoResponse> {
    let config = LedConfig::default();
    persist_lighting_config(&config).await?;
    Ok(Json(config))
}

async fn load_lighting_config() -> LedConfig {
    let paths = led_config::default_paths();
    led_config::load_led_config(&paths).unwrap_or_default()
}

async fn persist_lighting_config(config: &LedConfig) -> ApiResult<()> {
    let doc = LightingConfigDoc { leds: config.clone() };
    let serialized = toml::to_string_pretty(&doc).map_err(|err| ApiError::bad_request(format!("failed to serialize lighting config: {err}")))?;
    let persistent_path = led_config::writable_path();
    persisted_files::write_mirrored(&persistent_path, Some(std::path::Path::new(LEGACY_LED_SETTINGS_PATH)), serialized.as_bytes())
        .await
        .map_err(|err| ApiError::internal(format!("failed to write lighting config: {err}")))?;
    fs::metadata(&persistent_path).await.map_err(|err| ApiError::internal(format!("failed to confirm lighting config write: {err}")))?;
    Ok(())
}
