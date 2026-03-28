use super::*;
use std::mem::size_of;
use std::sync::{Mutex, OnceLock};

const DETECT_RETAIN_POINT_CAP: usize = 4 * 1024;
const DETECT_RETAIN_APPROX_CAP: usize = 1024;
const DECODE_RETAIN_WARPED_BUF_CAP: usize = 256 * 256;
const DECODE_RETAIN_SAMPLE_POS_CAP: usize = 1024;

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

fn detect_scratch() -> &'static Mutex<DetectScratch> {
    static DETECT_SCRATCH: OnceLock<Mutex<DetectScratch>> = OnceLock::new();
    DETECT_SCRATCH.get_or_init(|| Mutex::new(DetectScratch::default()))
}

fn decode_scratch() -> &'static Mutex<DecodeScratch> {
    static DECODE_SCRATCH: OnceLock<Mutex<DecodeScratch>> = OnceLock::new();
    DECODE_SCRATCH.get_or_init(|| Mutex::new(DecodeScratch::default()))
}

pub(super) fn with_detect_scratch<R>(f: impl FnOnce(&mut DetectScratch) -> R) -> R {
    let mut scratch = detect_scratch().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut scratch)
}

pub(super) fn with_decode_scratch<R>(f: impl FnOnce(&mut DecodeScratch) -> R) -> R {
    let mut scratch = decode_scratch().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut scratch)
}

#[inline(always)]
fn trim_retained_vec<T>(vec: &mut Vec<T>, retain_cap: usize) {
    vec.clear();
    if vec.capacity() > retain_cap {
        vec.shrink_to(retain_cap);
    }
}

fn detect_scratch_bytes(scratch: &DetectScratch) -> usize {
    scratch.distances.capacity() * size_of::<(f32, Point<f32>)>()
        + scratch.trimmed.capacity() * size_of::<Point<f32>>()
        + scratch.inliers.capacity() * size_of::<Point<f32>>()
        + scratch.best_inliers.capacity() * size_of::<Point<f32>>()
        + scratch.top_pts.capacity() * size_of::<Point<f32>>()
        + scratch.bottom_pts.capacity() * size_of::<Point<f32>>()
        + scratch.left_pts.capacity() * size_of::<Point<f32>>()
        + scratch.right_pts.capacity() * size_of::<Point<f32>>()
        + scratch.approx.capacity() * size_of::<Point<f32>>()
        + scratch.downsampled.capacity() * size_of::<Point<f32>>()
}

fn decode_scratch_bytes(scratch: &DecodeScratch) -> usize {
    scratch.warped_buf.capacity() * size_of::<u8>() + scratch.sample_positions.capacity() * size_of::<(f32, f32)>()
}

pub(super) fn report_detect_scratch() {
    with_detect_scratch(|scratch| {
        crate::diagnostics::report_scratch_high_water("aruco.detect_scratch", detect_scratch_bytes(&scratch));
    });
}

pub(crate) fn compact_detect_scratch_after_frame() {
    with_detect_scratch(|scratch| {
        trim_retained_vec(&mut scratch.distances, DETECT_RETAIN_POINT_CAP);
        trim_retained_vec(&mut scratch.trimmed, DETECT_RETAIN_POINT_CAP);
        trim_retained_vec(&mut scratch.inliers, DETECT_RETAIN_POINT_CAP);
        trim_retained_vec(&mut scratch.best_inliers, DETECT_RETAIN_POINT_CAP);
        trim_retained_vec(&mut scratch.top_pts, DETECT_RETAIN_POINT_CAP);
        trim_retained_vec(&mut scratch.bottom_pts, DETECT_RETAIN_POINT_CAP);
        trim_retained_vec(&mut scratch.left_pts, DETECT_RETAIN_POINT_CAP);
        trim_retained_vec(&mut scratch.right_pts, DETECT_RETAIN_POINT_CAP);
        trim_retained_vec(&mut scratch.approx, DETECT_RETAIN_APPROX_CAP);
        trim_retained_vec(&mut scratch.downsampled, DETECT_RETAIN_POINT_CAP);
    });
}

pub(crate) fn compact_decode_scratch_after_frame() {
    with_decode_scratch(|scratch| {
        if scratch.warped_buf.capacity() > DECODE_RETAIN_WARPED_BUF_CAP {
            scratch.warped_buf.clear();
            scratch.warped_buf.shrink_to(DECODE_RETAIN_WARPED_BUF_CAP);
            scratch.warped_side = 0;
        }
        if scratch.sample_positions.capacity() > DECODE_RETAIN_SAMPLE_POS_CAP {
            scratch.sample_positions.clear();
            scratch.sample_positions.shrink_to(DECODE_RETAIN_SAMPLE_POS_CAP);
            scratch.sample_positions_total_width = 0;
            scratch.sample_positions_grid = 0;
            scratch.sample_positions_margin_bits = 0;
            scratch.sample_positions_per_cell = 0;
        }
    });
}

pub(super) fn report_decode_scratch(scratch: &DecodeScratch) {
    crate::diagnostics::report_scratch_high_water("aruco.decode_scratch", decode_scratch_bytes(scratch));
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
