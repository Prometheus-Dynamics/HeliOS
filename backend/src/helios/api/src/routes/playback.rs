//! Playback (`playback/`): `POST /v1/playback` opens a player on a recording or an image folder,
//! `GET` lists them with their replay state, `DELETE /v1/playback/{id}` closes one, and the
//! transport goes through the player's camera settings (`/v1/cameras/{resource_id}/settings`).
//! Seeks and runs ("Run on this recording") have their own routes here.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::json;

use super::{check_id, pipelines};
use crate::{
    SharedState,
    error::{ApiError, ApiResult},
    host::now_ms,
    playback::{
        context,
        player::StartOptions,
        request::{CreatePlayback, DEFAULT_FPS, PlayerView, SourceKind},
        runs::{self, RunReport},
        source::{self, Roots},
        transport::{self, PlaybackStatus, SeekRequest, status_from_values},
    },
    recordings,
};

/// The roots a source may name: the recordings directory (wherever it is mounted now), and the
/// configured folders.
async fn roots(state: &SharedState) -> ApiResult<Roots> {
    let settings = state.store.recording_settings().await?;
    let recordings = helios_recorder::storage::resolve_dir(&settings).map_err(|error| recordings::api_error(helios_recorder::RecorderError::Storage(error)))?;
    Ok(Roots { recordings, extra: state.config.playback.roots.clone() })
}

/// The player `id`, or 404.
async fn player(state: &SharedState, id: &str) -> ApiResult<PlayerView> {
    check_id(id)?;
    state.playback.get(id).await.ok_or_else(|| ApiError::not_found(format!("no playback {id}")))
}

/// The player's replay state now, from its camera service's controls.
async fn status_of(state: &SharedState, view: &PlayerView) -> Result<PlaybackStatus, String> {
    let socket = state.playback.socket(&view.id).await.ok_or_else(|| "the player is closed".to_string())?;
    let client = state.cameras.client(&view.resource_id, &socket).await.map_err(|error| error.message)?;
    let controls = client.controls().await.map_err(|error| error.message)?;
    let values = controls.into_iter().map(|control| (control.name, control.current.unwrap_or(serde_json::Value::Null))).collect();
    Ok(status_from_values(&values))
}

async fn with_status(state: &SharedState, mut view: PlayerView) -> PlayerView {
    match status_of(state, &view).await {
        Ok(status) => view.status = Some(status),
        Err(error) => view.status_error = Some(error),
    }
    view
}

/// Writes the replay controls `values` (by name) to the player's camera service.
async fn write_controls(state: &SharedState, view: &PlayerView, values: serde_json::Map<String, serde_json::Value>) -> ApiResult<()> {
    let socket = state.playback.socket(&view.id).await.ok_or_else(|| ApiError::not_found(format!("no playback {}", view.id)))?;
    let client = state.cameras.client(&view.resource_id, &socket).await?;
    let changes = client.resolve(&values).await?;
    client.apply(changes).await?;
    Ok(())
}

/// The start options the device does not take: a speed other than 1 (0 is the device's unpaced
/// replay already) and opening paused.
async fn apply_start(state: &SharedState, view: &PlayerView, request: &CreatePlayback) -> ApiResult<()> {
    if view.kind != SourceKind::Recording {
        return Ok(());
    }
    let mut values = serde_json::Map::new();
    let speed = request.speed_or_default();
    if speed != 0.0 && speed != 1.0 {
        values.insert(transport::CONTROL_SPEED.into(), json!(speed));
    }
    if request.start_paused {
        values.insert(transport::CONTROL_PLAYING.into(), json!(false));
    }
    if values.is_empty() { Ok(()) } else { write_controls(state, view, values).await }
}

/// `POST /v1/playback`: opens a player (201 with its view). Its camera resource is
/// `resource_id`, which a pipeline binds to like a live camera.
pub async fn create(State(state): State<SharedState>, Json(request): Json<CreatePlayback>) -> ApiResult<(StatusCode, Json<PlayerView>)> {
    request.validate()?;
    let resolved = source::resolve(&roots(&state).await?, &request.source)?;
    let options = StartOptions { looping: request.looping, speed: request.speed_or_default(), fps: request.fps.unwrap_or(DEFAULT_FPS) };
    let view = state.playback.open(resolved, options, now_ms()).await?;
    if let Err(error) = apply_start(&state, &view, &request).await {
        close_player(&state, &view.id).await?;
        return Err(error);
    }
    let view = with_status(&state, view).await;
    state.events.publish("playback", json!({ "change": "opened", "player": view }));
    Ok((StatusCode::CREATED, Json(view)))
}

/// `GET /v1/playback`: the open players, with their replay state.
pub async fn list(State(state): State<SharedState>) -> ApiResult<Json<Vec<PlayerView>>> {
    let views = state.playback.views().await;
    let mut out = Vec::with_capacity(views.len());
    for view in views {
        out.push(with_status(&state, view).await);
    }
    Ok(Json(out))
}

/// `GET /v1/playback/{id}`.
pub async fn get_one(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<Json<PlayerView>> {
    let view = player(&state, &id).await?;
    Ok(Json(with_status(&state, view).await))
}

/// Closes player `id`: its runs (pipelines bound to it) are removed first, then its service
/// stops and its resource leaves Orion.
async fn close_player(state: &SharedState, id: &str) -> ApiResult<PlayerView> {
    let view = state.playback.get(id).await.ok_or_else(|| ApiError::not_found(format!("no playback {id}")))?;
    for pipeline in runs::runs_of(state, &view).await? {
        pipelines::remove(State(state.clone()), Path(pipeline)).await?;
    }
    let closed = state.playback.close(id).await?.ok_or_else(|| ApiError::not_found(format!("no playback {id}")))?;
    context::forget(&state.store, &closed.resource_id).await?;
    state.events.publish("playback", json!({ "change": "closed", "id": closed.id }));
    Ok(closed)
}

/// `DELETE /v1/playback/{id}`: closes the player (204).
pub async fn close(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    check_id(&id)?;
    close_player(&state, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /v1/playback/{id}/seek`: `{"ms": 1500}` or `{"frame": 42}`; the frame at that time is the
/// next one delivered, also while paused. Returns the replay state.
pub async fn seek(State(state): State<SharedState>, Path(id): Path<String>, Json(request): Json<SeekRequest>) -> ApiResult<Json<PlaybackStatus>> {
    let view = player(&state, &id).await?;
    if view.kind != SourceKind::Recording {
        return Err(ApiError::unprocessable("an image sequence has no seek; it plays at its frame rate"));
    }
    let (name, value) = transport::seek_change(request.seek()?);
    write_controls(&state, &view, serde_json::Map::from_iter([(name.to_string(), value)])).await?;
    status_of(&state, &view).await.map(Json).map_err(ApiError::backend)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunBody {
    /// The pipeline to run on the recording (its id, as `/v1/pipelines` lists it).
    pub pipeline: String,
}

/// `POST /v1/playback/{id}/runs`: "Run on this recording": a copy of the pipeline with its camera
/// bindings moved to the player (201). The live pipeline is not changed.
pub async fn run(State(state): State<SharedState>, Path(id): Path<String>, Json(body): Json<RunBody>) -> ApiResult<(StatusCode, Json<RunReport>)> {
    check_id(&body.pipeline)?;
    let view = player(&state, &id).await?;
    let report = runs::run(&state, &view, &body.pipeline).await?;
    state.events.publish("playback", json!({ "change": "run", "player": view.id, "pipeline": report.pipeline.id }));
    Ok((StatusCode::CREATED, Json(report)))
}
