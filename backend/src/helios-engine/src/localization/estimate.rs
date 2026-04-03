use std::collections::HashMap;

use lib_cv::modules::localization::{MarkerDefinition, MarkerMap, MarkerObservation, PoseEstimationMethod};
use nalgebra::{DMatrix, DVector, Matrix3, Quaternion, SymmetricEigen, UnitQuaternion, Vector3};

use super::config::LocalizationSolverRuntimeTuningConfig;
use super::math::{device_pose_to_transform, PoseTransform};

mod bundle;
mod consensus;
mod weights;

use bundle::*;
use consensus::*;
use weights::*;

#[derive(Clone, Copy)]
struct TranslationPair {
    field_from_tag_t: Vector3<f64>,
    robot_from_tag_t: Vector3<f64>,
    weight: f64,
}

#[derive(Clone, Copy)]
struct RotationPair {
    pair_index: usize,
    field_from_tag_r: UnitQuaternion<f64>,
    robot_from_tag_r: UnitQuaternion<f64>,
    weight: f64,
}

/// Best-effort field pose estimate used by the localization solvers.
///
/// Unknown marker IDs and malformed observations are ignored so a stray detection does not poison
/// an otherwise-valid solve.
pub(crate) fn estimate_field_from_robot(map: &MarkerMap, observations: &[MarkerObservation], runtime_tuning: &LocalizationSolverRuntimeTuningConfig) -> Result<PoseTransform, String> {
    if observations.is_empty() {
        return Err("no marker observations".to_string());
    }

    let lookup: HashMap<u32, &MarkerDefinition> = map.markers.iter().map(|m| (m.id, m)).collect();
    let adjusted_weights = apply_coplanar_height_weight_penalties(map, observations, runtime_tuning);
    let mut pairs = Vec::<TranslationPair>::new();
    let mut rotation_pairs = Vec::<RotationPair>::new();
    let mut filtered_observations = Vec::<MarkerObservation>::new();

    let min_observation_weight = runtime_tuning.min_observation_weight.max(0.0);
    for (index, obs) in observations.iter().enumerate() {
        let base_weight = adjusted_weights.get(index).copied().unwrap_or(obs.weight as f64);
        if !base_weight.is_finite() || base_weight <= 0.0 {
            continue;
        }
        if base_weight < min_observation_weight {
            continue;
        }
        let Some(marker) = lookup.get(&obs.id) else {
            // Ignore out-of-map detections (e.g. stray tag 20 in a 1..16 map).
            continue;
        };
        let Some(obs_translation) = obs.translation else {
            continue;
        };

        let pair_index = pairs.len();
        let robot_from_tag_t = translation_vec(&obs_translation);
        let translation_quality = translation_observation_quality(&robot_from_tag_t, runtime_tuning);
        let rotation_quality = rotation_observation_quality(&robot_from_tag_t, runtime_tuning);
        let translation_weight = base_weight * translation_quality;
        if !translation_weight.is_finite() || translation_weight <= 0.0 {
            continue;
        }
        pairs.push(TranslationPair { field_from_tag_t: translation_vec(&marker.translation), robot_from_tag_t, weight: translation_weight });
        let mut weighted_obs = obs.clone();
        weighted_obs.weight = translation_weight as f32;
        filtered_observations.push(weighted_obs);

        if let (Some(map_rot), Some(obs_rot)) = (marker.rotation.as_ref(), obs.rotation.as_ref()) {
            let rotation_weight = base_weight * rotation_quality;
            if rotation_weight > 0.0 {
                rotation_pairs.push(RotationPair { pair_index, field_from_tag_r: rotation_quat(map_rot), robot_from_tag_r: rotation_quat(obs_rot), weight: rotation_weight });
            }
        }
    }

    if pairs.is_empty() {
        return Err("no usable marker observations".to_string());
    }

    // Do not hard-fail low-confidence frames here. Solvers should still be able to produce a
    // degraded fallback pose when only one tag survives filtering (or multi-tag confidence
    // collapses). This avoids full field-pose dropouts during transient occlusion/blur.
    let mut low_confidence_observations = false;
    if pairs.len() == 1 && pairs[0].weight < runtime_tuning.min_single_tag_solve_weight {
        low_confidence_observations = true;
    }

    if pairs.len() >= 2 {
        let (total_weight, max_weight, effective_count) = pair_weight_stats(&pairs);
        if total_weight < runtime_tuning.min_multi_tag_total_weight {
            low_confidence_observations = true;
        }
        if effective_count < runtime_tuning.min_multi_tag_effective_count && max_weight < (runtime_tuning.min_single_tag_solve_weight + runtime_tuning.weak_single_tag_margin.max(0.0)) {
            low_confidence_observations = true;
        }
    }

    let (translation_total_weight, _translation_max_weight, _translation_effective_count) = pair_weight_stats(&pairs);
    let (rotation_total_weight, _rotation_max_weight, rotation_effective_count) = rotation_weight_stats(&rotation_pairs);
    let rotation_support_ratio = if translation_total_weight > f64::EPSILON { (rotation_total_weight / translation_total_weight).clamp(0.0, 1.5) } else { 0.0 };
    let weak_rotation_support = rotation_support_ratio < 0.58 || (rotation_effective_count < (runtime_tuning.min_multi_tag_effective_count * 0.85) && pairs.len() >= 2);
    let underconstrained_coplanar_solve = pairs.len() <= 2 && markers_are_coplanar_in_map(&filtered_observations, &lookup, runtime_tuning.coplanar_height_delta_m);

    // Single-tag low-confidence solves are especially vulnerable to wrong attitude branches.
    // In that regime, prefer translation-only fallback over forcing rotation consensus.
    let allow_rotation_consensus = !(rotation_pairs.is_empty() || (low_confidence_observations && pairs.len() == 1));
    if allow_rotation_consensus {
        if let Some(pose) = solve_with_rotation_consensus(&pairs, &rotation_pairs, runtime_tuning) {
            let refined = bundle_refine_pose(pose, &pairs, &rotation_pairs, runtime_tuning);
            let guarded = apply_low_confidence_attitude_guard(refined, &pairs, runtime_tuning, low_confidence_observations, weak_rotation_support, underconstrained_coplanar_solve);
            return Ok(guarded);
        }
    }

    if pairs.len() >= 3 {
        match map.estimate_pose(&filtered_observations, PoseEstimationMethod::RigidProcrustes) {
            Ok(pose) => {
                let refined = bundle_refine_pose(device_pose_to_transform(&pose), &pairs, &rotation_pairs, runtime_tuning);
                let guarded = apply_low_confidence_attitude_guard(refined, &pairs, runtime_tuning, low_confidence_observations, weak_rotation_support, underconstrained_coplanar_solve);
                return Ok(guarded);
            }
            Err(err) if !low_confidence_observations => return Err(err.to_string()),
            Err(_err) => {
                // For low-confidence frames, continue to translation fallback instead of
                // propagating a hard error.
            }
        }
    }

    let fallback = fallback_translation_only(&pairs)?;
    let refined = bundle_refine_pose(fallback, &pairs, &rotation_pairs, runtime_tuning);
    let guarded = apply_low_confidence_attitude_guard(refined, &pairs, runtime_tuning, low_confidence_observations, weak_rotation_support, underconstrained_coplanar_solve);
    Ok(guarded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lib_cv::modules::localization::MarkerObservation;

    fn marker(id: u32, x: f64, y: f64, z: f64, yaw_rad: f64) -> MarkerDefinition {
        MarkerDefinition { id, translation: lib_cv::Translation3 { x, y, z }, rotation: Some(lib_cv::Rotation3 { roll: 0.0, pitch: 0.0, yaw: yaw_rad }) }
    }

    fn observation(id: u32, x: f64, y: f64, z: f64, yaw_rad: f64) -> MarkerObservation {
        MarkerObservation { id, translation: Some(lib_cv::Translation3 { x, y, z }), rotation: Some(lib_cv::Rotation3 { roll: 0.0, pitch: 0.0, yaw: yaw_rad }), pixel: None, weight: 1.0 }
    }

    #[test]
    fn two_marker_solve_prefers_rotation_hint_over_mirrored_baseline() {
        // Field markers 15/16 from the map sit near z=8.24m with both facing the same direction.
        let map = MarkerMap { markers: vec![marker(16, -0.1408118, 0.55245, 8.2399764, std::f64::consts::PI), marker(15, 0.2909882, 0.55245, 8.2399764, std::f64::consts::PI)] };

        // Observations roughly match a camera ~0.86m from the board at ~0.3048m height.
        // Translation-only baseline alignment mirrors this case; rotation hint keeps it in-bounds.
        let observations = vec![observation(16, 0.1831406, 0.5648159, 0.8599873, std::f64::consts::PI), observation(15, -0.2116369, 0.5338395, 0.8585437, std::f64::consts::PI)];

        let solved = estimate_field_from_robot(&map, &observations, &LocalizationSolverRuntimeTuningConfig::default()).expect("pose solve");
        assert!(solved.translation.z < 8.26, "expected in-bounds z, got {}", solved.translation.z);
        assert!(solved.translation.z > 7.2, "expected near board-facing pose, got {}", solved.translation.z);
    }

    #[test]
    fn two_marker_solve_tracks_full_orientation_when_rotations_exist() {
        let map = MarkerMap { markers: vec![marker(1, -0.25, 0.55, 8.24, std::f64::consts::PI), marker(2, 0.25, 0.55, 8.24, std::f64::consts::PI)] };
        let field_from_robot_rot = UnitQuaternion::from_euler_angles(12_f64.to_radians(), -18_f64.to_radians(), 25_f64.to_radians());
        let field_from_tag = UnitQuaternion::from_euler_angles(0.0, 0.0, std::f64::consts::PI);
        let robot_from_tag_rot = field_from_robot_rot.inverse() * field_from_tag;
        let (roll, pitch, yaw) = robot_from_tag_rot.euler_angles();

        let observations = vec![
            MarkerObservation { id: 1, translation: Some(lib_cv::Translation3 { x: 0.2, y: 0.55, z: 0.9 }), rotation: Some(lib_cv::Rotation3 { roll, pitch, yaw }), pixel: None, weight: 1.0 },
            MarkerObservation { id: 2, translation: Some(lib_cv::Translation3 { x: -0.2, y: 0.52, z: 0.9 }), rotation: Some(lib_cv::Rotation3 { roll, pitch, yaw }), pixel: None, weight: 1.0 },
        ];

        let solved = estimate_field_from_robot(&map, &observations, &LocalizationSolverRuntimeTuningConfig::default()).expect("pose solve");
        let angle_err = solved.rotation.angle_to(&field_from_robot_rot).to_degrees();
        assert!(angle_err < 1.0, "expected close orientation recovery, got {angle_err}deg");
    }

    #[test]
    fn ignores_unknown_marker_ids() {
        let map = MarkerMap { markers: vec![marker(1, 1.0, 0.5, 2.0, std::f64::consts::PI)] };
        let observations = vec![observation(20, 0.0, 0.0, 1.0, 0.0), observation(1, 0.2, 0.5, 0.8, std::f64::consts::PI)];
        let solved = estimate_field_from_robot(&map, &observations, &LocalizationSolverRuntimeTuningConfig::default()).expect("solve with unknown marker ignored");
        assert!(solved.translation.z.is_finite());
    }

    #[test]
    fn consensus_rejects_outlier_rotation_hint() {
        let map = MarkerMap { markers: vec![marker(1, -0.8, 0.55, 4.0, std::f64::consts::PI), marker(2, 0.0, 0.55, 4.2, std::f64::consts::PI), marker(3, 0.8, 0.55, 3.9, std::f64::consts::PI)] };
        let true_pose = PoseTransform { translation: Vector3::new(0.1, 0.35, 3.0), rotation: UnitQuaternion::from_euler_angles(8_f64.to_radians(), -10_f64.to_radians(), 20_f64.to_radians()) };

        let mut observations = Vec::new();
        for marker in &map.markers {
            let field_from_tag_r = rotation_quat(marker.rotation.as_ref().unwrap());
            let robot_from_tag_r = true_pose.rotation.inverse() * field_from_tag_r;
            let robot_from_tag_t = true_pose.rotation.inverse().transform_vector(&(translation_vec(&marker.translation) - true_pose.translation));
            let (roll, pitch, yaw) = robot_from_tag_r.euler_angles();
            observations.push(MarkerObservation {
                id: marker.id,
                translation: Some(lib_cv::Translation3 { x: robot_from_tag_t.x, y: robot_from_tag_t.y, z: robot_from_tag_t.z }),
                rotation: Some(lib_cv::Rotation3 { roll, pitch, yaw }),
                pixel: None,
                weight: 1.0,
            });
        }

        // Corrupt one marker rotation heavily; translation remains valid.
        if let Some(obs) = observations.get_mut(1) {
            let wrong = UnitQuaternion::from_axis_angle(&Vector3::x_axis(), std::f64::consts::PI) * rotation_quat(obs.rotation.as_ref().unwrap());
            let (roll, pitch, yaw) = wrong.euler_angles();
            obs.rotation = Some(lib_cv::Rotation3 { roll, pitch, yaw });
        }

        let solved = estimate_field_from_robot(&map, &observations, &LocalizationSolverRuntimeTuningConfig::default()).expect("robust solve");
        let up_expected = true_pose.rotation.transform_vector(&Vector3::y());
        let up_solved = solved.rotation.transform_vector(&Vector3::y());
        let up_dot = up_expected.dot(&up_solved).clamp(-1.0, 1.0);
        let up_angle_err = up_dot.acos();
        assert!(up_angle_err < 10_f64.to_radians(), "expected robust roll/pitch despite one bad marker, got err rad={up_angle_err}");
    }
}
