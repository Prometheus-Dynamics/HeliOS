use super::*;

pub(super) fn translation_for_rotation(rotation: UnitQuaternion<f64>, pairs: &[TranslationPair]) -> Option<Vector3<f64>> {
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

pub(super) fn bundle_refine_pose(initial: PoseTransform, pairs: &[TranslationPair], rotation_pairs: &[RotationPair], runtime_tuning: &LocalizationSolverRuntimeTuningConfig) -> PoseTransform {
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
