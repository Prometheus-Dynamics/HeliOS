use std::collections::HashSet;

use super::*;

impl Default for LocalizationConfig {
    fn default() -> Self {
        let profile_id = "default".to_string();
        let profile = LocalizationProfile {
            id: profile_id.clone(),
            name: "Default".to_string(),
            tag_size_m: None,
            allowed_tag_ids: Vec::new(),
            excluded_tag_ids: Vec::new(),
            field_map_id: None,
            field_origin: LocalizationFieldOriginConfig::default(),
            snap_z_to_ground: false,
            snap_roll_to_ground: false,
            snap_pitch_to_ground: false,
            enabled: true,
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
        profile.allowed_tag_ids = normalize_tag_id_list(std::mem::take(&mut profile.allowed_tag_ids));
        profile.excluded_tag_ids = normalize_tag_id_list(std::mem::take(&mut profile.excluded_tag_ids));
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

fn normalize_tag_id_list(values: Vec<u32>) -> Vec<u32> {
    let mut out = values;
    out.sort_unstable();
    out.dedup();
    out
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
        let profile = config.profiles.iter().find(|profile| profile.id == id).ok_or_else(|| "localization profile not found".to_string())?;
        if !profile.enabled {
            return Err("localization profile disabled".to_string());
        }
        return Ok(profile);
    }

    if let Some(active_id) = config.active_profile_id.as_deref() {
        if let Some(profile) = config.profiles.iter().find(|profile| profile.id == active_id && profile.enabled) {
            return Ok(profile);
        }
    }

    config.profiles.iter().find(|profile| profile.enabled).ok_or_else(|| "no enabled localization profiles configured".to_string())
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
