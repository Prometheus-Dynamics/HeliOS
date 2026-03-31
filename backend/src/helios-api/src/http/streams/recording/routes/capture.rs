use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::Utc;
use uuid::Uuid;

use crate::http::error::ApiError;
use crate::http::media::{MediaItem, MediaMetadata, write_media_metadata};
use crate::http::storage;
use crate::http::streams::recording::CaptureShadowRecordingRequest;
use crate::http::streams::recording::media::{content_type_for_extension, ensure_extension, recording_extension, unique_media_name};
use crate::http::streams::recording::options::{infer_stream_codec, parse_capture_container, parse_codec_override};
use crate::http::streams::util::{engine_error_body, map_client_error};
use helios_engine::ipc::{EngineErrorCode, EngineEvent, RecordingCodec, RecordingContainer};

#[utoipa::path(
    post,
    path = "/streams/{id}/recording/capture",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    request_body = crate::http::streams::recording::CaptureShadowRecordingRequest,
    responses(
        (status = 201, description = "Captured recording", body = MediaItem),
        (status = 400, description = "Invalid request", body = crate::http::streams::types::EngineErrorBody),
        (status = 502, description = "Engine error", body = crate::http::streams::types::EngineErrorBody)
    )
)]
pub(crate) async fn capture_shadow_recording(State(state): State<crate::http::AppState>, Path(id): Path<Uuid>, Json(req): Json<CaptureShadowRecordingRequest>) -> Response {
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

    let size_bytes = tokio::fs::metadata(&output_path).await.map(|meta| meta.len()).unwrap_or(0);
    if size_bytes == 0 {
        let _ = tokio::fs::remove_file(&output_path).await;
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
