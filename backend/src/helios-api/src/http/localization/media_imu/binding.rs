use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::http::AppState;
use crate::http::media::{MediaMetadata, load_media_metadata as load_shared_media_metadata};
use crate::http::storage;
use helios_engine::ipc::StreamSummary;

pub(super) const MEDIA_IMU_EXTERNAL_PREFIX: &str = "media-imu-";
pub(crate) const MEDIA_IMU_OUTPUT_KEY: &str = "imu_pose";
pub(crate) const MEDIA_IMU_OUTPUT_KEY_LEGACY: &str = "pose";

#[derive(Debug, Clone)]
pub(super) struct MediaImuBinding {
    pub(super) stream_id: Uuid,
    pub(super) stream_label: String,
    pub(super) loop_forever: bool,
    pub(super) playback_fps: Option<f64>,
    pub(super) media_name: String,
    pub(super) sidecar_path: PathBuf,
    pub(super) frame_ts_path: Option<PathBuf>,
    pub(super) playback_duration_ms: Option<i64>,
}

impl MediaImuBinding {
    pub(super) fn signature(&self) -> String {
        let frame_ts = self.frame_ts_path.as_ref().map(|path| path.display().to_string()).unwrap_or_default();
        format!("{}|{}|{}|{}|{:.6}|{}", self.sidecar_path.display(), self.media_name, self.loop_forever, self.playback_duration_ms.unwrap_or(0), self.playback_fps.unwrap_or(0.0), frame_ts)
    }
}

pub(super) fn parse_media_imu_stream_id(source_id: &str) -> Option<Uuid> {
    source_id.strip_prefix(MEDIA_IMU_EXTERNAL_PREFIX).and_then(|value| Uuid::parse_str(value).ok())
}

pub(super) async fn resolve_binding_for_stream(state: &AppState, stream: &StreamSummary, media_meta_dir: &Path) -> Option<MediaImuBinding> {
    let (paths, loop_forever, playback_fps) = match &stream.manifest.capture.handle {
        styx::BackendHandle::File { paths, loop_forever, fps } => (paths, *loop_forever, Some(*fps as f64)),
        _ => return None,
    };

    let stream_label = stream.manifest.identity.alias.clone().filter(|value| !value.trim().is_empty()).unwrap_or_else(|| stream.stream_id.to_string());

    for path in paths {
        for media_name in media_name_candidates(path) {
            let Some(metadata) = load_media_metadata(media_meta_dir, &media_name).await else {
                continue;
            };
            let Some(sidecar_name) = metadata.imu_data_file_name.as_deref().and_then(storage::sanitize_name) else {
                continue;
            };
            let sidecar_path = media_meta_dir.join(&sidecar_name);
            if !tokio::fs::metadata(&sidecar_path).await.ok().is_some_and(|meta| meta.is_file()) {
                continue;
            }
            let frame_ts_path = match metadata.frame_timestamps_file_name.as_deref().and_then(storage::sanitize_name) {
                Some(name) => resolve_frame_ts_path(media_meta_dir, &name).await,
                None => None,
            };
            let playback_duration_ms = state.services.media.playback_duration_ms_cached(path.clone()).await;
            return Some(MediaImuBinding {
                stream_id: stream.stream_id,
                stream_label: stream_label.clone(),
                loop_forever,
                playback_fps,
                media_name,
                sidecar_path,
                frame_ts_path,
                playback_duration_ms,
            });
        }
    }

    None
}

async fn resolve_frame_ts_path(media_meta_dir: &Path, sidecar_name: &str) -> Option<PathBuf> {
    let meta_candidate = media_meta_dir.join(sidecar_name);
    if tokio::fs::metadata(&meta_candidate).await.ok().is_some_and(|meta| meta.is_file()) {
        return Some(meta_candidate);
    }
    let media_dir = storage::ensure_subdir_async("media").await.ok()?;
    let media_candidate = media_dir.join(sidecar_name);
    tokio::fs::metadata(&media_candidate).await.ok().and_then(|meta| meta.is_file().then_some(media_candidate))
}

pub(super) fn media_name_candidates(path: &Path) -> Vec<String> {
    let Some(raw_name) = path.file_name().and_then(|value| value.to_str()) else {
        return Vec::new();
    };
    let raw_name = raw_name.trim();
    if raw_name.is_empty() {
        return Vec::new();
    }

    let mut names = Vec::new();
    if let Some(name) = storage::sanitize_name(raw_name) {
        names.push(name);
    }
    if let Some(stripped) = raw_name.strip_suffix(".replay.mp4")
        && let Some(name) = storage::sanitize_name(stripped)
        && !names.iter().any(|value| value == &name)
    {
        names.push(name);
    }
    if let Some(no_mp4) = raw_name.strip_suffix(".mp4")
        && let Some((prefix, _)) = no_mp4.rsplit_once(".replay.")
        && let Some(name) = storage::sanitize_name(prefix)
        && !names.iter().any(|value| value == &name)
    {
        names.push(name);
    }
    names
}

async fn load_media_metadata(media_meta_dir: &Path, media_name: &str) -> Option<MediaMetadata> {
    load_shared_media_metadata(media_meta_dir, media_name).await
}
