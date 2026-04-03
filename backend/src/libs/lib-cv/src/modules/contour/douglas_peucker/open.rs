use super::*;

pub(super) fn approx_poly_dp_open_with_buffers_into(
    points: &[Point<f32>],
    epsilon: f32,
    stack: &mut Vec<(usize, usize)>,
    marker_epoch: &mut Vec<u32>,
    marker_gen: &mut u32,
    out: &mut Vec<Point<f32>>,
) {
    let len = points.len();
    if len < 3 || epsilon <= 0.0 {
        out.clear();
        out.extend_from_slice(points);
        return;
    }

    // We square epsilon for distance comparison (matching the C++ code style).
    let eps_sq = epsilon * epsilon;

    stack.clear();
    if marker_epoch.len() < len {
        marker_epoch.resize(len, 0);
    }
    *marker_gen = marker_gen.wrapping_add(1);
    if *marker_gen == 0 {
        marker_epoch.fill(0);
        *marker_gen = 1;
    }
    let epoch = *marker_gen;

    // We always keep the endpoints
    marker_epoch[0] = epoch;
    marker_epoch[len - 1] = epoch;

    // Push the entire range initially
    stack.push((0, len - 1));

    // Iterative “recursive” RDP
    while let Some((start, end)) = stack.pop() {
        let mut max_dist = 0.0;
        let mut index_of_max = 0usize;

        // The line from start -> end
        let (sx, sy) = (points[start].x, points[start].y);
        let (ex, ey) = (points[end].x, points[end].y);

        // Precompute for cross-product distance
        let dx = ex - sx;
        let dy = ey - sy;
        let sq_line_len = dx * dx + dy * dy;

        // Find point farthest from the line segment
        for (i, point) in points.iter().enumerate().take(end).skip(start + 1) {
            let (px, py) = (point.x, point.y);
            let dist = if sq_line_len > f32::EPSILON {
                let cross = (px - sx) * dy - (py - sy) * dx;
                (cross * cross) / sq_line_len // squared distance
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
            marker_epoch[index_of_max] = epoch;
            if index_of_max - start > 1 {
                stack.push((start, index_of_max));
            }
            if end - index_of_max > 1 {
                stack.push((index_of_max, end));
            }
        }
    }

    out.clear();
    out.reserve(len);
    for i in 0..len {
        if marker_epoch[i] == epoch {
            out.push(points[i]);
        }
    }
}

pub(super) fn approx_poly_dp_open_with_buffers_i32_into(
    points: &[Point<i32>],
    epsilon: f32,
    stack: &mut Vec<(usize, usize)>,
    marker_epoch: &mut Vec<u32>,
    marker_gen: &mut u32,
    out: &mut Vec<Point<i32>>,
) {
    let len = points.len();
    if len < 3 || epsilon <= 0.0 {
        out.clear();
        out.extend_from_slice(points);
        return;
    }

    let eps_sq = f64::from(epsilon) * f64::from(epsilon);

    stack.clear();
    if marker_epoch.len() < len {
        marker_epoch.resize(len, 0);
    }
    *marker_gen = marker_gen.wrapping_add(1);
    if *marker_gen == 0 {
        marker_epoch.fill(0);
        *marker_gen = 1;
    }
    let epoch = *marker_gen;

    marker_epoch[0] = epoch;
    marker_epoch[len - 1] = epoch;
    stack.push((0, len - 1));

    while let Some((start, end)) = stack.pop() {
        let mut max_dist = 0.0f64;
        let mut index_of_max = 0usize;

        let sx = points[start].x as f64;
        let sy = points[start].y as f64;
        let ex = points[end].x as f64;
        let ey = points[end].y as f64;
        let dx = ex - sx;
        let dy = ey - sy;
        let sq_line_len = dx * dx + dy * dy;

        for (i, point) in points.iter().enumerate().take(end).skip(start + 1) {
            let px = point.x as f64;
            let py = point.y as f64;
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
            marker_epoch[index_of_max] = epoch;
            if index_of_max - start > 1 {
                stack.push((start, index_of_max));
            }
            if end - index_of_max > 1 {
                stack.push((index_of_max, end));
            }
        }
    }

    out.clear();
    out.reserve(len);
    for i in 0..len {
        if marker_epoch[i] == epoch {
            out.push(points[i]);
        }
    }
}
