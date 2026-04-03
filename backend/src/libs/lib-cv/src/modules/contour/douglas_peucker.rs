use imageproc::point::Point;
use std::cmp::Ordering;

mod closed;
mod geometry;
mod open;
mod scratch;

use self::{closed::*, geometry::*, open::*, scratch::*};

pub(crate) use self::scratch::compact_rdp_scratch_after_frame;

/// Approximates a polygonal curve for lib-cv points using Ramer-Douglas-Peucker (f64).
pub fn approx_poly_dp_points(points: &[crate::Point], closed: bool, epsilon: f64) -> Vec<crate::Point> {
    if points.len() < 3 || epsilon <= 0.0 {
        return points.to_vec();
    }

    if closed {
        return approx_poly_dp_closed_points(points, epsilon);
    }

    let eps_sq = epsilon * epsilon;
    let curve: Vec<crate::Point> = points.to_vec();
    let mut stack = Vec::with_capacity(curve.len());
    let mut marker = vec![0u8; curve.len()];
    marker[0] = 1;
    marker[curve.len() - 1] = 1;
    stack.push((0usize, curve.len() - 1));

    while let Some((start, end)) = stack.pop() {
        let mut max_dist = 0.0f64;
        let mut index_of_max = 0usize;

        let (sx, sy) = (curve[start].x, curve[start].y);
        let (ex, ey) = (curve[end].x, curve[end].y);
        let dx = ex - sx;
        let dy = ey - sy;
        let sq_line_len = dx * dx + dy * dy;

        for (i, point) in curve.iter().enumerate().take(end).skip(start + 1) {
            let (px, py) = (point.x, point.y);
            let dist = if sq_line_len > f64::EPSILON {
                let cross = (px - sx) * dy - (py - sy) * dx;
                (cross * cross) / sq_line_len
            } else {
                let dxp = px - sx;
                let dyp = py - sy;
                dxp * dxp + dyp * dyp
            };

            if dist > max_dist {
                index_of_max = i;
                max_dist = dist;
            }
        }

        if max_dist > eps_sq {
            marker[index_of_max] = 1;
            if index_of_max - start > 1 {
                stack.push((start, index_of_max));
            }
            if end - index_of_max > 1 {
                stack.push((index_of_max, end));
            }
        }
    }

    let mut out = Vec::with_capacity(curve.len());
    for (i, &keep) in marker.iter().enumerate() {
        if keep != 0 {
            out.push(curve[i]);
        }
    }

    out
}

/// Approximates a polygonal curve using the Ramer-Douglas-Peucker algorithm.
pub fn approx_poly_dp(points: &[Point<f32>], closed: bool, epsilon: f32) -> Vec<Point<f32>> {
    if points.len() < 3 || epsilon <= 0.0 {
        return points.to_vec();
    }

    let mut out = Vec::with_capacity(points.len());
    approx_poly_dp_into(points, closed, epsilon, &mut out);
    out
}

pub fn approx_poly_dp_into(points: &[Point<f32>], closed: bool, epsilon: f32, out: &mut Vec<Point<f32>>) {
    if points.len() < 3 || epsilon <= 0.0 {
        out.clear();
        out.extend_from_slice(points);
        return;
    }

    with_rdp_scratch_f32(|scratch| {
        if closed {
            approx_poly_dp_closed_with_scratch(points, epsilon, out, scratch);
        } else {
            let RdpScratchF32 { stack, marker_epoch, marker_gen, .. } = &mut *scratch;
            approx_poly_dp_open_with_buffers_into(points, epsilon, stack, marker_epoch, marker_gen, out);
        }
    });
}

/// Faster closed-contour approximation used by hot candidate-extraction paths.
pub fn approx_poly_dp_closed_fast_into(points: &[Point<f32>], epsilon: f32, out: &mut Vec<Point<f32>>) {
    if points.len() < 3 || epsilon <= 0.0 {
        out.clear();
        out.extend_from_slice(points);
        return;
    }

    with_rdp_scratch_f32(|scratch| {
        approx_poly_dp_closed_fast_with_scratch(points, epsilon, out, scratch);
    });
}

/// Integer closed-contour fast approximation used by hot chain-code contour paths.
pub fn approx_poly_dp_closed_fast_i32_into(points: &[Point<i32>], epsilon: f32, out: &mut Vec<Point<i32>>) {
    if points.len() < 3 || epsilon <= 0.0 {
        out.clear();
        out.extend_from_slice(points);
        return;
    }

    with_rdp_scratch_i32(|scratch| {
        approx_poly_dp_closed_fast_i32_with_scratch(points, epsilon, out, scratch);
    });
}

#[cfg(test)]
mod tests {
    use super::{approx_poly_dp_closed_fast_i32_into, approx_poly_dp_closed_fast_into, approx_poly_dp_into};
    use imageproc::point::Point;

    fn dense_rectangle(width: i32, height: i32) -> Vec<Point<f32>> {
        let mut points = Vec::new();
        for x in 0..width {
            points.push(Point::new(x as f32, 0.0));
        }
        for y in 1..height {
            points.push(Point::new((width - 1) as f32, y as f32));
        }
        for x in (0..(width - 1)).rev() {
            points.push(Point::new(x as f32, (height - 1) as f32));
        }
        for y in (1..(height - 1)).rev() {
            points.push(Point::new(0.0, y as f32));
        }
        points
    }

    #[test]
    fn closed_fast_matches_closed_rdp_on_dense_rectangle() {
        let contour = dense_rectangle(128, 72);
        let mut exact = Vec::new();
        let mut fast = Vec::new();

        approx_poly_dp_into(&contour, true, 2.0, &mut exact);
        approx_poly_dp_closed_fast_into(&contour, 2.0, &mut fast);

        if exact.first() == exact.last() {
            exact.pop();
        }
        if fast.first() == fast.last() {
            fast.pop();
        }

        assert_eq!(exact.len(), 4);
        assert_eq!(fast.len(), 4);
        assert_eq!(exact, fast);
    }

    fn dense_rectangle_i32(width: i32, height: i32) -> Vec<Point<i32>> {
        let mut points = Vec::new();
        for x in 0..width {
            points.push(Point::new(x, 0));
        }
        for y in 1..height {
            points.push(Point::new(width - 1, y));
        }
        for x in (0..(width - 1)).rev() {
            points.push(Point::new(x, height - 1));
        }
        for y in (1..(height - 1)).rev() {
            points.push(Point::new(0, y));
        }
        points
    }

    #[test]
    fn closed_fast_i32_preserves_dense_rectangle_corners() {
        let contour = dense_rectangle_i32(128, 72);
        let mut fast = Vec::new();
        approx_poly_dp_closed_fast_i32_into(&contour, 2.0, &mut fast);
        if fast.first() == fast.last() {
            fast.pop();
        }
        assert_eq!(fast.len(), 4);
        assert_eq!(fast[0], Point::new(0, 0));
        assert_eq!(fast[1], Point::new(127, 0));
        assert_eq!(fast[2], Point::new(127, 71));
        assert_eq!(fast[3], Point::new(0, 71));
    }
}
