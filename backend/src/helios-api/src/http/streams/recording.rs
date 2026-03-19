use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::Utc;
use flate2::{Compression, write::GzEncoder};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path as StdPath, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::http::device::imu::imu_status_from_snapshot;
use crate::http::error::ApiError;
use crate::http::media::{MediaItem, MediaMetadata, write_media_metadata};
use crate::http::storage;
use crate::http::streams::util::{engine_error_body, map_client_error};
use helios_engine::ipc::{EngineErrorCode, EngineEvent, RecordingCodec, RecordingContainer, RecordingSource};
use helios_peripherals::dto::SensorScope;

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct StartRecordingRequest {
    /// Optional base filename; extension is inferred from container/codec.
    #[serde(default)]
    pub name: Option<String>,
    /// Desired codec: "h264" or "h265" (defaults to h265).
    #[serde(default)]
    pub codec: Option<String>,
    /// Container format: "raw" (default) or "mp4".
    #[serde(default)]
    pub container: Option<String>,
    /// Optional maximum duration in milliseconds.
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// Optional target recording FPS (best-effort frame drop).
    #[serde(default)]
    pub fps: Option<f32>,
    /// Optional target bitrate in bps (transcodes MP4 output).
    #[serde(default)]
    pub bitrate_bps: Option<u64>,
    /// Optional GOP size (transcodes MP4 output).
    #[serde(default)]
    pub gop: Option<i32>,
    /// Optional CRF quality (0-51, lower = higher quality).
    #[serde(default)]
    pub quality: Option<u8>,
    /// Optional max output width (transcodes MP4 output).
    #[serde(default)]
    pub max_width: Option<u32>,
    /// Optional max output height (transcodes MP4 output).
    #[serde(default)]
    pub max_height: Option<u32>,
    /// Recording source: multiplex (default), raw, or a specific pipeline output.
    #[serde(default)]
    pub source: Option<RecordingSource>,
    /// Also capture IMU telemetry sidecar for this recording.
    #[serde(default)]
    pub include_imu: Option<bool>,
    /// Optional IMU sidecar sample interval in milliseconds (default: 20ms).
    #[serde(default)]
    pub imu_interval_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CaptureShadowRecordingRequest {
    /// Rolling window to capture, in milliseconds.
    pub window_ms: u64,
    /// Optional base filename; extension is inferred from container/codec.
    #[serde(default)]
    pub name: Option<String>,
    /// Container format: "raw" (default) or "mp4".
    #[serde(default)]
    pub container: Option<String>,
    /// Optional codec override for raw output ("h264" or "h265").
    #[serde(default)]
    pub codec: Option<String>,
}

const IMU_SIDE_CAR_DEFAULT_INTERVAL_MS: u64 = 20;
const IMU_SIDE_CAR_MIN_INTERVAL_MS: u64 = 5;
const IMU_SIDE_CAR_MAX_INTERVAL_MS: u64 = 2_000;
const IMU_SIDE_CAR_HISTORY_WINDOW_MS: i64 = 10_000;
const IMU_SIDE_CAR_HISTORY_MAX_SAMPLES: usize = 4_096;
const IMU_SIDE_CAR_STOP_TAIL_IDLE_MS: u64 = 300;
const IMU_SIDE_CAR_STOP_TAIL_MAX_MS: u64 = 5_000;
const IMU_SIDE_CAR_STOP_WAIT_QUIET_MS: u64 = 250;
const IMU_SIDE_CAR_STOP_WAIT_MAX_MS: u64 = 5_000;
const RECORDING_STOP_GRACE_DEFAULT_MS: u64 = 0;
const RECORDING_STOP_GRACE_MIN_MS: u64 = 0;
const RECORDING_STOP_GRACE_MAX_MS: u64 = 2_000;

#[derive(Debug)]
struct ImuSidecarSession {
    media_name: String,
    sidecar_file_name: String,
    sidecar_path: PathBuf,
    cancel: CancellationToken,
    join: tokio::task::JoinHandle<Result<ImuSidecarSummary, String>>,
}

#[derive(Debug, Clone)]
struct ActiveRecordingSession {
    media_name: String,
    output_path: PathBuf,
}

#[derive(Debug, Clone, Copy)]
struct ImuSidecarSummary {
    samples: u64,
    bytes: u64,
}

#[derive(Debug, Serialize)]
struct ImuSidecarEvent {
    t_ms: i64,
    stream_id: Uuid,
    imu: lib_sensors::dto::ImuStatusPayload,
}

fn imu_sidecar_sessions() -> &'static Mutex<HashMap<Uuid, ImuSidecarSession>> {
    static SESSIONS: OnceLock<Mutex<HashMap<Uuid, ImuSidecarSession>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn active_recording_sessions() -> &'static Mutex<HashMap<Uuid, ActiveRecordingSession>> {
    static SESSIONS: OnceLock<Mutex<HashMap<Uuid, ActiveRecordingSession>>> = OnceLock::new();
    SESSIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

#[utoipa::path(
    post,
    path = "/streams/{id}/recording/start",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    request_body = StartRecordingRequest,
    responses(
        (status = 201, description = "Recording started", body = MediaItem),
        (status = 400, description = "Invalid request", body = crate::http::streams::types::EngineErrorBody),
        (status = 502, description = "Engine error", body = crate::http::streams::types::EngineErrorBody)
    )
)]
pub async fn start_recording(State(state): State<crate::http::AppState>, Path(id): Path<Uuid>, Json(req): Json<StartRecordingRequest>) -> Response {
    let (container, mut codec) = match parse_recording_options(&req) {
        Ok(value) => value,
        Err(err) => return (*err).into_response(),
    };
    let source = req.source.clone().unwrap_or_default();
    let codec_explicit = req.codec.as_deref().is_some_and(|value| !value.trim().is_empty());
    if !codec_explicit
        && matches!(source, RecordingSource::Multiplex)
        && let Ok(stream_codec) = infer_stream_codec(&state, id).await
    {
        codec = stream_codec;
    }
    let settings = parse_recording_settings(&req);
    let metadata_fps = infer_recording_fps(&state, id, settings.as_ref().and_then(|value| value.fps)).await;
    let dir = match storage::ensure_subdir("media") {
        Ok(dir) => dir,
        Err(err) => {
            return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("failed to prepare media dir: {err}")))).into_response();
        }
    };

    let ext = recording_extension(container, codec);
    let ts_ms = Utc::now().timestamp_millis();
    let requested_name = match req.name.as_deref() {
        Some(name) => storage::sanitize_name(name).ok_or_else(|| ApiError::bad_request("invalid recording name")),
        None => Ok(format!("recording_{id}_{ts_ms}")),
    };
    let base = match requested_name {
        Ok(value) => value,
        Err(err) => return err.into_response(),
    };
    let filename = match ensure_extension(&base, ext) {
        Some(name) => name,
        None => return ApiError::bad_request("invalid recording name").into_response(),
    };
    let filename = match unique_media_name(&dir, &filename).await {
        Ok(name) => name,
        Err(err) => {
            return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("failed to resolve media name: {err}")))).into_response();
        }
    };

    let include_imu = req.include_imu.unwrap_or(true);
    let imu_interval_ms = clamp_imu_interval_ms(req.imu_interval_ms);
    let imu_sidecar_name = include_imu.then(|| imu_sidecar_file_name(&filename));
    let frame_ts_name = frame_timestamps_file_name(&filename);
    info!(
        stream_id = %id,
        source = ?source,
        codec = ?codec,
        container = ?container,
        requested_fps = ?settings.as_ref().and_then(|value| value.fps),
        include_imu,
        duration_ms = ?req.duration_ms,
        output = %filename,
        "start recording request"
    );

    let output_path = dir.join(&filename);
    let params =
        crate::ipc::engine::StartRecordingParams { source, output_path: output_path.to_string_lossy().to_string(), container, codec, duration_ms: req.duration_ms, settings: settings.clone() };
    match state.engine.start_recording(id, params).await {
        Ok(EngineEvent::Ack { .. }) => {}
        Ok(EngineEvent::Nack { code, reason, .. }) => {
            return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(code), reason))).into_response();
        }
        Ok(other) => {
            return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("unexpected engine response: {other:?}")))).into_response();
        }
        Err(err) => return map_client_error(err),
    }

    let video_codec = Some(
        match codec {
            RecordingCodec::H264 => "h264",
            RecordingCodec::H265 => "h265",
        }
        .to_string(),
    );
    let meta = MediaMetadata {
        stream_id: Some(id),
        kind: Some("recording".into()),
        captured_at_ms: Some(ts_ms),
        fps: metadata_fps,
        video_codec: video_codec.clone(),
        imu_data_file_name: imu_sidecar_name.clone(),
        imu_data_samples: include_imu.then_some(0),
        frame_timestamps_file_name: Some(frame_ts_name.clone()),
        ..Default::default()
    };
    if let Err(err) = write_media_metadata(&filename, meta).await {
        let _ = state.engine.stop_recording(id).await;
        return err.into_response();
    }

    if include_imu
        && let Err(reason) = start_imu_sidecar_session(
            id,
            &filename,
            ts_ms,
            req.duration_ms,
            imu_interval_ms,
            imu_sidecar_name.clone().unwrap_or_else(|| imu_sidecar_file_name(&filename)),
            output_path.with_file_name(&frame_ts_name),
        )
        .await
    {
        let _ = state.engine.stop_recording(id).await;
        clear_media_imu_sidecar(&filename).await;
        return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("failed to start IMU sidecar capture: {reason}")))).into_response();
    }

    {
        let mut sessions = active_recording_sessions().lock().await;
        sessions.insert(id, ActiveRecordingSession { media_name: filename.clone(), output_path: output_path.clone() });
    }

    (
        StatusCode::CREATED,
        Json(MediaItem {
            name: filename,
            size_bytes: 0,
            content_type: content_type_for_extension(ext),
            description: None,
            tags: Vec::new(),
            stream_id: Some(id),
            kind: Some("recording".into()),
            captured_at_ms: Some(ts_ms),
            width: None,
            height: None,
            fps: metadata_fps,
            video_codec,
            label_attached: false,
            label_file_name: None,
            model_id: None,
            model_input_resolution: None,
            model_tensor_spec: None,
            imu_data_file_name: imu_sidecar_name,
            imu_data_samples: include_imu.then_some(0),
        }),
    )
        .into_response()
}

#[utoipa::path(
    post,
    path = "/streams/{id}/recording/stop",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    responses(
        (status = 204, description = "Recording stopped"),
        (status = 502, description = "Engine error", body = crate::http::streams::types::EngineErrorBody)
    )
)]
pub async fn stop_recording(State(state): State<crate::http::AppState>, Path(id): Path<Uuid>) -> Response {
    let active_session = {
        let mut sessions = active_recording_sessions().lock().await;
        sessions.remove(&id)
    };
    let imu_stop_delay = Duration::from_millis(recording_stop_grace_ms());
    let engine_response = state.engine.stop_recording(id).await;
    if let Some(session) = active_session.as_ref() {
        let frame_ts_path = session.output_path.with_file_name(frame_timestamps_file_name(&session.media_name));
        wait_for_frame_timestamps_settle(&frame_ts_path).await;
    }
    if !imu_stop_delay.is_zero() {
        tokio::time::sleep(imu_stop_delay).await;
    }
    if let Err(err) = stop_imu_sidecar_session(id).await {
        warn!(stream_id = %id, error = %err, "failed to finalize IMU sidecar recording");
    }
    match engine_response {
        Ok(EngineEvent::Ack { .. }) => {
            if let Some(session) = active_session.as_ref()
                && let Err(err) = update_media_recording_fps_from_frame_ts(&session.media_name).await
            {
                warn!(stream_id = %id, media_name = %session.media_name, error = %err, "failed to update recording fps from frame timestamps");
            }
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(EngineEvent::Nack { code, reason, .. }) => {
            if let Some(session) = active_session.as_ref() {
                if let Err(err) = cleanup_failed_recording_artifacts(session).await {
                    warn!(stream_id = %id, media_name = %session.media_name, error = %err, "failed to clean up failed recording artifacts");
                } else {
                    info!(stream_id = %id, media_name = %session.media_name, "cleaned up failed recording artifacts");
                }
            }
            (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(code), reason))).into_response()
        }
        Ok(other) => {
            if let Some(session) = active_session.as_ref() {
                if let Err(err) = cleanup_failed_recording_artifacts(session).await {
                    warn!(stream_id = %id, media_name = %session.media_name, error = %err, "failed to clean up failed recording artifacts");
                } else {
                    info!(stream_id = %id, media_name = %session.media_name, "cleaned up failed recording artifacts");
                }
            }
            (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("unexpected engine response: {other:?}")))).into_response()
        }
        Err(err) => map_client_error(err),
    }
}

#[utoipa::path(
    post,
    path = "/streams/{id}/recording/capture",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    request_body = CaptureShadowRecordingRequest,
    responses(
        (status = 201, description = "Captured recording", body = MediaItem),
        (status = 400, description = "Invalid request", body = crate::http::streams::types::EngineErrorBody),
        (status = 502, description = "Engine error", body = crate::http::streams::types::EngineErrorBody)
    )
)]
pub async fn capture_shadow_recording(State(state): State<crate::http::AppState>, Path(id): Path<Uuid>, Json(req): Json<CaptureShadowRecordingRequest>) -> Response {
    if !crate::features::shadow_recorder_enabled() {
        return (StatusCode::BAD_REQUEST, Json(engine_error_body(Some(EngineErrorCode::InvalidState), "shadow recorder feature is disabled".to_string()))).into_response();
    }
    if req.window_ms == 0 {
        return ApiError::bad_request("window_ms must be greater than zero").into_response();
    }
    let container = match parse_capture_container(&req) {
        Ok(value) => value,
        Err(err) => return (*err).into_response(),
    };

    let codec_override = match parse_codec_override(req.codec.as_deref()) {
        Ok(value) => value,
        Err(err) => return (*err).into_response(),
    };
    let ext_codec = if matches!(container, RecordingContainer::Raw) {
        match codec_override {
            Some(codec) => codec,
            None => match infer_stream_codec(&state, id).await {
                Ok(codec) => codec,
                Err(err) => return err.into_response(),
            },
        }
    } else {
        RecordingCodec::H265
    };

    let dir = match storage::ensure_subdir("media") {
        Ok(dir) => dir,
        Err(err) => {
            return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("failed to prepare media dir: {err}")))).into_response();
        }
    };

    let ext = recording_extension(container, ext_codec);
    let ts_ms = Utc::now().timestamp_millis();
    let requested_name = match req.name.as_deref() {
        Some(name) => storage::sanitize_name(name).ok_or_else(|| ApiError::bad_request("invalid recording name")),
        None => Ok(format!("recording_{id}_{ts_ms}")),
    };
    let base = match requested_name {
        Ok(value) => value,
        Err(err) => return err.into_response(),
    };
    let filename = match ensure_extension(&base, ext) {
        Some(name) => name,
        None => return ApiError::bad_request("invalid recording name").into_response(),
    };
    let filename = match unique_media_name(&dir, &filename).await {
        Ok(name) => name,
        Err(err) => {
            return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("failed to resolve media name: {err}")))).into_response();
        }
    };

    let output_path = dir.join(&filename);
    match state.engine.capture_shadow_recording(id, output_path.to_string_lossy().to_string(), container, req.window_ms).await {
        Ok(EngineEvent::Ack { .. }) => {}
        Ok(EngineEvent::Nack { code, reason, .. }) => {
            return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(code), reason))).into_response();
        }
        Ok(other) => {
            return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("unexpected engine response: {other:?}")))).into_response();
        }
        Err(err) => return map_client_error(err),
    }

    let size_bytes = fs::metadata(&output_path).await.map(|meta| meta.len()).unwrap_or(0);
    if size_bytes == 0 {
        let _ = fs::remove_file(&output_path).await;
        return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), "shadow capture produced empty output".to_string()))).into_response();
    }

    let video_codec = if matches!(container, RecordingContainer::Raw) {
        Some(
            match ext_codec {
                RecordingCodec::H264 => "h264",
                RecordingCodec::H265 => "h265",
            }
            .to_string(),
        )
    } else {
        None
    };
    let meta = MediaMetadata { stream_id: Some(id), kind: Some("recording".into()), captured_at_ms: Some(ts_ms), video_codec: video_codec.clone(), ..Default::default() };
    if let Err(err) = write_media_metadata(&filename, meta).await {
        return err.into_response();
    }

    (
        StatusCode::CREATED,
        Json(MediaItem {
            name: filename,
            size_bytes,
            content_type: content_type_for_extension(ext),
            description: None,
            tags: Vec::new(),
            stream_id: Some(id),
            kind: Some("recording".into()),
            captured_at_ms: Some(ts_ms),
            width: None,
            height: None,
            fps: None,
            video_codec,
            label_attached: false,
            label_file_name: None,
            model_id: None,
            model_input_resolution: None,
            model_tensor_spec: None,
            imu_data_file_name: None,
            imu_data_samples: None,
        }),
    )
        .into_response()
}

fn clamp_imu_interval_ms(value: Option<u64>) -> u64 {
    value.unwrap_or(IMU_SIDE_CAR_DEFAULT_INTERVAL_MS).clamp(IMU_SIDE_CAR_MIN_INTERVAL_MS, IMU_SIDE_CAR_MAX_INTERVAL_MS)
}

fn recording_stop_grace_ms() -> u64 {
    std::env::var("HELIOS_RECORDING_STOP_GRACE_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(RECORDING_STOP_GRACE_DEFAULT_MS)
        .clamp(RECORDING_STOP_GRACE_MIN_MS, RECORDING_STOP_GRACE_MAX_MS)
}

fn imu_sidecar_file_name(media_name: &str) -> String {
    format!("{media_name}.imu.jsonl.gz")
}

fn frame_timestamps_file_name(media_name: &str) -> String {
    format!("{media_name}.frame_ts.txt")
}

async fn media_meta_dir_async() -> Result<PathBuf, String> {
    storage::ensure_subdir_async("media-meta").await.map_err(|err| format!("failed to prepare media metadata directory: {err}"))
}

async fn load_media_metadata_entry(name: &str) -> Option<MediaMetadata> {
    let base = storage::sanitize_name(name)?;
    let dir = storage::ensure_subdir_async("media-meta").await.ok()?;
    let bytes = fs::read(dir.join(format!("{base}.json"))).await.ok()?;
    serde_json::from_slice::<MediaMetadata>(&bytes).ok()
}

async fn update_media_recording_fps_from_frame_ts(name: &str) -> Result<(), String> {
    let Some(base) = storage::sanitize_name(name) else {
        return Err("invalid media name".to_string());
    };
    let mut metadata = load_media_metadata_entry(&base).await.unwrap_or_default();
    let Some(frame_ts_name) = metadata.frame_timestamps_file_name.as_deref().and_then(storage::sanitize_name) else {
        return Ok(());
    };
    let meta_dir = media_meta_dir_async().await?;
    let media_dir = storage::ensure_subdir_async("media").await.map_err(|err| format!("failed to access media storage: {err}"))?;
    let candidates = [meta_dir.join(&frame_ts_name), media_dir.join(&frame_ts_name)];
    let mut derived_fps = None;
    for candidate in candidates {
        if let Some(fps) = derive_fps_from_frame_ts_file(&candidate).await {
            derived_fps = Some(fps);
            break;
        }
    }
    if let Some(fps) = derived_fps {
        metadata.fps = Some(fps);
        write_media_metadata(&base, metadata).await.map_err(|err| err.to_string())?;
    }
    Ok(())
}

async fn derive_fps_from_frame_ts_file(path: &StdPath) -> Option<f32> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(path).ok()?;
        let reader = std::io::BufRead::lines(std::io::BufReader::new(file));
        let mut count = 0u64;
        let mut first = None;
        let mut last = None;
        for line in reader {
            let line = line.ok()?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let ts = trimmed.parse::<u64>().ok()?;
            if first.is_none() {
                first = Some(ts);
            }
            last = Some(ts);
            count = count.saturating_add(1);
        }
        let first = first?;
        let last = last?;
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
    })
    .await
    .ok()
    .flatten()
}

async fn read_appended_frame_ts_values(path: &StdPath, offset: u64, carry: String) -> (u64, String, Vec<u64>) {
    let path = path.to_path_buf();
    let carry_fallback = carry.clone();
    tokio::task::spawn_blocking(move || {
        let mut file = match std::fs::File::open(&path) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return (offset, carry, Vec::new()),
            Err(_) => return (offset, carry, Vec::new()),
        };

        let len = file.metadata().map(|meta| meta.len()).unwrap_or(offset);
        let read_offset = offset.min(len);
        if file.seek(SeekFrom::Start(read_offset)).is_err() {
            return (offset, carry, Vec::new());
        }

        let mut chunk = String::new();
        if file.read_to_string(&mut chunk).is_err() {
            return (offset, carry, Vec::new());
        }
        let next_offset = file.stream_position().unwrap_or(len);

        let mut buffer = if read_offset < offset { String::new() } else { carry };
        buffer.push_str(&chunk);
        if buffer.is_empty() {
            return (next_offset, String::new(), Vec::new());
        }

        let has_trailing_newline = buffer.ends_with('\n');
        let mut lines: Vec<&str> = buffer.lines().collect();
        let mut next_carry = String::new();
        if !has_trailing_newline && let Some(partial) = lines.pop() {
            next_carry = partial.to_string();
        }

        let mut values = Vec::new();
        for line in lines {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(value) = trimmed.parse::<u64>() {
                values.push(value);
            }
        }
        (next_offset, next_carry, values)
    })
    .await
    .unwrap_or((offset, carry_fallback, Vec::new()))
}

async fn wait_for_frame_timestamps_settle(path: &StdPath) {
    let quiet_for = Duration::from_millis(IMU_SIDE_CAR_STOP_WAIT_QUIET_MS);
    let max_wait = Duration::from_millis(IMU_SIDE_CAR_STOP_WAIT_MAX_MS);
    let poll = Duration::from_millis(50);

    let started = Instant::now();
    let mut last_size = 0u64;
    let mut last_change = Instant::now();

    loop {
        if started.elapsed() >= max_wait {
            break;
        }
        let size = fs::metadata(path).await.map(|meta| meta.len()).unwrap_or(0);
        if size != last_size {
            last_size = size;
            last_change = Instant::now();
        } else if last_change.elapsed() >= quiet_for {
            break;
        }
        tokio::time::sleep(poll).await;
    }
}

fn select_imu_sample_at_or_before_wall_ms(imu_history: &VecDeque<(i64, lib_sensors::dto::ImuStatusPayload)>, frame_wall_ms: i64) -> Option<lib_sensors::dto::ImuStatusPayload> {
    imu_history.iter().rev().find_map(|(sample_wall_ms, imu)| (*sample_wall_ms <= frame_wall_ms).then(|| imu.clone()))
}

async fn update_media_imu_sidecar(name: &str, sidecar_name: Option<String>, samples: Option<u64>) -> Result<(), String> {
    let Some(base) = storage::sanitize_name(name) else {
        return Err("invalid media name".to_string());
    };
    let mut metadata = load_media_metadata_entry(&base).await.unwrap_or_default();
    metadata.imu_data_file_name = sidecar_name;
    metadata.imu_data_samples = samples;
    write_media_metadata(&base, metadata).await.map_err(|err| err.to_string())
}

async fn clear_media_imu_sidecar(name: &str) {
    if let Err(err) = update_media_imu_sidecar(name, None, None).await {
        warn!(media_name = %name, error = %err, "failed to clear IMU sidecar metadata");
    }
}

async fn cleanup_failed_recording_artifacts(session: &ActiveRecordingSession) -> Result<(), String> {
    let _ = fs::remove_file(&session.output_path).await;
    let Some(base) = storage::sanitize_name(&session.media_name) else {
        return Err("invalid media name".to_string());
    };
    let meta_dir = storage::ensure_subdir_async("media-meta").await.map_err(|err| format!("failed to prepare media metadata directory: {err}"))?;
    let _ = fs::remove_file(meta_dir.join(format!("{base}.json"))).await;
    let _ = fs::remove_file(meta_dir.join(imu_sidecar_file_name(&base))).await;
    let _ = fs::remove_file(session.output_path.with_file_name(frame_timestamps_file_name(&base))).await;
    Ok(())
}

async fn start_imu_sidecar_session(
    stream_id: Uuid,
    media_name: &str,
    recording_started_at_ms: i64,
    duration_ms: Option<u64>,
    interval_ms: u64,
    sidecar_file_name: String,
    frame_ts_path: PathBuf,
) -> Result<(), String> {
    if let Err(err) = stop_imu_sidecar_session(stream_id).await {
        warn!(stream_id = %stream_id, error = %err, "failed to finalize stale IMU sidecar session before starting a new one");
    }
    {
        let sessions = imu_sidecar_sessions().lock().await;
        if sessions.contains_key(&stream_id) {
            return Err("IMU sidecar recording already active for stream".to_string());
        }
    }

    let media_name_owned = media_name.to_string();
    let meta_dir = media_meta_dir_async().await?;
    let sidecar_path = meta_dir.join(&sidecar_file_name);
    let tmp_path = temp_output_path(&sidecar_path);
    let _ = fs::remove_file(&tmp_path).await;
    let _ = fs::remove_file(&sidecar_path).await;

    let conn = crate::ipc::peripherals::connect_sensors_stream().await.map_err(|err| format!("sensors stream unavailable: {err}"))?;
    let cancel = CancellationToken::new();
    let cancel_for_task = cancel.clone();
    let media_name_for_task = media_name_owned.clone();
    let sidecar_path_for_task = sidecar_path.clone();
    let tmp_path_for_task = tmp_path.clone();
    let frame_ts_path_for_task = frame_ts_path.clone();
    let join = tokio::spawn(async move {
        let (tx, rx) = std::sync::mpsc::channel::<ImuSidecarEvent>();
        let writer = tokio::spawn(write_imu_sidecar_stream(tmp_path_for_task, sidecar_path_for_task.clone(), rx));

        let scope = SensorScope::Device;
        let mut session = conn.session;
        let subscribe = helios_peripherals::ipc::SensorCommand::Subscribe { command_id: lib_ipc::types::CommandId::new(), scope: scope.clone() };
        session.send_command(conn.client.journal(), &subscribe).await.map_err(|err| format!("failed to subscribe to sensor snapshots: {err}"))?;

        let mut samples = 0u64;
        let mut imu_history: VecDeque<(i64, lib_sensors::dto::ImuStatusPayload)> = VecDeque::new();
        let mut frame_anchor_ts: Option<u64> = None;
        let mut frame_tail_offset = 0u64;
        let mut frame_tail_carry = String::new();
        let mut last_frame_ts: Option<u64> = None;
        let mut last_emitted_t_ms: Option<i64> = None;
        let mut sensor_stream_open = true;
        let mut stop_requested_at: Option<Instant> = None;
        let mut last_frame_seen_at: Option<Instant> = None;
        let mut frame_poll = tokio::time::interval(Duration::from_millis(5));
        frame_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let duration_limit_ms = duration_ms.filter(|ms| *ms > 0).map(|ms| (ms.min(i64::MAX as u64)) as i64);
        // Hard fallback to avoid runaway sidecar sessions if stream stop signaling is missed.
        // Main stop condition is cancellation on /recording/stop or stream teardown.
        let hard_deadline = duration_ms.filter(|ms| *ms > 0).map(|ms| Instant::now() + Duration::from_millis(ms.saturating_add(30_000)));

        'capture: loop {
            if let Some(deadline) = hard_deadline
                && Instant::now() >= deadline
            {
                break;
            }
            if let Some(stop_requested) = stop_requested_at {
                let now = Instant::now();
                if now.duration_since(stop_requested) >= Duration::from_millis(IMU_SIDE_CAR_STOP_TAIL_MAX_MS) {
                    break;
                }
                if let Some(last_frame_seen) = last_frame_seen_at
                    && now.duration_since(last_frame_seen) >= Duration::from_millis(IMU_SIDE_CAR_STOP_TAIL_IDLE_MS)
                {
                    break;
                }
            }

            tokio::select! {
                _ = cancel_for_task.cancelled(), if stop_requested_at.is_none() => {
                    stop_requested_at = Some(Instant::now());
                }
                _ = frame_poll.tick() => {
                    let (next_offset, next_carry, frame_ts_values) = read_appended_frame_ts_values(
                        &frame_ts_path_for_task,
                        frame_tail_offset,
                        std::mem::take(&mut frame_tail_carry),
                    ).await;
                    frame_tail_offset = next_offset;
                    frame_tail_carry = next_carry;
                    if frame_ts_values.is_empty() {
                        continue;
                    }
                    last_frame_seen_at = Some(Instant::now());

                    for raw_ts in frame_ts_values {
                        if let Some(prev_ts) = last_frame_ts
                            && raw_ts <= prev_ts
                        {
                            continue;
                        }

                        let anchor_ts = *frame_anchor_ts.get_or_insert(raw_ts);
                        let t_ms = raw_ts.saturating_sub(anchor_ts).min(i64::MAX as u64) as i64;
                        last_frame_ts = Some(raw_ts);

                        // Prefer direct wall-clock matching: pick the latest IMU sample received
                        // at/before the frame capture timestamp.
                        let frame_wall_ms = if raw_ts >= 1_000_000_000_000 {
                            raw_ts.min(i64::MAX as u64) as i64
                        } else {
                            recording_started_at_ms.saturating_add(t_ms).max(0)
                        };
                        let Some(imu) = select_imu_sample_at_or_before_wall_ms(&imu_history, frame_wall_ms) else {
                            // Do not use IMU samples newer than the frame timestamp; wait for a
                            // sample at/before this frame to avoid leading the video.
                            continue;
                        };
                        if let Some(last_t_ms) = last_emitted_t_ms
                            && t_ms.saturating_sub(last_t_ms) < interval_ms as i64
                        {
                            continue;
                        }
                        if tx.send(ImuSidecarEvent { t_ms, stream_id, imu }).is_err() {
                            break 'capture;
                        }
                        samples += 1;
                        last_emitted_t_ms = Some(t_ms);
                        if let Some(limit_ms) = duration_limit_ms
                            && t_ms >= limit_ms
                        {
                            break 'capture;
                        }
                    }
                }
                value = session.next_event(), if sensor_stream_open => {
                    match value {
                        Ok(Some(helios_peripherals::ipc::SensorEvent::Snapshot { scope: event_scope, values, .. })) if event_scope == scope => {
                            let imu = imu_status_from_snapshot(&values);
                            if imu.has_sample {
                                let sample_wall_ms = Utc::now().timestamp_millis();
                                imu_history.push_back((sample_wall_ms, imu));
                                while imu_history.len() > IMU_SIDE_CAR_HISTORY_MAX_SAMPLES {
                                    let _ = imu_history.pop_front();
                                }
                                while let Some((oldest_wall_ms, _)) = imu_history.front() {
                                    if sample_wall_ms.saturating_sub(*oldest_wall_ms) <= IMU_SIDE_CAR_HISTORY_WINDOW_MS {
                                        break;
                                    }
                                    let _ = imu_history.pop_front();
                                }
                            }
                        }
                        Ok(None) => {
                            sensor_stream_open = false;
                            warn!(stream_id = %stream_id, media_name = %media_name_for_task, "sensor stream ended during IMU sidecar capture; continuing with last cached sample");
                        }
                        Err(err) => {
                            sensor_stream_open = false;
                            warn!(stream_id = %stream_id, media_name = %media_name_for_task, error = ?err, "sensor stream error during IMU sidecar capture; continuing with last cached sample");
                        }
                        _ => {}
                    }
                }
            }
        }

        let _ = session.send_command(conn.client.journal(), &helios_peripherals::ipc::SensorCommand::Unsubscribe { command_id: lib_ipc::types::CommandId::new(), scope: scope.clone() }).await;

        drop(tx);
        let writer_summary = writer.await.map_err(|_| "IMU sidecar writer task failed".to_string())??;
        Ok(ImuSidecarSummary { samples, bytes: writer_summary.bytes })
    });

    let mut sessions = imu_sidecar_sessions().lock().await;
    if sessions.contains_key(&stream_id) {
        cancel.cancel();
        let _ = join.await;
        return Err("IMU sidecar recording already active for stream".to_string());
    }
    sessions.insert(stream_id, ImuSidecarSession { media_name: media_name_owned, sidecar_file_name, sidecar_path, cancel, join });
    Ok(())
}

async fn stop_imu_sidecar_session(stream_id: Uuid) -> Result<(), String> {
    let session = {
        let mut sessions = imu_sidecar_sessions().lock().await;
        sessions.remove(&stream_id)
    };
    let Some(session) = session else {
        return Ok(());
    };

    session.cancel.cancel();
    let summary = match session.join.await {
        Ok(Ok(summary)) => summary,
        Ok(Err(err)) => {
            let _ = fs::remove_file(&session.sidecar_path).await;
            clear_media_imu_sidecar(&session.media_name).await;
            return Err(err);
        }
        Err(_) => {
            let _ = fs::remove_file(&session.sidecar_path).await;
            clear_media_imu_sidecar(&session.media_name).await;
            return Err("IMU sidecar task join failed".to_string());
        }
    };

    if summary.samples == 0 || summary.bytes == 0 {
        let _ = fs::remove_file(&session.sidecar_path).await;
        clear_media_imu_sidecar(&session.media_name).await;
        return Ok(());
    }

    update_media_imu_sidecar(&session.media_name, Some(session.sidecar_file_name), Some(summary.samples)).await
}

#[derive(Debug, Clone, Copy)]
struct ImuSidecarWriterSummary {
    bytes: u64,
}

async fn write_imu_sidecar_stream(tmp_path: PathBuf, final_path: PathBuf, rx: std::sync::mpsc::Receiver<ImuSidecarEvent>) -> Result<ImuSidecarWriterSummary, String> {
    tokio::task::spawn_blocking(move || {
        if let Some(parent) = tmp_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| format!("failed to create IMU sidecar directory: {err}"))?;
        }
        let file = std::fs::File::create(&tmp_path).map_err(|err| format!("failed to open IMU sidecar temp file: {err}"))?;
        let mut enc = GzEncoder::new(std::io::BufWriter::new(file), Compression::default());
        while let Ok(event) = rx.recv() {
            serde_json::to_writer(&mut enc, &event).map_err(|err| format!("failed to encode IMU sidecar event: {err}"))?;
            enc.write_all(b"\n").map_err(|err| format!("failed to write IMU sidecar event: {err}"))?;
        }
        enc.finish().map_err(|err| format!("failed to finalize IMU sidecar stream: {err}"))?;

        if let Some(parent) = final_path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| format!("failed to create IMU sidecar output directory: {err}"))?;
        }
        if let Err(err) = std::fs::rename(&tmp_path, &final_path) {
            let _ = std::fs::remove_file(&final_path);
            std::fs::rename(&tmp_path, &final_path).map_err(|err2| format!("failed to rename IMU sidecar output: {err2}"))?;
            return Err(format!("failed to rename IMU sidecar output: {err}"));
        }
        let bytes = std::fs::metadata(&final_path).map_err(|err| format!("failed to stat IMU sidecar output: {err}"))?.len();
        Ok(ImuSidecarWriterSummary { bytes })
    })
    .await
    .map_err(|_| "IMU sidecar writer join failed".to_string())?
}

fn temp_output_path(output_path: &StdPath) -> PathBuf {
    let temp_ext = output_path.extension().and_then(|ext| ext.to_str()).filter(|ext| !ext.is_empty()).map(|ext| format!("{ext}.part")).unwrap_or_else(|| "part".to_string());
    output_path.with_extension(temp_ext)
}

fn parse_recording_options(req: &StartRecordingRequest) -> Result<(RecordingContainer, RecordingCodec), Box<ApiError>> {
    let mut codec = match req.codec.as_deref().unwrap_or("h265").trim().to_ascii_lowercase().as_str() {
        "h264" | "avc" => RecordingCodec::H264,
        "h265" | "hevc" => RecordingCodec::H265,
        other => return Err(Box::new(ApiError::bad_request(format!("unsupported codec: {other}")))),
    };

    let container_raw = req.container.as_deref().unwrap_or("raw").trim().to_ascii_lowercase();
    let container = match container_raw.as_str() {
        "mp4" => RecordingContainer::Mp4,
        "raw" | "annexb" => RecordingContainer::Raw,
        "h264" | "avc" => {
            codec = RecordingCodec::H264;
            RecordingContainer::Raw
        }
        "h265" | "hevc" => {
            codec = RecordingCodec::H265;
            RecordingContainer::Raw
        }
        other => return Err(Box::new(ApiError::bad_request(format!("unsupported container: {other}")))),
    };

    Ok((container, codec))
}

fn parse_capture_container(req: &CaptureShadowRecordingRequest) -> Result<RecordingContainer, Box<ApiError>> {
    let container_raw = req.container.as_deref().unwrap_or("raw").trim().to_ascii_lowercase();
    match container_raw.as_str() {
        "mp4" => Ok(RecordingContainer::Mp4),
        "raw" | "annexb" => Ok(RecordingContainer::Raw),
        "h264" | "avc" => Ok(RecordingContainer::Raw),
        "h265" | "hevc" => Ok(RecordingContainer::Raw),
        other => Err(Box::new(ApiError::bad_request(format!("unsupported container: {other}")))),
    }
}

fn parse_codec_override(value: Option<&str>) -> Result<Option<RecordingCodec>, Box<ApiError>> {
    let raw = match value.map(str::trim).filter(|v| !v.is_empty()) {
        Some(raw) => raw.to_ascii_lowercase(),
        None => return Ok(None),
    };
    let codec = match raw.as_str() {
        "h264" | "avc" => RecordingCodec::H264,
        "h265" | "hevc" => RecordingCodec::H265,
        other => return Err(Box::new(ApiError::bad_request(format!("unsupported codec: {other}")))),
    };
    Ok(Some(codec))
}

async fn infer_stream_codec(state: &crate::http::AppState, id: Uuid) -> Result<RecordingCodec, ApiError> {
    let streams = state.engine.list_streams().await.map_err(|err| ApiError::bad_gateway(format!("engine list failed: {err}")))?;
    let summary = streams.into_iter().find(|stream| stream.stream_id == id).ok_or_else(|| ApiError::not_found("stream not found"))?;
    let encoder_id = summary.manifest.encoder_id.as_deref().unwrap_or("");
    let normalized = encoder_id.to_ascii_lowercase();
    if normalized.contains("h265") || normalized.contains("hevc") {
        return Ok(RecordingCodec::H265);
    }
    if normalized.contains("h264") || normalized.contains("avc") {
        return Ok(RecordingCodec::H264);
    }
    Err(ApiError::bad_request("stream encoder is not h264/h265"))
}

async fn infer_recording_fps(state: &crate::http::AppState, id: Uuid, requested: Option<f32>) -> Option<f32> {
    if let Some(fps) = requested.filter(|value| value.is_finite() && *value > 0.0) {
        return Some(fps);
    }

    let metrics_fps = match state.engine.get_metrics(id).await {
        Ok(EngineEvent::Metrics { metrics, .. }) => metrics.encoder.as_ref().map(|encoder| encoder.fps as f32).or(Some(metrics.capture.fps as f32)).filter(|fps| fps.is_finite() && *fps > 0.0),
        _ => None,
    };
    if metrics_fps.is_some() {
        return metrics_fps;
    }

    let streams = state.engine.list_streams().await.ok()?;
    let summary = streams.into_iter().find(|stream| stream.stream_id == id)?;
    summary
        .manifest
        .capture
        .target_fps
        .map(|fps| fps as f32)
        .or_else(|| summary.manifest.capture.interval.map(|interval| interval.fps()))
        .or_else(|| summary.manifest.capture.mode.interval.map(|interval| interval.fps()))
        .filter(|fps| fps.is_finite() && *fps > 0.0)
}

fn parse_recording_settings(req: &StartRecordingRequest) -> Option<helios_engine::ipc::RecordingSettings> {
    let fps = req.fps.filter(|value| *value > 0.0);
    let bitrate_bps = req.bitrate_bps.filter(|value| *value > 0);
    let gop = req.gop.filter(|value| *value > 0);
    let quality = req.quality.filter(|value| *value <= 51);
    let max_width = req.max_width.filter(|value| *value > 0);
    let max_height = req.max_height.filter(|value| *value > 0);
    if fps.is_none() && bitrate_bps.is_none() && gop.is_none() && quality.is_none() && max_width.is_none() && max_height.is_none() {
        return None;
    }
    Some(helios_engine::ipc::RecordingSettings { fps, bitrate_bps, gop, quality, max_width, max_height })
}

fn recording_extension(container: RecordingContainer, codec: RecordingCodec) -> &'static str {
    match container {
        RecordingContainer::Mp4 => "mp4",
        RecordingContainer::Raw => match codec {
            RecordingCodec::H264 => "h264",
            RecordingCodec::H265 => "h265",
        },
    }
}

fn content_type_for_extension(ext: &str) -> String {
    match ext {
        "mp4" => "video/mp4".to_string(),
        "h264" => "video/h264".to_string(),
        "h265" => "video/h265".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}

fn ensure_extension(base: &str, ext: &str) -> Option<String> {
    let trimmed = base.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.to_ascii_lowercase().ends_with(&format!(".{ext}")) {
        return Some(trimmed.to_string());
    }
    let stem = trimmed.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(trimmed);
    Some(format!("{stem}.{ext}"))
}

async fn unique_media_name(dir: &std::path::Path, filename: &str) -> Result<String, std::io::Error> {
    let candidate = filename.to_string();
    if !tokio::fs::try_exists(dir.join(&candidate)).await.unwrap_or(false) {
        return Ok(candidate);
    }

    let suffix = Uuid::new_v4().simple().to_string();
    let mut attempts = 0;
    loop {
        attempts += 1;
        let next = append_suffix(filename, &suffix[..8], attempts);
        if !tokio::fs::try_exists(dir.join(&next)).await.unwrap_or(false) {
            return Ok(next);
        }
        if attempts > 5 {
            return Ok(format!("{}-{}", suffix, filename));
        }
    }
}

fn append_suffix(filename: &str, suffix: &str, attempt: usize) -> String {
    let suffix = if attempt <= 1 { suffix.to_string() } else { format!("{suffix}-{attempt}") };
    if let Some(stripped) = filename.strip_suffix(".tar.gz") {
        return format!("{stripped}-{suffix}.tar.gz");
    }
    if let Some((stem, ext)) = filename.rsplit_once('.') {
        return format!("{stem}-{suffix}.{ext}");
    }
    format!("{filename}-{suffix}")
}

#[cfg(test)]
mod tests {
    use super::append_suffix;

    #[test]
    fn append_suffix_preserves_double_extension() {
        assert_eq!(append_suffix("clip.tar.gz", "abc123", 1), "clip-abc123.tar.gz");
    }

    #[test]
    fn append_suffix_preserves_regular_extension() {
        assert_eq!(append_suffix("clip.mp4", "abc123", 1), "clip-abc123.mp4");
        assert_eq!(append_suffix("clip.mp4", "abc123", 2), "clip-abc123-2.mp4");
    }
}
