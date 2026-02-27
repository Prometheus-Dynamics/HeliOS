use image::{DynamicImage, GrayImage};
use imageproc::point::Point as CvPoint;
use memchr::{memchr, memrchr};
use rayon::prelude::*;

use super::detect::{ArucoTagDetectorConfig, candidate_quad_from_contour, candidate_quad_from_contour_fast};
use crate::contour::suzuki_abe::suzuki_abe_i32;
use crate::modules::image::luma::with_luma8_frame;

const ADAPTIVE_FAST_CANDIDATE_CONTOUR_CAP: usize = 96;
const ADAPTIVE_FAST_CANDIDATE_CONTOUR_CAP_MIN: usize = 24;
const ADAPTIVE_PREPROCESS_PAR_MIN_CONTOURS: usize = 128;
const ADAPTIVE_FAST_QUAD_PAR_MIN_CONTOURS: usize = 128;
const ADAPTIVE_FALLBACK_PAR_MIN_CONTOURS: usize = 128;
// Trigger the slower contour fallback only when the fast quad path is clearly under-producing.
// Lowering this threshold trims fallback work on noisy frames while preserving recall on this stream.
const ADAPTIVE_FALLBACK_TRIGGER_QUADS_MAX: usize = 8;
const ADAPTIVE_PREPROCESS_CONTOUR_CAP_MAX: usize = 512;

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

struct PreparedContour {
    pts: Vec<CvPoint<i32>>,
    perimeter: f32,
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
    out.reserve(len);
    // SAFETY: we immediately initialize every element before any read.
    unsafe {
        out.set_len(len);
    }
    if off_x == 0 && off_y == 0 {
        for i in 0..len {
            let p = points[i];
            out[i] = CvPoint::new(p.x as f32, p.y as f32);
        }
    } else {
        for i in 0..len {
            let p = points[i];
            out[i] = CvPoint::new((p.x + off_x) as f32, (p.y + off_y) as f32);
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
    let width = binary.width() as f32;
    let height = binary.height() as f32;
    let min_perimeter = (config.min_perimeter_rate.max(0.0)) * diag;
    let max_perimeter = (config.max_perimeter_rate.max(config.min_perimeter_rate + f32::EPSILON)) * diag;
    let max_step = std::f32::consts::SQRT_2;

    let (mut contours, contour_off_x, contour_off_y) = {
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

        if use_roi {
            let mut roi = GrayImage::new(roi_w as u32, roi_h as u32);
            let dst = roi.as_mut();
            for yy in 0..roi_h {
                let src_row = &buf[(min_y + yy) * w + min_x..(min_y + yy) * w + min_x + roi_w];
                dst[yy * roi_w..yy * roi_w + roi_w].copy_from_slice(src_row);
            }
            (suzuki_abe_i32(&roi), min_x as i32, min_y as i32)
        } else {
            (suzuki_abe_i32(binary), 0, 0)
        }
    };

    // Cap expensive contour-preprocess work under noisy masks. We keep the largest chain-code
    // contours first (proxy for perimeter/area) because true tags are unlikely to be tiny.
    let fast_cap_cfg = config.fallback_max_contours.clamp(ADAPTIVE_FAST_CANDIDATE_CONTOUR_CAP_MIN, ADAPTIVE_FAST_CANDIDATE_CONTOUR_CAP);
    let preprocess_cap = if config.fallback_max_contours > ADAPTIVE_FAST_CANDIDATE_CONTOUR_CAP {
        config.fallback_max_contours.saturating_mul(2).clamp(ADAPTIVE_FAST_CANDIDATE_CONTOUR_CAP, ADAPTIVE_PREPROCESS_CONTOUR_CAP_MAX)
    } else {
        fast_cap_cfg
    };
    if contours.len() > preprocess_cap {
        contours.select_nth_unstable_by(preprocess_cap - 1, |a, b| b.points.len().cmp(&a.points.len()));
        contours.truncate(preprocess_cap);
    }

    let preprocess = |mut pts: Vec<CvPoint<i32>>, scratch: &mut Vec<CvPoint<i32>>| -> Option<PreparedContour> {
        if pts.len() < 4 {
            return None;
        }
        // Cheap chain-code bounds: true geometric perimeter is within [len, len*sqrt(2)].
        // This rejects obvious out-of-range contours before perimeter estimation/conversion.
        let contour_len = pts.len() as f32;
        if contour_len > max_perimeter || contour_len * max_step < min_perimeter {
            return None;
        }

        if config.min_area > f32::EPSILON {
            let mut min_x = i32::MAX;
            let mut min_y = i32::MAX;
            let mut max_x = i32::MIN;
            let mut max_y = i32::MIN;
            for p in &pts {
                min_x = min_x.min(p.x);
                min_y = min_y.min(p.y);
                max_x = max_x.max(p.x);
                max_y = max_y.max(p.y);
            }
            let bbox_w = (max_x - min_x).unsigned_abs().saturating_add(1);
            let bbox_h = (max_y - min_y).unsigned_abs().saturating_add(1);
            let bbox_area = bbox_w as f32 * bbox_h as f32;
            if bbox_area < config.min_area {
                return None;
            }
            if config.min_side_px > 0.0 {
                let bbox_min_side = bbox_w.min(bbox_h) as f32;
                if bbox_min_side < config.min_side_px {
                    return None;
                }
            }
        } else if config.min_side_px > 0.0 {
            let mut min_x = i32::MAX;
            let mut min_y = i32::MAX;
            let mut max_x = i32::MIN;
            let mut max_y = i32::MIN;
            for p in &pts {
                min_x = min_x.min(p.x);
                min_y = min_y.min(p.y);
                max_x = max_x.max(p.x);
                max_y = max_y.max(p.y);
            }
            let bbox_w = (max_x - min_x).unsigned_abs().saturating_add(1);
            let bbox_h = (max_y - min_y).unsigned_abs().saturating_add(1);
            let bbox_min_side = bbox_w.min(bbox_h) as f32;
            if bbox_min_side < config.min_side_px {
                return None;
            }
        }

        // For fast candidate filtering, chain length is a sufficient perimeter proxy and avoids
        // an extra full contour walk with sqrt-heavy edge accumulation.
        let geometric_perimeter = contour_len;

        compress_chain_turn_points(&mut pts, scratch);
        if pts.len() < 4 {
            return None;
        }
        Some(PreparedContour { perimeter: geometric_perimeter, pts })
    };

    let contour_buffer: Vec<PreparedContour> = if contours.len() >= ADAPTIVE_PREPROCESS_PAR_MIN_CONTOURS && rayon::current_num_threads() > 1 {
        contours.into_par_iter().map_init(Vec::<CvPoint<i32>>::new, |scratch, contour| preprocess(contour.points, scratch)).filter_map(|contour| contour).collect()
    } else {
        let mut scratch: Vec<CvPoint<i32>> = Vec::new();
        let mut out: Vec<PreparedContour> = Vec::new();
        for contour in contours {
            if let Some(contour) = preprocess(contour.points, &mut scratch) {
                out.push(contour);
            }
        }
        out
    };

    if contour_buffer.is_empty() {
        return Vec::new();
    }

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

    // Fast path: only accept contours that simplify cleanly to a 4-corner polygon.
    //
    // Under noisy masks, contour count can spike and DP simplification dominates runtime. Cap the
    // expensive fast-path candidates to the largest perimeters first; true tags are usually among
    // those and this trims long-tail contour cost without downscaling/frame skipping.
    let fast_cap = fast_cap_cfg.min(contour_buffer.len());
    if config.fallback_max_contours <= fast_cap {
        let fast_slice = &contour_buffer[..fast_cap];
        let mut out = if fast_slice.len() >= ADAPTIVE_FAST_QUAD_PAR_MIN_CONTOURS && rayon::current_num_threads() > 1 {
            fast_slice
                .par_iter()
                .map_init(Vec::<CvPoint<f32>>::new, |pts_f32, contour| {
                    contour_to_f32(&contour.pts, pts_f32, contour_off_x, contour_off_y);
                    candidate_quad_from_contour_fast(pts_f32, contour.perimeter, min_perimeter_for_area, &detector_config).filter(|quad| quad_passes_post_filters(quad, config, width, height))
                })
                .filter_map(|quad| quad)
                .collect()
        } else {
            let mut out = Vec::with_capacity(fast_slice.len());
            let mut pts_f32: Vec<CvPoint<f32>> = Vec::new();
            for contour in fast_slice {
                contour_to_f32(&contour.pts, &mut pts_f32, contour_off_x, contour_off_y);
                if let Some(quad) =
                    candidate_quad_from_contour_fast(&pts_f32, contour.perimeter, min_perimeter_for_area, &detector_config).filter(|quad| quad_passes_post_filters(quad, config, width, height))
                {
                    out.push(quad);
                }
            }
            out
        };
        cap_quads_by_area(&mut out, config.max_quads);
        return out;
    }

    let mut fast_idx: Vec<usize> = (0..contour_buffer.len()).collect();
    if fast_idx.len() > fast_cap {
        fast_idx.select_nth_unstable_by(fast_cap - 1, |&a, &b| contour_buffer[b].pts.len().cmp(&contour_buffer[a].pts.len()));
        fast_idx.truncate(fast_cap);
    }
    let mut fast_success = vec![false; contour_buffer.len()];
    let mut fast_tested = vec![false; contour_buffer.len()];
    let mut quads: Vec<[CvPoint<f32>; 4]> = if fast_idx.len() >= ADAPTIVE_FAST_QUAD_PAR_MIN_CONTOURS && rayon::current_num_threads() > 1 {
        let results: Vec<(usize, Option<[CvPoint<f32>; 4]>)> = fast_idx
            .par_iter()
            .map_init(Vec::<CvPoint<f32>>::new, |pts_f32, &i| {
                let contour = &contour_buffer[i];
                contour_to_f32(&contour.pts, pts_f32, contour_off_x, contour_off_y);
                (i, candidate_quad_from_contour_fast(pts_f32, contour.perimeter, min_perimeter_for_area, &detector_config).filter(|quad| quad_passes_post_filters(quad, config, width, height)))
            })
            .collect();
        let mut out = Vec::with_capacity(results.len());
        for (i, quad) in results {
            fast_tested[i] = true;
            if let Some(quad) = quad {
                fast_success[i] = true;
                out.push(quad);
            }
        }
        out
    } else {
        let mut out = Vec::with_capacity(fast_idx.len());
        let mut pts_f32: Vec<CvPoint<f32>> = Vec::new();
        for i in fast_idx {
            fast_tested[i] = true;
            let contour = &contour_buffer[i];
            contour_to_f32(&contour.pts, &mut pts_f32, contour_off_x, contour_off_y);
            if let Some(quad) =
                candidate_quad_from_contour_fast(&pts_f32, contour.perimeter, min_perimeter_for_area, &detector_config).filter(|quad| quad_passes_post_filters(quad, config, width, height))
            {
                fast_success[i] = true;
                out.push(quad);
            }
        }
        out
    };

    // Fallback: when the fast path yields very few candidates (common under low contrast /
    // uneven lighting), run a slower but more forgiving quad estimation that can fall back
    // to a minimum-area rectangle. To keep this bounded, only consider the top-N contours by
    // perimeter (largest shapes first).
    if quads.len() < ADAPTIVE_FALLBACK_TRIGGER_QUADS_MAX {
        let fallback_cap = config.fallback_max_contours;
        let k = fallback_cap.min(contour_buffer.len());
        if k > 0 && k > fast_cap {
            // Avoid a full sort (O(n log n)) just to take the top-K. Partition is enough.
            let mut idx: Vec<usize> = (0..contour_buffer.len()).collect();
            idx.select_nth_unstable_by(k - 1, |&a, &b| contour_buffer[b].pts.len().cmp(&contour_buffer[a].pts.len()));

            // IMPORTANT: avoid cloning full contour point vectors here; we only need read-only
            // slices for the slower candidate filter.
            let top = &idx[..k];
            if k >= ADAPTIVE_FALLBACK_PAR_MIN_CONTOURS && rayon::current_num_threads() > 1 {
                let mut extra: Vec<[CvPoint<f32>; 4]> = top
                    .par_iter()
                    .filter(|&&i| !fast_success[i] && !fast_tested[i])
                    .map_init(Vec::<CvPoint<f32>>::new, |pts_f32, &i| {
                        contour_to_f32(&contour_buffer[i].pts, pts_f32, contour_off_x, contour_off_y);
                        candidate_quad_from_contour(pts_f32, &detector_config).filter(|quad| quad_passes_post_filters(quad, config, width, height))
                    })
                    .filter_map(|quad| quad)
                    .collect();
                quads.append(&mut extra);
            } else {
                let mut pts_f32: Vec<CvPoint<f32>> = Vec::new();
                for &i in top {
                    if fast_success[i] || fast_tested[i] {
                        continue;
                    }
                    contour_to_f32(&contour_buffer[i].pts, &mut pts_f32, contour_off_x, contour_off_y);
                    if let Some(quad) = candidate_quad_from_contour(&pts_f32, &detector_config).filter(|quad| quad_passes_post_filters(quad, config, width, height)) {
                        quads.push(quad);
                    }
                }
            }
        }
    }

    cap_quads_by_area(&mut quads, config.max_quads);
    quads
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
