use axum::{Json, http::StatusCode, response::IntoResponse};
use lib_schema_migration::{SyncSchemaPlan, normalize_to_current};
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

const CURRENT_NT4_SETTINGS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
struct StoredNt4SettingsFile {
    schema_version: u32,
    settings: Nt4Settings,
}

const NT4_SETTINGS_SCHEMA_PLAN: SyncSchemaPlan<serde_json::Value> = SyncSchemaPlan::strict("nt4 settings file", CURRENT_NT4_SETTINGS_SCHEMA_VERSION);

fn parse_nt4_settings_file(bytes: &[u8]) -> Result<(StoredNt4SettingsFile, bool), String> {
    let raw = serde_json::from_slice::<serde_json::Value>(bytes).map_err(|err| format!("failed to decode nt4 settings: {err}"))?;
    let migrated = normalize_to_current(raw.clone(), &NT4_SETTINGS_SCHEMA_PLAN)?;
    let parsed = serde_json::from_value::<StoredNt4SettingsFile>(migrated.clone()).map_err(|err| format!("failed to parse nt4 settings: {err}"))?;
    Ok((parsed, migrated != raw))
}

fn settings_path() -> PathBuf {
    match std::env::var_os("HELIOS_NT4_SETTINGS_FILE") {
        Some(path) => PathBuf::from(path),
        None => persisted_files::data_root_file("nt4.json"),
    }
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
    parsed.settings
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

    let path = settings_path();
    let stored = StoredNt4SettingsFile { schema_version: CURRENT_NT4_SETTINGS_SCHEMA_VERSION, settings: next };
    let bytes = serde_json::to_vec(&stored).map_err(|err| ApiError::internal(format!("failed to encode nt4 settings: {err}")))?;
    persisted_files::write_canonical(&path, &bytes).await.map_err(|err| ApiError::internal(format!("failed to persist nt4 settings: {err}")))?;
    Ok(StatusCode::NO_CONTENT)
}
