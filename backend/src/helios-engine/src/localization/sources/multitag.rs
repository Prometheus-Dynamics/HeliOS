use lib_runtime_policy::{ResolvedEngineLocalizationMultitagPolicy, HELIOS_ENGINE_LOCALIZATION_MULTITAG_POLICY};
use nalgebra::{UnitQuaternion, Vector3};
use std::sync::OnceLock;

use super::*;

fn multitag_policy() -> &'static ResolvedEngineLocalizationMultitagPolicy {
    static VALUE: OnceLock<ResolvedEngineLocalizationMultitagPolicy> = OnceLock::new();
    VALUE.get_or_init(|| HELIOS_ENGINE_LOCALIZATION_MULTITAG_POLICY.resolve())
}

fn localization_multitag_normal_lock_enabled() -> bool {
    multitag_policy().normal_lock_enabled
}

fn localization_multitag_normal_lock_strength() -> f64 {
    multitag_policy().normal_lock_strength
}

fn localization_multitag_normal_lock_max_spread_deg() -> f64 {
    multitag_policy().normal_lock_max_spread_deg
}

fn localization_multitag_normal_lock_max_depth_spread_m() -> f64 {
    multitag_policy().normal_lock_max_depth_spread_m
}

fn localization_multitag_full_lock_same_code_rot_enabled() -> bool {
    multitag_policy().full_lock_same_code_rot_enabled
}

fn localization_multitag_full_lock_strength() -> f64 {
    multitag_policy().full_lock_strength
}

fn localization_multitag_full_lock_max_spread_deg() -> f64 {
    multitag_policy().full_lock_max_spread_deg
}

fn localization_multitag_coplanar_depth_lock_enabled() -> bool {
    multitag_policy().coplanar_depth_lock_enabled
}

fn localization_multitag_coplanar_depth_lock_strength() -> f64 {
    multitag_policy().coplanar_depth_lock_strength
}

fn localization_multitag_coplanar_depth_lock_max_shift_m() -> f64 {
    multitag_policy().coplanar_depth_lock_max_shift_m
}

fn rotation_with_locked_normal(current: UnitQuaternion<f64>, target_normal: Vector3<f64>) -> Option<UnitQuaternion<f64>> {
    let z = target_normal.try_normalize(1e-12)?;
    let mut x = current.transform_vector(&Vector3::new(1.0, 0.0, 0.0));
    x -= z * x.dot(&z);
    let x = if let Some(xn) = x.try_normalize(1e-12) {
        xn
    } else {
        let mut y = current.transform_vector(&Vector3::new(0.0, 1.0, 0.0));
        y -= z * y.dot(&z);
        y.try_normalize(1e-12)?
    };
    let y = z.cross(&x).try_normalize(1e-12)?;
    let x = y.cross(&z).try_normalize(1e-12)?;

    let r = nalgebra::Matrix3::from_columns(&[x, y, z]);
    let r = nalgebra::Rotation3::from_matrix_unchecked(r);
    Some(UnitQuaternion::from_rotation_matrix(&r))
}

pub(super) fn apply_multitag_normal_consistency(detections: &mut [LocalizationDetection]) {
    if detections.len() < 2 || !localization_multitag_normal_lock_enabled() {
        return;
    }

    let mut normal_sum = Vector3::zeros();
    let mut weighted_normals = Vec::with_capacity(detections.len());
    let mut min_depth = f64::INFINITY;
    let mut max_depth = f64::NEG_INFINITY;
    for detection in detections.iter() {
        let normal = detection.camera_from_tag.rotation.transform_vector(&Vector3::new(0.0, 0.0, 1.0));
        if !normal.iter().all(|value| value.is_finite()) {
            continue;
        }
        let depth = detection.camera_from_tag.translation.z.abs();
        if depth.is_finite() {
            min_depth = min_depth.min(depth);
            max_depth = max_depth.max(depth);
        }
        let weight = ((detection.weight as f64).clamp(0.0, 1.0) * (detection.quality as f64).clamp(0.0, 1.0)).max(1e-6);
        normal_sum += normal * weight;
        weighted_normals.push((normal, weight));
    }
    if weighted_normals.len() < 2 {
        return;
    }
    if min_depth.is_finite() && max_depth.is_finite() {
        let depth_spread = (max_depth - min_depth).abs();
        if depth_spread > localization_multitag_normal_lock_max_depth_spread_m() {
            return;
        }
    }

    let Some(consensus_normal) = normal_sum.try_normalize(1e-12) else {
        return;
    };
    let max_spread_deg = localization_multitag_normal_lock_max_spread_deg();
    let strength = localization_multitag_normal_lock_strength();

    let mut max_spread_seen = 0.0f64;
    for (normal, _weight) in &weighted_normals {
        let dot = normal.normalize().dot(&consensus_normal).clamp(-1.0, 1.0);
        let spread = dot.acos().to_degrees().abs();
        max_spread_seen = max_spread_seen.max(spread);
    }
    if !max_spread_seen.is_finite() || max_spread_seen > max_spread_deg {
        return;
    }

    for detection in detections.iter_mut() {
        let normal = detection.camera_from_tag.rotation.transform_vector(&Vector3::new(0.0, 0.0, 1.0));
        if !normal.iter().all(|value| value.is_finite()) {
            continue;
        }
        let dot = normal.normalize().dot(&consensus_normal).clamp(-1.0, 1.0);
        let spread_deg = dot.acos().to_degrees().abs();
        if spread_deg < 0.5 {
            continue;
        }
        let rel = (spread_deg / max_spread_deg).clamp(0.0, 1.0);
        let blend = (strength * (1.0 - rel).sqrt()).clamp(0.0, 1.0);
        if blend <= 1e-4 {
            continue;
        }
        if let Some(locked) = rotation_with_locked_normal(detection.camera_from_tag.rotation, consensus_normal) {
            detection.camera_from_tag.rotation = detection.camera_from_tag.rotation.slerp(&locked, blend);
        }
    }

    let same_code_rotation =
        detections.first().and_then(|detection| detection.code_rotation).is_some_and(|code_rotation| detections.iter().all(|detection| detection.code_rotation == Some(code_rotation)));

    if localization_multitag_coplanar_depth_lock_enabled() && same_code_rotation {
        let mut depth_samples: Vec<(usize, f64, f64)> = Vec::with_capacity(detections.len());
        let mut depth_weight_sum = 0.0f64;
        let mut depth_weighted_sum = 0.0f64;
        for (idx, detection) in detections.iter().enumerate() {
            let translation = detection.camera_from_tag.translation;
            if !translation.iter().all(|value| value.is_finite()) {
                continue;
            }
            let depth_along_normal = consensus_normal.dot(&translation);
            if !depth_along_normal.is_finite() {
                continue;
            }
            let weight = ((detection.weight as f64).clamp(0.0, 1.0) * (detection.quality as f64).clamp(0.0, 1.0)).max(1e-6);
            depth_samples.push((idx, depth_along_normal, weight));
            depth_weight_sum += weight;
            depth_weighted_sum += weight * depth_along_normal;
        }

        if depth_samples.len() >= 2 && depth_weight_sum > f64::EPSILON {
            let anchor_weight = depth_samples.iter().map(|(_idx, _depth, weight)| *weight).fold(0.0f64, f64::max);
            if anchor_weight < 0.08 {
                return;
            }

            let depth_mean = depth_weighted_sum / depth_weight_sum;
            let max_depth_err = depth_samples.iter().map(|(_idx, depth, _weight)| (depth - depth_mean).abs()).fold(0.0f64, f64::max);

            if max_depth_err > 0.01 {
                let strength = localization_multitag_coplanar_depth_lock_strength();
                let max_shift = localization_multitag_coplanar_depth_lock_max_shift_m();
                for (idx, depth, weight) in depth_samples {
                    let error = depth_mean - depth;
                    if !error.is_finite() {
                        continue;
                    }
                    let rel = (error.abs() / max_depth_err.max(1e-6)).clamp(0.0, 1.0);
                    let confidence = weight.sqrt().clamp(0.35, 1.0);
                    let blend = (strength * rel.sqrt() * confidence).clamp(0.0, 1.0);
                    if blend <= 1e-4 {
                        continue;
                    }
                    let delta = (error * blend).clamp(-max_shift, max_shift);
                    if delta.abs() <= 1e-6 {
                        continue;
                    }

                    let translation = detections[idx].camera_from_tag.translation;
                    let depth_now = consensus_normal.dot(&translation);
                    let tangent = translation - consensus_normal * depth_now;
                    detections[idx].camera_from_tag.translation = tangent + consensus_normal * (depth_now + delta);
                }
            }
        }
    }

    if !localization_multitag_full_lock_same_code_rot_enabled() || !same_code_rotation {
        return;
    }

    let ref_q = *detections[0].camera_from_tag.rotation.quaternion();
    let ref_v = Vector3::new(ref_q.i, ref_q.j, ref_q.k);
    let mut sum_x = 0.0f64;
    let mut sum_y = 0.0f64;
    let mut sum_z = 0.0f64;
    let mut sum_w = 0.0f64;
    let mut total_w = 0.0f64;
    for detection in detections.iter() {
        let q = *detection.camera_from_tag.rotation.quaternion();
        let mut x = q.i;
        let mut y = q.j;
        let mut z = q.k;
        let mut w = q.w;
        let dot = x * ref_v.x + y * ref_v.y + z * ref_v.z + w * ref_q.w;
        if dot < 0.0 {
            x = -x;
            y = -y;
            z = -z;
            w = -w;
        }
        let weight = ((detection.weight as f64).clamp(0.0, 1.0) * (detection.quality as f64).clamp(0.0, 1.0)).max(1e-6);
        sum_x += weight * x;
        sum_y += weight * y;
        sum_z += weight * z;
        sum_w += weight * w;
        total_w += weight;
    }
    if total_w <= 0.0 {
        return;
    }
    let norm = (sum_x * sum_x + sum_y * sum_y + sum_z * sum_z + sum_w * sum_w).sqrt();
    if !norm.is_finite() || norm <= 1e-12 {
        return;
    }
    let consensus_q = UnitQuaternion::new_normalize(nalgebra::Quaternion::new(sum_w / norm, sum_x / norm, sum_y / norm, sum_z / norm));

    let max_spread = localization_multitag_full_lock_max_spread_deg();
    let mut worst_spread = 0.0f64;
    for detection in detections.iter() {
        let spread = detection.camera_from_tag.rotation.angle_to(&consensus_q).to_degrees().abs();
        if spread.is_finite() {
            worst_spread = worst_spread.max(spread);
        }
    }
    if !worst_spread.is_finite() || worst_spread > max_spread {
        return;
    }

    let strength = localization_multitag_full_lock_strength();
    for detection in detections.iter_mut() {
        let spread = detection.camera_from_tag.rotation.angle_to(&consensus_q).to_degrees().abs();
        if !spread.is_finite() || spread < 0.2 {
            continue;
        }
        let rel = (spread / max_spread).clamp(0.0, 1.0);
        let blend = (strength * (1.0 - rel).sqrt()).clamp(0.0, 1.0);
        if blend <= 1e-4 {
            continue;
        }
        detection.camera_from_tag.rotation = detection.camera_from_tag.rotation.slerp(&consensus_q, blend);
    }
}
