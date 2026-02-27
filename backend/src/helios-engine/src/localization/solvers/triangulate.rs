use std::collections::{HashMap, HashSet};

use lib_cv::modules::aruco::ArucoBitGrid;

use super::{default_supports_mode, LocalizationSolver, SolverContext, SolverOutcome};
use crate::localization::config::{LocalizationPoseSpace, LocalizationSolverConfig, LocalizationSolverMode};
use crate::localization::math::{compose_transforms, invert_transform, transform_to_pose, PoseTransform};
use crate::localization::merge::merge_robot_estimates;
use crate::localization::types::{LocalizationDetectionPose, LocalizationSolverOutputs};

pub struct TriangulateSolver;

impl TriangulateSolver {
    pub fn new() -> Self {
        Self
    }
}

impl Default for TriangulateSolver {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
struct RepDetection {
    camera_uid: String,
    camera_from_tag: PoseTransform,
    tag_size: Option<f64>,
    code_rotation: Option<u8>,
    tag_bits: Option<ArucoBitGrid>,
    weight: f32,
    quality: f32,
}

struct TagAccum {
    estimates: Vec<(PoseTransform, f32, String)>,
    rep: Option<RepDetection>,
}

impl LocalizationSolver for TriangulateSolver {
    fn supports(&self, config: &LocalizationSolverConfig) -> bool {
        default_supports_mode(config, LocalizationSolverMode::Triangulate)
    }

    fn solve(&self, ctx: SolverContext<'_>) -> SolverOutcome {
        let mut errors = Vec::new();
        let output_spaces = ctx.output_spaces;
        let samples = ctx.samples;
        let rig_poses = ctx.rig_poses;

        let needs_tag_in_camera = output_spaces.contains(&LocalizationPoseSpace::TagInCamera);
        let needs_camera_in_tag = output_spaces.contains(&LocalizationPoseSpace::CameraInTag);
        let needs_tag_in_robot = output_spaces.contains(&LocalizationPoseSpace::TagInRobot);
        let needs_robot_in_tag = output_spaces.contains(&LocalizationPoseSpace::RobotInTag);

        let needs_field_space = output_spaces.contains(&LocalizationPoseSpace::CameraInField) || output_spaces.contains(&LocalizationPoseSpace::RobotInField);
        if needs_field_space {
            errors.push("triangulate solver does not support field-space outputs".to_string());
        }

        let mut outputs = LocalizationSolverOutputs::default();
        let mut tag_in_camera = if needs_tag_in_camera { Some(Vec::new()) } else { None };
        let mut camera_in_tag = if needs_camera_in_tag { Some(Vec::new()) } else { None };
        let mut tag_in_robot = if needs_tag_in_robot { Some(Vec::new()) } else { None };
        let mut robot_in_tag = if needs_robot_in_tag { Some(Vec::new()) } else { None };

        let mut missing_rig_pose: HashSet<String> = HashSet::new();
        let mut per_tag: HashMap<u32, TagAccum> = HashMap::new();

        for sample in samples {
            for detection in &sample.detections {
                let entry = per_tag.entry(detection.tag_id).or_insert_with(|| TagAccum { estimates: Vec::new(), rep: None });

                let rep_candidate = RepDetection {
                    camera_uid: detection.camera_uid.clone(),
                    camera_from_tag: detection.camera_from_tag,
                    tag_size: detection.tag_size,
                    code_rotation: detection.code_rotation,
                    tag_bits: detection.tag_bits.clone(),
                    weight: detection.weight,
                    quality: detection.quality,
                };

                let replace = match &entry.rep {
                    None => true,
                    Some(existing) => rep_candidate.weight > existing.weight,
                };
                if replace {
                    entry.rep = Some(rep_candidate);
                }

                if let Some(robot_from_camera) = rig_poses.get(&detection.camera_uid) {
                    let robot_from_tag = compose_transforms(robot_from_camera, &detection.camera_from_tag);
                    entry.estimates.push((robot_from_tag, detection.weight, detection.source_id.clone()));
                } else if (needs_tag_in_robot || needs_robot_in_tag) && missing_rig_pose.insert(detection.camera_uid.clone()) {
                    errors.push(format!("missing rig pose for camera {}", detection.camera_uid));
                }
            }
        }

        for (tag_id, acc) in per_tag {
            let Some(rep) = acc.rep else {
                continue;
            };
            let merged_robot_from_tag = if needs_tag_in_robot || needs_robot_in_tag { merge_robot_estimates(&acc.estimates).map(|(pose, _ids)| pose) } else { None };

            if needs_tag_in_camera {
                tag_in_camera.as_mut().unwrap().push(LocalizationDetectionPose {
                    source_id: ctx.solver_id.to_string(),
                    camera_uid: rep.camera_uid.clone(),
                    tag_id,
                    pose: transform_to_pose(&rep.camera_from_tag),
                    weight: rep.weight,
                    quality: rep.quality,
                    tag_size: rep.tag_size,
                    code_rotation: rep.code_rotation,
                    tag_bits: rep.tag_bits.clone(),
                });
            }

            if needs_camera_in_tag {
                let tag_from_camera = invert_transform(&rep.camera_from_tag);
                camera_in_tag.as_mut().unwrap().push(LocalizationDetectionPose {
                    source_id: ctx.solver_id.to_string(),
                    camera_uid: rep.camera_uid.clone(),
                    tag_id,
                    pose: transform_to_pose(&tag_from_camera),
                    weight: rep.weight,
                    quality: rep.quality,
                    tag_size: rep.tag_size,
                    code_rotation: rep.code_rotation,
                    tag_bits: rep.tag_bits.clone(),
                });
            }

            if needs_tag_in_robot {
                if let Some(merged_robot_from_tag) = merged_robot_from_tag.as_ref() {
                    tag_in_robot.as_mut().unwrap().push(LocalizationDetectionPose {
                        source_id: ctx.solver_id.to_string(),
                        camera_uid: rep.camera_uid.clone(),
                        tag_id,
                        pose: transform_to_pose(merged_robot_from_tag),
                        weight: rep.weight,
                        quality: rep.quality,
                        tag_size: rep.tag_size,
                        code_rotation: rep.code_rotation,
                        tag_bits: rep.tag_bits.clone(),
                    });
                }
            }

            if needs_robot_in_tag {
                if let Some(merged_robot_from_tag) = merged_robot_from_tag.as_ref() {
                    let tag_from_robot = invert_transform(merged_robot_from_tag);
                    robot_in_tag.as_mut().unwrap().push(LocalizationDetectionPose {
                        source_id: ctx.solver_id.to_string(),
                        camera_uid: rep.camera_uid,
                        tag_id,
                        pose: transform_to_pose(&tag_from_robot),
                        weight: rep.weight,
                        quality: rep.quality,
                        tag_size: rep.tag_size,
                        code_rotation: rep.code_rotation,
                        tag_bits: rep.tag_bits,
                    });
                }
            }
        }

        outputs.tag_in_camera = tag_in_camera;
        outputs.camera_in_tag = camera_in_tag;
        outputs.tag_in_robot = tag_in_robot;
        outputs.robot_in_tag = robot_in_tag;

        SolverOutcome { outputs, errors }
    }
}
