use helios_engine::localization::config::{LocalizationPoseSpace, LocalizationSolverMode};
use lib_runtime_policy::HELIOS_API_LOCALIZATION_POLICY;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationCapabilitiesResponse {
    pub defaults: LocalizationValidationDefaults,
    pub constraints: LocalizationValidationConstraints,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationValidationDefaults {
    pub default_profile_id: String,
    pub default_solver_mode: LocalizationSolverMode,
    pub default_solver_output_spaces: Vec<LocalizationPoseSpace>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationValidationConstraints {
    pub supported_solver_modes: Vec<LocalizationSolverMode>,
    pub supported_pose_spaces: Vec<LocalizationPoseSpace>,
    pub max_map_upload_bytes: u64,
    pub min_poll_hz: u16,
    pub max_poll_hz: u16,
    pub default_poll_hz: u16,
    pub poll_step_hz: u16,
}

pub fn localization_capabilities() -> LocalizationCapabilitiesResponse {
    LocalizationCapabilitiesResponse {
        defaults: LocalizationValidationDefaults {
            default_profile_id: "default".to_string(),
            default_solver_mode: LocalizationSolverMode::RobustGroupSolve,
            default_solver_output_spaces: vec![LocalizationPoseSpace::TagInCamera, LocalizationPoseSpace::RobotInField],
        },
        constraints: LocalizationValidationConstraints {
            supported_solver_modes: vec![LocalizationSolverMode::GroupSolve, LocalizationSolverMode::RobustGroupSolve, LocalizationSolverMode::PerCameraMerge, LocalizationSolverMode::Triangulate],
            supported_pose_spaces: vec![
                LocalizationPoseSpace::TagInCamera,
                LocalizationPoseSpace::CameraInTag,
                LocalizationPoseSpace::TagInRobot,
                LocalizationPoseSpace::RobotInTag,
                LocalizationPoseSpace::CameraInField,
                LocalizationPoseSpace::RobotInField,
            ],
            max_map_upload_bytes: max_map_upload_bytes(),
            min_poll_hz: 5,
            max_poll_hz: 120,
            default_poll_hz: 30,
            poll_step_hz: 5,
        },
    }
}

pub(super) fn max_map_upload_bytes() -> u64 {
    HELIOS_API_LOCALIZATION_POLICY.resolve().max_map_upload_bytes
}
