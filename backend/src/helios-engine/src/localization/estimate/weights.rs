use super::*;

pub(super) fn lateral_ratio_scale(lateral_ratio: f64, thresholds: [f64; 4], scales: [f64; 4]) -> f64 {
    let [mild_threshold, medium_threshold, high_threshold, extreme_threshold] = thresholds;
    let [mild_scale, medium_scale, high_scale, extreme_scale] = scales;
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

pub(super) fn translation_vec(t: &lib_cv::Translation3) -> Vector3<f64> {
    Vector3::new(t.x, t.y, t.z)
}

pub(super) fn rotation_quat(r: &lib_cv::Rotation3) -> UnitQuaternion<f64> {
    UnitQuaternion::from_euler_angles(r.roll, r.pitch, r.yaw)
}

pub(super) fn rotation_observation_quality(
    robot_from_tag_t: &Vector3<f64>,
    runtime_tuning: &LocalizationSolverRuntimeTuningConfig,
) -> f64 {
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

    let vertical_ratio = (robot_from_tag_t.y.abs() / distance).clamp(0.0, 1.0);
    if vertical_ratio > runtime_tuning.rotation_vertical_ratio_severe {
        quality *= runtime_tuning.rotation_vertical_severe_scale.clamp(0.0, 1.0);
    } else if vertical_ratio > runtime_tuning.rotation_vertical_ratio_mild {
        quality *= runtime_tuning.rotation_vertical_mild_scale.clamp(0.0, 1.0);
    }

    let depth = robot_from_tag_t.z.abs().max(1e-6);
    let lateral_ratio = (robot_from_tag_t.x.abs() / depth).clamp(0.0, 10.0);
    quality *= lateral_ratio_scale(
        lateral_ratio,
        [
            runtime_tuning.lateral_ratio_mild,
            runtime_tuning.lateral_ratio_medium,
            runtime_tuning.lateral_ratio_high,
            runtime_tuning.lateral_ratio_extreme,
        ],
        [
            runtime_tuning.rotation_lateral_mild_scale,
            runtime_tuning.rotation_lateral_medium_scale,
            runtime_tuning.rotation_lateral_high_scale,
            runtime_tuning.rotation_lateral_extreme_scale,
        ],
    );

    quality.clamp(floor, 1.0)
}

pub(super) fn translation_observation_quality(
    robot_from_tag_t: &Vector3<f64>,
    runtime_tuning: &LocalizationSolverRuntimeTuningConfig,
) -> f64 {
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
        [
            runtime_tuning.lateral_ratio_mild,
            runtime_tuning.lateral_ratio_medium,
            runtime_tuning.lateral_ratio_high,
            runtime_tuning.lateral_ratio_extreme,
        ],
        [
            runtime_tuning.translation_lateral_mild_scale,
            runtime_tuning.translation_lateral_medium_scale,
            runtime_tuning.translation_lateral_high_scale,
            runtime_tuning.translation_lateral_extreme_scale,
        ],
    );

    quality.clamp(floor, 1.0)
}

pub(super) fn weighted_average_quaternion(samples: &[(UnitQuaternion<f64>, f64)]) -> Option<UnitQuaternion<f64>> {
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

pub(super) fn weighted_average_translation(samples: &[(Vector3<f64>, f64)]) -> Option<Vector3<f64>> {
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

pub(super) fn median(mut values: Vec<f64>) -> Option<f64> {
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

pub(super) fn pair_weight_stats(pairs: &[TranslationPair]) -> (f64, f64, f64) {
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

pub(super) fn rotation_weight_stats(pairs: &[RotationPair]) -> (f64, f64, f64) {
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
