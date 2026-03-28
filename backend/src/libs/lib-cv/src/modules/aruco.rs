pub mod adaptive;
pub mod detect;
pub mod map;
pub mod pose;
pub mod tag;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{Point, Translation3};

#[cfg_attr(feature = "engine", derive(daedalus_macros::DaedalusTypeExpr, daedalus_macros::DaedalusToValue))]
#[cfg_attr(feature = "engine", daedalus(type_key = "cv:aruco_bit_grid"))]
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ArucoBitGrid {
    pub width: u8,
    pub border: u8,
    pub rows: Vec<String>,
}

#[cfg_attr(feature = "engine", derive(daedalus_macros::DaedalusTypeExpr, daedalus_macros::DaedalusToValue))]
#[cfg_attr(feature = "engine", daedalus(type_key = "cv:aruco_detection_2d"))]
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ArucoDetection2D {
    pub id: u32,
    pub rotation: u8,
    pub corners: [Point; 4],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best_distance: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second_distance: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_mismatches: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contrast_range: Option<f32>,
    // Decoder metadata (useful for corner refinement and downstream filtering).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_width: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_width: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bits: Option<ArucoBitGrid>,
}

impl ArucoDetection2D {
    pub fn canonicalize_in_place(&mut self) {
        // Keep corner ordering as a *cyclic shift* of the incoming indexing. Downstream code
        // assumes corners are a CW quad, and uses `rotation` to map image-ordered corners to
        // tag-canonical corners. A non-cyclic permutation here can silently corrupt rotation
        // and break calibration.
        //
        // Upstream quad extraction sorts corners clockwise; here we just rotate so corner 0 is
        // the image top-left (lowest y, then lowest x).
        let mut idx_tl = 0usize;
        for i in 1..4 {
            let a = &self.corners[i];
            let b = &self.corners[idx_tl];
            if a.y < b.y || (a.y == b.y && a.x < b.x) {
                idx_tl = i;
            }
        }
        let ordered = [self.corners[idx_tl], self.corners[(idx_tl + 1) & 3], self.corners[(idx_tl + 2) & 3], self.corners[(idx_tl + 3) & 3]];
        self.corners = ordered;

        // `rotation` is defined relative to the corner indexing. Because we may have rotated the
        // corners (cyclic shift), rotate `rotation` by the same amount so downstream pose estimation
        // sees a consistent (corners, rotation) pair.
        self.rotation = (self.rotation.wrapping_add((idx_tl & 3) as u8)) & 3;
    }

    pub fn canonicalize(mut self) -> Self {
        self.canonicalize_in_place();
        self
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArucoDetectionsFilterMode {
    LeftMost,
    RightMost,
    TopMost,
    BottomMost,
    MiddleMost,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    LargestArea,
    SmallestArea,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArucoDetectionsOrderMode {
    None,
    LargestToSmallest,
    SmallestToLargest,
    TopMost,
    BottomMost,
    LeftMost,
    RightMost,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    CenterMost,
    Crosshair,
}

#[cfg_attr(feature = "engine", derive(daedalus_macros::DaedalusTypeExpr, daedalus_macros::DaedalusToValue))]
#[cfg_attr(feature = "engine", daedalus(type_key = "cv:aruco_detection_quat"))]
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DetectionQuat {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

#[cfg_attr(feature = "engine", derive(daedalus_macros::DaedalusTypeExpr, daedalus_macros::DaedalusToValue))]
#[cfg_attr(feature = "engine", daedalus(type_key = "cv:aruco_detection_pose_rotation"))]
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DetectionPoseRotation {
    pub roll: f64,
    pub pitch: f64,
    pub yaw: f64,
    pub quaternion: DetectionQuat,
}

#[cfg_attr(feature = "engine", derive(daedalus_macros::DaedalusTypeExpr, daedalus_macros::DaedalusToValue))]
#[cfg_attr(feature = "engine", daedalus(type_key = "cv:aruco_detection_pose"))]
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DetectionPose {
    pub id: u32,
    pub translation: Translation3,
    pub rotation: DetectionPoseRotation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reprojection_error_px: Option<f64>,
    pub code_rotation: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bits: Option<ArucoBitGrid>,
    pub tag_size: f64,
    pub pose_method: String,
}

#[cfg_attr(feature = "engine", derive(daedalus_macros::DaedalusTypeExpr, daedalus_macros::DaedalusToValue))]
#[cfg_attr(feature = "engine", daedalus(type_key = "cv:aruco_detection_pose_failure_counts"))]
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DetectionPoseFailureCounts {
    pub invalid: usize,
    pub homography: usize,
    pub decompose: usize,
}

#[cfg_attr(feature = "engine", derive(daedalus_macros::DaedalusTypeExpr, daedalus_macros::DaedalusToValue))]
#[cfg_attr(feature = "engine", daedalus(type_key = "cv:aruco_detection_pose_failure_sample"))]
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DetectionPoseFailureSample {
    pub tag_id: u32,
    pub reason: String,
    pub corners: [Point; 4],
}

#[cfg_attr(feature = "engine", derive(daedalus_macros::DaedalusTypeExpr, daedalus_macros::DaedalusToValue))]
#[cfg_attr(feature = "engine", daedalus(type_key = "cv:aruco_detection_pose_calibration_summary"))]
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DetectionPoseCalibrationSummary {
    pub fx: f64,
    pub fy: f64,
    pub cx: f64,
    pub cy: f64,
}

#[cfg_attr(feature = "engine", derive(daedalus_macros::DaedalusTypeExpr, daedalus_macros::DaedalusToValue))]
#[cfg_attr(feature = "engine", daedalus(type_key = "cv:aruco_detection_pose_stats"))]
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DetectionPoseStats {
    pub input_detections: usize,
    pub output_detections: usize,
    pub pose_method: String,
    pub tag_size: f64,
    pub calibration: DetectionPoseCalibrationSummary,
    pub estimated_intrinsics: bool,
    pub failures: DetectionPoseFailureCounts,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_failure: Option<DetectionPoseFailureSample>,
}

#[cfg_attr(feature = "engine", derive(daedalus_macros::DaedalusTypeExpr, daedalus_macros::DaedalusToValue))]
#[cfg_attr(feature = "engine", daedalus(type_key = "cv:aruco_detection_pose_output"))]
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DetectionPoseOutput {
    pub detections: Vec<DetectionPose>,
    pub stats: DetectionPoseStats,
}

#[cfg(feature = "engine")]
pub mod nodes;

pub use tag::ArucoTagFamilyKind;
pub use tag::dictionary::{ArucoDictionary, ArucoDictionaryKind, aruco_dictionary_from_name};

#[doc(hidden)]
pub(crate) fn compact_runtime_scratch_after_frame() {
    detect::compact_detect_scratch_after_frame();
    detect::compact_decode_scratch_after_frame();
    pose::compact_pose_scratch_after_frame();
    tag::compact_runtime_scratch_after_frame();
    #[cfg(feature = "engine")]
    nodes::compact_runtime_scratch_after_frame();
}

#[doc(hidden)]
pub(crate) fn release_runtime_scratch_on_idle() {
    compact_runtime_scratch_after_frame();
    adaptive::release_runtime_scratch_on_idle();
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArucoMaskMode {
    AdaptiveMean,
    Otsu,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArucoCandidateQuadsMode {
    Robust,
    Fast,
}

/// Return a list of supported ArUco dictionary names.
pub fn list_dictionaries() -> Vec<String> {
    tag::dictionary::list_dictionaries()
}

/// Return a list of supported ArUco tag family labels.
pub fn list_families() -> Vec<String> {
    tag::ArucoTagFamily::available().iter().map(|&s| s.to_string()).collect()
}
