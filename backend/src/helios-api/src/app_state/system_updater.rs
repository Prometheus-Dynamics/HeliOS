use std::sync::Arc;

use tokio::process::Child;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::api_observability::{ApiCacheMetric, ApiRealtimeDiagnostics, ApiRealtimeMetrics};
use crate::ipc::updater::UpdaterConnection;
use crate::logs::LogSource;
use crate::system_read_model::{ReadModelSnapshot, SharedDevicesUpdate, SharedProcessesSnapshot, SharedStreamMetricsSnapshot, SharedStreamOutputsEvent, SharedStreamOutputsPortsSnapshot};
use helios_updater::client::UpdaterSession;
use helios_updater::ipc::{PreflightReport, UpdateState, UpdaterCommand, UpdaterStorageReport};

#[derive(Clone, Default)]
pub struct SystemReadModelService {
    state: Arc<crate::system_read_model::SystemReadModelState>,
}

impl SystemReadModelService {
    pub async fn load_device_metrics_snapshot(&self) -> ReadModelSnapshot<crate::http::device::metrics::DeviceMetrics> {
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

    pub async fn bind_stream_metrics_state(&self, state: &crate::http::AppState) {
        self.state.bind_stream_metrics_state(state).await;
    }

    pub async fn subscribe_stream_metrics(&self, stream_id: Uuid) -> Result<(broadcast::Receiver<Arc<SharedStreamMetricsSnapshot>>, Option<Arc<SharedStreamMetricsSnapshot>>), String> {
        self.state.subscribe_stream_metrics(stream_id).await
    }

    pub async fn unsubscribe_stream_metrics(&self, stream_id: Uuid) {
        self.state.unsubscribe_stream_metrics(stream_id).await;
    }

    pub async fn bind_stream_outputs_state(&self, state: &crate::http::AppState) {
        self.state.bind_stream_outputs_state(state).await;
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

    pub async fn load_log_sources_snapshot(&self) -> ReadModelSnapshot<Vec<LogSource>> {
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

    pub async fn realtime_diagnostics(&self) -> ApiRealtimeDiagnostics {
        self.state.realtime_diagnostics().await
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

    pub async fn fetch_updater_storage(&self, state: &crate::http::AppState) -> Result<UpdaterStorageReport, String> {
        crate::updater_service::fetch_updater_storage(state).await
    }

    pub async fn fetch_updater_preflight(&self, state: &crate::http::AppState, update_id: Uuid) -> Result<PreflightReport, String> {
        crate::updater_service::fetch_updater_preflight(state, update_id).await
    }
}
