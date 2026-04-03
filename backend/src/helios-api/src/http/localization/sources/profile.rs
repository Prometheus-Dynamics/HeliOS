use serde_json::{Value as JsonValue, json};
use std::collections::BTreeSet;

use super::{ApiLocalizationSourceFetcher, PROFILE_OUTPUT_PREFIX};
use crate::http::localization::{config, maps, solve};
use helios_engine::localization::config::{LocalizationConfig, LocalizationPoseSpace, LocalizationProfile, select_profile};
use helios_engine::localization::types::{LocalizationDetectionPose, LocalizationPipelineSource, LocalizationSolverOutputs, LocalizationSourceKind};

#[derive(Debug, Clone)]
struct ProfileOutputSelector {
    solver_id: String,
    pose_space: LocalizationPoseSpace,
    camera_uid: Option<String>,
}

pub(super) fn build_profile_output_sources(config: &LocalizationConfig) -> Vec<LocalizationPipelineSource> {
    let mut out: Vec<LocalizationPipelineSource> = Vec::new();
    let mut seen = BTreeSet::<(String, String)>::new();

    for profile in &config.profiles {
        if !profile.enabled {
            continue;
        }
        let profile_id = profile.id.trim();
        if profile_id.is_empty() {
            continue;
        }

        let stream_id = format!("{}{}", super::PROFILE_STREAM_PREFIX, profile_id);
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
                            localization_kind: if is_detection_pose_space(*pose_space) { LocalizationSourceKind::Detection } else { LocalizationSourceKind::Pose },
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
                    localization_kind: LocalizationSourceKind::Pose,
                    data_type: Some(json!({ "kind": "localization_pose" })),
                });
            }
        }
    }

    out
}

pub(crate) async fn fetch_profile_output(fetcher: &ApiLocalizationSourceFetcher, profile_id: &str, output_key: &str) -> Result<JsonValue, String> {
    let profile_id = profile_id.trim();
    if profile_id.is_empty() {
        return Err("profile id is required".to_string());
    }

    let _guard = fetcher.profile_resolve_stack.enter(profile_id)?;
    fetch_profile_output_inner(fetcher, profile_id, output_key).await
}

async fn fetch_profile_output_inner(fetcher: &ApiLocalizationSourceFetcher, profile_id: &str, output_key: &str) -> Result<JsonValue, String> {
    let config = config::load_config().await.map_err(|err| err.to_string())?;
    let profile = select_profile(&config, Some(profile_id)).map_err(|err| format!("profile '{profile_id}': {err}"))?;
    let mut sources = solve::dedupe_enabled_sources(profile.sources.iter().filter(|source| source.enabled).cloned().collect::<Vec<_>>());
    let stream_summaries = fetcher.state.engine.list_streams().await.unwrap_or_default();
    solve::enrich_source_input_keys_from_streams(&stream_summaries, &mut sources);

    let mut rig_poses = solve::load_rig_poses_from_streams(&sources, &stream_summaries).await;
    solve::inject_imu_leveling_rig_pose(&fetcher.state, profile, &mut rig_poses).await;
    let field_map = if let Some(map_id) = profile.field_map_id.as_deref() { maps::load_map_document(map_id).await.ok() } else { None };
    let response = solve::solve_via_engine(&fetcher.state, profile, sources, &rig_poses, field_map.as_ref(), &stream_summaries, fetcher, true).await?;

    let selector = parse_profile_output_selector(output_key)?;
    let Some(solver) = response.solvers.into_iter().find(|solver| solver.id == selector.solver_id) else {
        return Err(format!("solver '{}' not found in profile '{}'", selector.solver_id, profile_id));
    };

    profile_output_payload(&solver.outputs, &selector)
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

#[cfg(test)]
mod tests {
    use super::{LocalizationPoseSpace, build_profile_output_key, parse_profile_output_selector};

    #[test]
    fn parses_camera_scoped_profile_output() {
        let key = build_profile_output_key("main", LocalizationPoseSpace::TagInCamera, Some("cam-a"));
        let parsed = parse_profile_output_selector(&key).expect("selector");
        assert_eq!(parsed.solver_id, "main");
        assert_eq!(parsed.camera_uid.as_deref(), Some("cam-a"));
    }
}
