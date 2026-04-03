use super::*;

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

pub(super) fn solve_with_rotation_consensus(
    pairs: &[TranslationPair],
    rotation_pairs: &[RotationPair],
    runtime_tuning: &LocalizationSolverRuntimeTuningConfig,
) -> Option<PoseTransform> {
    solve_with_rotation_consensus_impl(pairs, rotation_pairs, runtime_tuning, true)
}

fn solve_with_rotation_consensus_impl(
    pairs: &[TranslationPair],
    rotation_pairs: &[RotationPair],
    runtime_tuning: &LocalizationSolverRuntimeTuningConfig,
    allow_prune: bool,
) -> Option<PoseTransform> {
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
        if rotation_err <= (r_thresh * runtime_tuning.rotation_consensus_inlier_rotation_scale)
            && translation_err <= (t_thresh * runtime_tuning.rotation_consensus_inlier_translation_scale)
        {
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

fn level_pose_to_world_up(pose: PoseTransform, pairs: &[TranslationPair]) -> PoseTransform {
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

pub(super) fn apply_low_confidence_attitude_guard(
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
    let tilt_limit_deg = if underconstrained_coplanar_solve {
        (runtime_tuning.map_consensus_rotation_inlier_deg * 1.35).clamp(28.0, 60.0)
    } else {
        (runtime_tuning.map_consensus_rotation_inlier_deg * 1.8).clamp(30.0, 70.0)
    };
    if tilt_deg <= tilt_limit_deg {
        return pose;
    }

    let level_target = level_pose_to_world_up(pose, pairs);
    let severity = ((tilt_deg - tilt_limit_deg) / (85.0 - tilt_limit_deg).max(1e-6)).clamp(0.0, 1.0);
    let blend = if underconstrained_coplanar_solve {
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

pub(super) fn markers_are_coplanar_in_map(
    observations: &[MarkerObservation],
    lookup: &HashMap<u32, &MarkerDefinition>,
    coplanar_delta_m: f64,
) -> bool {
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

pub(super) fn fallback_translation_only(pairs: &[TranslationPair]) -> Result<PoseTransform, String> {
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

pub(super) fn apply_coplanar_height_weight_penalties(
    map: &MarkerMap,
    observations: &[MarkerObservation],
    runtime_tuning: &LocalizationSolverRuntimeTuningConfig,
) -> Vec<f64> {
    let mut adjusted = observations
        .iter()
        .map(|obs| {
            let weight = obs.weight as f64;
            if weight.is_finite() && weight > 0.0 { weight } else { 0.0 }
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
