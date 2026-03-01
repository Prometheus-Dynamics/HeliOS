use axum::{Json, extract::State, response::IntoResponse};
use std::path::PathBuf;

use super::super::AppState;
use super::super::error::{ApiError, ApiResult};
use super::super::{json_store, storage};
use helios_engine::localization::config::{LocalizationConfig, normalize_config};
use tracing::{info, warn};

use crate::http::validation::validation_error_response;

use super::validation::validate_localization_config;

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
    responses(
        (status = 200, description = "Updated localization config", body = LocalizationConfig),
        (status = 422, description = "Semantic validation failure", body = crate::http::validation::ValidationErrorBody)
    )
)]
pub async fn update_config(State(_state): State<AppState>, Json(body): Json<LocalizationConfig>) -> axum::response::Response {
    let path = match config_path().await {
        Ok(path) => path,
        Err(err) => return ApiError::internal(format!("failed to resolve localization config: {err}")).into_response(),
    };

    let validated = match validate_localization_config(body).await {
        Ok(result) => result,
        Err(err) => {
            warn!(
                issue_count = err.issues.len(),
                warning_count = err.warnings.len(),
                issues = ?err.issues,
                warnings = ?err.warnings,
                "localization config update rejected by semantic validator"
            );
            return validation_error_response("localization config failed semantic validation", err.issues, err.warnings);
        }
    };

    if !validated.warnings.is_empty() {
        info!(
            warning_count = validated.warnings.len(),
            warnings = ?validated.warnings,
            "localization config sanitized during semantic validation"
        );
    }

    match json_store::write_json(path, &validated.config).await {
        Ok(()) => Json(validated.config).into_response(),
        Err(err) => ApiError::internal(format!("failed to write localization config: {err}")).into_response(),
    }
}

pub(crate) async fn load_config() -> ApiResult<LocalizationConfig> {
    let path = config_path().await.map_err(|err| ApiError::internal(format!("failed to resolve localization config: {err}")))?;
    Ok(json_store::read_json_or_default(&path).await)
}

async fn config_path() -> std::io::Result<PathBuf> {
    let dir = storage::ensure_subdir_async("localization").await?;
    Ok(dir.join("config.json"))
}
