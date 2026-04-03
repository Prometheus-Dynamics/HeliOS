use std::collections::HashMap;
use std::time::Instant;

use lib_cv::modules::aruco::pose::TagPoseCalibration;
use lib_cv::modules::aruco::ArucoBitGrid;
use lib_cv::Translation3;

use super::config::LocalizationSourceConfig;
use super::fetch::{imu_backend_to_viewer_basis, LocalizationSourceFetcher};
use super::math::{pose_from_detection, PoseTransform};
use super::types::LocalizationQuaternion;

mod aruco;
mod detections;
mod multitag;
mod pose;
mod temporal;

use multitag::apply_multitag_normal_consistency;
use temporal::{
    apply_pair_distance_consistency, collapse_duplicate_tag_detections, pose_reliability_quality,
    smooth_detection_tag_poses,
};

pub enum SourceParse {
    Detections(ParsedDetections),
    Pose(PoseSample),
}

pub struct SourceParserContext<'a> {
    pub source: &'a LocalizationSourceConfig,
    pub default_tag_size_m: Option<f64>,
    pub calibrations: &'a HashMap<String, TagPoseCalibration>,
}

pub trait LocalizationSourceParser: Send + Sync {
    fn parse(&self, value: &serde_json::Value, ctx: &SourceParserContext<'_>) -> Option<Result<SourceParse, String>>;
}

pub struct SourceParserRegistry {
    parsers: Vec<Box<dyn LocalizationSourceParser>>,
}

impl SourceParserRegistry {
    pub fn new() -> Self {
        Self { parsers: Vec::new() }
    }

    pub fn register<P: LocalizationSourceParser + 'static>(&mut self, parser: P) {
        self.parsers.push(Box::new(parser));
    }

    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register(detections::DetectionPoseParser::new());
        registry.register(pose::PoseParser::new());
        registry.register(aruco::ArucoDetectionsParser::new());
        registry
    }

    pub fn parse(&self, value: &serde_json::Value, ctx: &SourceParserContext<'_>) -> Result<SourceParse, String> {
        for parser in &self.parsers {
            if let Some(result) = parser.parse(value, ctx) {
                return result;
            }
        }
        Err("unsupported source payload".to_string())
    }
}

impl Default for SourceParserRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct LocalizationDetection {
    pub source_id: String,
    pub camera_uid: String,
    pub tag_id: u32,
    pub camera_from_tag: PoseTransform,
    pub tag_size: Option<f64>,
    pub code_rotation: Option<u8>,
    pub tag_bits: Option<ArucoBitGrid>,
    pub weight: f32,
    pub quality: f32,
}

#[derive(Debug)]
pub struct SourceSample {
    pub source: LocalizationSourceConfig,
    pub detections: Vec<LocalizationDetection>,
    pub pose: Option<PoseSample>,
    pub poll_ms: f64,
    pub tag_size: Option<f64>,
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct ParsedDetections {
    pub detections: Vec<LocalizationDetection>,
    pub tag_size: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct PoseSample {
    pub pose: PoseTransform,
    pub has_translation: bool,
    pub has_rotation: bool,
}

pub async fn fetch_source_samples<F: LocalizationSourceFetcher>(
    fetcher: &F,
    sources: &[LocalizationSourceConfig],
    default_tag_size_m: Option<f64>,
    calibrations: &HashMap<String, TagPoseCalibration>,
) -> Vec<SourceSample> {
    let registry = SourceParserRegistry::with_defaults();
    fetch_source_samples_with_registry(fetcher, sources, default_tag_size_m, calibrations, &registry).await
}

pub async fn fetch_source_value<F: LocalizationSourceFetcher>(fetcher: &F, source: &LocalizationSourceConfig) -> Result<serde_json::Value, String> {
    fetcher.fetch_source_value(source).await
}

pub async fn fetch_source_samples_with_registry<F: LocalizationSourceFetcher>(
    fetcher: &F,
    sources: &[LocalizationSourceConfig],
    default_tag_size_m: Option<f64>,
    calibrations: &HashMap<String, TagPoseCalibration>,
    registry: &SourceParserRegistry,
) -> Vec<SourceSample> {
    let mut out = Vec::new();
    for source in sources {
        out.push(fetch_source_sample(fetcher, source, default_tag_size_m, calibrations, registry).await);
    }
    out
}

async fn fetch_source_sample<F: LocalizationSourceFetcher>(
    fetcher: &F,
    source: &LocalizationSourceConfig,
    default_tag_size_m: Option<f64>,
    calibrations: &HashMap<String, TagPoseCalibration>,
    registry: &SourceParserRegistry,
) -> SourceSample {
    let start = Instant::now();
    let mut detections = Vec::new();
    let mut pose = None;
    let mut tag_size = None;
    let mut error = None;

    match fetcher.fetch_source_value(source).await {
        Ok(value) => {
            let ctx = SourceParserContext { source, default_tag_size_m, calibrations };
            match registry.parse(&value, &ctx) {
                Ok(SourceParse::Detections(mut parsed)) => {
                    collapse_duplicate_tag_detections(source, &mut parsed.detections);
                    apply_pair_distance_consistency(source, &mut parsed.detections);
                    for detection in &mut parsed.detections {
                        detection.source_id = source.id.clone();
                        detection.camera_uid = source.camera_uid.clone();
                        let pose_quality = pose_reliability_quality(&detection.camera_from_tag);
                        detection.weight = source.weight.max(0.0) * detection.quality.max(0.0) * pose_quality;
                    }
                    smooth_detection_tag_poses(source, &mut parsed.detections);
                    apply_multitag_normal_consistency(&mut parsed.detections);
                    detections = parsed.detections;
                    tag_size = parsed.tag_size;
                }
                Ok(SourceParse::Pose(parsed_pose)) => {
                    pose = Some(parsed_pose);
                }
                Err(err) => error = Some(err),
            }
        }
        Err(err) => error = Some(err),
    }

    SourceSample { source: source.clone(), detections, pose, poll_ms: start.elapsed().as_secs_f64() * 1000.0, tag_size, error }
}

pub(crate) fn translation_from_value(value: &serde_json::Value) -> Option<Translation3> {
    Some(Translation3 { x: value.get("x")?.as_f64()?, y: value.get("y")?.as_f64()?, z: value.get("z")?.as_f64()? })
}

pub(crate) fn translation_from_pose_value(value: &serde_json::Value) -> Option<Translation3> {
    if let Some(translation) = value.get("translation").or_else(|| value.get("position")) {
        return translation_from_value(translation);
    }
    let x = value.get("x").and_then(|value| value.as_f64())?;
    let y = value.get("y").and_then(|value| value.as_f64())?;
    let z = value.get("z").and_then(|value| value.as_f64())?;
    Some(Translation3 { x, y, z })
}

pub(crate) fn rotation_from_value(value: &serde_json::Value) -> (Option<LocalizationQuaternion>, Option<(f64, f64, f64)>) {
    let quat =
        value.get("quaternion").and_then(|quat| Some(LocalizationQuaternion { x: quat.get("x")?.as_f64()?, y: quat.get("y")?.as_f64()?, z: quat.get("z")?.as_f64()?, w: quat.get("w")?.as_f64()? }));

    let roll = value.get("roll").and_then(|value| value.as_f64());
    let pitch = value.get("pitch").and_then(|value| value.as_f64());
    let yaw = value.get("yaw").and_then(|value| value.as_f64());
    let euler = if roll.is_some() || pitch.is_some() || yaw.is_some() { Some((roll.unwrap_or(0.0), pitch.unwrap_or(0.0), yaw.unwrap_or(0.0))) } else { None };

    (quat, euler)
}

pub(crate) fn pose_from_translation_rotation(translation: Translation3, quaternion: Option<LocalizationQuaternion>, euler: Option<(f64, f64, f64)>) -> PoseTransform {
    pose_from_detection(translation, quaternion, euler)
}

fn has_imu_token(value: &str) -> bool {
    value.split(|ch: char| !ch.is_ascii_alphanumeric()).any(|token| token.eq_ignore_ascii_case("imu"))
}

pub(crate) fn source_looks_like_imu(source: &LocalizationSourceConfig) -> bool {
    [source.id.as_str(), source.stream_id.as_str(), source.output_key.as_str(), source.camera_uid.as_str()].iter().any(|value| has_imu_token(value))
}

pub(crate) fn imu_pose_to_viewer_frame(pose: PoseTransform) -> PoseTransform {
    let basis = imu_backend_to_viewer_basis();
    let translation = basis.transform_vector(&pose.translation);
    let rotation = basis * pose.rotation * basis.inverse();
    PoseTransform { translation, rotation }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::localization::config::LocalizationSourceConfig;

    fn source_config() -> LocalizationSourceConfig {
        LocalizationSourceConfig {
            id: "cam-front".to_string(),
            stream_id: "stream-a".to_string(),
            output_key: "detections".to_string(),
            camera_uid: "cam-front".to_string(),
            pose_space: None,
            input_key: None,
            enabled: true,
            weight: 1.0,
        }
    }

    fn detection(tag_id: u32, quality: f32, x: f64, y: f64, z: f64) -> LocalizationDetection {
        LocalizationDetection {
            source_id: String::new(),
            camera_uid: String::new(),
            tag_id,
            camera_from_tag: PoseTransform { translation: Vector3::new(x, y, z), rotation: UnitQuaternion::identity() },
            tag_size: Some(0.165),
            code_rotation: Some(0),
            tag_bits: None,
            weight: 1.0,
            quality,
        }
    }

    #[test]
    fn collapse_duplicates_keeps_best_quality_detection() {
        let source = source_config();
        let mut detections = vec![detection(7, 0.25, 0.0, 0.0, 2.0), detection(7, 0.91, 0.12, 0.0, 2.1)];
        collapse_duplicate_tag_detections(&source, &mut detections);
        assert_eq!(detections.len(), 1);
        assert_eq!(detections[0].tag_id, 7);
        assert!((detections[0].camera_from_tag.translation.x - 0.12).abs() < 1e-6);
    }

    #[test]
    fn collapse_duplicates_penalizes_ambiguous_far_apart_candidates() {
        let source = source_config();
        let mut detections = vec![detection(3, 0.90, 0.0, 0.0, 2.0), detection(3, 0.82, 1.45, 0.0, 2.0)];
        collapse_duplicate_tag_detections(&source, &mut detections);
        assert_eq!(detections.len(), 1);
        assert_eq!(detections[0].tag_id, 3);
        assert!(detections[0].quality <= 0.5);
    }
}
