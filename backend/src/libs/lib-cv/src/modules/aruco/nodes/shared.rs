use super::*;
use std::mem::size_of;

pub(super) type Quad = [Point; 4];

const NODE_RETAIN_POINT_CAP: usize = 2 * 1024;
const CANDIDATE_RETAIN_POINT_CAP: usize = 4 * 1024;
const CANDIDATE_RETAIN_QUAD_CAP: usize = 512;
const CANDIDATE_RETAIN_GROUP_CAP: usize = 128;
const DECODE_RETAIN_QUAD_CAP: usize = 512;
const INTEGRAL_RETAIN_CAP: usize = 1_100_000;

pub(super) fn aruco_include_bits_enabled() -> bool {
    static INCLUDE_BITS: LazyLock<bool> = LazyLock::new(|| {
        std::env::var("HELIOS_ARUCO_INCLUDE_BITS")
            .ok()
            .map(|raw| {
                let raw = raw.trim();
                raw.eq_ignore_ascii_case("1") || raw.eq_ignore_ascii_case("true") || raw.eq_ignore_ascii_case("yes") || raw.eq_ignore_ascii_case("on")
            })
            .unwrap_or(false)
    });
    *INCLUDE_BITS
}

thread_local! {
    pub(super) static INTEGRAL_SCRATCH: RefCell<Vec<u32>> = const { RefCell::new(Vec::new()) };
    pub(super) static ARUCO_NODE_SCRATCH: RefCell<NodeDetectScratch> = RefCell::new(NodeDetectScratch::default());
    pub(super) static CANDIDATE_QUAD_SCRATCH: RefCell<CandidateQuadScratch> = RefCell::new(CandidateQuadScratch::default());
    pub(super) static DECODE_QUAD_SCRATCH: RefCell<DecodeQuadScratch> = RefCell::new(DecodeQuadScratch::default());
    pub(super) static DECODE_FAMILY_SCRATCH: RefCell<DecodeFamilyScratch> = RefCell::new(DecodeFamilyScratch::default());
    pub(super) static OVERLAY_ID_CACHE: RefCell<HashMap<u32, RgbaImage>> = RefCell::new(HashMap::new());
}

#[derive(Default)]
pub(super) struct NodeDetectScratch {
    pub(super) distances: Vec<(f32, CvPoint<f32>)>,
    pub(super) trimmed: Vec<CvPoint<f32>>,
    pub(super) inliers: Vec<CvPoint<f32>>,
    pub(super) best_inliers: Vec<CvPoint<f32>>,
    pub(super) edge_points: [Vec<CvPoint<f32>>; 4],
}

#[derive(Default)]
pub(super) struct CandidateQuadScratch {
    pub(super) pts: Vec<CvPoint<f32>>,
    pub(super) quads: Vec<[CvPoint<f32>; 4]>,
    pub(super) quad_centers: Vec<CvPoint<f32>>,
    pub(super) ordered_centers: Vec<CvPoint<f32>>,
    pub(super) quad_perimeters: Vec<f32>,
    pub(super) order: Vec<usize>,
    pub(super) min_marker_dist_sq: Vec<f32>,
    pub(super) group_id: Vec<isize>,
    pub(super) grouped: Vec<Vec<usize>>,
    pub(super) is_selected: Vec<bool>,
    pub(super) keep_sorted: Vec<bool>,
    pub(super) filtered: Vec<[CvPoint<f32>; 4]>,
    pub(super) filtered_quads: Vec<Quad>,
}

#[derive(Default)]
pub(super) struct DecodeQuadScratch {
    pub(super) quads: Vec<[CvPoint<f32>; 4]>,
}

pub(super) struct DecodeFamilyScratch {
    pub(super) kind: Option<ArucoTagFamilyKind>,
    pub(super) max_hamming: i64,
    pub(super) border_error_divisor: i64,
    pub(super) family: crate::modules::aruco::tag::ArucoTagFamily,
}

impl Default for DecodeFamilyScratch {
    fn default() -> Self {
        Self { kind: None, max_hamming: i64::MIN, border_error_divisor: i64::MIN, family: crate::modules::aruco::tag::ArucoTagFamily::new_family16h5() }
    }
}

fn candidate_quad_scratch_bytes(scratch: &CandidateQuadScratch) -> usize {
    let grouped_inner = scratch.grouped.iter().map(|group| group.capacity() * size_of::<usize>()).sum::<usize>();
    scratch.pts.capacity() * size_of::<CvPoint<f32>>()
        + scratch.quads.capacity() * size_of::<[CvPoint<f32>; 4]>()
        + scratch.quad_centers.capacity() * size_of::<CvPoint<f32>>()
        + scratch.ordered_centers.capacity() * size_of::<CvPoint<f32>>()
        + scratch.quad_perimeters.capacity() * size_of::<f32>()
        + scratch.order.capacity() * size_of::<usize>()
        + scratch.min_marker_dist_sq.capacity() * size_of::<f32>()
        + scratch.group_id.capacity() * size_of::<isize>()
        + scratch.grouped.capacity() * size_of::<Vec<usize>>()
        + grouped_inner
        + scratch.is_selected.capacity() * size_of::<bool>()
        + scratch.keep_sorted.capacity() * size_of::<bool>()
        + scratch.filtered.capacity() * size_of::<[CvPoint<f32>; 4]>()
        + scratch.filtered_quads.capacity() * size_of::<Quad>()
}

fn node_detect_scratch_bytes(scratch: &NodeDetectScratch) -> usize {
    scratch.distances.capacity() * size_of::<(f32, CvPoint<f32>)>()
        + scratch.trimmed.capacity() * size_of::<CvPoint<f32>>()
        + scratch.inliers.capacity() * size_of::<CvPoint<f32>>()
        + scratch.best_inliers.capacity() * size_of::<CvPoint<f32>>()
        + scratch.edge_points.iter().map(|points| points.capacity() * size_of::<CvPoint<f32>>()).sum::<usize>()
}

#[inline(always)]
fn trim_retained_vec<T>(vec: &mut Vec<T>, retain_cap: usize) {
    vec.clear();
    if vec.capacity() > retain_cap {
        vec.shrink_to(retain_cap);
    }
}

fn compact_candidate_quad_scratch_after_frame(scratch: &mut CandidateQuadScratch) {
    trim_retained_vec(&mut scratch.pts, CANDIDATE_RETAIN_POINT_CAP);
    trim_retained_vec(&mut scratch.quads, CANDIDATE_RETAIN_QUAD_CAP);
    trim_retained_vec(&mut scratch.quad_centers, CANDIDATE_RETAIN_QUAD_CAP);
    trim_retained_vec(&mut scratch.ordered_centers, CANDIDATE_RETAIN_QUAD_CAP);
    trim_retained_vec(&mut scratch.quad_perimeters, CANDIDATE_RETAIN_QUAD_CAP);
    trim_retained_vec(&mut scratch.order, CANDIDATE_RETAIN_QUAD_CAP);
    trim_retained_vec(&mut scratch.min_marker_dist_sq, CANDIDATE_RETAIN_QUAD_CAP);
    trim_retained_vec(&mut scratch.group_id, CANDIDATE_RETAIN_QUAD_CAP);
    for group in &mut scratch.grouped {
        trim_retained_vec(group, CANDIDATE_RETAIN_GROUP_CAP);
    }
    trim_retained_vec(&mut scratch.grouped, CANDIDATE_RETAIN_GROUP_CAP);
    trim_retained_vec(&mut scratch.is_selected, CANDIDATE_RETAIN_QUAD_CAP);
    trim_retained_vec(&mut scratch.keep_sorted, CANDIDATE_RETAIN_QUAD_CAP);
    trim_retained_vec(&mut scratch.filtered, CANDIDATE_RETAIN_QUAD_CAP);
    trim_retained_vec(&mut scratch.filtered_quads, CANDIDATE_RETAIN_QUAD_CAP);
}

pub(super) fn compact_shared_scratch_after_frame() {
    INTEGRAL_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        trim_retained_vec(&mut scratch, INTEGRAL_RETAIN_CAP);
    });
    ARUCO_NODE_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        trim_retained_vec(&mut scratch.distances, NODE_RETAIN_POINT_CAP);
        trim_retained_vec(&mut scratch.trimmed, NODE_RETAIN_POINT_CAP);
        trim_retained_vec(&mut scratch.inliers, NODE_RETAIN_POINT_CAP);
        trim_retained_vec(&mut scratch.best_inliers, NODE_RETAIN_POINT_CAP);
        for edge in &mut scratch.edge_points {
            trim_retained_vec(edge, NODE_RETAIN_POINT_CAP);
        }
    });
    CANDIDATE_QUAD_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        compact_candidate_quad_scratch_after_frame(&mut scratch);
    });
    DECODE_QUAD_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        trim_retained_vec(&mut scratch.quads, DECODE_RETAIN_QUAD_CAP);
    });
}

pub(super) struct ArucoDecodeFrameScratchGuard;

impl ArucoDecodeFrameScratchGuard {
    pub(super) fn new() -> Self {
        Self
    }
}

impl Drop for ArucoDecodeFrameScratchGuard {
    fn drop(&mut self) {
        compact_shared_scratch_after_frame();
        crate::modules::aruco::detect::compact_detect_scratch_after_frame();
        crate::modules::aruco::detect::compact_decode_scratch_after_frame();
        crate::modules::contour::douglas_peucker::compact_rdp_scratch_after_frame();
    }
}

pub(super) fn report_candidate_quad_scratch() {
    CANDIDATE_QUAD_SCRATCH.with(|scratch| {
        let scratch = scratch.borrow();
        crate::diagnostics::report_scratch_high_water("aruco.candidate_quad_scratch", candidate_quad_scratch_bytes(&scratch));
    });
}

pub(super) fn report_node_detect_scratch() {
    ARUCO_NODE_SCRATCH.with(|scratch| {
        let scratch = scratch.borrow();
        crate::diagnostics::report_scratch_high_water("aruco.node_detect_scratch", node_detect_scratch_bytes(&scratch));
    });
}

pub(super) fn report_overlay_id_cache_bytes(bytes: usize) {
    crate::diagnostics::report_scratch_high_water("aruco.overlay_id_cache", bytes);
}

pub(super) fn expect_cpu_frame(frame: Compute<DynamicImage>, label: &str, exec_ctx: Option<&ExecutionContext>) -> Result<DynamicImage, NodeError> {
    #[cfg(feature = "gpu")]
    {
        match frame {
            Compute::Cpu(img) => Ok(img),
            Compute::Gpu(handle) => {
                let ctx = exec_ctx.and_then(|ctx| ctx.gpu.as_ref()).ok_or_else(|| NodeError::Handler(format!("{label}: gpu payload missing context")))?;
                let bytes = ctx.read_texture(&handle).map_err(|e| NodeError::Handler(format!("{label}: {e}")))?;
                let rgba = RgbaImage::from_raw(handle.width, handle.height, bytes).ok_or_else(|| NodeError::Handler(format!("{label}: invalid image dimensions")))?;
                Ok(DynamicImage::ImageRgba8(rgba))
            }
        }
    }
    #[cfg(not(feature = "gpu"))]
    {
        let _ = exec_ctx;
        match frame {
            Compute::Cpu(img) => Ok(img),
            Compute::Gpu(_) => Err(NodeError::Handler(format!("{label}: gpu payload unsupported (insert cpu convert)"))),
        }
    }
}

pub(super) fn with_integral_scratch<R>(len: usize, f: impl FnOnce(&mut [u32]) -> R) -> R {
    INTEGRAL_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        if scratch.len() != len {
            scratch.resize(len, 0);
            crate::diagnostics::report_scratch_high_water("aruco.integral_scratch", scratch.capacity() * size_of::<u32>());
        } else {
            scratch.fill(0);
        }
        f(&mut scratch)
    })
}

#[derive(Clone, Debug, NodeConfig)]
pub(super) struct ArucoTagDecodeTuningConfig {
    #[port(default = 8i64, meta(ui_min = 0, ui_max = 255, ui_step = 1))]
    pub(super) min_warped_patch_contrast_range: i64,
    #[port(default = true)]
    pub(super) warp_fallback_on_decode_fail: bool,
    #[port(default = 2i64, meta(ui_min = 0, ui_max = 32, ui_step = 1))]
    pub(super) warp_fallback_max_hamming_extra: i64,
    #[port(default = 6i64, meta(ui_min = 0, ui_max = 32, ui_step = 1))]
    pub(super) warp_fallback_border_slack: i64,
    #[port(default = true)]
    pub(super) warp_fallback_on_low_contrast: bool,
    #[port(default = 4i64, meta(ui_min = 1, ui_max = 32, ui_step = 1))]
    pub(super) warp_min_sample_scale: i64,
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 200.0, ui_step = 0.5))]
    pub(super) min_quad_side_px: f64,
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 64.0, ui_step = 0.5))]
    pub(super) min_quiet_zone_delta: f64,
    #[port(default = 12.0f64, meta(ui_min = 0.0, ui_max = 64.0, ui_step = 0.5))]
    pub(super) quiet_zone_texture_penalty: f64,
    #[port(default = 99i64, meta(ui_min = 0, ui_max = 99, ui_step = 1))]
    pub(super) verify_warp_min_best_distance: i64,
    #[port(default = true)]
    pub(super) verify_warp_only_if_border_mismatch: bool,
    #[port(default = false)]
    pub(super) verify_warp_reject_on_fail: bool,
    #[port(default = -1.0f64, meta(ui_min = -100.0, ui_max = 100.0, ui_step = 1.0))]
    pub(super) min_decode_score: f64,
    #[port(default = 0i64, meta(ui_min = 0, ui_max = 7, ui_step = 1))]
    pub(super) cell_sample_grid: i64,
    #[port(default = 0.25f64, meta(ui_min = 0.0, ui_max = 0.45, ui_step = 0.01))]
    pub(super) cell_sample_margin: f64,
    #[port(default = 10.0f64, meta(ui_min = 0.0, ui_max = 255.0, ui_step = 1.0))]
    pub(super) min_cell_means_contrast_range: f64,
    #[port(default = 2i64, meta(ui_min = 0, ui_max = 32, ui_step = 1))]
    pub(super) min_hamming_margin: i64,
    #[port(default = 1i64, meta(ui_min = 0, ui_max = 32, ui_step = 1))]
    pub(super) min_hamming_margin_min_dist: i64,
    #[port(default = false)]
    pub(super) min_hamming_margin_only_if_border_mismatch: bool,
    #[port(default = 2.0f64, meta(ui_min = 0.0, ui_max = 255.0, ui_step = 1.0))]
    pub(super) min_bit_delta: f64,
}

pub(super) fn decode_tuning_to_config(cfg: &ArucoTagDecodeTuningConfig) -> ArucoTagDecodeConfig {
    ArucoTagDecodeConfig {
        min_warped_patch_contrast_range: u8::try_from(cfg.min_warped_patch_contrast_range.clamp(0, 255)).unwrap_or(8),
        warp_fallback_on_decode_fail: cfg.warp_fallback_on_decode_fail,
        warp_fallback_max_hamming_extra: u32::try_from(cfg.warp_fallback_max_hamming_extra.max(0)).unwrap_or(2),
        warp_fallback_border_slack: usize::try_from(cfg.warp_fallback_border_slack.max(0)).unwrap_or(6),
        warp_fallback_on_low_contrast: cfg.warp_fallback_on_low_contrast,
        warp_min_sample_scale: u32::try_from(cfg.warp_min_sample_scale.clamp(1, 32)).unwrap_or(4),
        min_quad_side_px: cfg.min_quad_side_px.max(0.0) as f32,
        min_quiet_zone_delta: cfg.min_quiet_zone_delta.max(0.0) as f32,
        quiet_zone_texture_penalty: cfg.quiet_zone_texture_penalty.max(0.0) as f32,
        verify_warp_min_best_distance: u32::try_from(cfg.verify_warp_min_best_distance.max(0)).unwrap_or(99),
        verify_warp_only_if_border_mismatch: cfg.verify_warp_only_if_border_mismatch,
        verify_warp_reject_on_fail: cfg.verify_warp_reject_on_fail,
        min_decode_score: cfg.min_decode_score as f32,
        cell_sample_grid: u8::try_from(cfg.cell_sample_grid.clamp(0, 7)).unwrap_or(0),
        cell_sample_margin: cfg.cell_sample_margin.clamp(0.0, 0.45) as f32,
        cell_decode: crate::modules::aruco::tag::ArucoTagDecodeTuning {
            min_cell_means_contrast_range: cfg.min_cell_means_contrast_range.clamp(0.0, 255.0) as f32,
            min_hamming_margin: u32::try_from(cfg.min_hamming_margin.max(0)).unwrap_or(0).min(32),
            min_hamming_margin_min_dist: u32::try_from(cfg.min_hamming_margin_min_dist.max(0)).unwrap_or(0).min(32),
            min_hamming_margin_only_if_border_mismatch: cfg.min_hamming_margin_only_if_border_mismatch,
            min_bit_delta: cfg.min_bit_delta.clamp(0.0, 255.0) as f32,
        },
    }
}

pub(super) fn decode_tuning_to_aruco_config(cfg: &ArucoTagDecodeTuningConfig) -> ArucoDecodeConfig {
    let mut out = ArucoDecodeConfig::default();
    out.min_warped_patch_contrast_range = u8::try_from(cfg.min_warped_patch_contrast_range.clamp(0, 255)).unwrap_or(out.min_warped_patch_contrast_range);
    out.min_cell_means_contrast_range = cfg.min_cell_means_contrast_range.clamp(0.0, 255.0) as f32;
    out.min_quad_side_px = cfg.min_quad_side_px.max(0.0) as f32;
    out.min_quiet_zone_delta = cfg.min_quiet_zone_delta.max(0.0) as f32;
    out.cell_sample_grid = u8::try_from(cfg.cell_sample_grid.clamp(0, 7)).unwrap_or(out.cell_sample_grid);
    out.cell_sample_margin = cfg.cell_sample_margin.clamp(0.0, 0.45) as f32;
    out.min_decode_score = cfg.min_decode_score as f32;
    out.min_hamming_margin = u32::try_from(cfg.min_hamming_margin.max(0)).unwrap_or(0).min(32);
    out.min_hamming_margin_min_dist = u32::try_from(cfg.min_hamming_margin_min_dist.max(0)).unwrap_or(0).min(32);
    out.min_hamming_margin_only_if_border_mismatch = cfg.min_hamming_margin_only_if_border_mismatch;
    out.min_bit_delta = cfg.min_bit_delta.clamp(0.0, 255.0) as f32;
    out
}

fn detection_in_frame(det: &ArucoDetection2D, width: u32, height: u32) -> bool {
    #[inline(always)]
    fn orient(a: &Point, b: &Point, c: &Point) -> f64 {
        (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
    }

    #[inline(always)]
    fn proper_segment_intersection(a: &Point, b: &Point, c: &Point, d: &Point) -> bool {
        let o1 = orient(a, b, c);
        let o2 = orient(a, b, d);
        let o3 = orient(c, d, a);
        let o4 = orient(c, d, b);
        (o1 * o2) < 0.0 && (o3 * o4) < 0.0
    }

    #[inline(always)]
    fn quad_has_crossed_edges(c: &[Point; 4]) -> bool {
        proper_segment_intersection(&c[0], &c[1], &c[2], &c[3]) || proper_segment_intersection(&c[1], &c[2], &c[3], &c[0])
    }

    #[inline(always)]
    fn quad_is_strictly_convex(c: &[Point; 4]) -> bool {
        let mut sign = 0.0f64;
        for i in 0..4usize {
            let a = &c[i];
            let b = &c[(i + 1) & 3];
            let d = &c[(i + 2) & 3];
            let cross = orient(a, b, d);
            if cross.abs() <= f64::EPSILON {
                return false;
            }
            if sign == 0.0 {
                sign = cross.signum();
            } else if sign * cross < 0.0 {
                return false;
            }
        }
        true
    }

    // Guard downstream overlay/calibration against degenerate quads (NaNs, huge coords, etc.).
    // These can otherwise trigger wrap/overflow in drawing code and look like "encoding noise".
    let w = width.max(1) as f64;
    let h = height.max(1) as f64;
    let margin = 4.0f64;
    // Corner-placement quality guard:
    // detections with high border mismatch counts are commonly decoded from unstable
    // interior quads (ID can decode, but corner placement is visibly wrong).
    //
    // Keep this strict enough to avoid accepting visibly unstable corner geometry.
    if let Some(border_mismatches) = det.border_mismatches {
        let best_distance = det.best_distance.unwrap_or(0);
        if border_mismatches >= 3 {
            return false;
        }
        if border_mismatches == 2 && best_distance >= 2 {
            return false;
        }
        if border_mismatches == 1 && best_distance >= 3 {
            return false;
        }
    }

    let mut area2 = 0.0f64;
    let mut min_edge_sq = f64::MAX;
    for i in 0..4usize {
        let a = det.corners[i];
        let b = det.corners[(i + 1) % 4];
        if !a.x.is_finite() || !a.y.is_finite() {
            return false;
        }
        if a.x < -margin || a.y < -margin || a.x > (w - 1.0 + margin) || a.y > (h - 1.0 + margin) {
            return false;
        }
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        min_edge_sq = min_edge_sq.min(dx * dx + dy * dy);
        area2 += a.x * b.y - b.x * a.y;
    }
    if min_edge_sq < 4.0 {
        return false;
    }
    if area2.abs() < 4.0 {
        return false;
    }
    if quad_has_crossed_edges(&det.corners) {
        return false;
    }
    quad_is_strictly_convex(&det.corners)
}

#[allow(dead_code)]
pub(super) fn filter_detections_in_frame_in_place(detections: &mut Vec<ArucoDetection2D>, width: u32, height: u32) {
    detections.retain(|det| detection_in_frame(det, width, height));
}

fn detection_matches_id_range(det: &ArucoDetection2D, min_id: i64, max_id: i64) -> bool {
    if min_id < 0 && max_id < 0 {
        return true;
    }
    let min = min_id.max(0) as u32;
    let max = if max_id < 0 { u32::MAX } else { max_id.max(0) as u32 };
    det.id >= min && det.id <= max
}

pub(super) fn filter_detections_id_range(detections: &[ArucoDetection2D], min_id: i64, max_id: i64) -> Vec<ArucoDetection2D> {
    detections.iter().filter(|det| detection_matches_id_range(det, min_id, max_id)).cloned().collect()
}

pub(super) fn filter_detections_id_range_in_place(detections: &mut Vec<ArucoDetection2D>, min_id: i64, max_id: i64) {
    detections.retain(|det| detection_matches_id_range(det, min_id, max_id));
}

pub(super) fn merge_detections_spatial_impl<I>(detections: I, center_dist_px: f64, min_area_ratio: f64, max_corner_dist_px: f64, max_center_dist_ratio: f64, min_iou: f64) -> Vec<ArucoDetection2D>
where
    I: IntoIterator<Item = ArucoDetection2D>,
{
    #[inline(always)]
    fn quad_area(quad: &[Point; 4]) -> f64 {
        let mut area = 0.0f64;
        for i in 0..4 {
            let j = (i + 1) % 4;
            area += quad[i].x * quad[j].y - quad[j].x * quad[i].y;
        }
        area.abs() * 0.5
    }

    #[inline(always)]
    fn quad_center(quad: &[Point; 4]) -> (f64, f64) {
        let mut x = 0.0f64;
        let mut y = 0.0f64;
        for p in quad {
            x += p.x;
            y += p.y;
        }
        (x * 0.25, y * 0.25)
    }

    #[inline(always)]
    fn quad_min_avg_corner_dist2(a: &[Point; 4], b: &[Point; 4]) -> f64 {
        let mut best = f64::INFINITY;
        for shift in 0..4usize {
            let mut sum = 0.0f64;
            for (i, point) in a.iter().enumerate().take(4) {
                let j = (i + shift) & 3;
                let dx = point.x - b[j].x;
                let dy = point.y - b[j].y;
                sum += dx * dx + dy * dy;
            }
            best = best.min(sum * 0.25);

            let mut sum_rev = 0.0f64;
            for (i, point) in a.iter().enumerate().take(4) {
                let j = (shift + 4 - i) & 3;
                let dx = point.x - b[j].x;
                let dy = point.y - b[j].y;
                sum_rev += dx * dx + dy * dy;
            }
            best = best.min(sum_rev * 0.25);
        }
        best
    }

    #[inline]
    fn quad_iou(a: &[Point; 4], b: &[Point; 4]) -> f64 {
        #[inline(always)]
        fn signed_area(poly: &[Point]) -> f64 {
            let mut area = 0.0f64;
            for i in 0..poly.len() {
                let j = (i + 1) % poly.len();
                area += poly[i].x * poly[j].y - poly[j].x * poly[i].y;
            }
            area * 0.5
        }

        #[inline(always)]
        fn area_abs(poly: &[Point]) -> f64 {
            signed_area(poly).abs()
        }

        #[inline(always)]
        fn is_inside(p: &Point, a: &Point, b: &Point, clip_clockwise: bool) -> bool {
            let cross = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
            if clip_clockwise { cross <= 0.0 } else { cross >= 0.0 }
        }

        #[inline(always)]
        fn line_intersect(s: &Point, e: &Point, a: &Point, b: &Point) -> Point {
            let dx1 = e.x - s.x;
            let dy1 = e.y - s.y;
            let dx2 = b.x - a.x;
            let dy2 = b.y - a.y;
            let denom = dx1 * dy2 - dy1 * dx2;
            if denom.abs() < 1e-9 {
                return *e;
            }
            let t = ((a.x - s.x) * dy2 - (a.y - s.y) * dx2) / denom;
            Point { x: s.x + t * dx1, y: s.y + t * dy1 }
        }

        fn clip_polygon(subject: Vec<Point>, clip: &[Point; 4]) -> Vec<Point> {
            let mut output = subject;
            if output.is_empty() {
                return output;
            }
            let clip_clockwise = signed_area(clip) < 0.0;
            for (i, &a) in clip.iter().enumerate().take(4) {
                let b = clip[(i + 1) & 3];
                let input = output;
                if input.is_empty() {
                    return Vec::new();
                }
                output = Vec::with_capacity(input.len().saturating_add(4));
                let mut s = *input.last().unwrap();
                for &e in &input {
                    let ein = is_inside(&e, &a, &b, clip_clockwise);
                    let sin = is_inside(&s, &a, &b, clip_clockwise);
                    if ein {
                        if !sin {
                            output.push(line_intersect(&s, &e, &a, &b));
                        }
                        output.push(e);
                    } else if sin {
                        output.push(line_intersect(&s, &e, &a, &b));
                    }
                    s = e;
                }
            }
            output
        }

        let area_a = area_abs(a);
        let area_b = area_abs(b);
        if area_a <= 0.0 || area_b <= 0.0 {
            return 0.0;
        }
        let inter_poly = clip_polygon(a.to_vec(), b);
        let inter_area = area_abs(&inter_poly);
        if inter_area <= 0.0 {
            return 0.0;
        }
        let union_area = area_a + area_b - inter_area;
        if union_area <= 0.0 {
            return 0.0;
        }
        (inter_area / union_area).clamp(0.0, 1.0)
    }

    let dist = center_dist_px.max(0.0);
    let min_area_ratio = min_area_ratio.clamp(0.0, 1.0);
    let max_corner_dist = max_corner_dist_px.max(0.0);
    let max_center_dist_ratio = max_center_dist_ratio.max(0.0);
    let max_center_dist_ratio2 = max_center_dist_ratio * max_center_dist_ratio;
    let min_iou = min_iou.clamp(0.0, 1.0);
    if dist <= 0.0 {
        return detections.into_iter().collect();
    }

    let dist2 = dist * dist;
    let cell_size = dist.max(1.0);
    let max_corner_dist2 = max_corner_dist * max_corner_dist;

    let mut out: Vec<ArucoDetection2D> = Vec::new();
    let mut centers: Vec<(f64, f64)> = Vec::new();
    let mut areas: Vec<f64> = Vec::new();
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();

    for det in detections {
        let (cx, cy) = quad_center(&det.corners);
        let area = quad_area(&det.corners);
        let gx = (cx / cell_size).floor() as i32;
        let gy = (cy / cell_size).floor() as i32;

        let mut duplicate_index: Option<usize> = None;
        'cells: for yy in (gy - 1)..=(gy + 1) {
            for xx in (gx - 1)..=(gx + 1) {
                let Some(indices) = grid.get(&(xx, yy)) else { continue };
                for &idx in indices {
                    // Never dedup across different marker IDs. Nearby valid tags can be spatially
                    // close; merging by geometry-only drops real detections in multi-tag scenes.
                    if det.id != out[idx].id {
                        continue;
                    }
                    let (px, py) = centers[idx];
                    let dx = cx - px;
                    let dy = cy - py;
                    if dx * dx + dy * dy > dist2 {
                        continue;
                    }

                    if max_center_dist_ratio2 > 0.0 {
                        let existing_area = areas[idx];
                        let size2 = area.min(existing_area);
                        if size2 > 0.0 && (dx * dx + dy * dy) > max_center_dist_ratio2 * size2 {
                            continue;
                        }
                    }

                    if min_area_ratio > 0.0 {
                        let existing_area = areas[idx];
                        let larger = area.max(existing_area);
                        if larger > 0.0 {
                            let smaller = area.min(existing_area);
                            if (smaller / larger) < min_area_ratio {
                                continue;
                            }
                        }
                    }

                    if max_corner_dist2 > 0.0 && quad_min_avg_corner_dist2(&det.corners, &out[idx].corners) > max_corner_dist2 {
                        continue;
                    }
                    if min_iou > 0.0 && quad_iou(&det.corners, &out[idx].corners) < min_iou {
                        continue;
                    }

                    duplicate_index = Some(idx);
                    break 'cells;
                }
            }
        }

        if let Some(idx) = duplicate_index {
            if area > areas[idx] {
                out[idx] = det;
                centers[idx] = (cx, cy);
                areas[idx] = area;
            }
            continue;
        }

        let idx = out.len();
        out.push(det);
        centers.push((cx, cy));
        areas.push(area);
        grid.entry((gx, gy)).or_default().push(idx);
    }

    out
}
