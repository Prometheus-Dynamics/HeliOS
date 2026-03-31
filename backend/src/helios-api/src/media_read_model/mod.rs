mod cache;
mod selection;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use serde::Deserialize;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::api_observability::ApiMediaCacheMetrics;

pub(crate) use selection::{MediaImuSelectionInput, ReplayFrameClock};

const MEDIA_IMU_CURSOR_IDLE_RESET_MS: i64 = 10_000;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MediaImuEvent {
    pub(crate) t_ms: i64,
    pub(crate) imu: lib_sensors::dto::ImuStatusPayload,
}

#[derive(Debug, Clone)]
struct MediaImuCursor {
    signature: String,
    started_at_ms: i64,
    last_seen_ms: i64,
    frame_anchor_seq: Option<u64>,
    frame_anchor_ts: Option<u64>,
    frame_anchor_elapsed_ms: i64,
    frame_ts_scale: Option<selection::MediaFrameTsScale>,
    last_frame_ts: Option<u64>,
}

#[derive(Default)]
pub struct MediaReadModelState {
    preview_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    thumbnail_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    media_imu_event_cache: Mutex<HashMap<PathBuf, Arc<Vec<MediaImuEvent>>>>,
    media_frame_timeline_cache: Mutex<HashMap<PathBuf, Arc<Vec<i64>>>>,
    media_playback_duration_cache: Mutex<HashMap<PathBuf, Option<i64>>>,
    media_imu_cursors: Mutex<HashMap<Uuid, MediaImuCursor>>,
}

impl MediaReadModelState {
    pub async fn cache_metrics(&self) -> ApiMediaCacheMetrics {
        let preview_lock_entries = self.preview_locks.lock().await.len() as u64;
        let thumbnail_lock_entries = self.thumbnail_locks.lock().await.len() as u64;
        let imu_event_entries = self.media_imu_event_cache.lock().await.len() as u64;
        let frame_timeline_entries = self.media_frame_timeline_cache.lock().await.len() as u64;
        let playback_duration_entries = self.media_playback_duration_cache.lock().await.len() as u64;
        let imu_cursor_entries = self.media_imu_cursors.lock().await.len() as u64;

        ApiMediaCacheMetrics { imu_event_entries, frame_timeline_entries, playback_duration_entries, preview_lock_entries, thumbnail_lock_entries, imu_cursor_entries }
    }

    pub async fn preview_generation_lock(&self, name: &str) -> Arc<Mutex<()>> {
        let mut locks = self.preview_locks.lock().await;
        locks.entry(name.to_string()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
    }

    pub async fn thumbnail_generation_lock(&self, name: &str) -> Arc<Mutex<()>> {
        let mut locks = self.thumbnail_locks.lock().await;
        locks.entry(name.to_string()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
    }

    pub async fn read_stream_frame_clock(&self, stream_id: Uuid) -> Option<selection::ReplayFrameClock> {
        tokio::task::spawn_blocking(move || helios_engine::stream::read_latest_header(stream_id).ok().map(|header| selection::ReplayFrameClock { seq: header.seq, ts: header.ts }))
            .await
            .ok()
            .flatten()
            .filter(|clock| clock.seq > 0 || clock.ts > 0)
    }

    pub async fn select_media_imu_event_index(&self, input: selection::MediaImuSelectionInput<'_>, events: &[MediaImuEvent]) -> Option<usize> {
        selection::select_media_imu_event_index(self, input, events).await
    }
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}
