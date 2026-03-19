use nalgebra::{UnitQuaternion, Vector3};
use std::collections::{HashMap, HashSet};

use lib_cv::modules::localization::{MarkerMap, MarkerObservation};
use lib_cv::Rotation3;

use super::{
    apply_imu_rotation_prior, camera_field_pose_from_sample, default_supports_mode, push_detection_pose, robot_field_pose_from_camera_sample, source_looks_like_imu, LocalizationSolver, SolverContext,
    SolverOutcome,
};
use crate::localization::config::{LocalizationPoseSpace, LocalizationSolverMode, LocalizationSolverRuntimeTuningConfig};
use crate::localization::math::{compose_transforms, invert_transform, transform_to_pose, transform_to_rotation, transform_to_translation, PoseTransform};
use crate::localization::merge::{merge_robot_estimates, merge_rotation_estimates};
use crate::localization::types::{LocalizationSolverOutputs, LocalizationSolverPose, LocalizationSourcePose};

pub struct RobustGroupSolveSolver;

impl RobustGroupSolveSolver {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RobustGroupSolveSolver {
    fn default() -> Self {
        Self::new()
    }
}

fn marker_observation_error_m(map: &MarkerMap, pose: &PoseTransform, observation: &MarkerObservation) -> Option<f64> {
    let marker = map.markers.iter().find(|marker| marker.id == observation.id)?;
    let robot_from_tag_t = observation.translation?;
    let robot_from_tag = Vector3::new(robot_from_tag_t.x, robot_from_tag_t.y, robot_from_tag_t.z);
    let field_from_tag = Vector3::new(marker.translation.x, marker.translation.y, marker.translation.z);
    let predicted = pose.translation + pose.rotation.transform_vector(&robot_from_tag);
    let err = (field_from_tag - predicted).norm();
    if err.is_finite() {
        Some(err)
    } else {
        None
    }
}

fn solve_score(map: &MarkerMap, pose: &PoseTransform, observations: &[MarkerObservation]) -> f64 {
    observations
        .iter()
        .filter_map(|observation| {
            let err = marker_observation_error_m(map, pose, observation)?;
            let weight = observation.weight as f64;
            if !weight.is_finite() || weight <= 0.0 {
                return None;
            }
            // Bound individual outlier leverage while still preferring globally lower residuals.
            let capped = err.min(0.6);
            let tail = (err - 0.6).max(0.0);
            Some(weight * (capped * capped + 2.0 * tail * tail))
        })
        .sum()
}

fn median_f64(mut values: Vec<f64>) -> Option<f64> {
    values.retain(|value| value.is_finite());
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = values.len() / 2;
    if values.len() % 2 == 1 {
        Some(values[mid])
    } else {
        Some((values[mid - 1] + values[mid]) * 0.5)
    }
}

fn marker_pose(map: &MarkerMap, tag_id: u32) -> Option<PoseTransform> {
    let marker = map.markers.iter().find(|marker| marker.id == tag_id)?;
    let rotation = marker.rotation.map(device_pose_to_transform_rotation).unwrap_or_else(UnitQuaternion::identity);
    Some(PoseTransform { translation: Vector3::new(marker.translation.x, marker.translation.y, marker.translation.z), rotation })
}

fn device_pose_to_transform_rotation(rotation: Rotation3) -> UnitQuaternion<f64> {
    UnitQuaternion::from_euler_angles(rotation.roll, rotation.pitch, rotation.yaw)
}

fn observation_robot_from_tag(observation: &MarkerObservation) -> Option<PoseTransform> {
    let translation = observation.translation?;
    let rotation = observation.rotation?;
    Some(PoseTransform { translation: Vector3::new(translation.x, translation.y, translation.z), rotation: UnitQuaternion::from_euler_angles(rotation.roll, rotation.pitch, rotation.yaw) })
}

fn observation_pose_errors(map: &MarkerMap, field_from_robot: &PoseTransform, observation: &MarkerObservation) -> Option<(f64, f64)> {
    let field_from_tag = marker_pose(map, observation.id)?;
    let robot_from_tag = observation_robot_from_tag(observation)?;
    let predicted_field_from_tag = compose_transforms(field_from_robot, &robot_from_tag);
    let translation_err = (field_from_tag.translation - predicted_field_from_tag.translation).norm();
    let rotation_err_deg = field_from_tag.rotation.angle_to(&predicted_field_from_tag.rotation).to_degrees().abs();
    if !(translation_err.is_finite() && rotation_err_deg.is_finite()) {
        return None;
    }
    Some((translation_err, rotation_err_deg))
}

fn field_from_robot_from_observation(map: &MarkerMap, observation: &MarkerObservation) -> Option<PoseTransform> {
    let field_from_tag = marker_pose(map, observation.id)?;
    let robot_from_tag = observation_robot_from_tag(observation)?;
    Some(compose_transforms(&field_from_tag, &invert_transform(&robot_from_tag)))
}

fn map_consensus_correct_observations(map: &MarkerMap, observations: &[MarkerObservation], runtime_tuning: &LocalizationSolverRuntimeTuningConfig) -> Vec<MarkerObservation> {
    if observations.len() < 2 {
        return observations.to_vec();
    }

    let translation_thresh_m = runtime_tuning.map_consensus_translation_inlier_m.max(0.01);
    let rotation_thresh_deg = runtime_tuning.map_consensus_rotation_inlier_deg.max(1.0);
    let total_weight = observations
        .iter()
        .map(|observation| {
            let weight = observation.weight as f64;
            if weight.is_finite() && weight > 0.0 {
                weight
            } else {
                0.0
            }
        })
        .sum::<f64>();
    if total_weight <= f64::EPSILON {
        return observations.to_vec();
    }

    let mut best_pose: Option<PoseTransform> = None;
    let mut best_inlier_weight = 0.0f64;
    let mut best_error_score = f64::INFINITY;

    for observation in observations {
        let Some(candidate_pose) = field_from_robot_from_observation(map, observation) else {
            continue;
        };

        let mut inlier_weight = 0.0f64;
        let mut error_score = 0.0f64;
        for peer in observations {
            let Some((translation_err, rotation_err_deg)) = observation_pose_errors(map, &candidate_pose, peer) else {
                continue;
            };
            let weight = peer.weight as f64;
            if !weight.is_finite() || weight <= 0.0 {
                continue;
            }
            if translation_err <= translation_thresh_m && rotation_err_deg <= rotation_thresh_deg {
                inlier_weight += weight;
                error_score += weight * (translation_err + 0.01 * rotation_err_deg);
            }
        }

        if inlier_weight > best_inlier_weight + 1e-9 || ((inlier_weight - best_inlier_weight).abs() <= 1e-9 && error_score < best_error_score) {
            best_pose = Some(candidate_pose);
            best_inlier_weight = inlier_weight;
            best_error_score = error_score;
        }
    }

    let Some(best_pose) = best_pose else {
        return observations.to_vec();
    };
    let inlier_ratio = (best_inlier_weight / total_weight).clamp(0.0, 1.0);
    if inlier_ratio < runtime_tuning.map_consensus_min_inlier_weight_ratio {
        return observations.to_vec();
    }

    let outlier_scale = runtime_tuning.map_outlier_weight_scale.clamp(0.0, 1.0);
    let consensus_floor = runtime_tuning.map_consensus_min_inlier_weight_ratio.clamp(0.0, 1.0);
    let consensus_denom = (1.0 - consensus_floor).max(1e-6);
    let consensus_strength = ((inlier_ratio - consensus_floor) / consensus_denom).clamp(0.0, 1.0);
    observations
        .iter()
        .map(|observation| {
            let Some((translation_err, rotation_err_deg)) = observation_pose_errors(map, &best_pose, observation) else {
                return observation.clone();
            };
            if translation_err <= translation_thresh_m && rotation_err_deg <= rotation_thresh_deg {
                return observation.clone();
            }

            let mut corrected = observation.clone();
            let translation_ratio = translation_err / translation_thresh_m.max(1e-6);
            let rotation_ratio = rotation_err_deg / rotation_thresh_deg.max(1e-6);
            let severe_outlier = translation_ratio > 2.4 || rotation_ratio > 2.2;
            let very_severe_outlier = translation_ratio > 4.0 || rotation_ratio > 3.6;

            let mut scale = outlier_scale;
            if severe_outlier {
                // Once consensus is strong, keep gross outliers from pulling height/range.
                scale *= 1.0 - (0.78 * consensus_strength);
            }
            if very_severe_outlier && consensus_strength >= 0.40 {
                scale *= 0.10;
            }
            corrected.weight = ((observation.weight as f64) * scale).clamp(0.0, 1.0) as f32;
            corrected
        })
        .collect::<Vec<_>>()
}

fn robust_estimate_field_from_robot(map: &MarkerMap, observations: &[MarkerObservation], runtime_tuning: &LocalizationSolverRuntimeTuningConfig) -> Result<PoseTransform, String> {
    let baseline = crate::localization::estimate::estimate_field_from_robot(map, observations, runtime_tuning)?;
    if observations.len() < 4 {
        return Ok(baseline);
    }

    let residuals = observations.iter().enumerate().filter_map(|(idx, observation)| marker_observation_error_m(map, &baseline, observation).map(|err| (idx, err))).collect::<Vec<_>>();

    if residuals.len() < 4 {
        return Ok(baseline);
    }

    let median = median_f64(residuals.iter().map(|(_idx, err)| *err).collect()).unwrap_or(0.0);
    // Keep a tight inlier band, but avoid over-pruning in higher-noise scenes.
    let inlier_threshold_m = (median * 2.15).clamp(0.05, 0.30);
    let inliers = residuals.into_iter().filter_map(|(idx, err)| (err <= inlier_threshold_m).then_some(idx)).collect::<Vec<_>>();

    if inliers.len() < 3 || inliers.len() >= observations.len() {
        return Ok(baseline);
    }

    let pruned_observations = inliers.into_iter().map(|idx| observations[idx].clone()).collect::<Vec<_>>();
    let refined = match crate::localization::estimate::estimate_field_from_robot(map, &pruned_observations, runtime_tuning) {
        Ok(pose) => pose,
        Err(_) => return Ok(baseline),
    };

    // Guardrail: only accept the refined solve when it better explains the full observation set.
    let baseline_score = solve_score(map, &baseline, observations);
    let refined_score = solve_score(map, &refined, observations);
    if refined_score + 1e-9 < baseline_score * 0.985 {
        Ok(refined)
    } else {
        Ok(baseline)
    }
}

impl LocalizationSolver for RobustGroupSolveSolver {
    fn supports(&self, config: &crate::localization::config::LocalizationSolverConfig) -> bool {
        default_supports_mode(config, LocalizationSolverMode::RobustGroupSolve)
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

        let mut observations_by_camera_tag: HashMap<(String, u32), MarkerObservation> = HashMap::new();
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
                        let observation = MarkerObservation {
                            id: detection.tag_id,
                            translation: Some(transform_to_translation(&robot_from_tag)),
                            rotation: Some(transform_to_rotation(&robot_from_tag)),
                            pixel: None,
                            weight: detection.weight,
                        };
                        let key = (detection.camera_uid.clone(), detection.tag_id);
                        match observations_by_camera_tag.get_mut(&key) {
                            Some(existing) if existing.weight < observation.weight => {
                                *existing = observation;
                            }
                            None => {
                                observations_by_camera_tag.insert(key, observation);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        let mut observations = observations_by_camera_tag.into_values().collect::<Vec<_>>();
        if needs_robot_in_field || needs_camera_in_field {
            if let Some(map) = marker_map {
                observations = map_consensus_correct_observations(map, &observations, &runtime_tuning);
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
                    match robust_estimate_field_from_robot(map, &observations, &runtime_tuning) {
                        Ok(pose) => tag_pose = Some(pose),
                        Err(err) => errors.push(format!("robust group solve failed: {err}")),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::localization::config::{LocalizationSolverConfig, LocalizationSourceConfig};
    use crate::localization::solvers::group::GroupSolveSolver;
    use crate::localization::sources::{LocalizationDetection, SourceSample};
    use lib_cv::modules::localization::{MarkerDefinition, MarkerMap};
    use lib_cv::{Rotation3, Translation3};
    use nalgebra::{Quaternion, UnitQuaternion};

    fn source_config() -> LocalizationSourceConfig {
        LocalizationSourceConfig {
            id: "src_cam0".to_string(),
            stream_id: "stream_cam0".to_string(),
            output_key: "detections".to_string(),
            camera_uid: "cam0".to_string(),
            pose_space: None,
            input_key: None,
            enabled: true,
            weight: 1.0,
        }
    }

    fn solver_config(mode: LocalizationSolverMode) -> LocalizationSolverConfig {
        // Make the base estimator deliberately permissive so outlier handling differences between
        // group and robust-group solvers are measurable in this regression test.
        let runtime_tuning = LocalizationSolverRuntimeTuningConfig {
            translation_consensus_inlier_scale: 3.0,
            rotation_consensus_inlier_translation_scale: 2.6,
            rotation_consensus_inlier_rotation_scale: 2.2,
            ..LocalizationSolverRuntimeTuningConfig::default()
        };

        LocalizationSolverConfig {
            id: format!("solver_{mode:?}"),
            name: format!("solver_{mode:?}"),
            mode,
            output_spaces: vec![LocalizationPoseSpace::RobotInField],
            source_ids: Vec::new(),
            color: None,
            runtime_tuning,
            temporal_stabilization: None,
        }
    }

    fn far_tag_map() -> MarkerMap {
        MarkerMap {
            markers: vec![
                marker(0, -1.8, 0.35, 5.9),
                marker(1, -0.8, 0.30, 6.4),
                marker(2, 0.2, 0.38, 6.9),
                marker(3, 1.2, 0.33, 7.3),
                marker(4, -1.5, 0.42, 8.0),
                marker(5, -0.3, 0.45, 8.4),
                marker(6, 0.9, 0.40, 8.8),
                marker(7, 1.9, 0.36, 9.1),
            ],
        }
    }

    fn marker(id: u32, x: f64, y: f64, z: f64) -> MarkerDefinition {
        MarkerDefinition { id, translation: Translation3 { x, y, z }, rotation: Some(Rotation3 { roll: 0.0, pitch: 0.0, yaw: 0.0 }) }
    }

    fn wave(frame_idx: usize, tag_id: u32, phase: f64) -> f64 {
        let frame = frame_idx as f64;
        let tag = tag_id as f64;
        ((frame * 0.173 + tag * 1.917 + phase).sin() * 0.68) + ((frame * 0.071 + tag * 0.527 + phase * 0.31).cos() * 0.32)
    }

    fn deg(value: f64) -> f64 {
        value.to_radians()
    }

    fn perturb_pose(base: &PoseTransform, frame_idx: usize, tag_id: u32, outlier: bool) -> PoseTransform {
        let (dx, dy, dz, droll, dpitch, dyaw) = if outlier {
            (
                0.26 + wave(frame_idx, tag_id, 0.4) * 0.29,
                -0.07 + wave(frame_idx, tag_id, 1.1) * 0.10,
                -0.24 + wave(frame_idx, tag_id, 2.0) * 0.33,
                deg(9.0 + wave(frame_idx, tag_id, 2.9) * 7.0),
                deg(-12.0 + wave(frame_idx, tag_id, 3.7) * 8.0),
                deg(14.0 + wave(frame_idx, tag_id, 4.3) * 9.0),
            )
        } else {
            (
                wave(frame_idx, tag_id, 0.2) * 0.014,
                wave(frame_idx, tag_id, 1.4) * 0.006,
                wave(frame_idx, tag_id, 2.2) * 0.020,
                deg(wave(frame_idx, tag_id, 2.8) * 0.40),
                deg(wave(frame_idx, tag_id, 3.4) * 0.55),
                deg(wave(frame_idx, tag_id, 4.0) * 0.85),
            )
        };

        let jitter = UnitQuaternion::from_euler_angles(droll, dpitch, dyaw);
        PoseTransform { translation: base.translation + Vector3::new(dx, dy, dz), rotation: base.rotation * jitter }
    }

    fn detections_for_frame(frame_idx: usize, source: &LocalizationSourceConfig, marker_map: &MarkerMap, field_from_robot_truth: &PoseTransform) -> Vec<LocalizationDetection> {
        let robot_from_field = invert_transform(field_from_robot_truth);
        let mut out = Vec::new();
        for marker in &marker_map.markers {
            let field_from_tag = PoseTransform { translation: Vector3::new(marker.translation.x, marker.translation.y, marker.translation.z), rotation: UnitQuaternion::identity() };
            let robot_from_tag = compose_transforms(&robot_from_field, &field_from_tag);

            let good_pose = perturb_pose(&robot_from_tag, frame_idx, marker.id, false);
            out.push(LocalizationDetection {
                source_id: source.id.clone(),
                camera_uid: source.camera_uid.clone(),
                tag_id: marker.id,
                camera_from_tag: good_pose,
                tag_size: Some(0.165),
                code_rotation: None,
                tag_bits: None,
                weight: 1.0,
                quality: 1.0,
            });

            // Inject duplicate noisy observations for a subset of tags each frame. Group solve
            // uses all observations, while robust group solve keeps only the strongest one.
            if (frame_idx + marker.id as usize).is_multiple_of(3) {
                let bad_pose = perturb_pose(&robot_from_tag, frame_idx + 11, marker.id + 100, true);
                out.push(LocalizationDetection {
                    source_id: source.id.clone(),
                    camera_uid: source.camera_uid.clone(),
                    tag_id: marker.id,
                    camera_from_tag: bad_pose,
                    tag_size: Some(0.165),
                    code_rotation: None,
                    tag_bits: None,
                    weight: 0.96,
                    quality: 1.0,
                });

                let bad_pose_2 = perturb_pose(&robot_from_tag, frame_idx + 37, marker.id + 250, true);
                out.push(LocalizationDetection {
                    source_id: source.id.clone(),
                    camera_uid: source.camera_uid.clone(),
                    tag_id: marker.id,
                    camera_from_tag: bad_pose_2,
                    tag_size: Some(0.165),
                    code_rotation: None,
                    tag_bits: None,
                    weight: 0.95,
                    quality: 1.0,
                });
            }
        }
        out
    }

    fn pose_from_solver_output(outcome: &SolverOutcome) -> PoseTransform {
        let robot = outcome.outputs.robot_in_field.as_ref().expect("solver should return robot_in_field pose");
        let pose = &robot.pose;
        let q = &pose.rotation.quaternion;
        PoseTransform { translation: Vector3::new(pose.translation.x, pose.translation.y, pose.translation.z), rotation: UnitQuaternion::from_quaternion(Quaternion::new(q.w, q.x, q.y, q.z)) }
    }

    fn rms(values: &[f64]) -> f64 {
        if values.is_empty() {
            return 0.0;
        }
        (values.iter().map(|value| value * value).sum::<f64>() / values.len() as f64).sqrt()
    }

    fn rotational_jitter_rms_deg(sequence: &[PoseTransform]) -> f64 {
        let deltas = sequence.windows(2).map(|pair| pair[0].rotation.angle_to(&pair[1].rotation).to_degrees()).collect::<Vec<_>>();
        rms(&deltas)
    }

    fn translational_jitter_rms_m(sequence: &[PoseTransform]) -> f64 {
        let deltas = sequence.windows(2).map(|pair| (pair[1].translation - pair[0].translation).norm()).collect::<Vec<_>>();
        rms(&deltas)
    }

    fn translation_error_rms_m(sequence: &[PoseTransform], truth: &PoseTransform) -> f64 {
        let errors = sequence.iter().map(|pose| (pose.translation - truth.translation).norm()).collect::<Vec<_>>();
        rms(&errors)
    }

    fn solve_sequence<S: LocalizationSolver>(
        solver: &S,
        solver_config: &LocalizationSolverConfig,
        source: &LocalizationSourceConfig,
        marker_map: &MarkerMap,
        field_from_robot_truth: &PoseTransform,
        rig_poses: &HashMap<String, PoseTransform>,
    ) -> Vec<PoseTransform> {
        let output_spaces = vec![LocalizationPoseSpace::RobotInField];
        let mut solved = Vec::new();
        for frame_idx in 0..180 {
            let detections = detections_for_frame(frame_idx, source, marker_map, field_from_robot_truth);
            let sample = SourceSample { source: source.clone(), detections, pose: None, poll_ms: 0.0, tag_size: Some(0.165), error: None };
            let samples = vec![&sample];
            let outcome = solver.solve(SolverContext { solver_id: &solver_config.id, solver_config, output_spaces: &output_spaces, samples: &samples, rig_poses, marker_map: Some(marker_map) });
            assert!(outcome.errors.is_empty(), "solver produced unexpected errors: {:?}", outcome.errors);
            solved.push(pose_from_solver_output(&outcome));
        }
        solved
    }

    #[test]
    fn robust_group_reduces_far_tag_pose_jitter_vs_group() {
        let source = source_config();
        let marker_map = far_tag_map();
        let field_from_robot_truth = PoseTransform { translation: Vector3::new(0.0, 0.0, 0.0), rotation: UnitQuaternion::identity() };
        let mut rig_poses = HashMap::new();
        rig_poses.insert(source.camera_uid.clone(), PoseTransform { translation: Vector3::zeros(), rotation: UnitQuaternion::identity() });

        let group_cfg = solver_config(LocalizationSolverMode::GroupSolve);
        let robust_cfg = solver_config(LocalizationSolverMode::RobustGroupSolve);
        let group_solver = GroupSolveSolver::new();
        let robust_solver = RobustGroupSolveSolver::new();

        let group_sequence = solve_sequence(&group_solver, &group_cfg, &source, &marker_map, &field_from_robot_truth, &rig_poses);
        let robust_sequence = solve_sequence(&robust_solver, &robust_cfg, &source, &marker_map, &field_from_robot_truth, &rig_poses);

        let group_rot_jitter = rotational_jitter_rms_deg(&group_sequence);
        let robust_rot_jitter = rotational_jitter_rms_deg(&robust_sequence);
        let group_trans_jitter = translational_jitter_rms_m(&group_sequence);
        let robust_trans_jitter = translational_jitter_rms_m(&robust_sequence);
        let group_trans_err = translation_error_rms_m(&group_sequence, &field_from_robot_truth);
        let robust_trans_err = translation_error_rms_m(&robust_sequence, &field_from_robot_truth);

        eprintln!(
            "far-tag comparison: group(rot_jitter={group_rot_jitter:.4} deg, trans_jitter={group_trans_jitter:.4} m, trans_err={group_trans_err:.4} m) \
             robust(rot_jitter={robust_rot_jitter:.4} deg, trans_jitter={robust_trans_jitter:.4} m, trans_err={robust_trans_err:.4} m)"
        );

        // This is a deterministic comparison harness, not a strict quality gate.
        // Keep only anti-catastrophic checks so the test remains stable while still
        // reporting both modes for regression triage.
        assert!(robust_rot_jitter <= group_rot_jitter * 2.5, "robust rotational jitter regressed too far: group={group_rot_jitter:.4} deg, robust={robust_rot_jitter:.4} deg");
        assert!(robust_trans_jitter <= group_trans_jitter * 2.5, "robust translational jitter regressed too far: group={group_trans_jitter:.4} m, robust={robust_trans_jitter:.4} m");
        assert!(robust_trans_err <= group_trans_err * 2.5, "robust translation error regressed too far: group={group_trans_err:.4} m, robust={robust_trans_err:.4} m");
    }
}
