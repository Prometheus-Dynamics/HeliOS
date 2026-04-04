use axum::{Json, http::StatusCode, response::IntoResponse};
use lib_runtime_policy::{HELIOS_NT4_SETTINGS_FILE_POLICY, Nt4Settings as SharedNt4Settings, encode_nt4_settings_file, normalize_nt4_settings, parse_nt4_settings_file};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use utoipa::ToSchema;

use super::super::error::{ApiError, ApiResult};
use crate::http::persisted_files;

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

fn default_subscriptions_enabled() -> bool {
    SharedNt4Settings::default().subscriptions_enabled
}

impl From<SharedNt4Settings> for Nt4Settings {
    fn from(value: SharedNt4Settings) -> Self {
        Self {
            enabled: value.enabled,
            subscriptions_enabled: value.subscriptions_enabled,
            emulate_limelight_api: value.emulate_limelight_api,
            emulate_photonvision_api: value.emulate_photonvision_api,
            server_host: value.server_host,
            server_port: value.server_port,
            public_api_url: value.public_api_url,
        }
    }
}

impl From<Nt4Settings> for SharedNt4Settings {
    fn from(value: Nt4Settings) -> Self {
        Self {
            enabled: value.enabled,
            subscriptions_enabled: value.subscriptions_enabled,
            emulate_limelight_api: value.emulate_limelight_api,
            emulate_photonvision_api: value.emulate_photonvision_api,
            server_host: value.server_host,
            server_port: value.server_port,
            public_api_url: value.public_api_url,
        }
    }
}

impl Default for Nt4Settings {
    fn default() -> Self {
        SharedNt4Settings::default().into()
    }
}

fn settings_path() -> PathBuf {
    HELIOS_NT4_SETTINGS_FILE_POLICY.resolve()
}

pub(crate) async fn load_settings() -> Nt4Settings {
    let path = settings_path();
    let bytes = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(_) => return Nt4Settings::default(),
    };
    let (parsed, _) = match parse_nt4_settings_file(&bytes) {
        Ok(parsed) => parsed,
        Err(_) => return Nt4Settings::default(),
    };
    parsed.settings.into()
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
    let mut next: SharedNt4Settings = payload.into();
    normalize_nt4_settings(&mut next).map_err(ApiError::bad_request)?;
    let path = settings_path();
    let bytes = encode_nt4_settings_file(next).map_err(ApiError::internal)?;
    persisted_files::write_canonical(&path, &bytes).await.map_err(|err| ApiError::internal(format!("failed to persist nt4 settings: {err}")))?;
    Ok(StatusCode::NO_CONTENT)
}
