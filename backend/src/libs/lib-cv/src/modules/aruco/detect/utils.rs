use super::*;

pub fn sort_corners_clockwise(points: &mut [Point<f32>; 4]) {
    let dx1 = points[1].x - points[0].x;
    let dy1 = points[1].y - points[0].y;
    let dx2 = points[2].x - points[0].x;
    let dy2 = points[2].y - points[0].y;
    let cross = (dx1 * dy2) - (dy1 * dx2);
    if cross < 0.0 {
        points.swap(1, 3);
    }
}

pub(super) fn rotate_corners_to_top_left(points: &mut [Point<f32>; 4]) {
    // After `sort_corners_clockwise()` we have a consistent winding, but the starting index is
    // whatever the contour approximation produced. Many downstream consumers (pose, calibration,
    // decoding) assume TL/TR/BR/BL ordering, so rotate the quad to start at the top-left corner.
    //
    // Use (x+y) as the primary key (common ArUco convention); break ties by y then x.
    let mut best = 0usize;
    let mut best_sum = f32::INFINITY;
    let mut best_y = f32::INFINITY;
    let mut best_x = f32::INFINITY;
    for (idx, p) in points.iter().enumerate() {
        let sum = p.x + p.y;
        if sum < best_sum || (sum == best_sum && (p.y < best_y || (p.y == best_y && p.x < best_x))) {
            best = idx;
            best_sum = sum;
            best_y = p.y;
            best_x = p.x;
        }
    }

    if best == 0 {
        return;
    }

    let rotated = [points[best], points[(best + 1) & 3], points[(best + 2) & 3], points[(best + 3) & 3]];
    *points = rotated;
}

pub(crate) fn is_convex_quad(points: &[Point<f32>; 4]) -> bool {
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
pub(super) fn angles_within_cos_range(points: &[Point<f32>; 4], cos_min: f32, cos_max: f32) -> bool {
    debug_assert!(cos_min <= cos_max);
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
        let denom = (ab2 * cb2).sqrt();
        if denom <= f32::EPSILON {
            return false;
        }

        let cos_theta = (dot / denom).clamp(-1.0, 1.0);
        if cos_theta < cos_min || cos_theta > cos_max {
            return false;
        }
    }
    true
}

pub(super) fn adaptive_sample_scale(corners: &[Point<f32>; 4], family: &ArucoTagFamily, requested_scale: u32) -> u32 {
    let requested_scale = requested_scale.max(1);
    let edge_lengths = [distance(&corners[0], &corners[1]), distance(&corners[1], &corners[2]), distance(&corners[2], &corners[3]), distance(&corners[3], &corners[0])];
    let avg_edge = edge_lengths.iter().copied().sum::<f32>() / edge_lengths.len() as f32;
    let cells = family.total_width().max(1) as f32;
    adaptive_sample_scale_from_avg_edge(avg_edge, cells, requested_scale)
}

#[inline(always)]
pub(super) fn adaptive_sample_scale_from_avg_edge(avg_edge: f32, cells: f32, requested_scale: u32) -> u32 {
    let requested_scale = requested_scale.max(1);
    let per_cell_samples = (avg_edge / cells.max(1.0)).max(1.0);
    let adaptive = per_cell_samples.round() as u32;
    // Oversampling improves decode robustness but can explode CPU cost at full-res when tags are large.
    // Allow the scale to shrink for very small/far tags (to avoid magnifying noise), but keep a
    // minimum to avoid severe undersampling of the tag grid.
    let min_scale = requested_scale.clamp(1, 4);
    let max_scale = requested_scale.saturating_mul(2).max(requested_scale);
    adaptive.clamp(min_scale, max_scale).clamp(1, 32)
}

pub(super) fn distance(a: &Point<f32>, b: &Point<f32>) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

pub(super) fn refined_quad_is_reasonable(original: &[Point<f32>; 4], refined: &[Point<f32>; 4]) -> bool {
    if original.iter().chain(refined.iter()).any(|p| !p.x.is_finite() || !p.y.is_finite()) {
        return false;
    }
    if !is_convex_quad(refined) {
        return false;
    }

    let quad_area = |pts: &[Point<f32>; 4]| -> f32 {
        let mut area = 0.0f32;
        for i in 0..4 {
            let j = (i + 1) & 3;
            area += pts[i].x * pts[j].y - pts[j].x * pts[i].y;
        }
        area.abs() * 0.5
    };

    let orig_area = quad_area(original);
    let refined_area = quad_area(refined);
    if !orig_area.is_finite() || !refined_area.is_finite() || orig_area <= f32::EPSILON || refined_area <= f32::EPSILON {
        return false;
    }
    let area_ratio = refined_area / orig_area;
    if !(0.45..=1.8).contains(&area_ratio) {
        return false;
    }

    let centroid = |pts: &[Point<f32>; 4]| -> Point<f32> { Point::new((pts[0].x + pts[1].x + pts[2].x + pts[3].x) * 0.25, (pts[0].y + pts[1].y + pts[2].y + pts[3].y) * 0.25) };

    let mut orig_avg_edge = 0.0f32;
    for i in 0..4 {
        orig_avg_edge += distance(&original[i], &original[(i + 1) & 3]);
    }
    orig_avg_edge *= 0.25;
    if !orig_avg_edge.is_finite() || orig_avg_edge <= f32::EPSILON {
        return false;
    }

    let max_corner_shift = orig_avg_edge * 0.45 + 2.5;
    for i in 0..4 {
        if distance(&original[i], &refined[i]) > max_corner_shift {
            return false;
        }
    }

    let c0 = centroid(original);
    let c1 = centroid(refined);
    let max_center_shift = orig_avg_edge * 0.35 + 2.0;
    if distance(&c0, &c1) > max_center_shift {
        return false;
    }

    true
}

pub(super) fn normalize_warped_patch(patch: &mut GrayImage, min_range: u8) -> bool {
    if patch.width() == 0 || patch.height() == 0 {
        return false;
    }
    let buf = patch.as_mut();
    let mut min_val = u8::MAX;
    let mut max_val = u8::MIN;
    for &v in buf.iter() {
        min_val = min_val.min(v);
        max_val = max_val.max(v);
    }
    if max_val <= min_val {
        return false;
    }
    // Reject low-contrast patches early (cuts false positives + saves decode work).
    let range = max_val - min_val;
    if range < min_range {
        return false;
    }
    let range_u16 = range as u16;
    let mut lut = [0u8; 256];
    for (i, out) in lut.iter_mut().enumerate() {
        let shifted = (i as u8).saturating_sub(min_val) as u16;
        *out = ((shifted * 255) / range_u16) as u8;
    }
    for v in buf.iter_mut() {
        *v = lut[*v as usize];
    }
    true
}
