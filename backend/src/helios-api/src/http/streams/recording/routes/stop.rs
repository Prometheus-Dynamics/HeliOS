use std::time::Duration;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use tracing::{info, warn};
use uuid::Uuid;

use crate::http::streams::recording::media::{cleanup_failed_recording_artifacts, frame_timestamps_file_name, update_media_recording_fps_from_frame_ts};
use crate::http::streams::recording::options::recording_stop_grace_ms;
use crate::http::streams::recording::sidecar::{stop_imu_sidecar_session, wait_for_frame_timestamps_settle};
use crate::http::streams::util::{engine_error_body, map_client_error};
use helios_engine::ipc::{EngineErrorCode, EngineEvent};

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
pub(crate) async fn stop_recording(State(state): State<crate::http::AppState>, Path(id): Path<Uuid>) -> Response {
    let recording_runtime = state.services.streams.recording_runtime();
    let active_session = recording_runtime.remove_active_recording_session(id).await;
    let imu_stop_delay = Duration::from_millis(recording_stop_grace_ms());
    let engine_response = state.engine.stop_recording(id).await;
    if let Some(session) = active_session.as_ref() {
        let frame_ts_path = session.output_path.with_file_name(frame_timestamps_file_name(&session.media_name));
        wait_for_frame_timestamps_settle(&frame_ts_path).await;
    }
    if !imu_stop_delay.is_zero() {
        tokio::time::sleep(imu_stop_delay).await;
    }
    if let Err(err) = stop_imu_sidecar_session(recording_runtime.as_ref(), id).await {
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
