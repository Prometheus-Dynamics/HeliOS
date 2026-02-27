use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use chrono::Utc;
use flate2::read::GzDecoder;
use serde::Deserialize;
use serde_json::{Map as JsonMap, Value as JsonValue};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::http::AppState;
use crate::http::error::{ApiError, ApiResult};
use crate::http::media::MediaMetadata;
use crate::http::storage;
use helios_engine::ipc::StreamSummary;
use helios_engine::localization::types::{LocalizationPipelineSource, PipelineOutputSample};

const MEDIA_IMU_EXTERNAL_PREFIX: &str = "media-imu-";
pub(crate) const MEDIA_IMU_OUTPUT_KEY: &str = "imu_pose";
pub(crate) const MEDIA_IMU_OUTPUT_KEY_LEGACY: &str = "pose";
const MEDIA_IMU_CURSOR_IDLE_RESET_MS: i64 = 10_000;

#[derive(Debug, Clone)]
struct MediaImuBinding {
    stream_id: Uuid,
    stream_label: String,
    loop_forever: bool,
    playback_fps: Option<f64>,
    media_name: String,
    sidecar_path: PathBuf,
    frame_ts_path: Option<PathBuf>,
    playback_duration_ms: Option<i64>,
}

impl MediaImuBinding {
    fn signature(&self) -> String {
        let frame_ts = self.frame_ts_path.as_ref().map(|path| path.display().to_string()).unwrap_or_default();
        format!("{}|{}|{}|{}|{:.6}|{}", self.sidecar_path.display(), self.media_name, self.loop_forever, self.playback_duration_ms.unwrap_or(0), self.playback_fps.unwrap_or(0.0), frame_ts)
    }
}

#[derive(Debug, Clone, Deserialize)]
struct MediaImuEvent {
    t_ms: i64,
    imu: lib_sensors::dto::ImuStatusPayload,
}

#[derive(Debug, Clone)]
struct MediaImuCursor {
    signature: String,
    started_at_ms: i64,
    last_seen_ms: i64,
    frame_anchor_seq: Option<u64>,
    frame_anchor_ts: Option<u64>,
    frame_anchor_elapsed_ms: i64,
    frame_ts_scale: Option<MediaFrameTsScale>,
    last_frame_ts: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
struct ReplayFrameClock {
    seq: u64,
    ts: u64,
}

#[derive(Debug, Clone, Copy)]
enum MediaFrameTsScale {
    Millis,
    Pts90k,
    Micros,
    Nanos,
}

fn media_imu_event_cache() -> &'static Mutex<HashMap<PathBuf, Arc<Vec<MediaImuEvent>>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<Vec<MediaImuEvent>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn media_frame_timeline_cache() -> &'static Mutex<HashMap<PathBuf, Arc<Vec<i64>>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<Vec<i64>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn media_playback_duration_cache() -> &'static Mutex<HashMap<PathBuf, Option<i64>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Option<i64>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn media_imu_cursors() -> &'static Mutex<HashMap<Uuid, MediaImuCursor>> {
    static CURSORS: OnceLock<Mutex<HashMap<Uuid, MediaImuCursor>>> = OnceLock::new();
    CURSORS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn is_media_imu_source_id(source_id: &str) -> bool {
    parse_media_imu_stream_id(source_id).is_some()
}

pub(crate) async fn source_for_stream(stream: &StreamSummary, media_meta_dir: &Path) -> Option<LocalizationPipelineSource> {
    let binding = resolve_binding_for_stream(stream, media_meta_dir).await?;
    let stream_id = binding.stream_id.to_string();
    let camera_uid = stream.manifest.identity.hardware_id.clone().or(stream.manifest.identity.alias.clone()).unwrap_or_else(|| stream_id.clone());
    let camera_path = match &stream.manifest.capture.handle {
        styx::BackendHandle::V4l2 { path } => path.clone(),
        styx::BackendHandle::Libcamera { id } => id.clone(),
        styx::BackendHandle::Netcam { url, .. } => url.clone(),
        styx::BackendHandle::File { paths, .. } => paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(","),
        other => format!("{other:?}"),
    };
    Some(LocalizationPipelineSource {
        id: format!("{stream_id}:{MEDIA_IMU_OUTPUT_KEY}"),
        stream_id: stream_id.clone(),
        stream_label: binding.stream_label,
        camera_uid,
        camera_path,
        pipeline_id: "media-imu".to_string(),
        pipeline_label: "Media IMU".to_string(),
        output_key: MEDIA_IMU_OUTPUT_KEY.to_string(),
        data_type: None,
    })
}

pub(crate) async fn fetch_media_imu_sample_for_stream(state: &AppState, stream_id: Uuid, output_key: &str) -> ApiResult<PipelineOutputSample> {
    let source_id = format!("{MEDIA_IMU_EXTERNAL_PREFIX}{stream_id}");
    fetch_media_imu_sample(state, &source_id, output_key).await
}

pub(crate) async fn fetch_media_imu_sample(state: &AppState, source_id: &str, output_key: &str) -> ApiResult<PipelineOutputSample> {
    let output = output_key.trim();
    if !output.eq_ignore_ascii_case(MEDIA_IMU_OUTPUT_KEY) && !output.eq_ignore_ascii_case(MEDIA_IMU_OUTPUT_KEY_LEGACY) {
        return Err(ApiError::bad_request("unsupported output key"));
    }

    let Some(stream_id) = parse_media_imu_stream_id(source_id) else {
        return Err(ApiError::not_found("external source not found"));
    };

    let streams = state.engine.list_streams().await.map_err(|err| ApiError::bad_gateway(err.to_string()))?;
    let Some(stream) = streams.into_iter().find(|entry| entry.stream_id == stream_id) else {
        return Err(ApiError::not_found("stream not found"));
    };

    let media_meta_dir = storage::ensure_subdir_async("media-meta").await.map_err(|err| ApiError::bad_gateway(format!("failed to access media metadata: {err}")))?;
    let Some(binding) = resolve_binding_for_stream(&stream, &media_meta_dir).await else {
        return Err(ApiError::not_found("IMU sidecar not found"));
    };

    let events = load_events_cached(binding.sidecar_path.clone()).await.map_err(|err| ApiError::bad_gateway(format!("failed to read IMU sidecar: {err}")))?;
    let frame_timeline = match binding.frame_ts_path.clone() {
        Some(path) => match load_frame_timeline_cached(path).await {
            Ok(values) => Some(values),
            Err(err) => {
                tracing::warn!(stream_id = %binding.stream_id, error = %err, "failed to read frame timestamp sidecar");
                None
            }
        },
        None => None,
    };
    let stream_started_at_ms = stream.status.started_at_ms.map(|value| value as i64).filter(|value| *value > 0);
    let replay_frame_clock = read_stream_frame_clock(binding.stream_id).await;
    let Some(idx) = select_event_index(
        binding.stream_id,
        &binding.signature(),
        binding.loop_forever,
        binding.playback_fps,
        stream_started_at_ms,
        binding.playback_duration_ms,
        replay_frame_clock,
        frame_timeline.as_deref().map(Vec::as_slice),
        events.as_ref(),
    )
    .await
    else {
        return Err(ApiError::not_found("no IMU samples available"));
    };
    let Some(sample) = imu_status_to_pose_sample(&events[idx].imu, events[idx].t_ms) else {
        return Err(ApiError::not_found("IMU orientation unavailable"));
    };
    Ok(sample)
}

fn parse_media_imu_stream_id(source_id: &str) -> Option<Uuid> {
    source_id.strip_prefix(MEDIA_IMU_EXTERNAL_PREFIX).and_then(|value| Uuid::parse_str(value).ok())
}

async fn resolve_binding_for_stream(stream: &StreamSummary, media_meta_dir: &Path) -> Option<MediaImuBinding> {
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
            let playback_duration_ms = playback_duration_ms_cached(path.clone()).await;
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

fn media_name_candidates(path: &Path) -> Vec<String> {
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
    let bytes = tokio::fs::read(media_meta_dir.join(format!("{media_name}.json"))).await.ok()?;
    serde_json::from_slice(&bytes).ok()
}

async fn load_events_cached(path: PathBuf) -> Result<Arc<Vec<MediaImuEvent>>, String> {
    {
        let cache = media_imu_event_cache().lock().await;
        if let Some(cached) = cache.get(&path) {
            return Ok(cached.clone());
        }
    }

    let path_for_load = path.clone();
    let loaded = tokio::task::spawn_blocking(move || load_events_sync(&path_for_load)).await.map_err(|_| "IMU sidecar parser task failed".to_string())??;
    let loaded = Arc::new(loaded);

    let mut cache = media_imu_event_cache().lock().await;
    let entry = cache.entry(path).or_insert_with(|| loaded.clone());
    Ok(entry.clone())
}

async fn load_frame_timeline_cached(path: PathBuf) -> Result<Arc<Vec<i64>>, String> {
    {
        let cache = media_frame_timeline_cache().lock().await;
        if let Some(cached) = cache.get(&path) {
            return Ok(cached.clone());
        }
    }

    let path_for_load = path.clone();
    let loaded = tokio::task::spawn_blocking(move || load_frame_timeline_sync(&path_for_load)).await.map_err(|_| "frame timestamp parser task failed".to_string())??;
    let loaded = Arc::new(loaded);

    let mut cache = media_frame_timeline_cache().lock().await;
    let entry = cache.entry(path).or_insert_with(|| loaded.clone());
    Ok(entry.clone())
}

async fn playback_duration_ms_cached(path: PathBuf) -> Option<i64> {
    {
        let cache = media_playback_duration_cache().lock().await;
        if let Some(cached) = cache.get(&path) {
            return *cached;
        }
    }

    let path_for_probe = path.clone();
    let probed = tokio::task::spawn_blocking(move || probe_media_duration_ms_sync(&path_for_probe)).await.ok().flatten().filter(|value| *value > 0);

    let mut cache = media_playback_duration_cache().lock().await;
    cache.insert(path, probed);
    probed
}

fn load_events_sync(path: &Path) -> Result<Vec<MediaImuEvent>, String> {
    let file = std::fs::File::open(path).map_err(|err| format!("open failed: {err}"))?;
    let decoder = GzDecoder::new(file);
    let reader = BufReader::new(decoder);
    let mut events = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|err| format!("read failed: {err}"))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(event) = serde_json::from_str::<MediaImuEvent>(trimmed) {
            events.push(event);
        }
    }
    events.sort_by_key(|event| event.t_ms);
    Ok(events)
}

fn load_frame_timeline_sync(path: &Path) -> Result<Vec<i64>, String> {
    let file = std::fs::File::open(path).map_err(|err| format!("open failed: {err}"))?;
    let reader = BufReader::new(file);
    let mut raw_values = Vec::new();
    let mut last_raw = 0u64;
    for line in reader.lines() {
        let line = line.map_err(|err| format!("read failed: {err}"))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(raw) = trimmed.parse::<u64>() else {
            continue;
        };
        let clamped = raw.max(last_raw);
        raw_values.push(clamped);
        last_raw = clamped;
    }
    if raw_values.is_empty() {
        return Err("frame timestamp sidecar had no valid samples".to_string());
    }

    let base = *raw_values.first().unwrap_or(&0);
    let scale = raw_values.windows(2).find_map(|window| {
        let [prev, next] = [window[0], window[1]];
        (next > prev).then(|| infer_frame_ts_scale(prev, next))
    });
    let mut values = Vec::with_capacity(raw_values.len());
    let mut last = 0i64;
    for raw in raw_values {
        let delta = raw.saturating_sub(base);
        let clamped = convert_frame_delta_ms(delta, scale).max(last);
        values.push(clamped);
        last = clamped;
    }
    Ok(values)
}

fn probe_media_duration_ms_sync(path: &Path) -> Option<i64> {
    let output =
        std::process::Command::new("ffprobe").arg("-v").arg("error").arg("-show_entries").arg("format=duration").arg("-of").arg("default=noprint_wrappers=1:nokey=1").arg(path).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let secs = text.trim().parse::<f64>().ok()?;
    if !secs.is_finite() || secs <= 0.0 {
        return None;
    }
    Some((secs * 1000.0).round() as i64)
}

async fn read_stream_frame_clock(stream_id: Uuid) -> Option<ReplayFrameClock> {
    tokio::task::spawn_blocking(move || helios_engine::stream::read_latest_header(stream_id).ok().map(|header| ReplayFrameClock { seq: header.seq, ts: header.ts }))
        .await
        .ok()
        .flatten()
        .filter(|clock| clock.seq > 0 || clock.ts > 0)
}

fn normalize_frame_delta_ms(delta: u64) -> i64 {
    let delta_ms = if delta >= 1_000_000_000 {
        delta / 1_000_000
    } else if delta >= 1_000_000 {
        delta / 1_000
    } else {
        delta
    };
    delta_ms.min(i64::MAX as u64) as i64
}

fn convert_frame_delta_ms(delta: u64, scale: Option<MediaFrameTsScale>) -> i64 {
    let delta_ms = match scale {
        Some(MediaFrameTsScale::Millis) => delta,
        Some(MediaFrameTsScale::Pts90k) => (delta.saturating_mul(1_000)) / 90_000,
        Some(MediaFrameTsScale::Micros) => delta / 1_000,
        Some(MediaFrameTsScale::Nanos) => delta / 1_000_000,
        None => return normalize_frame_delta_ms(delta),
    };
    delta_ms.min(i64::MAX as u64) as i64
}

fn infer_frame_ts_scale(prev: u64, next: u64) -> MediaFrameTsScale {
    let step = next.saturating_sub(prev);
    if (500..=6_000).contains(&step) {
        // Some demuxers expose video PTS in 90kHz ticks.
        return MediaFrameTsScale::Pts90k;
    } else if (6_000..=2_000_000).contains(&step) {
        return MediaFrameTsScale::Micros;
    } else if (2_000_000..=5_000_000_000).contains(&step) {
        return MediaFrameTsScale::Nanos;
    }

    if next >= 100_000_000_000_000_000 {
        MediaFrameTsScale::Nanos
    } else if next >= 100_000_000_000_000 {
        if step >= 2_000_000 { MediaFrameTsScale::Nanos } else { MediaFrameTsScale::Micros }
    } else {
        MediaFrameTsScale::Millis
    }
}

fn playback_wrapped_elapsed_ms(elapsed_ms: i64, playback_span_ms: i64, loop_forever: bool) -> i64 {
    if loop_forever { elapsed_ms.rem_euclid(playback_span_ms) } else { elapsed_ms.clamp(0, playback_span_ms) }
}

async fn select_event_index(
    stream_id: Uuid,
    signature: &str,
    loop_forever: bool,
    playback_fps: Option<f64>,
    replay_started_at_ms: Option<i64>,
    playback_duration_ms: Option<i64>,
    replay_frame_clock: Option<ReplayFrameClock>,
    frame_timeline: Option<&[i64]>,
    events: &[MediaImuEvent],
) -> Option<usize> {
    if events.is_empty() {
        return None;
    }
    if events.len() == 1 {
        return Some(0);
    }

    let last_t = events.last()?.t_ms;
    let max_sample_t = last_t.max(0);
    if max_sample_t <= 0 {
        return Some(events.len().saturating_sub(1));
    }
    let playback_span_ms = playback_duration_ms.filter(|value| *value > 0).unwrap_or(max_sample_t).max(1);

    let now_ms = Utc::now().timestamp_millis();
    let elapsed_ms = {
        let mut cursors = media_imu_cursors().lock().await;
        let started_at_ms = replay_started_at_ms.filter(|value| *value <= now_ms).unwrap_or(now_ms);
        let reset_for_idle = |cursor: &MediaImuCursor, now_ms: i64| replay_started_at_ms.is_none() && now_ms.saturating_sub(cursor.last_seen_ms) > MEDIA_IMU_CURSOR_IDLE_RESET_MS;
        let wall_elapsed_ms = now_ms.saturating_sub(started_at_ms);
        let replay_frame_seq = replay_frame_clock.map(|clock| clock.seq).filter(|value| *value > 0);
        let replay_frame_ts = replay_frame_clock.map(|clock| clock.ts).filter(|value| *value > 0);
        let frame_interval_ms = playback_fps.filter(|value| value.is_finite() && *value > 0.0).map(|value| 1000.0 / value);

        let entry = cursors.entry(stream_id).or_insert_with(|| MediaImuCursor {
            signature: signature.to_string(),
            started_at_ms,
            last_seen_ms: now_ms,
            frame_anchor_seq: replay_frame_seq,
            frame_anchor_ts: replay_frame_ts,
            frame_anchor_elapsed_ms: wall_elapsed_ms,
            frame_ts_scale: None,
            last_frame_ts: replay_frame_ts,
        });
        if entry.signature != signature || reset_for_idle(entry, now_ms) {
            entry.signature = signature.to_string();
            entry.started_at_ms = started_at_ms;
            entry.frame_anchor_seq = replay_frame_seq;
            entry.frame_anchor_ts = replay_frame_ts;
            entry.frame_anchor_elapsed_ms = wall_elapsed_ms;
            entry.frame_ts_scale = None;
            entry.last_frame_ts = replay_frame_ts;
        } else if replay_started_at_ms.is_some() {
            // Replay streams expose their actual start time; keep cursor anchored to that clock so
            // IMU sampling tracks playback even when localization polling starts late.
            entry.started_at_ms = started_at_ms;
        }
        entry.last_seen_ms = now_ms;

        let seq_elapsed_ms = match (replay_frame_seq, frame_interval_ms) {
            (Some(current_seq), Some(frame_interval_ms)) => {
                if entry.frame_anchor_seq.is_none() {
                    entry.frame_anchor_seq = Some(current_seq);
                    entry.frame_anchor_elapsed_ms = wall_elapsed_ms;
                }
                let anchor_seq = entry.frame_anchor_seq.unwrap_or(current_seq);
                if current_seq >= anchor_seq {
                    let delta_frames = current_seq.saturating_sub(anchor_seq) as f64;
                    let delta_ms = (delta_frames * frame_interval_ms).round();
                    Some(entry.frame_anchor_elapsed_ms.saturating_add(delta_ms as i64))
                } else {
                    // Sequence moved backwards; treat this as a replay restart/rewind.
                    entry.frame_anchor_seq = Some(current_seq);
                    entry.frame_anchor_ts = replay_frame_ts;
                    entry.frame_anchor_elapsed_ms = wall_elapsed_ms;
                    Some(wall_elapsed_ms)
                }
            }
            _ => None,
        };

        let frame_elapsed_ms = replay_frame_ts.and_then(|current_ts| {
            if let Some(last_ts) = entry.last_frame_ts
                && entry.frame_ts_scale.is_none()
                && current_ts > last_ts
            {
                entry.frame_ts_scale = Some(infer_frame_ts_scale(last_ts, current_ts));
            }
            if entry.frame_anchor_ts.is_none() {
                entry.frame_anchor_ts = Some(current_ts);
                entry.frame_anchor_elapsed_ms = seq_elapsed_ms.unwrap_or(wall_elapsed_ms);
            }
            let anchor_ts = entry.frame_anchor_ts.unwrap_or(current_ts);
            let result = if current_ts >= anchor_ts {
                entry.frame_ts_scale.map(|scale| {
                    let delta_ms = convert_frame_delta_ms(current_ts.saturating_sub(anchor_ts), Some(scale));
                    entry.frame_anchor_elapsed_ms.saturating_add(delta_ms)
                })
            } else {
                // Timestamp moved backwards (loop/seek); re-anchor to keep timeline continuous.
                entry.frame_anchor_ts = Some(current_ts);
                entry.frame_anchor_elapsed_ms = seq_elapsed_ms.unwrap_or(wall_elapsed_ms);
                None
            };
            entry.last_frame_ts = Some(current_ts);
            result
        });

        let timeline_elapsed_ms = replay_frame_seq.and_then(|current_seq| {
            let timeline = frame_timeline?;
            if timeline.is_empty() {
                return None;
            }
            let mut index = current_seq.saturating_sub(1) as usize;
            if loop_forever {
                index %= timeline.len();
            } else if index >= timeline.len() {
                index = timeline.len().saturating_sub(1);
            }
            let base = *timeline.first()?;
            let value = *timeline.get(index)?;
            Some(value.saturating_sub(base).max(0))
        });

        timeline_elapsed_ms.or(frame_elapsed_ms).or(seq_elapsed_ms).unwrap_or(wall_elapsed_ms)
    };

    let playback_offset_ms = playback_wrapped_elapsed_ms(elapsed_ms, playback_span_ms, loop_forever);
    let target_t = playback_offset_ms.clamp(0, max_sample_t);
    let insertion = events.partition_point(|event| event.t_ms <= target_t);
    if insertion == 0 {
        // No IMU sample exists for this playback instant yet (e.g., sidecar starts after first frame).
        return None;
    }
    Some(insertion - 1)
}

fn imu_status_to_pose_sample(status: &lib_sensors::dto::ImuStatusPayload, t_ms: i64) -> Option<PipelineOutputSample> {
    let orientation = status.orientation.as_ref()?;

    let rotation = if let Some(quat) = orientation.quaternion.as_ref() {
        serde_json::json!({
            "roll": orientation.roll,
            "pitch": orientation.pitch,
            "yaw": orientation.yaw,
            "quaternion": {
                "x": quat.x,
                "y": quat.y,
                "z": quat.z,
                "w": quat.w,
            }
        })
    } else {
        serde_json::json!({
            "roll": orientation.roll,
            "pitch": orientation.pitch,
            "yaw": orientation.yaw,
        })
    };
    let translation = status.position_world.as_ref().map(|position| {
        serde_json::json!({
            "x": position.x,
            "y": position.y,
            "z": position.z,
        })
    });
    let velocity = status.velocity_world.as_ref().map(|value| {
        serde_json::json!({
            "x": value.x,
            "y": value.y,
            "z": value.z,
        })
    });
    let velocity_delta = status.velocity_delta_world.as_ref().map(|value| {
        serde_json::json!({
            "x": value.x,
            "y": value.y,
            "z": value.z,
        })
    });
    let angular_velocity = status.angular_velocity_dps.as_ref().map(|value| {
        serde_json::json!({
            "x": value.x,
            "y": value.y,
            "z": value.z,
        })
    });
    let gyro_bias = status.gyro_bias_dps.as_ref().map(|value| {
        serde_json::json!({
            "x": value.x,
            "y": value.y,
            "z": value.z,
        })
    });
    let corrected_world_accel = status.corrected_world_accel_mps2.as_ref().map(|value| {
        serde_json::json!({
            "x": value.x,
            "y": value.y,
            "z": value.z,
        })
    });

    let mut payload = JsonMap::new();
    payload.insert("t_ms".to_string(), JsonValue::from(t_ms));
    payload.insert("rotation".to_string(), rotation);
    if let Some(translation) = translation {
        payload.insert("translation".to_string(), translation);
    }
    if let Some(velocity) = velocity {
        payload.insert("velocity_world".to_string(), velocity);
    }
    if let Some(velocity_delta) = velocity_delta {
        payload.insert("velocity_delta_world".to_string(), velocity_delta);
    }
    if let Some(angular_velocity) = angular_velocity {
        payload.insert("angular_velocity_dps".to_string(), angular_velocity);
    }
    if let Some(gyro_bias) = gyro_bias {
        payload.insert("gyro_bias_dps".to_string(), gyro_bias);
    }
    if let Some(corrected_world_accel) = corrected_world_accel {
        payload.insert("corrected_world_accel_mps2".to_string(), corrected_world_accel);
    }
    if let Some(value) = status.linear_speed_mps {
        payload.insert("linear_speed_mps".to_string(), serde_json::json!(value));
    }
    if let Some(value) = status.linear_speed_normalized {
        payload.insert("linear_speed_normalized".to_string(), serde_json::json!(value));
    }
    if let Some(value) = status.angular_speed_dps {
        payload.insert("angular_speed_dps".to_string(), serde_json::json!(value));
    }
    if let Some(value) = status.angular_speed_normalized {
        payload.insert("angular_speed_normalized".to_string(), serde_json::json!(value));
    }
    if let Some(value) = status.stillness_confidence {
        payload.insert("stillness_confidence".to_string(), serde_json::json!(value));
    }
    if let Some(value) = status.is_still {
        payload.insert("is_still".to_string(), serde_json::json!(value));
    }
    if let Some(value) = status.rotation_contaminated {
        payload.insert("rotation_contaminated".to_string(), serde_json::json!(value));
    }
    if let Some(value) = status.dr_confidence {
        payload.insert("dr_confidence".to_string(), serde_json::json!(value));
    }
    if let Some(value) = status.motion_fast_g {
        payload.insert("motion_fast_g".to_string(), serde_json::json!(value));
    }
    if let Some(value) = status.is_moving_fast {
        payload.insert("is_moving_fast".to_string(), serde_json::json!(value));
    }

    Some(PipelineOutputSample { data_type: None, value: JsonValue::Object(payload) })
}
