use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use chrono::Utc;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path as StdPath;
use std::sync::Arc;
use tokio::sync::Mutex;
use utoipa::ToSchema;
use uuid::Uuid;

use helios_engine::ipc::{EngineEvent, RecordingSource, StreamSummary};
use helios_engine::stream::touch_stream_preview;
use lib_ipc::client::ClientTransportError;

use crate::http::AppState;
use crate::http::error::{ApiError, ApiResult};
use crate::http::media::{MediaItem, MediaMetadata, write_media_metadata};
use crate::http::pipelines;
use crate::http::storage;

use super::preview::latest_frame_jpeg_bytes;
use super::util::apply_pipeline_host_inputs_update;
use super::util::is_engine_unavailable;

static SNAPSHOT_LOCKS: Lazy<Mutex<HashMap<Uuid, Arc<Mutex<()>>>>> = Lazy::new(|| Mutex::new(HashMap::new()));

pub(crate) async fn snapshot_guard(stream_id: Uuid) -> Arc<Mutex<()>> {
    let mut guard = SNAPSHOT_LOCKS.lock().await;
    guard.entry(stream_id).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct CaptureSnapshotRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub source: Option<RecordingSource>,
    #[serde(default)]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct SetStreamCropRequest {
    pub crop: [f64; 4],
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct SetStreamCrosshairRequest {
    pub crosshair: [f64; 2],
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct SetStreamOrderingRequest {
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SetStreamCropResponse {
    pub stream_id: Uuid,
    pub crop: [f64; 4],
    pub roi_x: i64,
    pub roi_y: i64,
    pub roi_w: i64,
    pub roi_h: i64,
    pub pipeline_id: Option<Uuid>,
    pub disabled: bool,
    #[serde(default)]
    pub roi_inputs_used: bool,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SetStreamCrosshairResponse {
    pub stream_id: Uuid,
    pub crosshair: [f64; 2],
    pub crosshair_x: i64,
    pub crosshair_y: i64,
    pub enabled: bool,
    pub pipeline_id: Option<Uuid>,
    #[serde(default)]
    pub crosshair_inputs_used: bool,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SetStreamOrderingResponse {
    pub stream_id: Uuid,
    pub mode: String,
    pub pipeline_id: Option<Uuid>,
    #[serde(default)]
    pub ordering_inputs_used: bool,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct StreamInputUsageResponse {
    pub stream_id: Uuid,
    pub pipeline_id: Option<Uuid>,
    #[serde(default)]
    pub roi_inputs_used: bool,
    #[serde(default)]
    pub crosshair_inputs_used: bool,
    #[serde(default)]
    pub ordering_inputs_used: bool,
    #[serde(default)]
    pub roi_warnings: Vec<String>,
    #[serde(default)]
    pub crosshair_warnings: Vec<String>,
    #[serde(default)]
    pub ordering_warnings: Vec<String>,
}

#[utoipa::path(
    post,
    path = "/streams/{id}/snapshot",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    request_body = CaptureSnapshotRequest,
    responses(
        (status = 201, description = "Snapshot stored", body = MediaItem),
        (status = 400, description = "Invalid request", body = crate::http::streams::types::EngineErrorBody),
        (status = 404, description = "Stream not found", body = crate::http::streams::types::EngineErrorBody),
        (status = 502, description = "Engine error", body = crate::http::streams::types::EngineErrorBody),
        (status = 503, description = "Engine unavailable", body = crate::http::streams::types::EngineErrorBody)
    )
)]
pub async fn capture_snapshot(State(state): State<AppState>, Path(id): Path<Uuid>, Json(req): Json<CaptureSnapshotRequest>) -> ApiResult<impl IntoResponse> {
    let item = capture_snapshot_for_stream(&state, id, req).await?;
    Ok((StatusCode::CREATED, Json(item)))
}

#[utoipa::path(
    post,
    path = "/streams/{id}/crop",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    request_body = SetStreamCropRequest,
    responses(
        (status = 200, description = "Crop applied", body = SetStreamCropResponse),
        (status = 400, description = "Invalid request", body = crate::http::streams::types::EngineErrorBody),
        (status = 404, description = "Stream not found", body = crate::http::streams::types::EngineErrorBody),
        (status = 502, description = "Engine error", body = crate::http::streams::types::EngineErrorBody)
    )
)]
pub async fn set_crop(State(state): State<AppState>, Path(id): Path<Uuid>, Json(req): Json<SetStreamCropRequest>) -> ApiResult<impl IntoResponse> {
    let payload = apply_stream_crop(&state, id, req.crop).await?;
    Ok(Json(payload))
}

#[utoipa::path(
    post,
    path = "/streams/{id}/crosshair",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    request_body = SetStreamCrosshairRequest,
    responses(
        (status = 200, description = "Crosshair applied", body = SetStreamCrosshairResponse),
        (status = 400, description = "Invalid request", body = crate::http::streams::types::EngineErrorBody),
        (status = 404, description = "Stream not found", body = crate::http::streams::types::EngineErrorBody),
        (status = 502, description = "Engine error", body = crate::http::streams::types::EngineErrorBody)
    )
)]
pub async fn set_crosshair(State(state): State<AppState>, Path(id): Path<Uuid>, Json(req): Json<SetStreamCrosshairRequest>) -> ApiResult<impl IntoResponse> {
    let payload = apply_stream_crosshair(&state, id, req.crosshair, req.enabled).await?;
    Ok(Json(payload))
}

#[utoipa::path(
    post,
    path = "/streams/{id}/ordering",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    request_body = SetStreamOrderingRequest,
    responses(
        (status = 200, description = "Ordering applied", body = SetStreamOrderingResponse),
        (status = 400, description = "Invalid request", body = crate::http::streams::types::EngineErrorBody),
        (status = 404, description = "Stream not found", body = crate::http::streams::types::EngineErrorBody),
        (status = 502, description = "Engine error", body = crate::http::streams::types::EngineErrorBody)
    )
)]
pub async fn set_ordering(State(state): State<AppState>, Path(id): Path<Uuid>, Json(req): Json<SetStreamOrderingRequest>) -> ApiResult<impl IntoResponse> {
    let payload = apply_stream_ordering(&state, id, req.mode).await?;
    Ok(Json(payload))
}

#[utoipa::path(
    get,
    path = "/streams/{id}/pipeline/input-usage",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    responses(
        (status = 200, description = "Active graph host-input usage", body = StreamInputUsageResponse),
        (status = 404, description = "Stream not found", body = crate::http::streams::types::EngineErrorBody),
        (status = 502, description = "Engine error", body = crate::http::streams::types::EngineErrorBody)
    )
)]
pub async fn get_input_usage(State(state): State<AppState>, Path(id): Path<Uuid>) -> ApiResult<impl IntoResponse> {
    let payload = inspect_stream_input_usage(&state, id).await?;
    Ok(Json(payload))
}

pub(crate) async fn capture_snapshot_for_stream(state: &AppState, stream_id: Uuid, req: CaptureSnapshotRequest) -> ApiResult<MediaItem> {
    let kind = normalize_snapshot_kind(req.kind.as_deref()).map_err(ApiError::bad_request)?;
    let snapshot_lock = snapshot_guard(stream_id).await;
    let _snapshot_guard = snapshot_lock.lock().await;

    // Calibration snapshots default to RAW if caller did not specify a source.
    let mut source = req.source.clone();
    if kind == "calibration" && source.is_none() {
        source = Some(RecordingSource::Raw);
    }
    let jpeg = capture_snapshot_jpeg(state, stream_id, source).await?;
    save_snapshot_media(stream_id, req.name.as_deref(), kind, &jpeg).await
}

pub(crate) async fn apply_stream_crop(state: &AppState, stream_id: Uuid, crop: [f64; 4]) -> ApiResult<SetStreamCropResponse> {
    let summary = find_stream_summary(state, stream_id).await?;

    let (width, height) = active_mode_resolution(&summary).ok_or_else(|| ApiError::bad_request("stream mode resolution is unavailable"))?;
    let (roi_x, roi_y, roi_w, roi_h, disabled) = limelight_crop_to_roi(crop, width, height).map_err(ApiError::bad_request)?;

    let target_pipeline_id = summary.manifest.active_pipeline_id.or_else(|| summary.manifest.pipelines.first().map(|binding| binding.pipeline_id));
    let roi_usage = inspect_roi_input_usage(&summary, target_pipeline_id).await;

    let mut inputs: BTreeMap<String, Option<serde_json::Value>> = BTreeMap::new();
    inputs.insert("roi_x".to_string(), Some(serde_json::Value::from(roi_x)));
    inputs.insert("roi_y".to_string(), Some(serde_json::Value::from(roi_y)));
    inputs.insert("roi_w".to_string(), Some(serde_json::Value::from(roi_w)));
    inputs.insert("roi_h".to_string(), Some(serde_json::Value::from(roi_h)));
    let persisted_inputs = inputs.clone();

    match state.engine.set_pipeline_inputs(stream_id, target_pipeline_id, inputs).await {
        Ok(EngineEvent::Ack { .. }) => {}
        Ok(EngineEvent::Nack { code, reason, .. }) => {
            return Err(ApiError::new(StatusCode::BAD_GATEWAY, "bad_gateway", format!("engine rejected crop update ({code:?}): {reason}")));
        }
        Ok(other) => {
            return Err(ApiError::new(StatusCode::BAD_GATEWAY, "bad_gateway", format!("unexpected engine response while applying crop: {other:?}")));
        }
        Err(err) => {
            return Err(ApiError::new(StatusCode::BAD_GATEWAY, "bad_gateway", format!("failed to apply crop inputs: {err}")));
        }
    }

    if let Err(err) = super::util::persist_live_stream_manifest_update(state, stream_id, |manifest| {
        apply_pipeline_host_inputs_update(manifest, &persisted_inputs);
    })
    .await
    {
        return Err(ApiError::internal(err));
    }

    Ok(SetStreamCropResponse { stream_id, crop, roi_x, roi_y, roi_w, roi_h, pipeline_id: target_pipeline_id, disabled, roi_inputs_used: roi_usage.used, warnings: roi_usage.warnings })
}

pub(crate) async fn apply_stream_crosshair(state: &AppState, stream_id: Uuid, crosshair: [f64; 2], enabled: Option<bool>) -> ApiResult<SetStreamCrosshairResponse> {
    let summary = find_stream_summary(state, stream_id).await?;
    let (width, height) = active_mode_resolution(&summary).ok_or_else(|| ApiError::bad_request("stream mode resolution is unavailable"))?;
    let (crosshair_x, crosshair_y, normalized_crosshair) = limelight_crosshair_to_px(crosshair, width, height).map_err(ApiError::bad_request)?;
    let crosshair_enabled = enabled.unwrap_or(true);

    let target_pipeline_id = summary.manifest.active_pipeline_id.or_else(|| summary.manifest.pipelines.first().map(|binding| binding.pipeline_id));
    let crosshair_usage = inspect_host_input_usage(&summary, target_pipeline_id, &CROSSHAIR_KEYS, "crosshair", "crosshair controls", "Crosshair controls").await;
    let draw_crosshair_declared = host_bridge_declares_input(&summary, target_pipeline_id, "draw_crosshair").await;

    let mut inputs: BTreeMap<String, Option<serde_json::Value>> = BTreeMap::new();
    inputs.insert("crosshair_x".to_string(), Some(serde_json::Value::from(crosshair_x)));
    inputs.insert("crosshair_y".to_string(), Some(serde_json::Value::from(crosshair_y)));
    if draw_crosshair_declared {
        inputs.insert("draw_crosshair".to_string(), Some(serde_json::Value::from(crosshair_enabled)));
    }
    let mut persisted_inputs = inputs.clone();
    persisted_inputs.insert("draw_crosshair".to_string(), Some(serde_json::Value::from(crosshair_enabled)));

    match state.engine.set_pipeline_inputs(stream_id, target_pipeline_id, inputs).await {
        Ok(EngineEvent::Ack { .. }) => {}
        Ok(EngineEvent::Nack { code, reason, .. }) => {
            return Err(ApiError::new(StatusCode::BAD_GATEWAY, "bad_gateway", format!("engine rejected crosshair update ({code:?}): {reason}")));
        }
        Ok(other) => {
            return Err(ApiError::new(StatusCode::BAD_GATEWAY, "bad_gateway", format!("unexpected engine response while applying crosshair: {other:?}")));
        }
        Err(err) => {
            return Err(ApiError::new(StatusCode::BAD_GATEWAY, "bad_gateway", format!("failed to apply crosshair inputs: {err}")));
        }
    }

    if let Err(err) = super::util::persist_live_stream_manifest_update(state, stream_id, |manifest| {
        apply_pipeline_host_inputs_update(manifest, &persisted_inputs);
    })
    .await
    {
        return Err(ApiError::internal(err));
    }

    Ok(SetStreamCrosshairResponse {
        stream_id,
        crosshair: normalized_crosshair,
        crosshair_x,
        crosshair_y,
        enabled: crosshair_enabled,
        pipeline_id: target_pipeline_id,
        crosshair_inputs_used: crosshair_usage.used,
        warnings: crosshair_usage.warnings,
    })
}

pub(crate) async fn apply_stream_ordering(state: &AppState, stream_id: Uuid, mode: String) -> ApiResult<SetStreamOrderingResponse> {
    let summary = find_stream_summary(state, stream_id).await?;
    let normalized_mode = normalize_detections_order_mode(&mode).map_err(ApiError::bad_request)?;

    let target_pipeline_id = summary.manifest.active_pipeline_id.or_else(|| summary.manifest.pipelines.first().map(|binding| binding.pipeline_id));
    let ordering_usage = inspect_host_input_usage(&summary, target_pipeline_id, &ORDERING_KEYS, "Order", "ordering controls", "Ordering controls").await;

    let mut inputs: BTreeMap<String, Option<serde_json::Value>> = BTreeMap::new();
    inputs.insert("order_mode".to_string(), Some(serde_json::Value::from(normalized_mode.clone())));
    let persisted_inputs = inputs.clone();

    match state.engine.set_pipeline_inputs(stream_id, target_pipeline_id, inputs).await {
        Ok(EngineEvent::Ack { .. }) => {}
        Ok(EngineEvent::Nack { code, reason, .. }) => {
            return Err(ApiError::new(StatusCode::BAD_GATEWAY, "bad_gateway", format!("engine rejected ordering update ({code:?}): {reason}")));
        }
        Ok(other) => {
            return Err(ApiError::new(StatusCode::BAD_GATEWAY, "bad_gateway", format!("unexpected engine response while applying ordering: {other:?}")));
        }
        Err(err) => {
            return Err(ApiError::new(StatusCode::BAD_GATEWAY, "bad_gateway", format!("failed to apply ordering inputs: {err}")));
        }
    }

    if let Err(err) = super::util::persist_live_stream_manifest_update(state, stream_id, |manifest| {
        apply_pipeline_host_inputs_update(manifest, &persisted_inputs);
    })
    .await
    {
        return Err(ApiError::internal(err));
    }

    Ok(SetStreamOrderingResponse { stream_id, mode: normalized_mode, pipeline_id: target_pipeline_id, ordering_inputs_used: ordering_usage.used, warnings: ordering_usage.warnings })
}

async fn inspect_stream_input_usage(state: &AppState, stream_id: Uuid) -> ApiResult<StreamInputUsageResponse> {
    let summary = find_stream_summary(state, stream_id).await?;
    let target_pipeline_id = summary.manifest.active_pipeline_id.or_else(|| summary.manifest.pipelines.first().map(|binding| binding.pipeline_id));

    let roi_usage = inspect_roi_input_usage(&summary, target_pipeline_id).await;
    let crosshair_usage = inspect_host_input_usage(&summary, target_pipeline_id, &CROSSHAIR_KEYS, "crosshair", "crosshair controls", "Crosshair controls").await;
    let ordering_usage = inspect_host_input_usage(&summary, target_pipeline_id, &ORDERING_KEYS, "Order", "ordering controls", "Ordering controls").await;

    Ok(StreamInputUsageResponse {
        stream_id,
        pipeline_id: target_pipeline_id,
        roi_inputs_used: roi_usage.used,
        crosshair_inputs_used: crosshair_usage.used,
        ordering_inputs_used: ordering_usage.used,
        roi_warnings: roi_usage.warnings,
        crosshair_warnings: crosshair_usage.warnings,
        ordering_warnings: ordering_usage.warnings,
    })
}

async fn capture_snapshot_jpeg(state: &AppState, stream_id: Uuid, source: Option<RecordingSource>) -> ApiResult<Vec<u8>> {
    // Ensure the stream is considered "preview active" so the engine keeps a recent decoded frame.
    let _ = touch_stream_preview(stream_id);

    let quality = snapshot_jpeg_quality();
    let can_fallback_to_preview = source_can_use_preview_fallback(source.as_ref());
    let requested_source = source.as_ref().map(snapshot_source_label).unwrap_or("default");
    for attempt in 0..5u8 {
        match state.engine.snapshot_jpeg(stream_id, quality, source.clone()).await {
            Ok(bytes) => return Ok(bytes),
            Err(err) => {
                let unavailable = is_engine_unavailable(&err);
                // If the engine transport is down and this source maps to preview output anyway,
                // fall back immediately instead of waiting for retry timeouts.
                if can_fallback_to_preview && (attempt >= 4 || unavailable) {
                    tracing::warn!(stream_id = %stream_id, source = requested_source, error = %err, "engine snapshot_jpeg failed; falling back to shmem preview frame");
                    return latest_frame_jpeg_bytes(stream_id).await.map_err(|fallback_err| {
                        if unavailable {
                            ApiError::service_unavailable(format!("snapshot capture failed: {err}; preview fallback failed: {fallback_err}"))
                        } else {
                            ApiError::new(StatusCode::BAD_GATEWAY, "bad_gateway", fallback_err)
                        }
                    });
                }
                if attempt >= 4 {
                    return Err(snapshot_capture_error(&err));
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }
    }
    Err(ApiError::new(StatusCode::BAD_GATEWAY, "bad_gateway", "snapshot capture failed"))
}

fn source_can_use_preview_fallback(source: Option<&RecordingSource>) -> bool {
    matches!(source, None | Some(RecordingSource::Multiplex) | Some(RecordingSource::Raw) | Some(RecordingSource::Pipeline { .. }))
}

fn snapshot_source_label(source: &RecordingSource) -> &'static str {
    match source {
        RecordingSource::Multiplex => "multiplex",
        RecordingSource::Raw => "raw",
        RecordingSource::Pipeline { .. } => "pipeline",
    }
}

fn snapshot_capture_error(err: &ClientTransportError) -> ApiError {
    if is_engine_unavailable(err) {
        ApiError::service_unavailable(format!("snapshot capture failed: {err}"))
    } else {
        ApiError::new(StatusCode::BAD_GATEWAY, "bad_gateway", format!("snapshot capture failed: {err}"))
    }
}

async fn save_snapshot_media(stream_id: Uuid, name: Option<&str>, kind: &str, jpeg: &[u8]) -> ApiResult<MediaItem> {
    let dir = storage::ensure_subdir("media").map_err(|err| ApiError::internal(format!("failed to prepare media dir: {err}")))?;
    if jpeg.is_empty() {
        return Err(ApiError::internal("snapshot jpeg was empty"));
    }

    let ts_ms = Utc::now().timestamp_millis();
    let default_name = format!("{kind}_{stream_id}_{ts_ms}_{}.jpg", Uuid::new_v4().simple());
    let mut filename = name.and_then(storage::sanitize_name).unwrap_or(default_name);
    let mut path = dir.join(&filename);

    if tokio::fs::try_exists(&path).await.unwrap_or(false) {
        let stem = StdPath::new(&filename).file_stem().and_then(|s| s.to_str()).unwrap_or(kind);
        let ext = StdPath::new(&filename).extension().and_then(|s| s.to_str()).unwrap_or("jpg");
        filename = format!("{stem}_{}.{}", Uuid::new_v4().simple(), ext);
        path = dir.join(&filename);
    }

    let tmp_path = dir.join(format!(".{filename}.tmp"));
    tokio::fs::write(&tmp_path, jpeg).await.map_err(|err| ApiError::internal(format!("failed to write snapshot: {err}")))?;
    tokio::fs::rename(&tmp_path, &path).await.map_err(|err| ApiError::internal(format!("failed to finalize snapshot: {err}")))?;

    write_media_metadata(&filename, MediaMetadata { stream_id: Some(stream_id), kind: Some(kind.to_string()), captured_at_ms: Some(ts_ms), ..Default::default() }).await?;

    Ok(MediaItem {
        name: filename,
        size_bytes: jpeg.len() as u64,
        content_type: "image/jpeg".into(),
        description: None,
        tags: Vec::new(),
        stream_id: Some(stream_id),
        kind: Some(kind.to_string()),
        captured_at_ms: Some(ts_ms),
        width: None,
        height: None,
        fps: None,
        video_codec: None,
        label_attached: false,
        label_file_name: None,
        model_id: None,
        model_input_resolution: None,
        model_tensor_spec: None,
        imu_data_file_name: None,
        imu_data_samples: None,
    })
}

fn normalize_snapshot_kind(kind: Option<&str>) -> Result<&'static str, String> {
    let value = kind.map(str::trim).filter(|value| !value.is_empty());
    match value {
        None => Ok("snapshot"),
        Some(raw) if raw.eq_ignore_ascii_case("snapshot") => Ok("snapshot"),
        Some(raw) if raw.eq_ignore_ascii_case("calibration") => Ok("calibration"),
        Some(raw) => Err(format!("unsupported snapshot kind: {raw}")),
    }
}

fn snapshot_jpeg_quality() -> u8 {
    std::env::var("HELIOS_SNAPSHOT_JPEG_QUALITY").ok().and_then(|raw| raw.parse::<u8>().ok()).unwrap_or(95).clamp(1, 100)
}

async fn find_stream_summary(state: &AppState, stream_id: Uuid) -> ApiResult<StreamSummary> {
    let streams = state.engine.list_streams().await.map_err(|err| ApiError::new(StatusCode::BAD_GATEWAY, "bad_gateway", format!("failed to list streams: {err}")))?;
    streams.into_iter().find(|stream| stream.stream_id == stream_id).ok_or_else(|| ApiError::not_found("stream not found"))
}

fn active_mode_resolution(summary: &StreamSummary) -> Option<(u32, u32)> {
    let mode_id = summary.manifest.capture.mode.clone();
    summary.descriptor.modes.iter().find(|mode| mode.id == mode_id).or_else(|| summary.descriptor.modes.first()).map(|mode| (mode.format.resolution.width.get(), mode.format.resolution.height.get()))
}

fn limelight_crop_to_roi(crop: [f64; 4], width: u32, height: u32) -> Result<(i64, i64, i64, i64, bool), String> {
    if !crop.iter().all(|value| value.is_finite()) {
        return Err("crop values must be finite numbers".to_string());
    }

    let [x0, x1, y0, y1] = crop;

    let to_unit = |value: f64| ((value + 1.0) * 0.5).clamp(0.0, 1.0);

    let mut left = to_unit(x0);
    let mut right = to_unit(x1);
    let mut top = to_unit(y0);
    let mut bottom = to_unit(y1);

    if right < left {
        std::mem::swap(&mut left, &mut right);
    }
    if bottom < top {
        std::mem::swap(&mut top, &mut bottom);
    }

    let full_frame = left <= 0.0 && right >= 1.0 && top <= 0.0 && bottom >= 1.0;
    if full_frame {
        return Ok((0, 0, 0, 0, true));
    }

    let width_f = width as f64;
    let height_f = height as f64;

    let x = (left * width_f).floor().clamp(0.0, width_f) as i64;
    let y = (top * height_f).floor().clamp(0.0, height_f) as i64;
    let x2 = (right * width_f).ceil().clamp(0.0, width_f) as i64;
    let y2 = (bottom * height_f).ceil().clamp(0.0, height_f) as i64;

    let w = (x2 - x).max(1);
    let h = (y2 - y).max(1);

    Ok((x, y, w, h, false))
}

fn limelight_crosshair_to_px(crosshair: [f64; 2], width: u32, height: u32) -> Result<(i64, i64, [f64; 2]), String> {
    if !crosshair.iter().all(|value| value.is_finite()) {
        return Err("crosshair values must be finite numbers".to_string());
    }

    let [x_norm, y_norm] = crosshair;
    let x_norm = x_norm.clamp(-1.0, 1.0);
    let y_norm = y_norm.clamp(-1.0, 1.0);

    let width_f = width.max(1) as f64;
    let height_f = height.max(1) as f64;

    let mut x = (((x_norm + 1.0) * 0.5) * width_f).round() as i64;
    let mut y = (((y_norm + 1.0) * 0.5) * height_f).round() as i64;
    x = x.clamp(0, i64::from(width.saturating_sub(1)));
    y = y.clamp(0, i64::from(height.saturating_sub(1)));

    Ok((x, y, [x_norm, y_norm]))
}

fn normalize_detections_order_mode(raw: &str) -> Result<String, String> {
    let normalized = raw.trim().to_ascii_lowercase().replace(['-', ' '], "_");
    let canonical = match normalized.as_str() {
        "none" => "none",
        "largest_to_smallest" => "largest_to_smallest",
        "smallest_to_largest" => "smallest_to_largest",
        "top_most" => "top_most",
        "bottom_most" => "bottom_most",
        "left_most" => "left_most",
        "right_most" => "right_most",
        "top_left" => "top_left",
        "top_right" => "top_right",
        "bottom_left" => "bottom_left",
        "bottom_right" => "bottom_right",
        "center_most" => "center_most",
        "crosshair" => "crosshair",
        _ => {
            return Err(format!(
                "unsupported ordering mode: {raw} (expected one of: none, largest_to_smallest, smallest_to_largest, top_most, bottom_most, left_most, right_most, top_left, top_right, bottom_left, bottom_right, center_most, crosshair)"
            ));
        }
    };
    Ok(canonical.to_string())
}

#[derive(Debug, Default)]
struct RoiUsageSummary {
    used: bool,
    warnings: Vec<String>,
}

const ROI_KEYS: [&str; 4] = ["roi_x", "roi_y", "roi_w", "roi_h"];
const CROSSHAIR_KEYS: [&str; 3] = ["crosshair_x", "crosshair_y", "draw_crosshair"];
const ORDERING_KEYS: [&str; 1] = ["order_mode"];

async fn inspect_roi_input_usage(summary: &StreamSummary, pipeline_id: Option<Uuid>) -> RoiUsageSummary {
    inspect_host_input_usage(summary, pipeline_id, &ROI_KEYS, "ROI", "crop controls", "Crop controls").await
}

async fn inspect_host_input_usage(summary: &StreamSummary, pipeline_id: Option<Uuid>, keys: &[&str], key_label: &str, controls_label_lower: &str, controls_label_title: &str) -> RoiUsageSummary {
    let mut out = RoiUsageSummary::default();
    let Some(graph_json) = resolve_pipeline_graph_json(summary, pipeline_id).await else {
        out.warnings.push(format!("Unable to verify {key_label} wiring for the active graph; {controls_label_lower} may have no effect."));
        return out;
    };

    let Some(graph) = normalize_graph_root(&graph_json) else {
        out.warnings.push(format!("Active graph payload is not a Daedalus graph; {controls_label_lower} may have no effect."));
        return out;
    };
    let Some(nodes) = graph.get("nodes").and_then(|value| value.as_array()) else {
        out.warnings.push(format!("Active graph has no node list; {controls_label_lower} may have no effect."));
        return out;
    };
    let Some(edges) = graph.get("edges").and_then(|value| value.as_array()) else {
        out.warnings.push(format!("Active graph has no edge list; {controls_label_lower} may have no effect."));
        return out;
    };

    let host_bridge_indices: Vec<u64> = nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| {
            let id = node.get("id").and_then(|value| value.as_str())?;
            if id == "io.host_bridge" || id.ends_with(":io.host_bridge") { Some(index as u64) } else { None }
        })
        .collect();

    if host_bridge_indices.is_empty() {
        out.warnings.push(format!("Active graph has no io.host_bridge node; {controls_label_lower} are not wired."));
        return out;
    }

    let mut connected = std::collections::BTreeSet::new();
    for edge in edges {
        let from_node = edge.get("from").and_then(|value| value.get("node")).and_then(|value| value.as_u64());
        let from_port = edge.get("from").and_then(|value| value.get("port")).and_then(|value| value.as_str());
        let (Some(from_node), Some(from_port)) = (from_node, from_port) else {
            continue;
        };
        if host_bridge_indices.contains(&from_node) && keys.contains(&from_port) {
            connected.insert(from_port.to_string());
        }
    }

    out.used = connected.len() == keys.len();
    if connected.is_empty() {
        out.warnings.push(format!("{key_label} inputs are not consumed by the active graph. {controls_label_title} will not affect pipeline detections/overlay."));
    } else if connected.len() < keys.len() {
        let missing: Vec<&str> = keys.iter().copied().filter(|key| !connected.contains(*key)).collect();
        out.warnings.push(format!("{key_label} wiring is partial in the active graph (missing: {}).", missing.join(", ")));
    }

    out
}

async fn host_bridge_declares_input(summary: &StreamSummary, pipeline_id: Option<Uuid>, key: &str) -> bool {
    let Some(graph_json) = resolve_pipeline_graph_json(summary, pipeline_id).await else {
        return false;
    };
    let Some(graph) = normalize_graph_root(&graph_json) else {
        return false;
    };
    let Some(nodes) = graph.get("nodes").and_then(|value| value.as_array()) else {
        return false;
    };

    for node in nodes {
        let Some(id) = node.get("id").and_then(|value| value.as_str()) else {
            continue;
        };
        if id != "io.host_bridge" && !id.ends_with(":io.host_bridge") {
            continue;
        }

        if node.get("outputs").and_then(|value| value.as_array()).is_some_and(|outputs| outputs.iter().filter_map(|value| value.as_str()).any(|output| output == key)) {
            return true;
        }

        if let Some(spec) = node.get("metadata").and_then(|metadata| metadata.get("host_bridge_outputs")).and_then(|entry| entry.get("value")).and_then(|value| value.as_str()) {
            let mut declared = spec.split(',').map(str::trim).filter(|value| !value.is_empty()).filter_map(|entry| entry.split_once(':').map(|(name, _)| name));
            if declared.any(|name| name == key) {
                return true;
            }
        }
    }

    false
}

async fn resolve_pipeline_graph_json(summary: &StreamSummary, pipeline_id: Option<Uuid>) -> Option<serde_json::Value> {
    let pipeline_id = pipeline_id?;
    if let Some(binding) = summary.manifest.pipelines.iter().find(|binding| binding.pipeline_id == pipeline_id)
        && let Some(graph) = binding.pipeline_graph.as_ref()
    {
        return Some(graph.as_value().clone());
    }

    let doc = pipelines::load_graph_document(pipeline_id).await.ok()?;
    Some(doc.graph)
}

fn normalize_graph_root(value: &serde_json::Value) -> Option<&serde_json::Value> {
    let has_nodes_edges = |candidate: &serde_json::Value| candidate.get("nodes").is_some_and(|nodes| nodes.is_array()) && candidate.get("edges").is_some_and(|edges| edges.is_array());

    if has_nodes_edges(value) {
        return Some(value);
    }
    value.get("graph").filter(|candidate| has_nodes_edges(candidate))
}
