use super::*;

#[path = "overlay_detect.rs"]
mod overlay_detect;
#[path = "overlay_draw.rs"]
mod overlay_draw;

pub(super) use self::overlay_detect::{cv_detect_aruco_detections, cv_detect_aruco_detections_from_contours};
pub(super) use self::overlay_draw::{cv_aruco_overlay, cv_aruco_overlay_quads, cv_aruco_overlay_quads_count, cv_detect_aruco_overlay, cv_overlay_tags_count};

#[derive(Clone, Debug, NodeConfig)]
struct ArucoTagOverlayConfig {
    #[port(default = "apriltag_16h5")]
    dictionary: ArucoDictionaryKind,
    // Override max Hamming distance accepted by the family decoder.
    // A negative value uses the family default.
    #[port(default = -1i64, meta(ui_min = -1, ui_max = 4, ui_step = 1))]
    max_hamming: i64,
    #[port(default = 1i64, meta(ui_min = 1, ui_max = 8, ui_step = 1))]
    downscale: i64,
    #[port(default = 1i64, meta(ui_min = 1, ui_max = 8, ui_step = 1))]
    sample_scale: i64,
    #[port(default = 31i64, meta(ui_min = 3, ui_max = 101, ui_step = 2))]
    threshold_window: i64,
    #[port(default = 10.0f64, meta(ui_min = -50.0, ui_max = 50.0, ui_step = 1.0))]
    threshold_offset: f64,
    // Threshold mode: "adaptive_mean" (default) or "otsu" (fast, lighting-dependent).
    #[port(default = "adaptive_mean")]
    threshold_mode: crate::modules::aruco::ArucoMaskMode,
    #[port(default = true)]
    invert_mask: bool,
    // Gamma correction applied to grayscale before thresholding/decoding. <1 brightens shadows.
    #[port(default = 1.0f64, meta(ui_min = 0.1, ui_max = 5.0, ui_step = 0.1))]
    gamma: f64,
    // Optional blur to reduce mask noise (set to 0 for speed).
    #[port(default = 1.0f64, meta(ui_min = 0.0, ui_max = 10.0, ui_step = 0.1))]
    blur_sigma: f64,
    // For robustness we can try both polarities and keep whichever yields more markers.
    // Disable for speed if lighting is stable.
    #[port(default = true)]
    try_opposite_polarity: bool,
    // For throughput tuning, allow skipping per-tag overlay drawing.
    #[port(default = true)]
    draw_markers: bool,
    // Control the on-frame "TAGS: N" HUD overlay.
    #[port(default = true)]
    draw_hud: bool,
    // Run the full (expensive) detection every N frames and reuse the last successful
    // detections in-between. Set to 1 to disable caching.
    #[port(default = 1i64, meta(ui_min = 1, ui_max = 120, ui_step = 1))]
    detect_every_n: i64,
    #[port(default = 0.03f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01))]
    epsilon: f64,
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 180.0, ui_step = 1.0))]
    min_angle_deg: f64,
    #[port(default = 180.0f64, meta(ui_min = 0.0, ui_max = 180.0, ui_step = 1.0))]
    max_angle_deg: f64,
    #[port(default = 10.0f64, meta(ui_min = 0.1, ui_max = 10.0, ui_step = 0.1))]
    max_side_cv: f64,
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 1000000.0, ui_step = 1000.0))]
    min_area: f64,
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 1000000.0, ui_step = 1000.0))]
    max_area: f64,
}
