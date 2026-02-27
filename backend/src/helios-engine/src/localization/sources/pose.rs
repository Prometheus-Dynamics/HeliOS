use serde_json::Value as JsonValue;

use super::{
    imu_pose_to_viewer_frame, pose_from_translation_rotation, rotation_from_value, source_looks_like_imu, translation_from_pose_value, LocalizationSourceParser, PoseSample, SourceParse,
    SourceParserContext,
};
use lib_cv::Translation3;

pub(crate) struct PoseParser;

impl PoseParser {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl LocalizationSourceParser for PoseParser {
    fn parse(&self, value: &JsonValue, ctx: &SourceParserContext<'_>) -> Option<Result<SourceParse, String>> {
        match parse_pose_output(value, ctx) {
            Ok(parsed) => Some(Ok(SourceParse::Pose(parsed))),
            Err(_) => None,
        }
    }
}

pub(crate) fn parse_pose_output(value: &JsonValue, ctx: &SourceParserContext<'_>) -> Result<PoseSample, String> {
    let candidate = value.get("pose").unwrap_or(value);
    if !candidate.is_object() {
        return Err("pose is not an object".to_string());
    }

    let translation = translation_from_pose_value(candidate);
    let has_translation = translation.is_some();
    let translation = translation.unwrap_or(Translation3 { x: 0.0, y: 0.0, z: 0.0 });

    let rotation_value = candidate.get("rotation").or_else(|| candidate.get("orientation")).unwrap_or(candidate);
    let (quat, euler) = rotation_from_value(rotation_value);
    let has_rotation = quat.is_some() || euler.is_some();

    if !has_translation && !has_rotation {
        return Err("missing pose translation or rotation".to_string());
    }

    let mut pose = pose_from_translation_rotation(translation, quat, euler);
    if source_looks_like_imu(ctx.source) {
        pose = imu_pose_to_viewer_frame(pose);
    }

    Ok(PoseSample { pose, has_translation, has_rotation })
}
