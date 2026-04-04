use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use lib_runtime_policy::{ResolvedEngineLocalizationTemporalPolicy, HELIOS_ENGINE_LOCALIZATION_TEMPORAL_POLICY};
use nalgebra::{UnitQuaternion, Vector3};

use super::*;

#[derive(Debug, Clone)]
struct TagPoseTemporalState {
    translation: Vector3<f64>,
    rotation: UnitQuaternion<f64>,
    updated_at: Instant,
    outlier_streak: u8,
}

#[derive(Debug, Clone)]
struct TagPairDistanceTemporalState {
    distance_m: f64,
    updated_at: Instant,
    outlier_streak: u8,
}

static TAG_POSE_TEMPORAL_STATE: OnceLock<Mutex<HashMap<String, TagPoseTemporalState>>> = OnceLock::new();
static TAG_PAIR_DISTANCE_TEMPORAL_STATE: OnceLock<Mutex<HashMap<String, TagPairDistanceTemporalState>>> = OnceLock::new();

fn temporal_policy() -> &'static ResolvedEngineLocalizationTemporalPolicy {
    static VALUE: OnceLock<ResolvedEngineLocalizationTemporalPolicy> = OnceLock::new();
    VALUE.get_or_init(|| HELIOS_ENGINE_LOCALIZATION_TEMPORAL_POLICY.resolve())
}

pub(super) fn pose_reliability_quality(camera_from_tag: &PoseTransform) -> f32 {
    if !camera_from_tag.translation.iter().all(|value| value.is_finite()) {
        return 0.0;
    }

    let tag_normal_in_camera = camera_from_tag.rotation.transform_vector(&Vector3::new(0.0, 0.0, 1.0));
    let frontal = tag_normal_in_camera.z.abs().clamp(0.0, 1.0);
    let mut quality = if frontal < 0.10 {
        0.08
    } else if frontal < 0.18 {
        0.16
    } else if frontal < 0.28 {
        0.34
    } else if frontal < 0.38 {
        0.56
    } else if frontal < 0.50 {
        0.78
    } else {
        1.0
    };

    let translation = camera_from_tag.translation;
    let depth = translation.z.abs();
    if !depth.is_finite() || depth < 0.08 {
        quality *= 0.2;
    } else if depth < 0.15 {
        quality *= 0.55;
    }

    let lateral = (translation.x * translation.x + translation.y * translation.y).sqrt();
    if lateral.is_finite() && depth.is_finite() && depth > 1e-6 {
        let skew_ratio = lateral / depth;
        if skew_ratio > 2.8 {
            quality *= 0.18;
        } else if skew_ratio > 2.2 {
            quality *= 0.35;
        } else if skew_ratio > 1.7 {
            quality *= 0.58;
        } else if skew_ratio > 1.3 {
            quality *= 0.78;
        }
    }

    quality as f32
}

pub(super) fn collapse_duplicate_tag_detections(source: &LocalizationSourceConfig, detections: &mut Vec<LocalizationDetection>) {
    if detections.len() < 2 {
        return;
    }

    let mut first_seen = HashMap::<u32, usize>::new();
    let mut grouped = HashMap::<u32, Vec<LocalizationDetection>>::new();
    for (idx, detection) in std::mem::take(detections).into_iter().enumerate() {
        first_seen.entry(detection.tag_id).or_insert(idx);
        grouped.entry(detection.tag_id).or_default().push(detection);
    }
    if grouped.values().all(|group| group.len() <= 1) {
        let mut ordered_ids = grouped.keys().copied().collect::<Vec<_>>();
        ordered_ids.sort_by_key(|tag_id| first_seen.get(tag_id).copied().unwrap_or(usize::MAX));
        detections.extend(ordered_ids.into_iter().filter_map(|tag_id| grouped.remove(&tag_id).and_then(|mut group| group.pop())));
        return;
    }

    let now = Instant::now();
    let stale_after = Duration::from_millis(900);
    let state_store = TAG_POSE_TEMPORAL_STATE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut prior_translation_by_tag = HashMap::<u32, Vector3<f64>>::new();
    if let Ok(state_store) = state_store.lock() {
        for (tag_id, group) in &grouped {
            if group.len() <= 1 {
                continue;
            }
            let key = format!("{}:{}:{tag_id}", source.stream_id.trim(), source.id.trim());
            if let Some(state) = state_store.get(&key) {
                if now.saturating_duration_since(state.updated_at) <= stale_after {
                    prior_translation_by_tag.insert(*tag_id, state.translation);
                }
            }
        }
    }

    let mut ordered_ids = grouped.keys().copied().collect::<Vec<_>>();
    ordered_ids.sort_by_key(|tag_id| first_seen.get(tag_id).copied().unwrap_or(usize::MAX));

    for tag_id in ordered_ids {
        let Some(group) = grouped.remove(&tag_id) else {
            continue;
        };
        if group.len() == 1 {
            detections.extend(group);
            continue;
        }

        let prior_translation = prior_translation_by_tag.get(&tag_id).copied();
        let mut scored = group
            .into_iter()
            .map(|detection| {
                let mut score = (detection.quality as f64).clamp(0.0, 1.0);
                let pose_quality = (pose_reliability_quality(&detection.camera_from_tag) as f64).clamp(0.0, 1.0);
                score *= 0.68 + (0.32 * pose_quality);
                if let Some(previous_translation) = prior_translation {
                    let shift = (detection.camera_from_tag.translation - previous_translation).norm();
                    if shift.is_finite() {
                        if shift > 2.0 {
                            score *= 0.22;
                        } else if shift > 1.2 {
                            score *= 0.36;
                        } else if shift > 0.7 {
                            score *= 0.55;
                        } else if shift > 0.35 {
                            score *= 0.78;
                        }
                    }
                }
                (detection, score.max(0.0))
            })
            .collect::<Vec<_>>();
        scored.sort_by(|left, right| right.1.partial_cmp(&left.1).unwrap_or(std::cmp::Ordering::Equal));

        let selected_score = scored.first().map(|entry| entry.1).unwrap_or(0.0);
        let mut selected = scored.remove(0).0;

        if let Some((runner_up, runner_score)) = scored.first() {
            let separation = (selected.camera_from_tag.translation - runner_up.camera_from_tag.translation).norm();
            if separation.is_finite() && selected_score > 1e-6 {
                let ambiguity = (runner_score / selected_score).clamp(0.0, 1.0);
                if ambiguity >= 0.85 && separation > 1.0 {
                    selected.quality *= 0.45;
                } else if ambiguity >= 0.72 && separation > 0.55 {
                    selected.quality *= 0.62;
                } else if ambiguity >= 0.58 && separation > 0.30 {
                    selected.quality *= 0.78;
                }
            }
        }

        detections.push(selected);
    }
}

fn detection_pose_state_key(source: &LocalizationSourceConfig, detection: &LocalizationDetection) -> String {
    format!("{}:{}:{}", source.stream_id.trim(), source.id.trim(), detection.tag_id)
}

fn detection_pair_state_key(source: &LocalizationSourceConfig, left_tag_id: u32, right_tag_id: u32) -> String {
    let (low_id, high_id) = if left_tag_id <= right_tag_id { (left_tag_id, right_tag_id) } else { (right_tag_id, left_tag_id) };
    format!("{}:{}:{}:{}", source.stream_id.trim(), source.id.trim(), low_id, high_id)
}

pub(super) fn apply_pair_distance_consistency(source: &LocalizationSourceConfig, detections: &mut [LocalizationDetection]) {
    if detections.len() < 2 {
        return;
    }

    let state_store = TAG_PAIR_DISTANCE_TEMPORAL_STATE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut state_store) = state_store.lock() else {
        return;
    };

    let now = Instant::now();
    let stale_after = Duration::from_millis(1500);
    let mut penalties = vec![1.0_f64; detections.len()];
    let pair_distance_lock_enabled = localization_pair_distance_translation_lock_enabled();
    let pair_distance_lock_strength = localization_pair_distance_translation_lock_strength();
    let pair_distance_lock_max_shift_m = localization_pair_distance_translation_lock_max_shift_m();
    let mut correction_accum = vec![Vector3::zeros(); detections.len()];
    let mut correction_weight_sum = vec![0.0_f64; detections.len()];

    for left_idx in 0..detections.len() {
        for right_idx in (left_idx + 1)..detections.len() {
            let left = &detections[left_idx];
            let right = &detections[right_idx];
            let delta = right.camera_from_tag.translation - left.camera_from_tag.translation;
            let observed_distance = delta.norm();
            if !observed_distance.is_finite() || observed_distance <= 0.02 {
                continue;
            }

            let key = detection_pair_state_key(source, left.tag_id, right.tag_id);
            let state = state_store.entry(key).or_insert_with(|| TagPairDistanceTemporalState { distance_m: observed_distance, updated_at: now, outlier_streak: 0 });

            let stale = now.saturating_duration_since(state.updated_at) > stale_after;
            if stale {
                state.distance_m = observed_distance;
                state.updated_at = now;
                state.outlier_streak = 0;
                continue;
            }

            let abs_err = (observed_distance - state.distance_m).abs();
            let rel_err = abs_err / state.distance_m.max(0.08);
            let (pair_penalty, severe_outlier) = if abs_err > 0.55 || rel_err > 0.45 {
                (0.08, true)
            } else if abs_err > 0.35 || rel_err > 0.30 {
                (0.25, true)
            } else if abs_err > 0.22 || rel_err > 0.20 {
                (0.52, false)
            } else if abs_err > 0.14 || rel_err > 0.12 {
                (0.76, false)
            } else {
                (1.0, false)
            };

            penalties[left_idx] = penalties[left_idx].min(pair_penalty);
            penalties[right_idx] = penalties[right_idx].min(pair_penalty);

            if pair_distance_lock_enabled {
                let left_confidence = ((left.weight as f64).clamp(0.0, 1.0) * (left.quality as f64).clamp(0.0, 1.0)).max(1e-6);
                let right_confidence = ((right.weight as f64).clamp(0.0, 1.0) * (right.quality as f64).clamp(0.0, 1.0)).max(1e-6);
                let (adjust_idx, anchor_idx, adjust_confidence, anchor_confidence) =
                    if left_confidence <= right_confidence { (left_idx, right_idx, left_confidence, right_confidence) } else { (right_idx, left_idx, right_confidence, left_confidence) };

                let disagreement = abs_err > 0.11 || rel_err > 0.12;
                let confidence_split = adjust_confidence < (anchor_confidence * 0.96) || severe_outlier;
                if disagreement && confidence_split {
                    let adjust_translation = detections[adjust_idx].camera_from_tag.translation;
                    let anchor_translation = detections[anchor_idx].camera_from_tag.translation;
                    let baseline = adjust_translation - anchor_translation;
                    let baseline_norm = baseline.norm();
                    if baseline_norm.is_finite() && baseline_norm > 1e-6 {
                        let target_distance = state.distance_m.max(0.02);
                        let direction = baseline / baseline_norm;
                        let correction_target = anchor_translation + direction * target_distance;
                        let correction_delta = correction_target - adjust_translation;
                        let correction_norm = correction_delta.norm();
                        if correction_norm.is_finite() && correction_norm > 1e-6 {
                            let rel_disagreement = (abs_err / state.distance_m.max(0.08)).clamp(0.0, 1.0);
                            let severity = if severe_outlier {
                                1.0
                            } else if pair_penalty <= 0.25 {
                                0.78
                            } else if pair_penalty <= 0.52 {
                                0.52
                            } else {
                                0.30
                            };
                            let confidence_term = (anchor_confidence.sqrt() * (1.0 - adjust_confidence).clamp(0.18, 1.0)).clamp(0.12, 1.0);
                            let blend = (pair_distance_lock_strength * rel_disagreement * severity * confidence_term).clamp(0.0, 0.85);
                            if blend > 1e-4 {
                                let max_shift = if severe_outlier { pair_distance_lock_max_shift_m } else { pair_distance_lock_max_shift_m * 0.55 };
                                let capped_target = adjust_translation + correction_delta * (max_shift / correction_norm).min(1.0);
                                correction_accum[adjust_idx] += capped_target * blend;
                                correction_weight_sum[adjust_idx] += blend;
                            }
                        }
                    }
                }
            }

            if severe_outlier {
                state.outlier_streak = state.outlier_streak.saturating_add(1);
                if state.outlier_streak >= 4 {
                    state.distance_m = observed_distance;
                    state.outlier_streak = 0;
                }
            } else {
                state.distance_m = (0.88 * state.distance_m) + (0.12 * observed_distance);
                state.outlier_streak = 0;
            }
            state.updated_at = now;
        }
    }

    if pair_distance_lock_enabled {
        for (idx, detection) in detections.iter_mut().enumerate() {
            let weight_sum = correction_weight_sum[idx];
            if weight_sum <= 1e-6 {
                continue;
            }
            let target = correction_accum[idx] / weight_sum;
            if !target.iter().all(|value| value.is_finite()) {
                continue;
            }
            let blend = weight_sum.clamp(0.0, 0.80);
            detection.camera_from_tag.translation = detection.camera_from_tag.translation + (target - detection.camera_from_tag.translation) * blend;
        }
    }

    for (index, detection) in detections.iter_mut().enumerate() {
        let penalty = penalties.get(index).copied().unwrap_or(1.0).clamp(0.0, 1.0) as f32;
        detection.quality *= penalty;
    }

    state_store.retain(|_key, state| now.saturating_duration_since(state.updated_at) <= stale_after);
}

fn scale_alpha_for_dt(alpha: f64, dt_s: f64) -> f64 {
    let alpha = alpha.clamp(0.0, 1.0);
    if alpha <= 0.0 {
        return 0.0;
    }
    if alpha >= 1.0 {
        return 1.0;
    }
    let dt_scale = (dt_s / (1.0 / 14.0)).clamp(0.45, 2.5);
    1.0 - (1.0 - alpha).powf(dt_scale)
}

fn localization_pair_distance_translation_lock_enabled() -> bool {
    temporal_policy().pair_distance_translation_lock_enabled
}

fn localization_pair_distance_translation_lock_strength() -> f64 {
    temporal_policy().pair_distance_translation_lock_strength
}

fn localization_pair_distance_translation_lock_max_shift_m() -> f64 {
    temporal_policy().pair_distance_translation_lock_max_shift_m
}

pub(super) fn smooth_detection_tag_poses(source: &LocalizationSourceConfig, detections: &mut [LocalizationDetection]) {
    if detections.is_empty() {
        return;
    }
    let state_store = TAG_POSE_TEMPORAL_STATE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut state_store) = state_store.lock() else {
        return;
    };
    let now = Instant::now();
    let stale_after = Duration::from_millis(1200);
    let fast_reanchor_after = Duration::from_millis(420);

    for detection in detections {
        let measurement_translation = detection.camera_from_tag.translation;
        let measurement_rotation = detection.camera_from_tag.rotation;
        if !measurement_translation.iter().all(|value| value.is_finite()) {
            continue;
        }

        let key = detection_pose_state_key(source, detection);
        let weight_quality = (detection.weight as f64).clamp(0.0, 1.0);
        let geometric_quality = (detection.quality as f64).clamp(0.0, 1.0);
        let confidence = (geometric_quality * weight_quality.sqrt()).clamp(0.0, 1.0);
        let distance_m = measurement_translation.norm();
        let mut alpha_t = if confidence >= 0.80 {
            0.42
        } else if confidence >= 0.60 {
            0.28
        } else if confidence >= 0.40 {
            0.18
        } else {
            0.08
        };
        let mut alpha_r = if confidence >= 0.80 {
            0.36
        } else if confidence >= 0.60 {
            0.22
        } else if confidence >= 0.40 {
            0.14
        } else {
            0.06
        };
        if distance_m.is_finite() {
            if distance_m > 2.5 {
                alpha_t *= 0.65;
                alpha_r *= 0.55;
            }
            if distance_m > 4.0 {
                alpha_t *= 0.55;
                alpha_r *= 0.45;
            }
        }

        let mut smoothed_translation = measurement_translation;
        let mut smoothed_rotation = measurement_rotation;
        let mut next_outlier_streak: u8 = 0;
        if let Some(previous) = state_store.get(&key) {
            let elapsed = now.saturating_duration_since(previous.updated_at);
            if elapsed <= fast_reanchor_after {
                let dt_s = elapsed.as_secs_f64().max(1e-3);
                alpha_t = scale_alpha_for_dt(alpha_t, dt_s);
                alpha_r = scale_alpha_for_dt(alpha_r, dt_s);

                let delta_t_m = (measurement_translation - previous.translation).norm();
                let delta_r_deg = previous.rotation.angle_to(&measurement_rotation).to_degrees();
                let translation_jump_limit = (0.15 + (2.8 * dt_s) + (0.03 * distance_m)).clamp(0.18, 0.55);
                let rotation_jump_limit_deg = (14.0 + (120.0 * dt_s) + (4.0 * distance_m)).clamp(18.0, 42.0);
                let severe_jump = delta_t_m > (translation_jump_limit * 2.4) || delta_r_deg > (rotation_jump_limit_deg * 2.4);
                let moderate_jump = delta_t_m > translation_jump_limit || delta_r_deg > rotation_jump_limit_deg;
                let strong_measurement = confidence >= 0.78;

                if moderate_jump {
                    next_outlier_streak = previous.outlier_streak.saturating_add(1);
                } else {
                    next_outlier_streak = 0;
                }

                if severe_jump && strong_measurement && previous.outlier_streak >= 3 {
                    smoothed_translation = measurement_translation;
                    smoothed_rotation = measurement_rotation;
                    next_outlier_streak = 0;
                } else {
                    if moderate_jump {
                        if confidence < 0.65 {
                            alpha_t = alpha_t.min(0.04);
                            alpha_r = alpha_r.min(0.03);
                        } else if confidence < 0.80 {
                            alpha_t = alpha_t.min(0.09);
                            alpha_r = alpha_r.min(0.07);
                        } else {
                            alpha_t = alpha_t.min(0.16);
                            alpha_r = alpha_r.min(0.13);
                        }
                    }

                    if next_outlier_streak >= 4 && confidence >= 0.58 {
                        smoothed_translation = measurement_translation;
                        smoothed_rotation = measurement_rotation;
                        next_outlier_streak = 0;
                    } else {
                        smoothed_translation = previous.translation + (measurement_translation - previous.translation) * alpha_t;
                        smoothed_rotation = previous.rotation.slerp(&measurement_rotation, alpha_r);
                    }
                }
            }
        }

        detection.camera_from_tag = PoseTransform { translation: smoothed_translation, rotation: smoothed_rotation };
        state_store.insert(key, TagPoseTemporalState { translation: smoothed_translation, rotation: smoothed_rotation, updated_at: now, outlier_streak: next_outlier_streak });
    }

    state_store.retain(|_key, state| now.saturating_duration_since(state.updated_at) <= stale_after);
}
