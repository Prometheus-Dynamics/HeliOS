use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use utoipa::ToSchema;

use super::super::error::{ApiError, ApiResult};
use super::super::json_store;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub struct Nt4Settings {
    #[serde(default)]
    pub enabled: bool,
    /// When disabled, HeliOS will avoid subscribing to NetworkTables topics (used by explorer + peer telemetry).
    #[serde(default = "default_subscriptions_enabled")]
    pub subscriptions_enabled: bool,
    /// Enable Limelight-compatible API emulation (per-stream adapters).
    #[serde(default)]
    pub emulate_limelight_api: bool,
    /// Enable PhotonVision-compatible API emulation (device-wide adapter).
    #[serde(default)]
    pub emulate_photonvision_api: bool,
    #[serde(default)]
    pub server_host: Option<String>,
    #[serde(default)]
    pub server_port: Option<u16>,
    #[serde(default)]
    pub public_api_url: Option<String>,
}

impl Default for Nt4Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            subscriptions_enabled: default_subscriptions_enabled(),
            emulate_limelight_api: false,
            emulate_photonvision_api: false,
            server_host: None,
            server_port: Some(5810),
            public_api_url: None,
        }
    }
}

fn default_subscriptions_enabled() -> bool {
    true
}

fn settings_path() -> PathBuf {
    std::env::var_os("HELIOS_NT4_SETTINGS_FILE").map(PathBuf::from).unwrap_or_else(|| "/etc/helios/nt4.json".into())
}

pub(crate) async fn load_settings() -> Nt4Settings {
    json_store::read_json_or_default(&settings_path()).await
}

#[utoipa::path(
    get,
    path = "/device/nt4",
    tag = "Device",
    responses((status = 200, description = "NT4 settings", body = Nt4Settings))
)]
pub async fn get_nt4_settings() -> ApiResult<impl IntoResponse> {
    Ok(Json(load_settings().await))
}

#[utoipa::path(
    post,
    path = "/device/nt4",
    tag = "Device",
    request_body = Nt4Settings,
    responses((status = 204, description = "NT4 settings updated"), (status = 400, description = "Invalid request", body = super::super::error::ErrorBody))
)]
pub async fn set_nt4_settings(Json(payload): Json<Nt4Settings>) -> ApiResult<impl IntoResponse> {
    let mut next = payload;
    if let Some(host) = next.server_host.as_ref() {
        let trimmed = host.trim();
        if trimmed.is_empty() {
            next.server_host = None;
        } else {
            next.server_host = Some(trimmed.to_string());
        }
    }
    if let Some(port) = next.server_port
        && port == 0
    {
        return Err(ApiError::bad_request("server_port must be > 0"));
    }

    json_store::write_json(settings_path(), &next).await.map_err(|err| ApiError::internal(format!("failed to persist nt4 settings: {err}")))?;
    Ok(StatusCode::NO_CONTENT)
}
