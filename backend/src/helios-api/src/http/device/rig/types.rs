use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{PoseRotation, PoseVector, RigPose};

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
