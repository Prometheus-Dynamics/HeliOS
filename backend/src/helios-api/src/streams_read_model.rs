use crate::api_observability::{ApiCacheMetric, CacheMetricCounters};
use crate::http::AppState;
use crate::http::streams::lifecycle::{descriptor_from_persisted_manifest, ensure_descriptor_has_mode};
use crate::http::streams::types::StreamInfo;
use crate::http::streams::util::{apply_effective_pipeline_layout, build_stream_info, list_streams_timeout, normalize_pipeline_manifest};
use crate::http::streams_persist;
use helios_engine::capture::CaptureControlInfo;
use helios_engine::ipc::{StreamManifest, StreamSummary};
use std::collections::{BTreeSet, HashMap};
use std::time::Instant;
use tokio::time::Duration;
use uuid::Uuid;

#[derive(Clone)]
struct StreamListCacheEntry {
    fetched_at: Instant,
    revision: u64,
    payload: Vec<StreamInfo>,
}

#[derive(Clone)]
struct ControlCacheEntry {
    fetched_at: Instant,
    controls: Vec<CaptureControlInfo>,
}

#[derive(Default)]
pub struct StreamsReadModelState {
    stream_list_cache: tokio::sync::RwLock<Option<StreamListCacheEntry>>,
    stream_start_lock: tokio::sync::Mutex<()>,
    stream_controls_cache: std::sync::RwLock<HashMap<Uuid, ControlCacheEntry>>,
    stream_list_stats: CacheMetricCounters,
}

fn stream_list_cache_ttl() -> Duration {
    const DEFAULT_MS: u64 = 750;
    const MIN_MS: u64 = 0;
    const MAX_MS: u64 = 5_000;

    let ms = std::env::var("HELIOS_API_STREAMS_CACHE_MS").ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(DEFAULT_MS);

    Duration::from_millis(ms.clamp(MIN_MS, MAX_MS))
}

fn controls_cache_ttl() -> Duration {
    Duration::from_secs(30)
}

fn stream_info_from_summary(StreamSummary { stream_id, mut descriptor, mut manifest, status }: StreamSummary) -> StreamInfo {
    normalize_pipeline_manifest(&mut manifest);
    apply_effective_pipeline_layout(&mut manifest);
    ensure_descriptor_has_mode(&mut descriptor, &manifest);
    build_stream_info(stream_id, descriptor, manifest, Some(status))
}

fn merge_persisted_streams(mut active: Vec<StreamInfo>, persisted: Vec<streams_persist::PersistedStreamRecord>) -> Vec<StreamInfo> {
    let mut seen_ids: BTreeSet<Uuid> = active.iter().map(|stream| stream.id).collect();
    let mut persisted_ids = BTreeSet::new();
    let mut pose_by_stream = HashMap::new();

    for record in &persisted {
        let Some(manifest) = record.manifest.as_ref() else {
            continue;
        };
        if manifest.internal {
            continue;
        }
        let stream_id = manifest.identity.id.or(record.last_stream_id).unwrap_or_else(|| streams_persist::derived_stream_id(&record.camera_id));
        if let Some(pose) = manifest.pose.clone() {
            pose_by_stream.insert(stream_id, pose);
        }
    }

    for stream in &mut active {
        if stream.manifest.pose.is_none()
            && let Some(pose) = pose_by_stream.get(&stream.id).cloned()
        {
            stream.manifest.pose = Some(pose);
        }
    }

    for record in persisted {
        let Some(mut manifest) = record.manifest else {
            continue;
        };
        if manifest.internal {
            continue;
        }
        let stream_id = manifest.identity.id.or(record.last_stream_id).unwrap_or_else(|| streams_persist::derived_stream_id(&record.camera_id));
        if !persisted_ids.insert(stream_id) {
            tracing::warn!(camera_id = %record.camera_id, stream_id = %stream_id, "duplicate persisted stream id; keeping first record");
            continue;
        }
        if seen_ids.contains(&stream_id) {
            tracing::debug!(camera_id = %record.camera_id, stream_id = %stream_id, "persisted stream already running; skipping");
            continue;
        }
        manifest.identity.id = Some(stream_id);
        normalize_pipeline_manifest(&mut manifest);
        apply_effective_pipeline_layout(&mut manifest);
        let descriptor = descriptor_from_persisted_manifest(&manifest);
        active.push(build_stream_info(stream_id, descriptor, manifest, None));
        seen_ids.insert(stream_id);
    }

    active
}

impl StreamsReadModelState {
    pub fn stream_list_cache_metrics(&self) -> ApiCacheMetric {
        self.stream_list_stats.snapshot()
    }

    pub async fn get_cached_streams_snapshot_with_revision(&self, state: &AppState) -> (Vec<StreamInfo>, bool, u64) {
        let ttl = stream_list_cache_ttl();
        if ttl != Duration::from_millis(0)
            && let Some(entry) = self.stream_list_cache.read().await.clone()
            && entry.fetched_at.elapsed() < ttl
        {
            self.stream_list_stats.record_hit();
            return (entry.payload, false, entry.revision);
        }

        self.stream_list_stats.record_miss();
        let mut stale = false;
        let active = match state.engine.list_streams_with_timeout(list_streams_timeout()).await {
            Ok(streams) => streams.into_iter().filter(|stream| !stream.manifest.internal).map(stream_info_from_summary).collect(),
            Err(err) => {
                tracing::warn!(error = %err, "engine list_streams timed out");
                stale = true;
                if let Some(entry) = self.stream_list_cache.read().await.clone() {
                    self.stream_list_stats.record_stale_fallback();
                    return (entry.payload, true, entry.revision);
                }
                Vec::new()
            }
        };

        let payload = merge_persisted_streams(active, streams_persist::list_persisted_records().await);
        let revision = self.stream_list_stats.record_refresh();
        *self.stream_list_cache.write().await = Some(StreamListCacheEntry { fetched_at: Instant::now(), revision, payload: payload.clone() });
        (payload, stale, revision)
    }

    pub async fn invalidate_stream_list_cache(&self) {
        *self.stream_list_cache.write().await = None;
    }

    pub async fn stream_start_guard(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.stream_start_lock.lock().await
    }

    pub fn cache_controls(&self, stream_id: Uuid, controls: &[CaptureControlInfo]) {
        let Ok(mut guard) = self.stream_controls_cache.write() else {
            return;
        };
        guard.insert(stream_id, ControlCacheEntry { fetched_at: Instant::now(), controls: controls.to_vec() });
    }

    pub fn cached_controls(&self, stream_id: Uuid) -> Option<Vec<CaptureControlInfo>> {
        let Ok(guard) = self.stream_controls_cache.read() else {
            return None;
        };
        let entry = guard.get(&stream_id)?;
        if entry.fetched_at.elapsed() > controls_cache_ttl() {
            return None;
        }
        Some(entry.controls.clone())
    }

    pub async fn cached_stream_info(&self, stream_id: Uuid) -> Option<StreamInfo> {
        self.stream_list_cache.read().await.as_ref()?.payload.iter().find(|stream| stream.id == stream_id).cloned()
    }

    pub async fn upsert_cached_stream_manifest(&self, stream_id: Uuid, manifest: StreamManifest) {
        let mut guard = self.stream_list_cache.write().await;
        let Some(entry) = guard.as_mut() else {
            return;
        };
        let Some(stream) = entry.payload.iter_mut().find(|stream| stream.id == stream_id) else {
            return;
        };
        stream.manifest = manifest;
    }

    pub async fn load_live_stream_manifest(&self, state: &AppState, stream_id: Uuid) -> Option<StreamManifest> {
        if let Some(stream) = self.cached_stream_info(stream_id).await {
            return Some(stream.manifest);
        }
        state.engine.list_streams().await.ok().and_then(|streams| streams.into_iter().find(|stream| stream.stream_id == stream_id).map(|stream| stream.manifest))
    }
}
