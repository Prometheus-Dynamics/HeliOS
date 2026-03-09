use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, patch, put},
};
use chrono::Utc;
use once_cell::sync::Lazy;
use reqwest::header::ACCEPT;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::http::json_store;
use crate::http::peers;
use crate::http::storage;
use crate::http::streams::util::camera_id_for_manifest;
use crate::http::streams_persist;
use crate::ipc::IpcHandles;
pub use helios_engine::ipc::{PoseRotation, PoseVector, RigPose};

pub fn router() -> Router<std::sync::Arc<IpcHandles>> {
    Router::new()
        .route("/camera-layout", get(get_camera_layout))
        .route("/robot-dimensions", patch(update_robot_dimensions))
        .route("/cameras/{camera_uid}/pose", put(update_camera_pose).delete(clear_camera_pose))
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct RobotDimensions {
    pub width_m: f64,
    pub length_m: f64,
    pub bumper_height_m: f64,
    pub bumper_thickness_m: f64,
    pub ground_clearance_m: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CameraLayoutCameraResponse {
    #[serde(default)]
    pub stream_id: Option<String>,
    #[serde(default)]
    pub stream_alias: Option<String>,
    #[serde(default)]
    pub camera_uid: Option<String>,
    pub driver_camera_id: String,
    pub display_name: String,
    pub backend: String,
    #[serde(default)]
    pub hardware_id: Option<String>,
    #[serde(default)]
    pub pose: Option<RigPose>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct CameraLayoutResponse {
    pub robot: RobotDimensions,
    pub cameras: Vec<CameraLayoutCameraResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
pub struct UpdateRobotDimensionsRequest {
    pub width_m: Option<f64>,
    pub length_m: Option<f64>,
    pub bumper_height_m: Option<f64>,
    pub bumper_thickness_m: Option<f64>,
    pub ground_clearance_m: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UpdateCameraPoseRequest {
    pub translation: PoseVector,
    pub rotation: PoseRotation,
}

const DEFAULT_ROBOT: RobotDimensions = RobotDimensions { width_m: 0.6, length_m: 0.6, bumper_height_m: 0.127, bumper_thickness_m: 0.0508, ground_clearance_m: 0.0 };
static PEER_RIG_HTTP: Lazy<reqwest::Client> = Lazy::new(|| reqwest::Client::builder().redirect(reqwest::redirect::Policy::limited(3)).user_agent("HeliOS/rig-sync").build().expect("reqwest client"));

impl Default for RobotDimensions {
    fn default() -> Self {
        DEFAULT_ROBOT
    }
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

async fn robot_state_path() -> std::io::Result<std::path::PathBuf> {
    let dir = storage::ensure_subdir_async("rig").await?;
    Ok(dir.join("robot.json"))
}

async fn load_robot_dimensions() -> RobotDimensions {
    let path = match robot_state_path().await {
        Ok(path) => path,
        Err(_) => return RobotDimensions::default(),
    };
    json_store::read_json_or_default(&path).await
}

async fn update_robot_dimensions_state<F, Fut>(updater: F) -> std::io::Result<RobotDimensions>
where
    F: FnOnce(RobotDimensions) -> Fut,
    Fut: std::future::Future<Output = RobotDimensions>,
{
    let path = robot_state_path().await?;
    json_store::update_json(path, updater).await
}

fn backend_label(device: &helios_engine::capture::DiscoveredDevice) -> String {
    device.backends.first().map(|backend| format!("{:?}", backend.kind).to_lowercase()).unwrap_or_else(|| "unknown".to_string())
}

pub(crate) fn camera_uid_from_keys(keys: &[String], fallback: Option<&str>) -> Option<String> {
    if keys.is_empty() {
        return fallback.map(|value| value.to_string());
    }

    if let Some(key) = keys.iter().find(|key| key.contains('/')) {
        return Some(key.clone());
    }

    if let Some(key) = keys.iter().find(|key| key.contains(':')) {
        return Some(key.clone());
    }

    let mut normalized = keys.to_vec();
    normalized.sort();
    normalized.into_iter().next().or_else(|| fallback.map(|value| value.to_string()))
}

fn canonical_camera_id(device: &helios_engine::capture::DiscoveredDevice) -> String {
    camera_uid_from_keys(&device.identity.keys, Some(&device.identity.display)).unwrap_or_else(|| device.identity.display.clone())
}

fn camera_hardware_id(device: &helios_engine::capture::DiscoveredDevice) -> Option<String> {
    if device.identity.keys.is_empty() {
        None
    } else {
        let mut keys = device.identity.keys.clone();
        keys.sort();
        Some(keys.join("|"))
    }
}

fn stream_matches_device(stream: &helios_engine::ipc::StreamSummary, device: &helios_engine::capture::DiscoveredDevice) -> bool {
    let keys = &stream.manifest.capture.device_keys;
    if keys.is_empty() {
        return false;
    }
    device.identity.keys.iter().any(|key| keys.iter().any(|k| k == key))
}

#[utoipa::path(
    get,
    path = "/device/camera-layout",
    tag = "Device",
    responses((status = 200, description = "Camera + rig layout snapshot", body = CameraLayoutResponse))
)]
async fn get_camera_layout(State(state): State<std::sync::Arc<IpcHandles>>) -> impl IntoResponse {
    let robot = load_robot_dimensions().await;
    let mut pose_map = streams_persist::list_pose_map().await;

    let discovery = match tokio::task::spawn_blocking(helios_engine::capture::discover_devices_with_errors).await {
        Ok(result) => result,
        Err(_) => helios_engine::capture::DiscoveryResult { devices: Vec::new(), errors: vec!["camera discovery task failed".into()] },
    };

    let streams = state.engine.list_streams().await.unwrap_or_default();
    for stream in &streams {
        if stream.manifest.internal {
            continue;
        }
        let fallback = stream.manifest.identity.alias.as_deref().or(stream.manifest.identity.hardware_id.as_deref());
        let camera_uid = camera_uid_from_keys(&stream.manifest.capture.device_keys, fallback).unwrap_or_else(|| stream.stream_id.to_string());
        if camera_uid.trim().is_empty() {
            continue;
        }
        if pose_map.contains_key(&camera_uid) {
            continue;
        }
        if let Some(pose) = stream.manifest.pose.clone() {
            pose_map.insert(camera_uid, pose);
        }
    }

    let mut cameras = Vec::with_capacity(discovery.devices.len());
    let mut camera_uids = std::collections::HashSet::<String>::new();
    for device in &discovery.devices {
        let camera_id = canonical_camera_id(device);
        camera_uids.insert(camera_id.clone());
        let pose = pose_map.get(&camera_id).cloned();

        let stream = streams.iter().find(|s| stream_matches_device(s, device));
        let (stream_id, stream_alias) = match stream {
            Some(s) => (Some(s.stream_id.to_string()), s.manifest.identity.alias.clone()),
            None => (None, None),
        };

        cameras.push(CameraLayoutCameraResponse {
            stream_id,
            stream_alias,
            camera_uid: Some(camera_id.clone()),
            driver_camera_id: camera_id.clone(),
            display_name: device.identity.display.clone(),
            backend: backend_label(device),
            hardware_id: camera_hardware_id(device),
            pose,
        });
    }

    // Include streams that don't currently match a discovered device in the rig layout so they can be posed.
    // This covers "non-discoverable" sources (Netcam/File) and also cases where discovery is temporarily
    // incomplete while a stream is active.
    for stream in &streams {
        if stream.manifest.internal {
            continue;
        }

        let fallback = stream.manifest.identity.alias.as_deref().or(stream.manifest.identity.hardware_id.as_deref());
        let camera_uid = camera_uid_from_keys(&stream.manifest.capture.device_keys, fallback).unwrap_or_else(|| stream.stream_id.to_string());
        if camera_uid.trim().is_empty() {
            continue;
        }
        if camera_uids.contains(&camera_uid) {
            continue;
        }
        camera_uids.insert(camera_uid.clone());
        let pose = pose_map.get(&camera_uid).cloned();
        let display_name = stream.manifest.identity.alias.clone().or_else(|| stream.manifest.identity.hardware_id.clone()).unwrap_or_else(|| camera_uid.clone());
        cameras.push(CameraLayoutCameraResponse {
            stream_id: Some(stream.stream_id.to_string()),
            stream_alias: stream.manifest.identity.alias.clone(),
            camera_uid: Some(camera_uid.clone()),
            driver_camera_id: camera_uid.clone(),
            display_name,
            backend: format!("{:?}", stream.manifest.capture.backend).to_lowercase(),
            hardware_id: stream.manifest.identity.hardware_id.clone(),
            pose,
        });
    }

    let peer_streams = peers::snapshot_peer_streams().await;
    for peer_stream in peer_streams.streams {
        let peer_camera_uid = peer_stream.camera_uid.as_deref().map(str::trim).filter(|value| !value.is_empty()).map(|value| value.to_string()).unwrap_or_else(|| peer_stream.stream_ref.clone());
        if camera_uids.contains(&peer_camera_uid) {
            continue;
        }
        camera_uids.insert(peer_camera_uid.clone());

        let display_name = peer_stream
            .display_name
            .as_ref()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string())
            .or_else(|| peer_stream.stream_alias.clone())
            .unwrap_or_else(|| peer_stream.remote_stream_id.clone());
        cameras.push(CameraLayoutCameraResponse {
            stream_id: Some(peer_stream.stream_ref.clone()),
            stream_alias: peer_stream.stream_alias.clone(),
            camera_uid: Some(peer_camera_uid.clone()),
            driver_camera_id: peer_camera_uid,
            display_name,
            backend: format!("peer/{:?}", peer_stream.peer_kind).to_ascii_lowercase(),
            hardware_id: Some(peer_stream.remote_stream_id.clone()),
            pose: peer_stream.pose.as_ref().map(map_remote_peer_pose),
        });
    }

    cameras.sort_by(|a, b| a.display_name.cmp(&b.display_name));

    Json(CameraLayoutResponse { robot, cameras }).into_response()
}

#[utoipa::path(
    patch,
    path = "/device/robot-dimensions",
    tag = "Device",
    request_body = UpdateRobotDimensionsRequest,
    responses((status = 200, description = "Updated camera layout snapshot", body = CameraLayoutResponse))
)]
async fn update_robot_dimensions(State(state): State<std::sync::Arc<IpcHandles>>, Json(patch_req): Json<UpdateRobotDimensionsRequest>) -> impl IntoResponse {
    let apply = |field: &mut f64, value: Option<f64>, allow_zero: bool| -> bool {
        let Some(v) = value else {
            return false;
        };
        if !v.is_finite() {
            return false;
        }
        if allow_zero {
            if v < 0.0 {
                return false;
            }
        } else if v <= 0.0 {
            return false;
        }
        *field = v;
        true
    };

    if let Err(err) = update_robot_dimensions_state(move |mut robot| async move {
        let _ = apply(&mut robot.width_m, patch_req.width_m, false);
        let _ = apply(&mut robot.length_m, patch_req.length_m, false);
        let _ = apply(&mut robot.bumper_height_m, patch_req.bumper_height_m, false);
        let _ = apply(&mut robot.bumper_thickness_m, patch_req.bumper_thickness_m, false);
        let _ = apply(&mut robot.ground_clearance_m, patch_req.ground_clearance_m, true);
        robot
    })
    .await
    {
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
pub async fn update_camera_pose(State(state): State<std::sync::Arc<IpcHandles>>, Path(camera_uid): Path<String>, Json(req): Json<UpdateCameraPoseRequest>) -> impl IntoResponse {
    if let Some((peer_id, remote_camera_uid)) = peers::parse_peer_scoped_ref(camera_uid.as_str()) {
        return forward_peer_camera_pose(&peer_id, &remote_camera_uid, Some(req)).await;
    }

    let clamp = |v: f64| if v.is_finite() { v } else { 0.0 };
    let pose = RigPose {
        translation: PoseVector { x: clamp(req.translation.x), y: clamp(req.translation.y), z: clamp(req.translation.z) },
        rotation: PoseRotation { roll: clamp(req.rotation.roll), pitch: clamp(req.rotation.pitch), yaw: clamp(req.rotation.yaw) },
        updated_at: Some(now_rfc3339()),
    };

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
pub async fn clear_camera_pose(State(state): State<std::sync::Arc<IpcHandles>>, Path(camera_uid): Path<String>) -> impl IntoResponse {
    if let Some((peer_id, remote_camera_uid)) = peers::parse_peer_scoped_ref(camera_uid.as_str()) {
        return forward_peer_camera_pose(&peer_id, &remote_camera_uid, None).await;
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

fn map_remote_peer_pose(pose: &peers::PeerRemoteRigPose) -> RigPose {
    RigPose {
        translation: PoseVector { x: pose.translation.x, y: pose.translation.y, z: pose.translation.z },
        rotation: PoseRotation { roll: pose.rotation.roll, pitch: pose.rotation.pitch, yaw: pose.rotation.yaw },
        updated_at: pose.updated_at.clone(),
    }
}

fn encode_path_segment(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect::<String>().replace('+', "%20")
}

async fn forward_peer_camera_pose(peer_id: &str, remote_camera_uid: &str, req: Option<UpdateCameraPoseRequest>) -> axum::response::Response {
    let peers = peers::snapshot_peers().await;
    let Some(peer) = peers.into_iter().find(|peer| peer.id == peer_id) else {
        return (StatusCode::NOT_FOUND, Json(crate::http::error::ErrorBody::new("not_found", "peer not found"))).into_response();
    };
    if !matches!(peer.integration.kind, peers::PeerIntegrationKind::Helios) {
        return (StatusCode::BAD_REQUEST, Json(crate::http::error::ErrorBody::new("bad_request", "camera pose forwarding is only available for helios peers"))).into_response();
    }

    let encoded_uid = encode_path_segment(remote_camera_uid);
    let path = format!("/device/cameras/{encoded_uid}/pose");
    let url = match peers::peer_v1_url(&peer, &path) {
        Ok(url) => url,
        Err(err) => return (StatusCode::BAD_REQUEST, Json(crate::http::error::ErrorBody::new("bad_request", err))).into_response(),
    };

    let response = if let Some(payload) = req {
        PEER_RIG_HTTP.put(url).header(ACCEPT, "application/json").json(&payload).timeout(std::time::Duration::from_millis(1800)).send().await
    } else {
        PEER_RIG_HTTP.delete(url).header(ACCEPT, "application/json").timeout(std::time::Duration::from_millis(1800)).send().await
    };

    let Ok(response) = response else {
        return (StatusCode::BAD_GATEWAY, Json(crate::http::error::ErrorBody::new("bad_gateway", "peer request failed"))).into_response();
    };
    if response.status().is_success() {
        return StatusCode::NO_CONTENT.into_response();
    }
    if response.status().as_u16() == 404 {
        return (StatusCode::NOT_FOUND, Json(crate::http::error::ErrorBody::new("not_found", "peer camera not found"))).into_response();
    }

    let detail = response.text().await.unwrap_or_else(|_| "peer camera pose request failed".to_string());
    (StatusCode::BAD_GATEWAY, Json(crate::http::error::ErrorBody::new("bad_gateway", detail))).into_response()
}

async fn update_running_stream_pose(state: &std::sync::Arc<IpcHandles>, camera_uid: &str, pose: Option<RigPose>) -> Result<bool, String> {
    let streams = state.engine.list_streams().await.map_err(|err| err.to_string())?;
    for stream in streams {
        let fallback = stream.manifest.identity.alias.as_deref().or(stream.manifest.identity.hardware_id.as_deref());
        let stream_uid = camera_uid_from_keys(&stream.manifest.capture.device_keys, fallback).unwrap_or_else(|| stream.stream_id.to_string());
        if stream_uid != camera_uid {
            continue;
        }
        let mut manifest = stream.manifest.clone();
        manifest.pose = pose.clone();
        streams_persist::persist_manifest_checked(&camera_id_for_manifest(&manifest), Some(stream.stream_id), manifest)
            .await
            .map_err(|err| format!("updated live stream pose but failed to persist: {err}"))?;
        return Ok(true);
    }
    Ok(false)
}
