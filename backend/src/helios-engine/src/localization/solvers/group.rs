use nalgebra::UnitQuaternion;
use std::collections::{HashMap, HashSet};

use lib_cv::modules::localization::MarkerObservation;

use super::{
    apply_imu_rotation_prior, camera_field_pose_from_sample, default_supports_mode, push_detection_pose, robot_field_pose_from_camera_sample, source_looks_like_imu, LocalizationSolver, SolverContext,
    SolverOutcome,
};
use crate::localization::config::{LocalizationPoseSpace, LocalizationSolverMode};
use crate::localization::math::{compose_transforms, invert_transform, transform_to_pose, transform_to_rotation, transform_to_translation, PoseTransform};
use crate::localization::merge::{merge_robot_estimates, merge_rotation_estimates};
use crate::localization::types::{LocalizationSolverOutputs, LocalizationSolverPose, LocalizationSourcePose};

pub struct GroupSolveSolver;

impl GroupSolveSolver {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GroupSolveSolver {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalizationSolver for GroupSolveSolver {
    fn supports(&self, config: &crate::localization::config::LocalizationSolverConfig) -> bool {
        default_supports_mode(config, LocalizationSolverMode::GroupSolve)
    }

    fn solve(&self, ctx: SolverContext<'_>) -> SolverOutcome {
        let mut errors = Vec::new();
        let output_spaces = ctx.output_spaces;
        let samples = ctx.samples;
        let rig_poses = ctx.rig_poses;
        let marker_map = ctx.marker_map;
        let runtime_tuning = ctx.solver_config.runtime_tuning.sanitized();

        const TAG_SOLVE_ID: &str = "__tag_solve__";
        let mut outputs = LocalizationSolverOutputs::default();
        let needs_tag_in_camera = output_spaces.contains(&LocalizationPoseSpace::TagInCamera);
        let needs_camera_in_tag = output_spaces.contains(&LocalizationPoseSpace::CameraInTag);
        let needs_tag_in_robot = output_spaces.contains(&LocalizationPoseSpace::TagInRobot);
        let needs_robot_in_tag = output_spaces.contains(&LocalizationPoseSpace::RobotInTag);
        let needs_camera_in_field = output_spaces.contains(&LocalizationPoseSpace::CameraInField);
        let needs_robot_in_field = output_spaces.contains(&LocalizationPoseSpace::RobotInField);

        let mut tag_in_camera = if needs_tag_in_camera { Some(Vec::new()) } else { None };
        let mut camera_in_tag = if needs_camera_in_tag { Some(Vec::new()) } else { None };
        let mut tag_in_robot = if needs_tag_in_robot { Some(Vec::new()) } else { None };
        let mut robot_in_tag = if needs_robot_in_tag { Some(Vec::new()) } else { None };

        let mut best_detections_by_camera_tag: HashMap<(String, u32), &crate::localization::sources::LocalizationDetection> = HashMap::new();
        for sample in samples {
            for detection in &sample.detections {
                let key = (detection.camera_uid.clone(), detection.tag_id);
                match best_detections_by_camera_tag.get(&key) {
                    Some(existing) if existing.weight >= detection.weight => {}
                    _ => {
                        best_detections_by_camera_tag.insert(key, detection);
                    }
                }
            }
        }
        let mut observations = Vec::new();
        for detection in best_detections_by_camera_tag.into_values() {
            let camera_from_tag = detection.camera_from_tag;
            let tag_size = detection.tag_size;
            if needs_tag_in_camera {
                push_detection_pose(tag_in_camera.as_mut().unwrap(), detection, &camera_from_tag, tag_size);
            }
            if needs_camera_in_tag {
                let tag_from_camera = invert_transform(&camera_from_tag);
                push_detection_pose(camera_in_tag.as_mut().unwrap(), detection, &tag_from_camera, tag_size);
            }
            let Some(robot_from_camera) = rig_poses.get(&detection.camera_uid) else {
                if needs_tag_in_robot || needs_robot_in_tag || needs_robot_in_field || needs_camera_in_field {
                    errors.push(format!("missing rig pose for camera {}", detection.camera_uid));
                }
                continue;
            };
            if needs_tag_in_robot || needs_robot_in_tag || needs_robot_in_field || needs_camera_in_field {
                let robot_from_tag = compose_transforms(robot_from_camera, &camera_from_tag);
                if needs_tag_in_robot {
                    push_detection_pose(tag_in_robot.as_mut().unwrap(), detection, &robot_from_tag, tag_size);
                }
                if needs_robot_in_tag {
                    let tag_from_robot = invert_transform(&robot_from_tag);
                    push_detection_pose(robot_in_tag.as_mut().unwrap(), detection, &tag_from_robot, tag_size);
                }
                if needs_robot_in_field || needs_camera_in_field {
                    observations.push(MarkerObservation {
                        id: detection.tag_id,
                        translation: Some(transform_to_translation(&robot_from_tag)),
                        rotation: Some(transform_to_rotation(&robot_from_tag)),
                        pixel: None,
                        weight: detection.weight,
                    });
                }
            }
        }

        outputs.tag_in_camera = tag_in_camera;
        outputs.camera_in_tag = camera_in_tag;
        outputs.tag_in_robot = tag_in_robot;
        outputs.robot_in_tag = robot_in_tag;

        if needs_robot_in_field || needs_camera_in_field {
            let mut tag_pose: Option<PoseTransform> = None;
            let has_pose_translation = samples.iter().any(|sample| sample.pose.as_ref().map(|pose| pose.has_translation).unwrap_or(false));
            let missing_map_without_translation = marker_map.is_none() && !has_pose_translation;
            if let Some(map) = marker_map {
                if !observations.is_empty() {
                    match crate::localization::estimate::estimate_field_from_robot(map, &observations, &runtime_tuning) {
                        Ok(pose) => tag_pose = Some(pose),
                        Err(err) => errors.push(format!("group solve failed: {err}")),
                    }
                }
            }

            let mut translation_estimates: Vec<(PoseTransform, f32, String)> = Vec::new();
            let mut non_imu_rotation_overrides: Vec<(UnitQuaternion<f64>, f32, String)> = Vec::new();
            let mut imu_rotation_overrides: Vec<(UnitQuaternion<f64>, f32, String)> = Vec::new();
            let mut tag_source_ids = Vec::new();
            let has_visual_rotation = tag_pose.is_some();
            let visual_tag_count = observations.len();

            if let Some(field_from_robot) = tag_pose {
                translation_estimates.push((field_from_robot, 1.0, TAG_SOLVE_ID.to_string()));
                tag_source_ids = samples.iter().filter(|sample| !sample.detections.is_empty()).map(|sample| sample.source.id.clone()).collect();
            }

            let mut camera_outputs = Vec::new();
            for sample in samples {
                let Some(pose_sample) = sample.pose.as_ref() else {
                    continue;
                };
                let pose_space = sample.source.pose_space.unwrap_or(LocalizationPoseSpace::RobotInField);
                match pose_space {
                    LocalizationPoseSpace::RobotInField => {
                        if pose_sample.has_translation {
                            translation_estimates.push((pose_sample.pose, sample.source.weight, sample.source.id.clone()));
                        } else if pose_sample.has_rotation {
                            if source_looks_like_imu(&sample.source) {
                                imu_rotation_overrides.push((pose_sample.pose.rotation, sample.source.weight, sample.source.id.clone()));
                            } else {
                                non_imu_rotation_overrides.push((pose_sample.pose.rotation, sample.source.weight, sample.source.id.clone()));
                            }
                        }
                    }
                    LocalizationPoseSpace::CameraInField => {
                        if needs_camera_in_field && !source_looks_like_imu(&sample.source) {
                            if let Some(pose) = camera_field_pose_from_sample(pose_sample) {
                                camera_outputs.push(LocalizationSourcePose {
                                    source_id: sample.source.id.clone(),
                                    camera_uid: sample.source.camera_uid.clone(),
                                    weight: sample.source.weight,
                                    pose: transform_to_pose(&pose),
                                });
                            }
                        }
                        if needs_robot_in_field && !source_looks_like_imu(&sample.source) {
                            match robot_field_pose_from_camera_sample(sample, rig_poses) {
                                Ok(Some(robot_pose)) => {
                                    if robot_pose.has_translation {
                                        translation_estimates.push((robot_pose.pose, sample.source.weight, sample.source.id.clone()));
                                    } else if robot_pose.has_rotation {
                                        non_imu_rotation_overrides.push((robot_pose.pose.rotation, sample.source.weight, sample.source.id.clone()));
                                    }
                                }
                                Ok(None) => {}
                                Err(err) => errors.push(err),
                            }
                        }
                    }
                    _ => {}
                }
            }

            let rotation_overrides = non_imu_rotation_overrides.iter().cloned().chain(imu_rotation_overrides.iter().cloned()).collect::<Vec<_>>();
            if missing_map_without_translation && translation_estimates.is_empty() && rotation_overrides.is_empty() {
                errors.push("missing field map".to_string());
            }

            let merged_robot = merge_robot_estimates(&translation_estimates);
            let merged_from_translation = merged_robot.is_some();
            // Do not synthesize translation from rotation-only sources.
            // IMU and other rotation-only feeds can refine orientation only once XYZ exists.

            if let Some((mut merged, mut source_ids)) = merged_robot {
                if source_ids.iter().any(|id| id == TAG_SOLVE_ID) {
                    source_ids.retain(|id| id != TAG_SOLVE_ID);
                    source_ids.extend(tag_source_ids);
                }

                // Keep vision-derived orientation authoritative when a marker-map solve succeeded.
                // Rotation-only sources (e.g. IMU-only feeds) are still used when visual orientation
                // is unavailable.
                if merged_from_translation && !has_visual_rotation {
                    if let Some((rotation, rotation_ids)) = merge_rotation_estimates(&rotation_overrides) {
                        merged.rotation = rotation;
                        let mut merged_ids: HashSet<String> = source_ids.into_iter().collect();
                        merged_ids.extend(rotation_ids);
                        source_ids = merged_ids.into_iter().collect();
                    }
                }

                if merged_from_translation && has_visual_rotation {
                    let imu_prior = merge_rotation_estimates(&imu_rotation_overrides);
                    let (rotation, imu_source_ids) = apply_imu_rotation_prior(merged.rotation, imu_prior, &runtime_tuning, visual_tag_count);
                    if !imu_source_ids.is_empty() {
                        let mut merged_ids: HashSet<String> = source_ids.into_iter().collect();
                        merged_ids.extend(imu_source_ids);
                        source_ids = merged_ids.into_iter().collect();
                    }
                    merged.rotation = rotation;
                }

                if needs_robot_in_field {
                    outputs.robot_in_field = Some(LocalizationSolverPose { pose: transform_to_pose(&merged), source_ids });
                }

                if needs_camera_in_field {
                    for sample in samples {
                        if source_looks_like_imu(&sample.source) {
                            continue;
                        }
                        let field_from_camera = if let Some(robot_from_camera) = rig_poses.get(&sample.source.camera_uid) {
                            compose_transforms(&merged, robot_from_camera)
                        } else if sample.pose.as_ref().is_some_and(|pose| pose.has_rotation && !pose.has_translation) {
                            merged
                        } else {
                            continue;
                        };
                        camera_outputs.push(LocalizationSourcePose {
                            source_id: sample.source.id.clone(),
                            camera_uid: sample.source.camera_uid.clone(),
                            weight: sample.source.weight,
                            pose: transform_to_pose(&field_from_camera),
                        });
                    }
                }
            }

            if needs_camera_in_field && !camera_outputs.is_empty() {
                outputs.camera_in_field = Some(camera_outputs);
            }
        }

        SolverOutcome { outputs, errors }
    }
}
