use std::sync::Arc;

use tokio::sync::{Mutex, broadcast};
use uuid::Uuid;

use crate::api_observability::{ApiCacheMetric, ApiMediaCacheMetrics};
use crate::http::peripherals::PeripheralInventory;
use crate::ws::sensors::{SensorEventsState, SharedSensorEvent, SharedSensorLatest};

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

    pub async fn bind_sensor_events_state(&self, state: &crate::http::AppState) {
        self.sensor_events.bind_state(state).await;
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
