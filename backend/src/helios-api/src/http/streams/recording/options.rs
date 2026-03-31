use helios_engine::ipc::{EngineEvent, RecordingCodec, RecordingContainer};
use uuid::Uuid;

use crate::http::error::ApiError;
use crate::http::streams::recording::{
    CaptureShadowRecordingRequest, IMU_SIDE_CAR_DEFAULT_INTERVAL_MS, IMU_SIDE_CAR_MAX_INTERVAL_MS, IMU_SIDE_CAR_MIN_INTERVAL_MS, RECORDING_STOP_GRACE_DEFAULT_MS, RECORDING_STOP_GRACE_MAX_MS,
    RECORDING_STOP_GRACE_MIN_MS, StartRecordingRequest,
};

pub(super) fn clamp_imu_interval_ms(value: Option<u64>) -> u64 {
    value.unwrap_or(IMU_SIDE_CAR_DEFAULT_INTERVAL_MS).clamp(IMU_SIDE_CAR_MIN_INTERVAL_MS, IMU_SIDE_CAR_MAX_INTERVAL_MS)
}

pub(super) fn recording_stop_grace_ms() -> u64 {
    std::env::var("HELIOS_RECORDING_STOP_GRACE_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(RECORDING_STOP_GRACE_DEFAULT_MS)
        .clamp(RECORDING_STOP_GRACE_MIN_MS, RECORDING_STOP_GRACE_MAX_MS)
}

pub(super) fn parse_recording_options(req: &StartRecordingRequest) -> Result<(RecordingContainer, RecordingCodec), Box<ApiError>> {
    let mut codec = match req.codec.as_deref().unwrap_or("h265").trim().to_ascii_lowercase().as_str() {
        "h264" | "avc" => RecordingCodec::H264,
        "h265" | "hevc" => RecordingCodec::H265,
        other => {
            return Err(Box::new(ApiError::bad_request(format!("unsupported codec: {other}"))));
        }
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
        other => {
            return Err(Box::new(ApiError::bad_request(format!("unsupported container: {other}"))));
        }
    };

    Ok((container, codec))
}

pub(super) fn parse_capture_container(req: &CaptureShadowRecordingRequest) -> Result<RecordingContainer, Box<ApiError>> {
    let container_raw = req.container.as_deref().unwrap_or("raw").trim().to_ascii_lowercase();
    match container_raw.as_str() {
        "mp4" => Ok(RecordingContainer::Mp4),
        "raw" | "annexb" => Ok(RecordingContainer::Raw),
        "h264" | "avc" => Ok(RecordingContainer::Raw),
        "h265" | "hevc" => Ok(RecordingContainer::Raw),
        other => Err(Box::new(ApiError::bad_request(format!("unsupported container: {other}")))),
    }
}

pub(super) fn parse_codec_override(value: Option<&str>) -> Result<Option<RecordingCodec>, Box<ApiError>> {
    let raw = match value.map(str::trim).filter(|v| !v.is_empty()) {
        Some(raw) => raw.to_ascii_lowercase(),
        None => return Ok(None),
    };
    let codec = match raw.as_str() {
        "h264" | "avc" => RecordingCodec::H264,
        "h265" | "hevc" => RecordingCodec::H265,
        other => {
            return Err(Box::new(ApiError::bad_request(format!("unsupported codec: {other}"))));
        }
    };
    Ok(Some(codec))
}

pub(super) async fn infer_stream_codec(state: &crate::http::AppState, id: Uuid) -> Result<RecordingCodec, ApiError> {
    let streams = state.engine.list_streams().await.map_err(|err| ApiError::bad_gateway(format!("engine list failed: {err}")))?;
    let summary = streams.into_iter().find(|stream| stream.stream_id == id).ok_or_else(|| ApiError::not_found("stream not found"))?;
    let encoder_id = summary.manifest.encoder_id().unwrap_or("");
    let normalized = encoder_id.to_ascii_lowercase();
    if normalized.contains("h265") || normalized.contains("hevc") {
        return Ok(RecordingCodec::H265);
    }
    if normalized.contains("h264") || normalized.contains("avc") {
        return Ok(RecordingCodec::H264);
    }
    Err(ApiError::bad_request("stream encoder is not h264/h265"))
}

pub(super) async fn infer_recording_fps(state: &crate::http::AppState, id: Uuid, requested: Option<f32>) -> Option<f32> {
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

pub(super) fn parse_recording_settings(req: &StartRecordingRequest) -> Option<helios_engine::ipc::RecordingSettings> {
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
