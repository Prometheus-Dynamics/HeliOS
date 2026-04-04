use std::sync::Arc;

use axum::response::Response;
use bytes::Bytes;
use helios_engine::capture::CaptureControlInfo;
use helios_engine::ipc::{EngineErrorCode, StreamManifest};
use serde_json::Value as JsonValue;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::api_observability::{ApiCacheMetric, RuntimeTopicBroadcastSnapshot};
use crate::http::pipelines::{PipelineSummary, PlannerDiagnostic};
use crate::http::streams::recording::RecordingRuntimeState;
use crate::http::streams::replay_bundle::ReplayBundleSessionsState;
use crate::http::streams::types::StreamInfo;
use crate::http::streams::{
    mjpeg::{MjpegFeedSubscription, MjpegFeedsState},
    sensor_bench::SensorBenchmarkJobsState,
    snapshot::SnapshotLocksState,
};

#[derive(Clone, Default)]
pub struct PipelinesReadModelService {
    state: Arc<crate::pipelines_read_model::PipelinesReadModelState>,
}

impl PipelinesReadModelService {
    pub async fn load_registry_snapshot_from_disk_or_helper(&self) -> Result<helios_engine::ipc::NodeRegistrySnapshot, String> {
        crate::pipelines_read_model::load_registry_snapshot_from_disk_or_helper().await
    }

    pub async fn set_graph_validation_state(&self, graph_id: Uuid, diagnostics: Vec<PlannerDiagnostic>) {
        self.state.set_graph_validation_state(graph_id, diagnostics).await;
    }

    pub async fn set_graph_validation_error(&self, graph_id: Uuid, code: Option<EngineErrorCode>, message: String) {
        self.state.set_graph_validation_error(graph_id, code, message).await;
    }

    pub async fn clear_graph_validation_state(&self, graph_id: Uuid) {
        self.state.clear_graph_validation_state(graph_id).await;
    }

    pub async fn invalidate_graph_list_cache(&self) {
        self.state.invalidate_graph_list_cache().await;
    }

    pub async fn invalidate_registry_cache(&self) {
        self.state.invalidate_registry_cache().await;
    }

    pub async fn get_cached_graph_summaries_snapshot(&self, state: &crate::http::AppState) -> Result<(Arc<Vec<PipelineSummary>>, u64), Box<Response>> {
        self.state.get_cached_graph_summaries_snapshot(state).await
    }

    pub async fn refresh_graph_validation(&self, state: &crate::http::AppState, graph_id: Uuid, graph: &JsonValue) {
        self.state.refresh_graph_validation(state, graph_id, graph).await;
    }

    pub async fn get_cached_registry_response_snapshot(&self, state: &crate::http::AppState) -> Option<(Bytes, bool, u64)> {
        self.state.get_cached_registry_response_snapshot(state).await
    }

    pub async fn warm_registry_cache(&self, state: crate::http::AppState) {
        self.state.warm_registry_cache(state).await;
    }

    pub async fn inject_cached_port_metadata(&self, state: &crate::http::AppState, graph: &mut JsonValue) {
        self.state.inject_cached_port_metadata(state, graph).await;
    }

    pub fn graph_list_cache_metrics(&self) -> ApiCacheMetric {
        self.state.graph_list_cache_metrics()
    }

    pub fn registry_cache_metrics(&self) -> ApiCacheMetric {
        self.state.registry_cache_metrics()
    }
}

#[derive(Clone, Default)]
pub struct StreamsReadModelService {
    state: Arc<crate::streams_read_model::StreamsReadModelState>,
    mjpeg_feeds: Arc<MjpegFeedsState>,
    recording_runtime: Arc<RecordingRuntimeState>,
    replay_bundle_sessions: Arc<ReplayBundleSessionsState>,
    sensor_benchmark_jobs: Arc<SensorBenchmarkJobsState>,
    snapshot_locks: Arc<SnapshotLocksState>,
}

impl StreamsReadModelService {
    pub async fn get_cached_streams_snapshot_with_revision(&self, state: &crate::http::AppState) -> (Vec<StreamInfo>, bool, u64) {
        self.state.get_cached_streams_snapshot_with_revision(state).await
    }

    pub async fn invalidate_stream_list_cache(&self) {
        self.state.invalidate_stream_list_cache().await;
    }

    pub async fn stream_start_guard(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.state.stream_start_guard().await
    }

    pub fn cache_controls(&self, stream_id: Uuid, controls: &[CaptureControlInfo]) {
        self.state.cache_controls(stream_id, controls);
    }

    pub fn cached_controls(&self, stream_id: Uuid) -> Option<Vec<CaptureControlInfo>> {
        self.state.cached_controls(stream_id)
    }

    pub async fn load_live_stream_manifest(&self, state: &crate::http::AppState, stream_id: Uuid) -> Option<StreamManifest> {
        self.state.load_live_stream_manifest(state, stream_id).await
    }

    pub async fn upsert_cached_stream_manifest(&self, stream_id: Uuid, manifest: StreamManifest) {
        self.state.upsert_cached_stream_manifest(stream_id, manifest).await;
    }

    pub async fn subscribe_mjpeg_feed(&self, stream_id: Uuid) -> MjpegFeedSubscription {
        self.mjpeg_feeds.clone().subscribe(stream_id).await
    }

    pub async fn mjpeg_snapshot(&self) -> RuntimeTopicBroadcastSnapshot {
        self.mjpeg_feeds.snapshot().await
    }

    pub async fn snapshot_guard(&self, stream_id: Uuid) -> Arc<Mutex<()>> {
        self.snapshot_locks.guard(stream_id).await
    }

    pub async fn release_snapshot_guard(&self, stream_id: Uuid, lock: Arc<Mutex<()>>) {
        self.snapshot_locks.release(stream_id, lock).await;
    }

    pub fn sensor_benchmark_jobs(&self) -> Arc<SensorBenchmarkJobsState> {
        self.sensor_benchmark_jobs.clone()
    }

    pub fn recording_runtime(&self) -> Arc<RecordingRuntimeState> {
        self.recording_runtime.clone()
    }

    pub fn replay_bundle_sessions(&self) -> Arc<ReplayBundleSessionsState> {
        self.replay_bundle_sessions.clone()
    }

    pub fn stream_list_cache_metrics(&self) -> ApiCacheMetric {
        self.state.stream_list_cache_metrics()
    }
}
