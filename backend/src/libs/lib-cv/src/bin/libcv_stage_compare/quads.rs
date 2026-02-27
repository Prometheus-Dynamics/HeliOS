use std::error::Error;
use std::path::Path;

use image::{DynamicImage, Rgba};
use imageproc::point::Point as CvPoint;

use lib_cv::modules::aruco::detect::ArucoTagDetectorConfig;
use lib_cv::modules::contour::douglas_peucker::approx_poly_dp;
use lib_cv::modules::draw::contour::overlay_contour_points;

use crate::env_util::env_bool;
use crate::types::{CandidateQuadsConfig, FilterStats};

#[derive(Debug, Default)]
pub(crate) struct CandidateQuadStages {
    pub(crate) contours_after_perimeter: Vec<Vec<CvPoint<f32>>>,
    pub(crate) approx_polys: Vec<ApproxDumpItem>,
    pub(crate) quads_after_approx: Vec<[CvPoint<f32>; 4]>,
    pub(crate) quads_after_convex: Vec<[CvPoint<f32>; 4]>,
    pub(crate) quads_after_angle: Vec<[CvPoint<f32>; 4]>,
    pub(crate) quads_after_area: Vec<[CvPoint<f32>; 4]>,
    pub(crate) quads_after_side_cv: Vec<[CvPoint<f32>; 4]>,
    pub(crate) quads_after_side_ratio: Vec<[CvPoint<f32>; 4]>,
    pub(crate) quads_after_diag_ratio: Vec<[CvPoint<f32>; 4]>,
    pub(crate) quads_after_candidate: Vec<[CvPoint<f32>; 4]>,
    pub(crate) quads_after_min_corner: Vec<[CvPoint<f32>; 4]>,
    pub(crate) quads_after_min_border: Vec<[CvPoint<f32>; 4]>,
    pub(crate) quads_after_marker_distance: Vec<[CvPoint<f32>; 4]>,
}

pub(crate) fn candidate_quads(contours: &[Vec<CvPoint<f32>>], cfg: &CandidateQuadsConfig) -> (Vec<[CvPoint<f32>; 4]>, FilterStats, CandidateQuadStages) {
    let mut stats = FilterStats { input_contours: contours.len(), ..Default::default() };
    if contours.is_empty() {
        return (Vec::new(), stats, CandidateQuadStages::default());
    }

    let downscale = cfg.downscale.max(1);
    let scale = downscale as f32;
    let area_scale = (scale * scale).max(1.0);
    let (max_dim, max_x, max_y) = if cfg.frame_width > 0 && cfg.frame_height > 0 {
        let max_dim = cfg.frame_width.max(cfg.frame_height) as f32;
        let max_x = (cfg.frame_width.saturating_sub(1)) as f32;
        let max_y = (cfg.frame_height.saturating_sub(1)) as f32;
        (max_dim.max(0.0), max_x.max(0.0), max_y.max(0.0))
    } else {
        let mut max_x = 0.0f32;
        let mut max_y = 0.0f32;
        for contour in contours {
            for p in contour {
                if p.x > max_x {
                    max_x = p.x;
                }
                if p.y > max_y {
                    max_y = p.y;
                }
            }
        }
        let max_dim = (max_x.max(max_y) + 1.0).max(0.0);
        (max_dim, max_x, max_y)
    };
    let min_perimeter_rate_px = if max_dim > 0.0 && cfg.min_perimeter_rate > 0.0 { Some(cfg.min_perimeter_rate * max_dim) } else { None };
    let max_perimeter_rate_px = if max_dim > 0.0 && cfg.max_perimeter_rate > 0.0 { Some(cfg.max_perimeter_rate * max_dim) } else { None };

    let detect_cfg = ArucoTagDetectorConfig {
        epsilon: cfg.epsilon.clamp(0.01, 1000.0),
        min_area: (cfg.min_area / area_scale).max(0.0),
        max_area: if cfg.max_area > 0.0 { Some((cfg.max_area / area_scale).max(0.0)) } else { None },
        min_angle_deg: cfg.min_angle_deg.clamp(0.0, 180.0),
        max_angle_deg: cfg.max_angle_deg.clamp(0.0, 180.0),
        max_side_cv: cfg.max_side_cv.clamp(0.05, 10.0),
        max_side_ratio: cfg.max_side_ratio.max(0.0),
        max_diag_ratio: cfg.max_diag_ratio.max(0.0),
        angle_cos_min: -1.0,
        angle_cos_max: 1.0,
    }
    .with_angle_cos_bounds();

    let min_perimeter = if detect_cfg.min_area > f32::EPSILON { Some((4.0 * std::f32::consts::PI * detect_cfg.min_area).sqrt()) } else { None };
    let max_step = std::f32::consts::SQRT_2;

    if cfg.refine_corners {
        eprintln!("note: refine_corners requested but not applied (matches OpenCV defaults: false)");
    }
    let _ = (cfg.refine_edge_dist, cfg.refine_min_points);

    let mut stages = CandidateQuadStages::default();
    let mut quads: Vec<[CvPoint<f32>; 4]> = Vec::new();
    let mut quad_perimeters: Vec<f32> = Vec::new();
    for (contour_index, contour) in contours.iter().enumerate() {
        if contour.len() < 4 {
            stats.rejected_contour_too_short += 1;
            continue;
        }
        let contour_points = if cfg.simplify_contours { simplify_contour_chain(contour) } else { contour.clone() };
        if contour_points.len() < 4 {
            stats.rejected_contour_too_short += 1;
            continue;
        }
        let perimeter = contour_perimeter_len(&contour_points);
        if let Some(min_rate_px) = min_perimeter_rate_px
            && perimeter < min_rate_px
        {
            stats.rejected_min_perimeter_rate += 1;
            continue;
        }
        if let Some(max_rate_px) = max_perimeter_rate_px
            && perimeter > max_rate_px
        {
            stats.rejected_max_perimeter_rate += 1;
            continue;
        }
        if let Some(min_perimeter) = min_perimeter
            && perimeter * max_step < min_perimeter
        {
            stats.rejected_min_perimeter += 1;
            continue;
        }
        if downscale != 1 {
            stages.contours_after_perimeter.push(scale_contour(&contour_points, scale));
        } else {
            stages.contours_after_perimeter.push(contour_points.clone());
        }
        if env_bool("LIBCV_DUMP_APPROX", false) {
            let adaptive_epsilon = contour_epsilon(perimeter, &detect_cfg);
            let mut approx = approx_poly_dp(&contour_points, true, adaptive_epsilon);
            if approx.first() == approx.last() {
                approx.pop();
            }
            let points = approx.into_iter().map(|p| [p.x, p.y]).collect::<Vec<_>>();
            stages.approx_polys.push(ApproxDumpItem { contour_index, perimeter, epsilon: adaptive_epsilon, points });
        }

        let staged = staged_quad_from_contour(&contour_points, perimeter, &detect_cfg, cfg.mode_fast);
        let Some(quad) = staged.quad_final else {
            stats.rejected_candidate_quad += 1;
            let reason = diagnose_quad_reject(&contour_points, perimeter, &detect_cfg);
            match reason {
                Some(QuadReject::NotFour) => stats.rejected_quad_not_four += 1,
                Some(QuadReject::NotConvex) => stats.rejected_quad_not_convex += 1,
                Some(QuadReject::Angle) => stats.rejected_quad_angle += 1,
                Some(QuadReject::Area) => stats.rejected_quad_area += 1,
                Some(QuadReject::MaxSideCv) => stats.rejected_quad_max_side_cv += 1,
                Some(QuadReject::MaxSideRatio) => stats.rejected_quad_max_side_ratio += 1,
                Some(QuadReject::MaxDiagRatio) => stats.rejected_quad_max_diag_ratio += 1,
                None => {}
            }
            continue;
        };
        let quad_scaled = if downscale != 1 { scale_quad(&quad, scale) } else { quad };
        if let Some(quad) = staged.quad_after_approx {
            stages.quads_after_approx.push(if downscale != 1 { scale_quad(&quad, scale) } else { quad });
        }
        if let Some(quad) = staged.quad_after_convex {
            stages.quads_after_convex.push(if downscale != 1 { scale_quad(&quad, scale) } else { quad });
        }
        if let Some(quad) = staged.quad_after_angle {
            stages.quads_after_angle.push(if downscale != 1 { scale_quad(&quad, scale) } else { quad });
        }
        if let Some(quad) = staged.quad_after_area {
            stages.quads_after_area.push(if downscale != 1 { scale_quad(&quad, scale) } else { quad });
        }
        if let Some(quad) = staged.quad_after_side_cv {
            stages.quads_after_side_cv.push(if downscale != 1 { scale_quad(&quad, scale) } else { quad });
        }
        if let Some(quad) = staged.quad_after_side_ratio {
            stages.quads_after_side_ratio.push(if downscale != 1 { scale_quad(&quad, scale) } else { quad });
        }
        if let Some(quad) = staged.quad_after_diag_ratio {
            stages.quads_after_diag_ratio.push(if downscale != 1 { scale_quad(&quad, scale) } else { quad });
        }
        stages.quads_after_candidate.push(quad_scaled);

        if cfg.min_corner_distance_rate > 0.0 {
            let mut min_edge = f32::MAX;
            for i in 0..4 {
                let a = quad[i];
                let b = quad[(i + 1) % 4];
                let dx = a.x - b.x;
                let dy = a.y - b.y;
                min_edge = min_edge.min((dx * dx + dy * dy).sqrt());
            }
            if min_edge < perimeter * cfg.min_corner_distance_rate {
                stats.rejected_min_corner_distance += 1;
                continue;
            }
        }
        stages.quads_after_min_corner.push(quad_scaled);

        if cfg.min_distance_to_border_px > 0.0 && max_dim > 0.0 && quad_too_near_border(&quad_scaled, max_x, max_y, cfg.min_distance_to_border_px) {
            stats.rejected_min_border_distance += 1;
            continue;
        }
        stages.quads_after_min_border.push(quad_scaled);

        quads.push(quad_scaled);
        quad_perimeters.push(quad_perimeter(&quad_scaled));
    }
    stats.kept_after_basic_filters = quads.len();

    if quads.is_empty() {
        stats.kept_after_marker_distance = 0;
        return (quads, stats, stages);
    }

    let mut order: Vec<usize> = (0..quads.len()).collect();
    order.sort_by(|a, b| quad_perimeters[*b].total_cmp(&quad_perimeters[*a]));

    let mut group_id = vec![-1isize; order.len()];
    let mut grouped: Vec<Vec<usize>> = Vec::new();
    let mut is_selected = vec![true; order.len()];

    for i in 0..order.len() {
        for j in (i + 1)..order.len() {
            let idx_i = order[i];
            let idx_j = order[j];
            let dist = quad_average_corner_distance(&quads[idx_i], &quads[idx_j]);
            if dist < quad_perimeters[idx_j] * cfg.min_marker_distance_rate {
                is_selected[i] = false;
                is_selected[j] = false;
                if group_id[i] < 0 && group_id[j] < 0 {
                    group_id[i] = grouped.len() as isize;
                    group_id[j] = grouped.len() as isize;
                    grouped.push(vec![i, j]);
                } else if group_id[i] >= 0 && group_id[j] < 0 {
                    let g = group_id[i] as usize;
                    group_id[j] = g as isize;
                    grouped[g].push(j);
                } else if group_id[j] >= 0 && group_id[i] < 0 {
                    let g = group_id[j] as usize;
                    group_id[i] = g as isize;
                    grouped[g].push(i);
                }
            }
        }
        if is_selected[i] {
            is_selected[i] = false;
            group_id[i] = grouped.len() as isize;
            grouped.push(vec![i]);
        }
    }

    let mut keep_sorted = vec![false; order.len()];
    for mut group in grouped {
        group.sort_unstable();
        let curr = group[0];
        let quad = &quads[order[curr]];
        if cfg.min_distance_to_border_px > 0.0 && max_dim > 0.0 && quad_too_near_border(quad, max_x, max_y, cfg.min_distance_to_border_px) {
            stats.rejected_min_border_distance += 1;
            continue;
        }
        keep_sorted[curr] = true;
    }

    let mut filtered = Vec::with_capacity(quads.len());
    for (sorted_idx, keep) in keep_sorted.into_iter().enumerate() {
        if keep {
            filtered.push(quads[order[sorted_idx]]);
        }
    }

    stats.rejected_marker_distance = stats.kept_after_basic_filters.saturating_sub(filtered.len());
    stats.kept_after_marker_distance = filtered.len();
    stages.quads_after_marker_distance = filtered.clone();
    (filtered, stats, stages)
}

fn scale_contour(contour: &[CvPoint<f32>], scale: f32) -> Vec<CvPoint<f32>> {
    contour.iter().map(|p| CvPoint::new(p.x * scale, p.y * scale)).collect()
}

fn contour_perimeter_len(contour: &[CvPoint<f32>]) -> f32 {
    if contour.len() < 2 {
        return 0.0;
    }
    let mut perimeter = 0.0f32;
    for i in 0..contour.len() {
        let a = contour[i];
        let b = contour[(i + 1) % contour.len()];
        let dx = a.x - b.x;
        let dy = a.y - b.y;
        perimeter += (dx * dx + dy * dy).sqrt();
    }
    perimeter
}

fn scale_quad(quad: &[CvPoint<f32>; 4], scale: f32) -> [CvPoint<f32>; 4] {
    [
        CvPoint::new(quad[0].x * scale, quad[0].y * scale),
        CvPoint::new(quad[1].x * scale, quad[1].y * scale),
        CvPoint::new(quad[2].x * scale, quad[2].y * scale),
        CvPoint::new(quad[3].x * scale, quad[3].y * scale),
    ]
}

fn simplify_contour_chain(contour: &[CvPoint<f32>]) -> Vec<CvPoint<f32>> {
    if contour.len() < 3 {
        return contour.to_vec();
    }
    let mut out = Vec::with_capacity(contour.len());
    let len = contour.len();
    for i in 0..len {
        let prev = contour[(i + len - 1) % len];
        let curr = contour[i];
        let next = contour[(i + 1) % len];
        let dx1 = (curr.x - prev.x).signum() as i32;
        let dy1 = (curr.y - prev.y).signum() as i32;
        let dx2 = (next.x - curr.x).signum() as i32;
        let dy2 = (next.y - curr.y).signum() as i32;
        if dx1 == dx2 && dy1 == dy2 {
            continue;
        }
        out.push(curr);
    }
    if out.len() < 3 { contour.to_vec() } else { out }
}

pub(crate) fn dump_stage_quads(input: &DynamicImage, quads: &[[CvPoint<f32>; 4]], output_dir: &Path, label: &str) -> Result<(), Box<dyn Error>> {
    let json_path = output_dir.join(format!("libcv_{}.json", label));
    dump_quads_json(quads, &json_path)?;
    let mut overlay = input.clone();
    for quad in quads {
        overlay_contour_points(&mut overlay, quad, 2, Rgba([0, 255, 0, 255]));
    }
    let png_path = output_dir.join(format!("libcv_{}.png", label));
    overlay.save(png_path)?;
    Ok(())
}

pub(crate) fn dump_quads_json(quads: &[[CvPoint<f32>; 4]], path: &Path) -> Result<(), Box<dyn Error>> {
    let mut out = Vec::with_capacity(quads.len());
    for quad in quads {
        let corners: Vec<[f32; 2]> = quad.iter().map(|p| [p.x, p.y]).collect();
        out.push(corners);
    }
    let file = std::fs::File::create(path)?;
    serde_json::to_writer_pretty(file, &out)?;
    Ok(())
}

#[derive(Debug, Default, serde::Serialize)]
pub(crate) struct ApproxDumpItem {
    pub(crate) contour_index: usize,
    pub(crate) perimeter: f32,
    pub(crate) epsilon: f32,
    pub(crate) points: Vec<[f32; 2]>,
}

pub(crate) fn dump_approx_json(items: &[ApproxDumpItem], path: &Path) -> Result<(), Box<dyn Error>> {
    std::fs::write(path, serde_json::to_vec(items)?)?;
    Ok(())
}

#[derive(Default)]
struct QuadStageResult {
    quad_after_approx: Option<[CvPoint<f32>; 4]>,
    quad_after_convex: Option<[CvPoint<f32>; 4]>,
    quad_after_angle: Option<[CvPoint<f32>; 4]>,
    quad_after_area: Option<[CvPoint<f32>; 4]>,
    quad_after_side_cv: Option<[CvPoint<f32>; 4]>,
    quad_after_side_ratio: Option<[CvPoint<f32>; 4]>,
    quad_after_diag_ratio: Option<[CvPoint<f32>; 4]>,
    quad_final: Option<[CvPoint<f32>; 4]>,
}

fn staged_quad_from_contour(contour: &[CvPoint<f32>], perimeter: f32, config: &ArucoTagDetectorConfig, mode_fast: bool) -> QuadStageResult {
    let mut out = QuadStageResult::default();
    if contour.len() < 4 || perimeter <= f32::EPSILON {
        return out;
    }
    if mode_fast && config.min_area > f32::EPSILON {
        let min_perimeter = (4.0 * std::f32::consts::PI * config.min_area).sqrt();
        let max_step = std::f32::consts::SQRT_2;
        if perimeter * max_step < min_perimeter {
            return out;
        }
    }
    let adaptive_epsilon = contour_epsilon(perimeter, config);
    let mut approx = approx_poly_dp(contour, true, adaptive_epsilon);
    if approx.first() == approx.last() {
        approx.pop();
    }
    if approx.len() != 4 {
        return out;
    }
    let mut quad = [approx[0], approx[1], approx[2], approx[3]];
    sort_corners_clockwise(&mut quad);
    out.quad_after_approx = Some(quad);

    if !is_convex_quad(&quad) {
        return out;
    }
    out.quad_after_convex = Some(quad);

    if !angles_within_cos_range(&quad, config.angle_cos_min, config.angle_cos_max) {
        return out;
    }
    out.quad_after_angle = Some(quad);

    let area = quad_area(&quad);
    if area < config.min_area {
        return out;
    }
    if let Some(max_area) = config.max_area
        && area > max_area
    {
        return out;
    }
    out.quad_after_area = Some(quad);

    let mut edges = [0.0f32; 4];
    for i in 0..4 {
        edges[i] = distance(&quad[i], &quad[(i + 1) % 4]);
    }
    let mean = edges.iter().copied().sum::<f32>() / 4.0;
    if mean <= f32::EPSILON {
        return out;
    }
    let variance = edges
        .iter()
        .map(|len| {
            let delta = len - mean;
            delta * delta
        })
        .sum::<f32>()
        / 4.0;
    if variance > (config.max_side_cv * config.max_side_cv) * (mean * mean) {
        return out;
    }
    out.quad_after_side_cv = Some(quad);

    if config.max_side_ratio > 0.0 {
        let mut min_edge = f32::MAX;
        let mut max_edge = 0.0f32;
        for edge in edges {
            if edge < min_edge {
                min_edge = edge;
            }
            if edge > max_edge {
                max_edge = edge;
            }
        }
        if min_edge <= f32::EPSILON || (max_edge / min_edge) > config.max_side_ratio {
            return out;
        }
    }
    out.quad_after_side_ratio = Some(quad);

    if config.max_diag_ratio > 0.0 {
        let d0 = distance(&quad[0], &quad[2]);
        let d1 = distance(&quad[1], &quad[3]);
        let min_diag = d0.min(d1);
        let max_diag = d0.max(d1);
        if min_diag <= f32::EPSILON || (max_diag / min_diag) > config.max_diag_ratio {
            return out;
        }
    }
    out.quad_after_diag_ratio = Some(quad);

    out.quad_final = Some(quad);
    out
}

#[derive(Clone, Copy, Debug)]
enum QuadReject {
    NotFour,
    NotConvex,
    Angle,
    Area,
    MaxSideCv,
    MaxSideRatio,
    MaxDiagRatio,
}

fn diagnose_quad_reject(contour: &[CvPoint<f32>], perimeter: f32, config: &ArucoTagDetectorConfig) -> Option<QuadReject> {
    if contour.len() < 4 || perimeter <= f32::EPSILON {
        return Some(QuadReject::NotFour);
    }
    let adaptive_epsilon = contour_epsilon(perimeter, config);
    let mut approx = approx_poly_dp(contour, true, adaptive_epsilon);
    if approx.first() == approx.last() {
        approx.pop();
    }
    if approx.len() != 4 {
        return Some(QuadReject::NotFour);
    }
    let mut quad = [approx[0], approx[1], approx[2], approx[3]];
    sort_corners_clockwise(&mut quad);
    if !is_convex_quad(&quad) {
        return Some(QuadReject::NotConvex);
    }
    if !angles_within_cos_range(&quad, config.angle_cos_min, config.angle_cos_max) {
        return Some(QuadReject::Angle);
    }
    let area = quad_area(&quad);
    if area < config.min_area {
        return Some(QuadReject::Area);
    }
    if let Some(max_area) = config.max_area
        && area > max_area
    {
        return Some(QuadReject::Area);
    }
    let mut edges = [0.0f32; 4];
    for i in 0..4 {
        edges[i] = distance(&quad[i], &quad[(i + 1) % 4]);
    }
    let mean = edges.iter().copied().sum::<f32>() / 4.0;
    if mean <= f32::EPSILON {
        return Some(QuadReject::MaxSideCv);
    }
    let variance = edges
        .iter()
        .map(|len| {
            let delta = len - mean;
            delta * delta
        })
        .sum::<f32>()
        / 4.0;
    if variance > (config.max_side_cv * config.max_side_cv) * (mean * mean) {
        return Some(QuadReject::MaxSideCv);
    }
    if config.max_side_ratio > 0.0 {
        let mut min_edge = f32::MAX;
        let mut max_edge = 0.0f32;
        for edge in edges {
            if edge < min_edge {
                min_edge = edge;
            }
            if edge > max_edge {
                max_edge = edge;
            }
        }
        if min_edge <= f32::EPSILON || (max_edge / min_edge) > config.max_side_ratio {
            return Some(QuadReject::MaxSideRatio);
        }
    }
    if config.max_diag_ratio > 0.0 {
        let d0 = distance(&quad[0], &quad[2]);
        let d1 = distance(&quad[1], &quad[3]);
        let min_diag = d0.min(d1);
        let max_diag = d0.max(d1);
        if min_diag <= f32::EPSILON || (max_diag / min_diag) > config.max_diag_ratio {
            return Some(QuadReject::MaxDiagRatio);
        }
    }
    None
}

fn contour_epsilon(perimeter: f32, config: &ArucoTagDetectorConfig) -> f32 {
    if config.epsilon <= 1.0 { (perimeter * config.epsilon).max(0.01) } else { (perimeter * 0.01).clamp(config.epsilon * 0.5, config.epsilon * 2.5) }
}

fn sort_corners_clockwise(points: &mut [CvPoint<f32>; 4]) {
    let dx1 = points[1].x - points[0].x;
    let dy1 = points[1].y - points[0].y;
    let dx2 = points[2].x - points[0].x;
    let dy2 = points[2].y - points[0].y;
    let cross = (dx1 * dy2) - (dy1 * dx2);
    if cross < 0.0 {
        points.swap(1, 3);
    }
}

fn is_convex_quad(points: &[CvPoint<f32>; 4]) -> bool {
    let mut prev_cross = 0.0;
    for i in 0..4 {
        let p0 = points[i];
        let p1 = points[(i + 1) % 4];
        let p2 = points[(i + 2) % 4];
        let v1 = (p1.x - p0.x, p1.y - p0.y);
        let v2 = (p2.x - p1.x, p2.y - p1.y);
        let cross = v1.0 * v2.1 - v1.1 * v2.0;
        if cross.abs() < f32::EPSILON {
            return false;
        }
        if prev_cross == 0.0 {
            prev_cross = cross;
        } else if prev_cross * cross < 0.0 {
            return false;
        }
    }
    true
}

#[inline(always)]
fn angles_within_cos_range(points: &[CvPoint<f32>; 4], cos_min: f32, cos_max: f32) -> bool {
    for i in 0..4 {
        let prev = points[(i + 3) & 3];
        let curr = points[i];
        let next = points[(i + 1) & 3];

        let abx = prev.x - curr.x;
        let aby = prev.y - curr.y;
        let cbx = next.x - curr.x;
        let cby = next.y - curr.y;

        let dot = abx * cbx + aby * cby;
        let ab2 = abx * abx + aby * aby;
        let cb2 = cbx * cbx + cby * cby;
        if ab2 <= f32::EPSILON || cb2 <= f32::EPSILON {
            return false;
        }
        let cos = dot / (ab2 * cb2).sqrt();
        if cos < cos_min || cos > cos_max {
            return false;
        }
    }
    true
}

fn distance(a: &CvPoint<f32>, b: &CvPoint<f32>) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

fn quad_area(points: &[CvPoint<f32>; 4]) -> f32 {
    let mut area = 0.0;
    for i in 0..4 {
        let j = (i + 1) % 4;
        area += points[i].x * points[j].y - points[j].x * points[i].y;
    }
    area.abs() * 0.5
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

fn quad_too_near_border(quad: &[CvPoint<f32>; 4], max_x: f32, max_y: f32, min_border: f32) -> bool {
    for p in quad {
        if p.x < min_border || p.y < min_border || p.x > max_x - min_border || p.y > max_y - min_border {
            return true;
        }
    }
    false
}
