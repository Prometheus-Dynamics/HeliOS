use axum::{
    Json,
    extract::{Query, State},
};
use futures::future::join_all;
use nalgebra::{UnitQuaternion, Vector3};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::OnceLock;
use std::time::Instant;
use tokio::sync::RwLock;
use utoipa::ToSchema;
use uuid::Uuid;

use super::super::AppState;
use super::super::error::{ApiError, ApiResult};
use super::super::streams_persist;
use super::config;
use super::maps;
use super::sources::ApiLocalizationSourceFetcher;

use helios_engine::ipc::{EngineEvent, LocalizationSolveRequest, LocalizationSolveSourceValue, RigPose as StreamRigPose, StreamCalibration, StreamSummary};
use helios_engine::localization::config::LocalizationSourceConfig;
use helios_engine::localization::config::select_profile;
use helios_engine::localization::fetch::LocalizationSourceFetcher;
use helios_engine::localization::fetch::imu_vec_to_viewer_frame;
use helios_engine::localization::maps::FieldMapDocument;
use helios_engine::localization::math::{PoseTransform, RigPose, RigRotation, RigTranslation, rig_pose_to_viewer_transform, transform_to_pose};
use helios_engine::localization::types::LocalizationSolveResponse;
use helios_peripherals::dto::SensorScope;

#[derive(Debug, Deserialize, ToSchema)]
pub(crate) struct LocalizationSolveQuery {
    // Accept both `profile_id` (legacy/cli/frontend) and `profileId` (camelCase) in query strings.
    // Keep the canonical field name `profile_id` so generated OpenAPI matches the docs.
    #[serde(default, rename = "profile_id", alias = "profileId")]
    profile_id: Option<String>,
    #[serde(default = "default_apply_field_origin", rename = "apply_field_origin", alias = "applyFieldOrigin")]
    apply_field_origin: bool,
    #[serde(default, rename = "field_poses_only", alias = "fieldPosesOnly")]
    field_poses_only: bool,
}

fn default_apply_field_origin() -> bool {
    true
}

const RAW_STREAM_PIPELINE_UUID: Uuid = Uuid::from_u128(0x00000000_0000_0000_0000_0000000000aa);

#[derive(Clone)]
struct CachedLocalizationSolveResponse {
    signature: Vec<u8>,
    response: LocalizationSolveResponse,
}

fn localization_solve_cache() -> &'static RwLock<HashMap<String, CachedLocalizationSolveResponse>> {
    static CACHE: OnceLock<RwLock<HashMap<String, CachedLocalizationSolveResponse>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

fn localization_solve_cache_key(profile_id: &str, apply_field_origin: bool) -> String {
    format!("{}|{}", profile_id.trim(), if apply_field_origin { "field" } else { "raw" })
}

fn localization_solve_signature(request: &LocalizationSolveRequest) -> Result<Vec<u8>, String> {
    serde_json::to_vec(request).map_err(|err| format!("failed to encode localization solve signature: {err}"))
}

#[utoipa::path(
    get,
    path = "/localization/solve",
    tag = "Localization",
    params(
        ("profile_id" = Option<String>, Query, description = "Profile id override"),
        ("apply_field_origin" = Option<bool>, Query, description = "Apply profile fieldOrigin transform to field-space outputs (default true)"),
        ("field_poses_only" = Option<bool>, Query, description = "Return only field-space solve poses, omitting raw detection-space outputs")
    ),
    responses((status = 200, description = "Localization solve outputs", body = LocalizationSolveResponse))
)]
pub async fn solve(State(state): State<AppState>, Query(query): Query<LocalizationSolveQuery>) -> ApiResult<Json<LocalizationSolveResponse>> {
    let config = config::load_config().await?;
    let profile = select_profile(&config, query.profile_id.as_deref()).map_err(ApiError::not_found)?;
    let mut sources = dedupe_enabled_sources(profile.sources.iter().filter(|source| source.enabled).cloned().collect::<Vec<_>>());
    let stream_summaries = state.engine.list_streams().await.unwrap_or_default();
    enrich_source_input_keys_from_streams(&stream_summaries, &mut sources);

    let mut rig_poses = load_rig_poses_from_streams(&sources, &stream_summaries).await;
    inject_imu_leveling_rig_pose(&state, profile, &mut rig_poses).await;
    let field_map = if let Some(map_id) = profile.field_map_id.as_deref() { maps::load_map_document(map_id).await.ok() } else { None };
    let fetcher = ApiLocalizationSourceFetcher::new(state.clone());
    let mut response = solve_via_engine(&state, profile, sources, &rig_poses, field_map.as_ref(), &stream_summaries, &fetcher, query.apply_field_origin).await.map_err(ApiError::bad_gateway)?;
    if query.field_poses_only {
        trim_localization_solve_response_to_field_poses(&mut response);
    }

    Ok(Json(response))
}

fn trim_localization_solve_response_to_field_poses(response: &mut LocalizationSolveResponse) {
    for solver in &mut response.solvers {
        solver.outputs.tag_in_camera = None;
        solver.outputs.camera_in_tag = None;
        solver.outputs.tag_in_robot = None;
        solver.outputs.robot_in_tag = None;
    }
}

pub(crate) async fn solve_via_engine(
    state: &AppState,
    profile: &helios_engine::localization::config::LocalizationProfile,
    sources: Vec<LocalizationSourceConfig>,
    rig_poses: &HashMap<String, PoseTransform>,
    field_map: Option<&helios_engine::localization::maps::FieldMapDocument>,
    stream_summaries: &[StreamSummary],
    fetcher: &ApiLocalizationSourceFetcher,
    apply_field_origin: bool,
) -> Result<LocalizationSolveResponse, String> {
    let request_started = Instant::now();
    let (request, source_fetch_ms) = build_localization_solve_request(state, profile, sources, rig_poses, field_map, stream_summaries, fetcher, apply_field_origin).await?;
    let cache_key = localization_solve_cache_key(&request.profile.id, request.apply_field_origin);
    let signature = localization_solve_signature(&request)?;
    if let Some(entry) = localization_solve_cache().read().await.get(&cache_key).cloned()
        && entry.signature == signature
    {
        let mut response = entry.response;
        response.timings.cache_hit = true;
        response.timings.source_fetch_ms = source_fetch_ms;
        response.timings.total_ms = request_started.elapsed().as_secs_f64() * 1000.0;
        return Ok(response);
    }
    match state.engine.solve_localization_event(request).await {
        Ok(EngineEvent::LocalizationSolved { response, .. }) => {
            let mut response: LocalizationSolveResponse = serde_json::from_value(response.into()).map_err(|err| format!("invalid localization solve response: {err}"))?;
            response.timings.cache_hit = false;
            response.timings.source_fetch_ms = source_fetch_ms;
            response.timings.total_ms = request_started.elapsed().as_secs_f64() * 1000.0;
            localization_solve_cache().write().await.insert(cache_key, CachedLocalizationSolveResponse { signature, response: response.clone() });
            Ok(response)
        }
        Ok(EngineEvent::Nack { reason, .. }) => Err(reason),
        Ok(other) => Err(format!("unexpected engine response: {other:?}")),
        Err(err) => Err(err.to_string()),
    }
}

async fn build_localization_solve_request(
    state: &AppState,
    profile: &helios_engine::localization::config::LocalizationProfile,
    sources: Vec<LocalizationSourceConfig>,
    rig_poses: &HashMap<String, PoseTransform>,
    field_map: Option<&helios_engine::localization::maps::FieldMapDocument>,
    stream_summaries: &[StreamSummary],
    fetcher: &ApiLocalizationSourceFetcher,
    apply_field_origin: bool,
) -> Result<(LocalizationSolveRequest, f64), String> {
    let source_fetch_started = Instant::now();
    let source_values = fetch_localization_source_values(fetcher, &sources).await;
    let source_fetch_ms = source_fetch_started.elapsed().as_secs_f64() * 1000.0;
    let calibrations = load_stream_calibrations_from_streams(stream_summaries).into_iter().collect::<BTreeMap<_, _>>();
    let rig_poses = rig_poses.iter().map(|(camera_uid, pose)| (camera_uid.clone(), transform_to_pose(pose))).collect::<BTreeMap<_, _>>();
    let field_map = strip_overlay_from_field_map(field_map);

    let request = LocalizationSolveRequest { profile: profile.clone(), sources, rig_poses, field_map, calibrations, source_values, apply_field_origin };
    let _ = state;
    Ok((request, source_fetch_ms))
}

fn strip_overlay_from_field_map(field_map: Option<&FieldMapDocument>) -> Option<FieldMapDocument> {
    let mut field_map = field_map.cloned()?;
    field_map.overlay = None;
    Some(field_map)
}

pub(crate) async fn fetch_localization_source_values(fetcher: &ApiLocalizationSourceFetcher, sources: &[LocalizationSourceConfig]) -> Vec<LocalizationSolveSourceValue> {
    join_all(sources.iter().map(|source| async move {
        match LocalizationSourceFetcher::fetch_source_value(fetcher, source).await {
            Ok(value) => LocalizationSolveSourceValue { source_id: source.id.clone(), value: Some(value.into()), error: None },
            Err(error) => LocalizationSolveSourceValue { source_id: source.id.clone(), value: None, error: Some(error) },
        }
    }))
    .await
}

pub(crate) fn dedupe_enabled_sources(sources: Vec<LocalizationSourceConfig>) -> Vec<LocalizationSourceConfig> {
    let mut seen = HashSet::<(String, String, String)>::new();
    let mut out = Vec::with_capacity(sources.len());
    for source in sources {
        let key = (source.stream_id.trim().to_ascii_lowercase(), source.output_key.trim().to_ascii_lowercase(), source.camera_uid.trim().to_ascii_lowercase());
        if !seen.insert(key) {
            continue;
        }
        out.push(source);
    }
    out
}

fn normalize_key(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_ascii_lowercase()) }
}

fn canonical_input_space_hint(raw: Option<&str>) -> Option<&'static str> {
    match normalize_key(raw).as_deref() {
        Some("undistorted") | Some("rectified") => Some("undistorted"),
        Some("raw") | Some("frame") | Some("distorted") => Some("raw"),
        _ => None,
    }
}

fn has_imu_token(value: &str) -> bool {
    value.split(|ch: char| !ch.is_ascii_alphanumeric()).any(|token| token.eq_ignore_ascii_case("imu"))
}

fn source_looks_like_imu(source: &LocalizationSourceConfig) -> bool {
    [source.id.as_str(), source.stream_id.as_str(), source.output_key.as_str(), source.camera_uid.as_str()].iter().any(|value| has_imu_token(value))
}

fn source_looks_like_imu_pose(source: &LocalizationSourceConfig) -> bool {
    if !source_looks_like_imu(source) {
        return false;
    }
    let key = source.output_key.trim().to_ascii_lowercase();
    key.contains("pose") || key.contains("orientation") || key.contains("quaternion") || key.contains("rotation")
}

fn infer_source_input_key(stream: &StreamSummary, source: &LocalizationSourceConfig) -> Option<String> {
    let manifest = &stream.manifest;
    let active_pipeline_id = manifest.active_pipeline_id.or_else(|| manifest.pipelines.first().map(|binding| binding.pipeline_id)).unwrap_or(RAW_STREAM_PIPELINE_UUID);
    let source_output_key = normalize_key(Some(source.output_key.as_str()));

    // If the stream is a raw-only pipeline binding, output key semantics are already the input
    // space semantics.
    if active_pipeline_id == RAW_STREAM_PIPELINE_UUID {
        return canonical_input_space_hint(Some(source.output_key.as_str())).map(str::to_string);
    }

    let mut default_port: Option<String> = None;
    for wire in &manifest.pipeline_wires {
        if wire.to.pipeline_id != active_pipeline_id {
            continue;
        }
        if normalize_key(wire.to.port.as_deref()).as_deref() != Some("frame") {
            continue;
        }
        let from_port = normalize_key(wire.from.port.as_deref());
        if from_port.is_none() {
            continue;
        }
        let wire_output_key = normalize_key(wire.to.output_key.as_deref());
        if let (Some(source_key), Some(wire_key)) = (source_output_key.as_ref(), wire_output_key.as_ref())
            && source_key == wire_key
        {
            return from_port;
        }
        if wire_output_key.is_none() && default_port.is_none() {
            default_port = from_port;
        }
    }

    default_port.or_else(|| canonical_input_space_hint(Some(source.output_key.as_str())).map(str::to_string))
}

pub(crate) fn enrich_source_input_keys_from_streams(streams: &[StreamSummary], sources: &mut [LocalizationSourceConfig]) {
    let needs_inference = sources.iter().any(|source| normalize_key(source.input_key.as_deref()).is_none());
    if !needs_inference {
        return;
    }
    let streams_by_id: HashMap<Uuid, StreamSummary> = streams.iter().cloned().map(|stream| (stream.stream_id, stream)).collect();

    for source in sources {
        if normalize_key(source.input_key.as_deref()).is_some() {
            continue;
        }
        let Ok(stream_id) = Uuid::parse_str(source.stream_id.trim()) else {
            source.input_key = canonical_input_space_hint(Some(source.output_key.as_str())).map(str::to_string);
            continue;
        };
        let inferred =
            streams_by_id.get(&stream_id).and_then(|stream| infer_source_input_key(stream, source)).or_else(|| canonical_input_space_hint(Some(source.output_key.as_str())).map(str::to_string));
        source.input_key = inferred;
    }
}

pub(crate) async fn load_rig_poses_from_streams(sources: &[LocalizationSourceConfig], stream_summaries: &[StreamSummary]) -> HashMap<String, PoseTransform> {
    let mut out: HashMap<String, PoseTransform> = streams_persist::list_pose_map().await.into_iter().map(|(uid, pose)| (uid, rig_pose_to_viewer_transform(&map_rig_pose(&pose)))).collect();

    // Backfill source camera_uids from their stream pose when `camera_uid` doesn't match the
    // persisted camera-id key (common for short aliases like `ov9782`).
    let running_by_stream: HashMap<Uuid, StreamRigPose> = stream_summaries.iter().filter_map(|stream| stream.manifest.pose.clone().map(|pose| (stream.stream_id, pose))).collect();

    for source in sources {
        let camera_uid = source.camera_uid.trim();
        if camera_uid.is_empty() || out.contains_key(camera_uid) {
            continue;
        }
        let Ok(stream_id) = Uuid::parse_str(source.stream_id.trim()) else {
            continue;
        };

        let stream_pose = if let Some(pose) = running_by_stream.get(&stream_id).cloned() { Some(pose) } else { streams_persist::pose_for_stream_id(stream_id).await };
        if let Some(pose) = stream_pose {
            out.insert(camera_uid.to_string(), rig_pose_to_viewer_transform(&map_rig_pose(&pose)));
        }
    }

    // Ensure every enabled source camera has at least an identity rig pose so localization
    // does not fail closed on new media/live streams without an explicit configured pose yet.
    for source in sources {
        let camera_uid = source.camera_uid.trim();
        if camera_uid.is_empty() || camera_uid.eq_ignore_ascii_case("imu") {
            continue;
        }
        out.entry(camera_uid.to_string()).or_insert_with(|| PoseTransform { translation: Vector3::new(0.0, 0.0, 0.0), rotation: UnitQuaternion::identity() });
    }

    out
}

pub(crate) async fn inject_imu_leveling_rig_pose(state: &AppState, profile: &helios_engine::localization::config::LocalizationProfile, rig_poses: &mut HashMap<String, PoseTransform>) {
    // Best-effort IMU leveling:
    // - Uses the device IMU accel vector (when available) to derive a rotation that aligns gravity
    //   with -Y in the viewer frame (+Y up, +Z forward).
    // - Injects a "robot_from_camera" rig pose for any enabled localization sources in this
    //   profile so tag poses can be expressed in a floor-level frame via `tag_in_robot`.
    //
    // NOTE: This assumes the IMU board axes are aligned with the camera body frame (common for
    // the integrated sensor rig). If a future rig needs a fixed IMU->camera transform, add it
    // as an explicit configuration term and compose it here.
    // Only apply IMU leveling when the profile explicitly includes an IMU source.
    // This keeps "stream only" profiles deterministic and avoids surprising users.
    let enabled_sources: Vec<&LocalizationSourceConfig> = profile.sources.iter().filter(|source| source.enabled).collect();
    let has_imu_source = enabled_sources.iter().any(|source| source_looks_like_imu(source));
    if !has_imu_source {
        return;
    }

    // If the profile already carries an explicit IMU pose/orientation source, do not also force
    // a camera rig leveling override from raw accel. Doing both applies two independent IMU
    // rotations and causes yaw/roll/pitch frame drift.
    let has_explicit_imu_pose_source = enabled_sources.iter().any(|source| source_looks_like_imu_pose(source));
    if has_explicit_imu_pose_source {
        return;
    }

    let mut camera_uid_set = BTreeSet::<String>::new();
    for imu_source in enabled_sources.iter().copied().filter(|source| source_looks_like_imu(source)) {
        let imu_camera_uid = imu_source.camera_uid.trim();
        if !imu_camera_uid.is_empty() && !imu_camera_uid.eq_ignore_ascii_case("imu") && !imu_camera_uid.starts_with("peer:") {
            camera_uid_set.insert(imu_camera_uid.to_string());
            continue;
        }

        let imu_stream_id = imu_source.stream_id.trim();
        if imu_stream_id.is_empty() {
            continue;
        }
        for source in enabled_sources.iter().copied().filter(|source| !source_looks_like_imu(source)) {
            if source.stream_id.trim() != imu_stream_id {
                continue;
            }
            let camera_uid = source.camera_uid.trim();
            if camera_uid.is_empty() || camera_uid.eq_ignore_ascii_case("imu") || camera_uid.starts_with("peer:") {
                continue;
            }
            camera_uid_set.insert(camera_uid.to_string());
        }
    }

    // If the IMU source is global and only one non-IMU camera is active, bind leveling to that one.
    if camera_uid_set.is_empty() {
        let non_imu_camera_uids = enabled_sources
            .iter()
            .copied()
            .filter(|source| !source_looks_like_imu(source))
            .filter_map(|source| {
                let camera_uid = source.camera_uid.trim();
                if camera_uid.is_empty() || camera_uid.eq_ignore_ascii_case("imu") || camera_uid.starts_with("peer:") {
                    return None;
                }
                Some(camera_uid.to_string())
            })
            .collect::<BTreeSet<_>>();
        if non_imu_camera_uids.len() == 1 {
            camera_uid_set = non_imu_camera_uids;
        }
    }

    let camera_uids: Vec<String> = camera_uid_set.into_iter().collect();
    if camera_uids.is_empty() {
        return;
    }

    let Some(peripherals) = state.ensure_sensors().await else {
        return;
    };
    let snapshot = match peripherals.sensor_snapshot(SensorScope::Device).await {
        Ok(Ok(values)) => values,
        _ => return,
    };
    let status = crate::http::device::imu::imu_status_from_snapshot(&snapshot);
    let Some(accel) = status.accel else {
        return;
    };

    let g_imu = Vector3::new(accel.x, accel.y, accel.z);
    let norm = g_imu.norm();
    if !norm.is_finite() || norm <= f64::EPSILON {
        return;
    }
    // Expect ~1g when stationary; be tolerant since IMU may include some motion.
    if !(0.3..=2.0).contains(&norm) {
        return;
    }
    let g_imu = g_imu / norm;

    // Use the same canonical backend->viewer IMU basis as localization source parsing.
    let mut g_view = imu_vec_to_viewer_frame(g_imu);
    let target = Vector3::new(0.0, -1.0, 0.0); // gravity down in viewer
    if g_view.dot(&target) < 0.0 {
        // Pick the direction closer to target in case the accel sign convention is flipped.
        g_view = -g_view;
    }

    // `rig_poses` are interpreted as `robot_from_camera`. For leveling, we want a rotation that
    // maps the camera-observed gravity direction (`g_view`) onto the leveled gravity direction
    // (`target`) so that composing `robot_from_camera * camera_from_tag` removes pitch/roll.
    //
    // `rotation_between(a, b)` returns a rotation `R` such that `R * a = b`.
    let Some(level_rot) = UnitQuaternion::rotation_between(&g_view, &target) else {
        return;
    };

    for uid in camera_uids {
        rig_poses
            .entry(uid)
            .and_modify(|pose| {
                // Preserve the configured camera yaw (heading) while replacing pitch/roll with IMU leveling.
                //
                // `level_rot` aligns gravity but leaves yaw underconstrained. If the user has already
                // configured an approximate yaw in the rig pose, keep it by extracting a yaw-only rotation
                // from the existing pose and composing it with the leveling rotation.
                let forward = pose.rotation.transform_vector(&Vector3::new(0.0, 0.0, 1.0));
                let mut forward_xz = Vector3::new(forward.x, 0.0, forward.z);
                let yaw_rot = if forward_xz.norm_squared() > 1e-12 {
                    forward_xz = forward_xz.normalize();
                    // Yaw angle that rotates +Z onto the projected forward vector.
                    let yaw = forward_xz.x.atan2(forward_xz.z);
                    UnitQuaternion::from_axis_angle(&Vector3::y_axis(), yaw)
                } else {
                    UnitQuaternion::identity()
                };

                pose.rotation = yaw_rot * level_rot;
                // Keep translation as-is; it is already expressed in the robot frame.
            })
            .or_insert_with(|| PoseTransform { translation: Vector3::new(0.0, 0.0, 0.0), rotation: level_rot });
    }
}

pub(crate) fn load_stream_calibrations_from_streams(streams: &[StreamSummary]) -> HashMap<String, StreamCalibration> {
    let mut out = HashMap::new();
    for stream in streams {
        let Some(calib) = stream.manifest.calibration.as_ref() else {
            continue;
        };
        out.insert(stream.stream_id.to_string(), calib.clone());
    }
    out
}

fn map_rig_pose(pose: &StreamRigPose) -> RigPose {
    RigPose {
        translation: RigTranslation { x: pose.translation.x, y: pose.translation.y, z: pose.translation.z },
        rotation: RigRotation { roll: pose.rotation.roll, pitch: pose.rotation.pitch, yaw: pose.rotation.yaw },
    }
}

#[cfg(test)]
mod tests {
    use super::{strip_overlay_from_field_map, trim_localization_solve_response_to_field_poses};
    use helios_engine::localization::config::LocalizationSolverMode;
    use helios_engine::localization::maps::{FieldMapDocument, FieldMapOverlay, FieldMapSource};
    use helios_engine::localization::types::{
        LocalizationDetectionPose, LocalizationPose, LocalizationQuaternion, LocalizationRotation, LocalizationSolveResponse, LocalizationSolveTimings, LocalizationSolverOutputs,
        LocalizationSolverPose, LocalizationSolverResult, LocalizationSourcePose, LocalizationVector,
    };

    fn sample_pose() -> LocalizationPose {
        LocalizationPose {
            translation: LocalizationVector { x: 1.0, y: 2.0, z: 3.0 },
            rotation: LocalizationRotation { roll: 4.0, pitch: 5.0, yaw: 6.0, quaternion: LocalizationQuaternion { x: 0.0, y: 0.0, z: 0.0, w: 1.0 } },
        }
    }

    #[test]
    fn strip_overlay_from_field_map_removes_embedded_image_data() {
        let field_map = FieldMapDocument {
            schema_version: 1,
            id: "test".to_string(),
            name: "Test".to_string(),
            width_m: 1.0,
            depth_m: 1.0,
            markers: Vec::new(),
            source: FieldMapSource::LimelightFmap { original_file_name: None, map_type: None },
            overlay: Some(FieldMapOverlay {
                data_url: "data:image/png;base64,AAAA".to_string(),
                mime_type: Some("image/png".to_string()),
                opacity: Some(1.0),
                width_m: Some(1.0),
                depth_m: Some(1.0),
                offset_x_m: Some(0.0),
                offset_z_m: Some(0.0),
                rotation_deg: Some(0.0),
            }),
        };

        let stripped = strip_overlay_from_field_map(Some(&field_map)).expect("stripped field map");

        assert!(stripped.overlay.is_none());
        assert_eq!(stripped.markers.len(), field_map.markers.len());
        assert_eq!(stripped.id, field_map.id);
        assert_eq!(stripped.name, field_map.name);
    }

    #[test]
    fn field_poses_only_trim_drops_detection_outputs_but_keeps_field_outputs() {
        let detection = LocalizationDetectionPose {
            source_id: "src".to_string(),
            camera_uid: "cam".to_string(),
            tag_id: 3,
            pose: sample_pose(),
            weight: 1.0,
            quality: 1.0,
            tag_size: Some(0.165),
            code_rotation: Some(0),
            tag_bits: Some(lib_cv::modules::aruco::ArucoBitGrid { width: 6, border: 1, rows: vec!["010101".to_string(); 6] }),
        };
        let mut response = LocalizationSolveResponse {
            profile_id: "profile".to_string(),
            solvers: vec![LocalizationSolverResult {
                id: "solver".to_string(),
                name: "Solver".to_string(),
                mode: LocalizationSolverMode::GroupSolve,
                output_spaces: Vec::new(),
                outputs: LocalizationSolverOutputs {
                    tag_in_camera: Some(vec![detection.clone()]),
                    camera_in_tag: Some(vec![detection.clone()]),
                    tag_in_robot: Some(vec![detection.clone()]),
                    robot_in_tag: Some(vec![detection]),
                    camera_in_field: Some(vec![LocalizationSourcePose { source_id: "src".to_string(), camera_uid: "cam".to_string(), weight: 1.0, pose: sample_pose() }]),
                    robot_in_field: Some(LocalizationSolverPose { pose: sample_pose(), source_ids: vec!["src".to_string()] }),
                },
                errors: Vec::new(),
            }],
            sources: Vec::new(),
            timings: LocalizationSolveTimings::default(),
        };

        trim_localization_solve_response_to_field_poses(&mut response);

        let outputs = &response.solvers[0].outputs;
        assert!(outputs.tag_in_camera.is_none());
        assert!(outputs.camera_in_tag.is_none());
        assert!(outputs.tag_in_robot.is_none());
        assert!(outputs.robot_in_tag.is_none());
        assert_eq!(outputs.camera_in_field.as_ref().map(Vec::len), Some(1));
        assert!(outputs.robot_in_field.is_some());
    }
}

// Re-export types for OpenAPI schema resolution.
