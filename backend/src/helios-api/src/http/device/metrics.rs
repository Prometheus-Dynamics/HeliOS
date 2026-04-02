use axum::{Json, http::StatusCode};
use utoipa::ToSchema;

use super::super::error::ApiResult;
use crate::api_observability::ApiRuntimeMetrics;
use crate::http::AppState;
use crate::http::revision::{apply_revision_headers, matches_if_none_match, not_modified_response};
use crate::system_read_model::ReadModelFreshness;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::IntoResponse;

#[derive(Debug, Clone, ToSchema, serde::Serialize)]
pub struct DeviceMetrics {
    pub status: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<DeviceHealthIssue>,
    pub cpu_avg_pct: f32,
    pub cpu_freq_mhz: u64,
    pub cpus: Vec<CpuCoreMetrics>,
    pub mem_total_bytes: u64,
    pub mem_used_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub disks: Vec<DiskMetrics>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub processes: Vec<ProcessMemoryMetrics>,
    pub temps: Vec<TempReading>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api: Option<ApiRuntimeMetrics>,
}

#[derive(Debug, Clone, ToSchema, serde::Serialize)]
pub struct DeviceMetricsResponse {
    pub freshness: ReadModelFreshness,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<DeviceMetrics>,
}

#[derive(Debug, Clone, ToSchema, serde::Serialize)]
pub struct DeviceHealthIssue {
    pub code: String,
    pub description: String,
}

#[derive(Debug, Clone, ToSchema, serde::Serialize)]
pub struct DiskMetrics {
    pub mount: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Debug, Clone, ToSchema, serde::Serialize)]
pub struct ProcessMemoryMetrics {
    pub pid: u32,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable_file_bytes: Option<u64>,
    pub threads: u64,
    pub rss_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pss_bytes: Option<u64>,
    pub private_dirty_bytes: u64,
    pub swap_bytes: u64,
    pub executable_pss_bytes: u64,
    pub shared_lib_pss_bytes: u64,
    pub heap_pss_bytes: u64,
    pub stack_pss_bytes: u64,
    pub anonymous_pss_bytes: u64,
    pub device_pss_bytes: u64,
    pub deleted_pss_bytes: u64,
    pub other_pss_bytes: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top_pss_mappings: Vec<ProcessMappingMetrics>,
}

#[derive(Debug, Clone, ToSchema, serde::Serialize)]
pub struct ProcessMappingMetrics {
    pub bucket: String,
    pub label: String,
    pub pss_bytes: u64,
}

#[derive(Debug, Clone, ToSchema, serde::Serialize)]
pub struct CpuCoreMetrics {
    pub id: usize,
    pub name: String,
    pub pct: f32,
    pub freq_mhz: u64,
}

#[derive(Debug, Clone, ToSchema, serde::Serialize)]
pub struct TempReading {
    pub label: String,
    pub temperature_c: f32,
}

#[utoipa::path(
    get,
    path = "/device/metrics",
    tag = "Device",
    responses((status = 200, description = "Device metrics", body = DeviceMetricsResponse))
)]
pub async fn metrics(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<impl axum::response::IntoResponse> {
    let snapshot = state.services.system.load_device_metrics_snapshot().await;
    if matches_if_none_match(&headers, snapshot.revision) {
        return Ok(not_modified_response(snapshot.revision));
    }

    let mut metrics = snapshot.payload;
    if let Some(body) = metrics.as_mut() {
        body.api = Some(ApiRuntimeMetrics {
            system_metrics_cache: state.services.system.device_metrics_cache_metrics(),
            log_sources_cache: state.services.system.log_sources_cache_metrics(),
            pipelines_graphs_cache: state.services.pipelines.graph_list_cache_metrics(),
            pipelines_registry_cache: state.services.pipelines.registry_cache_metrics(),
            streams_list_cache: state.services.streams.stream_list_cache_metrics(),
            peripherals_inventory_cache: state.services.hardware.peripheral_inventory_cache_metrics(),
            camera_discovery_cache: state.services.hardware.camera_discovery_cache_metrics(),
            realtime: state.services.system.realtime_metrics().await,
            media: state.services.media.cache_metrics().await,
        });
    }

    let mut response = (StatusCode::OK, Json(DeviceMetricsResponse { freshness: snapshot.freshness, metrics })).into_response();
    apply_revision_headers(response.headers_mut(), snapshot.revision);
    Ok(response)
}
