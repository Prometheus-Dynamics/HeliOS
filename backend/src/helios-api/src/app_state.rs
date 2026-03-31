use std::ops::Deref;
use std::sync::Arc;

use axum::response::Response;
use bytes::Bytes;
use helios_engine::capture::CaptureControlInfo;
use helios_engine::ipc::{EngineErrorCode, StreamManifest};
use helios_updater::client::UpdaterSession;
use helios_updater::ipc::{PreflightReport, UpdateState, UpdaterCommand};
use serde_json::Value as JsonValue;
use tokio::process::Child;
use tokio::sync::Mutex;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::api_observability::{ApiCacheMetric, ApiMediaCacheMetrics, ApiRealtimeMetrics};
use crate::http::peers::{PeerInfo, PeerRemoteStreamsResponse, PeersServiceState};
use crate::http::peripherals::PeripheralInventory;
use crate::http::pipelines::{PipelineSummary, PlannerDiagnostic};
use crate::http::streams::types::StreamInfo;
use crate::http::streams::{mjpeg::MjpegFeedsState, snapshot::SnapshotLocksState};
use crate::ipc::IpcHandles;
use crate::ipc::updater::UpdaterConnection;
use crate::logs::LogSource;
use crate::system_read_model::{SharedDevicesUpdate, SharedProcessesSnapshot, SharedStreamMetricsSnapshot, SharedStreamOutputsEvent, SharedStreamOutputsPortsSnapshot};
use crate::ws::sensors::{SensorEventsState, SharedSensorEvent, SharedSensorLatest};

#[derive(Default)]
pub struct ApiServices {
    pub hardware: HardwareReadModelService,
    pub media: MediaReadModelService,
    pub network: NetworkService,
    pub peers: PeersService,
    pub pipelines: PipelinesReadModelService,
    pub streams: StreamsReadModelService,
    pub system: SystemReadModelService,
    pub updater: UpdaterService,
}

pub struct ApiAppState {
    ipc: Arc<IpcHandles>,
    pub services: ApiServices,
}

impl ApiAppState {
    pub fn new(ipc: Arc<IpcHandles>) -> Self {
        Self { ipc, services: ApiServices::default() }
    }

    pub fn ipc(&self) -> &Arc<IpcHandles> {
        &self.ipc
    }
}

impl Deref for ApiAppState {
    type Target = IpcHandles;

    fn deref(&self) -> &Self::Target {
        self.ipc.as_ref()
    }
}

#[derive(Clone, Default)]
pub struct HardwareReadModelService {
    state: Arc<crate::hardware_read_model::HardwareReadModelState>,
    sensor_events: Arc<SensorEventsState>,
}

impl HardwareReadModelService {
    pub async fn invalidate_peripheral_inventory_cache(&self) {
        self.state.invalidate_peripheral_inventory_cache().await;
    }

    pub async fn load_peripheral_inventory_snapshot(&self, state: &crate::http::AppState) -> Result<(PeripheralInventory, u64), String> {
        self.state.load_peripheral_inventory_snapshot(state).await
    }

    pub async fn cached_discover_cameras_snapshot(&self, state: &crate::http::AppState) -> Result<(helios_engine::capture::DiscoveryResult, u64), String> {
        self.state.cached_discover_cameras_snapshot(state).await
    }

    pub async fn allow_inventory_refresh(&self) -> bool {
        self.state.allow_inventory_refresh().await
    }

    pub fn bind_sensor_events_state(&self, state: &crate::http::AppState) {
        self.sensor_events.bind_state(state);
    }

    pub async fn subscribe_sensor_events(&self) -> (broadcast::Receiver<Arc<SharedSensorEvent>>, SharedSensorLatest) {
        self.sensor_events.subscribe().await
    }

    pub fn peripheral_inventory_cache_metrics(&self) -> ApiCacheMetric {
        self.state.peripheral_inventory_cache_metrics()
    }

    pub fn camera_discovery_cache_metrics(&self) -> ApiCacheMetric {
        self.state.camera_discovery_cache_metrics()
    }
}

#[derive(Clone, Default)]
pub struct MediaReadModelService {
    state: Arc<crate::media_read_model::MediaReadModelState>,
}

impl MediaReadModelService {
    pub async fn preview_generation_lock(&self, name: &str) -> Arc<Mutex<()>> {
        self.state.preview_generation_lock(name).await
    }

    pub async fn thumbnail_generation_lock(&self, name: &str) -> Arc<Mutex<()>> {
        self.state.thumbnail_generation_lock(name).await
    }

    pub async fn load_media_imu_events_cached(&self, path: std::path::PathBuf) -> Result<Arc<Vec<crate::media_read_model::MediaImuEvent>>, String> {
        self.state.load_media_imu_events_cached(path).await
    }

    pub async fn load_media_frame_timeline_cached(&self, path: std::path::PathBuf) -> Result<Arc<Vec<i64>>, String> {
        self.state.load_media_frame_timeline_cached(path).await
    }

    pub async fn playback_duration_ms_cached(&self, path: std::path::PathBuf) -> Option<i64> {
        self.state.playback_duration_ms_cached(path).await
    }

    pub async fn read_stream_frame_clock(&self, stream_id: Uuid) -> Option<crate::media_read_model::ReplayFrameClock> {
        self.state.read_stream_frame_clock(stream_id).await
    }

    pub async fn select_media_imu_event_index(&self, input: crate::media_read_model::MediaImuSelectionInput<'_>, events: &[crate::media_read_model::MediaImuEvent]) -> Option<usize> {
        self.state.select_media_imu_event_index(input, events).await
    }

    pub async fn cache_metrics(&self) -> ApiMediaCacheMetrics {
        self.state.cache_metrics().await
    }
}

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

    pub async fn subscribe_mjpeg_feed(&self, stream_id: Uuid) -> broadcast::Receiver<Bytes> {
        self.mjpeg_feeds.clone().subscribe(stream_id).await
    }

    pub async fn snapshot_guard(&self, stream_id: Uuid) -> Arc<Mutex<()>> {
        self.snapshot_locks.guard(stream_id).await
    }

    pub fn stream_list_cache_metrics(&self) -> ApiCacheMetric {
        self.state.stream_list_cache_metrics()
    }
}

#[derive(Clone, Default)]
pub struct NetworkService {
    state: Arc<crate::http::device::network::DeviceNetworkState>,
}

impl NetworkService {
    pub(crate) fn inner(&self) -> &crate::http::device::network::DeviceNetworkState {
        self.state.as_ref()
    }

    pub fn spawn_team_autodetect_task(&self) {
        self.state.clone().spawn_team_autodetect_task();
    }
}

#[derive(Clone, Default)]
pub struct PeersService {
    state: Arc<PeersServiceState>,
}

impl PeersService {
    pub(crate) fn inner(&self) -> &PeersServiceState {
        self.state.as_ref()
    }

    pub async fn init_from_disk(&self) {
        self.state.init_from_disk().await;
    }

    pub async fn snapshot_peers(&self) -> Vec<PeerInfo> {
        self.state.snapshot_peers().await
    }

    pub async fn snapshot_peer_streams(&self, state: &crate::http::AppState) -> PeerRemoteStreamsResponse {
        self.state.snapshot_peer_streams(state).await
    }
}

#[derive(Clone, Default)]
pub struct SystemReadModelService {
    state: Arc<crate::system_read_model::SystemReadModelState>,
}

impl SystemReadModelService {
    pub async fn load_device_metrics_snapshot(&self) -> Result<(crate::http::device::metrics::DeviceMetrics, u64), String> {
        self.state.load_device_metrics_snapshot().await
    }

    pub fn bind_telemetry_state(&self, state: &crate::http::AppState) {
        self.state.bind_telemetry_state(state);
    }

    pub async fn subscribe_telemetry_payloads(&self) -> (broadcast::Receiver<Arc<str>>, Option<Arc<str>>) {
        self.state.subscribe_telemetry_payloads().await
    }

    pub fn bind_devices_updates_state(&self, state: &crate::http::AppState) {
        self.state.bind_devices_updates_state(state);
    }

    pub async fn subscribe_devices_updates(&self) -> broadcast::Receiver<Arc<SharedDevicesUpdate>> {
        self.state.subscribe_devices_updates().await
    }

    pub fn bind_stream_metrics_state(&self, state: &crate::http::AppState) {
        self.state.bind_stream_metrics_state(state);
    }

    pub async fn subscribe_stream_metrics(&self, stream_id: Uuid) -> Result<(broadcast::Receiver<Arc<SharedStreamMetricsSnapshot>>, Option<Arc<SharedStreamMetricsSnapshot>>), String> {
        self.state.subscribe_stream_metrics(stream_id).await
    }

    pub async fn unsubscribe_stream_metrics(&self, stream_id: Uuid) {
        self.state.unsubscribe_stream_metrics(stream_id).await;
    }

    pub fn bind_stream_outputs_state(&self, state: &crate::http::AppState) {
        self.state.bind_stream_outputs_state(state);
    }

    pub async fn subscribe_stream_outputs(
        &self,
        stream_id: Uuid,
        default_interval: std::time::Duration,
        ports_interval: std::time::Duration,
    ) -> Result<(Uuid, broadcast::Receiver<Arc<SharedStreamOutputsEvent>>, Arc<SharedStreamOutputsPortsSnapshot>), String> {
        self.state.subscribe_stream_outputs(stream_id, default_interval, ports_interval).await
    }

    pub async fn update_stream_outputs_subscription(&self, stream_id: Uuid, client_id: Uuid, ports: Vec<String>, sample_interval: std::time::Duration) -> Result<(), String> {
        self.state.update_stream_outputs_subscription(stream_id, client_id, ports, sample_interval).await
    }

    pub async fn current_stream_outputs_ports(&self, stream_id: Uuid) -> Result<Arc<SharedStreamOutputsPortsSnapshot>, String> {
        self.state.current_stream_outputs_ports(stream_id).await
    }

    pub async fn unsubscribe_stream_outputs(&self, stream_id: Uuid, client_id: Uuid) {
        self.state.unsubscribe_stream_outputs(stream_id, client_id).await;
    }

    pub async fn load_log_sources_snapshot(&self) -> (Vec<LogSource>, u64) {
        self.state.load_log_sources_snapshot().await
    }

    pub async fn resolve_log_source(&self, source_id: &str) -> Option<LogSource> {
        self.state.resolve_log_source(source_id).await
    }

    pub async fn read_log_lines(&self, source_id: &str, lines: u64) -> Result<Vec<String>, String> {
        self.state.read_log_lines(source_id, lines).await
    }

    pub fn spawn_log_download(&self, source: &LogSource, lines: Option<usize>) -> Result<(Child, String), String> {
        self.state.spawn_log_download(source, lines)
    }

    pub fn spawn_log_stream(&self, source: &LogSource, lines: usize, follow: bool) -> Result<Child, String> {
        self.state.spawn_log_stream(source, lines, follow)
    }

    pub async fn subscribe_process_snapshots(&self) -> (broadcast::Receiver<Arc<SharedProcessesSnapshot>>, Option<Arc<SharedProcessesSnapshot>>) {
        self.state.subscribe_process_snapshots().await
    }

    pub fn device_metrics_cache_metrics(&self) -> ApiCacheMetric {
        self.state.device_metrics_cache_metrics()
    }

    pub fn log_sources_cache_metrics(&self) -> ApiCacheMetric {
        self.state.log_sources_cache_metrics()
    }

    pub async fn realtime_metrics(&self) -> ApiRealtimeMetrics {
        self.state.realtime_metrics().await
    }
}

#[derive(Default)]
pub struct UpdaterService;

impl UpdaterService {
    pub async fn ensure_updater(&self, state: &crate::http::AppState) -> Result<Arc<UpdaterConnection>, String> {
        crate::updater_service::ensure_updater(state).await
    }

    pub async fn invalidate_updater(&self, state: &crate::http::AppState, conn: &Arc<UpdaterConnection>) {
        crate::updater_service::invalidate_updater(state, conn).await;
    }

    pub async fn send_query_state(&self, conn: &UpdaterConnection, session: &mut UpdaterSession, label: &str) -> Result<(), String> {
        crate::updater_service::send_query_state(conn, session, label).await
    }

    pub async fn send_updater_command(&self, state: &crate::http::AppState, command: UpdaterCommand, wait_for_ack: bool) -> Result<(), String> {
        crate::updater_service::send_updater_command(state, command, wait_for_ack).await
    }

    pub async fn fetch_updater_state(&self, state: &crate::http::AppState) -> Result<(Option<UpdateState>, u64), String> {
        crate::updater_service::fetch_updater_state(state).await
    }

    pub async fn fetch_updater_preflight(&self, state: &crate::http::AppState, update_id: Uuid) -> Result<PreflightReport, String> {
        crate::updater_service::fetch_updater_preflight(state, update_id).await
    }
}
