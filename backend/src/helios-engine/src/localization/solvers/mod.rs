use std::collections::HashMap;

use lib_cv::modules::localization::MarkerMap;
use nalgebra::UnitQuaternion;

use super::config::{LocalizationPoseSpace, LocalizationSolverConfig, LocalizationSolverMode, LocalizationSolverRuntimeTuningConfig, LocalizationSourceConfig};
use super::math::PoseTransform;
use super::sources::{LocalizationDetection, SourceSample};
use super::types::{LocalizationDetectionPose, LocalizationSolverOutputs};

pub mod group;
pub mod per_camera_merge;
pub mod robust_group;
pub mod triangulate;

pub struct SolverContext<'a> {
    pub solver_id: &'a str,
    pub solver_config: &'a LocalizationSolverConfig,
    pub output_spaces: &'a [LocalizationPoseSpace],
    pub samples: &'a [&'a SourceSample],
    pub rig_poses: &'a HashMap<String, PoseTransform>,
    pub marker_map: Option<&'a MarkerMap>,
}

pub struct SolverOutcome {
    pub outputs: LocalizationSolverOutputs,
    pub errors: Vec<String>,
}

pub trait LocalizationSolver: Send + Sync {
    fn supports(&self, config: &LocalizationSolverConfig) -> bool;
    fn solve(&self, ctx: SolverContext<'_>) -> SolverOutcome;
}

pub struct SolverRegistry {
    solvers: Vec<Box<dyn LocalizationSolver>>,
}

impl SolverRegistry {
    pub fn new() -> Self {
        Self { solvers: Vec::new() }
    }

    pub fn register<S: LocalizationSolver + 'static>(&mut self, solver: S) {
        self.solvers.push(Box::new(solver));
    }

    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register(group::GroupSolveSolver::new());
        registry.register(robust_group::RobustGroupSolveSolver::new());
        registry.register(per_camera_merge::PerCameraMergeSolver::new());
        registry.register(triangulate::TriangulateSolver::new());
        registry
    }

    pub fn solver_for(&self, config: &LocalizationSolverConfig) -> Option<&dyn LocalizationSolver> {
        self.solvers.iter().find(|solver| solver.supports(config)).map(|solver| solver.as_ref())
    }
}

impl Default for SolverRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) fn default_supports_mode(config: &LocalizationSolverConfig, mode: LocalizationSolverMode) -> bool {
    config.mode == mode
}

pub(crate) fn push_detection_pose(out: &mut Vec<LocalizationDetectionPose>, detection: &LocalizationDetection, transform: &PoseTransform, tag_size: Option<f64>) {
    out.push(LocalizationDetectionPose {
        source_id: detection.source_id.clone(),
        camera_uid: detection.camera_uid.clone(),
        tag_id: detection.tag_id,
        pose: super::math::transform_to_pose(transform),
        weight: detection.weight,
        quality: detection.quality,
        tag_size,
        code_rotation: detection.code_rotation,
        tag_bits: detection.tag_bits.clone(),
    });
}

fn has_imu_token(value: &str) -> bool {
    value.split(|ch: char| !ch.is_ascii_alphanumeric()).any(|token| token.eq_ignore_ascii_case("imu"))
}

pub(crate) fn source_looks_like_imu(source: &LocalizationSourceConfig) -> bool {
    [source.id.as_str(), source.stream_id.as_str(), source.output_key.as_str(), source.camera_uid.as_str()].iter().any(|value| has_imu_token(value))
}

pub(crate) fn apply_imu_rotation_prior(
    visual_rotation: UnitQuaternion<f64>,
    imu_rotation: Option<(UnitQuaternion<f64>, Vec<String>)>,
    runtime_tuning: &LocalizationSolverRuntimeTuningConfig,
    visual_tag_count: usize,
) -> (UnitQuaternion<f64>, Vec<String>) {
    let Some((imu_rotation, source_ids)) = imu_rotation else {
        return (visual_rotation, Vec::new());
    };
    if !runtime_tuning.imu_rotation_prior_enabled {
        return (visual_rotation, Vec::new());
    }
    if visual_tag_count < runtime_tuning.imu_rotation_prior_min_tags {
        return (visual_rotation, Vec::new());
    }
    let delta_deg = visual_rotation.angle_to(&imu_rotation).to_degrees();
    if !delta_deg.is_finite() || delta_deg > runtime_tuning.imu_rotation_prior_max_delta_deg {
        return (visual_rotation, Vec::new());
    }

    let mut alpha = runtime_tuning.imu_rotation_prior_weight.clamp(0.0, 1.0);
    if visual_tag_count >= 4 {
        alpha *= 0.45;
    } else if visual_tag_count >= 2 {
        alpha *= 0.70;
    }
    if alpha <= 0.0 {
        return (visual_rotation, Vec::new());
    }

    (visual_rotation.slerp(&imu_rotation, alpha), source_ids)
}
