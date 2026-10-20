//! Session recordings (`/v1/recordings`): start one from a camera, stop it, list the active and
//! stored recordings, download or delete a stored part, and the settings they are made with.
//! The recording itself is helios-recorder's (`recordings.rs` builds its request).

use std::path::PathBuf;

use axum::{
    Json,
    body::{Body, Bytes},
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use helios_recorder::{PartSummary, RecorderSettings, RecordingStatus, RingStatus, catalog, storage};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;

use crate::{
    SharedState,
    error::{ApiError, ApiResult},
    recordings::{self, StartRecording},
};

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
    let started = recordings::start_recording(&state, request, None).await?;
    Ok((StatusCode::CREATED, Json(started)))
}

/// `POST /v1/recordings/{id}/stop`: asks the recording to end. Returns at once (`202`) with the
/// state `finishing`; the last part is completed in the background (poll `GET /v1/recordings`).
pub async fn stop(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<(StatusCode, Json<RecordingStatus>)> {
    let status = recordings::stop_recording(&state, &id).await?;
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
    crate::recording_rings::sync_in_background(state.clone());
    Ok(Json(settings))
}

/// `POST /v1/recordings/capture-last`: the body (all optional) names the last `seconds` of the
/// ring and its `camera` (the first camera with a ring by default). Writes the clip (`201` with its
/// summary); the ring and any running recording keep going.
#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CaptureLast {
    #[serde(default)]
    pub seconds: Option<u32>,
    #[serde(default)]
    pub camera: Option<String>,
}

pub async fn capture_last(State(state): State<SharedState>, body: Option<Json<CaptureLast>>) -> ApiResult<(StatusCode, Json<PartSummary>)> {
    let request = body.map(|Json(request)| request).unwrap_or_default();
    let summary = recordings::capture_clip(&state, request.camera, request.seconds).await?;
    Ok((StatusCode::CREATED, Json(summary)))
}

/// What an Eidos export wrote: its directory (beside the recording, `<id>.eidos`), its files and
/// the frames and the camera it holds.
#[derive(Debug, Serialize)]
pub struct EidosExport {
    pub dir: String,
    pub frames: u64,
    pub width: u32,
    pub height: u32,
    /// `calibrated` or `nominal`.
    pub camera: &'static str,
    /// The files' names in the directory (`<id>.raw`, `<id>.replay.json`, `<id>.frames.jsonl`).
    pub files: Vec<String>,
}

/// `POST /v1/recordings/{id}/export/eidos`: writes the recording's Eidos replay input into
/// `<id>.eidos/` beside it (`201` with what was written). `409` while it is being written, `422`
/// when the file is not a readable recording.
pub async fn export_eidos(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<(StatusCode, Json<EidosExport>)> {
    recordings::check_recording_id(&id)?;
    if is_running(&state, &id) {
        return Err(ApiError::conflict(format!("recording {id} is still running; stop it first")));
    }
    let (_, dir) = recordings_dir(&state).await?;
    let path = catalog::file_of(&dir, &id).ok_or_else(|| ApiError::not_found(format!("no recording {id}")))?;
    let out = dir.join(format!("{id}.eidos"));
    let target = out.clone();
    let summary = tokio::task::spawn_blocking(move || helios_recorder::export::export_eidos(&path, &target))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?
        .map_err(|error| ApiError::unprocessable(format!("not a readable recording: {error}")))?;
    let names = [&summary.raw, &summary.replay, &summary.frames_file].iter().filter_map(|path| path.file_name()).map(|name| name.to_string_lossy().into_owned()).collect();
    Ok((StatusCode::CREATED, Json(EidosExport { dir: out.display().to_string(), frames: summary.frames, width: summary.width, height: summary.height, camera: summary.camera_status, files: names })))
}
