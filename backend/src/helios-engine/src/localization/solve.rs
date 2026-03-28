use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use lib_cv::modules::aruco::pose::TagPoseCalibration;
use lib_cv::modules::localization::{MarkerDefinition, MarkerMap};
use lib_cv::{Rotation3, Translation3};
use nalgebra::{Quaternion, UnitQuaternion};

use super::config::{
    LocalizationFieldOriginMode, LocalizationPoseSpace, LocalizationProfile, LocalizationSolverConfig, LocalizationSolverRuntimeTuningConfig, LocalizationSourceConfig,
    LocalizationTemporalStabilizationConfig,
};
use super::fetch::LocalizationSourceFetcher;
use super::maps::{FieldMapDocument, FieldMapSource};
use super::math::{compose_transforms, invert_transform, PoseTransform};
use super::solvers::{SolverContext, SolverRegistry};
use super::sources::{fetch_source_samples_with_registry, SourceParserRegistry, SourceSample};
use super::types::{LocalizationSolveResponse, LocalizationSolveTimings, LocalizationSolverOutputs, LocalizationSolverResult, LocalizationSourceSampleStatus};

#[derive(Debug, Clone)]
struct TemporalPoseState {
    translation: nalgebra::Vector3<f64>,
    rotation: UnitQuaternion<f64>,
    updated_at: Instant,
    last_multi_tag_at: Option<Instant>,
    last_tag_ids: Vec<u32>,
    reject_streak: u32,
    first_reject_at: Option<Instant>,
}

static SOLVER_TEMPORAL_STATE: OnceLock<Mutex<HashMap<String, TemporalPoseState>>> = OnceLock::new();

pub async fn solve_localization<F: LocalizationSourceFetcher>(
    profile: &LocalizationProfile,
    sources: &[LocalizationSourceConfig],
    rig_poses: &HashMap<String, PoseTransform>,
    field_map: Option<&FieldMapDocument>,
    calibrations: &HashMap<String, TagPoseCalibration>,
    fetcher: &F,
    apply_field_origin: bool,
) -> LocalizationSolveResponse {
    let solver_registry = SolverRegistry::with_defaults();
    let parser_registry = SourceParserRegistry::with_defaults();
    let ctx = LocalizationSolveContext { profile, sources, rig_poses, field_map, calibrations, fetcher, solver_registry: &solver_registry, parser_registry: &parser_registry, apply_field_origin };
    solve_localization_with_registry(ctx).await
}

pub struct LocalizationSolveContext<'a, F> {
    pub profile: &'a LocalizationProfile,
    pub sources: &'a [LocalizationSourceConfig],
    pub rig_poses: &'a HashMap<String, PoseTransform>,
    pub field_map: Option<&'a FieldMapDocument>,
    pub calibrations: &'a HashMap<String, TagPoseCalibration>,
    pub fetcher: &'a F,
    pub solver_registry: &'a SolverRegistry,
    pub parser_registry: &'a SourceParserRegistry,
    pub apply_field_origin: bool,
}

pub async fn solve_localization_with_registry<F: LocalizationSourceFetcher>(ctx: LocalizationSolveContext<'_, F>) -> LocalizationSolveResponse {
    let total_started = Instant::now();
    let marker_map = ctx.field_map.map(marker_map_from_field_map);
    let default_tag_size_m = ctx.profile.tag_size_m.or_else(|| infer_tag_size_from_field_map(ctx.field_map));

    let source_parse_started = Instant::now();
    let mut source_samples = fetch_source_samples_with_registry(ctx.fetcher, ctx.sources, default_tag_size_m, ctx.calibrations, ctx.parser_registry).await;
    let source_parse_ms = source_parse_started.elapsed().as_secs_f64() * 1000.0;
    apply_profile_tag_filter(ctx.profile, &mut source_samples);
    let source_statuses = source_samples
        .iter()
        .map(|sample| LocalizationSourceSampleStatus {
            source_id: sample.source.id.clone(),
            stream_id: sample.source.stream_id.clone(),
            output_key: sample.source.output_key.clone(),
            camera_uid: sample.source.camera_uid.clone(),
            detections: sample.detections.len(),
            poll_ms: sample.poll_ms,
            tag_size: sample.tag_size,
            error: sample.error.clone(),
        })
        .collect();

    let solver_started = Instant::now();
    let mut solver_results = ctx.profile.solvers.iter().map(|solver| solve_for_solver(ctx.solver_registry, solver, &source_samples, ctx.rig_poses, marker_map.as_ref())).collect::<Vec<_>>();
    apply_profile_postprocessing(ctx.profile, &mut solver_results, ctx.rig_poses, ctx.field_map, ctx.apply_field_origin);
    let solver_ms = solver_started.elapsed().as_secs_f64() * 1000.0;

    LocalizationSolveResponse {
        profile_id: ctx.profile.id.clone(),
        solvers: solver_results,
        sources: source_statuses,
        timings: LocalizationSolveTimings { source_fetch_ms: 0.0, source_parse_ms, solver_ms, engine_ms: total_started.elapsed().as_secs_f64() * 1000.0, total_ms: 0.0, cache_hit: false },
    }
}

fn infer_tag_size_from_field_map(field_map: Option<&FieldMapDocument>) -> Option<f64> {
    let map = field_map?;
    let mut buckets: HashMap<i64, (usize, f64)> = HashMap::new();

    for marker in &map.markers {
        let size = marker.size_m;
        if !size.is_finite() || size <= 0.0 {
            continue;
        }
        // Bucket by micrometers so equivalent sizes with minor float noise collapse.
        let key = (size * 1_000_000.0).round() as i64;
        let entry = buckets.entry(key).or_insert((0, size));
        entry.0 += 1;
    }

    buckets.into_iter().max_by_key(|(_key, (count, _size))| *count).map(|(_key, (_count, size))| size)
}

fn apply_profile_tag_filter(profile: &LocalizationProfile, samples: &mut [SourceSample]) {
    if profile.allowed_tag_ids.is_empty() && profile.excluded_tag_ids.is_empty() {
        return;
    }
    let allowed = profile.allowed_tag_ids.iter().copied().collect::<HashSet<_>>();
    let excluded = profile.excluded_tag_ids.iter().copied().collect::<HashSet<_>>();
    for sample in samples {
        sample.detections.retain(|detection| {
            let tag_id = detection.tag_id;
            let allowed_match = allowed.is_empty() || allowed.contains(&tag_id);
            allowed_match && !excluded.contains(&tag_id)
        });
    }
}

fn apply_profile_postprocessing(
    profile: &LocalizationProfile,
    solver_results: &mut [LocalizationSolverResult],
    rig_poses: &HashMap<String, PoseTransform>,
    field_map: Option<&FieldMapDocument>,
    apply_field_origin: bool,
) {
    apply_temporal_pose_stabilization(profile, solver_results);

    let snap_height = profile.snap_z_to_ground;
    let snap_roll = profile.snap_roll_to_ground;
    let snap_pitch = profile.snap_pitch_to_ground;

    if snap_height || snap_roll || snap_pitch {
        // Snap selected field-space pose components to ground-level orientation/height.
        for solver in solver_results.iter_mut() {
            let mut has_robot_pose = false;
            if let Some(pose) = solver.outputs.robot_in_field.as_mut() {
                apply_field_pose_snaps(&mut pose.pose, snap_height, snap_roll, snap_pitch);
                has_robot_pose = true;
            }

            // When a robot pose exists, camera poses are re-derived from robot+rig below to keep a
            // rigid camera/body relationship. Only snap camera outputs directly for camera-only solves.
            if !has_robot_pose {
                if let Some(list) = solver.outputs.camera_in_field.as_mut() {
                    for entry in list {
                        apply_field_pose_snaps(&mut entry.pose, snap_height, snap_roll, snap_pitch);
                    }
                }
            } else if let Some(list) = solver.outputs.camera_in_field.as_mut() {
                // Preserve legacy behavior for cameras that have no rig pose configured.
                for entry in list {
                    if !rig_poses.contains_key(&entry.camera_uid) {
                        apply_field_pose_snaps(&mut entry.pose, snap_height, snap_roll, snap_pitch);
                    }
                }
            }
        }
    }

    if apply_field_origin {
        apply_profile_field_origin(profile, field_map, solver_results);
    }

    for solver in solver_results {
        enforce_camera_pose_consistency(&mut solver.outputs, rig_poses);
    }
}

fn apply_profile_field_origin(profile: &LocalizationProfile, field_map: Option<&FieldMapDocument>, solver_results: &mut [LocalizationSolverResult]) {
    let origin_from_center = field_origin_from_center_transform(profile, field_map);
    if is_identity_transform(&origin_from_center) {
        return;
    }

    for solver in solver_results.iter_mut() {
        if let Some(robot_pose) = solver.outputs.robot_in_field.as_mut() {
            transform_localization_pose_in_place(&mut robot_pose.pose, &origin_from_center);
        }
        if let Some(camera_poses) = solver.outputs.camera_in_field.as_mut() {
            for entry in camera_poses {
                transform_localization_pose_in_place(&mut entry.pose, &origin_from_center);
            }
        }
    }
}

fn transform_localization_pose_in_place(pose: &mut crate::localization::types::LocalizationPose, parent_from_child: &PoseTransform) {
    let Some((translation, rotation)) = localization_pose_components(pose) else {
        return;
    };
    let child = PoseTransform { translation, rotation };
    let transformed = compose_transforms(parent_from_child, &child);
    update_localization_pose(pose, transformed.translation, transformed.rotation);
}

fn is_identity_transform(transform: &PoseTransform) -> bool {
    transform.translation.norm_squared() <= 1e-12 && transform.rotation.angle().abs() <= 1e-12
}

fn field_origin_from_center_transform(profile: &LocalizationProfile, field_map: Option<&FieldMapDocument>) -> PoseTransform {
    let (field_width_m, field_depth_m) = field_dimensions_for_origin(field_map);
    let half_width = field_width_m * 0.5;
    let half_depth = field_depth_m * 0.5;

    let sanitized_origin = profile.field_origin.sanitized();
    let (origin_x, origin_z, origin_yaw_deg) = match sanitized_origin.mode {
        LocalizationFieldOriginMode::Center => (0.0, 0.0, 0.0),
        LocalizationFieldOriginMode::Blue => (-half_width, -half_depth, 0.0),
        LocalizationFieldOriginMode::Red => (half_width, half_depth, 180.0),
        LocalizationFieldOriginMode::Custom => {
            if let Some(custom) = sanitized_origin.custom {
                (custom.x, custom.z, custom.yaw_deg)
            } else {
                (-half_width, -half_depth, 0.0)
            }
        }
    };

    let center_from_origin =
        PoseTransform { translation: nalgebra::Vector3::new(origin_x, 0.0, origin_z), rotation: UnitQuaternion::from_axis_angle(&nalgebra::Vector3::y_axis(), origin_yaw_deg.to_radians()) };
    invert_transform(&center_from_origin)
}

fn field_dimensions_for_origin(field_map: Option<&FieldMapDocument>) -> (f64, f64) {
    const DEFAULT_FIELD_WIDTH_M: f64 = 8.2296;
    const DEFAULT_FIELD_DEPTH_M: f64 = 16.4592;

    if let Some(map) = field_map {
        if map.width_m.is_finite() && map.width_m > 0.0 && map.depth_m.is_finite() && map.depth_m > 0.0 {
            return (map.width_m, map.depth_m);
        }
    }

    (DEFAULT_FIELD_WIDTH_M, DEFAULT_FIELD_DEPTH_M)
}

fn enforce_camera_pose_consistency(outputs: &mut LocalizationSolverOutputs, rig_poses: &HashMap<String, PoseTransform>) {
    let Some(robot_pose) = outputs.robot_in_field.as_ref() else {
        return;
    };
    let Some((robot_translation, robot_rotation)) = localization_pose_components(&robot_pose.pose) else {
        return;
    };
    let field_from_robot = PoseTransform { translation: robot_translation, rotation: robot_rotation };
    let Some(camera_outputs) = outputs.camera_in_field.as_mut() else {
        return;
    };

    for entry in camera_outputs {
        let Some(robot_from_camera) = rig_poses.get(&entry.camera_uid) else {
            continue;
        };
        let field_from_camera = compose_transforms(&field_from_robot, robot_from_camera);
        update_localization_pose(&mut entry.pose, field_from_camera.translation, field_from_camera.rotation);
    }
}

fn apply_field_pose_snaps(pose: &mut crate::localization::types::LocalizationPose, snap_height: bool, snap_roll: bool, snap_pitch: bool) {
    // Viewer frame: +Y is up.
    if snap_height {
        pose.translation.y = 0.0;
    }

    if !(snap_roll || snap_pitch) {
        return;
    }

    let q = &pose.rotation.quaternion;
    let quat_norm_sq = q.x * q.x + q.y * q.y + q.z * q.z + q.w * q.w;
    let base_rotation = if q.x.is_finite() && q.y.is_finite() && q.z.is_finite() && q.w.is_finite() && quat_norm_sq > f64::EPSILON {
        UnitQuaternion::new_normalize(Quaternion::new(q.w, q.x, q.y, q.z))
    } else {
        // Fallback for malformed quaternion payloads: match UI semantics
        // (pitch=X, yaw=Y, roll=Z).
        UnitQuaternion::from_euler_angles(pose.rotation.pitch.to_radians(), pose.rotation.yaw.to_radians(), pose.rotation.roll.to_radians())
    };

    // Preserve heading from projected forward (-Z), then clear selected tilt axes in yaw-neutral
    // space to avoid Euler branch flips near 180deg heading.
    let forward = base_rotation.transform_vector(&nalgebra::Vector3::new(0.0, 0.0, -1.0));
    let planar_len = (forward.x * forward.x + forward.z * forward.z).sqrt();
    let yaw_y = if planar_len > 1e-9 { (-forward.x).atan2(-forward.z) } else { 0.0 };
    let yaw_rotation = UnitQuaternion::from_axis_angle(&nalgebra::Vector3::y_axis(), yaw_y);
    let rotation = if snap_roll && snap_pitch {
        yaw_rotation
    } else {
        let residual = yaw_rotation.inverse() * base_rotation;
        let (mut pitch_x, _residual_yaw, mut roll_z) = residual.euler_angles();
        if snap_pitch {
            pitch_x = 0.0;
        }
        if snap_roll {
            roll_z = 0.0;
        }
        yaw_rotation * UnitQuaternion::from_euler_angles(pitch_x, 0.0, roll_z)
    };

    let (pitch_x, yaw_y, roll_z) = rotation.euler_angles();
    pose.rotation.pitch = pitch_x.to_degrees();
    pose.rotation.yaw = yaw_y.to_degrees();
    pose.rotation.roll = roll_z.to_degrees();
    pose.rotation.quaternion.x = rotation.i;
    pose.rotation.quaternion.y = rotation.j;
    pose.rotation.quaternion.z = rotation.k;
    pose.rotation.quaternion.w = rotation.w;
}

fn apply_temporal_pose_stabilization(profile: &LocalizationProfile, solver_results: &mut [LocalizationSolverResult]) {
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

    let state_store = SOLVER_TEMPORAL_STATE.get_or_init(|| Mutex::new(HashMap::new()));
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

fn localization_pose_components(pose: &crate::localization::types::LocalizationPose) -> Option<(nalgebra::Vector3<f64>, UnitQuaternion<f64>)> {
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

fn update_localization_pose(pose: &mut crate::localization::types::LocalizationPose, translation: nalgebra::Vector3<f64>, rotation: UnitQuaternion<f64>) {
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
struct LocalizationPoseSmoothingInput<'a> {
    settings: &'a LocalizationTemporalStabilizationConfig,
    runtime_tuning: &'a LocalizationSolverRuntimeTuningConfig,
    tag_count: usize,
    tag_ids: &'a [u32],
    solve_confidence: f64,
    now: Instant,
}

fn smooth_localization_pose(state_store: &mut HashMap<String, TemporalPoseState>, state_key: &str, pose: &mut crate::localization::types::LocalizationPose, input: LocalizationPoseSmoothingInput<'_>) {
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

fn solve_for_solver(
    registry: &SolverRegistry,
    solver: &LocalizationSolverConfig,
    samples: &[SourceSample],
    rig_poses: &HashMap<String, PoseTransform>,
    marker_map: Option<&MarkerMap>,
) -> LocalizationSolverResult {
    let requested_output_spaces = solver.output_spaces.clone();
    let requested_sources = solver.source_ids.iter().filter(|id| !id.trim().is_empty()).collect::<Vec<_>>();

    let mut scoped_samples = Vec::new();
    for sample in samples {
        if !requested_sources.is_empty() && !requested_sources.contains(&&sample.source.id) {
            continue;
        }
        scoped_samples.push(sample);
    }
    let output_spaces = filter_solver_output_spaces_for_scope(&requested_output_spaces, &scoped_samples);

    let Some(active_solver) = registry.solver_for(solver) else {
        return LocalizationSolverResult {
            id: solver.id.clone(),
            name: solver.name.clone(),
            mode: solver.mode,
            output_spaces: output_spaces.clone(),
            outputs: LocalizationSolverOutputs::default(),
            errors: vec![format!("unsupported solver mode: {:?}", solver.mode)],
        };
    };

    let outcome = active_solver.solve(SolverContext { solver_id: &solver.id, solver_config: solver, output_spaces: &output_spaces, samples: &scoped_samples, rig_poses, marker_map });

    LocalizationSolverResult { id: solver.id.clone(), name: solver.name.clone(), mode: solver.mode, output_spaces, outputs: outcome.outputs, errors: outcome.errors }
}

fn solver_scope_uses_multiple_cameras(samples: &[&SourceSample]) -> bool {
    let mut camera_uids = HashSet::new();
    for sample in samples {
        let camera_uid = sample.source.camera_uid.trim().to_ascii_lowercase();
        if camera_uid.is_empty() || camera_uid == "imu" {
            continue;
        }
        camera_uids.insert(camera_uid);
        if camera_uids.len() > 1 {
            return true;
        }
    }
    false
}

fn filter_solver_output_spaces_for_scope(requested: &[LocalizationPoseSpace], scoped_samples: &[&SourceSample]) -> Vec<LocalizationPoseSpace> {
    if !solver_scope_uses_multiple_cameras(scoped_samples) {
        return requested.to_vec();
    }
    requested.iter().copied().filter(|space| *space != LocalizationPoseSpace::CameraInField).collect()
}

fn marker_map_from_field_map(doc: &FieldMapDocument) -> MarkerMap {
    let prefer_heading_for_source = matches!(doc.source, FieldMapSource::LimelightFmap { .. });

    let markers = doc
        .markers
        .iter()
        .map(|marker| {
            let quaternion = marker.quaternion;
            let quaternion_is_finite = quaternion.x.is_finite() && quaternion.y.is_finite() && quaternion.z.is_finite() && quaternion.w.is_finite();
            let quaternion_is_identity = quaternion_is_finite && (quaternion.x.abs() + quaternion.y.abs() + quaternion.z.abs() <= 1e-9) && ((quaternion.w - 1.0).abs() <= 1e-9);

            let (rotation_from_quaternion, heading_ambiguous_from_quaternion) = if quaternion_is_finite {
                let norm_sq = quaternion.x * quaternion.x + quaternion.y * quaternion.y + quaternion.z * quaternion.z + quaternion.w * quaternion.w;
                if norm_sq > f64::EPSILON {
                    let q = UnitQuaternion::new_normalize(Quaternion::new(quaternion.w, quaternion.x, quaternion.y, quaternion.z));
                    let (roll, pitch, yaw) = q.euler_angles();

                    // `headingDeg` only captures horizontal heading of the tag normal.
                    // When the normal is close to vertical, heading becomes unstable and can
                    // collapse non-coplanar orientations into yaw-only results.
                    let normal = q.transform_vector(&nalgebra::Vector3::new(1.0, 0.0, 0.0));
                    let horizontal_norm = (normal.x * normal.x + normal.z * normal.z).sqrt();
                    (Some(Rotation3 { roll, pitch, yaw }), horizontal_norm < 0.20)
                } else {
                    (None, false)
                }
            } else {
                (None, false)
            };
            let heading_rad = marker.heading_deg.to_radians();
            let rotation_from_heading = if marker.heading_deg.is_finite() {
                // `headingDeg` encodes the horizontal heading of the tag normal, where identity
                // quaternion corresponds to heading=90deg. Convert heading to a Y-up yaw first.
                let yaw_y = heading_rad - std::f64::consts::FRAC_PI_2;
                let q = UnitQuaternion::from_axis_angle(&nalgebra::Vector3::y_axis(), yaw_y);
                let (roll, pitch, yaw) = q.euler_angles();
                Some(Rotation3 { roll, pitch, yaw })
            } else {
                None
            };

            // Limelight maps use heading for every marker, including exact zeros. For non-Limelight
            // maps, treat near-zero heading as "unset" unless quaternion is identity.
            let heading_has_signal = if prefer_heading_for_source { marker.heading_deg.is_finite() } else { marker.heading_deg.is_finite() && marker.heading_deg.abs() > 1e-6 };
            // Some maps contain mixed data: a subset of markers have calibrated quaternions while
            // others are left at identity with a meaningful heading. For identity quaternions, let
            // heading win to avoid 90deg side-of-tag errors on those markers.
            let prefer_heading = if prefer_heading_for_source { heading_has_signal && !heading_ambiguous_from_quaternion } else { heading_has_signal && quaternion_is_identity };
            let rotation = if prefer_heading { rotation_from_heading.or(rotation_from_quaternion) } else { rotation_from_quaternion.or(rotation_from_heading) };
            MarkerDefinition { id: marker.id, translation: Translation3 { x: marker.position[0], y: marker.position[1], z: marker.position[2] }, rotation }
        })
        .collect();
    MarkerMap { markers }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::localization::config::{LocalizationPoseSpace, LocalizationSolverConfig, LocalizationSolverMode, LocalizationSolverRuntimeTuningConfig, LocalizationTemporalStabilizationConfig};
    use crate::localization::types::{LocalizationPose, LocalizationQuaternion, LocalizationRotation, LocalizationSolverOutputs, LocalizationSolverPose, LocalizationSolverResult, LocalizationVector};

    fn dummy_pose(x: f64, y: f64, z: f64) -> LocalizationPose {
        LocalizationPose {
            translation: LocalizationVector { x, y, z },
            rotation: LocalizationRotation { roll: 0.0, pitch: 0.0, yaw: 0.0, quaternion: LocalizationQuaternion { x: 0.0, y: 0.0, z: 0.0, w: 1.0 } },
        }
    }

    fn clear_temporal_state() {
        if let Some(store) = SOLVER_TEMPORAL_STATE.get() {
            if let Ok(mut map) = store.lock() {
                map.clear();
            }
        }
    }

    fn quat_from_frontend_xyz_degrees(pitch_deg: f64, yaw_deg: f64, roll_deg: f64) -> UnitQuaternion<f64> {
        let qx = UnitQuaternion::from_axis_angle(&nalgebra::Vector3::x_axis(), pitch_deg.to_radians());
        let qy = UnitQuaternion::from_axis_angle(&nalgebra::Vector3::y_axis(), yaw_deg.to_radians());
        let qz = UnitQuaternion::from_axis_angle(&nalgebra::Vector3::z_axis(), roll_deg.to_radians());
        qx * qy * qz
    }

    fn heading_yaw_from_forward(rotation: UnitQuaternion<f64>) -> f64 {
        let forward = rotation.transform_vector(&nalgebra::Vector3::new(0.0, 0.0, -1.0));
        (-forward.x).atan2(-forward.z)
    }

    fn angle_diff_rad(a: f64, b: f64) -> f64 {
        let two_pi = 2.0 * std::f64::consts::PI;
        let mut d = (a - b).rem_euclid(two_pi);
        if d > std::f64::consts::PI {
            d -= two_pi;
        }
        d.abs()
    }

    fn source_sample_with_tag_ids(tag_ids: &[u32]) -> SourceSample {
        SourceSample {
            source: LocalizationSourceConfig {
                id: "src0".to_string(),
                stream_id: "stream0".to_string(),
                output_key: "tag_poses".to_string(),
                camera_uid: "cam0".to_string(),
                pose_space: None,
                input_key: None,
                enabled: true,
                weight: 1.0,
            },
            detections: tag_ids
                .iter()
                .copied()
                .map(|tag_id| crate::localization::sources::LocalizationDetection {
                    source_id: "src0".to_string(),
                    camera_uid: "cam0".to_string(),
                    tag_id,
                    camera_from_tag: PoseTransform { translation: nalgebra::Vector3::zeros(), rotation: UnitQuaternion::identity() },
                    tag_size: None,
                    code_rotation: None,
                    tag_bits: None,
                    weight: 1.0,
                    quality: 1.0,
                })
                .collect(),
            pose: None,
            poll_ms: 0.0,
            tag_size: None,
            error: None,
        }
    }

    #[test]
    fn tag_filter_applies_allowed_and_excluded_lists() {
        let profile = LocalizationProfile {
            id: "p_filter".to_string(),
            name: "p".to_string(),
            tag_size_m: None,
            allowed_tag_ids: vec![1, 2, 3],
            excluded_tag_ids: vec![2],
            field_map_id: None,
            field_origin: crate::localization::config::LocalizationFieldOriginConfig::default(),
            snap_z_to_ground: false,
            snap_roll_to_ground: false,
            snap_pitch_to_ground: false,
            enabled: true,
            color: None,
            view_enabled: true,
            temporal_stabilization: crate::localization::config::LocalizationTemporalStabilizationConfig::default(),
            sources: vec![],
            solvers: vec![],
        };

        let mut samples = vec![source_sample_with_tag_ids(&[1, 2, 3, 4])];
        apply_profile_tag_filter(&profile, &mut samples);
        let retained = samples[0].detections.iter().map(|d| d.tag_id).collect::<Vec<_>>();
        assert_eq!(retained, vec![1, 3]);
    }

    #[test]
    fn snap_z_to_ground_clamps_field_height() {
        let profile = LocalizationProfile {
            id: "p_snap_z".to_string(),
            name: "p".to_string(),
            tag_size_m: None,
            allowed_tag_ids: vec![],
            excluded_tag_ids: vec![],
            field_map_id: None,
            field_origin: crate::localization::config::LocalizationFieldOriginConfig::default(),
            snap_z_to_ground: true,
            snap_roll_to_ground: false,
            snap_pitch_to_ground: false,
            enabled: true,
            color: None,
            view_enabled: true,
            temporal_stabilization: crate::localization::config::LocalizationTemporalStabilizationConfig::default(),
            sources: vec![],
            solvers: vec![],
        };

        let mut results = vec![LocalizationSolverResult {
            id: "s".to_string(),
            name: "s".to_string(),
            mode: LocalizationSolverMode::GroupSolve,
            output_spaces: vec![LocalizationPoseSpace::RobotInField],
            outputs: LocalizationSolverOutputs {
                robot_in_field: Some(LocalizationSolverPose { pose: dummy_pose(1.0, -0.5, 2.0), source_ids: vec![] }),
                camera_in_field: Some(vec![crate::localization::types::LocalizationSourcePose {
                    source_id: "src".to_string(),
                    camera_uid: "cam".to_string(),
                    weight: 1.0,
                    pose: dummy_pose(0.0, 3.0, 0.0),
                }]),
                ..Default::default()
            },
            errors: vec![],
        }];

        let rig_poses: HashMap<String, PoseTransform> = HashMap::new();
        apply_profile_postprocessing(&profile, &mut results, &rig_poses, None, false);

        let robot_y = results[0].outputs.robot_in_field.as_ref().unwrap().pose.translation.y;
        let cam_y = results[0].outputs.camera_in_field.as_ref().unwrap()[0].pose.translation.y;
        assert_eq!(robot_y, 0.0);
        assert_eq!(cam_y, 0.0);
    }

    #[test]
    fn snap_z_to_ground_disabled_is_noop() {
        let profile = LocalizationProfile {
            id: "p_no_snap_z".to_string(),
            name: "p".to_string(),
            tag_size_m: None,
            allowed_tag_ids: vec![],
            excluded_tag_ids: vec![],
            field_map_id: None,
            field_origin: crate::localization::config::LocalizationFieldOriginConfig::default(),
            snap_z_to_ground: false,
            snap_roll_to_ground: false,
            snap_pitch_to_ground: false,
            enabled: true,
            color: None,
            view_enabled: true,
            temporal_stabilization: crate::localization::config::LocalizationTemporalStabilizationConfig::default(),
            sources: vec![],
            solvers: vec![],
        };

        let mut results = vec![LocalizationSolverResult {
            id: "s".to_string(),
            name: "s".to_string(),
            mode: LocalizationSolverMode::GroupSolve,
            output_spaces: vec![LocalizationPoseSpace::RobotInField],
            outputs: LocalizationSolverOutputs { robot_in_field: Some(LocalizationSolverPose { pose: dummy_pose(1.0, -0.5, 2.0), source_ids: vec![] }), ..Default::default() },
            errors: vec![],
        }];

        let rig_poses: HashMap<String, PoseTransform> = HashMap::new();
        apply_profile_postprocessing(&profile, &mut results, &rig_poses, None, false);
        assert_eq!(results[0].outputs.robot_in_field.as_ref().unwrap().pose.translation.y, -0.5);
    }

    #[test]
    fn field_origin_blue_offsets_field_space_outputs() {
        use crate::localization::config::LocalizationFieldOriginConfig;
        use crate::localization::maps::{FieldMapDocument, FieldMapSource};

        let profile = LocalizationProfile {
            id: "p_origin_blue".to_string(),
            name: "p".to_string(),
            tag_size_m: None,
            allowed_tag_ids: vec![],
            excluded_tag_ids: vec![],
            field_map_id: Some("map".to_string()),
            field_origin: LocalizationFieldOriginConfig::default(),
            snap_z_to_ground: false,
            snap_roll_to_ground: false,
            snap_pitch_to_ground: false,
            enabled: true,
            color: None,
            view_enabled: true,
            temporal_stabilization: crate::localization::config::LocalizationTemporalStabilizationConfig { enabled: false, ..Default::default() },
            sources: vec![],
            solvers: vec![],
        };

        let field_map = FieldMapDocument {
            schema_version: 1,
            id: "map".to_string(),
            name: "map".to_string(),
            width_m: 8.046,
            depth_m: 16.520,
            markers: vec![],
            source: FieldMapSource::LimelightFmap { original_file_name: None, map_type: None },
            overlay: None,
        };

        let mut results = vec![LocalizationSolverResult {
            id: "s".to_string(),
            name: "s".to_string(),
            mode: LocalizationSolverMode::GroupSolve,
            output_spaces: vec![LocalizationPoseSpace::RobotInField],
            outputs: LocalizationSolverOutputs {
                robot_in_field: Some(LocalizationSolverPose {
                    pose: LocalizationPose {
                        translation: LocalizationVector { x: 0.377, y: 0.0, z: 2.684 },
                        rotation: LocalizationRotation { roll: 0.0, pitch: 0.0, yaw: 7.9, quaternion: LocalizationQuaternion { x: 0.0, y: 0.068843223, z: 0.0, w: 0.997627343 } },
                    },
                    source_ids: vec![],
                }),
                ..Default::default()
            },
            errors: vec![],
        }];

        let rig_poses: HashMap<String, PoseTransform> = HashMap::new();
        apply_profile_postprocessing(&profile, &mut results, &rig_poses, Some(&field_map), true);

        let pose = &results[0].outputs.robot_in_field.as_ref().unwrap().pose;
        assert!((pose.translation.x - 4.4).abs() < 1e-3);
        assert!((pose.translation.z - 10.944).abs() < 1e-3);
    }

    #[test]
    fn marker_map_from_field_map_converts_heading_to_viewer_yaw() {
        use crate::localization::maps::{FieldMapDocument, FieldMapMarker, FieldMapSource, FieldQuaternion};

        let doc = FieldMapDocument {
            schema_version: 1,
            id: "map".to_string(),
            name: "map".to_string(),
            width_m: 8.0,
            depth_m: 16.0,
            markers: vec![FieldMapMarker {
                id: 15,
                family: "36h11".to_string(),
                size_m: 0.165,
                position: [0.0, 0.0, 0.0],
                quaternion: FieldQuaternion { x: 0.0, y: 0.0, z: 0.0, w: 1.0 },
                heading_deg: 90.0,
                tag_bits: None,
                unique: true,
            }],
            source: FieldMapSource::LimelightFmap { original_file_name: None, map_type: None },
            overlay: None,
        };

        let map = marker_map_from_field_map(&doc);
        let rotation = map.markers[0].rotation.expect("rotation");
        let q = UnitQuaternion::from_euler_angles(rotation.roll, rotation.pitch, rotation.yaw);
        let normal = q.transform_vector(&nalgebra::Vector3::new(1.0, 0.0, 0.0));
        let heading = normal.x.atan2(normal.z).to_degrees();
        assert!((heading - 90.0).abs() < 1e-6);
    }

    #[test]
    fn marker_map_from_field_map_prefers_heading_for_limelight_maps() {
        use crate::localization::maps::{FieldMapDocument, FieldMapMarker, FieldMapSource, FieldQuaternion};

        let doc = FieldMapDocument {
            schema_version: 1,
            id: "map".to_string(),
            name: "map".to_string(),
            width_m: 8.0,
            depth_m: 16.0,
            markers: vec![
                FieldMapMarker {
                    id: 1,
                    family: "36h11".to_string(),
                    size_m: 0.165,
                    position: [0.0, 0.0, 0.0],
                    quaternion: FieldQuaternion { x: 0.0, y: 0.0, z: 0.0, w: 1.0 },
                    heading_deg: 0.0,
                    tag_bits: None,
                    unique: true,
                },
                FieldMapMarker {
                    id: 2,
                    family: "36h11".to_string(),
                    size_m: 0.165,
                    position: [1.0, 0.0, 0.0],
                    quaternion: FieldQuaternion { x: 0.0, y: std::f64::consts::FRAC_1_SQRT_2, z: 0.0, w: std::f64::consts::FRAC_1_SQRT_2 },
                    heading_deg: 180.0,
                    tag_bits: None,
                    unique: true,
                },
            ],
            source: FieldMapSource::LimelightFmap { original_file_name: None, map_type: None },
            overlay: None,
        };

        let map = marker_map_from_field_map(&doc);
        let rotation = map.markers[1].rotation.expect("rotation");
        let q = UnitQuaternion::from_euler_angles(rotation.roll, rotation.pitch, rotation.yaw);
        let normal = q.transform_vector(&nalgebra::Vector3::new(1.0, 0.0, 0.0));
        let heading = normal.x.atan2(normal.z).to_degrees();
        assert!((heading - 180.0).abs() < 1e-6, "limelight maps should use heading orientation");
    }

    #[test]
    fn marker_map_from_field_map_preserves_tilted_quaternion_for_limelight_maps() {
        use crate::localization::maps::{FieldMapDocument, FieldMapMarker, FieldMapSource, FieldQuaternion};

        let doc = FieldMapDocument {
            schema_version: 1,
            id: "map".to_string(),
            name: "map".to_string(),
            width_m: 8.0,
            depth_m: 16.0,
            markers: vec![FieldMapMarker {
                id: 3,
                family: "36h11".to_string(),
                size_m: 0.165,
                position: [0.0, 0.0, 0.0],
                quaternion: FieldQuaternion { x: 0.0, y: 0.0, z: std::f64::consts::FRAC_1_SQRT_2, w: std::f64::consts::FRAC_1_SQRT_2 },
                heading_deg: 0.0,
                tag_bits: None,
                unique: true,
            }],
            source: FieldMapSource::LimelightFmap { original_file_name: None, map_type: None },
            overlay: None,
        };

        let map = marker_map_from_field_map(&doc);
        let rotation = map.markers[0].rotation.expect("rotation");
        let q = UnitQuaternion::from_euler_angles(rotation.roll, rotation.pitch, rotation.yaw);
        let normal = q.transform_vector(&nalgebra::Vector3::new(1.0, 0.0, 0.0));
        let horizontal_norm = (normal.x * normal.x + normal.z * normal.z).sqrt();
        assert!(horizontal_norm < 0.20, "tilted Limelight tags should keep quaternion tilt instead of yaw-only heading");
    }

    #[test]
    fn marker_map_from_field_map_uses_heading_for_identity_quat_in_mixed_maps() {
        use crate::localization::maps::{FieldMapDocument, FieldMapMarker, FieldMapSource, FieldQuaternion};

        let doc = FieldMapDocument {
            schema_version: 1,
            id: "map".to_string(),
            name: "map".to_string(),
            width_m: 8.0,
            depth_m: 16.0,
            markers: vec![
                FieldMapMarker {
                    id: 9,
                    family: "36h11".to_string(),
                    size_m: 0.165,
                    position: [0.0, 0.0, 0.0],
                    quaternion: FieldQuaternion { x: 0.0, y: 0.0, z: 0.0, w: 1.0 },
                    heading_deg: 90.0,
                    tag_bits: None,
                    unique: true,
                },
                FieldMapMarker {
                    id: 10,
                    family: "36h11".to_string(),
                    size_m: 0.165,
                    position: [1.0, 0.0, 0.0],
                    quaternion: FieldQuaternion { x: 0.0, y: std::f64::consts::FRAC_1_SQRT_2, z: 0.0, w: std::f64::consts::FRAC_1_SQRT_2 },
                    heading_deg: 180.0,
                    tag_bits: None,
                    unique: true,
                },
            ],
            source: FieldMapSource::LimelightFmap { original_file_name: None, map_type: None },
            overlay: None,
        };

        let map = marker_map_from_field_map(&doc);
        let rotation = map.markers[0].rotation.expect("rotation");
        let q = UnitQuaternion::from_euler_angles(rotation.roll, rotation.pitch, rotation.yaw);
        let normal = q.transform_vector(&nalgebra::Vector3::new(1.0, 0.0, 0.0));
        let heading = normal.x.atan2(normal.z).to_degrees();
        assert!((heading - 90.0).abs() < 1e-6, "identity marker in mixed map should follow heading");
    }

    #[test]
    fn snap_roll_and_pitch_to_ground_levels_field_rotation() {
        let profile = LocalizationProfile {
            id: "p_snap_rp".to_string(),
            name: "p".to_string(),
            tag_size_m: None,
            allowed_tag_ids: vec![],
            excluded_tag_ids: vec![],
            field_map_id: None,
            field_origin: crate::localization::config::LocalizationFieldOriginConfig::default(),
            snap_z_to_ground: false,
            snap_roll_to_ground: true,
            snap_pitch_to_ground: true,
            enabled: true,
            color: None,
            view_enabled: true,
            temporal_stabilization: crate::localization::config::LocalizationTemporalStabilizationConfig::default(),
            sources: vec![],
            solvers: vec![],
        };

        let input_rotation = quat_from_frontend_xyz_degrees(-7.0, 31.0, 15.0);
        let mut pose = dummy_pose(0.0, 0.0, 0.0);
        pose.rotation.roll = 0.0;
        pose.rotation.pitch = 0.0;
        pose.rotation.yaw = 0.0;
        pose.rotation.quaternion.x = input_rotation.i;
        pose.rotation.quaternion.y = input_rotation.j;
        pose.rotation.quaternion.z = input_rotation.k;
        pose.rotation.quaternion.w = input_rotation.w;

        let mut results = vec![LocalizationSolverResult {
            id: "s".to_string(),
            name: "s".to_string(),
            mode: LocalizationSolverMode::GroupSolve,
            output_spaces: vec![LocalizationPoseSpace::RobotInField],
            outputs: LocalizationSolverOutputs { robot_in_field: Some(LocalizationSolverPose { pose, source_ids: vec![] }), ..Default::default() },
            errors: vec![],
        }];

        let rig_poses: HashMap<String, PoseTransform> = HashMap::new();
        apply_profile_postprocessing(&profile, &mut results, &rig_poses, None, false);

        let rotation = &results[0].outputs.robot_in_field.as_ref().unwrap().pose.rotation;
        let actual = &rotation.quaternion;
        let actual_q = UnitQuaternion::new_normalize(Quaternion::new(actual.w, actual.x, actual.y, actual.z));
        let input_heading = heading_yaw_from_forward(input_rotation);
        let output_heading = heading_yaw_from_forward(actual_q);
        assert!(angle_diff_rad(output_heading, input_heading) < 1e-9);
        let up = actual_q.transform_vector(&nalgebra::Vector3::new(0.0, 1.0, 0.0));
        assert!(up.y > 0.999_999);
    }

    #[test]
    fn snap_pitch_only_preserves_yaw_and_roll() {
        let profile = LocalizationProfile {
            id: "p_snap_pitch".to_string(),
            name: "p".to_string(),
            tag_size_m: None,
            allowed_tag_ids: vec![],
            excluded_tag_ids: vec![],
            field_map_id: None,
            field_origin: crate::localization::config::LocalizationFieldOriginConfig::default(),
            snap_z_to_ground: false,
            snap_roll_to_ground: false,
            snap_pitch_to_ground: true,
            enabled: true,
            color: None,
            view_enabled: true,
            temporal_stabilization: crate::localization::config::LocalizationTemporalStabilizationConfig::default(),
            sources: vec![],
            solvers: vec![],
        };

        let input_rotation = quat_from_frontend_xyz_degrees(12.0, 170.0, -18.0);
        let mut pose = dummy_pose(0.0, 0.0, 0.0);
        pose.rotation.quaternion.x = input_rotation.i;
        pose.rotation.quaternion.y = input_rotation.j;
        pose.rotation.quaternion.z = input_rotation.k;
        pose.rotation.quaternion.w = input_rotation.w;

        let mut results = vec![LocalizationSolverResult {
            id: "s".to_string(),
            name: "s".to_string(),
            mode: LocalizationSolverMode::GroupSolve,
            output_spaces: vec![LocalizationPoseSpace::RobotInField],
            outputs: LocalizationSolverOutputs { robot_in_field: Some(LocalizationSolverPose { pose, source_ids: vec![] }), ..Default::default() },
            errors: vec![],
        }];

        let rig_poses: HashMap<String, PoseTransform> = HashMap::new();
        apply_profile_postprocessing(&profile, &mut results, &rig_poses, None, false);

        let actual = &results[0].outputs.robot_in_field.as_ref().unwrap().pose.rotation.quaternion;
        let actual_q = UnitQuaternion::new_normalize(Quaternion::new(actual.w, actual.x, actual.y, actual.z));
        let input_heading = heading_yaw_from_forward(input_rotation);
        let output_heading = heading_yaw_from_forward(actual_q);
        assert!(angle_diff_rad(output_heading, input_heading) < 1e-9);

        let yaw_rotation = UnitQuaternion::from_axis_angle(&nalgebra::Vector3::y_axis(), output_heading);
        let residual_in = yaw_rotation.inverse() * input_rotation;
        let residual_out = yaw_rotation.inverse() * actual_q;
        let (_in_pitch_x, _in_yaw, in_roll_z) = residual_in.euler_angles();
        let (out_pitch_x, _out_yaw, out_roll_z) = residual_out.euler_angles();
        assert!(out_pitch_x.abs() < 1e-9);
        assert!(angle_diff_rad(out_roll_z, in_roll_z) < 1e-9);
    }

    #[test]
    fn snap_roll_only_preserves_heading_and_pitch() {
        let profile = LocalizationProfile {
            id: "p_snap_roll".to_string(),
            name: "p".to_string(),
            tag_size_m: None,
            allowed_tag_ids: vec![],
            excluded_tag_ids: vec![],
            field_map_id: None,
            field_origin: crate::localization::config::LocalizationFieldOriginConfig::default(),
            snap_z_to_ground: false,
            snap_roll_to_ground: true,
            snap_pitch_to_ground: false,
            enabled: true,
            color: None,
            view_enabled: true,
            temporal_stabilization: crate::localization::config::LocalizationTemporalStabilizationConfig::default(),
            sources: vec![],
            solvers: vec![],
        };

        let input_rotation = quat_from_frontend_xyz_degrees(14.0, -155.0, 26.0);
        let mut pose = dummy_pose(0.0, 0.0, 0.0);
        pose.rotation.quaternion.x = input_rotation.i;
        pose.rotation.quaternion.y = input_rotation.j;
        pose.rotation.quaternion.z = input_rotation.k;
        pose.rotation.quaternion.w = input_rotation.w;

        let mut results = vec![LocalizationSolverResult {
            id: "s".to_string(),
            name: "s".to_string(),
            mode: LocalizationSolverMode::GroupSolve,
            output_spaces: vec![LocalizationPoseSpace::RobotInField],
            outputs: LocalizationSolverOutputs { robot_in_field: Some(LocalizationSolverPose { pose, source_ids: vec![] }), ..Default::default() },
            errors: vec![],
        }];

        let rig_poses: HashMap<String, PoseTransform> = HashMap::new();
        apply_profile_postprocessing(&profile, &mut results, &rig_poses, None, false);

        let actual = &results[0].outputs.robot_in_field.as_ref().unwrap().pose.rotation.quaternion;
        let actual_q = UnitQuaternion::new_normalize(Quaternion::new(actual.w, actual.x, actual.y, actual.z));
        let input_heading = heading_yaw_from_forward(input_rotation);
        let output_heading = heading_yaw_from_forward(actual_q);
        assert!(angle_diff_rad(output_heading, input_heading) < 1e-9);

        let yaw_rotation = UnitQuaternion::from_axis_angle(&nalgebra::Vector3::y_axis(), output_heading);
        let residual_in = yaw_rotation.inverse() * input_rotation;
        let residual_out = yaw_rotation.inverse() * actual_q;
        let (in_pitch_x, _in_yaw, _in_roll_z) = residual_in.euler_angles();
        let (out_pitch_x, _out_yaw, out_roll_z) = residual_out.euler_angles();
        assert!(angle_diff_rad(out_pitch_x, in_pitch_x) < 1e-9);
        assert!(out_roll_z.abs() < 1e-9);
    }

    #[test]
    fn snap_roll_and_pitch_near_180_stays_upright() {
        let profile = LocalizationProfile {
            id: "p_snap_180".to_string(),
            name: "p".to_string(),
            tag_size_m: None,
            allowed_tag_ids: vec![],
            excluded_tag_ids: vec![],
            field_map_id: None,
            field_origin: crate::localization::config::LocalizationFieldOriginConfig::default(),
            snap_z_to_ground: false,
            snap_roll_to_ground: true,
            snap_pitch_to_ground: true,
            enabled: true,
            color: None,
            view_enabled: true,
            temporal_stabilization: crate::localization::config::LocalizationTemporalStabilizationConfig::default(),
            sources: vec![],
            solvers: vec![],
        };

        let input_rotation = quat_from_frontend_xyz_degrees(20.0, 179.0, -10.0);
        let mut pose = dummy_pose(0.0, 0.0, 0.0);
        pose.rotation.quaternion.x = input_rotation.i;
        pose.rotation.quaternion.y = input_rotation.j;
        pose.rotation.quaternion.z = input_rotation.k;
        pose.rotation.quaternion.w = input_rotation.w;

        let mut results = vec![LocalizationSolverResult {
            id: "s".to_string(),
            name: "s".to_string(),
            mode: LocalizationSolverMode::GroupSolve,
            output_spaces: vec![LocalizationPoseSpace::RobotInField],
            outputs: LocalizationSolverOutputs { robot_in_field: Some(LocalizationSolverPose { pose, source_ids: vec![] }), ..Default::default() },
            errors: vec![],
        }];

        let rig_poses: HashMap<String, PoseTransform> = HashMap::new();
        apply_profile_postprocessing(&profile, &mut results, &rig_poses, None, false);

        let actual = &results[0].outputs.robot_in_field.as_ref().unwrap().pose.rotation.quaternion;
        let actual_q = UnitQuaternion::new_normalize(Quaternion::new(actual.w, actual.x, actual.y, actual.z));
        let input_heading = heading_yaw_from_forward(input_rotation);
        let output_heading = heading_yaw_from_forward(actual_q);
        assert!(angle_diff_rad(output_heading, input_heading) < 1e-9);
        let up = actual_q.transform_vector(&nalgebra::Vector3::new(0.0, 1.0, 0.0));
        assert!(up.y > 0.999_999);
    }

    #[test]
    fn camera_pose_tracks_robot_pose_via_rig_transform() {
        let profile = LocalizationProfile {
            id: "p_cam_consistency".to_string(),
            name: "p".to_string(),
            tag_size_m: None,
            allowed_tag_ids: vec![],
            excluded_tag_ids: vec![],
            field_map_id: None,
            field_origin: crate::localization::config::LocalizationFieldOriginConfig::default(),
            snap_z_to_ground: false,
            snap_roll_to_ground: false,
            snap_pitch_to_ground: false,
            enabled: true,
            color: None,
            view_enabled: true,
            temporal_stabilization: crate::localization::config::LocalizationTemporalStabilizationConfig::default(),
            sources: vec![],
            solvers: vec![],
        };

        let mut robot = dummy_pose(1.0, 0.5, -2.0);
        robot.rotation.yaw = 90.0;
        let q = UnitQuaternion::from_euler_angles(0.0, 0.0, std::f64::consts::FRAC_PI_2);
        robot.rotation.quaternion.x = q.i;
        robot.rotation.quaternion.y = q.j;
        robot.rotation.quaternion.z = q.k;
        robot.rotation.quaternion.w = q.w;

        let camera = dummy_pose(-10.0, -10.0, -10.0);
        let mut results = vec![LocalizationSolverResult {
            id: "s".to_string(),
            name: "s".to_string(),
            mode: LocalizationSolverMode::GroupSolve,
            output_spaces: vec![LocalizationPoseSpace::RobotInField, LocalizationPoseSpace::CameraInField],
            outputs: LocalizationSolverOutputs {
                robot_in_field: Some(LocalizationSolverPose { pose: robot, source_ids: vec![] }),
                camera_in_field: Some(vec![crate::localization::types::LocalizationSourcePose { source_id: "src".to_string(), camera_uid: "cam".to_string(), weight: 1.0, pose: camera }]),
                ..Default::default()
            },
            errors: vec![],
        }];

        let mut rig_poses: HashMap<String, PoseTransform> = HashMap::new();
        let robot_from_camera = PoseTransform { translation: nalgebra::Vector3::new(0.0, 0.3048, 0.0), rotation: UnitQuaternion::identity() };
        rig_poses.insert("cam".to_string(), robot_from_camera);

        apply_profile_postprocessing(&profile, &mut results, &rig_poses, None, false);

        let camera_pose = &results[0].outputs.camera_in_field.as_ref().unwrap()[0].pose;
        let expected = compose_transforms(
            &PoseTransform { translation: nalgebra::Vector3::new(1.0, 0.5, -2.0), rotation: UnitQuaternion::from_euler_angles(0.0, 0.0, std::f64::consts::FRAC_PI_2) },
            &robot_from_camera,
        );
        assert!((camera_pose.translation.x - expected.translation.x).abs() < 1e-6);
        assert!((camera_pose.translation.y - expected.translation.y).abs() < 1e-6);
        assert!((camera_pose.translation.z - expected.translation.z).abs() < 1e-6);
    }

    #[test]
    fn temporal_stabilization_smooths_robot_pose_jumps() {
        clear_temporal_state();
        let profile = LocalizationProfile {
            id: "p_temporal".to_string(),
            name: "p".to_string(),
            tag_size_m: None,
            allowed_tag_ids: vec![],
            excluded_tag_ids: vec![],
            field_map_id: None,
            field_origin: crate::localization::config::LocalizationFieldOriginConfig::default(),
            snap_z_to_ground: false,
            snap_roll_to_ground: false,
            snap_pitch_to_ground: false,
            enabled: true,
            color: None,
            view_enabled: true,
            temporal_stabilization: LocalizationTemporalStabilizationConfig {
                enabled: true,
                single_tag_translation_alpha: 0.2,
                single_tag_rotation_alpha: 0.2,
                multi_tag_translation_alpha: 0.5,
                multi_tag_rotation_alpha: 0.5,
                max_translation_jump_m: 3.0,
                max_rotation_jump_deg: 120.0,
                reanchor_reject_window_ms: 600,
            },
            sources: vec![],
            solvers: vec![LocalizationSolverConfig {
                id: "s".to_string(),
                name: "s".to_string(),
                mode: LocalizationSolverMode::GroupSolve,
                output_spaces: vec![LocalizationPoseSpace::RobotInField],
                source_ids: vec![],
                color: None,
                runtime_tuning: LocalizationSolverRuntimeTuningConfig::default(),
                temporal_stabilization: None,
            }],
        };

        let mut results = vec![LocalizationSolverResult {
            id: "s".to_string(),
            name: "s".to_string(),
            mode: LocalizationSolverMode::GroupSolve,
            output_spaces: vec![LocalizationPoseSpace::RobotInField],
            outputs: LocalizationSolverOutputs { robot_in_field: Some(LocalizationSolverPose { pose: dummy_pose(0.0, 0.0, 0.0), source_ids: vec![] }), ..Default::default() },
            errors: vec![],
        }];
        let rig_poses: HashMap<String, PoseTransform> = HashMap::new();

        apply_profile_postprocessing(&profile, &mut results, &rig_poses, None, false);
        results[0].outputs.robot_in_field.as_mut().unwrap().pose.translation.x = 1.0;
        apply_profile_postprocessing(&profile, &mut results, &rig_poses, None, false);

        let x = results[0].outputs.robot_in_field.as_ref().unwrap().pose.translation.x;
        assert!(x > 0.02 && x < 0.9, "expected smoothing to keep x between prior and measurement, got {x}");
        clear_temporal_state();
    }

    #[test]
    fn solver_temporal_override_can_disable_smoothing() {
        clear_temporal_state();
        let profile = LocalizationProfile {
            id: "p_temporal_override".to_string(),
            name: "p".to_string(),
            tag_size_m: None,
            allowed_tag_ids: vec![],
            excluded_tag_ids: vec![],
            field_map_id: None,
            field_origin: crate::localization::config::LocalizationFieldOriginConfig::default(),
            snap_z_to_ground: false,
            snap_roll_to_ground: false,
            snap_pitch_to_ground: false,
            enabled: true,
            color: None,
            view_enabled: true,
            temporal_stabilization: LocalizationTemporalStabilizationConfig {
                enabled: true,
                single_tag_translation_alpha: 0.1,
                single_tag_rotation_alpha: 0.1,
                multi_tag_translation_alpha: 0.3,
                multi_tag_rotation_alpha: 0.3,
                max_translation_jump_m: 3.0,
                max_rotation_jump_deg: 120.0,
                reanchor_reject_window_ms: 600,
            },
            sources: vec![],
            solvers: vec![LocalizationSolverConfig {
                id: "s".to_string(),
                name: "s".to_string(),
                mode: LocalizationSolverMode::GroupSolve,
                output_spaces: vec![LocalizationPoseSpace::RobotInField],
                source_ids: vec![],
                color: None,
                runtime_tuning: LocalizationSolverRuntimeTuningConfig::default(),
                temporal_stabilization: Some(LocalizationTemporalStabilizationConfig { enabled: false, ..LocalizationTemporalStabilizationConfig::default() }),
            }],
        };

        let mut results = vec![LocalizationSolverResult {
            id: "s".to_string(),
            name: "s".to_string(),
            mode: LocalizationSolverMode::GroupSolve,
            output_spaces: vec![LocalizationPoseSpace::RobotInField],
            outputs: LocalizationSolverOutputs { robot_in_field: Some(LocalizationSolverPose { pose: dummy_pose(0.0, 0.0, 0.0), source_ids: vec![] }), ..Default::default() },
            errors: vec![],
        }];
        let rig_poses: HashMap<String, PoseTransform> = HashMap::new();

        apply_profile_postprocessing(&profile, &mut results, &rig_poses, None, false);
        results[0].outputs.robot_in_field.as_mut().unwrap().pose.translation.x = 1.0;
        apply_profile_postprocessing(&profile, &mut results, &rig_poses, None, false);

        let x = results[0].outputs.robot_in_field.as_ref().unwrap().pose.translation.x;
        assert!((x - 1.0).abs() < 1e-9, "solver override disabled smoothing, expected raw measurement, got {x}");
        clear_temporal_state();
    }

    #[test]
    fn temporal_stabilization_damps_single_tag_switch_jump() {
        let mut state_store = HashMap::new();
        let settings = LocalizationTemporalStabilizationConfig {
            enabled: true,
            single_tag_translation_alpha: 0.2,
            single_tag_rotation_alpha: 0.2,
            multi_tag_translation_alpha: 0.5,
            multi_tag_rotation_alpha: 0.5,
            max_translation_jump_m: 1.2,
            max_rotation_jump_deg: 70.0,
            reanchor_reject_window_ms: 450,
        };
        let now = Instant::now();

        let mut first = dummy_pose(0.0, 0.0, 0.0);
        let runtime_tuning = LocalizationSolverRuntimeTuningConfig::default();
        smooth_localization_pose(
            &mut state_store,
            "k",
            &mut first,
            LocalizationPoseSmoothingInput { settings: &settings, runtime_tuning: &runtime_tuning, tag_count: 1, tag_ids: &[1], solve_confidence: 0.50, now },
        );

        let mut switched = dummy_pose(1.0, 0.0, 0.0);
        smooth_localization_pose(
            &mut state_store,
            "k",
            &mut switched,
            LocalizationPoseSmoothingInput { settings: &settings, runtime_tuning: &runtime_tuning, tag_count: 1, tag_ids: &[2], solve_confidence: 0.50, now: now + Duration::from_millis(33) },
        );

        assert!(switched.translation.x < 0.2, "single-tag switch should be strongly damped, got {}", switched.translation.x);
    }

    #[test]
    fn temporal_stabilization_does_not_ping_pong_on_alternating_single_tags() {
        let mut state_store = HashMap::new();
        let settings = LocalizationTemporalStabilizationConfig {
            enabled: true,
            single_tag_translation_alpha: 0.2,
            single_tag_rotation_alpha: 0.2,
            multi_tag_translation_alpha: 0.5,
            multi_tag_rotation_alpha: 0.5,
            max_translation_jump_m: 1.2,
            max_rotation_jump_deg: 70.0,
            reanchor_reject_window_ms: 450,
        };
        let now = Instant::now();

        let mut initial = dummy_pose(0.0, 0.0, 0.0);
        let runtime_tuning = LocalizationSolverRuntimeTuningConfig::default();
        smooth_localization_pose(
            &mut state_store,
            "k",
            &mut initial,
            LocalizationPoseSmoothingInput { settings: &settings, runtime_tuning: &runtime_tuning, tag_count: 1, tag_ids: &[1], solve_confidence: 0.50, now },
        );

        let mut latest = initial;
        for step in 1..=20 {
            let tag = if step % 2 == 0 { 1 } else { 2 };
            let x = if tag == 1 { -1.8 } else { 1.8 };
            latest = dummy_pose(x, 0.0, 0.0);
            smooth_localization_pose(
                &mut state_store,
                "k",
                &mut latest,
                LocalizationPoseSmoothingInput {
                    settings: &settings,
                    runtime_tuning: &runtime_tuning,
                    tag_count: 1,
                    tag_ids: &[tag],
                    solve_confidence: 0.50,
                    now: now + Duration::from_millis((step * 40) as u64),
                },
            );
        }

        assert!(latest.translation.x.abs() < 0.5, "alternating tags should not cause large pose ping-pong, got {}", latest.translation.x);
    }

    #[test]
    fn temporal_stabilization_reanchors_after_persistent_single_tag_switch() {
        let mut state_store = HashMap::new();
        let settings = LocalizationTemporalStabilizationConfig {
            enabled: true,
            single_tag_translation_alpha: 0.2,
            single_tag_rotation_alpha: 0.2,
            multi_tag_translation_alpha: 0.5,
            multi_tag_rotation_alpha: 0.5,
            max_translation_jump_m: 1.2,
            max_rotation_jump_deg: 70.0,
            reanchor_reject_window_ms: 450,
        };
        let now = Instant::now();

        let mut initial = dummy_pose(0.0, 0.0, 0.0);
        let runtime_tuning = LocalizationSolverRuntimeTuningConfig::default();
        smooth_localization_pose(
            &mut state_store,
            "k",
            &mut initial,
            LocalizationPoseSmoothingInput { settings: &settings, runtime_tuning: &runtime_tuning, tag_count: 1, tag_ids: &[1], solve_confidence: 0.50, now },
        );

        let mut latest = initial;
        for step in 1..=30 {
            latest = dummy_pose(1.6, 0.0, 0.0);
            smooth_localization_pose(
                &mut state_store,
                "k",
                &mut latest,
                LocalizationPoseSmoothingInput {
                    settings: &settings,
                    runtime_tuning: &runtime_tuning,
                    tag_count: 1,
                    tag_ids: &[2],
                    solve_confidence: 0.50,
                    now: now + Duration::from_millis((step * 50) as u64),
                },
            );
        }

        assert!(latest.translation.x > 1.0, "persistent single-tag regime should eventually re-anchor, got {}", latest.translation.x);
    }
}
