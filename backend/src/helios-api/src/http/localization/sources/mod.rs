mod catalog;
mod profile;
mod profile_stack;
mod sample_refresh;
mod sampling;

pub(crate) use profile::fetch_profile_output;
use profile_stack::ProfileResolveStack;
pub(crate) use sample_refresh::LocalizationStreamSampleRefreshRuntime;
pub(crate) use sampling::{fetch_peer_output, fetch_stream_output};

use axum::{
    Json,
    extract::{Path, State},
};
use serde_json::Value as JsonValue;
use uuid::Uuid;

use super::super::AppState;
use helios_engine::localization::config::LocalizationSourceConfig;
use helios_engine::localization::fetch::LocalizationSourceFetcher;
use helios_engine::localization::types::{LocalizationPipelineSource, PipelineOutputSample};

pub(super) use helios_engine::contracts::localization::DEVICE_IMU_EXTERNAL_SOURCE_ID as IMU_EXTERNAL_ID;
pub(super) const PROFILE_STREAM_PREFIX: &str = "profile:";
pub(super) const PROFILE_OUTPUT_PREFIX: &str = "solver:";

#[derive(Clone)]
pub struct ApiLocalizationSourceFetcher {
    pub(super) state: AppState,
    pub(super) profile_resolve_stack: ProfileResolveStack,
}

impl ApiLocalizationSourceFetcher {
    pub fn new(state: AppState) -> Self {
        Self { state, profile_resolve_stack: ProfileResolveStack::default() }
    }
}

impl LocalizationSourceFetcher for ApiLocalizationSourceFetcher {
    fn fetch_source_value<'a>(&'a self, source: &'a LocalizationSourceConfig) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<JsonValue, String>> + Send + 'a>> {
        Box::pin(async move {
            if source.stream_id.starts_with("peer:") {
                fetch_peer_output(&self.state, &source.stream_id, &source.output_key).await
            } else if let Some(source_id) = source.stream_id.strip_prefix("external:") {
                super::external::fetch_external_value(&self.state, source_id, &source.output_key).await
            } else if let Some(profile_id) = source.stream_id.strip_prefix(PROFILE_STREAM_PREFIX) {
                fetch_profile_output(self, profile_id, &source.output_key).await
            } else {
                fetch_stream_output(&self.state, &source.stream_id, &source.output_key).await
            }
        })
    }
}

#[utoipa::path(
    get,
    path = "/localization/sources",
    tag = "Localization",
    responses((status = 200, description = "Available localization pipeline outputs", body = [LocalizationPipelineSource]))
)]
pub async fn list_sources(state: State<AppState>) -> crate::http::error::ApiResult<Json<Vec<LocalizationPipelineSource>>> {
    catalog::list_sources(state).await
}

#[utoipa::path(
    get,
    path = "/localization/streams/{id}/outputs/{output_key}",
    tag = "Localization",
    params(("id" = Uuid, Path, description = "Stream ID"), ("output_key" = String, Path, description = "Graph output port")),
    responses(
        (status = 200, description = "Latest output sample", body = PipelineOutputSample),
        (status = 404, description = "No sample available", body = crate::http::streams::types::EngineErrorBody),
        (status = 502, description = "Engine error", body = crate::http::streams::types::EngineErrorBody)
    )
)]
pub async fn sample_output(state: State<AppState>, path: Path<(Uuid, String)>) -> axum::response::Response {
    sampling::sample_output(state, path).await
}

#[utoipa::path(
    get,
    path = "/localization/peers/{id}/outputs/{output_key}",
    tag = "Localization",
    params(("id" = String, Path, description = "Peer ID"), ("output_key" = String, Path, description = "Output key (e.g. tag_poses)")),
    responses(
        (status = 200, description = "Latest output sample", body = PipelineOutputSample),
        (status = 404, description = "No sample available", body = crate::http::streams::types::EngineErrorBody),
        (status = 400, description = "Unsupported output", body = crate::http::streams::types::EngineErrorBody),
        (status = 502, description = "Peer error", body = crate::http::streams::types::EngineErrorBody)
    )
)]
pub async fn sample_peer_output(state: State<AppState>, path: Path<(String, String)>) -> axum::response::Response {
    sampling::sample_peer_output(state, path).await
}

#[utoipa::path(
    get,
    path = "/localization/profiles/{id}/outputs/{output_key}",
    tag = "Localization",
    params(("id" = String, Path, description = "Localization profile ID"), ("output_key" = String, Path, description = "Profile solver output key")),
    responses(
        (status = 200, description = "Latest profile output sample", body = PipelineOutputSample),
        (status = 404, description = "No sample available", body = crate::http::streams::types::EngineErrorBody),
        (status = 400, description = "Unsupported output", body = crate::http::streams::types::EngineErrorBody),
        (status = 502, description = "Solver error", body = crate::http::streams::types::EngineErrorBody)
    )
)]
pub async fn sample_profile_output(state: State<AppState>, path: Path<(String, String)>) -> axum::response::Response {
    sampling::sample_profile_output(state, path).await
}
