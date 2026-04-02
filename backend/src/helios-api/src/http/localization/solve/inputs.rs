use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::http::streams::RAW_PIPELINE_UUID;
use helios_engine::ipc::StreamSummary;
use helios_engine::localization::config::LocalizationSourceConfig;

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

pub(super) fn source_looks_like_imu(source: &LocalizationSourceConfig) -> bool {
    [source.id.as_str(), source.stream_id.as_str(), source.output_key.as_str(), source.camera_uid.as_str()].iter().any(|value| has_imu_token(value))
}

pub(super) fn source_looks_like_imu_pose(source: &LocalizationSourceConfig) -> bool {
    if !source_looks_like_imu(source) {
        return false;
    }
    let key = source.output_key.trim().to_ascii_lowercase();
    key.contains("pose") || key.contains("orientation") || key.contains("quaternion") || key.contains("rotation")
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

fn infer_source_input_key(stream: &StreamSummary, source: &LocalizationSourceConfig) -> Option<String> {
    let manifest = &stream.manifest;
    let active_pipeline_id = manifest.active_pipeline_id.or_else(|| manifest.pipelines.first().map(|binding| binding.pipeline_id)).unwrap_or(RAW_PIPELINE_UUID);
    let source_output_key = normalize_key(Some(source.output_key.as_str()));

    if active_pipeline_id == RAW_PIPELINE_UUID {
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

#[cfg(test)]
mod tests {
    use helios_engine::localization::config::LocalizationSourceConfig;

    use super::{dedupe_enabled_sources, source_looks_like_imu, source_looks_like_imu_pose};

    fn source(id: &str, stream_id: &str, output_key: &str, camera_uid: &str) -> LocalizationSourceConfig {
        LocalizationSourceConfig {
            id: id.to_string(),
            stream_id: stream_id.to_string(),
            output_key: output_key.to_string(),
            camera_uid: camera_uid.to_string(),
            pose_space: None,
            input_key: None,
            enabled: true,
            weight: 1.0,
        }
    }

    #[test]
    fn dedupe_enabled_sources_removes_duplicate_triples() {
        let sources = vec![source("a", "stream-1", "tag_poses", "cam0"), source("b", "stream-1", "tag_poses", "cam0"), source("c", "stream-1", "tag_poses", "cam1")];
        let deduped = dedupe_enabled_sources(sources);
        assert_eq!(deduped.len(), 2);
    }

    #[test]
    fn imu_source_detection_distinguishes_pose_outputs() {
        assert!(source_looks_like_imu(&source("imu", "external:imu", "accel", "imu")));
        assert!(source_looks_like_imu_pose(&source("imu", "external:imu", "imu_pose", "imu")));
        assert!(!source_looks_like_imu_pose(&source("imu", "external:imu", "accel", "imu")));
    }
}
