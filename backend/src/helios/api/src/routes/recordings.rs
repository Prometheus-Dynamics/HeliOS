//! Session recordings (`/v1/recordings`): start one from a camera, stop it, list the active and
//! stored recordings, download or delete a stored part, and the settings they are made with.
//! The recording itself is helios-recorder's (`recordings.rs` builds its request).

use std::{path::PathBuf, time::Duration};

use axum::{
    Json,
    body::{Body, Bytes},
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use helios_recorder::{ClipRequest, PartSummary, RecorderSettings, RecordingStatus, RingStatus, catalog, storage};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;

use crate::{
    SharedState,
    error::{ApiError, ApiResult},
    recording_rings,
    recordings::{self, StartRecording},
    routes::cameras,
};

use super::check_id;

/// Read chunk for downloads.
const DOWNLOAD_CHUNK: usize = 1 << 20;

#[derive(Debug, Serialize)]
pub struct StorageFacts {
    pub dir: String,
    pub free_bytes: Option<u64>,
    pub reserve_bytes: u64,
}

/// `GET /v1/recordings`: the running recording (if any), the stored parts, and the storage.
#[derive(Debug, Serialize)]
pub struct Listing {
    pub active: Option<RecordingStatus>,
    pub stored: Vec<catalog::StoredRecording>,
    /// `null` with `storage_error` when the recordings directory cannot be used now.
    pub storage: Option<StorageFacts>,
    pub storage_error: Option<String>,
    /// The capture-last rings (one per camera the settings ask for).
    pub rings: Vec<RingStatus>,
}

async fn recordings_dir(state: &SharedState) -> ApiResult<(RecorderSettings, PathBuf)> {
    let settings = state.store.recording_settings().await?;
    let dir = storage::resolve_dir(&settings).map_err(|error| recordings::api_error(helios_recorder::RecorderError::Storage(error)))?;
    Ok((settings, dir))
}

pub async fn list(State(state): State<SharedState>) -> ApiResult<Json<Listing>> {
    let settings = state.store.recording_settings().await?;
    let active = state.recorder.status().filter(|status| status.state.is_active());
    let (storage, storage_error, stored) = match storage::resolve_dir(&settings) {
        Ok(dir) => {
            let free_bytes = storage::free_bytes(&dir).ok();
            let stored = catalog::list(&dir).map_err(|error| ApiError::internal(error.to_string()))?;
            (Some(StorageFacts { dir: dir.display().to_string(), free_bytes, reserve_bytes: settings.reserve_bytes }), None, stored)
        }
        Err(error) => (None, Some(error.to_string()), Vec::new()),
    };
    let rings = state.recorder.rings();
    Ok(Json(Listing { active, stored, storage, storage_error, rings }))
}

/// `POST /v1/recordings`: starts one (`201` with its status). `409` while another runs, `507`
/// below the free-space reserve, `503` when the camera's service is not there.
pub async fn start(State(state): State<SharedState>, Json(request): Json<StartRecording>) -> ApiResult<(StatusCode, Json<RecordingStatus>)> {
    check_id(&request.camera)?;
    let stored = state.store.recording_settings().await?;
    let settings = recordings::effective_settings(&stored, &request);
    settings.validate().map_err(ApiError::unprocessable)?;
    let recording = recordings::request_for(&state, &request.camera, settings).await?;
    let started = {
        let state = state.clone();
        tokio::task::spawn_blocking(move || state.recorder.start(recording)).await.map_err(|error| ApiError::internal(error.to_string()))?.map_err(recordings::api_error)?
    };
    if let Some(host) = state.recorder.host() {
        drop(recordings::spawn_session_tasks(state.clone(), host, started.id.clone()));
    }
    recordings::publish(&state, &started);
    Ok((StatusCode::CREATED, Json(started)))
}

/// `POST /v1/recordings/{id}/stop`: asks the recording to end. Returns at once (`202`) with the
/// state `finishing`; the last part is completed in the background (poll `GET /v1/recordings`).
pub async fn stop(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<(StatusCode, Json<RecordingStatus>)> {
    recordings::check_recording_id(&id)?;
    let status = state.recorder.stop(&id).map_err(recordings::api_error)?;
    recordings::publish(&state, &status);
    Ok((StatusCode::ACCEPTED, Json(status)))
}

/// Whether `id` names a part of the running recording (which must not be read or deleted yet).
fn is_running(state: &SharedState, id: &str) -> bool {
    state.recorder.status().is_some_and(|status| status.state.is_active() && (status.id == id || status.file.as_deref().is_some_and(|file| file.strip_suffix(".mcap") == Some(id))))
}

/// `GET /v1/recordings/{id}`: a stored part, streamed (`application/octet-stream`, an MCAP).
pub async fn download(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<Response> {
    recordings::check_recording_id(&id)?;
    if is_running(&state, &id) {
        return Err(ApiError::conflict(format!("recording {id} is still running")));
    }
    let (_, dir) = recordings_dir(&state).await?;
    let path = catalog::file_of(&dir, &id).ok_or_else(|| ApiError::not_found(format!("no recording {id}")))?;
    let file = tokio::fs::File::open(&path).await?;
    let length = file.metadata().await?.len();
    let stream = futures_util::stream::try_unfold(file, |mut file| async move {
        let mut chunk = vec![0u8; DOWNLOAD_CHUNK];
        let read = file.read(&mut chunk).await?;
        if read == 0 {
            return Ok(None);
        }
        chunk.truncate(read);
        Ok::<_, std::io::Error>(Some((Bytes::from(chunk), file)))
    });
    let headers = [
        (header::CONTENT_TYPE, "application/octet-stream".to_string()),
        (header::CONTENT_LENGTH, length.to_string()),
        (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{id}.mcap\"")),
        (header::CACHE_CONTROL, "no-store".to_string()),
    ];
    Ok((headers, Body::from_stream(stream)).into_response())
}

/// `DELETE /v1/recordings/{id}`: deletes a stored part and its summary (`409` while it is being
/// written).
pub async fn remove(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    recordings::check_recording_id(&id)?;
    if is_running(&state, &id) {
        return Err(ApiError::conflict(format!("recording {id} is still running; stop it first")));
    }
    let (_, dir) = recordings_dir(&state).await?;
    if catalog::delete(&dir, &id).map_err(|error| ApiError::internal(error.to_string()))? { Ok(StatusCode::NO_CONTENT) } else { Err(ApiError::not_found(format!("no recording {id}"))) }
}

/// `GET /v1/recordings/settings`.
pub async fn get_settings(State(state): State<SharedState>) -> ApiResult<Json<RecorderSettings>> {
    Ok(Json(state.store.recording_settings().await?))
}

/// `PUT /v1/recordings/settings`: the whole settings object (defaults for the missing fields).
pub async fn put_settings(State(state): State<SharedState>, Json(settings): Json<RecorderSettings>) -> ApiResult<Json<RecorderSettings>> {
    settings.validate().map_err(ApiError::unprocessable)?;
    state.store.put_recording_settings(&settings).await?;
    state.events.publish("recording_settings", serde_json::to_value(&settings).unwrap_or_default());
    recording_rings::sync_in_background(state.clone());
    Ok(Json(settings))
}

/// `POST /v1/recordings/capture-last`: the body (all optional) names the last `seconds` of the
/// ring and its `camera` (the first camera with a ring by default).
#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CaptureLast {
    #[serde(default)]
    pub seconds: Option<u32>,
    #[serde(default)]
    pub camera: Option<String>,
}

/// Writes the ring's last frames to a stored recording (`201` with its summary). The recording
/// (if one runs) and the ring keep going. `409` when the camera has no ring enabled, `503` when
/// its ring cannot run (the camera is missing or refused), `422` for a window longer than the ring.
pub async fn capture_last(State(state): State<SharedState>, body: Option<Json<CaptureLast>>) -> ApiResult<(StatusCode, Json<PartSummary>)> {
    let request = body.map(|Json(request)| request).unwrap_or_default();
    let settings = state.store.recording_settings().await?;
    let camera = match request.camera {
        Some(camera) => camera,
        None => settings.capture_last_cameras.first().cloned().ok_or_else(|| ApiError::conflict("no camera has a capture-last ring; enable one in the recording settings"))?,
    };
    check_id(&camera)?;
    if !settings.capture_last_cameras.contains(&camera) {
        return Err(ApiError::conflict(format!("capture-last is not enabled for {camera}")));
    }
    if let Some(seconds) = request.seconds.filter(|seconds| *seconds == 0 || *seconds > settings.capture_last_seconds) {
        return Err(ApiError::unprocessable(format!("seconds must be 1 to {} (the ring's length)", settings.capture_last_seconds)));
    }
    let statuses = recording_rings::sync(&state).await?;
    match statuses.iter().find(|status| status.camera == camera) {
        Some(status) if status.state == "running" => {}
        Some(status) => return Err(ApiError::backend(status.error.clone().unwrap_or_else(|| "the capture-last ring is not running".into()))),
        None => return Err(ApiError::backend(format!("{camera} has no camera service to ring"))),
    }
    let (view, record) = cameras::find(&state, &camera).await?;
    let socket = cameras::frames_socket(&record.endpoints).ok_or_else(|| ApiError::backend(format!("{camera} has no Styx frames endpoint")))?;
    let info = recordings::recording_info(&state, &camera, &socket, &view).await;
    let (board, camera_label, commit) = recordings::file_names(&state, &camera, &record);
    let dir = storage::resolve_dir(&settings).map_err(|error| recordings::api_error(helios_recorder::RecorderError::Storage(error)))?;
    let last = request.seconds.map(|seconds| Duration::from_secs(u64::from(seconds)));
    let recorder_state = state.clone();
    let summary = tokio::task::spawn_blocking(move || {
        let clip = ClipRequest {
            camera_id: &camera,
            board: &board,
            camera_label: &camera_label,
            commit: &commit,
            info: &info,
            compression: settings.compression,
            queue_bytes: settings.queue_bytes,
            max_fps: settings.max_fps,
        };
        recorder_state.recorder.capture_last(last, &clip, &dir)
    })
    .await
    .map_err(|error| ApiError::internal(error.to_string()))?
    .map_err(recordings::api_error)?;
    Ok((StatusCode::CREATED, Json(summary)))
}
