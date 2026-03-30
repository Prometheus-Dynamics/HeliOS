use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::http::AppState;
use crate::http::error::ApiError;
use helios_engine::stream::{PipelineFlamegraphMetrics, PipelineGraphMetrics, StreamMetrics};

const PIPELINE_METRICS_PRIME_INTERVAL_MS: u64 = 200;
const PIPELINE_METRICS_PRIME_SETTLE_MS: u64 = 120;

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct SetPipelinePerfRequest {
    #[serde(default)]
    pub pipeline_id: Option<Uuid>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ResetPipelineMetricsRequest {
    #[serde(default)]
    pub pipeline_id: Option<Uuid>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct PipelineProfileRequest {
    /// Optional warmup delay before resetting metrics / capturing profiles.
    #[serde(default)]
    pub warmup_ms: Option<u64>,
    /// Benchmark/profile capture duration in milliseconds.
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub pipeline_id: Option<Uuid>,
    /// Enable perf counters (cache misses, branch stats) during the capture window.
    #[serde(default)]
    pub enable_perf_counters: Option<bool>,
    /// Capture a CPU flamegraph SVG during the capture window.
    #[serde(default)]
    pub capture_flamegraph: Option<bool>,
    /// Reset rolling metrics before the capture window (recommended).
    #[serde(default)]
    pub reset_metrics: Option<bool>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PipelineProfileNodeSummary {
    pub node: String,
    pub average_time_ms: f64,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PipelineProfileSummary {
    pub graph_average_time_ms: f64,
    pub top_nodes: Vec<PipelineProfileNodeSummary>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct PipelineProfileResponse {
    pub stream_id: Uuid,
    pub pipeline_id: Option<Uuid>,
    pub warmup_ms: u64,
    pub duration_ms: u64,
    pub metrics: StreamMetrics,
    #[serde(default)]
    pub pipeline: Option<PipelineGraphMetrics>,
    #[serde(default)]
    pub flamegraph: Option<PipelineFlamegraphMetrics>,
    #[serde(default)]
    pub summary: Option<PipelineProfileSummary>,
}

#[derive(Debug, Clone, Deserialize, IntoParams)]
pub struct FlamegraphQuery {
    #[serde(default)]
    pub pipeline_id: Option<Uuid>,
}

fn pick_pipeline_metrics(metrics: &StreamMetrics, pipeline_id: Option<Uuid>) -> Option<&PipelineGraphMetrics> {
    if let Some(pipeline_id) = pipeline_id {
        // Prefer per-pipeline instance metrics when available. Some stream configurations only
        // populate `metrics.pipeline` (active pipeline) and leave `pipeline_instances` empty.
        if let Some(hit) = metrics.pipeline_instances.as_ref().and_then(|m| m.get(&pipeline_id.to_string())) {
            return Some(hit);
        }
        return metrics.pipeline.as_ref();
    }
    metrics.pipeline.as_ref()
}

fn summarize_pipeline(pipeline: &PipelineGraphMetrics) -> PipelineProfileSummary {
    let mut nodes: Vec<PipelineProfileNodeSummary> = pipeline
        .nodes
        .iter()
        .filter_map(|(k, v)| {
            if k == "graph" {
                return None;
            }
            Some(PipelineProfileNodeSummary { node: k.clone(), average_time_ms: v.metrics.average_time_ms })
        })
        .collect();
    nodes.sort_by(|a, b| b.average_time_ms.partial_cmp(&a.average_time_ms).unwrap_or(std::cmp::Ordering::Equal));
    nodes.truncate(12);
    let graph_average_time_ms = pipeline.nodes.get("graph").map(|n| n.metrics.average_time_ms).unwrap_or(0.0);
    PipelineProfileSummary { graph_average_time_ms, top_nodes: nodes }
}

pub(crate) fn pipeline_metrics_need_host_output_priming(metrics: &StreamMetrics) -> bool {
    metrics.pipeline.as_ref().is_some_and(|pipeline| {
        pipeline.nodes.is_empty()
            && pipeline.groups.as_ref().is_none_or(BTreeMap::is_empty)
            && pipeline.edges.as_ref().is_none_or(BTreeMap::is_empty)
            && pipeline.flamegraph.is_none()
    })
}

async fn active_pipeline_output_for_metrics(state: &AppState, stream_id: Uuid) -> Option<String> {
    let manifest = state.services.streams.load_live_stream_manifest(state, stream_id).await?;
    if let Some(pipeline_id) = manifest.active_pipeline_id
        && let Some(output) = super::util::preferred_pipeline_output(&manifest, pipeline_id)
    {
        return Some(output);
    }
    manifest.active_pipeline_output.and_then(|output: String| {
        let trimmed = output.trim();
        if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
    })
}

pub(crate) async fn prime_pipeline_metrics_once(state: &AppState, stream_id: Uuid) -> bool {
    let Some(output) = active_pipeline_output_for_metrics(state, stream_id).await else {
        return false;
    };
    let _ = state.engine.get_graph_output_sample_event(stream_id, output).await;
    tokio::time::sleep(std::time::Duration::from_millis(PIPELINE_METRICS_PRIME_SETTLE_MS)).await;
    true
}

async fn hold_pipeline_metrics_demand(state: AppState, stream_id: Uuid, duration_ms: u64) {
    let Some(output) = active_pipeline_output_for_metrics(&state, stream_id).await else {
        return;
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(duration_ms.saturating_add(PIPELINE_METRICS_PRIME_SETTLE_MS));
    loop {
        let _ = state.engine.get_graph_output_sample_event(stream_id, output.clone()).await;
        if std::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(PIPELINE_METRICS_PRIME_INTERVAL_MS)).await;
    }
}

#[utoipa::path(
    post,
    path = "/streams/{id}/pipeline/perf",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    request_body = SetPipelinePerfRequest,
    responses((status = 204, description = "Perf counters updated"))
)]
pub async fn set_pipeline_perf(State(state): State<AppState>, Path(id): Path<Uuid>, Json(req): Json<SetPipelinePerfRequest>) -> Result<impl IntoResponse, ApiError> {
    match state.engine.set_graph_perf(id, req.pipeline_id, req.enabled).await {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(err) => Err(ApiError::bad_gateway(err.to_string())),
    }
}

#[utoipa::path(
    post,
    path = "/streams/{id}/pipeline/metrics/reset",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    request_body = ResetPipelineMetricsRequest,
    responses((status = 204, description = "Pipeline metrics reset"))
)]
pub async fn reset_pipeline_metrics(State(state): State<AppState>, Path(id): Path<Uuid>, Json(req): Json<ResetPipelineMetricsRequest>) -> Result<impl IntoResponse, ApiError> {
    match state.engine.reset_graph_metrics(id, req.pipeline_id).await {
        Ok(_) => Ok(StatusCode::NO_CONTENT),
        Err(err) => Err(ApiError::bad_gateway(err.to_string())),
    }
}

#[utoipa::path(
    post,
    path = "/streams/{id}/pipeline/profile",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    request_body = PipelineProfileRequest,
    responses((status = 200, description = "Pipeline profile result", body = PipelineProfileResponse))
)]
pub async fn profile_pipeline(State(state): State<AppState>, Path(id): Path<Uuid>, Json(req): Json<PipelineProfileRequest>) -> Result<impl IntoResponse, ApiError> {
    let warmup_ms = req.warmup_ms.unwrap_or(500).min(30_000);
    let duration_ms = req.duration_ms.unwrap_or(5_000).clamp(250, 120_000);
    let pipeline_id = req.pipeline_id;
    let enable_perf = req.enable_perf_counters.unwrap_or(true);
    let capture_flamegraph = req.capture_flamegraph.unwrap_or(true);
    let reset_metrics = req.reset_metrics.unwrap_or(true);
    let metrics_keepalive = tokio::spawn(hold_pipeline_metrics_demand(state.clone(), id, warmup_ms.saturating_add(duration_ms)));

    if warmup_ms > 0 {
        tokio::time::sleep(std::time::Duration::from_millis(warmup_ms)).await;
    }

    if reset_metrics {
        let _ = state.engine.reset_graph_metrics(id, pipeline_id).await;
    }
    let _ = state.engine.set_graph_perf(id, pipeline_id, enable_perf).await;
    if capture_flamegraph {
        match state.engine.capture_graph_flamegraph(id, pipeline_id, duration_ms).await {
            Ok(_) => {}
            Err(err) => {
                // Don't fail the benchmark if flamegraph isn't available; surface as a warning via metrics.
                tracing::warn!(stream_id = %id, error = %err, "flamegraph capture request failed");
            }
        }
    }

    tokio::time::sleep(std::time::Duration::from_millis(duration_ms)).await;
    let _ = metrics_keepalive.await;

    let mut metrics = match state.engine.get_metrics(id).await {
        Ok(helios_engine::ipc::EngineEvent::Metrics { metrics, .. }) => metrics,
        Ok(helios_engine::ipc::EngineEvent::Nack { reason, .. }) => return Err(ApiError::bad_gateway(reason)),
        Ok(other) => return Err(ApiError::bad_gateway(format!("unexpected engine response: {other:?}"))),
        Err(err) => return Err(ApiError::bad_gateway(err.to_string())),
    };
    if pipeline_metrics_need_host_output_priming(&metrics) && prime_pipeline_metrics_once(&state, id).await {
        metrics = match state.engine.get_metrics(id).await {
            Ok(helios_engine::ipc::EngineEvent::Metrics { metrics, .. }) => metrics,
            Ok(helios_engine::ipc::EngineEvent::Nack { reason, .. }) => return Err(ApiError::bad_gateway(reason)),
            Ok(other) => return Err(ApiError::bad_gateway(format!("unexpected engine response: {other:?}"))),
            Err(err) => return Err(ApiError::bad_gateway(err.to_string())),
        };
    }

    let pipeline = pick_pipeline_metrics(&metrics, pipeline_id).cloned();
    let flamegraph = pipeline.as_ref().and_then(|p| p.flamegraph.clone());
    let summary = pipeline.as_ref().map(summarize_pipeline);

    Ok(Json(PipelineProfileResponse { stream_id: id, pipeline_id, warmup_ms, duration_ms, metrics, pipeline, flamegraph, summary }))
}

#[utoipa::path(
    get,
    path = "/streams/{id}/pipeline/flamegraph.svg",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID"), FlamegraphQuery),
    responses((status = 200, description = "Flamegraph SVG", content_type = "image/svg+xml"))
)]
pub async fn download_pipeline_flamegraph_svg(State(state): State<AppState>, Path(id): Path<Uuid>, Query(query): Query<FlamegraphQuery>) -> Result<impl IntoResponse, ApiError> {
    let metrics = match state.engine.get_metrics(id).await {
        Ok(helios_engine::ipc::EngineEvent::Metrics { metrics, .. }) => metrics,
        Ok(helios_engine::ipc::EngineEvent::Nack { reason, .. }) => return Err(ApiError::bad_gateway(reason)),
        Ok(other) => return Err(ApiError::bad_gateway(format!("unexpected engine response: {other:?}"))),
        Err(err) => return Err(ApiError::bad_gateway(err.to_string())),
    };

    let Some(pipeline) = pick_pipeline_metrics(&metrics, query.pipeline_id) else {
        return Err(ApiError::not_found("pipeline metrics unavailable"));
    };
    let Some(fg) = pipeline.flamegraph.as_ref() else {
        return Err(ApiError::not_found("flamegraph unavailable"));
    };

    let path = std::path::Path::new(&fg.path);
    if !path.is_absolute() {
        return Err(ApiError::internal("flamegraph path is not absolute"));
    }
    if path.extension().and_then(|e| e.to_str()).unwrap_or("") != "svg" {
        return Err(ApiError::internal("flamegraph path is not an svg"));
    }

    // Only allow reading from common scratch/data locations.
    //
    // The engine currently writes flamegraphs under the Helios log directory on-device.
    let allowed = ["/tmp", "/var/tmp", "/run", "/var/lib/helios", "/var/lib/helios/api-data", "/var/log/helios"];
    let raw = path.to_string_lossy();
    if !allowed.iter().any(|prefix| raw.starts_with(prefix)) {
        return Err(ApiError::internal("flamegraph path not in allowed directories"));
    }

    let bytes = tokio::fs::read(path).await.map_err(ApiError::from)?;
    let mut headers = HeaderMap::new();
    headers.insert(axum::http::header::CONTENT_TYPE, HeaderValue::from_static("image/svg+xml"));
    Ok((headers, bytes))
}

#[cfg(test)]
mod tests {
    use super::pipeline_metrics_need_host_output_priming;
    use helios_engine::stream::{PipelineGraphMetrics, PipelineNodeMetrics, PipelineNodeRuntimeMetrics, StreamMetrics};
    use std::collections::BTreeMap;

    #[test]
    fn empty_pipeline_metrics_request_priming() {
        let metrics = StreamMetrics { pipeline: Some(PipelineGraphMetrics::default()), ..StreamMetrics::default() };
        assert!(pipeline_metrics_need_host_output_priming(&metrics));
    }

    #[test]
    fn populated_pipeline_metrics_skip_priming() {
        let mut pipeline = PipelineGraphMetrics::default();
        pipeline.nodes.insert(
            "graph".to_string(),
            PipelineNodeRuntimeMetrics {
                metrics: PipelineNodeMetrics { average_time_ms: 1.0, average_fps: 30.0, sample_count: 1, window_size: 32, last_sample_age_ms: Some(0) },
                ..PipelineNodeRuntimeMetrics::default()
            },
        );
        let metrics = StreamMetrics { pipeline: Some(pipeline), ..StreamMetrics::default() };
        assert!(!pipeline_metrics_need_host_output_priming(&metrics));
    }

    #[test]
    fn group_only_pipeline_metrics_skip_priming() {
        let mut pipeline = PipelineGraphMetrics::default();
        pipeline.groups = Some(BTreeMap::from([(
            "group".to_string(),
            PipelineNodeRuntimeMetrics {
                metrics: PipelineNodeMetrics { average_time_ms: 1.0, average_fps: 30.0, sample_count: 1, window_size: 32, last_sample_age_ms: Some(0) },
                ..PipelineNodeRuntimeMetrics::default()
            },
        )]));
        let metrics = StreamMetrics { pipeline: Some(pipeline), ..StreamMetrics::default() };
        assert!(!pipeline_metrics_need_host_output_priming(&metrics));
    }
}
