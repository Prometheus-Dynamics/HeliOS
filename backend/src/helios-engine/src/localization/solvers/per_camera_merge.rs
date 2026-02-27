use nalgebra::{UnitQuaternion, Vector3};
use std::collections::{HashMap, HashSet};

use lib_cv::modules::localization::MarkerObservation;

use super::{apply_imu_rotation_prior, default_supports_mode, push_detection_pose, source_looks_like_imu, LocalizationSolver, SolverContext, SolverOutcome};
use crate::localization::config::{LocalizationPoseSpace, LocalizationSolverMode};
use crate::localization::estimate::estimate_field_from_robot;
use crate::localization::math::{compose_transforms, invert_transform, transform_to_pose, transform_to_rotation, transform_to_translation, PoseTransform};
use crate::localization::merge::{merge_robot_estimates, merge_rotation_estimates};
use crate::localization::types::{LocalizationSolverOutputs, LocalizationSolverPose, LocalizationSourcePose};

pub struct PerCameraMergeSolver;

impl PerCameraMergeSolver {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PerCameraMergeSolver {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalizationSolver for PerCameraMergeSolver {
    fn supports(&self, config: &crate::localization::config::LocalizationSolverConfig) -> bool {
        default_supports_mode(config, LocalizationSolverMode::PerCameraMerge)
    }

    fn solve(&self, ctx: SolverContext<'_>) -> SolverOutcome {
        let mut errors = Vec::new();
        let output_spaces = ctx.output_spaces;
        let samples = ctx.samples;
        let rig_poses = ctx.rig_poses;
        let marker_map = ctx.marker_map;
        let runtime_tuning = ctx.solver_config.runtime_tuning.sanitized();
        let marker_lookup = marker_map.map(|map| map.markers.iter().map(|marker| (marker.id, marker)).collect::<HashMap<_, _>>());
        let known_marker_ids = marker_lookup.as_ref().map(|lookup| lookup.keys().copied().collect::<HashSet<_>>());

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

        let mut camera_outputs = Vec::new();
        let mut robot_estimates = Vec::new();
        let mut non_imu_rotation_overrides: Vec<(UnitQuaternion<f64>, f32, String)> = Vec::new();
        let mut imu_rotation_overrides: Vec<(UnitQuaternion<f64>, f32, String)> = Vec::new();
        let mut has_visual_robot_rotation = false;
        let mut missing_map = false;
        let mut visual_tag_count = 0usize;

        for sample in samples {
            for detection in &sample.detections {
                let camera_from_tag = detection.camera_from_tag;
                let tag_size = detection.tag_size;
                if needs_tag_in_camera {
                    push_detection_pose(tag_in_camera.as_mut().unwrap(), detection, &camera_from_tag, tag_size);
                }
                if needs_camera_in_tag {
                    let tag_from_camera = invert_transform(&camera_from_tag);
                    push_detection_pose(camera_in_tag.as_mut().unwrap(), detection, &tag_from_camera, tag_size);
                }
                if needs_tag_in_robot || needs_robot_in_tag {
                    if let Some(robot_from_camera) = rig_poses.get(&detection.camera_uid) {
                        let robot_from_tag = compose_transforms(robot_from_camera, &camera_from_tag);
                        if needs_tag_in_robot {
                            push_detection_pose(tag_in_robot.as_mut().unwrap(), detection, &robot_from_tag, tag_size);
                        }
                        if needs_robot_in_tag {
                            let tag_from_robot = invert_transform(&robot_from_tag);
                            push_detection_pose(robot_in_tag.as_mut().unwrap(), detection, &tag_from_robot, tag_size);
                        }
                    }
                }
            }

            if needs_camera_in_field || needs_robot_in_field {
                let Some(map) = marker_map else {
                    missing_map = true;
                    continue;
                };
                if sample.detections.is_empty() {
                    continue;
                }
                let mapped_detections = sample.detections.iter().filter(|det| known_marker_ids.as_ref().is_none_or(|ids| ids.contains(&det.tag_id))).collect::<Vec<_>>();
                if mapped_detections.is_empty() {
                    continue;
                }
                visual_tag_count += mapped_detections.len();

                let observations = mapped_detections
                    .iter()
                    .filter_map(|det| {
                        (det.weight > 0.0).then_some(MarkerObservation {
                            id: det.tag_id,
                            translation: Some(transform_to_translation(&det.camera_from_tag)),
                            rotation: Some(transform_to_rotation(&det.camera_from_tag)),
                            pixel: None,
                            weight: det.weight,
                        })
                    })
                    .collect::<Vec<_>>();
                if observations.is_empty() {
                    continue;
                }
                if observations.len() == 1 && (observations[0].weight as f64) < runtime_tuning.min_single_tag_solve_weight {
                    continue;
                }

                match estimate_field_from_robot(map, &observations, &runtime_tuning) {
                    Ok(field_from_camera) => {
                        if needs_camera_in_field {
                            camera_outputs.push(LocalizationSourcePose {
                                source_id: sample.source.id.clone(),
                                camera_uid: sample.source.camera_uid.clone(),
                                weight: sample.source.weight,
                                pose: transform_to_pose(&field_from_camera),
                            });
                        }

                        if needs_robot_in_field {
                            if let Some(robot_from_camera) = rig_poses.get(&sample.source.camera_uid) {
                                let field_from_robot = compose_transforms(&field_from_camera, &invert_transform(robot_from_camera));
                                robot_estimates.push((field_from_robot, sample.source.weight, sample.source.id.clone()));
                                has_visual_robot_rotation = true;
                            } else {
                                errors.push(format!("missing rig pose for camera {}", sample.source.camera_uid));
                            }
                        }
                    }
                    Err(err) => errors.push(format!("camera solve failed for {}: {err}", sample.source.id)),
                }
            }
        }

        for sample in samples {
            let Some(pose_sample) = sample.pose.as_ref() else {
                continue;
            };
            let pose_space = sample.source.pose_space.unwrap_or(LocalizationPoseSpace::RobotInField);
            match pose_space {
                LocalizationPoseSpace::RobotInField => {
                    if pose_sample.has_translation {
                        robot_estimates.push((pose_sample.pose, sample.source.weight, sample.source.id.clone()));
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
                        let pose = if pose_sample.has_translation {
                            Some(pose_sample.pose)
                        } else if pose_sample.has_rotation {
                            Some(PoseTransform { translation: Vector3::zeros(), rotation: pose_sample.pose.rotation })
                        } else {
                            None
                        };
                        if let Some(pose) = pose {
                            camera_outputs.push(LocalizationSourcePose {
                                source_id: sample.source.id.clone(),
                                camera_uid: sample.source.camera_uid.clone(),
                                weight: sample.source.weight,
                                pose: transform_to_pose(&pose),
                            });
                        }
                    }
                }
                _ => {}
            }
        }

        outputs.tag_in_camera = tag_in_camera;
        outputs.camera_in_tag = camera_in_tag;
        outputs.tag_in_robot = tag_in_robot;
        outputs.robot_in_tag = robot_in_tag;

        if needs_robot_in_field {
            let rotation_overrides = non_imu_rotation_overrides.iter().cloned().chain(imu_rotation_overrides.iter().cloned()).collect::<Vec<_>>();
            let merged_robot = merge_robot_estimates(&robot_estimates);
            let merged_from_translation = merged_robot.is_some();
            // Do not synthesize translation from rotation-only sources.
            // IMU and other rotation-only feeds can refine orientation only once XYZ exists.

            if let Some((mut merged, mut source_ids)) = merged_robot {
                // Keep vision-derived orientation authoritative when available from camera/map solves.
                // Rotation-only sources remain a fallback when visual orientation cannot be estimated.
                if merged_from_translation && !has_visual_robot_rotation {
                    if let Some((rotation, rotation_ids)) = merge_rotation_estimates(&rotation_overrides) {
                        merged.rotation = rotation;
                        let mut merged_ids: HashSet<String> = source_ids.into_iter().collect();
                        merged_ids.extend(rotation_ids);
                        source_ids = merged_ids.into_iter().collect();
                    }
                }

                if merged_from_translation && has_visual_robot_rotation {
                    let imu_prior = merge_rotation_estimates(&imu_rotation_overrides);
                    let (rotation, imu_source_ids) = apply_imu_rotation_prior(merged.rotation, imu_prior, &runtime_tuning, visual_tag_count);
                    if !imu_source_ids.is_empty() {
                        let mut merged_ids: HashSet<String> = source_ids.into_iter().collect();
                        merged_ids.extend(imu_source_ids);
                        source_ids = merged_ids.into_iter().collect();
                    }
                    merged.rotation = rotation;
                }
                outputs.robot_in_field = Some(LocalizationSolverPose { pose: transform_to_pose(&merged), source_ids });

                if needs_camera_in_field {
                    for sample in samples {
                        if source_looks_like_imu(&sample.source) {
                            continue;
                        }
                        let Some(pose_sample) = sample.pose.as_ref() else {
                            continue;
                        };
                        if !pose_sample.has_rotation || pose_sample.has_translation {
                            continue;
                        }
                        let field_from_camera = if let Some(robot_from_camera) = rig_poses.get(&sample.source.camera_uid) { compose_transforms(&merged, robot_from_camera) } else { merged };
                        camera_outputs.push(LocalizationSourcePose {
                            source_id: sample.source.id.clone(),
                            camera_uid: sample.source.camera_uid.clone(),
                            weight: sample.source.weight,
                            pose: transform_to_pose(&field_from_camera),
                        });
                    }
                }
            }
        }

        if needs_camera_in_field && !camera_outputs.is_empty() {
            outputs.camera_in_field = Some(camera_outputs);
        }

        if missing_map && outputs.robot_in_field.is_none() && outputs.camera_in_field.is_none() {
            errors.push("missing field map".to_string());
        }

        SolverOutcome { outputs, errors }
    }
}
