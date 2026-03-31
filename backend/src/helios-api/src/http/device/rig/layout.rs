use std::collections::HashSet;

use axum::{Json, extract::State, response::IntoResponse};

use crate::http::{AppState, peers, streams_persist};

use super::{
    CameraLayoutCameraResponse, CameraLayoutResponse,
    state::{backend_label, camera_hardware_id, camera_uid_from_keys, canonical_camera_id, load_robot_dimensions, peer_camera_layout_entry, stream_matches_device},
};

#[utoipa::path(
    get,
    path = "/device/camera-layout",
    tag = "Device",
    responses((status = 200, description = "Camera + rig layout snapshot", body = CameraLayoutResponse))
)]
pub(crate) async fn get_camera_layout(State(state): State<AppState>) -> impl IntoResponse {
    let robot = load_robot_dimensions().await;
    let mut pose_map = streams_persist::list_pose_map().await;

    let discovery =
        state.engine.discover_devices().await.unwrap_or_else(|err| helios_engine::capture::DiscoveryResult { devices: Vec::new(), errors: vec![format!("camera discovery failed: {err}")] });

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
    let mut camera_uids = HashSet::<String>::new();
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
            driver_camera_id: camera_id,
            display_name: device.identity.display.clone(),
            backend: backend_label(device),
            hardware_id: camera_hardware_id(device),
            pose,
        });
    }

    for stream in &streams {
        if stream.manifest.internal {
            continue;
        }

        let fallback = stream.manifest.identity.alias.as_deref().or(stream.manifest.identity.hardware_id.as_deref());
        let camera_uid = camera_uid_from_keys(&stream.manifest.capture.device_keys, fallback).unwrap_or_else(|| stream.stream_id.to_string());
        if camera_uid.trim().is_empty() || camera_uids.contains(&camera_uid) {
            continue;
        }
        camera_uids.insert(camera_uid.clone());
        let pose = pose_map.get(&camera_uid).cloned();
        let display_name = stream.manifest.identity.alias.clone().or_else(|| stream.manifest.identity.hardware_id.clone()).unwrap_or_else(|| camera_uid.clone());
        cameras.push(CameraLayoutCameraResponse {
            stream_id: Some(stream.stream_id.to_string()),
            stream_alias: stream.manifest.identity.alias.clone(),
            camera_uid: Some(camera_uid.clone()),
            driver_camera_id: camera_uid,
            display_name,
            backend: format!("{:?}", stream.manifest.capture.backend).to_lowercase(),
            hardware_id: stream.manifest.identity.hardware_id.clone(),
            pose,
        });
    }

    let peer_streams = peers::snapshot_peer_streams(&state).await;
    for peer_stream in peer_streams.streams {
        let entry = peer_camera_layout_entry(&peer_stream);
        if camera_uids.contains(&entry.driver_camera_id) {
            continue;
        }
        camera_uids.insert(entry.driver_camera_id.clone());
        cameras.push(entry);
    }

    cameras.sort_by(|a, b| a.display_name.cmp(&b.display_name));

    Json(CameraLayoutResponse { robot, cameras }).into_response()
}
