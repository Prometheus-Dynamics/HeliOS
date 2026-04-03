use axum::{
    Json,
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use lib_runtime_policy::HELIOS_API_STREAMS_POLICY;
use once_cell::sync::Lazy;
use serde::Deserialize;
use std::convert::Infallible;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio::task;
use tokio::time::Duration;
use tokio_stream::wrappers::ReceiverStream;
use utoipa::IntoParams;
use uuid::Uuid;

use crate::http::AppState;
use crate::http::streams::snapshot::capture_snapshot_jpeg_without_preview_fallback;
use helios_engine::ipc::StreamManifest;
use helios_engine::ipc::{EngineErrorCode, RecordingSource};
use helios_engine::stream::{ShmemFrameHeader, read_latest_frame_with_header, read_latest_frame_with_header_if_newer_than, read_latest_header, touch_stream_preview};

use super::mjpeg;
use super::types::StreamFormatInfo;
use super::util::engine_error_body;
use super::util::fourcc_to_format;

#[derive(Debug, Clone, Default, Deserialize, IntoParams)]
pub(crate) struct PreviewSelectionQuery {
    #[serde(default)]
    pub pipeline: Option<Uuid>,
    #[serde(default)]
    pub output: Option<String>,
}

static PREVIEW_POLL: Lazy<std::time::Duration> = Lazy::new(|| {
    let policy = HELIOS_API_STREAMS_POLICY.resolve();
    policy.preview_poll_ms.map(std::time::Duration::from_millis).unwrap_or_else(|| std::time::Duration::from_millis(10))
});

static PREVIEW_OUTAGE: Lazy<std::time::Duration> = Lazy::new(|| std::time::Duration::from_millis(HELIOS_API_STREAMS_POLICY.resolve().preview_outage_ms));

fn is_mjpeg_fourcc(fourcc: styx::prelude::FourCc) -> bool {
    matches!(&fourcc.to_u32().to_le_bytes(), b"MJPG" | b"JPEG")
}

fn normalize_preview_output(output: Option<String>) -> Option<String> {
    let normalized = output.and_then(|raw| {
        let trimmed = raw.trim();
        if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
    });
    match normalized.as_deref() {
        Some(value) if value.eq_ignore_ascii_case("frame") || value.eq_ignore_ascii_case("raw") || value.eq_ignore_ascii_case("undistorted") => None,
        _ => normalized,
    }
}

fn override_recording_source(query: &PreviewSelectionQuery) -> Option<RecordingSource> {
    let output_key = normalize_preview_output(query.output.clone());
    if query.pipeline.is_none() && output_key.is_none() {
        return None;
    }
    Some(RecordingSource::Pipeline { pipeline_id: query.pipeline, output_key })
}

async fn query_targets_active_preview(state: &AppState, id: Uuid, query: &PreviewSelectionQuery) -> bool {
    let requested_output = normalize_preview_output(query.output.clone());
    if query.pipeline.is_none() && requested_output.is_none() {
        return true;
    }

    let Some(manifest) = state.services.streams.load_live_stream_manifest(state, id).await else {
        return false;
    };
    preview_targets_active_pipeline(&manifest, query)
}

fn preview_targets_active_pipeline(manifest: &StreamManifest, query: &PreviewSelectionQuery) -> bool {
    let active_pipeline_id = manifest.active_pipeline_id.or_else(|| manifest.pipelines.first().map(|binding| binding.pipeline_id));
    let active_output = normalize_preview_output(manifest.active_pipeline_output.clone());
    let requested_output = normalize_preview_output(query.output.clone());

    let requested_pipeline_id = query.pipeline.or(active_pipeline_id);
    let requested_output = requested_output.or_else(|| active_output.clone());

    requested_pipeline_id == active_pipeline_id && requested_output == active_output
}

fn snapshot_mjpeg_part_header() -> Bytes {
    Bytes::from_static(b"--frame\r\nContent-Type: image/jpeg\r\n\r\n")
}

fn snapshot_mjpeg_part_footer() -> Bytes {
    Bytes::from_static(b"\r\n")
}

fn snapshot_mjpeg_interval() -> Duration {
    Duration::from_millis(HELIOS_API_STREAMS_POLICY.resolve().snapshot_interval_ms)
}

fn touch_preview_if_due(id: Uuid, last_touch: &mut Instant, interval: Duration) {
    if last_touch.elapsed() < interval {
        return;
    }
    let _ = touch_stream_preview(id);
    *last_touch = Instant::now();
}

pub(crate) async fn stream_format(id: Uuid, query: PreviewSelectionQuery) -> Response {
    let header = match tokio::task::spawn_blocking(move || read_latest_header(id)).await {
        Ok(Ok(header)) => header,
        Ok(Err(_)) | Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    if header.len == 0 || header.fourcc.to_u32() == 0 {
        return StatusCode::NOT_FOUND.into_response();
    }
    if override_recording_source(&query).is_some() {
        return Json(StreamFormatInfo { fourcc: "MJPG".to_string(), format: "mjpeg".to_string(), width: header.width, height: header.height }).into_response();
    }
    Json(StreamFormatInfo { fourcc: header.fourcc.to_string(), format: fourcc_to_format(header.fourcc).to_string(), width: header.width, height: header.height }).into_response()
}

async fn preview_snapshot_stream(state: AppState, id: Uuid, source: RecordingSource) -> Response {
    let first_frame = match capture_snapshot_jpeg_without_preview_fallback(&state, id, Some(source.clone())).await {
        Ok(bytes) => bytes,
        Err(err) => return err.into_response(),
    };
    let interval = snapshot_mjpeg_interval();
    let body_stream = futures::stream::unfold((state, id, source, Some(first_frame), 0u8), move |(state, id, source, mut pending, phase)| async move {
        match phase {
            0 => {
                if pending.is_none() {
                    tokio::time::sleep(interval).await;
                    pending = match capture_snapshot_jpeg_without_preview_fallback(&state, id, Some(source.clone())).await {
                        Ok(bytes) => Some(bytes),
                        Err(_) => return None,
                    };
                }
                Some((Ok::<Bytes, Infallible>(snapshot_mjpeg_part_header()), (state, id, source, pending, 1)))
            }
            1 => {
                let jpeg = pending?;
                Some((Ok::<Bytes, Infallible>(Bytes::from(jpeg)), (state, id, source, None, 2)))
            }
            2 => Some((Ok::<Bytes, Infallible>(snapshot_mjpeg_part_footer()), (state, id, source, None, 0))),
            _ => None,
        }
    });
    let body = Body::from_stream(body_stream);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "multipart/x-mixed-replace; boundary=frame")
        .header(header::CACHE_CONTROL, "no-store, no-cache")
        .header(header::PRAGMA, "no-cache")
        .body(body)
        .unwrap()
}

pub(crate) async fn preview_stream(state: AppState, id: Uuid, query: PreviewSelectionQuery) -> Response {
    if !query_targets_active_preview(&state, id, &query).await
        && let Some(source) = override_recording_source(&query)
    {
        return preview_snapshot_stream(state, id, source).await;
    }
    let header = match tokio::time::timeout(
        Duration::from_secs(2),
        tokio::task::spawn_blocking(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut last_touch = Instant::now().checked_sub(Duration::from_secs(10)).unwrap_or_else(Instant::now);
            loop {
                touch_preview_if_due(id, &mut last_touch, Duration::from_millis(500));
                match read_latest_header(id) {
                    Ok(header) if header.len > 0 && header.fourcc.to_u32() != 0 => return Ok(header),
                    Ok(_) | Err(_) => {
                        if Instant::now() >= deadline {
                            return read_latest_header(id);
                        }
                        std::thread::sleep(Duration::from_millis(25));
                    }
                }
            }
        }),
    )
    .await
    {
        Ok(Ok(Ok(header))) => header,
        _ => return (StatusCode::NOT_FOUND, Json(engine_error_body(Some(EngineErrorCode::NotFound), "preview unavailable"))).into_response(),
    };
    if header.len == 0 || header.fourcc.to_u32() == 0 {
        return (StatusCode::NOT_FOUND, Json(engine_error_body(Some(EngineErrorCode::NotFound), "preview unavailable"))).into_response();
    }

    if is_mjpeg_fourcc(header.fourcc) {
        return mjpeg::mjpeg_stream(state.clone(), id).await;
    }

    let (tx, rx) = mpsc::channel::<Result<Bytes, Infallible>>(4);
    let poll = *PREVIEW_POLL;
    let outage = *PREVIEW_OUTAGE;
    task::spawn_blocking(move || {
        let mut last_seq = 0u64;
        let mut first_read_error_at: Option<Instant> = None;
        let mut last_touch = Instant::now().checked_sub(Duration::from_secs(10)).unwrap_or_else(Instant::now);
        loop {
            if tx.is_closed() {
                break;
            }
            if last_touch.elapsed() >= Duration::from_millis(500) {
                let _ = touch_stream_preview(id);
                last_touch = Instant::now();
            }
            let next = match read_latest_frame_with_header_if_newer_than(id, last_seq) {
                Ok(next) => next,
                Err(_) => {
                    let now = Instant::now();
                    first_read_error_at.get_or_insert(now);
                    // Transient shmem errors can occur during reconfigure/restart; keep the
                    // preview connection alive and only give up after an extended outage.
                    if first_read_error_at.is_some_and(|ts| now.saturating_duration_since(ts) >= outage) {
                        break;
                    }
                    std::thread::sleep(poll);
                    continue;
                }
            };
            first_read_error_at = None;
            let Some((hdr, payload)) = next else {
                std::thread::sleep(poll);
                continue;
            };
            if hdr.len == 0 || hdr.fourcc.to_u32() == 0 || hdr.seq == last_seq {
                std::thread::sleep(poll);
                continue;
            }
            last_seq = hdr.seq;
            let len = payload.len().min(u32::MAX as usize) as u32;
            if tx.blocking_send(Ok(Bytes::copy_from_slice(&len.to_le_bytes()))).is_err() {
                break;
            }
            if tx.blocking_send(Ok(Bytes::from(payload))).is_err() {
                break;
            }
        }
    });

    let body = Body::from_stream(ReceiverStream::new(rx));
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CACHE_CONTROL, "no-store, no-cache")
        .header(header::PRAGMA, "no-cache")
        .header("x-encoded-fourcc", header.fourcc.to_string())
        .header("x-encoded-format", fourcc_to_format(header.fourcc))
        .header("x-encoded-width", header.width.to_string())
        .header("x-encoded-height", header.height.to_string())
        .body(body)
        .unwrap()
}

pub(crate) async fn frame_jpeg(state: AppState, id: Uuid, query: PreviewSelectionQuery) -> Response {
    if query_targets_active_preview(&state, id, &query).await {
        match latest_frame_jpeg_bytes(id).await {
            Ok(jpeg) => return (StatusCode::OK, [(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "no-store, no-cache")], Body::from(jpeg)).into_response(),
            Err(_) => {}
        }
    }

    let source = override_recording_source(&query).unwrap_or(RecordingSource::Raw);
    match capture_snapshot_jpeg_without_preview_fallback(&state, id, Some(source)).await {
        Ok(jpeg) => (StatusCode::OK, [(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "no-store, no-cache")], Body::from(jpeg)).into_response(),
        Err(err) => err.into_response(),
    }
}

pub(crate) async fn latest_frame_jpeg_bytes(id: Uuid) -> Result<Vec<u8>, String> {
    match tokio::task::spawn_blocking(move || {
        // Allow enough time for a newly-started stream to produce its first preview frame.
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut last_touch = Instant::now().checked_sub(Duration::from_secs(10)).unwrap_or_else(Instant::now);
        loop {
            touch_preview_if_due(id, &mut last_touch, Duration::from_millis(500));
            match read_latest_frame_with_header(id) {
                Ok(value) => return Ok(value),
                Err(err) => {
                    if Instant::now() >= deadline {
                        return Err(err.to_string());
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
    })
    .await
    {
        Ok(Ok((header, bytes))) => preview_jpeg_from_encoded(header, bytes),
        Ok(Err(err)) => Err(err),
        Err(err) => Err(err.to_string()),
    }
}

fn preview_jpeg_from_encoded(header: ShmemFrameHeader, bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    match &header.fourcc.to_u32().to_le_bytes() {
        b"MJPG" | b"JPEG" => return Ok(bytes),
        _ => {}
    }
    Err("preview unavailable".into())
}

#[cfg(test)]
mod tests {
    use super::{PreviewSelectionQuery, preview_targets_active_pipeline};
    use helios_engine::capture::{BackendHandle, BackendKind, CaptureConfig, ModeId};
    use helios_engine::identity::DeviceIdentity;
    use helios_engine::ipc::{StreamManifest, StreamPipelineBinding};
    use styx::prelude::{ColorSpace, FourCc, MediaFormat, Resolution};
    use uuid::Uuid;

    fn manifest_with_active(pipeline_id: Uuid, output: Option<&str>) -> StreamManifest {
        let format = MediaFormat::new(FourCc::new(*b"RGB3"), Resolution::new(1, 1).expect("valid resolution"), ColorSpace::Srgb);
        StreamManifest {
            schema_version: helios_engine::ipc::CURRENT_STREAM_CONFIG_SCHEMA_VERSION,
            identity: DeviceIdentity { id: None, alias: None, hardware_id: None },
            capture: CaptureConfig {
                device_keys: vec![],
                device_identity: None,
                backend: BackendKind::Virtual,
                handle: BackendHandle::Virtual,
                mode: ModeId { format, interval: None },
                target_fps: None,
                interval: None,
                controls: vec![],
                enable_tdn_output: false,
            },
            host_buffer: 2,
            internal: false,
            pipeline_enabled: true,
            pipelines: vec![StreamPipelineBinding { pipeline_id, pipeline_graph: None, pipeline_output: output.map(str::to_string), pipeline_patch: None }],
            active_pipeline_id: Some(pipeline_id),
            active_pipeline_output: output.map(str::to_string),
            pipeline_layout: None,
            pipeline_wires: Vec::new(),
            pipeline_host_inputs: std::collections::BTreeMap::new(),
            calibration: None,
            pose: None,
            encoder: helios_engine::ipc::RequestedEncoderConfig::default(),
            decoder: helios_engine::ipc::RequestedDecoderConfig::default(),
            preview_jpeg_quality: 30,
            recording_mode: helios_engine::ipc::StreamRecordingMode::shadow_buffer(helios_engine::ipc::default_shadow_recording_codec()),
            start_on_boot: false,
        }
    }

    #[test]
    fn preview_targets_active_pipeline_matches_explicit_active_selection() {
        let pipeline_id = Uuid::new_v4();
        let manifest = manifest_with_active(pipeline_id, Some("overlay"));
        let query = PreviewSelectionQuery { pipeline: Some(pipeline_id), output: Some("overlay".to_string()) };

        assert!(preview_targets_active_pipeline(&manifest, &query));
    }

    #[test]
    fn preview_targets_active_pipeline_rejects_non_active_output() {
        let pipeline_id = Uuid::new_v4();
        let manifest = manifest_with_active(pipeline_id, Some("overlay"));
        let query = PreviewSelectionQuery { pipeline: Some(pipeline_id), output: Some("mask".to_string()) };

        assert!(!preview_targets_active_pipeline(&manifest, &query));
    }

    #[test]
    fn preview_targets_active_pipeline_treats_raw_frame_aliases_as_default_output() {
        let pipeline_id = Uuid::new_v4();
        let manifest = manifest_with_active(pipeline_id, None);
        let query = PreviewSelectionQuery { pipeline: None, output: Some("frame".to_string()) };

        assert!(preview_targets_active_pipeline(&manifest, &query));
    }
}
