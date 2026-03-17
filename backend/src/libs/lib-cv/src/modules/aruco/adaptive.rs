use image::{DynamicImage, GrayImage};
use imageproc::point::Point as CvPoint;
use memchr::{memchr, memrchr};
use std::cell::RefCell;
use std::mem::size_of;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::detect::{ArucoTagDetectorConfig, candidate_quad_from_contour, candidate_quad_from_contour_fast};
use crate::contour::suzuki_abe::{CompactContour, suzuki_abe_i32_compact_capped_into};
use crate::modules::image::luma::with_luma8_frame;

const ADAPTIVE_FAST_CANDIDATE_CONTOUR_CAP: usize = 96;
const ADAPTIVE_FAST_CANDIDATE_CONTOUR_CAP_MIN: usize = 24;
// Trigger the slower contour fallback only when the fast quad path is clearly under-producing.
// Lowering this threshold trims fallback work on noisy frames while preserving recall on this stream.
const ADAPTIVE_FALLBACK_TRIGGER_QUADS_MAX: usize = 8;
const ADAPTIVE_PREPROCESS_CONTOUR_CAP_MAX: usize = 512;
const ADAPTIVE_RETAIN_CONTOUR_POINTS_CAP: usize = 4 * 1024;
const ADAPTIVE_RETAIN_POINT_STORE_CAP: usize = 32 * 1024;
const ADAPTIVE_RETAIN_CONTOUR_COUNT_CAP: usize = ADAPTIVE_PREPROCESS_CONTOUR_CAP_MAX;
const ADAPTIVE_RETAIN_ROI_BYTES_CAP: usize = 1024 * 1024;
const ADAPTIVE_TRACE_POINTS_PER_CONTOUR: usize = 2 * 1024;
const ADAPTIVE_TRACE_POINT_BUDGET_MIN: usize = 128 * 1024;
const ADAPTIVE_TRACE_POINT_BUDGET_MAX: usize = 512 * 1024;
const ADAPTIVE_TRACE_CONTOUR_BUDGET_MIN: usize = 128;
const ADAPTIVE_TRACE_CONTOUR_BUDGET_MAX: usize = 2048;

static ADAPTIVE_TRACE_BUDGET_HITS: AtomicUsize = AtomicUsize::new(0);

#[inline(always)]
fn adaptive_trace_point_budget(preprocess_cap: usize) -> usize {
    preprocess_cap.saturating_mul(ADAPTIVE_TRACE_POINTS_PER_CONTOUR).clamp(ADAPTIVE_TRACE_POINT_BUDGET_MIN, ADAPTIVE_TRACE_POINT_BUDGET_MAX)
}

#[inline(always)]
fn adaptive_trace_contour_budget(preprocess_cap: usize) -> usize {
    preprocess_cap.saturating_mul(4).clamp(ADAPTIVE_TRACE_CONTOUR_BUDGET_MIN, ADAPTIVE_TRACE_CONTOUR_BUDGET_MAX)
}

fn log_adaptive_trace_budget_hit(width: u32, height: u32, point_budget: usize, contour_budget: usize, preprocess_cap: usize) {
    let hit = ADAPTIVE_TRACE_BUDGET_HITS.fetch_add(1, Ordering::Relaxed) + 1;
    if hit <= 5 || hit.is_multiple_of(100) {
        tracing::warn!(width, height, point_budget, contour_budget, preprocess_cap, hit, "adaptive contour trace aborted at safety budget");
    }
}

#[derive(Debug, Clone)]
pub struct AdaptiveDetectorConfig {
    pub adaptive_window: u32,
    pub adaptive_offset: f32,
    pub threshold_offset: f32,
    /// When true, flip the adaptive-threshold mask polarity (matches invert=true in adaptive threshold nodes).
    pub invert: bool,
    /// Optional morphology open kernel size (0 disables).
    pub open_k: u8,
    pub min_perimeter_rate: f32,
    pub max_perimeter_rate: f32,
    pub epsilon: f32,
    pub min_area: f32,
    pub max_area: Option<f32>,
    pub min_angle: f32,
    pub max_angle: f32,
    pub max_side_cv: f32,
    /// OpenCV-style minimum spacing between neighboring corners as a fraction of quad perimeter.
    pub min_corner_distance_rate: f32,
    /// OpenCV-style minimum allowed distance from marker corners to the image border in pixels.
    pub min_distance_to_border: u32,
    /// Fast contour prefilter: reject contours whose bbox min side is below this threshold.
    pub min_side_px: f32,
    /// Upper bound for slow fallback contour checks when fast 4-corner filtering yields few quads.
    /// Set to 0 to disable the fallback pass entirely.
    pub fallback_max_contours: usize,
    /// Optional hard cap on emitted quads after filtering/merge. Set to 0 for unlimited.
    pub max_quads: usize,
}

impl Default for AdaptiveDetectorConfig {
    fn default() -> Self {
        Self {
            adaptive_window: 15,
            adaptive_offset: 7.0,
            threshold_offset: 0.0,
            invert: false,
            open_k: 0,
            min_perimeter_rate: 0.03,
            max_perimeter_rate: 4.0,
            epsilon: 5.0,
            min_area: 400.0,
            max_area: None,
            min_angle: 60.0,
            max_angle: 120.0,
            max_side_cv: 1.5,
            min_corner_distance_rate: 0.05,
            min_distance_to_border: 3,
            min_side_px: 0.0,
            fallback_max_contours: 120,
            max_quads: 0,
        }
    }
}

fn quad_perimeter(quad: &[CvPoint<f32>; 4]) -> f32 {
    let mut perimeter = 0.0f32;
    for i in 0..4 {
        let a = quad[i];
        let b = quad[(i + 1) % 4];
        let dx = a.x - b.x;
        let dy = a.y - b.y;
        perimeter += (dx * dx + dy * dy).sqrt();
    }
    perimeter
}

fn quad_average_corner_distance(a: &[CvPoint<f32>; 4], b: &[CvPoint<f32>; 4]) -> f32 {
    let mut min_dist_sq = f32::MAX;
    for fc in 0..4 {
        let mut dist_sq = 0.0f32;
        for c in 0..4 {
            let ac = a[(c + fc) % 4];
            let bc = b[c];
            let dx = ac.x - bc.x;
            let dy = ac.y - bc.y;
            dist_sq += dx * dx + dy * dy;
        }
        dist_sq *= 0.25;
        min_dist_sq = min_dist_sq.min(dist_sq);
    }
    min_dist_sq.sqrt()
}

fn quad_perimeter_and_min_edge(quad: &[CvPoint<f32>; 4]) -> (f32, f32) {
    let mut perimeter = 0.0f32;
    let mut min_edge_sq = f32::MAX;
    for i in 0..4 {
        let a = quad[i];
        let b = quad[(i + 1) % 4];
        let dx = a.x - b.x;
        let dy = a.y - b.y;
        let edge_sq = dx * dx + dy * dy;
        min_edge_sq = min_edge_sq.min(edge_sq);
        perimeter += edge_sq.sqrt();
    }
    (perimeter, min_edge_sq)
}

fn quad_passes_post_filters(quad: &[CvPoint<f32>; 4], cfg: &AdaptiveDetectorConfig, width: f32, height: f32) -> bool {
    if cfg.min_corner_distance_rate > 0.0 {
        let (perimeter, min_edge_sq) = quad_perimeter_and_min_edge(quad);
        if perimeter <= f32::EPSILON {
            return false;
        }
        let min_edge_limit = perimeter * cfg.min_corner_distance_rate;
        if min_edge_sq < min_edge_limit * min_edge_limit {
            return false;
        }
    }

    if cfg.min_distance_to_border > 0 {
        let margin = cfg.min_distance_to_border as f32;
        let max_x = (width - 1.0) - margin;
        let max_y = (height - 1.0) - margin;
        for p in quad {
            if p.x < margin || p.y < margin || p.x > max_x || p.y > max_y {
                return false;
            }
        }
    }

    true
}

fn merge_quads(out: &mut Vec<[CvPoint<f32>; 4]>, mut candidates: Vec<[CvPoint<f32>; 4]>, min_distance_rate: f32) {
    let mut out_perimeters: Vec<f32> = out.iter().map(quad_perimeter).collect();
    for quad in candidates.drain(..) {
        let perim = quad_perimeter(&quad);
        let mut replaced = false;
        for (idx, existing) in out.iter_mut().enumerate() {
            let dist = quad_average_corner_distance(existing, &quad);
            let existing_perim = out_perimeters[idx];
            let min_perim = existing_perim.min(perim);
            if dist < min_perim * min_distance_rate {
                if perim > existing_perim {
                    *existing = quad;
                    out_perimeters[idx] = perim;
                }
                replaced = true;
                break;
            }
        }
        if !replaced {
            out.push(quad);
            out_perimeters.push(perim);
        }
    }
}

thread_local! {
    static ADAPTIVE_EXTRACT_SCRATCH: RefCell<AdaptiveExtractScratch> = RefCell::new(AdaptiveExtractScratch::default());
}

#[derive(Clone, Copy)]
struct PreparedContour {
    start: usize,
    len: usize,
    perimeter: f32,
}

#[derive(Default)]
struct AdaptiveExtractScratch {
    work_i32: Vec<CvPoint<i32>>,
    compress_scratch: Vec<CvPoint<i32>>,
    roi_bytes: Vec<u8>,
    raw_point_store: Vec<CvPoint<i32>>,
    raw_contours: Vec<CompactContour>,
    point_store: Vec<CvPoint<i32>>,
    prepared: Vec<PreparedContour>,
    fast_idx: Vec<usize>,
    fast_success: Vec<bool>,
    fast_tested: Vec<bool>,
    fallback_idx: Vec<usize>,
    pts_f32: Vec<CvPoint<f32>>,
}

impl AdaptiveExtractScratch {
    fn compact_after_frame(&mut self) {
        trim_retained_vec(&mut self.work_i32, ADAPTIVE_RETAIN_CONTOUR_POINTS_CAP);
        trim_retained_vec(&mut self.compress_scratch, ADAPTIVE_RETAIN_CONTOUR_POINTS_CAP);
        trim_retained_vec(&mut self.roi_bytes, ADAPTIVE_RETAIN_ROI_BYTES_CAP);
        trim_retained_vec(&mut self.raw_point_store, ADAPTIVE_RETAIN_POINT_STORE_CAP);
        trim_retained_vec(&mut self.raw_contours, ADAPTIVE_RETAIN_CONTOUR_COUNT_CAP);
        trim_retained_vec(&mut self.point_store, ADAPTIVE_RETAIN_POINT_STORE_CAP);
        trim_retained_vec(&mut self.prepared, ADAPTIVE_RETAIN_CONTOUR_COUNT_CAP);
        trim_retained_vec(&mut self.fast_idx, ADAPTIVE_RETAIN_CONTOUR_COUNT_CAP);
        trim_retained_vec(&mut self.fast_success, ADAPTIVE_RETAIN_CONTOUR_COUNT_CAP);
        trim_retained_vec(&mut self.fast_tested, ADAPTIVE_RETAIN_CONTOUR_COUNT_CAP);
        trim_retained_vec(&mut self.fallback_idx, ADAPTIVE_RETAIN_CONTOUR_COUNT_CAP);
        trim_retained_vec(&mut self.pts_f32, ADAPTIVE_RETAIN_CONTOUR_POINTS_CAP);
    }
}

#[inline(always)]
fn report_adaptive_allocation_high_water(name: &'static str, bytes: usize) {
    crate::diagnostics::report_scratch_high_water(name, bytes);
}

#[inline(always)]
fn prepared_contours_bytes(point_store_capacity: usize, prepared_capacity: usize) -> usize {
    let point_bytes = point_store_capacity * size_of::<CvPoint<i32>>();
    let vec_bytes = prepared_capacity * size_of::<PreparedContour>();
    point_bytes + vec_bytes
}

#[inline(always)]
fn prepared_contour_points<'a>(contour: &PreparedContour, point_store: &'a [CvPoint<i32>]) -> &'a [CvPoint<i32>] {
    &point_store[contour.start..contour.start + contour.len]
}

#[inline(always)]
fn trim_retained_vec<T>(vec: &mut Vec<T>, retain_cap: usize) {
    vec.clear();
    if vec.capacity() > retain_cap {
        vec.shrink_to(retain_cap);
    }
}

#[inline(always)]
fn quad_area_f32(quad: &[CvPoint<f32>; 4]) -> f32 {
    let mut area = 0.0f32;
    for i in 0..4usize {
        let j = (i + 1) & 3;
        area += quad[i].x * quad[j].y - quad[j].x * quad[i].y;
    }
    0.5 * area.abs()
}

fn cap_quads_by_area(quads: &mut Vec<[CvPoint<f32>; 4]>, max_quads: usize) {
    if max_quads == 0 || quads.len() <= max_quads {
        return;
    }
    quads.select_nth_unstable_by(max_quads - 1, |a, b| quad_area_f32(b).total_cmp(&quad_area_f32(a)));
    quads.truncate(max_quads);
}

#[inline(always)]
fn contour_to_f32(points: &[CvPoint<i32>], out: &mut Vec<CvPoint<f32>>, off_x: i32, off_y: i32) {
    let len = points.len();
    out.clear();
    if len == 0 {
        return;
    }
    out.resize(len, CvPoint::new(0.0, 0.0));
    if off_x == 0 && off_y == 0 {
        for (dst, p) in out.iter_mut().zip(points.iter()) {
            *dst = CvPoint::new(p.x as f32, p.y as f32);
        }
    } else {
        for (dst, p) in out.iter_mut().zip(points.iter()) {
            *dst = CvPoint::new((p.x + off_x) as f32, (p.y + off_y) as f32);
        }
    }
}

#[inline(always)]
fn compress_chain_turn_points(points: &mut Vec<CvPoint<i32>>, scratch: &mut Vec<CvPoint<i32>>) {
    let n = points.len();
    if n < 32 {
        return;
    }

    scratch.clear();
    scratch.reserve(n.saturating_sub(scratch.capacity()));
    for i in 0..n {
        let prev = points[(i + n - 1) % n];
        let curr = points[i];
        let next = points[(i + 1) % n];
        let d1x = (curr.x - prev.x).signum();
        let d1y = (curr.y - prev.y).signum();
        let d2x = (next.x - curr.x).signum();
        let d2y = (next.y - curr.y).signum();
        if d1x != d2x || d1y != d2y {
            scratch.push(curr);
        }
    }

    if scratch.len() >= 8 && scratch.len() + 4 < n {
        std::mem::swap(points, scratch);
    }
}

fn extract_quads_from_binary(binary: &GrayImage, config: &AdaptiveDetectorConfig, diag: f32) -> Vec<[CvPoint<f32>; 4]> {
    let binary_width = binary.width() as usize;
    let width = binary.width() as f32;
    let height = binary.height() as f32;
    let min_perimeter = (config.min_perimeter_rate.max(0.0)) * diag;
    let max_perimeter = (config.max_perimeter_rate.max(config.min_perimeter_rate + f32::EPSILON)) * diag;
    let max_step = std::f32::consts::SQRT_2;

    let roi_bounds = {
        let w = binary.width() as usize;
        let h = binary.height() as usize;
        if w == 0 || h == 0 {
            return Vec::new();
        }

        // Mirror the contour-node ROI fast-path: trace only the active mask bbox when sparse.
        let buf = binary.as_raw();
        let mut any = false;
        let mut min_x = w;
        let mut max_x = 0usize;
        let mut min_y = h;
        let mut max_y = 0usize;

        for y in 0..h {
            let row = &buf[y * w..(y + 1) * w];
            let Some(x0) = memchr(255, row) else { continue };
            let x1 = memrchr(255, row).unwrap_or(x0);
            any = true;
            min_y = min_y.min(y);
            max_y = max_y.max(y);
            min_x = min_x.min(x0);
            max_x = max_x.max(x1);
        }

        if !any {
            return Vec::new();
        }

        min_x = min_x.saturating_sub(1);
        min_y = min_y.saturating_sub(1);
        max_x = (max_x + 1).min(w.saturating_sub(1));
        max_y = (max_y + 1).min(h.saturating_sub(1));

        let roi_w = max_x.saturating_sub(min_x) + 1;
        let roi_h = max_y.saturating_sub(min_y) + 1;
        let full_area = w.saturating_mul(h);
        let roi_area = roi_w.saturating_mul(roi_h);
        let min_perimeter = (config.min_perimeter_rate.max(0.0)) * diag;

        // Fast impossible-shape rejects before contour tracing:
        // 1) bbox area cannot satisfy downstream min-area
        // 2) bbox perimeter cannot satisfy downstream min-perimeter
        if config.min_area > 0.0 && (roi_area as f32) < config.min_area {
            return Vec::new();
        }
        let bbox_perimeter = ((roi_w + roi_h) * 2) as f32;
        if bbox_perimeter < min_perimeter {
            return Vec::new();
        }

        let use_roi = roi_area > 0 && full_area > 0 && roi_area * 100 < full_area * 85;

        if use_roi { Some((min_x, min_y, roi_w, roi_h)) } else { None }
    };

    // Cap expensive contour-preprocess work under noisy masks. We keep the largest chain-code
    // contours first (proxy for perimeter/area) because true tags are unlikely to be tiny.
    let fast_cap_cfg = config.fallback_max_contours.clamp(ADAPTIVE_FAST_CANDIDATE_CONTOUR_CAP_MIN, ADAPTIVE_FAST_CANDIDATE_CONTOUR_CAP);
    let preprocess_cap = if config.fallback_max_contours > ADAPTIVE_FAST_CANDIDATE_CONTOUR_CAP {
        config.fallback_max_contours.saturating_mul(2).clamp(ADAPTIVE_FAST_CANDIDATE_CONTOUR_CAP, ADAPTIVE_PREPROCESS_CONTOUR_CAP_MAX)
    } else {
        fast_cap_cfg
    };
    let trace_point_budget = adaptive_trace_point_budget(preprocess_cap);
    let trace_contour_budget = adaptive_trace_contour_budget(preprocess_cap);

    let detector_config = ArucoTagDetectorConfig {
        epsilon: config.epsilon,
        min_area: config.min_area,
        max_area: config.max_area,
        min_angle_deg: config.min_angle,
        max_angle_deg: config.max_angle,
        max_side_cv: config.max_side_cv,
        max_side_ratio: 0.0,
        max_diag_ratio: 0.0,
        angle_cos_min: -1.0,
        angle_cos_max: 1.0,
    }
    .with_angle_cos_bounds();

    let min_perimeter_for_area = if detector_config.min_area > f32::EPSILON { Some((4.0 * std::f32::consts::PI * detector_config.min_area).sqrt()) } else { None };

    ADAPTIVE_EXTRACT_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        let AdaptiveExtractScratch { work_i32, compress_scratch, roi_bytes, raw_point_store, raw_contours, point_store, prepared, fast_idx, fast_success, fast_tested, fallback_idx, pts_f32 } =
            &mut *scratch;

        work_i32.clear();
        compress_scratch.clear();
        roi_bytes.clear();
        raw_point_store.clear();
        raw_contours.clear();
        point_store.clear();
        prepared.clear();
        fast_idx.clear();
        fast_success.clear();
        fast_tested.clear();
        fallback_idx.clear();
        pts_f32.clear();

        let (contour_off_x, contour_off_y, trace_complete) = if let Some((min_x, min_y, roi_w, roi_h)) = roi_bounds {
            let roi_len = roi_w.saturating_mul(roi_h);
            if roi_bytes.len() != roi_len {
                roi_bytes.resize(roi_len, 0);
                report_adaptive_allocation_high_water("aruco.adaptive_roi_mask", roi_bytes.capacity() * size_of::<u8>());
            }
            let src = binary.as_raw();
            for yy in 0..roi_h {
                let src_row = &src[(min_y + yy) * binary_width + min_x..(min_y + yy) * binary_width + min_x + roi_w];
                let dst_row = &mut roi_bytes[yy * roi_w..yy * roi_w + roi_w];
                dst_row.copy_from_slice(src_row);
            }
            let roi = GrayImage::from_raw(roi_w as u32, roi_h as u32, std::mem::take(roi_bytes)).unwrap_or_else(|| GrayImage::new(roi_w as u32, roi_h as u32));
            let trace_complete = suzuki_abe_i32_compact_capped_into(&roi, raw_point_store, raw_contours, trace_point_budget, trace_contour_budget);
            *roi_bytes = roi.into_raw();
            (min_x as i32, min_y as i32, trace_complete)
        } else {
            (0, 0, suzuki_abe_i32_compact_capped_into(binary, raw_point_store, raw_contours, trace_point_budget, trace_contour_budget))
        };
        if !trace_complete {
            log_adaptive_trace_budget_hit(binary.width(), binary.height(), trace_point_budget, trace_contour_budget, preprocess_cap);
            scratch.compact_after_frame();
            crate::modules::aruco::detect::compact_detect_scratch_after_frame();
            crate::modules::contour::douglas_peucker::compact_rdp_scratch_after_frame();
            return Vec::new();
        }

        if raw_contours.len() > preprocess_cap {
            raw_contours.select_nth_unstable_by(preprocess_cap - 1, |a, b| b.len.cmp(&a.len));
            raw_contours.truncate(preprocess_cap);
        }
        report_adaptive_allocation_high_water("aruco.adaptive_raw_contours", raw_point_store.capacity() * size_of::<CvPoint<i32>>() + raw_contours.capacity() * size_of::<CompactContour>());

        for contour in raw_contours.iter().copied() {
            let points = contour.points(raw_point_store);
            if points.len() < 4 {
                continue;
            }
            work_i32.clear();
            work_i32.extend_from_slice(points);

            let contour_len = work_i32.len() as f32;
            if contour_len > max_perimeter || contour_len * max_step < min_perimeter {
                continue;
            }

            if config.min_area > f32::EPSILON || config.min_side_px > 0.0 {
                let mut min_x = i32::MAX;
                let mut min_y = i32::MAX;
                let mut max_x = i32::MIN;
                let mut max_y = i32::MIN;
                for p in work_i32.iter() {
                    min_x = min_x.min(p.x);
                    min_y = min_y.min(p.y);
                    max_x = max_x.max(p.x);
                    max_y = max_y.max(p.y);
                }
                let bbox_w = (max_x - min_x).unsigned_abs().saturating_add(1);
                let bbox_h = (max_y - min_y).unsigned_abs().saturating_add(1);
                if config.min_area > f32::EPSILON {
                    let bbox_area = bbox_w as f32 * bbox_h as f32;
                    if bbox_area < config.min_area {
                        continue;
                    }
                }
                if config.min_side_px > 0.0 {
                    let bbox_min_side = bbox_w.min(bbox_h) as f32;
                    if bbox_min_side < config.min_side_px {
                        continue;
                    }
                }
            }

            compress_chain_turn_points(work_i32, compress_scratch);
            if work_i32.len() < 4 {
                continue;
            }

            let start = point_store.len();
            point_store.extend_from_slice(work_i32);
            prepared.push(PreparedContour { start, len: work_i32.len(), perimeter: contour_len });
        }

        report_adaptive_allocation_high_water("aruco.adaptive_prepared_contours", prepared_contours_bytes(point_store.capacity(), prepared.capacity()));

        let result = if prepared.is_empty() {
            Vec::new()
        } else {
            let fast_cap = fast_cap_cfg.min(prepared.len());
            if config.fallback_max_contours <= fast_cap {
                let mut out = Vec::with_capacity(fast_cap);
                for contour in prepared.iter().take(fast_cap) {
                    contour_to_f32(prepared_contour_points(contour, point_store), pts_f32, contour_off_x, contour_off_y);
                    if let Some(quad) =
                        candidate_quad_from_contour_fast(pts_f32, contour.perimeter, min_perimeter_for_area, &detector_config).filter(|quad| quad_passes_post_filters(quad, config, width, height))
                    {
                        out.push(quad);
                    }
                }
                cap_quads_by_area(&mut out, config.max_quads);
                report_adaptive_allocation_high_water("aruco.adaptive_quads", out.capacity() * size_of::<[CvPoint<f32>; 4]>());
                out
            } else {
                fast_idx.extend(0..prepared.len());
                if fast_idx.len() > fast_cap {
                    fast_idx.select_nth_unstable_by(fast_cap - 1, |&a, &b| prepared[b].len.cmp(&prepared[a].len));
                    fast_idx.truncate(fast_cap);
                }
                fast_success.resize(prepared.len(), false);
                fast_success.fill(false);
                fast_tested.resize(prepared.len(), false);
                fast_tested.fill(false);
                report_adaptive_allocation_high_water(
                    "aruco.adaptive_fast_state",
                    fast_idx.capacity() * size_of::<usize>() + fast_success.capacity() * size_of::<bool>() + fast_tested.capacity() * size_of::<bool>(),
                );

                let mut quads: Vec<[CvPoint<f32>; 4]> = Vec::with_capacity(fast_idx.len());
                for &idx in fast_idx.iter() {
                    fast_tested[idx] = true;
                    let contour = prepared[idx];
                    contour_to_f32(prepared_contour_points(&contour, point_store), pts_f32, contour_off_x, contour_off_y);
                    if let Some(quad) =
                        candidate_quad_from_contour_fast(pts_f32, contour.perimeter, min_perimeter_for_area, &detector_config).filter(|quad| quad_passes_post_filters(quad, config, width, height))
                    {
                        fast_success[idx] = true;
                        quads.push(quad);
                    }
                }
                report_adaptive_allocation_high_water("aruco.adaptive_quads", quads.capacity() * size_of::<[CvPoint<f32>; 4]>());

                if quads.len() < ADAPTIVE_FALLBACK_TRIGGER_QUADS_MAX {
                    let fallback_cap = config.fallback_max_contours;
                    let k = fallback_cap.min(prepared.len());
                    if k > 0 && k > fast_cap {
                        fallback_idx.extend(0..prepared.len());
                        fallback_idx.select_nth_unstable_by(k - 1, |&a, &b| prepared[b].len.cmp(&prepared[a].len));
                        report_adaptive_allocation_high_water("aruco.adaptive_fallback_index", fallback_idx.capacity() * size_of::<usize>());

                        for &idx in fallback_idx.iter().take(k) {
                            if fast_success[idx] || fast_tested[idx] {
                                continue;
                            }
                            let contour = prepared[idx];
                            contour_to_f32(prepared_contour_points(&contour, point_store), pts_f32, contour_off_x, contour_off_y);
                            if let Some(quad) = candidate_quad_from_contour(pts_f32, &detector_config).filter(|quad| quad_passes_post_filters(quad, config, width, height)) {
                                quads.push(quad);
                            }
                        }
                    }
                }

                cap_quads_by_area(&mut quads, config.max_quads);
                report_adaptive_allocation_high_water("aruco.adaptive_quads", quads.capacity() * size_of::<[CvPoint<f32>; 4]>());
                quads
            }
        };

        scratch.compact_after_frame();
        crate::modules::aruco::detect::compact_detect_scratch_after_frame();
        crate::modules::contour::douglas_peucker::compact_rdp_scratch_after_frame();
        result
    })
}

pub fn adaptive_quads(frame: &DynamicImage, config: &AdaptiveDetectorConfig) -> Vec<[CvPoint<f32>; 4]> {
    with_luma8_frame(frame, |gray| {
        let diag = ((gray.width() as f32).powi(2) + (gray.height() as f32).powi(2)).sqrt();
        let window = ensure_odd_window(config.adaptive_window);
        let binary = crate::modules::image::binary::adaptive_mean_threshold_fast_with_invert(gray, window, config.adaptive_offset + config.threshold_offset, config.invert);
        let binary = if config.open_k > 0 { crate::ops::morphology::open(&binary, imageproc::distance_transform::Norm::L1, config.open_k) } else { binary };
        extract_quads_from_binary(&binary, config, diag)
    })
}

pub fn adaptive_quads_multi(frame: &DynamicImage, config: &AdaptiveDetectorConfig, window_min: u32, window_max: u32, window_step: u32) -> Vec<[CvPoint<f32>; 4]> {
    with_luma8_frame(frame, |gray| {
        let diag = ((gray.width() as f32).powi(2) + (gray.height() as f32).powi(2)).sqrt();

        let mut out: Vec<[CvPoint<f32>; 4]> = Vec::new();
        let mut window = ensure_odd_window(window_min.max(3));
        let max = ensure_odd_window(window_max.max(window_min));
        let step = window_step.max(1);

        while window <= max {
            let binary = crate::modules::image::binary::adaptive_mean_threshold_fast_with_invert(gray, window, config.adaptive_offset + config.threshold_offset, config.invert);
            let binary = if config.open_k > 0 { crate::ops::morphology::open(&binary, imageproc::distance_transform::Norm::L1, config.open_k) } else { binary };
            let quads = extract_quads_from_binary(&binary, config, diag);
            merge_quads(&mut out, quads, 0.125);

            let next = window.saturating_add(step);
            window = if next.is_multiple_of(2) { next + 1 } else { next };
        }

        out
    })
}

pub fn adaptive_quads_from_mask(mask: &GrayImage, config: &AdaptiveDetectorConfig) -> Vec<[CvPoint<f32>; 4]> {
    if mask.width() == 0 || mask.height() == 0 {
        return Vec::new();
    }
    let diag = ((mask.width() as f32).powi(2) + (mask.height() as f32).powi(2)).sqrt();
    extract_quads_from_binary(mask, config, diag)
}

fn ensure_odd_window(value: u32) -> u32 {
    let candidate = if value < 3 { 3 } else { value };
    if candidate % 2 == 0 { candidate + 1 } else { candidate }
}
