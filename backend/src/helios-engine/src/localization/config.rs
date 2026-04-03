mod defaults;
mod normalization;

use defaults::*;
pub use normalization::{normalize_config, select_profile};

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationConfig {
    #[serde(default)]
    pub active_profile_id: Option<String>,
    #[serde(default)]
    pub profiles: Vec<LocalizationProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationProfile {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub tag_size_m: Option<f64>,
    #[serde(default)]
    pub allowed_tag_ids: Vec<u32>,
    #[serde(default)]
    pub excluded_tag_ids: Vec<u32>,
    #[serde(default)]
    pub field_map_id: Option<String>,
    #[serde(default)]
    pub field_origin: LocalizationFieldOriginConfig,
    /// Snap the "height" axis to the ground plane when reporting field-space poses.
    ///
    /// Note: in the viewer/three.js frame, +Y is up. This option name uses "Z" to match
    /// common robotics conventions (e.g. WPILib) where Z is up.
    #[serde(default)]
    pub snap_z_to_ground: bool,
    /// Snap field-space roll to level (0 deg).
    #[serde(default)]
    pub snap_roll_to_ground: bool,
    /// Snap field-space pitch to level (0 deg).
    #[serde(default)]
    pub snap_pitch_to_ground: bool,
    #[serde(default = "default_profile_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default = "default_view_enabled")]
    pub view_enabled: bool,
    #[serde(default)]
    pub temporal_stabilization: LocalizationTemporalStabilizationConfig,
    #[serde(default)]
    pub sources: Vec<LocalizationSourceConfig>,
    #[serde(default)]
    pub solvers: Vec<LocalizationSolverConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationFieldOriginConfig {
    #[serde(default)]
    pub mode: LocalizationFieldOriginMode,
    #[serde(default)]
    pub custom: Option<LocalizationCustomFieldOrigin>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LocalizationFieldOriginMode {
    Center,
    #[default]
    Blue,
    Red,
    Custom,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationCustomFieldOrigin {
    pub x: f64,
    pub z: f64,
    pub yaw_deg: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationSourceConfig {
    pub id: String,
    pub stream_id: String,
    pub output_key: String,
    pub camera_uid: String,
    #[serde(default)]
    pub pose_space: Option<LocalizationPoseSpace>,
    #[serde(default)]
    pub input_key: Option<String>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_source_weight")]
    pub weight: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationSolverConfig {
    pub id: String,
    pub name: String,
    pub mode: LocalizationSolverMode,
    #[serde(default)]
    pub output_spaces: Vec<LocalizationPoseSpace>,
    #[serde(default)]
    pub source_ids: Vec<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub runtime_tuning: LocalizationSolverRuntimeTuningConfig,
    #[serde(default)]
    pub temporal_stabilization: Option<LocalizationTemporalStabilizationConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationSolverRuntimeTuningConfig {
    #[serde(default = "default_min_observation_weight")]
    pub min_observation_weight: f64,
    #[serde(default = "default_min_single_tag_solve_weight")]
    pub min_single_tag_solve_weight: f64,
    #[serde(default = "default_min_multi_tag_total_weight")]
    pub min_multi_tag_total_weight: f64,
    #[serde(default = "default_min_multi_tag_effective_count")]
    pub min_multi_tag_effective_count: f64,
    #[serde(default = "default_weak_single_tag_margin")]
    pub weak_single_tag_margin: f64,
    #[serde(default = "default_coplanar_height_delta_m")]
    pub coplanar_height_delta_m: f64,
    #[serde(default = "default_severe_observed_height_delta_m")]
    pub severe_observed_height_delta_m: f64,
    #[serde(default = "default_moderate_observed_height_delta_m")]
    pub moderate_observed_height_delta_m: f64,
    #[serde(default = "default_mild_observed_height_delta_m")]
    pub mild_observed_height_delta_m: f64,
    #[serde(default = "default_severe_penalty")]
    pub severe_penalty: f64,
    #[serde(default = "default_moderate_penalty")]
    pub moderate_penalty: f64,
    #[serde(default = "default_mild_penalty")]
    pub mild_penalty: f64,
    #[serde(default = "default_dt_scale_min")]
    pub dt_scale_min: f64,
    #[serde(default = "default_dt_scale_max")]
    pub dt_scale_max: f64,
    #[serde(default = "default_switched_single_tag_max_translation_jump_m")]
    pub switched_single_tag_max_translation_jump_m: f64,
    #[serde(default = "default_switched_single_tag_max_rotation_jump_deg")]
    pub switched_single_tag_max_rotation_jump_deg: f64,
    #[serde(default = "default_dropped_multi_to_single_max_translation_jump_m")]
    pub dropped_multi_to_single_max_translation_jump_m: f64,
    #[serde(default = "default_dropped_multi_to_single_max_rotation_jump_deg")]
    pub dropped_multi_to_single_max_rotation_jump_deg: f64,
    #[serde(default = "default_switched_single_tag_reject_window_scale")]
    pub switched_single_tag_reject_window_scale: f64,
    #[serde(default = "default_switched_single_tag_reject_window_min_ms")]
    pub switched_single_tag_reject_window_min_ms: u64,
    #[serde(default = "default_dropped_multi_to_single_reject_window_scale")]
    pub dropped_multi_to_single_reject_window_scale: f64,
    #[serde(default = "default_dropped_multi_to_single_reject_window_min_ms")]
    pub dropped_multi_to_single_reject_window_min_ms: u64,
    #[serde(default = "default_switched_single_tag_gain_damp")]
    pub switched_single_tag_gain_damp: f64,
    #[serde(default = "default_switched_single_tag_min_translation_gain")]
    pub switched_single_tag_min_translation_gain: f64,
    #[serde(default = "default_switched_single_tag_min_rotation_gain")]
    pub switched_single_tag_min_rotation_gain: f64,
    #[serde(default = "default_dropped_multi_to_single_gain_damp")]
    pub dropped_multi_to_single_gain_damp: f64,
    #[serde(default = "default_dropped_multi_to_single_min_translation_gain")]
    pub dropped_multi_to_single_min_translation_gain: f64,
    #[serde(default = "default_dropped_multi_to_single_min_rotation_gain")]
    pub dropped_multi_to_single_min_rotation_gain: f64,
    #[serde(default = "default_rotation_distance_near_m")]
    pub rotation_distance_near_m: f64,
    #[serde(default = "default_rotation_distance_mid_m")]
    pub rotation_distance_mid_m: f64,
    #[serde(default = "default_rotation_distance_far_m")]
    pub rotation_distance_far_m: f64,
    #[serde(default = "default_translation_distance_mid_quality")]
    pub translation_distance_mid_quality: f64,
    #[serde(default = "default_translation_distance_far_quality")]
    pub translation_distance_far_quality: f64,
    #[serde(default = "default_translation_quality_floor")]
    pub translation_quality_floor: f64,
    #[serde(default = "default_rotation_distance_mid_quality")]
    pub rotation_distance_mid_quality: f64,
    #[serde(default = "default_rotation_distance_far_quality")]
    pub rotation_distance_far_quality: f64,
    #[serde(default = "default_rotation_quality_floor")]
    pub rotation_quality_floor: f64,
    #[serde(default = "default_rotation_vertical_ratio_mild")]
    pub rotation_vertical_ratio_mild: f64,
    #[serde(default = "default_rotation_vertical_ratio_severe")]
    pub rotation_vertical_ratio_severe: f64,
    #[serde(default = "default_rotation_vertical_mild_scale")]
    pub rotation_vertical_mild_scale: f64,
    #[serde(default = "default_rotation_vertical_severe_scale")]
    pub rotation_vertical_severe_scale: f64,
    #[serde(default = "default_lateral_ratio_mild")]
    pub lateral_ratio_mild: f64,
    #[serde(default = "default_lateral_ratio_medium")]
    pub lateral_ratio_medium: f64,
    #[serde(default = "default_lateral_ratio_high")]
    pub lateral_ratio_high: f64,
    #[serde(default = "default_lateral_ratio_extreme")]
    pub lateral_ratio_extreme: f64,
    #[serde(default = "default_rotation_lateral_mild_scale")]
    pub rotation_lateral_mild_scale: f64,
    #[serde(default = "default_rotation_lateral_medium_scale")]
    pub rotation_lateral_medium_scale: f64,
    #[serde(default = "default_rotation_lateral_high_scale")]
    pub rotation_lateral_high_scale: f64,
    #[serde(default = "default_rotation_lateral_extreme_scale")]
    pub rotation_lateral_extreme_scale: f64,
    #[serde(default = "default_translation_lateral_mild_scale")]
    pub translation_lateral_mild_scale: f64,
    #[serde(default = "default_translation_lateral_medium_scale")]
    pub translation_lateral_medium_scale: f64,
    #[serde(default = "default_translation_lateral_high_scale")]
    pub translation_lateral_high_scale: f64,
    #[serde(default = "default_translation_lateral_extreme_scale")]
    pub translation_lateral_extreme_scale: f64,
    #[serde(default = "default_bundle_refine_enabled")]
    pub bundle_refine_enabled: bool,
    #[serde(default = "default_bundle_max_iterations")]
    pub bundle_max_iterations: usize,
    #[serde(default = "default_bundle_damping")]
    pub bundle_damping: f64,
    #[serde(default = "default_bundle_translation_huber_m")]
    pub bundle_translation_huber_m: f64,
    #[serde(default = "default_bundle_rotation_huber_deg")]
    pub bundle_rotation_huber_deg: f64,
    #[serde(default = "default_bundle_rotation_weight")]
    pub bundle_rotation_weight: f64,
    #[serde(default = "default_bundle_min_improvement_ratio")]
    pub bundle_min_improvement_ratio: f64,
    #[serde(default = "default_bundle_condition_mid_ratio")]
    pub bundle_condition_mid_ratio: f64,
    #[serde(default = "default_bundle_condition_low_ratio")]
    pub bundle_condition_low_ratio: f64,
    #[serde(default = "default_bundle_condition_mid_rotation_scale")]
    pub bundle_condition_mid_rotation_scale: f64,
    #[serde(default = "default_bundle_condition_low_rotation_scale")]
    pub bundle_condition_low_rotation_scale: f64,
    #[serde(default = "default_imu_rotation_prior_enabled")]
    pub imu_rotation_prior_enabled: bool,
    #[serde(default = "default_imu_rotation_prior_weight")]
    pub imu_rotation_prior_weight: f64,
    #[serde(default = "default_imu_rotation_prior_max_delta_deg")]
    pub imu_rotation_prior_max_delta_deg: f64,
    #[serde(default = "default_imu_rotation_prior_min_tags")]
    pub imu_rotation_prior_min_tags: usize,
    #[serde(default = "default_rotation_consensus_inlier_rotation_scale")]
    pub rotation_consensus_inlier_rotation_scale: f64,
    #[serde(default = "default_rotation_consensus_inlier_translation_scale")]
    pub rotation_consensus_inlier_translation_scale: f64,
    #[serde(default = "default_translation_consensus_inlier_scale")]
    pub translation_consensus_inlier_scale: f64,
    #[serde(default = "default_map_consensus_rotation_inlier_deg")]
    pub map_consensus_rotation_inlier_deg: f64,
    #[serde(default = "default_map_consensus_translation_inlier_m")]
    pub map_consensus_translation_inlier_m: f64,
    #[serde(default = "default_map_consensus_min_inlier_weight_ratio")]
    pub map_consensus_min_inlier_weight_ratio: f64,
    #[serde(default = "default_map_outlier_weight_scale")]
    pub map_outlier_weight_scale: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LocalizationTemporalStabilizationConfig {
    #[serde(default = "default_temporal_enabled")]
    pub enabled: bool,
    #[serde(default = "default_single_tag_translation_alpha")]
    pub single_tag_translation_alpha: f64,
    #[serde(default = "default_single_tag_rotation_alpha")]
    pub single_tag_rotation_alpha: f64,
    #[serde(default = "default_multi_tag_translation_alpha")]
    pub multi_tag_translation_alpha: f64,
    #[serde(default = "default_multi_tag_rotation_alpha")]
    pub multi_tag_rotation_alpha: f64,
    #[serde(default = "default_max_translation_jump_m")]
    pub max_translation_jump_m: f64,
    #[serde(default = "default_max_rotation_jump_deg")]
    pub max_rotation_jump_deg: f64,
    #[serde(default = "default_reanchor_reject_window_ms")]
    pub reanchor_reject_window_ms: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocalizationSolverMode {
    GroupSolve,
    RobustGroupSolve,
    PerCameraMerge,
    Triangulate,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LocalizationPoseSpace {
    TagInCamera,
    CameraInTag,
    TagInRobot,
    RobotInTag,
    CameraInField,
    RobotInField,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(id: &str, camera_uid: &str) -> LocalizationSourceConfig {
        LocalizationSourceConfig {
            id: id.to_string(),
            stream_id: format!("stream:{id}"),
            output_key: "detections".to_string(),
            camera_uid: camera_uid.to_string(),
            pose_space: None,
            input_key: None,
            enabled: true,
            weight: 1.0,
        }
    }

    fn solver(source_ids: Vec<&str>, output_spaces: Vec<LocalizationPoseSpace>) -> LocalizationSolverConfig {
        LocalizationSolverConfig {
            id: "solver".to_string(),
            name: "solver".to_string(),
            mode: LocalizationSolverMode::GroupSolve,
            output_spaces,
            source_ids: source_ids.into_iter().map(|value| value.to_string()).collect(),
            color: None,
            runtime_tuning: LocalizationSolverRuntimeTuningConfig::default(),
            temporal_stabilization: None,
        }
    }

    fn base_profile() -> LocalizationProfile {
        LocalizationProfile {
            id: "p".to_string(),
            name: "p".to_string(),
            tag_size_m: None,
            allowed_tag_ids: vec![],
            excluded_tag_ids: vec![],
            field_map_id: Some("map".to_string()),
            field_origin: LocalizationFieldOriginConfig::default(),
            snap_z_to_ground: false,
            snap_roll_to_ground: false,
            snap_pitch_to_ground: false,
            enabled: true,
            color: None,
            view_enabled: true,
            temporal_stabilization: LocalizationTemporalStabilizationConfig::default(),
            sources: vec![],
            solvers: vec![],
        }
    }

    #[test]
    fn normalize_config_removes_camera_in_field_for_multi_camera_solver() {
        let mut profile = base_profile();
        profile.sources = vec![source("s1", "cam_a"), source("s2", "cam_b")];
        profile.solvers = vec![solver(vec![], vec![LocalizationPoseSpace::CameraInField, LocalizationPoseSpace::RobotInField])];

        let normalized = normalize_config(LocalizationConfig { active_profile_id: Some(profile.id.clone()), profiles: vec![profile] });
        let spaces = &normalized.profiles[0].solvers[0].output_spaces;
        assert!(spaces.contains(&LocalizationPoseSpace::RobotInField));
        assert!(!spaces.contains(&LocalizationPoseSpace::CameraInField));
    }

    #[test]
    fn normalize_config_keeps_camera_in_field_for_single_scoped_camera_solver() {
        let mut profile = base_profile();
        profile.sources = vec![source("s1", "cam_a"), source("s2", "cam_b")];
        profile.solvers = vec![solver(vec!["s1"], vec![LocalizationPoseSpace::CameraInField])];

        let normalized = normalize_config(LocalizationConfig { active_profile_id: Some(profile.id.clone()), profiles: vec![profile] });
        let spaces = &normalized.profiles[0].solvers[0].output_spaces;
        assert!(spaces.contains(&LocalizationPoseSpace::CameraInField));
        assert!(spaces.contains(&LocalizationPoseSpace::RobotInField));
    }

    #[test]
    fn normalize_config_adds_field_spaces_for_single_camera_when_map_present() {
        let mut profile = base_profile();
        profile.sources = vec![source("s1", "cam_a")];
        profile.solvers = vec![solver(vec!["s1"], vec![LocalizationPoseSpace::TagInCamera])];

        let normalized = normalize_config(LocalizationConfig { active_profile_id: Some(profile.id.clone()), profiles: vec![profile] });
        let spaces = &normalized.profiles[0].solvers[0].output_spaces;
        assert!(spaces.contains(&LocalizationPoseSpace::RobotInField));
        assert!(spaces.contains(&LocalizationPoseSpace::CameraInField));
    }

    #[test]
    fn normalize_config_sorts_and_deduplicates_tag_filters() {
        let mut profile = base_profile();
        profile.allowed_tag_ids = vec![9, 2, 9, 3];
        profile.excluded_tag_ids = vec![4, 1, 4, 2];

        let normalized = normalize_config(LocalizationConfig { active_profile_id: Some(profile.id.clone()), profiles: vec![profile] });
        let normalized_profile = &normalized.profiles[0];
        assert_eq!(normalized_profile.allowed_tag_ids, vec![2, 3, 9]);
        assert_eq!(normalized_profile.excluded_tag_ids, vec![1, 2, 4]);
    }

    #[test]
    fn select_profile_skips_disabled_active_profile() {
        let mut disabled = base_profile();
        disabled.id = "disabled".to_string();
        disabled.enabled = false;
        let mut enabled = base_profile();
        enabled.id = "enabled".to_string();
        let config = LocalizationConfig { active_profile_id: Some("disabled".to_string()), profiles: vec![disabled, enabled] };
        let selected = select_profile(&config, None).expect("enabled profile");
        assert_eq!(selected.id, "enabled");
    }

    #[test]
    fn select_profile_rejects_disabled_override() {
        let mut profile = base_profile();
        profile.enabled = false;
        let err = select_profile(&LocalizationConfig { active_profile_id: Some(profile.id.clone()), profiles: vec![profile] }, Some("p")).expect_err("disabled override should fail");
        assert_eq!(err, "localization profile disabled");
    }
}
