use axum::{Json, extract::State};
use std::path::PathBuf;

use super::super::AppState;
use super::super::error::{ApiError, ApiResult};
use super::super::{json_store, storage};
use helios_engine::localization::config::{LocalizationConfig, normalize_config};

#[utoipa::path(
    get,
    path = "/localization/config",
    tag = "Localization",
    responses((status = 200, description = "Localization config", body = LocalizationConfig))
)]
pub async fn get_config(State(_state): State<AppState>) -> ApiResult<Json<LocalizationConfig>> {
    let config = load_config().await?;
    Ok(Json(normalize_config(config)))
}

#[utoipa::path(
    put,
    path = "/localization/config",
    tag = "Localization",
    request_body = LocalizationConfig,
    responses((status = 200, description = "Updated localization config", body = LocalizationConfig))
)]
pub async fn update_config(State(_state): State<AppState>, Json(body): Json<LocalizationConfig>) -> ApiResult<Json<LocalizationConfig>> {
    let path = config_path().await.map_err(|err| ApiError::internal(format!("failed to resolve localization config: {err}")))?;
    let normalized = normalize_config(body);
    json_store::write_json(path, &normalized).await.map_err(|err| ApiError::internal(format!("failed to write localization config: {err}")))?;
    Ok(Json(normalized))
}

pub(crate) async fn load_config() -> ApiResult<LocalizationConfig> {
    let path = config_path().await.map_err(|err| ApiError::internal(format!("failed to resolve localization config: {err}")))?;
    Ok(json_store::read_json_or_default(&path).await)
}

async fn config_path() -> std::io::Result<PathBuf> {
    let dir = storage::ensure_subdir_async("localization").await?;
    Ok(dir.join("config.json"))
}
