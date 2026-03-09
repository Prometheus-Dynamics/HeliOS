use axum::{
    Json, Router,
    extract::{Path, Query, State},
    response::IntoResponse,
    routing::get,
};
use serde::Deserialize;
use std::future::Future;
use std::pin::Pin;
use utoipa::ToSchema;
use uuid::Uuid;

use super::super::AppState;
use super::super::error::ApiError;
use super::super::pipelines;
use super::config;
use super::sources::ApiLocalizationSourceFetcher;

use helios_engine::localization::config::select_profile;
use helios_engine::localization::pipeline::{
    LocalizationPipelineGraphProvider, LocalizationPipelineStatus, PipelineGraphDocument, list_outputs as list_pipeline_outputs, sample_output as sample_pipeline_output, status as pipeline_status,
};
use helios_engine::localization::types::PipelineOutputSample;

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct PipelineQuery {
    #[serde(default)]
    profile_id: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/pipeline/status", get(status)).route("/pipeline/outputs", get(list_outputs)).route("/pipeline/outputs/{output_key}", get(sample_output))
}

struct PipelineAdapter;

impl LocalizationPipelineGraphProvider for PipelineAdapter {
    fn load_graph_document<'a>(&'a self, id: Uuid) -> Pin<Box<dyn Future<Output = Result<PipelineGraphDocument, String>> + Send + 'a>> {
        Box::pin(async move {
            let doc = pipelines::load_graph_document(id).await.map_err(|err| format!("failed to load pipeline graph: {err}"))?;
            Ok(PipelineGraphDocument { graph: doc.graph, updated_at_ms: doc.updated_at_ms })
        })
    }

    fn load_template_graph<'a>(&'a self, template_id: &'a str) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, String>> + Send + 'a>> {
        Box::pin(async move { pipelines::load_template_graph(template_id).await.map_err(|err| format!("failed to load pipeline template: {err}")) })
    }

    fn template_last_modified_ms<'a>(&'a self, template_id: &'a str) -> Pin<Box<dyn Future<Output = Result<Option<i64>, String>> + Send + 'a>> {
        Box::pin(async move { template_last_modified_ms(template_id).await })
    }
}

#[utoipa::path(
    get,
    path = "/localization/pipeline/status",
    tag = "Localization",
    params(("profile_id" = Option<String>, Query, description = "Profile id override")),
    responses((status = 200, description = "Localization pipeline status", body = LocalizationPipelineStatus))
)]
async fn status(State(_state): State<AppState>, Query(query): Query<PipelineQuery>) -> impl IntoResponse {
    let config = match config::load_config().await {
        Ok(cfg) => cfg,
        Err(err) => return ApiError::bad_gateway(err.to_string()).into_response(),
    };

    match pipeline_status(&config, query.profile_id.as_deref()).await {
        Ok(status) => Json(status).into_response(),
        Err(err) => ApiError::not_found(err).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/localization/pipeline/outputs",
    tag = "Localization",
    params(("profile_id" = Option<String>, Query, description = "Profile id override")),
    responses((status = 200, description = "Localization pipeline outputs", body = [String]))
)]
async fn list_outputs(State(_state): State<AppState>, Query(query): Query<PipelineQuery>) -> impl IntoResponse {
    let config = match config::load_config().await {
        Ok(cfg) => cfg,
        Err(err) => return ApiError::bad_gateway(err.to_string()).into_response(),
    };
    let profile = match select_profile(&config, query.profile_id.as_deref()) {
        Ok(profile) => profile,
        Err(err) => return ApiError::not_found(err).into_response(),
    };
    let Some(template_id) = profile.pipeline_template_id.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) else {
        return ApiError::not_found("localization pipeline not configured").into_response();
    };

    let provider = PipelineAdapter;
    match list_pipeline_outputs(&provider, profile, &template_id).await {
        Ok(outputs) => Json(outputs).into_response(),
        Err(err) => ApiError::bad_gateway(err).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/localization/pipeline/outputs/{output_key}",
    tag = "Localization",
    params(("profile_id" = Option<String>, Query, description = "Profile id override"), ("output_key" = String, Path, description = "Output key")),
    responses(
        (status = 200, description = "Latest output sample", body = PipelineOutputSample),
        (status = 404, description = "No sample available", body = super::super::error::ErrorBody),
        (status = 502, description = "Pipeline error", body = super::super::error::ErrorBody)
    )
)]
async fn sample_output(State(state): State<AppState>, Query(query): Query<PipelineQuery>, Path(output_key): Path<String>) -> impl IntoResponse {
    let config = match config::load_config().await {
        Ok(cfg) => cfg,
        Err(err) => return ApiError::bad_gateway(err.to_string()).into_response(),
    };
    let profile = match select_profile(&config, query.profile_id.as_deref()) {
        Ok(profile) => profile,
        Err(err) => return ApiError::not_found(err).into_response(),
    };
    let Some(template_id) = profile.pipeline_template_id.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) else {
        return ApiError::not_found("localization pipeline not configured").into_response();
    };

    let provider = PipelineAdapter;
    let fetcher = ApiLocalizationSourceFetcher::new(state.clone());

    match sample_pipeline_output(&provider, &fetcher, profile, &template_id, &output_key).await {
        Ok(sample) => Json(sample).into_response(),
        Err(err) if err == "output sample not available" => ApiError::not_found(err).into_response(),
        Err(err) => ApiError::bad_gateway(err).into_response(),
    }
}

async fn template_last_modified_ms(template_id: &str) -> Result<Option<i64>, String> {
    let path = pipelines::pipeline_template_dir().join(format!("{template_id}.json"));
    match tokio::fs::metadata(&path).await {
        Ok(meta) => match meta.modified() {
            Ok(time) => {
                let ms = time.duration_since(std::time::UNIX_EPOCH).map_err(|err| format!("invalid template mtime: {err}"))?.as_millis() as i64;
                Ok(Some(ms))
            }
            Err(err) => Err(format!("failed to read template mtime: {err}")),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("failed to stat template: {err}")),
    }
}

// Re-export types for OpenAPI schema resolution.
