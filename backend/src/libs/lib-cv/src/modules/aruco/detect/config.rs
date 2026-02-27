use super::*;

#[derive(Debug, Clone, Copy)]
pub struct ArucoTagDecodeConfig {
    pub min_warped_patch_contrast_range: u8,
    pub warp_fallback_on_decode_fail: bool,
    pub warp_fallback_max_hamming_extra: u32,
    pub warp_fallback_border_slack: usize,
    pub warp_fallback_on_low_contrast: bool,
    pub warp_min_sample_scale: u32,
    pub min_quad_side_px: f32,
    pub min_quiet_zone_delta: f32,
    pub quiet_zone_texture_penalty: f32,
    pub verify_warp_min_best_distance: u32,
    pub verify_warp_only_if_border_mismatch: bool,
    pub verify_warp_reject_on_fail: bool,
    pub min_decode_score: f32,
    pub cell_sample_grid: u8,
    pub cell_sample_margin: f32,
    pub cell_decode: ArucoTagDecodeTuning,
}

impl Default for ArucoTagDecodeConfig {
    fn default() -> Self {
        Self {
            // Historical default: 24 (stored as 8 in env-based version).
            min_warped_patch_contrast_range: 8,
            // OpenCV default path does not use warp fallback; keep this off for parity.
            warp_fallback_on_decode_fail: false,
            warp_fallback_max_hamming_extra: 0,
            warp_fallback_border_slack: 0,
            warp_fallback_on_low_contrast: false,
            warp_min_sample_scale: 4,
            min_quad_side_px: 0.0,
            min_quiet_zone_delta: 0.0,
            // Conservative default (used to only lightly downweight textured backgrounds).
            quiet_zone_texture_penalty: 12.0,
            verify_warp_min_best_distance: 99,
            verify_warp_only_if_border_mismatch: true,
            verify_warp_reject_on_fail: false,
            min_decode_score: -1.0,
            cell_sample_grid: 0,
            // OpenCV default: perspectiveRemoveIgnoredMarginPerCell = 0.13.
            cell_sample_margin: 0.13,
            cell_decode: ArucoTagDecodeTuning::default(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ArucoDecodeConfig {
    pub min_warped_patch_contrast_range: u8,
    pub min_cell_means_contrast_range: f32,
    pub min_quad_side_px: f32,
    /// Minimum required difference between the mean intensity in a 1-cell quiet-zone ring
    /// just outside the marker border and the mean intensity of the marker border ring.
    ///
    /// This is a detector-side false-positive guard for dense ChArUco boards: plain chessboard
    /// squares can look like valid quads and may accidentally decode to an in-range ArUco ID when
    /// sampling bleeds into the surrounding background.
    pub min_quiet_zone_delta: f32,
    pub max_border_error_rate: f32,
    pub error_correction_rate: f32,
    pub cell_sample_grid: u8,
    pub cell_sample_margin: f32,
    pub min_decode_score: f32,
    pub min_hamming_margin: u32,
    pub min_hamming_margin_min_dist: u32,
    pub min_hamming_margin_only_if_border_mismatch: bool,
    pub min_bit_delta: f32,
}

impl Default for ArucoDecodeConfig {
    fn default() -> Self {
        Self {
            min_warped_patch_contrast_range: 5,
            min_cell_means_contrast_range: 5.0,
            min_quad_side_px: 0.0,
            min_quiet_zone_delta: 0.0,
            max_border_error_rate: 0.35,
            error_correction_rate: 0.6,
            cell_sample_grid: 0,
            cell_sample_margin: 0.13,
            min_decode_score: -1.0,
            min_hamming_margin: 0,
            min_hamming_margin_min_dist: 0,
            min_hamming_margin_only_if_border_mismatch: false,
            min_bit_delta: 0.0,
        }
    }
}

#[cfg_attr(feature = "engine", derive(daedalus_macros::DaedalusTypeExpr, daedalus_macros::DaedalusToValue))]
#[cfg_attr(feature = "engine", daedalus(type_key = "cv:camera_calibration"))]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CameraCalibration {
    pub fx: f32,
    pub fy: f32,
    pub cx: f32,
    pub cy: f32,
    pub k1: f32,
    pub k2: f32,
    pub p1: f32,
    pub p2: f32,
    pub k3: f32,
    pub undistort_iters: u8,
    #[serde(default)]
    pub lens_model: LensModel,
}

impl Default for CameraCalibration {
    fn default() -> Self {
        Self { fx: 0.0, fy: 0.0, cx: 0.0, cy: 0.0, k1: 0.0, k2: 0.0, p1: 0.0, p2: 0.0, k3: 0.0, undistort_iters: 5, lens_model: LensModel::Pinhole }
    }
}
/// Geometric constraints used when filtering candidate quads.
#[derive(Debug, Clone)]
pub struct ArucoTagDetectorConfig {
    pub epsilon: f32,
    pub min_area: f32,
    pub max_area: Option<f32>,
    pub min_angle_deg: f32,
    pub max_angle_deg: f32,
    /// Maximum allowed coefficient-of-variation for quad edge lengths.
    /// Lower values reject more perspective-distorted candidates.
    pub max_side_cv: f32,
    /// Maximum allowed ratio between longest and shortest quad edges.
    /// A value <= 0 disables this check.
    pub max_side_ratio: f32,
    /// Maximum allowed ratio between the two diagonals (squareness check).
    /// A value <= 0 disables this check.
    pub max_diag_ratio: f32,
    // Precomputed cosine bounds for the (tolerance-expanded) angle range check.
    // For 0..=180deg, cos is monotonic decreasing, so an angle in [lower, upper]
    // is equivalent to cos(theta) in [cos(upper), cos(lower)].
    pub angle_cos_min: f32,
    pub angle_cos_max: f32,
}

impl Default for ArucoTagDetectorConfig {
    fn default() -> Self {
        Self {
            epsilon: 2.5,
            min_area: 500.0,
            max_area: None,
            min_angle_deg: 45.0,
            max_angle_deg: 135.0,
            max_side_cv: 1.2,
            max_side_ratio: 0.0,
            max_diag_ratio: 0.0,
            angle_cos_min: -1.0,
            angle_cos_max: 1.0,
        }
    }
}

impl ArucoTagDetectorConfig {
    pub fn with_angle_cos_bounds(mut self) -> Self {
        // Match historical tolerance behavior in `angles_within_range`.
        let tolerance = 7.5_f32;
        let lower = (self.min_angle_deg - tolerance).max(0.0);
        let upper = (self.max_angle_deg + tolerance).min(180.0).max(lower + f32::EPSILON);
        self.angle_cos_max = lower.to_radians().cos();
        self.angle_cos_min = upper.to_radians().cos();
        self
    }
}
