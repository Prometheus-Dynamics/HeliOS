//! `/v1/maps`: the saved maps (`crate::maps`): list, upload a map file, download, delete. And
//! `POST /v1/pipelines/{id}/map/save`: asks a pipeline's live mapper to save its map, which is then
//! kept here (the same save the `map/save` NetworkTables trigger makes, `save_pipeline`).
//!
//! The mapper saves on the rising edge of its `save` input, and gives `saved_map` on that tick. The
//! API reads that output from the live relay (`live_outputs`) only while it waits for it, so the
//! engine serializes a map only when one is being saved.

use std::time::Duration;

use axum::{
    Json,
    body::Bytes,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use helios_maps::SAVED_MAP_PORT;
use serde::Deserialize;
use serde_json::json;
use tokio::{
    sync::broadcast::error::RecvError,
    time::Instant,
};

use crate::{
    SharedState,
    error::{ApiError, ApiResult},
    live_outputs::LiveReceiver,
    maps::{self, MapEntry, Source},
    routes::{
        check_id,
        pipeline_bindings::BindingSource,
        pipelines::{self, PipelineSpec},
    },
};

/// How long a save waits for the mapper's map before saying it did not come.
pub const SAVE_WAIT: Duration = Duration::from_secs(10);
/// How long a save waits before it asks again. The engine saves for the connected live socket
/// only, and the relay connects when the first request is made, so the first request can be the
/// one that races the connection; a request still raised is not saved twice.
const SAVE_RETRY: Duration = Duration::from_millis(1500);

/// `GET /v1/maps`: the stored maps, newest first, with the store's size and limits.
pub async fn list(State(state): State<SharedState>) -> ApiResult<Json<serde_json::Value>> {
    let maps = state.maps.list().await?;
    let total_bytes: u64 = maps.iter().map(|entry| entry.size_bytes).sum();
    Ok(Json(json!({
        "maps": maps,
        "total_bytes": total_bytes,
        "max_total_bytes": maps::MAX_TOTAL_BYTES,
        "max_map_bytes": maps::MAX_MAP_BYTES,
    })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UploadQuery {
    /// The map's name (default: its id).
    pub name: Option<String>,
}

/// `POST /v1/maps?name=..`, the map file as the body: kept as a new map when it is a version 1
/// map file (`201`, the entry).
pub async fn upload(State(state): State<SharedState>, Query(query): Query<UploadQuery>, body: Bytes) -> ApiResult<(StatusCode, Json<MapEntry>)> {
    let entry = state.maps.put(query.name.as_deref(), &body, Source::default()).await?;
    publish(&state, &entry.id);
    Ok((StatusCode::CREATED, Json(entry)))
}

/// `GET /v1/maps/{id}`: the map file, to keep.
pub async fn download(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<Response> {
    check_id(&id)?;
    let bytes = state.maps.read(&id).await?;
    let disposition = format!("attachment; filename=\"{id}.eidmap\"");
    Ok(([(header::CONTENT_TYPE, "application/octet-stream".to_string()), (header::CONTENT_DISPOSITION, disposition)], bytes).into_response())
}

/// `DELETE /v1/maps/{id}`: forgets a map. Refused (`409`) while a pipeline starts from it.
pub async fn remove(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    check_id(&id)?;
    // Under the deploy lock: a pipeline cannot take the map between the check and the delete.
    let _deploying = state.deploys.lock().await;
    let view = state.orion.view().await?;
    let users = pipelines::pipelines_using_map(&view, &id);
    if !users.is_empty() {
        return Err(ApiError::conflict(format!("pipelines {} start from map {id}; choose another map for them first", users.join(", "))));
    }
    state.maps.delete(&id).await?;
    publish(&state, &id);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveRequest {
    /// The saved map's name (default: its id).
    #[serde(default)]
    pub name: Option<String>,
}

/// `POST /v1/pipelines/{id}/map/save` `{name?}`: saves the pipeline's map now and keeps it
/// (`201`, the entry). `409` when the pipeline has no live mapper or is not running; `503` when
/// the mapper did not give its map within [`SAVE_WAIT`].
pub async fn save_pipeline_map(State(state): State<SharedState>, Path(id): Path<String>, Json(request): Json<SaveRequest>) -> ApiResult<(StatusCode, Json<MapEntry>)> {
    check_id(&id)?;
    save_pipeline(&state, &id, request.name).await.map(|entry| (StatusCode::CREATED, Json(entry)))
}

/// Saves pipeline `id`'s map (see the module's docs) and keeps it under `name`. The route and the
/// NetworkTables trigger both make the save this way.
pub async fn save_pipeline(state: &SharedState, id: &str, name: Option<String>) -> ApiResult<MapEntry> {
    let view = state.orion.view().await?;
    let workload = pipelines::find_workload(&view, id).ok_or_else(|| ApiError::not_found(format!("no pipeline {id}")))?;
    if workload.desired_state != orion::control_plane::DesiredState::Running {
        return Err(ApiError::conflict(format!("pipeline {id} is not running: start it to save its map")));
    }
    let history = state.store.history(id).await?;
    let spec = history.latest().and_then(|revision| serde_json::from_value::<PipelineSpec>(revision.spec.clone()).ok());
    let Some(spec) = spec.filter(|spec| helios_maps::has_live_map(&spec.graph)) else {
        return Err(ApiError::conflict(format!("pipeline {id} has no live map to save")));
    };
    let camera = spec.bindings.values().find(|binding| binding.source == BindingSource::Resource && !binding.resource_id.is_empty()).map(|binding| binding.resource_id.clone());
    let pipeline = id.to_string();

    // Subscribed before the first request: the map cannot come before the listener is there.
    let mut lines = state.live.subscribe();
    let deadline = Instant::now() + SAVE_WAIT;
    loop {
        request_save(state, id).await?;
        let attempt = (Instant::now() + SAVE_RETRY).min(deadline);
        match tokio::time::timeout_at(attempt, next_saved_map(&mut lines, &pipeline)).await {
            Ok(bytes) => {
                let bytes = bytes?;
                let entry = state.maps.put(name.as_deref(), &bytes, Source { pipeline: Some(pipeline.clone()), camera }).await?;
                publish(state, &entry.id);
                return Ok(entry);
            }
            Err(_) if Instant::now() >= deadline => {
                return Err(ApiError::backend(format!("pipeline {id} did not give its map within {} s; it may still be starting or calibrating", SAVE_WAIT.as_secs())));
            }
            Err(_) => {}
        }
    }
}

/// Raises pipeline `id`'s save input once more: its workload's `map.save` counts the requests.
async fn request_save(state: &SharedState, id: &str) -> ApiResult<()> {
    // Writes whole workload records from this view: one writer at a time, as the deploys do.
    let _writing = state.deploys.lock().await;
    let view = state.orion.view().await?;
    let workload = pipelines::find_workload(&view, id).ok_or_else(|| ApiError::not_found(format!("no pipeline {id}")))?;
    let mut record = workload.clone();
    pipelines::set_map_saves(&mut record, pipelines::map_saves(workload) + 1);
    state.orion.apply(vec![orion::control_plane::DesiredStateMutation::PutWorkload(record)]).await
}

/// The next `saved_map` of pipeline `pipeline` on the live relay, as the map file's bytes.
async fn next_saved_map(lines: &mut LiveReceiver, pipeline: &str) -> ApiResult<Vec<u8>> {
    loop {
        match lines.recv().await {
            Ok(chunk) if chunk.contains(SAVED_MAP_PORT) => {
                if let Some(bytes) = chunk.lines().find_map(|line| saved_map_bytes(line, pipeline)) {
                    return Ok(bytes);
                }
            }
            Ok(_) | Err(RecvError::Lagged(_)) => {}
            Err(RecvError::Closed) => return Err(ApiError::backend("the pipeline outputs are not running")),
        }
    }
}

/// The map file in a live line of `pipeline`'s `saved_map` output, if the line is one. The bytes
/// are base64 (`{"value": "<base64>"}`, the output's JSON).
fn saved_map_bytes(line: &str, pipeline: &str) -> Option<Vec<u8>> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    if value["pipeline"] != pipeline || value["port"] != SAVED_MAP_PORT {
        return None;
    }
    let encoded = value["value"]["value"].as_str().or_else(|| value["value"].as_str())?;
    STANDARD.decode(encoded).ok()
}

/// Tells the other clients the maps changed (the `settings` event, domain `maps`).
fn publish(state: &SharedState, key: &str) {
    state.events.publish("settings", json!({ "domain": "maps", "key": key }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_map_line_gives_the_pipelines_map_bytes() {
        let bytes = b"EIDMAP\0\0 and more";
        let line = format!(r#"{{"pipeline":"live","port":"saved_map","observed_at_ms":1,"frame_ts_ns":2,"value":{{"value":"{}"}}}}"#, STANDARD.encode(bytes));
        assert_eq!(saved_map_bytes(&line, "live"), Some(bytes.to_vec()));
        assert_eq!(saved_map_bytes(&line, "other"), None, "another pipeline's map");
        let pose = r#"{"pipeline":"live","port":"pose","observed_at_ms":1,"frame_ts_ns":2,"value":{"valid":true}}"#;
        assert_eq!(saved_map_bytes(pose, "live"), None, "another output");
        assert_eq!(saved_map_bytes("not json", "live"), None);
    }

    #[test]
    fn a_saved_map_with_bare_base64_is_read_too() {
        let line = format!(r#"{{"pipeline":"live","port":"saved_map","value":"{}"}}"#, STANDARD.encode(b"EIDMAP\0\0"));
        assert_eq!(saved_map_bytes(&line, "live"), Some(b"EIDMAP\0\0".to_vec()));
    }
}
