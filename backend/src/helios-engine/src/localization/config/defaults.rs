use super::*;

pub(super) fn default_source_weight() -> f32 {
    1.0
}

pub(super) fn default_profile_enabled() -> bool {
    true
}

pub(super) fn default_view_enabled() -> bool {
    true
}

pub(super) fn default_solver_configs() -> Vec<LocalizationSolverConfig> {
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

pub(super) fn clamp_unit(value: f64) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    value.clamp(0.0, 1.0)
}

pub(super) fn clamp_positive(value: f64, fallback: f64) -> f64 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        fallback
    }
}

pub(super) fn clamp_non_negative(value: f64, fallback: f64) -> f64 {
    if value.is_finite() && value >= 0.0 {
        value
    } else {
        fallback
    }
}

pub(super) fn default_temporal_enabled() -> bool {
    true
}

pub(super) fn default_single_tag_translation_alpha() -> f64 {
    0.18
}

pub(super) fn default_single_tag_rotation_alpha() -> f64 {
    0.16
}

pub(super) fn default_multi_tag_translation_alpha() -> f64 {
    0.45
}

pub(super) fn default_multi_tag_rotation_alpha() -> f64 {
    0.38
}

pub(super) fn default_max_translation_jump_m() -> f64 {
    1.2
}

pub(super) fn default_max_rotation_jump_deg() -> f64 {
    70.0
}

pub(super) fn default_reanchor_reject_window_ms() -> u64 {
    450
}

pub(super) fn default_min_observation_weight() -> f64 {
    0.03
}

pub(super) fn default_min_single_tag_solve_weight() -> f64 {
    0.34
}

pub(super) fn default_min_multi_tag_total_weight() -> f64 {
    0.58
}

pub(super) fn default_min_multi_tag_effective_count() -> f64 {
    1.2
}

pub(super) fn default_weak_single_tag_margin() -> f64 {
    0.08
}

pub(super) fn default_coplanar_height_delta_m() -> f64 {
    0.08
}

pub(super) fn default_severe_observed_height_delta_m() -> f64 {
    0.45
}

pub(super) fn default_moderate_observed_height_delta_m() -> f64 {
    0.25
}

pub(super) fn default_mild_observed_height_delta_m() -> f64 {
    0.15
}

pub(super) fn default_severe_penalty() -> f64 {
    0.1
}

pub(super) fn default_moderate_penalty() -> f64 {
    0.3
}

pub(super) fn default_mild_penalty() -> f64 {
    0.6
}

pub(super) fn default_dt_scale_min() -> f64 {
    0.4
}

pub(super) fn default_dt_scale_max() -> f64 {
    2.5
}

pub(super) fn default_switched_single_tag_max_translation_jump_m() -> f64 {
    0.38
}

pub(super) fn default_switched_single_tag_max_rotation_jump_deg() -> f64 {
    24.0
}

pub(super) fn default_dropped_multi_to_single_max_translation_jump_m() -> f64 {
    0.58
}

pub(super) fn default_dropped_multi_to_single_max_rotation_jump_deg() -> f64 {
    36.0
}

pub(super) fn default_switched_single_tag_reject_window_scale() -> f64 {
    1.8
}

pub(super) fn default_switched_single_tag_reject_window_min_ms() -> u64 {
    700
}

pub(super) fn default_dropped_multi_to_single_reject_window_scale() -> f64 {
    1.3
}

pub(super) fn default_dropped_multi_to_single_reject_window_min_ms() -> u64 {
    520
}

pub(super) fn default_switched_single_tag_gain_damp() -> f64 {
    0.35
}

pub(super) fn default_switched_single_tag_min_translation_gain() -> f64 {
    0.04
}

pub(super) fn default_switched_single_tag_min_rotation_gain() -> f64 {
    0.04
}

pub(super) fn default_dropped_multi_to_single_gain_damp() -> f64 {
    0.5
}

pub(super) fn default_dropped_multi_to_single_min_translation_gain() -> f64 {
    0.06
}

pub(super) fn default_dropped_multi_to_single_min_rotation_gain() -> f64 {
    0.06
}

pub(super) fn default_rotation_distance_near_m() -> f64 {
    1.0
}

pub(super) fn default_rotation_distance_mid_m() -> f64 {
    2.2
}

pub(super) fn default_rotation_distance_far_m() -> f64 {
    3.8
}

pub(super) fn default_translation_distance_mid_quality() -> f64 {
    0.86
}

pub(super) fn default_translation_distance_far_quality() -> f64 {
    0.68
}

pub(super) fn default_translation_quality_floor() -> f64 {
    0.10
}

pub(super) fn default_rotation_distance_mid_quality() -> f64 {
    0.72
}

pub(super) fn default_rotation_distance_far_quality() -> f64 {
    0.30
}

pub(super) fn default_rotation_quality_floor() -> f64 {
    0.08
}

pub(super) fn default_rotation_vertical_ratio_mild() -> f64 {
    0.45
}

pub(super) fn default_rotation_vertical_ratio_severe() -> f64 {
    0.60
}

pub(super) fn default_rotation_vertical_mild_scale() -> f64 {
    0.84
}

pub(super) fn default_rotation_vertical_severe_scale() -> f64 {
    0.70
}

pub(super) fn default_lateral_ratio_mild() -> f64 {
    0.45
}

pub(super) fn default_lateral_ratio_medium() -> f64 {
    0.65
}

pub(super) fn default_lateral_ratio_high() -> f64 {
    0.85
}

pub(super) fn default_lateral_ratio_extreme() -> f64 {
    1.10
}

pub(super) fn default_rotation_lateral_mild_scale() -> f64 {
    0.74
}

pub(super) fn default_rotation_lateral_medium_scale() -> f64 {
    0.52
}

pub(super) fn default_rotation_lateral_high_scale() -> f64 {
    0.32
}

pub(super) fn default_rotation_lateral_extreme_scale() -> f64 {
    0.18
}

pub(super) fn default_translation_lateral_mild_scale() -> f64 {
    0.85
}

pub(super) fn default_translation_lateral_medium_scale() -> f64 {
    0.68
}

pub(super) fn default_translation_lateral_high_scale() -> f64 {
    0.50
}

pub(super) fn default_translation_lateral_extreme_scale() -> f64 {
    0.32
}

pub(super) fn default_bundle_refine_enabled() -> bool {
    true
}

pub(super) fn default_bundle_max_iterations() -> usize {
    8
}

pub(super) fn default_bundle_damping() -> f64 {
    1e-3
}

pub(super) fn default_bundle_translation_huber_m() -> f64 {
    0.12
}

pub(super) fn default_bundle_rotation_huber_deg() -> f64 {
    14.0
}

pub(super) fn default_bundle_rotation_weight() -> f64 {
    0.20
}

pub(super) fn default_bundle_min_improvement_ratio() -> f64 {
    0.995
}

pub(super) fn default_bundle_condition_mid_ratio() -> f64 {
    0.12
}

pub(super) fn default_bundle_condition_low_ratio() -> f64 {
    0.06
}

pub(super) fn default_bundle_condition_mid_rotation_scale() -> f64 {
    0.72
}

pub(super) fn default_bundle_condition_low_rotation_scale() -> f64 {
    0.40
}

pub(super) fn default_imu_rotation_prior_enabled() -> bool {
    true
}

pub(super) fn default_imu_rotation_prior_weight() -> f64 {
    0.18
}

pub(super) fn default_imu_rotation_prior_max_delta_deg() -> f64 {
    45.0
}

pub(super) fn default_imu_rotation_prior_min_tags() -> usize {
    1
}

pub(super) fn default_rotation_consensus_inlier_rotation_scale() -> f64 {
    1.00
}

pub(super) fn default_rotation_consensus_inlier_translation_scale() -> f64 {
    1.10
}

pub(super) fn default_translation_consensus_inlier_scale() -> f64 {
    1.14
}

pub(super) fn default_map_consensus_rotation_inlier_deg() -> f64 {
    22.0
}

pub(super) fn default_map_consensus_translation_inlier_m() -> f64 {
    0.32
}

pub(super) fn default_map_consensus_min_inlier_weight_ratio() -> f64 {
    0.50
}

pub(super) fn default_map_outlier_weight_scale() -> f64 {
    0.22
}
