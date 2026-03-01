use serde_json::Value as JsonValue;

use nalgebra::{UnitQuaternion, Vector3};

use lib_cv::modules::aruco::ArucoBitGrid;
use lib_cv::modules::aruco::DetectionPoseOutput;

use super::{pose_from_translation_rotation, rotation_from_value, translation_from_value, LocalizationDetection, LocalizationSourceParser, ParsedDetections, SourceParse, SourceParserContext};
use crate::localization::math::PoseTransform;
use crate::localization::types::LocalizationQuaternion;

pub(crate) struct DetectionPoseParser;

impl DetectionPoseParser {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl LocalizationSourceParser for DetectionPoseParser {
    fn parse(&self, value: &JsonValue, _ctx: &SourceParserContext<'_>) -> Option<Result<SourceParse, String>> {
        match parse_detection_output(value) {
            Ok(parsed) => Some(Ok(SourceParse::Detections(parsed))),
            Err(_) => None,
        }
    }
}

fn reprojection_quality_weight(reprojection_error_px: Option<f64>) -> f32 {
    let Some(err) = reprojection_error_px else {
        return 1.0;
    };
    if !err.is_finite() || err <= 0.0 {
        return 1.0;
    }
    // Keep moderate reprojection-error detections in play (small/far tags under mild blur), while
    // still suppressing severe outliers that cause tag-switch teleports.
    if err >= 8.0 {
        return 0.0;
    }
    if err >= 6.0 {
        return 0.08;
    }
    if err >= 5.0 {
        return 0.18;
    }
    if err >= 4.0 {
        return 0.32;
    }
    if err >= 3.0 {
        return 0.5;
    }
    if err >= 2.5 {
        return 0.62;
    }
    if err >= 2.0 {
        return 0.74;
    }
    if err >= 1.5 {
        return 0.84;
    }
    if err >= 1.0 {
        return 0.92;
    }
    1.0
}

fn detection_pose_to_viewer(transform: PoseTransform) -> PoseTransform {
    // lib-cv pose outputs are expressed in a Three.js-friendly camera basis:
    // +X right, +Y up, -Z forward.
    //
    // The localization viewer conventions use:
    // +X left, +Y up, +Z forward.
    //
    // Convert `camera_from_tag` by applying a basis change on the *camera* frame only
    // (left-multiply). This flips X/Z while leaving Y (up) intact.
    let fix = UnitQuaternion::from_axis_angle(&Vector3::y_axis(), std::f64::consts::PI);
    let translation = fix.transform_vector(&transform.translation);
    let rotation = fix * transform.rotation;
    PoseTransform { translation, rotation }
}

pub(crate) fn parse_detection_output(value: &JsonValue) -> Result<ParsedDetections, String> {
    if let Ok(output) = serde_json::from_value::<DetectionPoseOutput>(value.clone()) {
        return parsed_from_pose_output(output);
    }

    let detections_value = value.get("detections").and_then(|value| value.as_array()).ok_or_else(|| "missing detections array".to_string())?;

    // Avoid falsely claiming generic `{detections:[...]}` payloads that are *not* pose outputs.
    // ArUco detection JSON uses `corners` (and has no translation); those should be handled by
    // `ArucoDetectionsParser`, not this pose-output parser.
    if detections_value.iter().any(|det| det.get("corners").is_some()) {
        return Err("detections array looks like 2D corner detections, not pose detections".to_string());
    }

    let mut detections = Vec::new();
    for det in detections_value {
        let id = det.get("id").and_then(|value| value.as_u64()).unwrap_or(u64::MAX);
        if id == u64::MAX {
            continue;
        }
        let translation = det.get("translation").and_then(translation_from_value);
        let Some(translation) = translation else {
            continue;
        };

        let (quat, euler) = det.get("rotation").map(rotation_from_value).unwrap_or((None, None));
        let camera_from_tag = detection_pose_to_viewer(pose_from_translation_rotation(translation, quat, euler));
        let tag_bits = det.get("bits").and_then(|bits| serde_json::from_value::<ArucoBitGrid>(bits.clone()).ok());

        detections.push(LocalizationDetection {
            source_id: String::new(),
            camera_uid: String::new(),
            tag_id: id as u32,
            camera_from_tag,
            tag_size: det.get("tag_size").and_then(|value| value.as_f64()),
            code_rotation: det.get("code_rotation").and_then(|value| value.as_u64()).map(|value| value as u8),
            tag_bits,
            weight: 1.0,
            quality: reprojection_quality_weight(det.get("reprojection_error_px").and_then(|value| value.as_f64())),
        });
    }

    Ok(ParsedDetections { detections, tag_size: None })
}

pub(crate) fn parsed_from_pose_output(output: DetectionPoseOutput) -> Result<ParsedDetections, String> {
    let detections = output
        .detections
        .into_iter()
        .map(|det| LocalizationDetection {
            source_id: String::new(),
            camera_uid: String::new(),
            tag_id: det.id,
            camera_from_tag: detection_pose_to_viewer(pose_from_translation_rotation(
                det.translation,
                Some(LocalizationQuaternion { x: det.rotation.quaternion.x, y: det.rotation.quaternion.y, z: det.rotation.quaternion.z, w: det.rotation.quaternion.w }),
                Some((det.rotation.roll, det.rotation.pitch, det.rotation.yaw)),
            )),
            tag_size: Some(det.tag_size),
            code_rotation: Some(det.code_rotation),
            tag_bits: det.bits.clone(),
            weight: 1.0,
            quality: reprojection_quality_weight(det.reprojection_error_px),
        })
        .collect();
    Ok(ParsedDetections { detections, tag_size: Some(output.stats.tag_size) })
}
