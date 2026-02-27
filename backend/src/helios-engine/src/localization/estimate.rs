use std::collections::HashMap;

use lib_cv::modules::localization::{MarkerDefinition, MarkerMap, MarkerObservation, PoseEstimationMethod};
use nalgebra::{DMatrix, DVector, Matrix3, Quaternion, SymmetricEigen, UnitQuaternion, Vector3};

use super::config::LocalizationSolverRuntimeTuningConfig;
use super::math::{device_pose_to_transform, PoseTransform};

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

fn lateral_ratio_scale(
    lateral_ratio: f64,
    mild_threshold: f64,
    medium_threshold: f64,
    high_threshold: f64,
    extreme_threshold: f64,
    mild_scale: f64,
    medium_scale: f64,
    high_scale: f64,
    extreme_scale: f64,
) -> f64 {
    if lateral_ratio > extreme_threshold {
        extreme_scale
    } else if lateral_ratio > high_threshold {
        high_scale
    } else if lateral_ratio > medium_threshold {
        medium_scale
    } else if lateral_ratio > mild_threshold {
        mild_scale
    } else {
        1.0
    }
}

fn translation_vec(t: &lib_cv::Translation3) -> Vector3<f64> {
    Vector3::new(t.x, t.y, t.z)
}

fn rotation_quat(r: &lib_cv::Rotation3) -> UnitQuaternion<f64> {
    UnitQuaternion::from_euler_angles(r.roll, r.pitch, r.yaw)
}

fn rotation_observation_quality(robot_from_tag_t: &Vector3<f64>, runtime_tuning: &LocalizationSolverRuntimeTuningConfig) -> f64 {
    if !robot_from_tag_t.iter().all(|value| value.is_finite()) {
        return 0.0;
    }
    let distance = robot_from_tag_t.norm();
    if !distance.is_finite() || distance <= 1e-6 {
        return 0.0;
    }

    let near = runtime_tuning.rotation_distance_near_m.max(1e-6);
    let mid = runtime_tuning.rotation_distance_mid_m.max(near + 1e-6);
    let far = runtime_tuning.rotation_distance_far_m.max(mid + 1e-6);
    let mid_q = runtime_tuning.rotation_distance_mid_quality.clamp(0.0, 1.0);
    let far_q = runtime_tuning.rotation_distance_far_quality.clamp(0.0, mid_q);
    let floor = runtime_tuning.rotation_quality_floor.clamp(0.0, far_q);

    // Rotation is materially noisier than translation for small/far tags. Damp angular influence
    // as range increases so far markers still help translation without dominating attitude.
    let mut quality: f64 = if distance <= near {
        1.0
    } else if distance <= mid {
        let t = ((distance - near) / (mid - near)).clamp(0.0, 1.0);
        1.0 + (mid_q - 1.0) * t
    } else if distance <= far {
        let t = ((distance - mid) / (far - mid)).clamp(0.0, 1.0);
        mid_q + (far_q - mid_q) * t
    } else {
        far_q
    };

    // High vertical parallax relative to range is another common source of unstable roll/pitch.
    let vertical_ratio = (robot_from_tag_t.y.abs() / distance).clamp(0.0, 1.0);
    if vertical_ratio > runtime_tuning.rotation_vertical_ratio_severe {
        quality *= runtime_tuning.rotation_vertical_severe_scale.clamp(0.0, 1.0);
    } else if vertical_ratio > runtime_tuning.rotation_vertical_ratio_mild {
        quality *= runtime_tuning.rotation_vertical_mild_scale.clamp(0.0, 1.0);
    }

    // Oblique side-views (large lateral/depth ratio) are a major yaw/roll instability source.
    // Damp rotational influence aggressively in these regimes.
    let depth = robot_from_tag_t.z.abs().max(1e-6);
    let lateral_ratio = (robot_from_tag_t.x.abs() / depth).clamp(0.0, 10.0);
    quality *= lateral_ratio_scale(
        lateral_ratio,
        runtime_tuning.lateral_ratio_mild,
        runtime_tuning.lateral_ratio_medium,
        runtime_tuning.lateral_ratio_high,
        runtime_tuning.lateral_ratio_extreme,
        runtime_tuning.rotation_lateral_mild_scale,
        runtime_tuning.rotation_lateral_medium_scale,
        runtime_tuning.rotation_lateral_high_scale,
        runtime_tuning.rotation_lateral_extreme_scale,
    );

    quality.clamp(floor, 1.0)
}

fn translation_observation_quality(robot_from_tag_t: &Vector3<f64>, runtime_tuning: &LocalizationSolverRuntimeTuningConfig) -> f64 {
    if !robot_from_tag_t.iter().all(|value| value.is_finite()) {
        return 0.0;
    }
    let distance = robot_from_tag_t.norm();
    if !distance.is_finite() || distance <= 1e-6 {
        return 0.0;
    }

    let near = runtime_tuning.rotation_distance_near_m.max(1e-6);
    let mid = runtime_tuning.rotation_distance_mid_m.max(near + 1e-6);
    let far = runtime_tuning.rotation_distance_far_m.max(mid + 1e-6);
    let mid_q = runtime_tuning.translation_distance_mid_quality.clamp(0.0, 1.0);
    let far_q = runtime_tuning.translation_distance_far_quality.clamp(0.0, mid_q);
    let floor = runtime_tuning.translation_quality_floor.clamp(0.0, far_q);

    // Translation remains more reliable than rotation at distance, so keep a gentler curve.
    let mut quality = if distance <= near {
        1.0
    } else if distance <= mid {
        let t = ((distance - near) / (mid - near)).clamp(0.0, 1.0);
        1.0 + (mid_q - 1.0) * t
    } else if distance <= far {
        let t = ((distance - mid) / (far - mid)).clamp(0.0, 1.0);
        mid_q + (far_q - mid_q) * t
    } else {
        far_q
    };

    let depth = robot_from_tag_t.z.abs().max(1e-6);
    let lateral_ratio = (robot_from_tag_t.x.abs() / depth).clamp(0.0, 10.0);
    quality *= lateral_ratio_scale(
        lateral_ratio,
        runtime_tuning.lateral_ratio_mild,
        runtime_tuning.lateral_ratio_medium,
        runtime_tuning.lateral_ratio_high,
        runtime_tuning.lateral_ratio_extreme,
        runtime_tuning.translation_lateral_mild_scale,
        runtime_tuning.translation_lateral_medium_scale,
        runtime_tuning.translation_lateral_high_scale,
        runtime_tuning.translation_lateral_extreme_scale,
    );

    quality.clamp(floor, 1.0)
}

fn weighted_average_quaternion(samples: &[(UnitQuaternion<f64>, f64)]) -> Option<UnitQuaternion<f64>> {
    if samples.is_empty() {
        return None;
    }
    let mut acc_w = 0.0;
    let mut acc_i = 0.0;
    let mut acc_j = 0.0;
    let mut acc_k = 0.0;
    let ref_q = samples[0].0;
    for (sample, weight) in samples {
        if !weight.is_finite() || *weight <= 0.0 {
            continue;
        }
        let mut q = *sample;
        if q.coords.dot(&ref_q.coords) < 0.0 {
            q = UnitQuaternion::new_normalize(-q.into_inner());
        }
        let quat = q.into_inner();
        acc_w += quat.w * *weight;
        acc_i += quat.i * *weight;
        acc_j += quat.j * *weight;
        acc_k += quat.k * *weight;
    }
    let norm_sq = acc_w * acc_w + acc_i * acc_i + acc_j * acc_j + acc_k * acc_k;
    if !norm_sq.is_finite() || norm_sq <= f64::EPSILON {
        return None;
    }
    Some(UnitQuaternion::new_normalize(Quaternion::new(acc_w, acc_i, acc_j, acc_k)))
}

fn weighted_average_translation(samples: &[(Vector3<f64>, f64)]) -> Option<Vector3<f64>> {
    let mut total_w = 0.0f64;
    let mut acc = Vector3::zeros();
    for (value, weight) in samples {
        if !weight.is_finite() || *weight <= 0.0 {
            continue;
        }
        acc += *value * *weight;
        total_w += *weight;
    }
    if !total_w.is_finite() || total_w <= f64::EPSILON {
        return None;
    }
    Some(acc / total_w)
}

fn median(mut values: Vec<f64>) -> Option<f64> {
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

fn translation_for_rotation(rotation: UnitQuaternion<f64>, pairs: &[TranslationPair]) -> Option<Vector3<f64>> {
    let translated = pairs.iter().map(|pair| (pair.field_from_tag_t - rotation.transform_vector(&pair.robot_from_tag_t), pair.weight)).collect::<Vec<_>>();
    weighted_average_translation(&translated)
}

fn huber_weight(norm: f64, delta: f64) -> f64 {
    if !norm.is_finite() || norm <= 0.0 || !delta.is_finite() || delta <= 0.0 {
        return 1.0;
    }
    if norm <= delta {
        1.0
    } else {
        (delta / norm).clamp(0.0, 1.0)
    }
}

fn rotation_condition_scale(pairs: &[TranslationPair], runtime_tuning: &LocalizationSolverRuntimeTuningConfig) -> f64 {
    if pairs.len() < 2 {
        return 1.0;
    }
    let mut covariance = Matrix3::zeros();
    let mut weight_sum = 0.0;
    for pair in pairs {
        if !pair.weight.is_finite() || pair.weight <= 0.0 {
            continue;
        }
        let norm = pair.robot_from_tag_t.norm();
        if !norm.is_finite() || norm <= 1e-6 {
            continue;
        }
        let direction = pair.robot_from_tag_t / norm;
        covariance += pair.weight * (direction * direction.transpose());
        weight_sum += pair.weight;
    }
    if weight_sum <= f64::EPSILON {
        return 1.0;
    }
    covariance /= weight_sum;
    let eigen = SymmetricEigen::new(covariance);
    let mut values = [eigen.eigenvalues[0], eigen.eigenvalues[1], eigen.eigenvalues[2]];
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let max_value = values[2].max(1e-9);
    let ratio = (values[0].max(0.0) / max_value).clamp(0.0, 1.0);
    let low_ratio = runtime_tuning.bundle_condition_low_ratio.max(1e-6);
    let mid_ratio = runtime_tuning.bundle_condition_mid_ratio.max(low_ratio + 1e-6);
    if ratio <= low_ratio {
        runtime_tuning.bundle_condition_low_rotation_scale
    } else if ratio <= mid_ratio {
        let t = ((ratio - low_ratio) / (mid_ratio - low_ratio)).clamp(0.0, 1.0);
        runtime_tuning.bundle_condition_low_rotation_scale + (runtime_tuning.bundle_condition_mid_rotation_scale - runtime_tuning.bundle_condition_low_rotation_scale) * t
    } else {
        1.0
    }
}

fn apply_pose_step(pose: PoseTransform, step: &DVector<f64>) -> Option<PoseTransform> {
    if step.len() != 6 {
        return None;
    }
    if !step.iter().all(|v| v.is_finite()) {
        return None;
    }
    let dt = Vector3::new(step[0], step[1], step[2]);
    let dr = Vector3::new(step[3], step[4], step[5]);
    let delta_rot = UnitQuaternion::from_scaled_axis(dr);
    let next = PoseTransform { translation: pose.translation + dt, rotation: delta_rot * pose.rotation };
    if !next.translation.iter().all(|v| v.is_finite()) || !next.rotation.coords.iter().all(|v| v.is_finite()) {
        return None;
    }
    Some(next)
}

fn bundle_residual_vector(
    pose: &PoseTransform,
    pairs: &[TranslationPair],
    rotation_pairs: &[RotationPair],
    runtime_tuning: &LocalizationSolverRuntimeTuningConfig,
    rotation_scale: f64,
) -> DVector<f64> {
    let translation_huber = runtime_tuning.bundle_translation_huber_m.max(1e-6);
    let rotation_huber = runtime_tuning.bundle_rotation_huber_deg.to_radians().max(1e-6);
    let rotation_weight = runtime_tuning.bundle_rotation_weight.max(0.0) * rotation_scale.clamp(0.0, 1.0);
    let mut residuals = Vec::<f64>::with_capacity(3 * (pairs.len() + rotation_pairs.len()));

    for pair in pairs {
        if !pair.weight.is_finite() || pair.weight <= 0.0 {
            continue;
        }
        let predicted = pose.translation + pose.rotation.transform_vector(&pair.robot_from_tag_t);
        let delta = pair.field_from_tag_t - predicted;
        let norm = delta.norm();
        let weight = pair.weight * huber_weight(norm, translation_huber);
        if !weight.is_finite() || weight <= 0.0 {
            continue;
        }
        let sqrt_w = weight.sqrt();
        residuals.push(sqrt_w * delta.x);
        residuals.push(sqrt_w * delta.y);
        residuals.push(sqrt_w * delta.z);
    }

    if rotation_weight > 0.0 {
        for pair in rotation_pairs {
            if !pair.weight.is_finite() || pair.weight <= 0.0 {
                continue;
            }
            let predicted = pose.rotation * pair.robot_from_tag_r;
            let error_quat = pair.field_from_tag_r * predicted.inverse();
            let error_axis = error_quat.scaled_axis();
            let norm = error_axis.norm();
            let weight = pair.weight * rotation_weight * huber_weight(norm, rotation_huber);
            if !weight.is_finite() || weight <= 0.0 {
                continue;
            }
            let sqrt_w = weight.sqrt();
            residuals.push(sqrt_w * error_axis.x);
            residuals.push(sqrt_w * error_axis.y);
            residuals.push(sqrt_w * error_axis.z);
        }
    }

    DVector::from_vec(residuals)
}

fn bundle_refine_pose(initial: PoseTransform, pairs: &[TranslationPair], rotation_pairs: &[RotationPair], runtime_tuning: &LocalizationSolverRuntimeTuningConfig) -> PoseTransform {
    if !runtime_tuning.bundle_refine_enabled || pairs.len() < 3 {
        return initial;
    }

    let rotation_scale = rotation_condition_scale(pairs, runtime_tuning);
    let mut pose = initial;
    let mut best_residual = bundle_residual_vector(&pose, pairs, rotation_pairs, runtime_tuning, rotation_scale);
    if best_residual.len() < 6 {
        return initial;
    }
    let mut best_cost = best_residual.dot(&best_residual);
    let initial_cost = best_cost;
    let damping = runtime_tuning.bundle_damping.max(1e-9);

    for _ in 0..runtime_tuning.bundle_max_iterations {
        let residual = bundle_residual_vector(&pose, pairs, rotation_pairs, runtime_tuning, rotation_scale);
        if residual.len() < 6 {
            break;
        }
        let m = residual.len();
        let mut jacobian = DMatrix::<f64>::zeros(m, 6);
        let eps_t = 1e-4;
        let eps_r = 1e-5;

        for col in 0..6 {
            let eps = if col < 3 { eps_t } else { eps_r };
            let mut step = DVector::<f64>::zeros(6);
            step[col] = eps;
            let Some(plus_pose) = apply_pose_step(pose, &step) else {
                continue;
            };
            step[col] = -eps;
            let Some(minus_pose) = apply_pose_step(pose, &step) else {
                continue;
            };
            let plus = bundle_residual_vector(&plus_pose, pairs, rotation_pairs, runtime_tuning, rotation_scale);
            let minus = bundle_residual_vector(&minus_pose, pairs, rotation_pairs, runtime_tuning, rotation_scale);
            if plus.len() != m || minus.len() != m {
                continue;
            }
            let derivative = (plus - minus) * (0.5 / eps);
            jacobian.set_column(col, &derivative);
        }

        let jt = jacobian.transpose();
        let hessian = (&jt * &jacobian) + (DMatrix::<f64>::identity(6, 6) * damping);
        let gradient = &jt * &residual;
        let Some(step) = hessian.lu().solve(&(-gradient)) else {
            break;
        };
        if step.norm() <= 1e-7 || !step.iter().all(|v| v.is_finite()) {
            break;
        }

        let mut accepted = false;
        let mut candidate_pose = pose;
        let mut candidate_residual = best_residual.clone();
        let mut candidate_cost = best_cost;
        for scale in [1.0, 0.5, 0.25, 0.1] {
            let trial_step = &step * scale;
            let Some(trial_pose) = apply_pose_step(pose, &trial_step) else {
                continue;
            };
            let trial_residual = bundle_residual_vector(&trial_pose, pairs, rotation_pairs, runtime_tuning, rotation_scale);
            if trial_residual.len() != m {
                continue;
            }
            let trial_cost = trial_residual.dot(&trial_residual);
            if trial_cost + 1e-12 < candidate_cost {
                accepted = true;
                candidate_pose = trial_pose;
                candidate_residual = trial_residual;
                candidate_cost = trial_cost;
            }
        }

        if !accepted {
            break;
        }

        pose = candidate_pose;
        best_residual = candidate_residual;
        if (best_cost - candidate_cost).abs() < 1e-9 {
            best_cost = candidate_cost;
            break;
        }
        best_cost = candidate_cost;
    }

    if best_cost <= (initial_cost * runtime_tuning.bundle_min_improvement_ratio) {
        pose
    } else {
        initial
    }
}

fn world_up_tilt_rad(rotation: UnitQuaternion<f64>) -> f64 {
    let up = rotation.transform_vector(&Vector3::y());
    up.dot(&Vector3::y()).clamp(-1.0, 1.0).acos()
}

fn score_pose(pose: &PoseTransform, pairs: &[TranslationPair], rotation_pairs: &[RotationPair]) -> (f64, Vec<f64>, Vec<f64>) {
    let rotation_weight = 0.22_f64;
    let mut score = 0.0f64;
    let mut translation_residuals = Vec::with_capacity(pairs.len());
    let mut rotation_residuals = Vec::with_capacity(rotation_pairs.len());

    for pair in pairs {
        let predicted = pose.translation + pose.rotation.transform_vector(&pair.robot_from_tag_t);
        let delta = pair.field_from_tag_t - predicted;
        let horizontal = (delta.x * delta.x + delta.z * delta.z).sqrt();
        let vertical = delta.y.abs();
        // Under heavy perspective skew, y/depth can drift badly. Emphasize vertical consistency
        // so co-planar tags in the map don't split apart in solved space.
        let err = (horizontal * horizontal + (2.0 * vertical) * (2.0 * vertical)).sqrt();
        translation_residuals.push(err);

        let capped = err.min(0.35);
        let tail = (err - 0.35).max(0.0);
        score += pair.weight * (capped + (2.0 * tail * tail));
    }

    for pair in rotation_pairs {
        let predicted = pose.rotation * pair.robot_from_tag_r;
        let err = predicted.angle_to(&pair.field_from_tag_r).abs();
        rotation_residuals.push(err);

        let cap = 25_f64.to_radians();
        let capped = err.min(cap);
        let tail = (err - cap).max(0.0);
        score += pair.weight * rotation_weight * (capped + (1.5 * tail * tail));
    }

    // Wrong homography/PnP branches can produce near-inverted attitudes under heavy skew.
    // Penalize those strongly so multi-tag consensus lands on the physically plausible branch.
    let tilt = world_up_tilt_rad(pose.rotation);
    if tilt > 55_f64.to_radians() {
        let excess = tilt - 55_f64.to_radians();
        let norm = excess / 12_f64.to_radians();
        score += 6.0 * norm * norm;
    }
    if tilt > 80_f64.to_radians() {
        let excess = tilt - 80_f64.to_radians();
        let norm = excess / 10_f64.to_radians();
        score += 200.0 + (400.0 * norm * norm);
    }

    (score, translation_residuals, rotation_residuals)
}

fn candidate_pose_from_rotation_pair(pair: &RotationPair, pairs: &[TranslationPair]) -> PoseTransform {
    let rotation = pair.field_from_tag_r * pair.robot_from_tag_r.inverse();
    let t_pair = pairs[pair.pair_index];
    let translation = t_pair.field_from_tag_t - rotation.transform_vector(&t_pair.robot_from_tag_t);
    PoseTransform { translation, rotation }
}

fn pair_translation_errors(pose: &PoseTransform, pairs: &[TranslationPair]) -> Vec<f64> {
    pairs
        .iter()
        .map(|pair| {
            let predicted = pose.translation + pose.rotation.transform_vector(&pair.robot_from_tag_t);
            (pair.field_from_tag_t - predicted).norm()
        })
        .collect::<Vec<_>>()
}

fn solve_with_rotation_consensus(pairs: &[TranslationPair], rotation_pairs: &[RotationPair], runtime_tuning: &LocalizationSolverRuntimeTuningConfig) -> Option<PoseTransform> {
    solve_with_rotation_consensus_impl(pairs, rotation_pairs, runtime_tuning, true)
}

fn solve_with_rotation_consensus_impl(pairs: &[TranslationPair], rotation_pairs: &[RotationPair], runtime_tuning: &LocalizationSolverRuntimeTuningConfig, allow_prune: bool) -> Option<PoseTransform> {
    if pairs.is_empty() || rotation_pairs.is_empty() {
        return None;
    }

    let mut best_pose: Option<PoseTransform> = None;
    let mut best_score = f64::INFINITY;
    let mut best_t_residuals = Vec::new();
    let mut best_r_residuals = Vec::new();

    for pair in rotation_pairs {
        let candidate = candidate_pose_from_rotation_pair(pair, pairs);
        let (score, t_residuals, r_residuals) = score_pose(&candidate, pairs, rotation_pairs);
        if score < best_score {
            best_score = score;
            best_pose = Some(candidate);
            best_t_residuals = t_residuals;
            best_r_residuals = r_residuals;
        }
    }

    let best = best_pose?;
    let t_med = median(best_t_residuals).unwrap_or(0.0);
    let r_med = median(best_r_residuals).unwrap_or(0.0);
    let t_thresh = (t_med * 2.2).clamp(0.05, 0.26);
    let r_thresh = (r_med * 2.2).clamp(7_f64.to_radians(), 32_f64.to_radians());
    let pair_t_err = pair_translation_errors(&best, pairs);

    if allow_prune && pairs.len() >= 3 {
        let prune_limit = (t_thresh * 1.12).clamp(0.05, 0.24);
        let inlier_indices = pair_t_err.iter().enumerate().filter_map(|(idx, err)| (*err <= prune_limit).then_some(idx)).collect::<Vec<_>>();

        if inlier_indices.len() >= 2 && inlier_indices.len() < pairs.len() {
            let index_remap = inlier_indices.iter().enumerate().map(|(new_idx, old_idx)| (*old_idx, new_idx)).collect::<HashMap<usize, usize>>();
            let inlier_pairs = inlier_indices.iter().map(|idx| pairs[*idx]).collect::<Vec<_>>();
            let inlier_rot_pairs = rotation_pairs
                .iter()
                .filter_map(|pair| {
                    let pair_index = *index_remap.get(&pair.pair_index)?;
                    Some(RotationPair { pair_index, field_from_tag_r: pair.field_from_tag_r, robot_from_tag_r: pair.robot_from_tag_r, weight: pair.weight })
                })
                .collect::<Vec<_>>();

            if !inlier_rot_pairs.is_empty() {
                if let Some(pruned_pose) = solve_with_rotation_consensus_impl(&inlier_pairs, &inlier_rot_pairs, runtime_tuning, false) {
                    let (pruned_score, _, _) = score_pose(&pruned_pose, pairs, rotation_pairs);
                    if pruned_score + 1e-9 < best_score {
                        return Some(pruned_pose);
                    }
                }
            }
        }
    }

    let mut rotation_samples = Vec::new();
    for pair in rotation_pairs {
        let predicted = best.rotation * pair.robot_from_tag_r;
        let rotation_err = predicted.angle_to(&pair.field_from_tag_r).abs();
        let translation_err = pair_t_err[pair.pair_index];
        if rotation_err <= (r_thresh * runtime_tuning.rotation_consensus_inlier_rotation_scale) && translation_err <= (t_thresh * runtime_tuning.rotation_consensus_inlier_translation_scale) {
            let candidate = candidate_pose_from_rotation_pair(pair, pairs);
            rotation_samples.push((candidate.rotation, pair.weight));
        }
    }

    let rotation = weighted_average_quaternion(&rotation_samples).unwrap_or(best.rotation);
    let translation_samples = pairs
        .iter()
        .enumerate()
        .filter_map(|(idx, pair)| {
            let candidate_t = pair.field_from_tag_t - rotation.transform_vector(&pair.robot_from_tag_t);
            (pair_t_err[idx] <= (t_thresh * runtime_tuning.translation_consensus_inlier_scale)).then_some((candidate_t, pair.weight))
        })
        .collect::<Vec<_>>();
    let translation = weighted_average_translation(&translation_samples).or_else(|| translation_for_rotation(rotation, pairs))?;

    Some(PoseTransform { translation, rotation })
}

fn pair_weight_stats(pairs: &[TranslationPair]) -> (f64, f64, f64) {
    let mut total_weight = 0.0f64;
    let mut max_weight = 0.0f64;
    let mut sum_sq = 0.0f64;
    for pair in pairs {
        if !pair.weight.is_finite() || pair.weight <= 0.0 {
            continue;
        }
        total_weight += pair.weight;
        max_weight = max_weight.max(pair.weight);
        sum_sq += pair.weight * pair.weight;
    }
    let effective_count = if sum_sq > f64::EPSILON { (total_weight * total_weight / sum_sq).clamp(0.0, pairs.len() as f64) } else { 0.0 };
    (total_weight, max_weight, effective_count)
}

fn rotation_weight_stats(pairs: &[RotationPair]) -> (f64, f64, f64) {
    let mut total_weight = 0.0f64;
    let mut max_weight = 0.0f64;
    let mut sum_sq = 0.0f64;
    for pair in pairs {
        if !pair.weight.is_finite() || pair.weight <= 0.0 {
            continue;
        }
        total_weight += pair.weight;
        max_weight = max_weight.max(pair.weight);
        sum_sq += pair.weight * pair.weight;
    }
    let effective_count = if sum_sq > f64::EPSILON { (total_weight * total_weight / sum_sq).clamp(0.0, pairs.len() as f64) } else { 0.0 };
    (total_weight, max_weight, effective_count)
}

fn level_pose_to_world_up(pose: PoseTransform, pairs: &[TranslationPair]) -> PoseTransform {
    // Preserve heading from the solved pose, but force world-up alignment. This avoids
    // low-confidence roll/pitch branches from sending field-space height into impossible values.
    let forward = pose.rotation.transform_vector(&Vector3::new(0.0, 0.0, 1.0));
    let horizontal_forward = Vector3::new(forward.x, 0.0, forward.z);
    let Some(horizontal_forward) = horizontal_forward.try_normalize(1e-9) else {
        return pose;
    };
    let yaw = horizontal_forward.x.atan2(horizontal_forward.z);
    let leveled_rotation = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), yaw);
    if let Some(translation) = translation_for_rotation(leveled_rotation, pairs) {
        PoseTransform { translation, rotation: leveled_rotation }
    } else {
        pose
    }
}

fn apply_low_confidence_attitude_guard(
    pose: PoseTransform,
    pairs: &[TranslationPair],
    runtime_tuning: &LocalizationSolverRuntimeTuningConfig,
    low_confidence_observations: bool,
    weak_rotation_support: bool,
    underconstrained_coplanar_solve: bool,
) -> PoseTransform {
    if pairs.len() < 2 {
        return pose;
    }
    if !(low_confidence_observations || weak_rotation_support || underconstrained_coplanar_solve) {
        return pose;
    }
    let tilt_deg = world_up_tilt_rad(pose.rotation).to_degrees().abs();
    // Only apply emergency leveling on extreme tilt in low-confidence solves.
    // This avoids flattening valid camera attitudes while still guarding against
    // catastrophic branch flips.
    let tilt_limit_deg = if underconstrained_coplanar_solve {
        (runtime_tuning.map_consensus_rotation_inlier_deg * 1.35).clamp(28.0, 60.0)
    } else {
        (runtime_tuning.map_consensus_rotation_inlier_deg * 1.8).clamp(30.0, 70.0)
    };
    if tilt_deg <= tilt_limit_deg {
        return pose;
    }

    // Blend toward level based on how implausible the tilt is, but never fully clamp.
    // This keeps natural non-level camera attitude while suppressing catastrophic spikes.
    let level_target = level_pose_to_world_up(pose, pairs);
    let severity = ((tilt_deg - tilt_limit_deg) / (85.0 - tilt_limit_deg).max(1e-6)).clamp(0.0, 1.0);
    let blend = if underconstrained_coplanar_solve {
        // Underconstrained 2-tag coplanar solves have ambiguous roll/pitch and can blow up height.
        // Use a nonlinear stronger (but still partial) bias toward world-up so mild tilt is
        // preserved while catastrophic branches are damped hard.
        if tilt_deg >= 45.0 {
            (0.75 + severity * 0.15).clamp(0.0, 0.90)
        } else {
            ((severity * severity) * 1.8).clamp(0.0, 0.90)
        }
    } else {
        let confidence_scale = if low_confidence_observations { 1.0 } else { 0.65 };
        let support_scale = if weak_rotation_support { 1.0 } else { 0.70 };
        (severity * confidence_scale * support_scale * 0.80).clamp(0.0, 0.80)
    };
    if blend <= 1e-6 {
        return pose;
    }

    let blended_rotation = pose.rotation.slerp(&level_target.rotation, blend);
    let blended_translation = translation_for_rotation(blended_rotation, pairs).unwrap_or(pose.translation);
    PoseTransform { translation: blended_translation, rotation: blended_rotation }
}

fn markers_are_coplanar_in_map(observations: &[MarkerObservation], lookup: &HashMap<u32, &MarkerDefinition>, coplanar_delta_m: f64) -> bool {
    if observations.len() < 2 {
        return false;
    }
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut count = 0usize;
    for observation in observations {
        let Some(marker) = lookup.get(&observation.id) else {
            continue;
        };
        let y = marker.translation.y;
        if !y.is_finite() {
            continue;
        }
        min_y = min_y.min(y);
        max_y = max_y.max(y);
        count += 1;
    }
    if count < 2 || !min_y.is_finite() || !max_y.is_finite() {
        return false;
    }
    (max_y - min_y).abs() <= coplanar_delta_m.max(1e-6)
}

fn wrap_angle_rad(angle: f64) -> f64 {
    let mut wrapped = angle;
    while wrapped > std::f64::consts::PI {
        wrapped -= std::f64::consts::TAU;
    }
    while wrapped <= -std::f64::consts::PI {
        wrapped += std::f64::consts::TAU;
    }
    wrapped
}

fn translation_yaw_hints(pairs: &[TranslationPair]) -> Vec<(f64, f64)> {
    let mut hints = Vec::new();
    for i in 0..pairs.len() {
        for j in (i + 1)..pairs.len() {
            let p = pairs[i];
            let q = pairs[j];
            let mut v_field = q.field_from_tag_t - p.field_from_tag_t;
            let mut v_robot = q.robot_from_tag_t - p.robot_from_tag_t;
            v_field.y = 0.0;
            v_robot.y = 0.0;
            let nf = v_field.norm();
            let nr = v_robot.norm();
            if !nf.is_finite() || !nr.is_finite() || nf <= 1e-9 || nr <= 1e-9 {
                continue;
            }
            v_field /= nf;
            v_robot /= nr;
            let dot = (v_robot.x * v_field.x) + (v_robot.z * v_field.z);
            let cross_y = (v_robot.z * v_field.x) - (v_robot.x * v_field.z);
            let yaw = cross_y.atan2(dot);
            let weight = (p.weight * q.weight * nf.min(2.0)).max(1e-6);
            hints.push((wrap_angle_rad(yaw), weight));
        }
    }
    hints
}

fn fallback_translation_only(pairs: &[TranslationPair]) -> Result<PoseTransform, String> {
    if pairs.is_empty() {
        return Err("no usable marker observations".to_string());
    }
    if pairs.len() == 1 {
        let pair = pairs[0];
        return Ok(PoseTransform { translation: pair.field_from_tag_t - pair.robot_from_tag_t, rotation: UnitQuaternion::identity() });
    }

    let yaw_hints = translation_yaw_hints(pairs);
    if yaw_hints.is_empty() {
        return Err("degenerate marker configuration".to_string());
    }

    let mut best_pose = None;
    let mut best_score = f64::INFINITY;
    for (yaw, _weight) in yaw_hints {
        let rotation = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), yaw);
        let Some(translation) = translation_for_rotation(rotation, pairs) else {
            continue;
        };
        let mut score = 0.0;
        for pair in pairs {
            let predicted = translation + rotation.transform_vector(&pair.robot_from_tag_t);
            score += pair.weight * (pair.field_from_tag_t - predicted).norm();
        }
        if score < best_score {
            best_score = score;
            best_pose = Some(PoseTransform { translation, rotation });
        }
    }

    best_pose.ok_or_else(|| "failed translation solve".to_string())
}

fn apply_coplanar_height_weight_penalties(map: &MarkerMap, observations: &[MarkerObservation], runtime_tuning: &LocalizationSolverRuntimeTuningConfig) -> Vec<f64> {
    let mut adjusted = observations
        .iter()
        .map(|obs| {
            let weight = obs.weight as f64;
            if weight.is_finite() && weight > 0.0 {
                weight
            } else {
                0.0
            }
        })
        .collect::<Vec<_>>();

    if observations.len() < 2 {
        return adjusted;
    }

    let marker_lookup: HashMap<u32, &MarkerDefinition> = map.markers.iter().map(|marker| (marker.id, marker)).collect();
    for i in 0..observations.len() {
        for j in (i + 1)..observations.len() {
            if adjusted[i] <= 0.0 || adjusted[j] <= 0.0 {
                continue;
            }
            let left = &observations[i];
            let right = &observations[j];
            let (Some(left_marker), Some(right_marker)) = (marker_lookup.get(&left.id), marker_lookup.get(&right.id)) else {
                continue;
            };
            let map_height_delta = (left_marker.translation.y - right_marker.translation.y).abs();
            if map_height_delta > runtime_tuning.coplanar_height_delta_m {
                continue;
            }
            let (Some(left_t), Some(right_t)) = (left.translation.as_ref(), right.translation.as_ref()) else {
                continue;
            };
            let observed_height_delta = (left_t.y - right_t.y).abs();
            let penalty = if observed_height_delta > runtime_tuning.severe_observed_height_delta_m {
                runtime_tuning.severe_penalty
            } else if observed_height_delta > runtime_tuning.moderate_observed_height_delta_m {
                runtime_tuning.moderate_penalty
            } else if observed_height_delta > runtime_tuning.mild_observed_height_delta_m {
                runtime_tuning.mild_penalty
            } else {
                1.0
            };

            if penalty >= 1.0 {
                continue;
            }
            if adjusted[i] <= adjusted[j] {
                adjusted[i] *= penalty;
            } else {
                adjusted[j] *= penalty;
            }
        }
    }

    adjusted
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
        let base_weight = adjusted_weights.get(index).copied().unwrap_or_else(|| obs.weight as f64);
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
    let allow_rotation_consensus = !rotation_pairs.is_empty() && !(low_confidence_observations && pairs.len() == 1);
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
