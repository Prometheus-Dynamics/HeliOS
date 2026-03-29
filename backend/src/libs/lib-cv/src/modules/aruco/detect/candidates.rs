use super::*;
use geo::{LineString, Polygon, algorithm::minimum_rotated_rect::MinimumRotatedRect};

const FAST_DP_DOWNSAMPLE_MIN_LEN: usize = 1024;
const FAST_DP_DOWNSAMPLE_TARGET: usize = 384;
const MIN_RECT_FALLBACK_FILL_RATIO: f32 = 0.2;

fn contour_epsilon(perimeter: f32, config: &ArucoTagDetectorConfig) -> f32 {
    if config.epsilon <= 1.0 {
        // Interpret epsilon <= 1.0 as a perimeter rate (OpenCV-style).
        (perimeter * config.epsilon).max(0.01)
    } else {
        (perimeter * 0.01).clamp(config.epsilon * 0.5, config.epsilon * 2.5)
    }
}

#[inline(always)]
fn downsample_closed_contour_for_fast_dp<'a>(contour: &'a [Point<f32>], scratch: &'a mut Vec<Point<f32>>) -> &'a [Point<f32>] {
    let len = contour.len();
    if len < FAST_DP_DOWNSAMPLE_MIN_LEN {
        return contour;
    }
    let step = len.div_ceil(FAST_DP_DOWNSAMPLE_TARGET).max(2);
    scratch.clear();
    scratch.reserve(len / step + 2);
    for i in (0..len).step_by(step) {
        scratch.push(contour[i]);
    }
    if let (Some(&last_src), Some(&last_ds)) = (contour.last(), scratch.last())
        && (last_ds.x != last_src.x || last_ds.y != last_src.y)
    {
        scratch.push(last_src);
    }
    if scratch.len() < 8 {
        return contour;
    }
    scratch.as_slice()
}

#[inline(always)]
fn downsample_closed_contour_for_fast_dp_i32<'a>(contour: &'a [Point<i32>], scratch: &'a mut Vec<Point<i32>>) -> &'a [Point<i32>] {
    let len = contour.len();
    if len < FAST_DP_DOWNSAMPLE_MIN_LEN {
        return contour;
    }
    let step = len.div_ceil(FAST_DP_DOWNSAMPLE_TARGET).max(2);
    scratch.clear();
    scratch.reserve(len / step + 2);
    for i in (0..len).step_by(step) {
        scratch.push(contour[i]);
    }
    if let (Some(&last_src), Some(&last_ds)) = (contour.last(), scratch.last())
        && (last_ds.x != last_src.x || last_ds.y != last_src.y)
    {
        scratch.push(last_src);
    }
    if scratch.len() < 8 {
        return contour;
    }
    scratch.as_slice()
}

fn contour_perimeter_len(contour: &[Point<f32>]) -> f32 {
    contour.len() as f32
}

fn contour_area_abs(contour: &[Point<f32>]) -> f32 {
    if contour.len() < 3 {
        return 0.0;
    }
    let mut area = 0.0f32;
    for i in 0..contour.len() {
        let a = contour[i];
        let b = contour[(i + 1) % contour.len()];
        area += a.x * b.y - b.x * a.y;
    }
    area.abs() * 0.5
}

fn min_rotated_rect_quad(contour: &[Point<f32>]) -> Option<[Point<f32>; 4]> {
    if contour.len() < 3 {
        return None;
    }
    let coords: Vec<(f64, f64)> = contour.iter().map(|p| (p.x as f64, p.y as f64)).collect();
    let poly = Polygon::new(LineString::from(coords), vec![]);
    let rect = poly.minimum_rotated_rect()?;
    let pts: Vec<_> = rect.exterior().points().collect();
    if pts.len() < 4 {
        return None;
    }
    // `minimum_rotated_rect` exterior is closed, so we take the first 4 vertices.
    let mut quad = [Point::new(0.0f32, 0.0f32); 4];
    for (dst, src) in quad.iter_mut().zip(pts.iter().take(4)) {
        *dst = Point::new(src.x() as f32, src.y() as f32);
    }
    Some(quad)
}

fn filter_candidate(contour: &[Point<f32>], config: &ArucoTagDetectorConfig) -> Option<[Point<f32>; 4]> {
    if contour.len() < 4 {
        return None;
    }

    let perimeter = contour_perimeter_len(contour);
    if perimeter <= f32::EPSILON {
        return None;
    }
    if config.min_area > f32::EPSILON {
        // Conservative: if even the *maximum* possible perimeter length (all diagonal steps) is too
        // small to enclose `min_area`, skip this contour before doing any heavier work.
        //
        // Isoperimetric inequality: A <= P^2 / (4π)  =>  P >= sqrt(4πA).
        let min_perimeter = (4.0 * std::f32::consts::PI * config.min_area).sqrt();
        let max_step = std::f32::consts::SQRT_2;
        if perimeter * max_step < min_perimeter {
            return None;
        }
    }
    let adaptive_epsilon = contour_epsilon(perimeter, config);
    let result = with_detect_scratch(|scratch| {
        let (contour, approx) = {
            let DetectScratch { downsampled, approx, .. } = &mut *scratch;
            (downsample_closed_contour_for_fast_dp(contour, downsampled), approx)
        };
        crate::modules::contour::douglas_peucker::approx_poly_dp_into(contour, true, adaptive_epsilon, approx);
        if approx.first() == approx.last() {
            approx.pop();
        }
        let mut quad = if approx.len() == 4 {
            [approx[0], approx[1], approx[2], approx[3]]
        } else {
            // Fallback for low-light/noisy masks where contour simplification does not converge
            // to 4 vertices: use the minimum-rotated-rectangle hull as a candidate.
            let mut rect_quad = min_rotated_rect_quad(contour)?;
            sort_corners_clockwise(&mut rect_quad);
            rotate_corners_to_top_left(&mut rect_quad);

            let contour_area = contour_area_abs(contour);
            let rect_area = quad_area(&rect_quad);
            if rect_area <= f32::EPSILON {
                return None;
            }
            // Reject obviously poor fits (e.g. elongated/streak contours with tiny support).
            if contour_area / rect_area < MIN_RECT_FALLBACK_FILL_RATIO {
                return None;
            }
            rect_quad
        };
        if !quad_satisfies_config(&quad, config) {
            return None;
        }
        sort_corners_clockwise(&mut quad);
        rotate_corners_to_top_left(&mut quad);
        Some(quad)
    });
    report_detect_scratch();
    result
}

pub fn candidate_quad_from_contour(contour: &[Point<f32>], config: &ArucoTagDetectorConfig) -> Option<[Point<f32>; 4]> {
    filter_candidate(contour, config)
}

pub fn candidate_quad_from_contour_fast_in(
    contour: &[Point<f32>],
    perimeter: f32,
    min_perimeter: Option<f32>,
    config: &ArucoTagDetectorConfig,
    downsampled: &mut Vec<Point<f32>>,
    approx: &mut Vec<Point<f32>>,
) -> Option<[Point<f32>; 4]> {
    if contour.len() < 4 {
        return None;
    }
    if perimeter <= f32::EPSILON {
        return None;
    }
    if let Some(min_perimeter) = min_perimeter
        && config.min_area > f32::EPSILON
    {
        let max_step = std::f32::consts::SQRT_2;
        if perimeter * max_step < min_perimeter {
            return None;
        }
    }

    let adaptive_epsilon = contour_epsilon(perimeter, config);
    let contour = downsample_closed_contour_for_fast_dp(contour, downsampled);
    crate::modules::contour::douglas_peucker::approx_poly_dp_closed_fast_into(contour, adaptive_epsilon, approx);
    if approx.first() == approx.last() {
        approx.pop();
    }
    if approx.len() != 4 {
        return None;
    }

    let mut quad = [approx[0], approx[1], approx[2], approx[3]];
    sort_corners_clockwise(&mut quad);
    rotate_corners_to_top_left(&mut quad);
    if !quad_satisfies_config(&quad, config) {
        return None;
    }
    Some(quad)
}

pub fn candidate_quad_from_contour_fast_i32_in(
    contour: &[Point<i32>],
    perimeter: f32,
    min_perimeter: Option<f32>,
    config: &ArucoTagDetectorConfig,
    downsampled: &mut Vec<Point<i32>>,
    approx: &mut Vec<Point<i32>>,
) -> Option<[Point<f32>; 4]> {
    if contour.len() < 4 {
        return None;
    }
    if perimeter <= f32::EPSILON {
        return None;
    }
    if let Some(min_perimeter) = min_perimeter
        && config.min_area > f32::EPSILON
    {
        let max_step = std::f32::consts::SQRT_2;
        if perimeter * max_step < min_perimeter {
            return None;
        }
    }

    let adaptive_epsilon = contour_epsilon(perimeter, config);
    let contour = downsample_closed_contour_for_fast_dp_i32(contour, downsampled);
    crate::modules::contour::douglas_peucker::approx_poly_dp_closed_fast_i32_into(contour, adaptive_epsilon, approx);
    if approx.first() == approx.last() {
        approx.pop();
    }
    if approx.len() != 4 {
        return None;
    }

    let mut quad = [
        Point::new(approx[0].x as f32, approx[0].y as f32),
        Point::new(approx[1].x as f32, approx[1].y as f32),
        Point::new(approx[2].x as f32, approx[2].y as f32),
        Point::new(approx[3].x as f32, approx[3].y as f32),
    ];
    sort_corners_clockwise(&mut quad);
    rotate_corners_to_top_left(&mut quad);
    if !quad_satisfies_config(&quad, config) {
        return None;
    }
    Some(quad)
}

pub fn candidate_quad_from_contour_fast(contour: &[Point<f32>], perimeter: f32, min_perimeter: Option<f32>, config: &ArucoTagDetectorConfig) -> Option<[Point<f32>; 4]> {
    let result = with_detect_scratch(|scratch| {
        let (downsampled, approx) = {
            let DetectScratch { downsampled, approx, .. } = &mut *scratch;
            (downsampled, approx)
        };
        candidate_quad_from_contour_fast_in(contour, perimeter, min_perimeter, config, downsampled, approx)
    });
    report_detect_scratch();
    result
}

pub fn filter_candidates(contours: &[Vec<Point<f32>>], config: &ArucoTagDetectorConfig) -> Vec<[Point<f32>; 4]> {
    if contours.len() < 64 || rayon::current_num_threads() <= 1 {
        contours.iter().filter_map(|contour| filter_candidate(contour, config)).collect()
    } else {
        contours.par_iter().filter_map(|contour| filter_candidate(contour, config)).collect()
    }
}

/// Faster candidate filtering when per-contour perimeters are already known.
///
/// This skips the minimum-area-rect fallback (which allocates and is slower), and only accepts
/// contours that simplify cleanly to a 4-corner polygon. This is tuned for well-printed ArUco tags
/// where edges are high-contrast and mostly complete.
pub fn filter_candidates_fast(contours: &[(Vec<Point<f32>>, f32)], config: &ArucoTagDetectorConfig) -> Vec<[Point<f32>; 4]> {
    let min_perimeter = if config.min_area > f32::EPSILON { Some((4.0 * std::f32::consts::PI * config.min_area).sqrt()) } else { None };
    if contours.len() < 64 || rayon::current_num_threads() <= 1 {
        contours.iter().filter_map(|(contour, perimeter)| candidate_quad_from_contour_fast(contour, *perimeter, min_perimeter, config)).collect()
    } else {
        contours.par_iter().filter_map(|(contour, perimeter)| candidate_quad_from_contour_fast(contour, *perimeter, min_perimeter, config)).collect()
    }
}

pub fn quad_satisfies_config(quad: &[Point<f32>; 4], config: &ArucoTagDetectorConfig) -> bool {
    if !is_convex_quad(quad) {
        return false;
    }
    let area = quad_area(quad);
    if area < config.min_area {
        return false;
    }
    if let Some(max_area) = config.max_area
        && area > max_area
    {
        return false;
    }
    if !angles_within_cos_range(quad, config.angle_cos_min, config.angle_cos_max) {
        return false;
    }

    let mut min_edge = f32::MAX;
    let mut max_edge = 0.0f32;
    let mut sum_edge = 0.0f32;
    let mut sum_edge_sq = 0.0f32;
    for i in 0..4 {
        let len = distance(&quad[i], &quad[(i + 1) % 4]);
        sum_edge += len;
        sum_edge_sq += len * len;
        if len < min_edge {
            min_edge = len;
        }
        if len > max_edge {
            max_edge = len;
        }
    }
    let mean = sum_edge * 0.25;
    if mean <= f32::EPSILON {
        return false;
    }
    let variance = (sum_edge_sq * 0.25) - (mean * mean);
    // stddev/mean <= max_side_cv  <=>  variance <= (max_side_cv^2) * mean^2
    if variance > (config.max_side_cv * config.max_side_cv) * (mean * mean) {
        return false;
    }

    if config.max_side_ratio > 0.0 && (min_edge <= f32::EPSILON || (max_edge / min_edge) > config.max_side_ratio) {
        return false;
    }

    if config.max_diag_ratio > 0.0 {
        let dx0 = quad[0].x - quad[2].x;
        let dy0 = quad[0].y - quad[2].y;
        let dx1 = quad[1].x - quad[3].x;
        let dy1 = quad[1].y - quad[3].y;
        let d0_sq = dx0 * dx0 + dy0 * dy0;
        let d1_sq = dx1 * dx1 + dy1 * dy1;
        let (min_diag_sq, max_diag_sq) = if d0_sq <= d1_sq { (d0_sq, d1_sq) } else { (d1_sq, d0_sq) };
        if min_diag_sq <= f32::EPSILON {
            return false;
        }
        let ratio_sq = config.max_diag_ratio * config.max_diag_ratio;
        if max_diag_sq > ratio_sq * min_diag_sq {
            return false;
        }
    }

    true
}

pub(super) fn quad_area(points: &[Point<f32>; 4]) -> f32 {
    let mut area = 0.0;
    for i in 0..4 {
        let j = (i + 1) % 4;
        area += points[i].x * points[j].y - points[j].x * points[i].y;
    }
    area.abs() * 0.5
}
