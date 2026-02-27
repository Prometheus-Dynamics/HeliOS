use super::*;

#[derive(Clone)]
pub(super) struct ArucoDetectionF32 {
    pub(super) id: u32,
    pub(super) rotation: u8,
    pub(super) border_width: u8,
    pub(super) data_width: u8,
    pub(super) corners: Option<Vec<Point<f32>>>,
    pub(super) score: Option<f32>,
    pub(super) best_distance: Option<u32>,
    pub(super) second_distance: Option<u32>,
    pub(super) border_mismatches: Option<usize>,
    pub(super) contrast_range: Option<f32>,
}

#[derive(Default)]
pub(super) struct DetectScratch {
    pub(super) distances: Vec<(f32, Point<f32>)>,
    pub(super) trimmed: Vec<Point<f32>>,
    pub(super) inliers: Vec<Point<f32>>,
    pub(super) best_inliers: Vec<Point<f32>>,
    pub(super) top_pts: Vec<Point<f32>>,
    pub(super) bottom_pts: Vec<Point<f32>>,
    pub(super) left_pts: Vec<Point<f32>>,
    pub(super) right_pts: Vec<Point<f32>>,
    pub(super) approx: Vec<Point<f32>>,
    pub(super) downsampled: Vec<Point<f32>>,
}

pub(super) struct DecodeScratch {
    pub(super) warped_side: u32,
    pub(super) warped_buf: Vec<u8>,
    pub(super) cell_means: [f32; 100],
    pub(super) sample_positions: Vec<(f32, f32)>,
    pub(super) sample_positions_total_width: usize,
    pub(super) sample_positions_grid: u8,
    pub(super) sample_positions_margin_bits: u32,
    pub(super) sample_positions_per_cell: usize,
}

impl Default for DecodeScratch {
    fn default() -> Self {
        Self {
            warped_side: 0,
            warped_buf: Vec::new(),
            cell_means: [0.0; 100],
            sample_positions: Vec::new(),
            sample_positions_total_width: 0,
            sample_positions_grid: 0,
            sample_positions_margin_bits: 0,
            sample_positions_per_cell: 0,
        }
    }
}

thread_local! {
    pub(super) static DETECT_SCRATCH: RefCell<DetectScratch> = RefCell::new(DetectScratch::default());
    pub(super) static DECODE_SCRATCH: RefCell<DecodeScratch> = RefCell::new(DecodeScratch::default());
}

pub(super) fn marker_f32_to_detection_2d(marker: ArucoDetectionF32, bits: Option<ArucoBitGrid>) -> Option<ArucoDetection2D> {
    let corners = marker.corners?;
    if corners.len() < 4 {
        return None;
    }
    Some(
        ArucoDetection2D {
            id: marker.id,
            rotation: marker.rotation,
            corners: [
                crate::Point { x: corners[0].x as f64, y: corners[0].y as f64 },
                crate::Point { x: corners[1].x as f64, y: corners[1].y as f64 },
                crate::Point { x: corners[2].x as f64, y: corners[2].y as f64 },
                crate::Point { x: corners[3].x as f64, y: corners[3].y as f64 },
            ],
            score: marker.score,
            best_distance: marker.best_distance,
            second_distance: marker.second_distance,
            border_mismatches: marker.border_mismatches,
            contrast_range: marker.contrast_range,
            border_width: Some(marker.border_width),
            data_width: Some(marker.data_width),
            bits,
        }
        .canonicalize(),
    )
}
