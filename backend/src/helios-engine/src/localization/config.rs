use serde::{Deserialize, Serialize};
use std::collections::HashSet;
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
    #[serde(default)]
    pub pipeline_template_id: Option<String>,
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

impl Default for LocalizationConfig {
    fn default() -> Self {
        let profile_id = "default".to_string();
        let profile = LocalizationProfile {
            id: profile_id.clone(),
            name: "Default".to_string(),
            tag_size_m: None,
            allowed_tag_ids: Vec::new(),
            field_map_id: None,
            field_origin: LocalizationFieldOriginConfig::default(),
            snap_z_to_ground: false,
            snap_roll_to_ground: false,
            snap_pitch_to_ground: false,
            pipeline_template_id: None,
            color: None,
            view_enabled: true,
            temporal_stabilization: LocalizationTemporalStabilizationConfig::default(),
            sources: Vec::new(),
            solvers: default_solver_configs(),
        };

        Self { active_profile_id: Some(profile_id), profiles: vec![profile] }
    }
}

pub fn normalize_config(mut config: LocalizationConfig) -> LocalizationConfig {
    for profile in &mut config.profiles {
        if profile.solvers.is_empty() {
            profile.solvers = default_solver_configs();
        }
        let has_field_map = profile.field_map_id.as_deref().map(str::trim).is_some_and(|value| !value.is_empty());
        let enabled_source_refs =
            profile.sources.iter().filter(|source| source.enabled).map(|source| (source.id.trim().to_string(), source.camera_uid.trim().to_ascii_lowercase())).collect::<Vec<_>>();
        profile.field_origin = profile.field_origin.sanitized();
        profile.temporal_stabilization = profile.temporal_stabilization.sanitized();
        for solver in &mut profile.solvers {
            normalize_solver_source_ids(solver);
            normalize_solver_output_spaces(solver, solver_uses_multiple_camera_sources(&enabled_source_refs, solver), has_field_map);
            solver.temporal_stabilization = solver.temporal_stabilization.clone().map(|settings| settings.sanitized());
            solver.runtime_tuning = solver.runtime_tuning.sanitized();
        }
    }
    config
}

fn normalize_solver_source_ids(solver: &mut LocalizationSolverConfig) {
    let mut seen = HashSet::<String>::new();
    let mut normalized = Vec::with_capacity(solver.source_ids.len());
    for source_id in &solver.source_ids {
        let source_id = source_id.trim();
        if source_id.is_empty() {
            continue;
        }
        let owned = source_id.to_string();
        if seen.insert(owned.clone()) {
            normalized.push(owned);
        }
    }
    solver.source_ids = normalized;
}

fn normalize_solver_output_spaces(solver: &mut LocalizationSolverConfig, multiple_cameras: bool, has_field_map: bool) {
    let mut seen = HashSet::<LocalizationPoseSpace>::new();
    let mut normalized = Vec::with_capacity(solver.output_spaces.len());
    for space in &solver.output_spaces {
        if seen.insert(*space) {
            normalized.push(*space);
        }
    }
    if has_field_map && !normalized.contains(&LocalizationPoseSpace::RobotInField) {
        normalized.push(LocalizationPoseSpace::RobotInField);
    }
    if has_field_map && !multiple_cameras && !normalized.contains(&LocalizationPoseSpace::CameraInField) {
        normalized.push(LocalizationPoseSpace::CameraInField);
    }
    if multiple_cameras {
        normalized.retain(|space| *space != LocalizationPoseSpace::CameraInField);
    }
    solver.output_spaces = normalized;
}

fn solver_uses_multiple_camera_sources(enabled_sources: &[(String, String)], solver: &LocalizationSolverConfig) -> bool {
    let requested = solver.source_ids.iter().map(|source_id| source_id.as_str()).collect::<HashSet<_>>();
    let scoped_to_solver = !requested.is_empty();
    let mut seen_camera_uids = HashSet::<&str>::new();

    for (source_id, camera_uid) in enabled_sources {
        if scoped_to_solver && !requested.contains(source_id.as_str()) {
            continue;
        }
        let camera_uid = camera_uid.trim();
        if camera_uid.is_empty() || camera_uid == "imu" {
            continue;
        }
        seen_camera_uids.insert(camera_uid);
        if seen_camera_uids.len() > 1 {
            return true;
        }
    }
    false
}

pub fn select_profile<'a>(config: &'a LocalizationConfig, override_id: Option<&str>) -> Result<&'a LocalizationProfile, String> {
    if let Some(id) = override_id {
        return config.profiles.iter().find(|profile| profile.id == id).ok_or_else(|| "localization profile not found".to_string());
    }

    if let Some(active_id) = config.active_profile_id.as_deref() {
        if let Some(profile) = config.profiles.iter().find(|profile| profile.id == active_id) {
            return Ok(profile);
        }
    }

    config.profiles.first().ok_or_else(|| "no localization profiles configured".to_string())
}

fn default_source_weight() -> f32 {
    1.0
}

fn default_view_enabled() -> bool {
    true
}

fn default_solver_configs() -> Vec<LocalizationSolverConfig> {
    vec![LocalizationSolverConfig {
        id: "group".to_string(),
        name: "Robust group solve".to_string(),
        mode: LocalizationSolverMode::RobustGroupSolve,
        output_spaces: vec![LocalizationPoseSpace::TagInCamera, LocalizationPoseSpace::RobotInField],
        source_ids: Vec::new(),
        color: None,
        runtime_tuning: LocalizationSolverRuntimeTuningConfig::default(),
        temporal_stabilization: None,
    }]
}

impl Default for LocalizationSolverRuntimeTuningConfig {
    fn default() -> Self {
        Self {
            min_observation_weight: default_min_observation_weight(),
            min_single_tag_solve_weight: default_min_single_tag_solve_weight(),
            min_multi_tag_total_weight: default_min_multi_tag_total_weight(),
            min_multi_tag_effective_count: default_min_multi_tag_effective_count(),
            weak_single_tag_margin: default_weak_single_tag_margin(),
            coplanar_height_delta_m: default_coplanar_height_delta_m(),
            severe_observed_height_delta_m: default_severe_observed_height_delta_m(),
            moderate_observed_height_delta_m: default_moderate_observed_height_delta_m(),
            mild_observed_height_delta_m: default_mild_observed_height_delta_m(),
            severe_penalty: default_severe_penalty(),
            moderate_penalty: default_moderate_penalty(),
            mild_penalty: default_mild_penalty(),
            dt_scale_min: default_dt_scale_min(),
            dt_scale_max: default_dt_scale_max(),
            switched_single_tag_max_translation_jump_m: default_switched_single_tag_max_translation_jump_m(),
            switched_single_tag_max_rotation_jump_deg: default_switched_single_tag_max_rotation_jump_deg(),
            dropped_multi_to_single_max_translation_jump_m: default_dropped_multi_to_single_max_translation_jump_m(),
            dropped_multi_to_single_max_rotation_jump_deg: default_dropped_multi_to_single_max_rotation_jump_deg(),
            switched_single_tag_reject_window_scale: default_switched_single_tag_reject_window_scale(),
            switched_single_tag_reject_window_min_ms: default_switched_single_tag_reject_window_min_ms(),
            dropped_multi_to_single_reject_window_scale: default_dropped_multi_to_single_reject_window_scale(),
            dropped_multi_to_single_reject_window_min_ms: default_dropped_multi_to_single_reject_window_min_ms(),
            switched_single_tag_gain_damp: default_switched_single_tag_gain_damp(),
            switched_single_tag_min_translation_gain: default_switched_single_tag_min_translation_gain(),
            switched_single_tag_min_rotation_gain: default_switched_single_tag_min_rotation_gain(),
            dropped_multi_to_single_gain_damp: default_dropped_multi_to_single_gain_damp(),
            dropped_multi_to_single_min_translation_gain: default_dropped_multi_to_single_min_translation_gain(),
            dropped_multi_to_single_min_rotation_gain: default_dropped_multi_to_single_min_rotation_gain(),
            rotation_distance_near_m: default_rotation_distance_near_m(),
            rotation_distance_mid_m: default_rotation_distance_mid_m(),
            rotation_distance_far_m: default_rotation_distance_far_m(),
            translation_distance_mid_quality: default_translation_distance_mid_quality(),
            translation_distance_far_quality: default_translation_distance_far_quality(),
            translation_quality_floor: default_translation_quality_floor(),
            rotation_distance_mid_quality: default_rotation_distance_mid_quality(),
            rotation_distance_far_quality: default_rotation_distance_far_quality(),
            rotation_quality_floor: default_rotation_quality_floor(),
            rotation_vertical_ratio_mild: default_rotation_vertical_ratio_mild(),
            rotation_vertical_ratio_severe: default_rotation_vertical_ratio_severe(),
            rotation_vertical_mild_scale: default_rotation_vertical_mild_scale(),
            rotation_vertical_severe_scale: default_rotation_vertical_severe_scale(),
            lateral_ratio_mild: default_lateral_ratio_mild(),
            lateral_ratio_medium: default_lateral_ratio_medium(),
            lateral_ratio_high: default_lateral_ratio_high(),
            lateral_ratio_extreme: default_lateral_ratio_extreme(),
            rotation_lateral_mild_scale: default_rotation_lateral_mild_scale(),
            rotation_lateral_medium_scale: default_rotation_lateral_medium_scale(),
            rotation_lateral_high_scale: default_rotation_lateral_high_scale(),
            rotation_lateral_extreme_scale: default_rotation_lateral_extreme_scale(),
            translation_lateral_mild_scale: default_translation_lateral_mild_scale(),
            translation_lateral_medium_scale: default_translation_lateral_medium_scale(),
            translation_lateral_high_scale: default_translation_lateral_high_scale(),
            translation_lateral_extreme_scale: default_translation_lateral_extreme_scale(),
            bundle_refine_enabled: default_bundle_refine_enabled(),
            bundle_max_iterations: default_bundle_max_iterations(),
            bundle_damping: default_bundle_damping(),
            bundle_translation_huber_m: default_bundle_translation_huber_m(),
            bundle_rotation_huber_deg: default_bundle_rotation_huber_deg(),
            bundle_rotation_weight: default_bundle_rotation_weight(),
            bundle_min_improvement_ratio: default_bundle_min_improvement_ratio(),
            bundle_condition_mid_ratio: default_bundle_condition_mid_ratio(),
            bundle_condition_low_ratio: default_bundle_condition_low_ratio(),
            bundle_condition_mid_rotation_scale: default_bundle_condition_mid_rotation_scale(),
            bundle_condition_low_rotation_scale: default_bundle_condition_low_rotation_scale(),
            imu_rotation_prior_enabled: default_imu_rotation_prior_enabled(),
            imu_rotation_prior_weight: default_imu_rotation_prior_weight(),
            imu_rotation_prior_max_delta_deg: default_imu_rotation_prior_max_delta_deg(),
            imu_rotation_prior_min_tags: default_imu_rotation_prior_min_tags(),
            rotation_consensus_inlier_rotation_scale: default_rotation_consensus_inlier_rotation_scale(),
            rotation_consensus_inlier_translation_scale: default_rotation_consensus_inlier_translation_scale(),
            translation_consensus_inlier_scale: default_translation_consensus_inlier_scale(),
            map_consensus_rotation_inlier_deg: default_map_consensus_rotation_inlier_deg(),
            map_consensus_translation_inlier_m: default_map_consensus_translation_inlier_m(),
            map_consensus_min_inlier_weight_ratio: default_map_consensus_min_inlier_weight_ratio(),
            map_outlier_weight_scale: default_map_outlier_weight_scale(),
        }
    }
}

impl Default for LocalizationTemporalStabilizationConfig {
    fn default() -> Self {
        Self {
            enabled: default_temporal_enabled(),
            single_tag_translation_alpha: default_single_tag_translation_alpha(),
            single_tag_rotation_alpha: default_single_tag_rotation_alpha(),
            multi_tag_translation_alpha: default_multi_tag_translation_alpha(),
            multi_tag_rotation_alpha: default_multi_tag_rotation_alpha(),
            max_translation_jump_m: default_max_translation_jump_m(),
            max_rotation_jump_deg: default_max_rotation_jump_deg(),
            reanchor_reject_window_ms: default_reanchor_reject_window_ms(),
        }
    }
}

impl Default for LocalizationFieldOriginConfig {
    fn default() -> Self {
        Self { mode: LocalizationFieldOriginMode::Blue, custom: None }
    }
}

impl LocalizationFieldOriginConfig {
    pub fn sanitized(&self) -> Self {
        let custom = self.custom.and_then(|origin| {
            if !(origin.x.is_finite() && origin.z.is_finite() && origin.yaw_deg.is_finite()) {
                return None;
            }
            Some(origin)
        });
        let mode = if self.mode == LocalizationFieldOriginMode::Custom && custom.is_none() { LocalizationFieldOriginMode::Blue } else { self.mode };
        Self { mode, custom }
    }
}

impl LocalizationTemporalStabilizationConfig {
    pub fn sanitized(&self) -> Self {
        Self {
            enabled: self.enabled,
            single_tag_translation_alpha: clamp_unit(self.single_tag_translation_alpha),
            single_tag_rotation_alpha: clamp_unit(self.single_tag_rotation_alpha),
            multi_tag_translation_alpha: clamp_unit(self.multi_tag_translation_alpha),
            multi_tag_rotation_alpha: clamp_unit(self.multi_tag_rotation_alpha),
            max_translation_jump_m: clamp_positive(self.max_translation_jump_m, default_max_translation_jump_m()),
            max_rotation_jump_deg: clamp_positive(self.max_rotation_jump_deg, default_max_rotation_jump_deg()),
            reanchor_reject_window_ms: self.reanchor_reject_window_ms.max(50),
        }
    }
}

impl LocalizationSolverRuntimeTuningConfig {
    pub fn sanitized(&self) -> Self {
        let dt_scale_min = clamp_positive(self.dt_scale_min, default_dt_scale_min());
        let dt_scale_max = clamp_positive(self.dt_scale_max, default_dt_scale_max()).max(dt_scale_min);
        let rotation_distance_near_m = clamp_positive(self.rotation_distance_near_m, default_rotation_distance_near_m());
        let rotation_distance_mid_m = clamp_positive(self.rotation_distance_mid_m, default_rotation_distance_mid_m()).max(rotation_distance_near_m + 1e-6);
        let rotation_distance_far_m = clamp_positive(self.rotation_distance_far_m, default_rotation_distance_far_m()).max(rotation_distance_mid_m + 1e-6);
        let translation_distance_mid_quality = clamp_unit(self.translation_distance_mid_quality);
        let translation_distance_far_quality = clamp_unit(self.translation_distance_far_quality).min(translation_distance_mid_quality);
        let translation_quality_floor = clamp_unit(self.translation_quality_floor).min(translation_distance_far_quality.max(0.0));
        let rotation_distance_mid_quality = clamp_unit(self.rotation_distance_mid_quality);
        let rotation_distance_far_quality = clamp_unit(self.rotation_distance_far_quality).min(rotation_distance_mid_quality);
        let rotation_quality_floor = clamp_unit(self.rotation_quality_floor).min(rotation_distance_far_quality.max(0.0));
        let rotation_vertical_ratio_mild = clamp_unit(self.rotation_vertical_ratio_mild);
        let rotation_vertical_ratio_severe = clamp_unit(self.rotation_vertical_ratio_severe).max(rotation_vertical_ratio_mild);
        let lateral_ratio_mild = clamp_non_negative(self.lateral_ratio_mild, default_lateral_ratio_mild());
        let lateral_ratio_medium = clamp_non_negative(self.lateral_ratio_medium, default_lateral_ratio_medium()).max(lateral_ratio_mild);
        let lateral_ratio_high = clamp_non_negative(self.lateral_ratio_high, default_lateral_ratio_high()).max(lateral_ratio_medium);
        let lateral_ratio_extreme = clamp_non_negative(self.lateral_ratio_extreme, default_lateral_ratio_extreme()).max(lateral_ratio_high);
        let rotation_lateral_mild_scale = clamp_unit(self.rotation_lateral_mild_scale);
        let rotation_lateral_medium_scale = clamp_unit(self.rotation_lateral_medium_scale).min(rotation_lateral_mild_scale);
        let rotation_lateral_high_scale = clamp_unit(self.rotation_lateral_high_scale).min(rotation_lateral_medium_scale);
        let rotation_lateral_extreme_scale = clamp_unit(self.rotation_lateral_extreme_scale).min(rotation_lateral_high_scale);
        let translation_lateral_mild_scale = clamp_unit(self.translation_lateral_mild_scale);
        let translation_lateral_medium_scale = clamp_unit(self.translation_lateral_medium_scale).min(translation_lateral_mild_scale);
        let translation_lateral_high_scale = clamp_unit(self.translation_lateral_high_scale).min(translation_lateral_medium_scale);
        let translation_lateral_extreme_scale = clamp_unit(self.translation_lateral_extreme_scale).min(translation_lateral_high_scale);
        let bundle_max_iterations = self.bundle_max_iterations.clamp(1, 64);
        let bundle_damping = clamp_positive(self.bundle_damping, default_bundle_damping());
        let bundle_translation_huber_m = clamp_positive(self.bundle_translation_huber_m, default_bundle_translation_huber_m());
        let bundle_rotation_huber_deg = clamp_positive(self.bundle_rotation_huber_deg, default_bundle_rotation_huber_deg());
        let bundle_rotation_weight = clamp_non_negative(self.bundle_rotation_weight, default_bundle_rotation_weight());
        let bundle_min_improvement_ratio = clamp_non_negative(self.bundle_min_improvement_ratio, default_bundle_min_improvement_ratio()).min(1.0);
        let bundle_condition_mid_ratio = clamp_non_negative(self.bundle_condition_mid_ratio, default_bundle_condition_mid_ratio());
        let bundle_condition_low_ratio = clamp_non_negative(self.bundle_condition_low_ratio, default_bundle_condition_low_ratio()).min(bundle_condition_mid_ratio);
        let bundle_condition_mid_rotation_scale = clamp_unit(self.bundle_condition_mid_rotation_scale);
        let bundle_condition_low_rotation_scale = clamp_unit(self.bundle_condition_low_rotation_scale).min(bundle_condition_mid_rotation_scale);
        let imu_rotation_prior_weight = clamp_unit(self.imu_rotation_prior_weight);
        let imu_rotation_prior_max_delta_deg = clamp_positive(self.imu_rotation_prior_max_delta_deg, default_imu_rotation_prior_max_delta_deg());
        let imu_rotation_prior_min_tags = self.imu_rotation_prior_min_tags.max(1);

        Self {
            min_observation_weight: clamp_non_negative(self.min_observation_weight, default_min_observation_weight()),
            min_single_tag_solve_weight: clamp_positive(self.min_single_tag_solve_weight, default_min_single_tag_solve_weight()),
            min_multi_tag_total_weight: clamp_positive(self.min_multi_tag_total_weight, default_min_multi_tag_total_weight()),
            min_multi_tag_effective_count: clamp_positive(self.min_multi_tag_effective_count, default_min_multi_tag_effective_count()),
            weak_single_tag_margin: clamp_non_negative(self.weak_single_tag_margin, default_weak_single_tag_margin()),
            coplanar_height_delta_m: clamp_non_negative(self.coplanar_height_delta_m, default_coplanar_height_delta_m()),
            severe_observed_height_delta_m: clamp_non_negative(self.severe_observed_height_delta_m, default_severe_observed_height_delta_m()),
            moderate_observed_height_delta_m: clamp_non_negative(self.moderate_observed_height_delta_m, default_moderate_observed_height_delta_m()),
            mild_observed_height_delta_m: clamp_non_negative(self.mild_observed_height_delta_m, default_mild_observed_height_delta_m()),
            severe_penalty: clamp_unit(self.severe_penalty),
            moderate_penalty: clamp_unit(self.moderate_penalty),
            mild_penalty: clamp_unit(self.mild_penalty),
            dt_scale_min,
            dt_scale_max,
            switched_single_tag_max_translation_jump_m: clamp_positive(self.switched_single_tag_max_translation_jump_m, default_switched_single_tag_max_translation_jump_m()),
            switched_single_tag_max_rotation_jump_deg: clamp_positive(self.switched_single_tag_max_rotation_jump_deg, default_switched_single_tag_max_rotation_jump_deg()),
            dropped_multi_to_single_max_translation_jump_m: clamp_positive(self.dropped_multi_to_single_max_translation_jump_m, default_dropped_multi_to_single_max_translation_jump_m()),
            dropped_multi_to_single_max_rotation_jump_deg: clamp_positive(self.dropped_multi_to_single_max_rotation_jump_deg, default_dropped_multi_to_single_max_rotation_jump_deg()),
            switched_single_tag_reject_window_scale: clamp_positive(self.switched_single_tag_reject_window_scale, default_switched_single_tag_reject_window_scale()),
            switched_single_tag_reject_window_min_ms: self.switched_single_tag_reject_window_min_ms.max(default_switched_single_tag_reject_window_min_ms()),
            dropped_multi_to_single_reject_window_scale: clamp_positive(self.dropped_multi_to_single_reject_window_scale, default_dropped_multi_to_single_reject_window_scale()),
            dropped_multi_to_single_reject_window_min_ms: self.dropped_multi_to_single_reject_window_min_ms.max(default_dropped_multi_to_single_reject_window_min_ms()),
            switched_single_tag_gain_damp: clamp_unit(self.switched_single_tag_gain_damp),
            switched_single_tag_min_translation_gain: clamp_unit(self.switched_single_tag_min_translation_gain),
            switched_single_tag_min_rotation_gain: clamp_unit(self.switched_single_tag_min_rotation_gain),
            dropped_multi_to_single_gain_damp: clamp_unit(self.dropped_multi_to_single_gain_damp),
            dropped_multi_to_single_min_translation_gain: clamp_unit(self.dropped_multi_to_single_min_translation_gain),
            dropped_multi_to_single_min_rotation_gain: clamp_unit(self.dropped_multi_to_single_min_rotation_gain),
            rotation_distance_near_m,
            rotation_distance_mid_m,
            rotation_distance_far_m,
            translation_distance_mid_quality,
            translation_distance_far_quality,
            translation_quality_floor,
            rotation_distance_mid_quality,
            rotation_distance_far_quality,
            rotation_quality_floor,
            rotation_vertical_ratio_mild,
            rotation_vertical_ratio_severe,
            rotation_vertical_mild_scale: clamp_unit(self.rotation_vertical_mild_scale),
            rotation_vertical_severe_scale: clamp_unit(self.rotation_vertical_severe_scale),
            lateral_ratio_mild,
            lateral_ratio_medium,
            lateral_ratio_high,
            lateral_ratio_extreme,
            rotation_lateral_mild_scale,
            rotation_lateral_medium_scale,
            rotation_lateral_high_scale,
            rotation_lateral_extreme_scale,
            translation_lateral_mild_scale,
            translation_lateral_medium_scale,
            translation_lateral_high_scale,
            translation_lateral_extreme_scale,
            bundle_refine_enabled: self.bundle_refine_enabled,
            bundle_max_iterations,
            bundle_damping,
            bundle_translation_huber_m,
            bundle_rotation_huber_deg,
            bundle_rotation_weight,
            bundle_min_improvement_ratio,
            bundle_condition_mid_ratio,
            bundle_condition_low_ratio,
            bundle_condition_mid_rotation_scale,
            bundle_condition_low_rotation_scale,
            imu_rotation_prior_enabled: self.imu_rotation_prior_enabled,
            imu_rotation_prior_weight,
            imu_rotation_prior_max_delta_deg,
            imu_rotation_prior_min_tags,
            rotation_consensus_inlier_rotation_scale: clamp_positive(self.rotation_consensus_inlier_rotation_scale, default_rotation_consensus_inlier_rotation_scale()),
            rotation_consensus_inlier_translation_scale: clamp_positive(self.rotation_consensus_inlier_translation_scale, default_rotation_consensus_inlier_translation_scale()),
            translation_consensus_inlier_scale: clamp_positive(self.translation_consensus_inlier_scale, default_translation_consensus_inlier_scale()),
            map_consensus_rotation_inlier_deg: clamp_positive(self.map_consensus_rotation_inlier_deg, default_map_consensus_rotation_inlier_deg()),
            map_consensus_translation_inlier_m: clamp_positive(self.map_consensus_translation_inlier_m, default_map_consensus_translation_inlier_m()),
            map_consensus_min_inlier_weight_ratio: clamp_unit(self.map_consensus_min_inlier_weight_ratio),
            map_outlier_weight_scale: clamp_unit(self.map_outlier_weight_scale),
        }
    }
}

fn clamp_unit(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    value.clamp(0.0, 1.0)
}

fn clamp_positive(value: f64, fallback: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        fallback
    }
}

fn clamp_non_negative(value: f64, fallback: f64) -> f64 {
    if value.is_finite() && value >= 0.0 {
        value
    } else {
        fallback
    }
}

fn default_temporal_enabled() -> bool {
    true
}

fn default_single_tag_translation_alpha() -> f64 {
    0.18
}

fn default_single_tag_rotation_alpha() -> f64 {
    0.16
}

fn default_multi_tag_translation_alpha() -> f64 {
    0.45
}

fn default_multi_tag_rotation_alpha() -> f64 {
    0.38
}

fn default_max_translation_jump_m() -> f64 {
    1.2
}

fn default_max_rotation_jump_deg() -> f64 {
    70.0
}

fn default_reanchor_reject_window_ms() -> u64 {
    450
}

fn default_min_observation_weight() -> f64 {
    0.03
}

fn default_min_single_tag_solve_weight() -> f64 {
    0.34
}

fn default_min_multi_tag_total_weight() -> f64 {
    0.58
}

fn default_min_multi_tag_effective_count() -> f64 {
    1.2
}

fn default_weak_single_tag_margin() -> f64 {
    0.08
}

fn default_coplanar_height_delta_m() -> f64 {
    0.08
}

fn default_severe_observed_height_delta_m() -> f64 {
    0.45
}

fn default_moderate_observed_height_delta_m() -> f64 {
    0.25
}

fn default_mild_observed_height_delta_m() -> f64 {
    0.15
}

fn default_severe_penalty() -> f64 {
    0.1
}

fn default_moderate_penalty() -> f64 {
    0.3
}

fn default_mild_penalty() -> f64 {
    0.6
}

fn default_dt_scale_min() -> f64 {
    0.4
}

fn default_dt_scale_max() -> f64 {
    2.5
}

fn default_switched_single_tag_max_translation_jump_m() -> f64 {
    0.38
}

fn default_switched_single_tag_max_rotation_jump_deg() -> f64 {
    24.0
}

fn default_dropped_multi_to_single_max_translation_jump_m() -> f64 {
    0.58
}

fn default_dropped_multi_to_single_max_rotation_jump_deg() -> f64 {
    36.0
}

fn default_switched_single_tag_reject_window_scale() -> f64 {
    1.8
}

fn default_switched_single_tag_reject_window_min_ms() -> u64 {
    700
}

fn default_dropped_multi_to_single_reject_window_scale() -> f64 {
    1.3
}

fn default_dropped_multi_to_single_reject_window_min_ms() -> u64 {
    520
}

fn default_switched_single_tag_gain_damp() -> f64 {
    0.35
}

fn default_switched_single_tag_min_translation_gain() -> f64 {
    0.04
}

fn default_switched_single_tag_min_rotation_gain() -> f64 {
    0.04
}

fn default_dropped_multi_to_single_gain_damp() -> f64 {
    0.5
}

fn default_dropped_multi_to_single_min_translation_gain() -> f64 {
    0.06
}

fn default_dropped_multi_to_single_min_rotation_gain() -> f64 {
    0.06
}

fn default_rotation_distance_near_m() -> f64 {
    1.0
}

fn default_rotation_distance_mid_m() -> f64 {
    2.2
}

fn default_rotation_distance_far_m() -> f64 {
    3.8
}

fn default_translation_distance_mid_quality() -> f64 {
    0.86
}

fn default_translation_distance_far_quality() -> f64 {
    0.68
}

fn default_translation_quality_floor() -> f64 {
    0.10
}

fn default_rotation_distance_mid_quality() -> f64 {
    0.72
}

fn default_rotation_distance_far_quality() -> f64 {
    0.30
}

fn default_rotation_quality_floor() -> f64 {
    0.08
}

fn default_rotation_vertical_ratio_mild() -> f64 {
    0.45
}

fn default_rotation_vertical_ratio_severe() -> f64 {
    0.60
}

fn default_rotation_vertical_mild_scale() -> f64 {
    0.84
}

fn default_rotation_vertical_severe_scale() -> f64 {
    0.70
}

fn default_lateral_ratio_mild() -> f64 {
    0.45
}

fn default_lateral_ratio_medium() -> f64 {
    0.65
}

fn default_lateral_ratio_high() -> f64 {
    0.85
}

fn default_lateral_ratio_extreme() -> f64 {
    1.10
}

fn default_rotation_lateral_mild_scale() -> f64 {
    0.74
}

fn default_rotation_lateral_medium_scale() -> f64 {
    0.52
}

fn default_rotation_lateral_high_scale() -> f64 {
    0.32
}

fn default_rotation_lateral_extreme_scale() -> f64 {
    0.18
}

fn default_translation_lateral_mild_scale() -> f64 {
    0.85
}

fn default_translation_lateral_medium_scale() -> f64 {
    0.68
}

fn default_translation_lateral_high_scale() -> f64 {
    0.50
}

fn default_translation_lateral_extreme_scale() -> f64 {
    0.32
}

fn default_bundle_refine_enabled() -> bool {
    true
}

fn default_bundle_max_iterations() -> usize {
    8
}

fn default_bundle_damping() -> f64 {
    1e-3
}

fn default_bundle_translation_huber_m() -> f64 {
    0.12
}

fn default_bundle_rotation_huber_deg() -> f64 {
    14.0
}

fn default_bundle_rotation_weight() -> f64 {
    0.20
}

fn default_bundle_min_improvement_ratio() -> f64 {
    0.995
}

fn default_bundle_condition_mid_ratio() -> f64 {
    0.12
}

fn default_bundle_condition_low_ratio() -> f64 {
    0.06
}

fn default_bundle_condition_mid_rotation_scale() -> f64 {
    0.72
}

fn default_bundle_condition_low_rotation_scale() -> f64 {
    0.40
}

fn default_imu_rotation_prior_enabled() -> bool {
    true
}

fn default_imu_rotation_prior_weight() -> f64 {
    0.18
}

fn default_imu_rotation_prior_max_delta_deg() -> f64 {
    45.0
}

fn default_imu_rotation_prior_min_tags() -> usize {
    1
}

fn default_rotation_consensus_inlier_rotation_scale() -> f64 {
    1.00
}

fn default_rotation_consensus_inlier_translation_scale() -> f64 {
    1.10
}

fn default_translation_consensus_inlier_scale() -> f64 {
    1.14
}

fn default_map_consensus_rotation_inlier_deg() -> f64 {
    22.0
}

fn default_map_consensus_translation_inlier_m() -> f64 {
    0.32
}

fn default_map_consensus_min_inlier_weight_ratio() -> f64 {
    0.50
}

fn default_map_outlier_weight_scale() -> f64 {
    0.22
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
            field_map_id: Some("map".to_string()),
            field_origin: LocalizationFieldOriginConfig::default(),
            snap_z_to_ground: false,
            snap_roll_to_ground: false,
            snap_pitch_to_ground: false,
            pipeline_template_id: None,
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
}
