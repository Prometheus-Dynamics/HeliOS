use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use flate2::read::GzDecoder;
use serde::Deserialize;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;
use utoipa::ToSchema;
use uuid::Uuid;

use super::types::EngineErrorBody;
use super::util::engine_error_body;
use crate::http::media::{MediaMetadata, load_media_metadata};
use crate::http::{AppState, storage, streams_persist};
use helios_engine::capture::{BackendHandle, BackendKind, CaptureConfig, ModeId};
use helios_engine::identity::DeviceIdentity;
use helios_engine::ipc::EngineErrorCode;
use helios_engine::ipc::{CURRENT_STREAM_CONFIG_SCHEMA_VERSION, RequestedDecoderConfig, RequestedEncoderConfig, StreamManifest, StreamPipelineGridSlot, StreamPipelineLayout};
use styx::capture_api::make_file_device;
use styx::core::format::{ColorSpace, MediaFormat, Resolution};
use styx::prelude::FourCc;

#[derive(Debug, Clone, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MediaReplayRequest {
    /// Media file names as returned by `GET /media` (stored under the API data `media/` dir).
    ///
    /// If omitted/empty, `streamId` (+ optional `kind`) can be used to select files automatically.
    #[serde(default)]
    pub files: Vec<String>,
    /// If set, auto-select media files that belong to this stream (based on stored metadata).
    #[serde(default)]
    pub stream_id: Option<Uuid>,
    /// When `streamId` is set, optionally filter by media kind (defaults to `calibration`).
    #[serde(default)]
    pub kind: Option<String>,
    /// Playback fps (image cadence).
    #[serde(default)]
    pub fps: Option<u32>,
    /// Loop the list forever.
    #[serde(default)]
    pub loop_forever: Option<bool>,
    /// Optional alias for this replay stream (used for persistence/UI display).
    #[serde(default)]
    pub alias: Option<String>,
}

#[utoipa::path(
    post,
    path = "/streams/replay/media",
    tag = "EngineStreams",
    request_body = MediaReplayRequest,
    responses(
        (status = 200, description = "Replay stream started", body = super::types::StartStreamResponse),
        (status = 400, description = "Invalid request", body = EngineErrorBody),
        (status = 502, description = "Engine error", body = EngineErrorBody)
    )
)]
pub(crate) async fn start_media_replay_stream(State(state): State<AppState>, Json(req): Json<MediaReplayRequest>) -> Response {
    const MAX_REPLAY_FILES: usize = 64;

    if req.files.len() > MAX_REPLAY_FILES {
        return (StatusCode::BAD_REQUEST, Json(engine_error_body(Some(EngineErrorCode::InvalidInput), format!("too many files (max {MAX_REPLAY_FILES})")))).into_response();
    }
    if let Some(fps) = req.fps
        && !(1..=240).contains(&fps)
    {
        return (StatusCode::BAD_REQUEST, Json(engine_error_body(Some(EngineErrorCode::InvalidInput), "fps must be between 1 and 240"))).into_response();
    }
    if let Some(alias) = req.alias.as_deref() {
        let trimmed = alias.trim();
        if !trimmed.is_empty() && trimmed.len() > 96 {
            return (StatusCode::BAD_REQUEST, Json(engine_error_body(Some(EngineErrorCode::InvalidInput), "alias is too long (max 96 chars)"))).into_response();
        }
    }

    let media_dir = match storage::ensure_subdir_async("media").await {
        Ok(dir) => dir,
        Err(err) => {
            return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("failed to access media storage: {err}")))).into_response();
        }
    };
    let meta_dir = match storage::ensure_subdir_async("media-meta").await {
        Ok(dir) => dir,
        Err(err) => {
            return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("failed to access media metadata: {err}")))).into_response();
        }
    };

    let files = if !req.files.is_empty() {
        req.files.clone()
    } else if let Some(stream_id) = req.stream_id {
        let kind = req.kind.as_deref().map(str::trim).filter(|v| !v.is_empty()).unwrap_or("calibration");

        let mut entries = match tokio::fs::read_dir(&media_dir).await {
            Ok(entries) => entries,
            Err(err) => {
                return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("failed to read media directory: {err}")))).into_response();
            }
        };

        let mut candidates: Vec<(i64, String)> = Vec::new();
        loop {
            let entry = match entries.next_entry().await {
                Ok(Some(entry)) => entry,
                Ok(None) => break,
                Err(err) => {
                    return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("failed to read media entry: {err}")))).into_response();
                }
            };
            let meta = match entry.metadata().await {
                Ok(meta) if meta.is_file() => meta,
                Ok(_) => continue,
                Err(_) => continue,
            };
            if meta.len() == 0 {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".json") {
                continue;
            }
            let md = match load_media_metadata(&meta_dir, &name).await {
                Some(md) => md,
                None => continue,
            };
            if md.stream_id != Some(stream_id) {
                continue;
            }
            if md.kind.as_deref() != Some(kind) {
                continue;
            }
            let ts = md.captured_at_ms.unwrap_or(0);
            candidates.push((ts, name));
        }

        candidates.sort_by_key(|(ts, _)| *ts);
        let names: Vec<String> = candidates.into_iter().map(|(_, n)| n).collect();
        if names.is_empty() {
            return (StatusCode::NOT_FOUND, Json(engine_error_body(Some(EngineErrorCode::NotFound), format!("no media matched streamId={stream_id} kind={kind}")))).into_response();
        }
        names
    } else {
        return (StatusCode::BAD_REQUEST, Json(engine_error_body(Some(EngineErrorCode::InvalidInput), "provide either files or streamId"))).into_response();
    };

    let mut paths = Vec::with_capacity(files.len());
    let mut inferred_source_stream_ids: std::collections::BTreeSet<Uuid> = std::collections::BTreeSet::new();
    let mut inferred_playback_fps: Option<f32> = None;
    for raw in &files {
        let raw_trimmed = raw.trim();
        if raw_trimmed.is_empty() || raw_trimmed.contains('/') || raw_trimmed.contains('\\') || raw_trimmed.contains("..") {
            return (StatusCode::BAD_REQUEST, Json(engine_error_body(Some(EngineErrorCode::InvalidInput), format!("invalid media name: {raw}")))).into_response();
        }
        let Some(name) = storage::sanitize_name(raw) else {
            return (StatusCode::BAD_REQUEST, Json(engine_error_body(Some(EngineErrorCode::InvalidInput), format!("invalid media name: {raw}")))).into_response();
        };
        if req.stream_id.is_none()
            && let Some(stream_id) = media_metadata_stream_id(&meta_dir, &name).await
        {
            inferred_source_stream_ids.insert(stream_id);
        }
        let path = media_dir.join(&name);
        match tokio::fs::metadata(&path).await {
            Ok(meta) if meta.is_file() => {}
            Ok(_) => {
                return (StatusCode::BAD_REQUEST, Json(engine_error_body(Some(EngineErrorCode::InvalidInput), format!("media is not a file: {name}")))).into_response();
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return (StatusCode::NOT_FOUND, Json(engine_error_body(Some(EngineErrorCode::NotFound), format!("media not found: {name}")))).into_response();
            }
            Err(err) => {
                return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("failed to stat media {name}: {err}")))).into_response();
            }
        }
        let fps_hint = req.fps.map(|value| value as f32).or(replay_fps_hint(&meta_dir, &name, &path).await);
        if req.fps.is_none() && inferred_playback_fps.is_none() {
            inferred_playback_fps = fps_hint;
        }
        let replay_path = match ensure_replay_compatible_path(&meta_dir, &path, &name, fps_hint).await {
            Ok(value) => value,
            Err(reason) => {
                return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("failed to prepare replay input for {name}: {reason}")))).into_response();
            }
        };
        paths.push(replay_path);
    }

    let fps = req.fps.or_else(|| inferred_playback_fps.map(|value| value.round() as u32)).unwrap_or(30).clamp(1, 240);
    let loop_forever = req.loop_forever.unwrap_or(true);
    let source_stream_id = req.stream_id.or_else(|| if inferred_source_stream_ids.len() == 1 { inferred_source_stream_ids.iter().next().copied() } else { None });
    let replay_calibration = resolve_replay_calibration(&state, source_stream_id).await;

    let alias = req.alias.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(|v| v.to_string()).unwrap_or_else(|| format!("media-replay-{}", Uuid::new_v4()));

    let device = make_file_device("file-replay", paths.clone(), fps, loop_forever);
    let dummy_mode = device.backends.first().and_then(|backend| backend.descriptor.modes.first()).map(|mode| mode.id.clone()).unwrap_or_else(|| {
        // If no modes are detected, fall back to a safe placeholder.
        let dummy_res = Resolution::new(1, 1).unwrap();
        let dummy_fmt = MediaFormat::new(FourCc::new(*b"RGBA"), dummy_res, ColorSpace::Srgb);
        ModeId { format: dummy_fmt, interval: None }
    });

    let manifest = StreamManifest {
        schema_version: CURRENT_STREAM_CONFIG_SCHEMA_VERSION,
        identity: DeviceIdentity { id: None, alias: Some(alias), hardware_id: None },
        capture: CaptureConfig {
            device_keys: Vec::new(),
            device_identity: None,
            backend: BackendKind::File,
            handle: BackendHandle::File { paths, fps, loop_forever },
            mode: dummy_mode,
            target_fps: None,
            interval: None,
            controls: Vec::new(),
            enable_tdn_output: false,
        },
        host_buffer: helios_engine::ipc::default_host_buffer(),
        internal: false,
        // Media replay streams should be immediately viewable in the UI even when codecs are disabled.
        // The reserved RAW pipeline provides a cheap preview path (and can be wired into other pipelines).
        pipeline_enabled: true,
        pipelines: Vec::new(),
        active_pipeline_id: Some(super::RAW_PIPELINE_UUID),
        // Use the engine-native key for the raw passthrough port.
        active_pipeline_output: Some("raw".to_string()),
        pipeline_layout: Some(StreamPipelineLayout {
            rows: 1,
            columns: 1,
            slots: vec![StreamPipelineGridSlot { row: 0, column: 0, pipeline_id: Some(super::RAW_PIPELINE_UUID), output_key: Some("raw".to_string()) }],
        }),
        pipeline_wires: Vec::new(),
        pipeline_host_inputs: std::collections::BTreeMap::new(),
        calibration: replay_calibration,
        pose: None,
        encoder: RequestedEncoderConfig::default(),
        decoder: RequestedDecoderConfig::default(),
        preview_jpeg_quality: 30,
        recording_mode: helios_engine::ipc::default_recording_mode(),
        start_on_boot: false,
    };

    super::lifecycle::start_stream(state, manifest).await
}

async fn media_metadata_stream_id(meta_dir: &Path, media_name: &str) -> Option<Uuid> {
    let meta = load_media_metadata(meta_dir, media_name).await?;
    meta.stream_id
}

async fn resolve_replay_calibration(state: &AppState, source_stream_id: Option<Uuid>) -> Option<helios_engine::ipc::StreamCalibration> {
    let Some(source_stream_id) = source_stream_id else {
        return None;
    };

    if let Ok(streams) = state.engine.list_streams().await
        && let Some(summary) = streams.into_iter().find(|entry| entry.stream_id == source_stream_id)
        && summary.manifest.calibration.is_some()
    {
        return summary.manifest.calibration;
    }

    for record in streams_persist::list_persisted_records().await {
        let Some(manifest) = record.requested_manifest() else {
            continue;
        };
        if manifest.internal {
            continue;
        }
        let matches = manifest.identity.id == Some(source_stream_id) || record.last_stream_id == Some(source_stream_id) || streams_persist::derived_stream_id(&record.camera_id) == source_stream_id;
        if !matches {
            continue;
        }
        if manifest.calibration.is_some() {
            return manifest.calibration;
        }
    }

    None
}

async fn ensure_replay_compatible_path(meta_dir: &Path, source: &Path, source_name: &str, fps_hint: Option<f32>) -> Result<PathBuf, String> {
    // TEMP_SHIM: streams-replay-annexb-remux-compat-cache
    // Older/offboard raw annex-b captures still need an MP4 wrapper before the replay stack can consume them reliably.
    let Some(format_hint) = raw_annexb_format_hint(source_name) else {
        return Ok(source.to_path_buf());
    };

    let output = replay_cache_path(meta_dir, source_name, fps_hint);
    if !replay_cache_fresh(source, &output).await.unwrap_or(false) {
        remux_raw_to_mp4(source, &output, format_hint, fps_hint).await?;
    }
    Ok(output)
}

fn raw_annexb_format_hint(source_name: &str) -> Option<&'static str> {
    let ext = source_name.rsplit('.').next()?.trim().to_ascii_lowercase();
    match ext.as_str() {
        "h264" | "avc" => Some("h264"),
        "h265" | "hevc" => Some("hevc"),
        _ => None,
    }
}

fn replay_cache_path(meta_dir: &Path, source_name: &str, fps_hint: Option<f32>) -> PathBuf {
    let fps_tag = fps_hint.filter(|value| value.is_finite() && *value > 0.0).map(|value| format!("{value:.3}").replace('.', "_")).unwrap_or_else(|| "auto".to_string());
    meta_dir.join(format!("{source_name}.replay.{fps_tag}.mp4"))
}

async fn replay_cache_fresh(source: &Path, replay: &Path) -> Result<bool, std::io::Error> {
    let source_meta = tokio::fs::metadata(source).await?;
    let replay_meta = match tokio::fs::metadata(replay).await {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };
    let source_modified = source_meta.modified().ok();
    let replay_modified = replay_meta.modified().ok();
    Ok(matches!((source_modified, replay_modified), (Some(src), Some(dst)) if dst >= src))
}

async fn remux_raw_to_mp4(source: &Path, output: &Path, format_hint: &'static str, fps_hint: Option<f32>) -> Result<(), String> {
    if let Some(parent) = output.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|err| format!("replay cache dir create failed: {err}"))?;
    }
    let temp = output.with_extension("tmp.mp4");
    let source_path = source.to_path_buf();
    let output_path = temp.clone();
    let fps_hint = fps_hint.filter(|value| value.is_finite() && *value > 0.0);

    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y").arg("-loglevel").arg("error");
        cmd.arg("-fflags").arg("+genpts");
        cmd.arg("-f").arg(format_hint);
        if let Some(fps) = fps_hint {
            cmd.arg("-r").arg(format!("{fps:.6}"));
        }
        cmd.arg("-i").arg(&source_path);
        cmd.arg("-an");
        // Preserve the original bitstream for speed; this wraps annex-b raw into MP4.
        cmd.arg("-c:v").arg("copy");
        cmd.arg("-movflags").arg("+faststart");
        cmd.arg(&output_path);
        let out = cmd.output().map_err(|err| format!("ffmpeg launch failed: {err}"))?;
        if out.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        let reason = stderr.trim();
        if reason.is_empty() { Err(format!("ffmpeg failed with status {}", out.status)) } else { Err(format!("ffmpeg failed: {reason}")) }
    })
    .await
    .map_err(|_| "ffmpeg replay remux task failed".to_string())??;

    if let Err(err) = tokio::fs::rename(&temp, output).await {
        let _ = tokio::fs::remove_file(output).await;
        tokio::fs::rename(&temp, output).await.map_err(|err2| format!("replay cache rename failed after retry ({err}): {err2}"))?;
    }
    Ok(())
}

async fn replay_fps_hint(meta_dir: &Path, name: &str, source_path: &Path) -> Option<f32> {
    let bytes = tokio::fs::read(meta_dir.join(format!("{name}.json"))).await.ok()?;
    let meta: MediaMetadata = serde_json::from_slice(&bytes).ok()?;
    let metadata_fps = meta.fps.filter(|fps| fps.is_finite() && *fps > 0.0);
    if let Some(frame_ts_name) = meta.frame_timestamps_file_name.as_deref().and_then(storage::sanitize_name) {
        let frame_ts_path = resolve_frame_ts_path(meta_dir, &frame_ts_name).await.unwrap_or_else(|| meta_dir.join(frame_ts_name));
        if let Some(fps) = derive_fps_from_frame_timestamps(&frame_ts_path).await {
            return Some(fps);
        }
    }
    let Some(format_hint) = raw_annexb_format_hint(name) else {
        return metadata_fps;
    };
    let Some(sidecar_name) = meta.imu_data_file_name.as_deref().and_then(storage::sanitize_name) else {
        return metadata_fps;
    };
    let sidecar_path = meta_dir.join(sidecar_name);
    derive_fps_from_sidecar(&sidecar_path, source_path, format_hint).await.or(metadata_fps)
}

async fn resolve_frame_ts_path(meta_dir: &Path, sidecar_name: &str) -> Option<PathBuf> {
    let meta_candidate = meta_dir.join(sidecar_name);
    if tokio::fs::metadata(&meta_candidate).await.ok().is_some_and(|meta| meta.is_file()) {
        return Some(meta_candidate);
    }
    let media_dir = storage::ensure_subdir_async("media").await.ok()?;
    let media_candidate = media_dir.join(sidecar_name);
    tokio::fs::metadata(&media_candidate).await.ok().and_then(|meta| meta.is_file().then_some(media_candidate))
}

async fn derive_fps_from_frame_timestamps(path: &Path) -> Option<f32> {
    let path = path.to_path_buf();
    let (count, first, last) = tokio::task::spawn_blocking(move || frame_timestamp_span_sync(&path)).await.ok().flatten()?;
    if count <= 1 || last <= first {
        return None;
    }
    let span_ms = last.saturating_sub(first) as f64;
    if span_ms <= f64::EPSILON {
        return None;
    }
    let fps = (count as f64 * 1000.0) / span_ms;
    if !fps.is_finite() || fps <= 0.0 {
        return None;
    }
    Some((fps as f32).clamp(1.0, 240.0))
}

async fn derive_fps_from_sidecar(sidecar_path: &Path, source_path: &Path, format_hint: &'static str) -> Option<f32> {
    let sidecar_path = sidecar_path.to_path_buf();
    let span_ms = tokio::task::spawn_blocking(move || imu_sidecar_span_ms_sync(&sidecar_path)).await.ok().flatten()?;
    if span_ms <= 0 {
        return None;
    }

    let source_path = source_path.to_path_buf();
    let format_hint = format_hint.to_string();
    let frame_count = tokio::task::spawn_blocking(move || probe_raw_frame_count_sync(&source_path, &format_hint)).await.ok().flatten()?;
    if frame_count == 0 {
        return None;
    }

    let fps = (frame_count as f64 * 1000.0) / (span_ms as f64);
    if !fps.is_finite() || fps <= 0.0 {
        return None;
    }
    Some((fps as f32).clamp(1.0, 240.0))
}

fn imu_sidecar_span_ms_sync(path: &Path) -> Option<i64> {
    let file = std::fs::File::open(path).ok()?;
    let reader = BufReader::new(GzDecoder::new(file));
    let mut first: Option<i64> = None;
    let mut last: Option<i64> = None;

    for line in reader.lines() {
        let Ok(line) = line else {
            continue;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            continue;
        };
        let Some(t_ms) = value.get("t_ms").and_then(|value| value.as_i64()) else {
            continue;
        };
        if first.is_none() {
            first = Some(t_ms);
        }
        last = Some(t_ms);
    }

    let first = first?;
    let last = last?;
    (last > first).then_some(last - first)
}

fn probe_raw_frame_count_sync(path: &Path, format_hint: &str) -> Option<u64> {
    run_ffprobe_frame_count(path, format_hint, "-count_frames", "nb_read_frames").or_else(|| run_ffprobe_frame_count(path, format_hint, "-count_packets", "nb_read_packets"))
}

fn frame_timestamp_span_sync(path: &Path) -> Option<(u64, u64, u64)> {
    let file = std::fs::File::open(path).ok()?;
    let reader = BufReader::new(file);
    let mut count = 0u64;
    let mut first = None;
    let mut last = None;

    for line in reader.lines() {
        let Ok(line) = line else {
            continue;
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(ts) = trimmed.parse::<u64>() else {
            continue;
        };
        if first.is_none() {
            first = Some(ts);
        }
        last = Some(ts);
        count = count.saturating_add(1);
    }

    Some((count, first?, last?))
}

fn run_ffprobe_frame_count(path: &Path, format_hint: &str, count_mode: &str, field: &str) -> Option<u64> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-f")
        .arg(format_hint)
        .arg(count_mode)
        .arg("-select_streams")
        .arg("v:0")
        .arg("-show_entries")
        .arg(format!("stream={field}"))
        .arg("-of")
        .arg("default=noprint_wrappers=1:nokey=1")
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    text.trim().parse::<u64>().ok()
}
