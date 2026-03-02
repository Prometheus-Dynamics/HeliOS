use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};

use super::AppState;
use crate::http::validation::validation_error_response;
use helios_engine::localization::config::LocalizationConfig;

pub mod config;
pub mod external;
pub mod maps;
mod media_imu;
pub mod peers;
pub mod pipeline;
pub mod solve;
pub mod sources;
pub mod validation;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/validate", post(validate_localization))
        .route("/capabilities", get(localization_capabilities_handler))
        .route("/sources", get(sources::list_sources))
        .route("/config", get(config::get_config).put(config::update_config))
        .route("/profiles/export", get(config::export_profiles))
        .route("/profiles/import", post(config::import_profiles))
        .route("/solve", get(solve::solve))
        .route("/streams/:id/outputs/:output_key", get(sources::sample_output))
        .route("/peers/:id/outputs/:output_key", get(sources::sample_peer_output))
        .route("/profiles/:id/outputs/:output_key", get(sources::sample_profile_output))
        .merge(external::router())
        .merge(pipeline::router())
        .nest("/maps", maps::router())
}

#[utoipa::path(
    post,
    path = "/localization/validate",
    tag = "Localization",
    request_body = LocalizationConfig,
    responses(
        (status = 200, description = "Validated + canonicalized localization config", body = validation::LocalizationValidateResponse),
        (status = 422, description = "Semantic validation failure", body = crate::http::validation::ValidationErrorBody)
    )
)]
async fn validate_localization(Json(config): Json<LocalizationConfig>) -> axum::response::Response {
    match validation::validate_localization_config(config).await {
        Ok(result) => Json(validation::LocalizationValidateResponse { config: result.config, warnings: result.warnings }).into_response(),
        Err(err) => validation_error_response("localization config failed semantic validation", err.issues, err.warnings),
    }
}

#[utoipa::path(
    get,
    path = "/localization/capabilities",
    tag = "Localization",
    responses((status = 200, description = "Localization validation constraints and defaults", body = validation::LocalizationCapabilitiesResponse))
)]
async fn localization_capabilities_handler() -> Json<validation::LocalizationCapabilitiesResponse> {
    Json(validation::localization_capabilities())
}
