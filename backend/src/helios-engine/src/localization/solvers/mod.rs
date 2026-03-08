use std::collections::HashMap;

use lib_cv::modules::localization::MarkerMap;
use nalgebra::{UnitQuaternion, Vector3};

use super::config::{LocalizationPoseSpace, LocalizationSolverConfig, LocalizationSolverMode, LocalizationSolverRuntimeTuningConfig, LocalizationSourceConfig};
use super::math::{compose_transforms, invert_transform, PoseTransform};
use super::sources::{LocalizationDetection, PoseSample, SourceSample};
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

pub(crate) fn camera_field_pose_from_sample(pose_sample: &PoseSample) -> Option<PoseTransform> {
    if pose_sample.has_translation {
        Some(pose_sample.pose)
    } else if pose_sample.has_rotation {
        Some(PoseTransform { translation: Vector3::zeros(), rotation: pose_sample.pose.rotation })
    } else {
        None
    }
}

pub(crate) fn robot_field_pose_from_camera_sample(sample: &SourceSample, rig_poses: &HashMap<String, PoseTransform>) -> Result<Option<PoseSample>, String> {
    let Some(pose_sample) = sample.pose.as_ref() else {
        return Ok(None);
    };
    if sample.source.pose_space.unwrap_or(LocalizationPoseSpace::RobotInField) != LocalizationPoseSpace::CameraInField || source_looks_like_imu(&sample.source) {
        return Ok(None);
    }

    let Some(field_from_camera) = camera_field_pose_from_sample(pose_sample) else {
        return Ok(None);
    };
    let Some(robot_from_camera) = rig_poses.get(&sample.source.camera_uid) else {
        return Err(format!("missing rig pose for camera {}", sample.source.camera_uid));
    };

    let mut field_from_robot = compose_transforms(&field_from_camera, &invert_transform(robot_from_camera));
    if !pose_sample.has_translation {
        field_from_robot.translation = Vector3::zeros();
    }
    if !pose_sample.has_rotation {
        field_from_robot.rotation = UnitQuaternion::identity();
    }

    Ok(Some(PoseSample { pose: field_from_robot, has_translation: pose_sample.has_translation, has_rotation: pose_sample.has_rotation }))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::localization::config::{LocalizationSolverConfig, LocalizationSolverMode, LocalizationSourceConfig};
    use crate::localization::solvers::group::GroupSolveSolver;
    use crate::localization::solvers::per_camera_merge::PerCameraMergeSolver;
    use crate::localization::solvers::robust_group::RobustGroupSolveSolver;
    use crate::localization::types::LocalizationSolverPose;

    fn solver_config(mode: LocalizationSolverMode) -> LocalizationSolverConfig {
        LocalizationSolverConfig {
            id: format!("solver_{mode:?}"),
            name: format!("solver_{mode:?}"),
            mode,
            output_spaces: vec![LocalizationPoseSpace::RobotInField, LocalizationPoseSpace::CameraInField],
            source_ids: Vec::new(),
            color: None,
            runtime_tuning: LocalizationSolverRuntimeTuningConfig::default(),
            temporal_stabilization: None,
        }
    }

    fn source_config() -> LocalizationSourceConfig {
        LocalizationSourceConfig {
            id: "src_cam0".to_string(),
            stream_id: "stream_cam0".to_string(),
            output_key: "camera_pose".to_string(),
            camera_uid: "cam0".to_string(),
            pose_space: Some(LocalizationPoseSpace::CameraInField),
            input_key: None,
            enabled: true,
            weight: 1.0,
        }
    }

    fn sample() -> SourceSample {
        SourceSample {
            source: source_config(),
            detections: Vec::new(),
            pose: Some(PoseSample { pose: PoseTransform { translation: Vector3::new(5.0, 0.0, 1.0), rotation: UnitQuaternion::identity() }, has_translation: true, has_rotation: true }),
            poll_ms: 0.0,
            tag_size: None,
            error: None,
        }
    }

    fn robot_pose_translation(pose: &LocalizationSolverPose) -> Vector3<f64> {
        Vector3::new(pose.pose.translation.x, pose.pose.translation.y, pose.pose.translation.z)
    }

    fn run_camera_pose_projection_test<S: LocalizationSolver>(solver: &S, mode: LocalizationSolverMode) {
        let sample = sample();
        let samples = vec![&sample];
        let output_spaces = vec![LocalizationPoseSpace::RobotInField, LocalizationPoseSpace::CameraInField];
        let mut rig_poses = HashMap::new();
        rig_poses.insert(sample.source.camera_uid.clone(), PoseTransform { translation: Vector3::new(1.0, 0.0, 0.25), rotation: UnitQuaternion::identity() });

        let outcome =
            solver.solve(SolverContext { solver_id: "solver", solver_config: &solver_config(mode), output_spaces: &output_spaces, samples: &samples, rig_poses: &rig_poses, marker_map: None });

        assert!(outcome.errors.is_empty(), "unexpected solver errors: {:?}", outcome.errors);
        let robot = outcome.outputs.robot_in_field.as_ref().expect("robot pose");
        let camera = outcome.outputs.camera_in_field.as_ref().and_then(|entries| entries.first()).expect("camera pose");

        let robot_translation = robot_pose_translation(robot);
        assert!((robot_translation.x - 4.0).abs() < 1e-9, "expected camera x offset to shift robot x, got {}", robot_translation.x);
        assert!((robot_translation.z - 0.75).abs() < 1e-9, "expected camera z offset to shift robot z, got {}", robot_translation.z);
        assert!((camera.pose.translation.x - 5.0).abs() < 1e-9, "expected camera pose to remain unchanged");
        assert!((camera.pose.translation.z - 1.0).abs() < 1e-9, "expected camera pose to remain unchanged");
    }

    #[test]
    fn camera_in_field_pose_sources_project_through_rig_layout() {
        run_camera_pose_projection_test(&GroupSolveSolver::new(), LocalizationSolverMode::GroupSolve);
        run_camera_pose_projection_test(&RobustGroupSolveSolver::new(), LocalizationSolverMode::RobustGroupSolve);
        run_camera_pose_projection_test(&PerCameraMergeSolver::new(), LocalizationSolverMode::PerCameraMerge);
    }
}
