use crate::{BoolPolicy, BoundedF64Policy, OptionalStringPolicy, StringPolicy};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EngineLocalizationMultitagPolicy {
    pub normal_lock_enabled: BoolPolicy,
    pub normal_lock_strength: BoundedF64Policy,
    pub normal_lock_max_spread_deg: BoundedF64Policy,
    pub normal_lock_max_depth_spread_m: BoundedF64Policy,
    pub full_lock_same_code_rot_enabled: BoolPolicy,
    pub full_lock_strength: BoundedF64Policy,
    pub full_lock_max_spread_deg: BoundedF64Policy,
    pub coplanar_depth_lock_enabled: BoolPolicy,
    pub coplanar_depth_lock_strength: BoundedF64Policy,
    pub coplanar_depth_lock_max_shift_m: BoundedF64Policy,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedEngineLocalizationMultitagPolicy {
    pub normal_lock_enabled: bool,
    pub normal_lock_strength: f64,
    pub normal_lock_max_spread_deg: f64,
    pub normal_lock_max_depth_spread_m: f64,
    pub full_lock_same_code_rot_enabled: bool,
    pub full_lock_strength: f64,
    pub full_lock_max_spread_deg: f64,
    pub coplanar_depth_lock_enabled: bool,
    pub coplanar_depth_lock_strength: f64,
    pub coplanar_depth_lock_max_shift_m: f64,
}

impl EngineLocalizationMultitagPolicy {
    pub fn resolve(self) -> ResolvedEngineLocalizationMultitagPolicy {
        ResolvedEngineLocalizationMultitagPolicy {
            normal_lock_enabled: self.normal_lock_enabled.resolve(),
            normal_lock_strength: self.normal_lock_strength.resolve(),
            normal_lock_max_spread_deg: self.normal_lock_max_spread_deg.resolve(),
            normal_lock_max_depth_spread_m: self.normal_lock_max_depth_spread_m.resolve(),
            full_lock_same_code_rot_enabled: self.full_lock_same_code_rot_enabled.resolve(),
            full_lock_strength: self.full_lock_strength.resolve(),
            full_lock_max_spread_deg: self.full_lock_max_spread_deg.resolve(),
            coplanar_depth_lock_enabled: self.coplanar_depth_lock_enabled.resolve(),
            coplanar_depth_lock_strength: self.coplanar_depth_lock_strength.resolve(),
            coplanar_depth_lock_max_shift_m: self.coplanar_depth_lock_max_shift_m.resolve(),
        }
    }
}

pub const HELIOS_ENGINE_LOCALIZATION_MULTITAG_POLICY: EngineLocalizationMultitagPolicy = EngineLocalizationMultitagPolicy {
    normal_lock_enabled: BoolPolicy { env_var: "HELIOS_LOCALIZATION_MULTITAG_NORMAL_LOCK", default: true },
    normal_lock_strength: BoundedF64Policy { env_var: "HELIOS_LOCALIZATION_MULTITAG_NORMAL_LOCK_STRENGTH", default: 0.78, min: 0.0, max: 1.0 },
    normal_lock_max_spread_deg: BoundedF64Policy { env_var: "HELIOS_LOCALIZATION_MULTITAG_NORMAL_LOCK_MAX_SPREAD_DEG", default: 80.0, min: 8.0, max: 140.0 },
    normal_lock_max_depth_spread_m: BoundedF64Policy { env_var: "HELIOS_LOCALIZATION_MULTITAG_NORMAL_LOCK_MAX_DEPTH_SPREAD_M", default: 0.85, min: 0.05, max: 4.0 },
    full_lock_same_code_rot_enabled: BoolPolicy { env_var: "HELIOS_LOCALIZATION_MULTITAG_FULL_LOCK_SAME_CODE_ROT", default: true },
    full_lock_strength: BoundedF64Policy { env_var: "HELIOS_LOCALIZATION_MULTITAG_FULL_LOCK_STRENGTH", default: 0.62, min: 0.0, max: 1.0 },
    full_lock_max_spread_deg: BoundedF64Policy { env_var: "HELIOS_LOCALIZATION_MULTITAG_FULL_LOCK_MAX_SPREAD_DEG", default: 50.0, min: 6.0, max: 120.0 },
    coplanar_depth_lock_enabled: BoolPolicy { env_var: "HELIOS_LOCALIZATION_MULTITAG_COPLANAR_DEPTH_LOCK", default: true },
    coplanar_depth_lock_strength: BoundedF64Policy { env_var: "HELIOS_LOCALIZATION_MULTITAG_COPLANAR_DEPTH_LOCK_STRENGTH", default: 0.72, min: 0.0, max: 1.0 },
    coplanar_depth_lock_max_shift_m: BoundedF64Policy { env_var: "HELIOS_LOCALIZATION_MULTITAG_COPLANAR_DEPTH_LOCK_MAX_SHIFT_M", default: 0.45, min: 0.01, max: 2.0 },
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EngineLocalizationTemporalPolicy {
    pub pair_distance_translation_lock_enabled: BoolPolicy,
    pub pair_distance_translation_lock_strength: BoundedF64Policy,
    pub pair_distance_translation_lock_max_shift_m: BoundedF64Policy,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedEngineLocalizationTemporalPolicy {
    pub pair_distance_translation_lock_enabled: bool,
    pub pair_distance_translation_lock_strength: f64,
    pub pair_distance_translation_lock_max_shift_m: f64,
}

impl EngineLocalizationTemporalPolicy {
    pub fn resolve(self) -> ResolvedEngineLocalizationTemporalPolicy {
        ResolvedEngineLocalizationTemporalPolicy {
            pair_distance_translation_lock_enabled: self.pair_distance_translation_lock_enabled.resolve(),
            pair_distance_translation_lock_strength: self.pair_distance_translation_lock_strength.resolve(),
            pair_distance_translation_lock_max_shift_m: self.pair_distance_translation_lock_max_shift_m.resolve(),
        }
    }
}

pub const HELIOS_ENGINE_LOCALIZATION_TEMPORAL_POLICY: EngineLocalizationTemporalPolicy = EngineLocalizationTemporalPolicy {
    pair_distance_translation_lock_enabled: BoolPolicy { env_var: "HELIOS_LOCALIZATION_PAIR_DISTANCE_TRANSLATION_LOCK", default: true },
    pair_distance_translation_lock_strength: BoundedF64Policy { env_var: "HELIOS_LOCALIZATION_PAIR_DISTANCE_TRANSLATION_LOCK_STRENGTH", default: 0.62, min: 0.0, max: 1.0 },
    pair_distance_translation_lock_max_shift_m: BoundedF64Policy { env_var: "HELIOS_LOCALIZATION_PAIR_DISTANCE_TRANSLATION_LOCK_MAX_SHIFT_M", default: 0.40, min: 0.02, max: 2.0 },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineLocalizationArucoPolicy {
    pub pose_scale_sweep: BoolPolicy,
    pub dual_model_eval: BoolPolicy,
    pub undistorted_fisheye_model: OptionalStringPolicy,
    pub tag_pose_method: StringPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedEngineLocalizationArucoPolicy {
    pub pose_scale_sweep: bool,
    pub dual_model_eval: bool,
    pub undistorted_fisheye_model: Option<String>,
    pub tag_pose_method: String,
}

impl EngineLocalizationArucoPolicy {
    pub fn resolve(self) -> ResolvedEngineLocalizationArucoPolicy {
        ResolvedEngineLocalizationArucoPolicy {
            pose_scale_sweep: self.pose_scale_sweep.resolve(),
            dual_model_eval: self.dual_model_eval.resolve(),
            undistorted_fisheye_model: self.undistorted_fisheye_model.resolve(),
            tag_pose_method: self.tag_pose_method.resolve(),
        }
    }
}

pub const HELIOS_ENGINE_LOCALIZATION_ARUCO_POLICY: EngineLocalizationArucoPolicy = EngineLocalizationArucoPolicy {
    pose_scale_sweep: BoolPolicy { env_var: "HELIOS_LOCALIZATION_POSE_SCALE_SWEEP", default: false },
    dual_model_eval: BoolPolicy { env_var: "HELIOS_LOCALIZATION_DUAL_MODEL_EVAL", default: false },
    undistorted_fisheye_model: OptionalStringPolicy { env_var: "HELIOS_LOCALIZATION_UNDISTORTED_FISHEYE_MODEL" },
    tag_pose_method: StringPolicy { env_var: "HELIOS_LOCALIZATION_TAG_POSE_METHOD", default: "homography_v2" },
};
