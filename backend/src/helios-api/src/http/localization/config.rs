use axum::http::HeaderMap;
use axum::{Json, extract::State, response::IntoResponse};
use lib_schema_migration::{SyncSchemaPlan, migrate_to_current};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use utoipa::ToSchema;

use super::super::AppState;
use super::super::error::{ApiError, ApiResult};
use super::super::{json_store, storage};
use helios_engine::localization::config::{LocalizationConfig, normalize_config};
use tracing::{info, warn};

use crate::http::revision::{apply_revision_headers, matches_if_none_match, not_modified_response};
use crate::http::validation::validation_error_response;

use super::validation::validate_localization_config;

const LOCALIZATION_PROFILES_SCHEMA_V1: &str = "helios.localization.profiles.v1";
const CURRENT_LOCALIZATION_CONFIG_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredLocalizationConfigDocument {
    pub schema_version: u32,
    #[serde(default)]
    pub config: LocalizationConfig,
}

const LOCALIZATION_CONFIG_SCHEMA_PLAN: SyncSchemaPlan<serde_json::Value> = SyncSchemaPlan {
    document_name: "localization config document",
    legacy_version: CURRENT_LOCALIZATION_CONFIG_SCHEMA_VERSION,
    current_version: CURRENT_LOCALIZATION_CONFIG_SCHEMA_VERSION,
    migrations: &[],
};

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
pub async fn get_config(State(_state): State<AppState>, headers: HeaderMap) -> ApiResult<axum::response::Response> {
    let revision = current_config_revision().await;
    if matches_if_none_match(&headers, revision) {
        return Ok(not_modified_response(revision));
    }

    let config = load_config().await?;
    let mut response = Json(normalize_config(config)).into_response();
    apply_revision_headers(response.headers_mut(), revision);
    Ok(response)
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

    match json_store::write_json(path, &StoredLocalizationConfigDocument { schema_version: CURRENT_LOCALIZATION_CONFIG_SCHEMA_VERSION, config: validated.config.clone() }).await {
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
        Err(err) => return ApiError::bad_request(err).into_response(),
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

    match json_store::write_json(path, &StoredLocalizationConfigDocument { schema_version: CURRENT_LOCALIZATION_CONFIG_SCHEMA_VERSION, config: validated.config.clone() }).await {
        Ok(()) => Json(validated.config).into_response(),
        Err(err) => ApiError::internal(format!("failed to write localization config: {err}")).into_response(),
    }
}

pub(crate) async fn load_config() -> ApiResult<LocalizationConfig> {
    let path = config_path().await.map_err(|err| ApiError::internal(format!("failed to resolve localization config: {err}")))?;
    let bytes = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(LocalizationConfig::default()),
        Err(err) => return Err(ApiError::internal(format!("failed to read localization config: {err}"))),
    };
    match decode_localization_config(&bytes) {
        Ok(config) => Ok(config),
        Err(err) => {
            warn!(path = %path.display(), %err, "invalid persisted localization config; using defaults");
            Ok(LocalizationConfig::default())
        }
    }
}

async fn config_path() -> std::io::Result<PathBuf> {
    let dir = storage::ensure_subdir_async("localization").await?;
    Ok(dir.join("config.json"))
}

async fn current_config_revision() -> u64 {
    let Ok(path) = config_path().await else {
        return 0;
    };
    let Ok(metadata) = tokio::fs::metadata(path).await else {
        return 0;
    };
    metadata.modified().ok().and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok()).map(|value| value.as_millis() as u64).unwrap_or(0)
}

fn extract_imported_config(body: Value) -> Result<LocalizationConfig, String> {
    if let Ok(payload) = serde_json::from_value::<LocalizationProfilesImportRequest>(body.clone()) {
        if let Some(schema) = payload.schema.as_deref().map(str::trim).filter(|value| !value.is_empty())
            && schema != LOCALIZATION_PROFILES_SCHEMA_V1
        {
            return Err(format!("unsupported localization profiles schema `{schema}`; expected `{LOCALIZATION_PROFILES_SCHEMA_V1}`"));
        }
        return Ok(payload.config);
    }

    serde_json::from_value::<LocalizationConfig>(body)
        .map_err(|err| format!("invalid localization profiles payload: expected `{LOCALIZATION_PROFILES_SCHEMA_V1}` envelope or localization config: {err}"))
}

fn decode_localization_config(bytes: &[u8]) -> Result<LocalizationConfig, String> {
    let raw = serde_json::from_slice::<serde_json::Value>(bytes).map_err(|err| format!("failed to decode localization config document: {err}"))?;
    let migrated = migrate_to_current(raw, &LOCALIZATION_CONFIG_SCHEMA_PLAN)?;
    let parsed: StoredLocalizationConfigDocument = serde_json::from_value(migrated).map_err(|err| format!("failed to parse localization config document: {err}"))?;
    Ok(parsed.config)
}

#[cfg(test)]
mod tests {
    use super::{CURRENT_LOCALIZATION_CONFIG_SCHEMA_VERSION, LocalizationConfig, StoredLocalizationConfigDocument, decode_localization_config};

    #[test]
    fn decode_localization_config_rejects_missing_schema_version() {
        let raw = serde_json::json!({
            "config": LocalizationConfig::default()
        });

        let err = decode_localization_config(&serde_json::to_vec(&raw).expect("encode")).expect_err("missing schema version should fail");
        assert!(err.contains("missing required schema_version"));
    }

    #[test]
    fn decode_localization_config_rejects_future_schema_version() {
        let raw = serde_json::json!({
            "schema_version": CURRENT_LOCALIZATION_CONFIG_SCHEMA_VERSION + 1,
            "config": LocalizationConfig::default()
        });

        let err = decode_localization_config(&serde_json::to_vec(&raw).expect("encode")).expect_err("future schema version should fail");
        assert!(err.contains("unsupported localization config document schema_version"));
    }

    #[test]
    fn stored_localization_config_document_serializes_current_schema_version() {
        let raw = serde_json::to_value(StoredLocalizationConfigDocument {
            schema_version: CURRENT_LOCALIZATION_CONFIG_SCHEMA_VERSION,
            config: LocalizationConfig::default(),
        })
        .expect("encode");

        assert_eq!(raw.get("schema_version").and_then(serde_json::Value::as_u64), Some(u64::from(CURRENT_LOCALIZATION_CONFIG_SCHEMA_VERSION)));
    }
}
