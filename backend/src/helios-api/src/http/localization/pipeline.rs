use axum::{
    Json, Router,
    extract::{Path, Query, State},
    response::IntoResponse,
    routing::get,
};
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::super::AppState;
use super::super::error::ApiError;
use super::super::pipelines;
use super::config;
use super::solve;
use super::sources::ApiLocalizationSourceFetcher;

use helios_engine::ipc::{EngineEvent, LocalizationPipelineGraphRequest, LocalizationPipelineSampleRequest, LocalizationPipelineStatusRequest};
use helios_engine::localization::config::select_profile;
use helios_engine::localization::types::{LocalizationPipelineStatus, PipelineOutputSample};

const LOCALIZATION_PIPELINE_GRAPH_NAME: &str = "daedalus_aruco_fast";

#[derive(Debug, Clone)]
struct LoadedPipelineGraph {
    graph: serde_json::Value,
    graph_updated_at_ms: Option<i64>,
    template_mtime_ms: Option<i64>,
}

impl LoadedPipelineGraph {
    fn into_request(self, profile: helios_engine::localization::config::LocalizationProfile) -> LocalizationPipelineGraphRequest {
        LocalizationPipelineGraphRequest { profile, graph: self.graph.into(), graph_updated_at_ms: self.graph_updated_at_ms, template_mtime_ms: self.template_mtime_ms }
    }

    fn into_sample_request(
        self,
        profile: helios_engine::localization::config::LocalizationProfile,
        sources: Vec<helios_engine::localization::config::LocalizationSourceConfig>,
        source_values: Vec<helios_engine::ipc::LocalizationSolveSourceValue>,
        output_key: String,
    ) -> LocalizationPipelineSampleRequest {
        LocalizationPipelineSampleRequest {
            profile,
            sources,
            graph: self.graph.into(),
            graph_updated_at_ms: self.graph_updated_at_ms,
            template_mtime_ms: self.template_mtime_ms,
            source_values,
            output_key,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
struct PipelineQuery {
    #[serde(default)]
    profile_id: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new().route("/pipeline/status", get(status)).route("/pipeline/outputs", get(list_outputs)).route("/pipeline/outputs/{output_key}", get(sample_output))
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
    let profile = match select_profile(&config, query.profile_id.as_deref()) {
        Ok(profile) => profile,
        Err(err) => return ApiError::not_found(err).into_response(),
    };
    if let Err(err) = load_pipeline_graph().await {
        return ApiError::bad_gateway(err).into_response();
    }

    match _state.engine.localization_pipeline_status_event(LocalizationPipelineStatusRequest { profile_id: profile.id.clone() }).await {
        Ok(EngineEvent::LocalizationPipelineStatus { response, .. }) => match serde_json::from_value::<LocalizationPipelineStatus>(response.into()) {
            Ok(status) => Json(status).into_response(),
            Err(err) => ApiError::bad_gateway(format!("invalid localization pipeline status response: {err}")).into_response(),
        },
        Ok(EngineEvent::Nack { reason, .. }) => ApiError::bad_gateway(reason).into_response(),
        Ok(other) => ApiError::bad_gateway(format!("unexpected engine response: {other:?}")).into_response(),
        Err(err) => ApiError::bad_gateway(err.to_string()).into_response(),
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
    let graph = match load_pipeline_graph().await {
        Ok(graph) => graph,
        Err(err) => return ApiError::bad_gateway(err).into_response(),
    };

    match _state.engine.localization_pipeline_outputs_event(graph.into_request(profile.clone())).await {
        Ok(EngineEvent::LocalizationPipelineOutputs { outputs, .. }) => Json(outputs).into_response(),
        Ok(EngineEvent::Nack { reason, .. }) => ApiError::bad_gateway(reason).into_response(),
        Ok(other) => ApiError::bad_gateway(format!("unexpected engine response: {other:?}")).into_response(),
        Err(err) => ApiError::bad_gateway(err.to_string()).into_response(),
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
    let graph = match load_pipeline_graph().await {
        Ok(graph) => graph,
        Err(err) => return ApiError::bad_gateway(err).into_response(),
    };
    let fetcher = ApiLocalizationSourceFetcher::new(state.clone());
    let source_values = solve::fetch_localization_source_values(&fetcher, &profile.sources).await;
    let request = graph.into_sample_request(profile.clone(), profile.sources.clone(), source_values, output_key.clone());

    match state.engine.localization_pipeline_output_sample_event(request).await {
        Ok(EngineEvent::LocalizationPipelineOutputSample { response, .. }) => match serde_json::from_value::<PipelineOutputSample>(response.into()) {
            Ok(sample) => Json(sample).into_response(),
            Err(err) => ApiError::bad_gateway(format!("invalid localization pipeline sample response: {err}")).into_response(),
        },
        Ok(EngineEvent::Nack { code, reason, .. }) if code == helios_engine::ipc::EngineErrorCode::NotFound => ApiError::not_found(reason).into_response(),
        Ok(EngineEvent::Nack { reason, .. }) => ApiError::bad_gateway(reason).into_response(),
        Ok(other) => ApiError::bad_gateway(format!("unexpected engine response: {other:?}")).into_response(),
        Err(err) => ApiError::bad_gateway(err.to_string()).into_response(),
    }
}

async fn load_pipeline_graph() -> Result<LoadedPipelineGraph, String> {
    let graph_name = LOCALIZATION_PIPELINE_GRAPH_NAME;
    if let Ok(id) = Uuid::parse_str(graph_name) {
        let doc = pipelines::load_graph_document(id).await.map_err(|err| format!("failed to load pipeline graph: {err}"))?;
        return Ok(LoadedPipelineGraph { graph: doc.graph, graph_updated_at_ms: Some(doc.updated_at_ms), template_mtime_ms: None });
    }
    let graph = pipelines::load_template_graph(graph_name).await.map_err(|err| format!("failed to load localization pipeline graph: {err}"))?;
    let path = pipelines::pipeline_template_dir().join(format!("{graph_name}.json"));
    let template_mtime_ms = match tokio::fs::metadata(&path).await {
        Ok(meta) => match meta.modified() {
            Ok(time) => Some(time.duration_since(std::time::UNIX_EPOCH).map_err(|err| format!("invalid template mtime: {err}"))?.as_millis() as i64),
            Err(err) => return Err(format!("failed to read template mtime: {err}")),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(format!("failed to stat template: {err}")),
    };
    Ok(LoadedPipelineGraph { graph, graph_updated_at_ms: None, template_mtime_ms })
}

// Re-export types for OpenAPI schema resolution.
