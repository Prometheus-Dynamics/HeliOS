use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
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
        if raw_annexb_format_hint(&name).is_some() {
            return (
                StatusCode::BAD_REQUEST,
                Json(engine_error_body(Some(EngineErrorCode::InvalidInput), format!("raw annex-b replay is no longer supported for {name}; containerize the capture as mp4 first"))),
            )
                .into_response();
        }
        let fps_hint = req.fps.map(|value| value as f32).or(replay_fps_hint(&meta_dir, &name).await);
        if req.fps.is_none() && inferred_playback_fps.is_none() {
            inferred_playback_fps = fps_hint;
        }
        paths.push(path);
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

fn raw_annexb_format_hint(source_name: &str) -> Option<&'static str> {
    let ext = source_name.rsplit('.').next()?.trim().to_ascii_lowercase();
    match ext.as_str() {
        "h264" | "avc" => Some("h264"),
        "h265" | "hevc" => Some("hevc"),
        _ => None,
    }
}

async fn replay_fps_hint(meta_dir: &Path, name: &str) -> Option<f32> {
    let bytes = tokio::fs::read(meta_dir.join(format!("{name}.json"))).await.ok()?;
    let meta: MediaMetadata = serde_json::from_slice(&bytes).ok()?;
    let metadata_fps = meta.fps.filter(|fps| fps.is_finite() && *fps > 0.0);
    if let Some(frame_ts_name) = meta.frame_timestamps_file_name.as_deref().and_then(storage::sanitize_name) {
        let frame_ts_path = resolve_frame_ts_path(meta_dir, &frame_ts_name).await.unwrap_or_else(|| meta_dir.join(frame_ts_name));
        if let Some(fps) = derive_fps_from_frame_timestamps(&frame_ts_path).await {
            return Some(fps);
        }
    }
    metadata_fps
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

fn frame_timestamp_span_sync(path: &Path) -> Option<(u64, u64, u64)> {
    let file = std::fs::File::open(path).ok()?;
    let reader = BufReader::new(file);
    let mut count = 0u64;
    let mut first = None;
    let mut last = None;

    for line_result in reader.lines() {
        let line = match line_result {
            Ok(line) => line,
            Err(_) => continue,
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
