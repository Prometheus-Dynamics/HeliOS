use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde_json::Value as JsonValue;
use serde_json::json;
use std::collections::BTreeSet;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;

use super::super::AppState;
use super::super::error::{ApiError, ApiResult};
use super::super::streams::types::EngineErrorBody;
use super::super::streams::util::{engine_error_body, map_client_error};
use super::super::{peers, pipelines};
use super::config;
use super::external;
use super::maps;
use super::media_imu;
use super::peers as localization_peers;
use super::solve;

use helios_engine::ipc::{EngineErrorCode, EngineEvent};
use helios_engine::localization::config::{LocalizationConfig, LocalizationPoseSpace, LocalizationProfile, LocalizationSourceConfig, select_profile};
use helios_engine::localization::solve::solve_localization;
use helios_engine::localization::sources::LocalizationSourceFetcher;
use helios_engine::localization::types::{LocalizationDetectionPose, LocalizationPipelineSource, LocalizationSolverOutputs, PipelineOutputSample};

const IMU_EXTERNAL_ID: &str = "imu";
const PROFILE_STREAM_PREFIX: &str = "profile:";
const PROFILE_OUTPUT_PREFIX: &str = "solver:";

fn is_media_imu_output_key(output_key: &str) -> bool {
    output_key.eq_ignore_ascii_case(media_imu::MEDIA_IMU_OUTPUT_KEY) || output_key.eq_ignore_ascii_case(media_imu::MEDIA_IMU_OUTPUT_KEY_LEGACY)
}

#[derive(Clone)]
pub struct ApiLocalizationSourceFetcher {
    state: AppState,
    profile_resolve_stack: Arc<Mutex<Vec<String>>>,
}

impl ApiLocalizationSourceFetcher {
    pub fn new(state: AppState) -> Self {
        Self { state, profile_resolve_stack: Arc::new(Mutex::new(Vec::new())) }
    }
}

impl LocalizationSourceFetcher for ApiLocalizationSourceFetcher {
    fn fetch_source_value<'a>(&'a self, source: &'a LocalizationSourceConfig) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<JsonValue, String>> + Send + 'a>> {
        Box::pin(async move {
            if source.stream_id.starts_with("peer:") {
                fetch_peer_output(&source.stream_id, &source.output_key).await
            } else if let Some(source_id) = source.stream_id.strip_prefix("external:") {
                external::fetch_external_value(&self.state, source_id, &source.output_key).await
            } else if let Some(profile_id) = source.stream_id.strip_prefix(PROFILE_STREAM_PREFIX) {
                fetch_profile_output(self, profile_id, &source.output_key).await
            } else {
                fetch_stream_output(&self.state, &source.stream_id, &source.output_key).await
            }
        })
    }
}

#[utoipa::path(
    get,
    path = "/localization/sources",
    tag = "Localization",
    responses((status = 200, description = "Available localization pipeline outputs", body = [LocalizationPipelineSource]))
)]
pub async fn list_sources(State(state): State<AppState>) -> ApiResult<Json<Vec<LocalizationPipelineSource>>> {
    let streams = state.engine.list_streams().await.map_err(|err| ApiError::bad_gateway(err.to_string()))?;
    let media_meta_dir = crate::http::storage::ensure_subdir_async("media-meta").await.ok();

    let mut out: Vec<LocalizationPipelineSource> = Vec::new();
    for stream in streams {
        if stream.manifest.internal {
            continue;
        }
        if let Some(media_meta_dir) = media_meta_dir.as_deref()
            && let Some(media_imu_source) = media_imu::source_for_stream(&stream, media_meta_dir).await
        {
            out.push(media_imu_source);
        }

        let stream_id = stream.stream_id;
        let outputs = match state.engine.list_graph_outputs_event(stream_id).await {
            Ok(EngineEvent::GraphOutputs { outputs, .. }) => outputs,
            Ok(EngineEvent::Nack { .. }) => Vec::new(),
            Ok(_) => Vec::new(),
            Err(_) => Vec::new(),
        };
        let ports = outputs.into_iter().map(|desc| desc.name).filter(|port| !port.eq_ignore_ascii_case("frame"));

        let stream_label = stream.manifest.identity.alias.clone().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| stream_id.to_string());

        let camera_uid = stream.manifest.identity.hardware_id.clone().or(stream.manifest.identity.alias.clone()).unwrap_or_else(|| stream_id.to_string());

        let camera_path = match &stream.manifest.capture.handle {
            styx::BackendHandle::V4l2 { path } => path.clone(),
            styx::BackendHandle::Libcamera { id } => id.clone(),
            styx::BackendHandle::Netcam { url, .. } => url.clone(),
            styx::BackendHandle::File { paths, .. } => paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(","),
            other => format!("{other:?}"),
        };

        let active_pipeline_id = stream.manifest.active_pipeline_id;
        let (pipeline_id, pipeline_label) = if let Some(pipeline_id) = active_pipeline_id {
            let label = match pipelines::load_graph_document(pipeline_id).await {
                Ok(doc) => doc.name.unwrap_or_else(|| pipeline_id.to_string()),
                Err(_) => pipeline_id.to_string(),
            };
            (pipeline_id.to_string(), label)
        } else if let Some(binding) = stream.manifest.pipelines.first() {
            let label = match pipelines::load_graph_document(binding.pipeline_id).await {
                Ok(doc) => doc.name.unwrap_or_else(|| binding.pipeline_id.to_string()),
                Err(_) => binding.pipeline_id.to_string(),
            };
            (binding.pipeline_id.to_string(), label)
        } else {
            ("none".to_string(), "Raw".to_string())
        };

        for output_key in ports {
            out.push(LocalizationPipelineSource {
                id: format!("{stream_id}:{output_key}"),
                stream_id: stream_id.to_string(),
                stream_label: stream_label.clone(),
                camera_uid: camera_uid.clone(),
                camera_path: camera_path.clone(),
                pipeline_id: pipeline_id.clone(),
                pipeline_label: pipeline_label.clone(),
                output_key,
                data_type: None,
            });
        }
    }

    if let Ok(localization_config) = config::load_config().await {
        let excluded_profile_stream = localization_config.active_profile_id.as_deref().map(str::trim).filter(|id| !id.is_empty()).map(|id| format!("{PROFILE_STREAM_PREFIX}{id}"));

        out.extend(build_profile_output_sources(&localization_config).into_iter().filter(|source| match excluded_profile_stream.as_deref() {
            Some(stream_id) => source.stream_id != stream_id,
            None => true,
        }));
    }

    let peers = peers::snapshot_peers().await;
    for peer in peers {
        out.extend(localization_peers::sources::list_peer_sources(&peer).await);
    }

    let external_sources = external::list_external_sources_snapshot().await;
    let has_external_imu = external_sources.iter().any(|source| source.id == IMU_EXTERNAL_ID);
    if !has_external_imu {
        out.push(LocalizationPipelineSource {
            id: "external:imu:imu_pose".to_string(),
            stream_id: "external:imu".to_string(),
            stream_label: "IMU".to_string(),
            camera_uid: "imu".to_string(),
            camera_path: "device:imu".to_string(),
            pipeline_id: "external".to_string(),
            pipeline_label: "IMU".to_string(),
            output_key: "imu_pose".to_string(),
            data_type: None,
        });
    }

    for source in external_sources {
        let source_id = source.id;
        let output_key = source.output_key;
        out.push(LocalizationPipelineSource {
            id: format!("external:{source_id}:{output_key}"),
            stream_id: format!("external:{source_id}"),
            stream_label: source.label,
            camera_uid: source.camera_uid.unwrap_or_else(|| source_id.clone()),
            camera_path: source.camera_path.unwrap_or_default(),
            pipeline_id: "external".to_string(),
            pipeline_label: source.pipeline_label.unwrap_or_else(|| "External".to_string()),
            output_key,
            data_type: source.data_type,
        });
    }

    Ok(Json(out))
}

#[utoipa::path(
    get,
    path = "/localization/streams/{id}/outputs/{output_key}",
    tag = "Localization",
    params(("id" = Uuid, Path, description = "Stream ID"), ("output_key" = String, Path, description = "Graph output port")),
    responses(
        (status = 200, description = "Latest output sample", body = PipelineOutputSample),
        (status = 404, description = "No sample available", body = EngineErrorBody),
        (status = 502, description = "Engine error", body = EngineErrorBody)
    )
)]
pub async fn sample_output(State(state): State<AppState>, Path((id, output_key)): Path<(Uuid, String)>) -> axum::response::Response {
    match state.engine.get_graph_output_sample_event(id, output_key.clone()).await {
        Ok(EngineEvent::GraphOutputSample { value, .. }) => Json(PipelineOutputSample { data_type: None, value: value.into() }).into_response(),
        Ok(EngineEvent::Nack { code, .. }) if code == EngineErrorCode::NotFound && is_media_imu_output_key(&output_key) => {
            match media_imu::fetch_media_imu_sample_for_stream(&state, id, &output_key).await {
                Ok(sample) => Json(sample).into_response(),
                Err(err) => err.into_response(),
            }
        }
        Ok(EngineEvent::Nack { code, reason, .. }) => {
            let status = if code == EngineErrorCode::NotFound { StatusCode::NOT_FOUND } else { StatusCode::BAD_REQUEST };
            (status, Json(engine_error_body(Some(code), reason))).into_response()
        }
        Ok(_) => (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), "unexpected engine response"))).into_response(),
        Err(err) => map_client_error(err),
    }
}

#[utoipa::path(
    get,
    path = "/localization/peers/{id}/outputs/{output_key}",
    tag = "Localization",
    params(("id" = String, Path, description = "Peer ID"), ("output_key" = String, Path, description = "Output key (e.g. tag_poses)")),
    responses(
        (status = 200, description = "Latest output sample", body = PipelineOutputSample),
        (status = 404, description = "No sample available", body = EngineErrorBody),
        (status = 400, description = "Unsupported output", body = EngineErrorBody),
        (status = 502, description = "Peer error", body = EngineErrorBody)
    )
)]
pub async fn sample_peer_output(Path((id, output_key)): Path<(String, String)>) -> axum::response::Response {
    let (peer_id, camera) = localization_peers::sources::parse_peer_stream_id(&id);
    let peers = peers::snapshot_peers().await;
    let Some(peer) = peers.into_iter().find(|p| p.id == peer_id) else {
        return (StatusCode::NOT_FOUND, Json(engine_error_body(Some(EngineErrorCode::NotFound), "peer not found"))).into_response();
    };

    match localization_peers::sources::fetch_peer_output_sample(&peer, camera.as_deref(), &output_key).await {
        Ok(sample) => Json(sample).into_response(),
        Err(err) => (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), err))).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/localization/profiles/{id}/outputs/{output_key}",
    tag = "Localization",
    params(("id" = String, Path, description = "Localization profile ID"), ("output_key" = String, Path, description = "Profile solver output key")),
    responses(
        (status = 200, description = "Latest profile output sample", body = PipelineOutputSample),
        (status = 404, description = "No sample available", body = EngineErrorBody),
        (status = 400, description = "Unsupported output", body = EngineErrorBody),
        (status = 502, description = "Solver error", body = EngineErrorBody)
    )
)]
pub async fn sample_profile_output(State(state): State<AppState>, Path((id, output_key)): Path<(String, String)>) -> axum::response::Response {
    let fetcher = ApiLocalizationSourceFetcher::new(state);
    match fetch_profile_output(&fetcher, id.trim(), &output_key).await {
        Ok(value) => Json(PipelineOutputSample { data_type: None, value }).into_response(),
        Err(reason) => {
            let lower = reason.to_ascii_lowercase();
            let not_found = lower.contains("not found") || lower.contains("missing");
            let status = if not_found { StatusCode::NOT_FOUND } else { StatusCode::BAD_REQUEST };
            let code = if not_found { EngineErrorCode::NotFound } else { EngineErrorCode::InvalidInput };
            (status, Json(engine_error_body(Some(code), reason))).into_response()
        }
    }
}

pub(crate) async fn fetch_stream_output(state: &AppState, stream_id: &str, output_key: &str) -> Result<JsonValue, String> {
    let stream_uuid = Uuid::parse_str(stream_id).map_err(|_| "invalid stream id".to_string())?;
    match state.engine.get_graph_output_sample_event(stream_uuid, output_key.to_string()).await {
        Ok(EngineEvent::GraphOutputSample { value, .. }) => Ok(value.into()),
        Ok(EngineEvent::Nack { code, .. }) if code == EngineErrorCode::NotFound && is_media_imu_output_key(output_key) => {
            media_imu::fetch_media_imu_sample_for_stream(state, stream_uuid, output_key).await.map(|sample| sample.value).map_err(|err| err.to_string())
        }
        Ok(EngineEvent::Nack { reason, .. }) => Err(reason),
        Ok(_) => Err("unexpected engine response".to_string()),
        Err(err) => Err(err.to_string()),
    }
}

pub(crate) async fn fetch_peer_output(stream_id: &str, output_key: &str) -> Result<JsonValue, String> {
    let (peer_id, camera) = localization_peers::sources::parse_peer_stream_id(stream_id.trim_start_matches("peer:"));
    let peers = peers::snapshot_peers().await;
    let Some(peer) = peers.into_iter().find(|peer| peer.id == peer_id) else {
        return Err("peer not found".to_string());
    };

    localization_peers::sources::fetch_peer_output_value(&peer, camera.as_deref(), output_key).await
}

#[derive(Debug, Clone)]
struct ProfileOutputSelector {
    solver_id: String,
    pose_space: LocalizationPoseSpace,
    camera_uid: Option<String>,
}

fn build_profile_output_sources(config: &LocalizationConfig) -> Vec<LocalizationPipelineSource> {
    let mut out: Vec<LocalizationPipelineSource> = Vec::new();
    let mut seen = BTreeSet::<(String, String)>::new();

    for profile in &config.profiles {
        let profile_id = profile.id.trim();
        if profile_id.is_empty() {
            continue;
        }

        let stream_id = format!("{PROFILE_STREAM_PREFIX}{profile_id}");
        let stream_label = {
            let name = profile.name.trim();
            if name.is_empty() { profile_id.to_string() } else { name.to_string() }
        };
        let camera_path = format!("profile:{profile_id}");
        let camera_uids = collect_profile_camera_uids(profile);
        let profile_camera_uid = format!("profile:{profile_id}");

        for solver in &profile.solvers {
            let solver_id = solver.id.trim();
            if solver_id.is_empty() {
                continue;
            }

            let pipeline_id = format!("profile-solver:{profile_id}:{solver_id}");
            let pipeline_label = {
                let solver_name = solver.name.trim();
                if solver_name.is_empty() { format!("{stream_label} · {solver_id}") } else { format!("{stream_label} · {solver_name}") }
            };

            for pose_space in &solver.output_spaces {
                if is_camera_scoped_pose_space(*pose_space) {
                    for camera_uid in &camera_uids {
                        let output_key = build_profile_output_key(solver_id, *pose_space, Some(camera_uid.as_str()));
                        if !seen.insert((stream_id.clone(), output_key.clone())) {
                            continue;
                        }
                        out.push(LocalizationPipelineSource {
                            id: format!("{stream_id}:{output_key}"),
                            stream_id: stream_id.clone(),
                            stream_label: stream_label.clone(),
                            camera_uid: camera_uid.clone(),
                            camera_path: camera_path.clone(),
                            pipeline_id: pipeline_id.clone(),
                            pipeline_label: pipeline_label.clone(),
                            output_key,
                            data_type: Some(json!({ "kind": if is_detection_pose_space(*pose_space) { "localization_detection_pose" } else { "localization_pose" } })),
                        });
                    }
                    continue;
                }

                let output_key = build_profile_output_key(solver_id, *pose_space, None);
                if !seen.insert((stream_id.clone(), output_key.clone())) {
                    continue;
                }
                out.push(LocalizationPipelineSource {
                    id: format!("{stream_id}:{output_key}"),
                    stream_id: stream_id.clone(),
                    stream_label: stream_label.clone(),
                    camera_uid: profile_camera_uid.clone(),
                    camera_path: camera_path.clone(),
                    pipeline_id: pipeline_id.clone(),
                    pipeline_label: pipeline_label.clone(),
                    output_key,
                    data_type: Some(json!({ "kind": "localization_pose" })),
                });
            }
        }
    }

    out
}

fn collect_profile_camera_uids(profile: &LocalizationProfile) -> Vec<String> {
    let mut out = BTreeSet::<String>::new();
    for source in profile.sources.iter().filter(|source| source.enabled) {
        collect_profile_camera_uid(&mut out, &source.camera_uid);
    }

    if out.is_empty() {
        for source in &profile.sources {
            collect_profile_camera_uid(&mut out, &source.camera_uid);
        }
    }

    out.into_iter().collect()
}

fn collect_profile_camera_uid(out: &mut BTreeSet<String>, raw: &str) {
    let camera_uid = raw.trim();
    if camera_uid.is_empty() || camera_uid.eq_ignore_ascii_case("imu") {
        return;
    }
    out.insert(camera_uid.to_string());
}

fn build_profile_output_key(solver_id: &str, pose_space: LocalizationPoseSpace, camera_uid: Option<&str>) -> String {
    match camera_uid {
        Some(camera_uid) => format!("{PROFILE_OUTPUT_PREFIX}{solver_id}:{}:{camera_uid}", pose_space_key(pose_space)),
        None => format!("{PROFILE_OUTPUT_PREFIX}{solver_id}:{}", pose_space_key(pose_space)),
    }
}

fn parse_profile_output_selector(output_key: &str) -> Result<ProfileOutputSelector, String> {
    let Some(raw) = output_key.strip_prefix(PROFILE_OUTPUT_PREFIX) else {
        return Err("profile output key must start with 'solver:'".to_string());
    };
    let mut parts = raw.splitn(3, ':');
    let solver_id = parts.next().map(str::trim).unwrap_or_default();
    let pose_space_key = parts.next().map(str::trim).unwrap_or_default();
    let camera_uid = parts.next().map(str::trim).filter(|value| !value.is_empty()).map(ToString::to_string);

    if solver_id.is_empty() {
        return Err("profile output key is missing solver id".to_string());
    }
    let Some(pose_space) = pose_space_from_key(pose_space_key) else {
        return Err(format!("unsupported profile pose space '{pose_space_key}'"));
    };
    if is_camera_scoped_pose_space(pose_space) && camera_uid.is_none() {
        return Err(format!("profile output '{output_key}' must include camera uid"));
    }
    if !is_camera_scoped_pose_space(pose_space) && camera_uid.is_some() {
        return Err(format!("profile output '{output_key}' does not accept camera uid"));
    }

    Ok(ProfileOutputSelector { solver_id: solver_id.to_string(), pose_space, camera_uid })
}

fn pose_space_from_key(raw: &str) -> Option<LocalizationPoseSpace> {
    match raw.trim() {
        "tag_in_camera" => Some(LocalizationPoseSpace::TagInCamera),
        "camera_in_tag" => Some(LocalizationPoseSpace::CameraInTag),
        "tag_in_robot" => Some(LocalizationPoseSpace::TagInRobot),
        "robot_in_tag" => Some(LocalizationPoseSpace::RobotInTag),
        "camera_in_field" => Some(LocalizationPoseSpace::CameraInField),
        "robot_in_field" => Some(LocalizationPoseSpace::RobotInField),
        _ => None,
    }
}

fn pose_space_key(pose_space: LocalizationPoseSpace) -> &'static str {
    match pose_space {
        LocalizationPoseSpace::TagInCamera => "tag_in_camera",
        LocalizationPoseSpace::CameraInTag => "camera_in_tag",
        LocalizationPoseSpace::TagInRobot => "tag_in_robot",
        LocalizationPoseSpace::RobotInTag => "robot_in_tag",
        LocalizationPoseSpace::CameraInField => "camera_in_field",
        LocalizationPoseSpace::RobotInField => "robot_in_field",
    }
}

fn is_camera_scoped_pose_space(pose_space: LocalizationPoseSpace) -> bool {
    matches!(
        pose_space,
        LocalizationPoseSpace::TagInCamera | LocalizationPoseSpace::CameraInTag | LocalizationPoseSpace::TagInRobot | LocalizationPoseSpace::RobotInTag | LocalizationPoseSpace::CameraInField
    )
}

fn is_detection_pose_space(pose_space: LocalizationPoseSpace) -> bool {
    matches!(pose_space, LocalizationPoseSpace::TagInCamera | LocalizationPoseSpace::CameraInTag | LocalizationPoseSpace::TagInRobot | LocalizationPoseSpace::RobotInTag)
}

pub(crate) async fn fetch_profile_output(fetcher: &ApiLocalizationSourceFetcher, profile_id: &str, output_key: &str) -> Result<JsonValue, String> {
    let profile_id = profile_id.trim();
    if profile_id.is_empty() {
        return Err("profile id is required".to_string());
    }

    {
        let mut stack = fetcher.profile_resolve_stack.lock().await;
        if stack.iter().any(|entry| entry == profile_id) {
            return Err(format!("profile source cycle detected for '{profile_id}'"));
        }
        stack.push(profile_id.to_string());
    }

    let result = fetch_profile_output_inner(fetcher, profile_id, output_key).await;

    let mut stack = fetcher.profile_resolve_stack.lock().await;
    if let Some(index) = stack.iter().rposition(|entry| entry == profile_id) {
        stack.remove(index);
    }

    result
}

async fn fetch_profile_output_inner(fetcher: &ApiLocalizationSourceFetcher, profile_id: &str, output_key: &str) -> Result<JsonValue, String> {
    let config = config::load_config().await.map_err(|err| err.to_string())?;
    let profile = select_profile(&config, Some(profile_id)).map_err(|_| format!("profile '{profile_id}' not found"))?;
    let mut sources = solve::dedupe_enabled_sources(profile.sources.iter().filter(|source| source.enabled).cloned().collect::<Vec<_>>());
    solve::enrich_source_input_keys(&fetcher.state, &mut sources).await;

    let mut rig_poses = solve::load_rig_poses(&fetcher.state, &sources).await;
    solve::inject_imu_leveling_rig_pose(&fetcher.state, profile, &mut rig_poses).await;
    let field_map = if let Some(map_id) = profile.field_map_id.as_deref() { maps::load_map_document(map_id).await.ok() } else { None };
    let calibrations = solve::load_stream_calibrations(&fetcher.state).await;
    let response = solve_localization(profile, &sources, &rig_poses, field_map.as_ref(), &calibrations, fetcher).await;

    let selector = parse_profile_output_selector(output_key)?;
    let Some(solver) = response.solvers.into_iter().find(|solver| solver.id == selector.solver_id) else {
        return Err(format!("solver '{}' not found in profile '{}'", selector.solver_id, profile_id));
    };

    profile_output_payload(&solver.outputs, &selector)
}

fn profile_output_payload(outputs: &LocalizationSolverOutputs, selector: &ProfileOutputSelector) -> Result<JsonValue, String> {
    let pose_space_label = pose_space_key(selector.pose_space);
    match selector.pose_space {
        LocalizationPoseSpace::TagInCamera => {
            let camera_uid = selector.camera_uid.as_deref().ok_or_else(|| "camera uid is required".to_string())?;
            let detections = outputs.tag_in_camera.as_ref().ok_or_else(|| format!("solver does not publish '{pose_space_label}'"))?;
            detection_output_payload(detections, camera_uid, pose_space_label)
        }
        LocalizationPoseSpace::CameraInTag => {
            let camera_uid = selector.camera_uid.as_deref().ok_or_else(|| "camera uid is required".to_string())?;
            let detections = outputs.camera_in_tag.as_ref().ok_or_else(|| format!("solver does not publish '{pose_space_label}'"))?;
            detection_output_payload(detections, camera_uid, pose_space_label)
        }
        LocalizationPoseSpace::TagInRobot => {
            let camera_uid = selector.camera_uid.as_deref().ok_or_else(|| "camera uid is required".to_string())?;
            let detections = outputs.tag_in_robot.as_ref().ok_or_else(|| format!("solver does not publish '{pose_space_label}'"))?;
            detection_output_payload(detections, camera_uid, pose_space_label)
        }
        LocalizationPoseSpace::RobotInTag => {
            let camera_uid = selector.camera_uid.as_deref().ok_or_else(|| "camera uid is required".to_string())?;
            let detections = outputs.robot_in_tag.as_ref().ok_or_else(|| format!("solver does not publish '{pose_space_label}'"))?;
            detection_output_payload(detections, camera_uid, pose_space_label)
        }
        LocalizationPoseSpace::CameraInField => {
            let camera_uid = selector.camera_uid.as_deref().ok_or_else(|| "camera uid is required".to_string())?;
            let poses = outputs.camera_in_field.as_ref().ok_or_else(|| format!("solver does not publish '{pose_space_label}'"))?;
            let Some(entry) = poses.iter().find(|entry| entry.camera_uid.trim() == camera_uid) else {
                return Err(format!("solver output '{pose_space_label}' missing camera '{camera_uid}'"));
            };
            Ok(json!({ "pose": entry.pose }))
        }
        LocalizationPoseSpace::RobotInField => {
            let pose = outputs.robot_in_field.as_ref().ok_or_else(|| format!("solver does not publish '{pose_space_label}'"))?;
            Ok(json!({ "pose": pose.pose }))
        }
    }
}

fn detection_output_payload(detections: &[LocalizationDetectionPose], camera_uid: &str, pose_space: &str) -> Result<JsonValue, String> {
    let rows: Vec<JsonValue> = detections
        .iter()
        .filter(|detection| detection.camera_uid.trim() == camera_uid)
        .map(|detection| {
            json!({
                "id": detection.tag_id,
                "translation": detection.pose.translation,
                "rotation": detection.pose.rotation,
                "tag_size": detection.tag_size,
                "code_rotation": detection.code_rotation,
                "bits": detection.tag_bits
            })
        })
        .collect();

    if rows.is_empty() {
        return Err(format!("solver output '{pose_space}' missing camera '{camera_uid}'"));
    }

    Ok(json!({ "detections": rows }))
}
