use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};

use crate::http::{AppState, peers, streams_persist};

use super::{
    PoseRotation, PoseVector, RigPose, UpdateCameraPoseRequest, UpdateRobotDimensionsRequest,
    layout::get_camera_layout,
    peer_forward::{forward_peer_camera_pose, update_running_stream_pose},
    state::{apply_robot_dimensions_patch, now_rfc3339, update_robot_dimensions_state},
};

#[utoipa::path(
    patch,
    path = "/device/robot-dimensions",
    tag = "Device",
    request_body = UpdateRobotDimensionsRequest,
    responses((status = 200, description = "Updated camera layout snapshot", body = super::CameraLayoutResponse))
)]
pub(crate) async fn update_robot_dimensions(State(state): State<AppState>, Json(patch_req): Json<UpdateRobotDimensionsRequest>) -> impl IntoResponse {
    if let Err(err) = update_robot_dimensions_state(move |robot| async move { apply_robot_dimensions_patch(robot, patch_req) }).await {
        return (StatusCode::BAD_GATEWAY, Json(crate::http::error::ErrorBody::new("bad_gateway", err.to_string()))).into_response();
    }

    get_camera_layout(State(state)).await.into_response()
}

#[utoipa::path(
    put,
    path = "/device/cameras/{camera_uid}/pose",
    tag = "Device",
    params(("camera_uid" = String, Path, description = "Camera UID (driver key)")),
    request_body = UpdateCameraPoseRequest,
    responses((status = 204, description = "Pose updated"))
)]
pub async fn update_camera_pose(State(state): State<AppState>, Path(camera_uid): Path<String>, Json(req): Json<UpdateCameraPoseRequest>) -> impl IntoResponse {
    if let Some((peer_id, remote_camera_uid)) = peers::parse_peer_scoped_ref(camera_uid.as_str()) {
        return forward_peer_camera_pose(&state, &peer_id, &remote_camera_uid, Some(req)).await;
    }

    let pose = sanitize_pose(req);

    match streams_persist::update_manifest_pose_by_camera_id(&camera_uid, Some(pose.clone())).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => match update_running_stream_pose(&state, &camera_uid, Some(pose)).await {
            Ok(true) => StatusCode::NO_CONTENT.into_response(),
            Ok(false) => (StatusCode::NOT_FOUND, Json(crate::http::error::ErrorBody::new("not_found", "camera not found"))).into_response(),
            Err(err) => (StatusCode::BAD_GATEWAY, Json(crate::http::error::ErrorBody::new("bad_gateway", err))).into_response(),
        },
        Err(err) => (StatusCode::BAD_GATEWAY, Json(crate::http::error::ErrorBody::new("bad_gateway", err.to_string()))).into_response(),
    }
}

#[utoipa::path(
    delete,
    path = "/device/cameras/{camera_uid}/pose",
    tag = "Device",
    params(("camera_uid" = String, Path, description = "Camera UID (driver key)")),
    responses((status = 204, description = "Pose cleared"))
)]
pub async fn clear_camera_pose(State(state): State<AppState>, Path(camera_uid): Path<String>) -> impl IntoResponse {
    if let Some((peer_id, remote_camera_uid)) = peers::parse_peer_scoped_ref(camera_uid.as_str()) {
        return forward_peer_camera_pose(&state, &peer_id, &remote_camera_uid, None).await;
    }

    match streams_persist::update_manifest_pose_by_camera_id(&camera_uid, None).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => match update_running_stream_pose(&state, &camera_uid, None).await {
            Ok(true) => StatusCode::NO_CONTENT.into_response(),
            Ok(false) => (StatusCode::NOT_FOUND, Json(crate::http::error::ErrorBody::new("not_found", "camera not found"))).into_response(),
            Err(err) => (StatusCode::BAD_GATEWAY, Json(crate::http::error::ErrorBody::new("bad_gateway", err))).into_response(),
        },
        Err(err) => (StatusCode::BAD_GATEWAY, Json(crate::http::error::ErrorBody::new("bad_gateway", err.to_string()))).into_response(),
    }
}

fn sanitize_pose(req: UpdateCameraPoseRequest) -> RigPose {
    let clamp = |v: f64| if v.is_finite() { v } else { 0.0 };
    RigPose {
        translation: PoseVector { x: clamp(req.translation.x), y: clamp(req.translation.y), z: clamp(req.translation.z) },
        rotation: PoseRotation { roll: clamp(req.rotation.roll), pitch: clamp(req.rotation.pitch), yaw: clamp(req.rotation.yaw) },
        updated_at: Some(now_rfc3339()),
    }
}
