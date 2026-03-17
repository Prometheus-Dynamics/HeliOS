use super::super::*;

pub(super) fn fit_edge_line(points: &[CvPoint<f32>]) -> Option<(f32, f32, f32)> {
    if points.len() < 2 {
        return None;
    }

    let mut mx = 0.0f32;
    let mut my = 0.0f32;

    let inv = 1.0f32 / points.len() as f32;
    for p in points {
        mx += p.x;
        my += p.y;
    }
    mx *= inv;
    my *= inv;

    let mut sxx = 0.0f32;
    let mut sxy = 0.0f32;
    let mut syy = 0.0f32;
    for p in points {
        let dx = p.x - mx;
        let dy = p.y - my;
        sxx += dx * dx;
        sxy += dx * dy;
        syy += dy * dy;
    }
    if (sxx + syy) <= f32::EPSILON {
        return None;
    }

    let theta = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    let (sin_t, cos_t) = theta.sin_cos();
    let nx = -sin_t;
    let ny = cos_t;
    let c = nx * mx + ny * my;
    Some((nx, ny, c))
}

pub(super) fn fit_edge_line_refined_with_buffers(
    points: &[CvPoint<f32>],
    trim_ratio: f32,
    base: (f32, f32, f32),
    distances: &mut Vec<(f32, CvPoint<f32>)>,
    trimmed: &mut Vec<CvPoint<f32>>,
) -> Option<(f32, f32, f32)> {
    let (nx, ny, c) = base;
    distances.clear();
    distances.reserve(points.len());
    for &p in points {
        let d = (nx * p.x + ny * p.y - c).abs();
        distances.push((d, p));
    }
    distances.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let keep = ((points.len() as f32) * (1.0 - trim_ratio)).round() as usize;
    let keep = keep.clamp(2, points.len());
    trimmed.clear();
    trimmed.reserve(keep);
    for (_, point) in distances.iter().take(keep) {
        trimmed.push(*point);
    }

    fit_edge_line(trimmed).or(Some(base))
}

pub(super) fn fit_edge_line_ransac(points: &[CvPoint<f32>], max_dist: f32, iters: usize) -> Option<(f32, f32, f32)> {
    if points.len() < 2 {
        return None;
    }
    let iters = iters.clamp(8, 128);
    let result = ARUCO_NODE_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        let NodeDetectScratch { inliers, best_inliers, distances, trimmed, .. } = &mut *scratch;
        best_inliers.clear();
        let mut state = (points.len() as u32).wrapping_mul(2654435761);

        for _ in 0..iters {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            let i = (state as usize) % points.len();
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            let j = (state as usize) % points.len();
            if i == j {
                continue;
            }
            let a = points[i];
            let b = points[j];
            let dx = b.x - a.x;
            let dy = b.y - a.y;
            let len2 = dx * dx + dy * dy;
            if len2 <= f32::EPSILON {
                continue;
            }
            let inv_len = 1.0f32 / len2.sqrt();
            inliers.clear();
            for &p in points {
                let px = p.x - a.x;
                let py = p.y - a.y;
                let dist = (px * dy - py * dx).abs() * inv_len;
                if dist <= max_dist {
                    inliers.push(p);
                }
            }
            if inliers.len() > best_inliers.len() {
                std::mem::swap(inliers, best_inliers);
                if best_inliers.len() == points.len() {
                    break;
                }
            }
        }

        let base_slice = if best_inliers.len() >= 2 { best_inliers.as_slice() } else { points };
        let base = fit_edge_line(base_slice)?;
        fit_edge_line_refined_with_buffers(base_slice, 0.2, base, distances, trimmed)
    });
    report_node_detect_scratch();
    result
}

pub(super) fn intersect_lines(l1: (f32, f32, f32), l2: (f32, f32, f32)) -> Option<CvPoint<f32>> {
    let (a1, b1, c1) = l1;
    let (a2, b2, c2) = l2;
    let det = a1 * b2 - b1 * a2;
    if det.abs() <= 1.0e-6 {
        return None;
    }
    let x = (c1 * b2 - b1 * c2) / det;
    let y = (a1 * c2 - c1 * a2) / det;
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    Some(CvPoint::new(x, y))
}

pub(super) fn quad_perimeter(quad: &[CvPoint<f32>; 4]) -> f32 {
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

#[inline(always)]
pub(super) fn quad_perimeter_and_min_edge_sq(quad: &[CvPoint<f32>; 4]) -> (f32, f32) {
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

#[inline(always)]
pub(super) fn quad_perimeter_and_min_edge_sq_f64(quad: &Quad) -> (f64, f64) {
    let mut perimeter = 0.0f64;
    let mut min_edge_sq = f64::MAX;
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

#[inline(always)]
pub(super) fn quad_area_and_perimeter_f64(quad: &Quad) -> (f64, f64) {
    let mut area = 0.0f64;
    let mut perimeter = 0.0f64;
    for i in 0..4 {
        let j = (i + 1) % 4;
        area += quad[i].x * quad[j].y - quad[j].x * quad[i].y;
        let dx = quad[i].x - quad[j].x;
        let dy = quad[i].y - quad[j].y;
        perimeter += (dx * dx + dy * dy).sqrt();
    }
    (area.abs() * 0.5, perimeter)
}

pub(super) fn quad_from_cv(quad: &[CvPoint<f32>; 4]) -> Quad {
    [
        Point { x: quad[0].x as f64, y: quad[0].y as f64 },
        Point { x: quad[1].x as f64, y: quad[1].y as f64 },
        Point { x: quad[2].x as f64, y: quad[2].y as f64 },
        Point { x: quad[3].x as f64, y: quad[3].y as f64 },
    ]
}

pub(super) fn quad_to_cv(quad: &Quad) -> [CvPoint<f32>; 4] {
    [
        CvPoint::new(quad[0].x as f32, quad[0].y as f32),
        CvPoint::new(quad[1].x as f32, quad[1].y as f32),
        CvPoint::new(quad[2].x as f32, quad[2].y as f32),
        CvPoint::new(quad[3].x as f32, quad[3].y as f32),
    ]
}

pub(super) fn quad_average_corner_distance_sq(a: &[CvPoint<f32>; 4], b: &[CvPoint<f32>; 4]) -> f32 {
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
    min_dist_sq
}

pub(super) fn quad_too_near_border(quad: &[CvPoint<f32>; 4], max_x: f32, max_y: f32, min_border: f32) -> bool {
    for p in quad {
        if p.x < min_border || p.y < min_border || p.x > max_x - min_border || p.y > max_y - min_border {
            return true;
        }
    }
    false
}

#[inline(always)]
pub(super) fn fill_contour_points_f32(contour: &[Point], out: &mut Vec<CvPoint<f32>>) -> (f32, f32, f32, f32, f32) {
    debug_assert!(!contour.is_empty());
    let len = contour.len();
    out.clear();
    if out.capacity() < len {
        out.reserve(len - out.capacity());
    }
    out.resize(len, CvPoint::new(0.0, 0.0));

    let first = contour[0];
    let first_x = first.x as f32;
    let first_y = first.y as f32;
    out[0] = CvPoint::new(first_x, first_y);

    let mut prev_x = first_x;
    let mut prev_y = first_y;
    let mut min_x = first_x;
    let mut max_x = first_x;
    let mut min_y = first_y;
    let mut max_y = first_y;
    let mut perimeter = 0.0f32;

    for (i, p) in contour.iter().enumerate().skip(1) {
        let x = p.x as f32;
        let y = p.y as f32;
        let dx = x - prev_x;
        let dy = y - prev_y;
        perimeter += match (dx.abs(), dy.abs()) {
            (0.0, 0.0) => 0.0,
            (0.0, d) => d,
            (d, 0.0) => d,
            (1.0, 1.0) => std::f32::consts::SQRT_2,
            _ => (dx * dx + dy * dy).sqrt(),
        };
        prev_x = x;
        prev_y = y;
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
        out[i] = CvPoint::new(x, y);
    }

    let dx = first_x - prev_x;
    let dy = first_y - prev_y;
    perimeter += match (dx.abs(), dy.abs()) {
        (0.0, 0.0) => 0.0,
        (0.0, d) => d,
        (d, 0.0) => d,
        (1.0, 1.0) => std::f32::consts::SQRT_2,
        _ => (dx * dx + dy * dy).sqrt(),
    };

    (perimeter, min_x, max_x, min_y, max_y)
}

pub(super) fn refine_quad_corners_from_contour(contour: &[CvPoint<f32>], quad: &mut [CvPoint<f32>; 4], max_dist: f32, min_points: usize) -> bool {
    if contour.len() < min_points || max_dist <= 0.0 {
        return false;
    }

    fn restore_edge_points(edge_points: [Vec<CvPoint<f32>>; 4]) {
        ARUCO_NODE_SCRATCH.with(|scratch| {
            let mut scratch = scratch.borrow_mut();
            scratch.edge_points = edge_points;
        });
        report_node_detect_scratch();
    }

    let mut edge_points = ARUCO_NODE_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        std::mem::take(&mut scratch.edge_points)
    });
    for edge in &mut edge_points {
        edge.clear();
    }
    for i in 0..4 {
        let a = quad[i];
        let b = quad[(i + 1) % 4];
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let len2 = dx * dx + dy * dy;
        if len2 <= f32::EPSILON {
            restore_edge_points(edge_points);
            return false;
        }
        let inv_len = 1.0f32 / len2.sqrt();

        for p in contour {
            let px = p.x - a.x;
            let py = p.y - a.y;
            let t = (px * dx + py * dy) / len2;
            if !(-0.2..=1.2).contains(&t) {
                continue;
            }
            let dist = (px * dy - py * dx).abs() * inv_len;
            if dist <= max_dist {
                edge_points[i].push(*p);
            }
        }
    }

    let mut lines = [(0.0f32, 0.0f32, 0.0f32); 4];
    for i in 0..4 {
        if edge_points[i].len() < min_points {
            restore_edge_points(edge_points);
            return false;
        }
        let Some(line) = fit_edge_line_ransac(&edge_points[i], max_dist, 48) else {
            restore_edge_points(edge_points);
            return false;
        };
        lines[i] = line;
    }

    let mut refined = [quad[0], quad[1], quad[2], quad[3]];
    for i in 0..4 {
        let prev = (i + 3) % 4;
        let Some(p) = intersect_lines(lines[prev], lines[i]) else {
            restore_edge_points(edge_points);
            return false;
        };
        refined[i] = p;
    }
    sort_corners_clockwise(&mut refined);
    *quad = refined;
    restore_edge_points(edge_points);
    true
}
