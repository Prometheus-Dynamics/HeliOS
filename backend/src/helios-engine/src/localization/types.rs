use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use lib_cv::modules::aruco::ArucoBitGrid;

use super::config::{LocalizationPoseSpace, LocalizationSolverMode};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationPipelineStatus {
    pub configured: bool,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub last_run_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationPipelineSource {
    pub id: String,
    pub stream_id: String,
    pub stream_label: String,
    pub camera_uid: String,
    pub camera_path: String,
    pub pipeline_id: String,
    pub pipeline_label: String,
    pub output_key: String,
    #[serde(default)]
    pub data_type: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PipelineOutputSample {
    #[serde(default)]
    pub data_type: Option<serde_json::Value>,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationSolveResponse {
    pub profile_id: String,
    pub solvers: Vec<LocalizationSolverResult>,
    pub sources: Vec<LocalizationSourceSampleStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationSourceSampleStatus {
    pub source_id: String,
    pub stream_id: String,
    pub output_key: String,
    pub camera_uid: String,
    pub detections: usize,
    pub poll_ms: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag_size: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationSolverResult {
    pub id: String,
    pub name: String,
    pub mode: LocalizationSolverMode,
    pub output_spaces: Vec<LocalizationPoseSpace>,
    pub outputs: LocalizationSolverOutputs,
    #[serde(default)]
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationSolverOutputs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag_in_camera: Option<Vec<LocalizationDetectionPose>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_in_tag: Option<Vec<LocalizationDetectionPose>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag_in_robot: Option<Vec<LocalizationDetectionPose>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub robot_in_tag: Option<Vec<LocalizationDetectionPose>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_in_field: Option<Vec<LocalizationSourcePose>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub robot_in_field: Option<LocalizationSolverPose>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationDetectionPose {
    pub source_id: String,
    pub camera_uid: String,
    pub tag_id: u32,
    pub pose: LocalizationPose,
    #[serde(default = "default_detection_weight")]
    pub weight: f32,
    #[serde(default = "default_detection_quality")]
    pub quality: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag_size: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_rotation: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag_bits: Option<ArucoBitGrid>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationSourcePose {
    pub source_id: String,
    pub camera_uid: String,
    pub weight: f32,
    pub pose: LocalizationPose,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationSolverPose {
    pub pose: LocalizationPose,
    pub source_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationPose {
    pub translation: LocalizationVector,
    pub rotation: LocalizationRotation,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationVector {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationRotation {
    pub roll: f64,
    pub pitch: f64,
    pub yaw: f64,
    pub quaternion: LocalizationQuaternion,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationQuaternion {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

fn default_detection_weight() -> f32 {
    1.0
}

fn default_detection_quality() -> f32 {
    1.0
}
