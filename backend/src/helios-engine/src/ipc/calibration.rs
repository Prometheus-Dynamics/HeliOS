use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::JsonWire;
use lib_cv::modules::calibration::LensModel;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationBoard {
    pub squares_x: u32,
    pub squares_y: u32,
    pub square_size: f64,
    pub marker_size: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dictionary: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationSolveConfig {
    #[serde(default = "default_min_views")]
    pub min_views: u32,
    #[serde(default = "default_min_points_per_view")]
    pub min_points_per_view: u32,
    #[serde(default = "default_refine_distortion")]
    pub refine_distortion: bool,
    #[serde(default = "default_undistort_iters")]
    pub undistort_iters: u8,
    #[serde(default = "default_refine_undistort_iters")]
    pub refine_undistort_iters: u8,
    #[serde(default = "default_lens_model")]
    pub lens_model: LensModel,
}

impl Default for CalibrationSolveConfig {
    fn default() -> Self {
        Self {
            min_views: default_min_views(),
            min_points_per_view: default_min_points_per_view(),
            refine_distortion: default_refine_distortion(),
            undistort_iters: default_undistort_iters(),
            refine_undistort_iters: default_refine_undistort_iters(),
            lens_model: default_lens_model(),
        }
    }
}

fn default_min_views() -> u32 {
    2
}

fn default_min_points_per_view() -> u32 {
    12
}

fn default_refine_distortion() -> bool {
    true
}

fn default_undistort_iters() -> u8 {
    5
}

fn default_refine_undistort_iters() -> u8 {
    8
}

fn default_lens_model() -> LensModel {
    LensModel::Pinhole
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationImage {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationSolveRequest {
    pub images: Vec<CalibrationImage>,
    pub board: CalibrationBoard,
    #[serde(default)]
    pub graph: Option<JsonWire>,
    #[serde(default)]
    pub graph_id: Option<Uuid>,
    #[serde(default)]
    pub graph_template_id: Option<String>,
    #[serde(default)]
    pub detections_port: Option<String>,
    #[serde(default)]
    pub overlay_port: Option<String>,
    #[serde(default)]
    pub include_overlays: bool,
    #[serde(default)]
    pub overlay_output_dir: Option<String>,
    /// Optional override for how overlays are written (e.g. "copy", "input", "graph").
    /// This avoids relying on process-global env vars for per-request behavior.
    #[serde(default)]
    pub overlay_save_mode: Option<String>,
    #[serde(default)]
    pub config: CalibrationSolveConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationSolveDebugView {
    pub image: String,
    pub tags_detected: u32,
    pub points_detected: u32,
    pub used: bool,
    pub coverage_ratio: f64,
    #[serde(default)]
    pub raw_tags_detected: u32,
    #[serde(default)]
    pub raw_ids: Vec<u32>,
    #[serde(default)]
    pub raw_duplicate_ids: Vec<u32>,
    #[serde(default)]
    pub raw_out_of_range_ids: Vec<u32>,
    #[serde(default)]
    pub overlay_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct CalibrationSolveResponse {
    pub calibration: super::StreamCalibration,
    pub reprojection_error_px: f64,
    pub views_used: u32,
    pub points_used: u32,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub debug_views: Vec<CalibrationSolveDebugView>,
}
