use chrono::Utc;
use lib_schema_migration::{SyncSchemaPlan, normalize_to_current};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::http::{json_store, peers, storage};

use super::{CameraLayoutCameraResponse, PoseRotation, PoseVector, RigPose, RobotDimensions, UpdateRobotDimensionsRequest};

const DEFAULT_ROBOT: RobotDimensions = RobotDimensions { width_m: 0.6, length_m: 0.6, bumper_height_m: 0.127, bumper_thickness_m: 0.0508, ground_clearance_m: 0.0 };
const CURRENT_ROBOT_DIMENSIONS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredRobotDimensionsDocument {
    schema_version: u32,
    robot: RobotDimensions,
}

const ROBOT_DIMENSIONS_SCHEMA_PLAN: SyncSchemaPlan<serde_json::Value> = SyncSchemaPlan::strict("robot dimensions document", CURRENT_ROBOT_DIMENSIONS_SCHEMA_VERSION);

impl Default for RobotDimensions {
    fn default() -> Self {
        DEFAULT_ROBOT
    }
}

pub(super) fn now_rfc3339() -> String {
    Utc::now().to_rfc3339()
}

pub(super) async fn robot_state_path() -> std::io::Result<std::path::PathBuf> {
    let dir = storage::ensure_subdir_async("rig").await?;
    Ok(dir.join("robot.json"))
}

pub(super) async fn load_robot_dimensions() -> RobotDimensions {
    let path = match robot_state_path().await {
        Ok(path) => path,
        Err(_) => return RobotDimensions::default(),
    };
    let bytes = match tokio::fs::read(&path).await {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return RobotDimensions::default(),
        Err(_) => return RobotDimensions::default(),
    };
    match decode_robot_dimensions(&bytes) {
        Ok(robot) => robot,
        Err(err) => {
            warn!(path = %path.display(), %err, "invalid persisted robot dimensions; using defaults");
            RobotDimensions::default()
        }
    }
}

pub(super) async fn update_robot_dimensions_state<F, Fut>(updater: F) -> std::io::Result<RobotDimensions>
where
    F: FnOnce(RobotDimensions) -> Fut,
    Fut: std::future::Future<Output = RobotDimensions>,
{
    let path = robot_state_path().await?;
    json_store::update_bytes(
        path,
        RobotDimensions::default(),
        |bytes| decode_robot_dimensions(bytes).map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
        |robot| serde_json::to_vec(&StoredRobotDimensionsDocument { schema_version: CURRENT_ROBOT_DIMENSIONS_SCHEMA_VERSION, robot: robot.clone() }).map_err(std::io::Error::other),
        updater,
    )
    .await
}

pub(super) fn apply_robot_dimensions_patch(mut robot: RobotDimensions, patch_req: UpdateRobotDimensionsRequest) -> RobotDimensions {
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

    let _ = apply(&mut robot.width_m, patch_req.width_m, false);
    let _ = apply(&mut robot.length_m, patch_req.length_m, false);
    let _ = apply(&mut robot.bumper_height_m, patch_req.bumper_height_m, false);
    let _ = apply(&mut robot.bumper_thickness_m, patch_req.bumper_thickness_m, false);
    let _ = apply(&mut robot.ground_clearance_m, patch_req.ground_clearance_m, true);
    robot
}

pub(super) fn decode_robot_dimensions(bytes: &[u8]) -> Result<RobotDimensions, String> {
    let raw = serde_json::from_slice::<serde_json::Value>(bytes).map_err(|err| format!("failed to decode robot dimensions document: {err}"))?;
    let migrated = normalize_to_current(raw, &ROBOT_DIMENSIONS_SCHEMA_PLAN)?;
    let parsed: StoredRobotDimensionsDocument = serde_json::from_value(migrated).map_err(|err| format!("failed to parse robot dimensions document: {err}"))?;
    Ok(parsed.robot)
}

pub(super) fn backend_label(device: &helios_engine::capture::DiscoveredDevice) -> String {
    device.backends.first().map(|backend| format!("{:?}", backend.kind).to_lowercase()).unwrap_or_else(|| "unknown".to_string())
}

pub(crate) fn camera_uid_from_keys(keys: &[String], fallback: Option<&str>) -> Option<String> {
    helios_engine::capture::canonical_device_id(keys, fallback)
}

pub(super) fn canonical_camera_id(device: &helios_engine::capture::DiscoveredDevice) -> String {
    helios_engine::capture::CaptureDeviceIdentity::from_device(device).camera_id().unwrap_or_else(|| device.identity.display.clone())
}

pub(super) fn camera_hardware_id(device: &helios_engine::capture::DiscoveredDevice) -> Option<String> {
    if device.identity.keys.is_empty() {
        None
    } else {
        let mut keys = device.identity.keys.clone();
        keys.sort();
        Some(keys.join("|"))
    }
}

pub(super) fn stream_matches_device(stream: &helios_engine::ipc::StreamSummary, device: &helios_engine::capture::DiscoveredDevice) -> bool {
    stream.manifest.capture.matches_discovered_device(device)
}

pub(super) fn map_remote_peer_pose(pose: &peers::PeerRemoteRigPose) -> RigPose {
    RigPose {
        translation: PoseVector { x: pose.translation.x, y: pose.translation.y, z: pose.translation.z },
        rotation: PoseRotation { roll: pose.rotation.roll, pitch: pose.rotation.pitch, yaw: pose.rotation.yaw },
        updated_at: pose.updated_at.clone(),
    }
}

pub(super) fn remote_camera_display_name(peer_stream: &peers::PeerRemoteStreamSummary) -> String {
    peer_stream
        .display_name
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .or_else(|| peer_stream.stream_alias.clone())
        .unwrap_or_else(|| peer_stream.remote_stream_id.clone())
}

pub(super) fn peer_camera_layout_entry(peer_stream: &peers::PeerRemoteStreamSummary) -> CameraLayoutCameraResponse {
    let peer_camera_uid = peer_stream.camera_uid.as_deref().map(str::trim).filter(|value| !value.is_empty()).map(|value| value.to_string()).unwrap_or_else(|| peer_stream.stream_ref.clone());

    CameraLayoutCameraResponse {
        stream_id: Some(peer_stream.stream_ref.clone()),
        stream_alias: peer_stream.stream_alias.clone(),
        camera_uid: Some(peer_camera_uid.clone()),
        driver_camera_id: peer_camera_uid,
        display_name: remote_camera_display_name(peer_stream),
        backend: format!("peer/{:?}", peer_stream.peer_kind).to_ascii_lowercase(),
        hardware_id: Some(peer_stream.remote_stream_id.clone()),
        pose: peer_stream.pose.as_ref().map(map_remote_peer_pose),
    }
}
