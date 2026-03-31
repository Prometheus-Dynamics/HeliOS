use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use chrono::Utc;
use tracing::info;
use uuid::Uuid;

use crate::http::error::ApiError;
use crate::http::media::{MediaItem, MediaMetadata, write_media_metadata};
use crate::http::storage;
use crate::http::streams::recording::media::{
    clear_media_imu_sidecar, content_type_for_extension, ensure_extension, frame_timestamps_file_name, imu_sidecar_file_name, recording_extension, unique_media_name,
};
use crate::http::streams::recording::options::{clamp_imu_interval_ms, infer_recording_fps, infer_stream_codec, parse_recording_options, parse_recording_settings};
use crate::http::streams::recording::sidecar::start_imu_sidecar_session;
use crate::http::streams::recording::{ActiveRecordingSession, StartRecordingRequest, active_recording_sessions};
use crate::http::streams::util::{engine_error_body, map_client_error};
use helios_engine::ipc::{EngineErrorCode, EngineEvent, RecordingCodec, RecordingSource};

#[utoipa::path(
    post,
    path = "/streams/{id}/recording/start",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    request_body = crate::http::streams::recording::StartRecordingRequest,
    responses(
        (status = 201, description = "Recording started", body = MediaItem),
        (status = 400, description = "Invalid request", body = crate::http::streams::types::EngineErrorBody),
        (status = 502, description = "Engine error", body = crate::http::streams::types::EngineErrorBody)
    )
)]
pub(crate) async fn start_recording(State(state): State<crate::http::AppState>, Path(id): Path<Uuid>, Json(req): Json<StartRecordingRequest>) -> Response {
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
