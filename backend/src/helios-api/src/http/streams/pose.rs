use super::*;

async fn resolve_camera_uid(state: &AppState, stream_id: Uuid) -> Result<String, ApiError> {
    let streams = state.engine.list_streams().await.map_err(|err| ApiError::bad_gateway(err.to_string()))?;
    let stream = streams.iter().find(|summary| summary.stream_id == stream_id).ok_or_else(|| ApiError::not_found("stream not found"))?;
    let fallback = stream.manifest.identity.alias.as_deref().or(stream.manifest.identity.hardware_id.as_deref()).unwrap_or("");
    let camera_uid = rig_device::camera_uid_from_keys(&stream.manifest.capture.device_keys, Some(fallback)).unwrap_or_else(|| stream_id.to_string());
    Ok(camera_uid)
}

#[utoipa::path(
    put,
    path = "/streams/{id}/pose",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    request_body = UpdateCameraPoseRequest,
    responses((status = 204, description = "Pose updated"))
)]
pub async fn update_stream_pose(State(state): State<AppState>, Path(id): Path<Uuid>, body: Json<UpdateCameraPoseRequest>) -> Response {
    let Json(req) = body;
    match resolve_camera_uid(&state, id).await {
        Ok(camera_uid) => rig_device::update_camera_pose(State(state.clone()), Path(camera_uid), Json(req)).await.into_response(),
        Err(err) => err.into_response(),
    }
}

#[utoipa::path(
    delete,
    path = "/streams/{id}/pose",
    tag = "EngineStreams",
    params(("id" = Uuid, Path, description = "Stream ID")),
    responses((status = 204, description = "Pose cleared"))
)]
pub async fn clear_stream_pose(State(state): State<AppState>, Path(id): Path<Uuid>) -> Response {
    match resolve_camera_uid(&state, id).await {
        Ok(camera_uid) => rig_device::clear_camera_pose(State(state.clone()), Path(camera_uid)).await.into_response(),
        Err(err) => err.into_response(),
    }
}
