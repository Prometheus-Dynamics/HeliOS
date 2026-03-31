use axum::{Json, http::StatusCode, response::IntoResponse};
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

fn settings_paths() -> (PathBuf, Option<PathBuf>) {
    // TEMP_SHIM: device-nt4-settings-etc-fallback
    // Keep the /etc fallback until every deployed image writes NT4 settings into the data-root path.
    match std::env::var_os("HELIOS_NT4_SETTINGS_FILE") {
        Some(path) => (PathBuf::from(path), None),
        None => (persisted_files::data_root_file("nt4.json"), Some(persisted_files::legacy_helios_etc_file("nt4.json"))),
    }
}

pub(crate) async fn load_settings() -> Nt4Settings {
    let (path, legacy_path) = settings_paths();
    let bytes = match persisted_files::read(&path, legacy_path.as_deref()).await {
        Ok(bytes) => bytes,
        Err(_) => return Nt4Settings::default(),
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
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

    let (path, legacy_path) = settings_paths();
    let bytes = serde_json::to_vec(&next).map_err(|err| ApiError::internal(format!("failed to encode nt4 settings: {err}")))?;
    persisted_files::write_mirrored(&path, legacy_path.as_deref(), &bytes).await.map_err(|err| ApiError::internal(format!("failed to persist nt4 settings: {err}")))?;
    Ok(StatusCode::NO_CONTENT)
}
