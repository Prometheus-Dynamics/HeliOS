use crate::api_observability::{ApiCacheMetric, CacheMetricCounters};
use crate::http::AppState;
use crate::http::streams::lifecycle::ensure_descriptor_has_mode;
use crate::http::streams::types::StreamInfo;
use crate::http::streams::util::{apply_effective_pipeline_layout, build_stream_info, list_streams_timeout, normalize_pipeline_manifest};
use crate::http::streams_persist;
use helios_engine::capture::CaptureControlInfo;
use helios_engine::ipc::{StreamManifest, StreamSummary};
use lib_runtime_policy::HELIOS_API_STREAMS_POLICY;
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
    Duration::from_millis(HELIOS_API_STREAMS_POLICY.resolve().cache_ms)
}

fn controls_cache_ttl() -> Duration {
    Duration::from_secs(30)
}

fn stream_info_from_summary(StreamSummary { stream_id, mut descriptor, manifest, status, runtime }: StreamSummary) -> StreamInfo {
    let mut requested = manifest.to_requested_manifest();
    normalize_pipeline_manifest(&mut requested);
    apply_effective_pipeline_layout(&mut requested);
    ensure_descriptor_has_mode(&mut descriptor, &requested);
    build_stream_info(stream_id, descriptor, manifest, Some(status), Some(runtime))
}

fn merge_persisted_streams(mut active: Vec<StreamInfo>, persisted: Vec<streams_persist::PersistedStreamRecord>) -> Vec<StreamInfo> {
    let mut seen_ids: BTreeSet<Uuid> = active.iter().map(|stream| stream.id).collect();
    let mut persisted_ids = BTreeSet::new();
    let mut pose_by_stream = HashMap::new();

    for record in &persisted {
        let Some(resolved) = record.resolved_config.as_ref() else {
            continue;
        };
        if resolved.internal {
            continue;
        }
        let stream_id = record.stream_id().unwrap_or_else(|| streams_persist::derived_stream_id(&record.camera_id));
        if let Some(pose) = resolved.pose.clone() {
            pose_by_stream.insert(stream_id, pose);
        }
    }

    for stream in &mut active {
        if stream.manifest.pose.is_none()
            && let Some(pose) = pose_by_stream.get(&stream.id).cloned()
        {
            stream.manifest.pose = Some(pose);
            stream.resolved.pose = stream.manifest.pose.clone();
        }
    }

    for record in persisted {
        let stream_id = record.stream_id().unwrap_or_else(|| streams_persist::derived_stream_id(&record.camera_id));
        let descriptor = streams_persist::descriptor_snapshot_for_record(&record);
        let Some(mut resolved) = record.resolved_config else {
            continue;
        };
        if resolved.internal {
            continue;
        }
        if !persisted_ids.insert(stream_id) {
            tracing::warn!(camera_id = %record.camera_id, stream_id = %stream_id, "duplicate persisted stream id; keeping first record");
            continue;
        }
        if seen_ids.contains(&stream_id) {
            tracing::debug!(camera_id = %record.camera_id, stream_id = %stream_id, "persisted stream already running; skipping");
            continue;
        }
        resolved.identity.id = Some(stream_id);
        let mut manifest = resolved.to_requested_manifest();
        normalize_pipeline_manifest(&mut manifest);
        apply_effective_pipeline_layout(&mut manifest);
        let descriptor = descriptor.unwrap_or_else(|| streams_persist::synthesize_descriptor_snapshot_from_manifest(&manifest));
        active.push(build_stream_info(stream_id, descriptor, resolved, None, None));
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
        let resolved = manifest.resolve();
        stream.manifest = manifest;
        stream.resolved = resolved;
        if let Some(runtime) = stream.runtime.as_mut() {
            runtime.pipeline.enabled = stream.resolved.pipeline_enabled;
            runtime.pipeline.active_pipeline_id = stream.resolved.active_pipeline_id;
            runtime.pipeline.active_output_key = stream.resolved.active_pipeline_output.clone();
            runtime.pipeline.pipeline_count = stream.resolved.pipelines.len() as u64;
        }
    }

    pub async fn load_live_stream_manifest(&self, state: &AppState, stream_id: Uuid) -> Option<StreamManifest> {
        if let Some(stream) = self.cached_stream_info(stream_id).await {
            return Some(stream.manifest);
        }
        state.engine.list_streams().await.ok().and_then(|streams| streams.into_iter().find(|stream| stream.stream_id == stream_id).map(|stream| stream.manifest.to_requested_manifest()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use helios_engine::capture::CaptureMode;
    use serde_json::json;
    use styx::prelude::{ColorSpace, FourCc, MediaFormat, Resolution};

    fn sample_manifest() -> StreamManifest {
        serde_json::from_value(json!({
            "schema_version": 1,
            "identity": {},
            "capture": {
                "device_keys": [],
                "backend": "Virtual",
                "handle": { "type": "virtual" },
                "mode": {
                    "format": {
                        "code": "RGB3",
                        "resolution": { "width": 1, "height": 1 },
                        "color": "Srgb"
                    },
                    "interval": null
                },
                "controls": []
            },
            "pose": {
                "translation": { "x": 1.0, "y": 2.0, "z": 3.0 },
                "rotation": { "roll": 4.0, "pitch": 5.0, "yaw": 6.0 },
                "updated_at": "2026-03-31T00:00:00Z"
            }
        }))
        .expect("decode sample manifest")
    }

    #[test]
    fn merge_persisted_streams_reconstructs_requested_manifest_from_resolved_only_record() {
        let stream_id = Uuid::new_v4();
        let manifest = sample_manifest();
        let expected_manifest = {
            let mut expected = manifest.clone().resolve().to_requested_manifest();
            expected.identity.id = Some(stream_id);
            expected
        };
        let persisted = vec![streams_persist::PersistedStreamRecord {
            camera_id: "virtual-camera".to_string(),
            last_stream_id: Some(stream_id),
            resolved_config: Some(manifest.clone().resolve()),
            ..Default::default()
        }];

        let merged = merge_persisted_streams(Vec::new(), persisted);
        assert_eq!(merged.len(), 1);

        let stream = &merged[0];
        assert_eq!(stream.id, stream_id);
        assert!(stream.status.is_none());
        assert_eq!(stream.manifest.identity.id, Some(stream_id));
        assert_eq!(stream.resolved.identity.id, Some(stream_id));
        assert_eq!(serde_json::to_value(&stream.manifest).expect("encode merged manifest"), serde_json::to_value(expected_manifest).expect("encode expected manifest"));
    }

    #[test]
    fn merge_persisted_streams_prefers_persisted_descriptor_snapshot() {
        let stream_id = Uuid::new_v4();
        let manifest = sample_manifest();
        let alternate_format = MediaFormat::new(FourCc::new(*b"NV12"), Resolution::new(2, 2).unwrap(), ColorSpace::Srgb);
        let snapshot = helios_engine::capture::CaptureDescriptor {
            modes: vec![
                CaptureMode { id: manifest.capture.mode.clone(), format: manifest.capture.mode.format, intervals: Default::default(), interval_stepwise: None },
                CaptureMode { id: helios_engine::capture::ModeId { format: alternate_format, interval: None }, format: alternate_format, intervals: Default::default(), interval_stepwise: None },
            ],
            controls: Vec::new(),
        };
        let persisted = vec![streams_persist::PersistedStreamRecord {
            camera_id: "virtual-camera".to_string(),
            last_stream_id: Some(stream_id),
            resolved_config: Some(manifest.resolve()),
            descriptor_snapshot: Some(snapshot),
            ..Default::default()
        }];

        let merged = merge_persisted_streams(Vec::new(), persisted);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].descriptor.modes.len(), 2);
    }
}
