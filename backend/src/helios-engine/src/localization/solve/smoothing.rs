use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use nalgebra::{Quaternion, UnitQuaternion};

use crate::localization::config::{LocalizationProfile, LocalizationSolverRuntimeTuningConfig, LocalizationTemporalStabilizationConfig};
use crate::localization::types::{LocalizationPose, LocalizationSolverOutputs, LocalizationSolverResult};

use super::{TemporalPoseState, solver_temporal_state};

pub(super) fn apply_temporal_pose_stabilization(profile: &LocalizationProfile, solver_results: &mut [LocalizationSolverResult]) {
    let now = Instant::now();
    let stale_after = Duration::from_secs(5);
    let profile_prefix = format!("{}:", profile.id);

    let mut solver_settings = HashMap::new();
    let mut solver_runtime_tuning = HashMap::new();
    for solver in &profile.solvers {
        let settings = solver.temporal_stabilization.clone().unwrap_or_else(|| profile.temporal_stabilization.clone()).sanitized();
        let runtime_tuning = solver.runtime_tuning.sanitized();
        solver_settings.insert(solver.id.as_str(), settings);
        solver_runtime_tuning.insert(solver.id.as_str(), runtime_tuning);
    }

    let state_store = solver_temporal_state();
    let Ok(mut state_store) = state_store.lock() else {
        return;
    };

    for solver in solver_results.iter_mut() {
        let settings = solver_settings.get(solver.id.as_str()).cloned().unwrap_or_else(|| profile.temporal_stabilization.sanitized());
        let runtime_tuning = solver_runtime_tuning.get(solver.id.as_str()).cloned().unwrap_or_else(LocalizationSolverRuntimeTuningConfig::default);
        let tag_ids = solver_tag_ids(&solver.outputs);
        let tag_count = tag_ids.len();
        let solve_confidence = solver_detection_confidence(&solver.outputs);
        let solver_prefix = format!("{}{}", profile_prefix, solver.id);
        let smoothing_input = LocalizationPoseSmoothingInput { settings: &settings, runtime_tuning: &runtime_tuning, tag_count, tag_ids: &tag_ids, solve_confidence, now };

        if let Some(robot_pose) = solver.outputs.robot_in_field.as_mut() {
            let state_key = format!("{solver_prefix}:robot");
            smooth_localization_pose(&mut state_store, &state_key, &mut robot_pose.pose, smoothing_input);
            continue;
        }

        if let Some(camera_poses) = solver.outputs.camera_in_field.as_mut() {
            for entry in camera_poses {
                let state_key = format!("{solver_prefix}:camera:{}", entry.camera_uid);
                smooth_localization_pose(&mut state_store, &state_key, &mut entry.pose, smoothing_input);
            }
        }
    }

    state_store.retain(|key, state| {
        if !key.starts_with(&profile_prefix) {
            return true;
        }
        now.saturating_duration_since(state.updated_at) <= stale_after
    });
}

fn solver_tag_ids(outputs: &LocalizationSolverOutputs) -> Vec<u32> {
    let mut ids = outputs.tag_in_robot.as_ref().or(outputs.tag_in_camera.as_ref()).map(|list| list.iter().map(|entry| entry.tag_id).collect::<Vec<_>>()).unwrap_or_default();
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn solver_detection_confidence(outputs: &LocalizationSolverOutputs) -> f64 {
    let detections = outputs.tag_in_robot.as_ref().or(outputs.tag_in_camera.as_ref());
    let Some(detections) = detections else {
        return 0.0;
    };
    if detections.is_empty() {
        return 0.0;
    }

    let mut weighted_sum = 0.0f64;
    let mut count = 0usize;
    let mut unique_tag_ids = HashSet::<u32>::new();
    for detection in detections {
        let weight = (detection.weight as f64).clamp(0.0, 1.0);
        let quality = (detection.quality as f64).clamp(0.0, 1.0);
        weighted_sum += (weight * quality.sqrt()).clamp(0.0, 1.0);
        count += 1;
        unique_tag_ids.insert(detection.tag_id);
    }
    if count == 0 {
        return 0.0;
    }

    let mean_conf = (weighted_sum / count as f64).clamp(0.0, 1.0);
    let tag_count_conf = match unique_tag_ids.len() {
        0 => 0.0,
        1 => 0.35,
        2 => 0.60,
        3 => 0.82,
        _ => 1.0,
    };
    (0.65 * mean_conf + 0.35 * tag_count_conf).clamp(0.0, 1.0)
}

pub(super) fn localization_pose_components(pose: &LocalizationPose) -> Option<(nalgebra::Vector3<f64>, UnitQuaternion<f64>)> {
    let translation = nalgebra::Vector3::new(pose.translation.x, pose.translation.y, pose.translation.z);
    if !translation.iter().all(|value| value.is_finite()) {
        return None;
    }

    let q = &pose.rotation.quaternion;
    if !(q.x.is_finite() && q.y.is_finite() && q.z.is_finite() && q.w.is_finite()) {
        return None;
    }
    let norm_sq = q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w;
    if norm_sq <= f64::EPSILON {
        return None;
    }
    let rotation = UnitQuaternion::new_normalize(Quaternion::new(q.w, q.x, q.y, q.z));
    Some((translation, rotation))
}

pub(super) fn update_localization_pose(pose: &mut LocalizationPose, translation: nalgebra::Vector3<f64>, rotation: UnitQuaternion<f64>) {
    pose.translation.x = translation.x;
    pose.translation.y = translation.y;
    pose.translation.z = translation.z;

    let (roll, pitch, yaw) = rotation.euler_angles();
    pose.rotation.roll = roll.to_degrees();
    pose.rotation.pitch = pitch.to_degrees();
    pose.rotation.yaw = yaw.to_degrees();
    pose.rotation.quaternion.x = rotation.i;
    pose.rotation.quaternion.y = rotation.j;
    pose.rotation.quaternion.z = rotation.k;
    pose.rotation.quaternion.w = rotation.w;
}

#[derive(Clone, Copy)]
pub(super) struct LocalizationPoseSmoothingInput<'a> {
    pub(super) settings: &'a LocalizationTemporalStabilizationConfig,
    pub(super) runtime_tuning: &'a LocalizationSolverRuntimeTuningConfig,
    pub(super) tag_count: usize,
    pub(super) tag_ids: &'a [u32],
    pub(super) solve_confidence: f64,
    pub(super) now: Instant,
}

pub(super) fn smooth_localization_pose(state_store: &mut HashMap<String, TemporalPoseState>, state_key: &str, pose: &mut LocalizationPose, input: LocalizationPoseSmoothingInput<'_>) {
    let settings = input.settings;
    let runtime_tuning = input.runtime_tuning;
    let tag_count = input.tag_count;
    let tag_ids = input.tag_ids;
    let solve_confidence = input.solve_confidence;
    let now = input.now;

    let Some((measurement_translation, measurement_rotation)) = localization_pose_components(pose) else {
        state_store.remove(state_key);
        return;
    };

    let state = state_store.entry(state_key.to_string()).or_insert_with(|| TemporalPoseState {
        translation: measurement_translation,
        rotation: measurement_rotation,
        updated_at: now,
        last_multi_tag_at: if tag_count >= 2 { Some(now) } else { None },
        last_tag_ids: tag_ids.to_vec(),
        reject_streak: 0,
        first_reject_at: None,
    });

    let delta_t_m = (measurement_translation - state.translation).norm();
    let delta_rot_deg = state.rotation.angle_to(&measurement_rotation).to_degrees();
    let dt_s = now.saturating_duration_since(state.updated_at).as_secs_f64();
    let recently_multi_tag = state.last_multi_tag_at.map(|last| now.saturating_duration_since(last) <= Duration::from_millis(400)).unwrap_or(false);
    let tag_set_changed = !state.last_tag_ids.is_empty() && state.last_tag_ids != tag_ids;
    let had_multiple_tags = state.last_tag_ids.len() >= 2;
    let has_tag_overlap = tag_ids.iter().any(|id| state.last_tag_ids.contains(id));
    let switched_single_tag = tag_count == 1 && tag_set_changed && !has_tag_overlap;
    let dropped_from_multi_to_single = tag_count == 1 && had_multiple_tags;

    if !settings.enabled {
        update_localization_pose(pose, measurement_translation, measurement_rotation);
        state.translation = measurement_translation;
        state.rotation = measurement_rotation;
        state.updated_at = now;
        state.last_multi_tag_at = if tag_count >= 2 { Some(now) } else { state.last_multi_tag_at };
        state.last_tag_ids = tag_ids.to_vec();
        state.reject_streak = 0;
        state.first_reject_at = None;
        return;
    }

    let use_multi_gains = tag_count >= 2 || recently_multi_tag;
    let mut translation_gain = if use_multi_gains { settings.multi_tag_translation_alpha } else { settings.single_tag_translation_alpha };
    let mut rotation_gain = if use_multi_gains { settings.multi_tag_rotation_alpha } else { settings.single_tag_rotation_alpha };

    // Keep perceived smoothing roughly stable even when poll rate changes.
    let dt_scale = if dt_s > 1e-6 { (dt_s / (1.0 / 30.0)).clamp(runtime_tuning.dt_scale_min, runtime_tuning.dt_scale_max) } else { 1.0 };
    translation_gain = scale_gain_for_dt(translation_gain, dt_scale);
    rotation_gain = scale_gain_for_dt(rotation_gain, dt_scale);

    // Confidence-adaptive smoothing:
    // low confidence => strongly damp gains; high confidence => allow responsiveness.
    let confidence = solve_confidence.clamp(0.0, 1.0);
    let confidence_scale = if confidence < 0.30 {
        0.45
    } else if confidence < 0.50 {
        0.62
    } else if confidence < 0.70 {
        0.82
    } else if confidence > 0.90 {
        1.12
    } else if confidence > 0.80 {
        1.04
    } else {
        1.0
    };
    translation_gain *= confidence_scale;
    rotation_gain *= if tag_count <= 1 { (confidence_scale * 0.90).max(0.20) } else { confidence_scale };

    // Single-tag solves are the least stable path. Clamp gains harder when the tracked tag
    // identity changes so adjacent-tag alternation cannot yank the estimate frame-to-frame.
    if switched_single_tag {
        translation_gain = translation_gain.min(0.14);
        rotation_gain = rotation_gain.min(0.12);
    } else if dropped_from_multi_to_single {
        translation_gain = translation_gain.min(0.22);
        rotation_gain = rotation_gain.min(0.19);
    } else if tag_count <= 1 && tag_set_changed {
        translation_gain *= 0.85;
        rotation_gain *= 0.85;
    }

    // Treat single-tag transitions as lower-confidence and tighten the outlier window.
    let mut max_translation_jump_m = settings.max_translation_jump_m;
    let mut max_rotation_jump_deg = settings.max_rotation_jump_deg;
    if switched_single_tag {
        max_translation_jump_m = max_translation_jump_m.min(runtime_tuning.switched_single_tag_max_translation_jump_m);
        max_rotation_jump_deg = max_rotation_jump_deg.min(runtime_tuning.switched_single_tag_max_rotation_jump_deg);
    } else if dropped_from_multi_to_single {
        max_translation_jump_m = max_translation_jump_m.min(runtime_tuning.dropped_multi_to_single_max_translation_jump_m);
        max_rotation_jump_deg = max_rotation_jump_deg.min(runtime_tuning.dropped_multi_to_single_max_rotation_jump_deg);
    }

    if tag_set_changed && tag_count <= 1 {
        state.reject_streak = 0;
        state.first_reject_at = None;
    }

    let is_outlier = tag_count <= 1 && (delta_t_m > max_translation_jump_m || delta_rot_deg > max_rotation_jump_deg);
    if is_outlier {
        state.reject_streak = state.reject_streak.saturating_add(1);
        let first_reject_at = state.first_reject_at.get_or_insert(now);
        let reject_age = now.saturating_duration_since(*first_reject_at);
        let mut reject_window_ms = settings.reanchor_reject_window_ms;
        if switched_single_tag {
            reject_window_ms = ((reject_window_ms as f64) * runtime_tuning.switched_single_tag_reject_window_scale).round() as u64;
            reject_window_ms = reject_window_ms.max(runtime_tuning.switched_single_tag_reject_window_min_ms);
        } else if dropped_from_multi_to_single {
            reject_window_ms = ((reject_window_ms as f64) * runtime_tuning.dropped_multi_to_single_reject_window_scale).round() as u64;
            reject_window_ms = reject_window_ms.max(runtime_tuning.dropped_multi_to_single_reject_window_min_ms);
        }
        if reject_age < Duration::from_millis(reject_window_ms) {
            let (damp, min_t, min_r) = if switched_single_tag {
                (runtime_tuning.switched_single_tag_gain_damp, runtime_tuning.switched_single_tag_min_translation_gain, runtime_tuning.switched_single_tag_min_rotation_gain)
            } else if dropped_from_multi_to_single {
                (runtime_tuning.dropped_multi_to_single_gain_damp, runtime_tuning.dropped_multi_to_single_min_translation_gain, runtime_tuning.dropped_multi_to_single_min_rotation_gain)
            } else if tag_set_changed && tag_count <= 1 {
                (0.55, 0.08, 0.07)
            } else {
                (0.30, 0.03, 0.03)
            };
            translation_gain = (translation_gain * damp).max(min_t);
            rotation_gain = (rotation_gain * damp).max(min_r);
            if state.reject_streak >= 3 {
                if switched_single_tag {
                    translation_gain = translation_gain.max(0.10);
                    rotation_gain = rotation_gain.max(0.09);
                } else {
                    translation_gain = translation_gain.max(0.22);
                    rotation_gain = rotation_gain.max(0.18);
                }
            }
        } else {
            // If a new regime persists, re-anchor instead of pinning indefinitely.
            if switched_single_tag {
                translation_gain = 0.88;
                rotation_gain = 0.88;
            } else {
                translation_gain = 1.0;
                rotation_gain = 1.0;
            }
            state.reject_streak = 0;
            state.first_reject_at = None;
        }
    } else {
        state.reject_streak = 0;
        state.first_reject_at = None;
    }

    let smoothed_translation = state.translation + (measurement_translation - state.translation) * translation_gain.clamp(0.0, 1.0);
    let smoothed_rotation = state.rotation.slerp(&measurement_rotation, rotation_gain.clamp(0.0, 1.0));
    update_localization_pose(pose, smoothed_translation, smoothed_rotation);

    state.translation = smoothed_translation;
    state.rotation = smoothed_rotation;
    state.updated_at = now;
    if tag_count >= 2 {
        state.last_multi_tag_at = Some(now);
    }
    state.last_tag_ids = tag_ids.to_vec();
}

fn scale_gain_for_dt(gain: f64, dt_scale: f64) -> f64 {
    let clamped = gain.clamp(0.0, 1.0);
    if clamped <= 0.0 {
        return 0.0;
    }
    if clamped >= 1.0 {
        return 1.0;
    }
    1.0 - (1.0 - clamped).powf(dt_scale)
}
