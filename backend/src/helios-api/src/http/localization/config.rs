use axum::{Json, extract::State, response::IntoResponse};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use utoipa::ToSchema;

use super::super::AppState;
use super::super::error::{ApiError, ApiResult};
use super::super::{json_store, storage};
use helios_engine::localization::config::{LocalizationConfig, normalize_config};
use tracing::{info, warn};

use crate::http::validation::validation_error_response;

use super::validation::validate_localization_config;

const LOCALIZATION_PROFILES_SCHEMA_V1: &str = "helios.localization.profiles.v1";

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationProfilesExportEnvelope {
    pub schema: String,
    pub exported_at: String,
    pub config: LocalizationConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationProfilesImportRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exported_at: Option<String>,
    pub config: LocalizationConfig,
}

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

#[utoipa::path(
    get,
    path = "/localization/profiles/export",
    tag = "Localization",
    responses((status = 200, description = "Exported localization profiles envelope", body = LocalizationProfilesExportEnvelope))
)]
pub async fn export_profiles(State(_state): State<AppState>) -> ApiResult<Json<LocalizationProfilesExportEnvelope>> {
    let config = normalize_config(load_config().await?);
    Ok(Json(LocalizationProfilesExportEnvelope { schema: LOCALIZATION_PROFILES_SCHEMA_V1.to_string(), exported_at: chrono::Utc::now().to_rfc3339(), config }))
}

#[utoipa::path(
    post,
    path = "/localization/profiles/import",
    tag = "Localization",
    request_body = LocalizationProfilesImportRequest,
    responses(
        (status = 200, description = "Imported localization config", body = LocalizationConfig),
        (status = 400, description = "Invalid import payload", body = crate::http::error::ErrorBody),
        (status = 422, description = "Semantic validation failure", body = crate::http::validation::ValidationErrorBody)
    )
)]
pub async fn import_profiles(State(_state): State<AppState>, Json(body): Json<Value>) -> axum::response::Response {
    let path = match config_path().await {
        Ok(path) => path,
        Err(err) => return ApiError::internal(format!("failed to resolve localization config: {err}")).into_response(),
    };

    let imported = match extract_imported_config(body) {
        Ok(config) => config,
        Err(err) => return err.into_response(),
    };

    let validated = match validate_localization_config(imported).await {
        Ok(result) => result,
        Err(err) => {
            warn!(
                issue_count = err.issues.len(),
                warning_count = err.warnings.len(),
                issues = ?err.issues,
                warnings = ?err.warnings,
                "localization profiles import rejected by semantic validator"
            );
            return validation_error_response("localization config failed semantic validation", err.issues, err.warnings);
        }
    };

    if !validated.warnings.is_empty() {
        info!(
            warning_count = validated.warnings.len(),
            warnings = ?validated.warnings,
            "localization profiles import sanitized during semantic validation"
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

fn extract_imported_config(body: Value) -> Result<LocalizationConfig, ApiError> {
    if let Ok(payload) = serde_json::from_value::<LocalizationProfilesImportRequest>(body.clone()) {
        if let Some(schema) = payload.schema.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
            if schema != LOCALIZATION_PROFILES_SCHEMA_V1 {
                return Err(ApiError::bad_request(format!("unsupported localization profiles schema `{schema}`; expected `{LOCALIZATION_PROFILES_SCHEMA_V1}`")));
            }
        }
        return Ok(payload.config);
    }

    serde_json::from_value::<LocalizationConfig>(body)
        .map_err(|err| ApiError::bad_request(format!("invalid localization profiles payload: expected `{LOCALIZATION_PROFILES_SCHEMA_V1}` envelope or localization config: {err}")))
}
