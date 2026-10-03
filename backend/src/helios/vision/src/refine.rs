//! Sub-pixel corner refinement for dark quads on a light background.

use crate::geometry::{Point, distance};
use crate::image::GrayImage;

/// Edge search radius as a fraction of the side length, for the first pass.
/// Polygon approximation can cut a corner by several percent of the side;
/// this stays well under one cell of a 4x4 or 36h11 marker, so inner bit
/// edges are out of reach.
const SEARCH_RATE: f32 = 0.06;
/// Search radius bounds in pixels; the lower bound covers the error of quads
/// found on a 2x/4x downscaled image.
const MIN_RADIUS: f32 = 3.0;
const MAX_RADIUS: f32 = 12.0;
/// Search radius of the second, polishing pass.
const POLISH_RADIUS: f32 = 1.5;

/// Move the corners of a dark quad onto its sub-pixel outer edges: along
/// each side, find the strongest dark-to-light step going outward, fit a
/// line through those points and intersect neighbouring lines. Returns
/// `None` when an edge is too weak or the result moves too far, in which
/// case the coarse corners should be kept. A wide first pass recovers cut
/// corners, a narrow second pass polishes the fit.
pub fn refine_corners(gray: &GrayImage, corners: [Point; 4]) -> Option<[Point; 4]> {
    let first = refine_pass(gray, corners, None)?;
    Some(refine_pass(gray, first, Some(POLISH_RADIUS)).unwrap_or(first))
}

fn refine_pass(gray: &GrayImage, corners: [Point; 4], radius: Option<f32>) -> Option<[Point; 4]> {
    let mut max_radius = 0.0f32;
    let centre = [corners.iter().map(|c| c[0]).sum::<f32>() / 4.0, corners.iter().map(|c| c[1]).sum::<f32>() / 4.0];
    let mut lines = [([0.0f32; 2], [0.0f32; 2]); 4];
    for (i, line) in lines.iter_mut().enumerate() {
        let (a, b) = (corners[i], corners[(i + 1) % 4]);
        let length = distance(a, b);
        if length < 8.0 {
            return None;
        }
        let dir = [(b[0] - a[0]) / length, (b[1] - a[1]) / length];
        let mut normal = [-dir[1], dir[0]];
        let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
        if (mid[0] - centre[0]) * normal[0] + (mid[1] - centre[1]) * normal[1] < 0.0 {
            normal = [-normal[0], -normal[1]];
        }
        let radius = radius.unwrap_or((SEARCH_RATE * length).clamp(MIN_RADIUS, MAX_RADIUS));
        max_radius = max_radius.max(radius);
        let samples = ((length / 2.0) as usize).clamp(4, 32);
        let mut points = Vec::with_capacity(samples);
        for k in 0..samples {
            let t = 0.15 + 0.7 * k as f32 / (samples - 1) as f32;
            let p = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
            if let Some(offset) = edge_offset(gray, p, normal, radius) {
                points.push([p[0] + normal[0] * offset, p[1] + normal[1] * offset]);
            }
        }
        if points.len() < samples / 2 + 1 {
            return None;
        }
        *line = fit_line(&points);
    }
    let mut refined = [[0.0f32; 2]; 4];
    for i in 0..4 {
        refined[i] = intersect(lines[(i + 3) % 4], lines[i])?;
        if distance(refined[i], corners[i]) > 2.0 * max_radius {
            return None;
        }
    }
    Some(refined)
}

/// Signed distance along `normal` from `p` to the strongest dark-to-light
/// step within `radius`, with parabolic sub-sample interpolation.
fn edge_offset(gray: &GrayImage, p: Point, normal: Point, radius: f32) -> Option<f32> {
    const STEP: f32 = 0.5;
    const MIN_STEP: f32 = 4.0;
    let n = (2.0 * radius / STEP) as usize + 1;
    let values: Vec<f32> = (0..n)
        .map(|j| {
            let s = -radius + j as f32 * STEP;
            gray.sample(p[0] + normal[0] * s, p[1] + normal[1] * s)
        })
        .collect();
    let gradient: Vec<f32> = (1..n - 1).map(|j| values[j + 1] - values[j - 1]).collect();
    let (best, &peak) = gradient.iter().enumerate().max_by(|x, y| x.1.total_cmp(y.1))?;
    if peak < MIN_STEP {
        return None;
    }
    let mut shift = 0.0;
    if best > 0 && best + 1 < gradient.len() {
        let (l, r) = (gradient[best - 1], gradient[best + 1]);
        let denom = l - 2.0 * peak + r;
        if denom.abs() > f32::EPSILON {
            shift = (0.5 * (l - r) / denom).clamp(-0.5, 0.5);
        }
    }
    Some(-radius + (best + 1) as f32 * STEP + shift * STEP)
}

/// Total least squares line through `points`, as (point, unit direction).
fn fit_line(points: &[Point]) -> (Point, Point) {
    let count = points.len() as f32;
    let mean = [points.iter().map(|p| p[0]).sum::<f32>() / count, points.iter().map(|p| p[1]).sum::<f32>() / count];
    let (mut sxx, mut sxy, mut syy) = (0.0f32, 0.0f32, 0.0f32);
    for p in points {
        let (dx, dy) = (p[0] - mean[0], p[1] - mean[1]);
        sxx += dx * dx;
        sxy += dx * dy;
        syy += dy * dy;
    }
    let angle = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    (mean, [angle.cos(), angle.sin()])
}

fn intersect((p, d): (Point, Point), (q, e): (Point, Point)) -> Option<Point> {
    let cross = d[0] * e[1] - d[1] * e[0];
    if cross.abs() < 1e-3 {
        return None;
    }
    let t = ((q[0] - p[0]) * e[1] - (q[1] - p[1]) * e[0]) / cross;
    Some([p[0] + d[0] * t, p[1] + d[1] * t])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refines_offset_corners_onto_square_edges() {
        // Dark square covering pixels 20..60: its edges are at 19.5 and 59.5.
        let mut gray = GrayImage::filled(80, 80, 220);
        for y in 20..60 {
            for x in 20..60 {
                gray.set(x, y, 30);
            }
        }
        let coarse = [[21.2, 18.9], [58.4, 20.8], [60.7, 58.6], [18.8, 60.3]];
        let refined = refine_corners(&gray, coarse).unwrap();
        for (corner, expected) in refined.iter().zip([[19.5, 19.5], [59.5, 19.5], [59.5, 59.5], [19.5, 59.5]]) {
            assert!(distance(*corner, expected) < 0.25, "{corner:?} vs {expected:?}");
        }
    }

    #[test]
    fn recovers_a_cut_corner() {
        let mut gray = GrayImage::filled(160, 160, 220);
        for y in 20..140 {
            for x in 20..140 {
                gray.set(x, y, 30);
            }
        }
        // Corner 1 cut 10 px short along the top edge.
        let coarse = [[19.5, 19.5], [129.5, 19.5], [139.5, 139.5], [19.5, 139.5]];
        let refined = refine_corners(&gray, coarse).unwrap();
        assert!(distance(refined[1], [139.5, 19.5]) < 0.3, "{:?}", refined[1]);
    }

    #[test]
    fn flat_image_does_not_refine() {
        let gray = GrayImage::filled(80, 80, 128);
        assert!(refine_corners(&gray, [[20.0, 20.0], [60.0, 20.0], [60.0, 60.0], [20.0, 60.0]]).is_none());
    }
}
