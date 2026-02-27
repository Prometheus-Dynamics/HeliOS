use imageproc::point::Point;
use std::cell::RefCell;
use std::cmp::Ordering;

#[derive(Default)]
struct RdpScratchF32 {
    stack: Vec<(usize, usize)>,
    marker_epoch: Vec<u32>,
    marker_gen: u32,
    seg1: Vec<Point<f32>>,
    seg2: Vec<Point<f32>>,
    out2: Vec<Point<f32>>,
    idxs: Vec<usize>,
    uniq: Vec<usize>,
    hull: Vec<usize>,
}

thread_local! {
    static RDP_SCRATCH_F32: RefCell<RdpScratchF32> = RefCell::new(RdpScratchF32::default());
}

/// Approximates a polygonal curve for lib-cv points using Ramer–Douglas–Peucker (f64).
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

/// Approximates a polygonal curve using the Ramer–Douglas–Peucker algorithm.
///
/// # Arguments
///
/// * `points`  - Input slice of 2D points (x, y).
/// * `closed`  - If `true`, the contour is considered closed; if `false`, it's open.
/// * `epsilon` - Maximum distance of a point to the approximated segment.
///
/// Returns a new `Vec<(f32, f32)>` containing the simplified polygon.
pub fn approx_poly_dp(points: &[Point<f32>], closed: bool, epsilon: f32) -> Vec<Point<f32>> {
    // If there are too few points or epsilon is non-positive, return the original set
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

    RDP_SCRATCH_F32.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        if closed {
            approx_poly_dp_closed_with_scratch(points, epsilon, out, &mut scratch);
        } else {
            let RdpScratchF32 { stack, marker_epoch, marker_gen, .. } = &mut *scratch;
            approx_poly_dp_open_with_buffers_into(points, epsilon, stack, marker_epoch, marker_gen, out);
        }
    });
}

fn approx_poly_dp_closed_points(points: &[crate::Point], epsilon: f64) -> Vec<crate::Point> {
    let n = points.len();
    if n < 3 || epsilon <= 0.0 {
        return points.to_vec();
    }

    let (mut a, mut b) = farthest_pair_points(points);
    if a > b {
        std::mem::swap(&mut a, &mut b);
    }

    let seg1: Vec<crate::Point> = points[a..=b].to_vec();
    let mut seg2: Vec<crate::Point> = Vec::with_capacity(n - (b - a));
    seg2.extend_from_slice(&points[b..]);
    seg2.extend_from_slice(&points[..=a]);

    let mut out1 = approx_poly_dp_points(&seg1, false, epsilon);
    let mut out2 = approx_poly_dp_points(&seg2, false, epsilon);
    merge_closed_segments(&mut out1, &mut out2);
    out1
}

fn approx_poly_dp_open_with_buffers_into(points: &[Point<f32>], epsilon: f32, stack: &mut Vec<(usize, usize)>, marker_epoch: &mut Vec<u32>, marker_gen: &mut u32, out: &mut Vec<Point<f32>>) {
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

fn approx_poly_dp_closed_with_scratch(points: &[Point<f32>], epsilon: f32, out: &mut Vec<Point<f32>>, scratch: &mut RdpScratchF32) {
    let n = points.len();
    if n < 3 || epsilon <= 0.0 {
        out.clear();
        out.extend_from_slice(points);
        return;
    }

    let (mut a, mut b) = farthest_pair_with_scratch(points, scratch);
    if a > b {
        std::mem::swap(&mut a, &mut b);
    }

    let RdpScratchF32 { stack, marker_epoch, marker_gen, seg1, seg2, out2, .. } = scratch;
    seg1.clear();
    seg1.extend_from_slice(&points[a..=b]);
    seg2.clear();
    seg2.extend_from_slice(&points[b..]);
    seg2.extend_from_slice(&points[..=a]);

    approx_poly_dp_open_with_buffers_into(seg1.as_slice(), epsilon, stack, marker_epoch, marker_gen, out);
    approx_poly_dp_open_with_buffers_into(seg2.as_slice(), epsilon, stack, marker_epoch, marker_gen, out2);
    merge_closed_segments(out, out2);
}

fn merge_closed_segments<T: PartialEq + Copy>(left: &mut Vec<T>, right: &mut Vec<T>) {
    if left.is_empty() {
        std::mem::swap(left, right);
        return;
    }
    if let Some(first_right) = right.first()
        && left.last() == Some(first_right)
    {
        right.remove(0);
    }
    if let Some(last_right) = right.last()
        && left.first() == Some(last_right)
    {
        right.pop();
    }
    left.append(right);
}

fn farthest_pair_points(points: &[crate::Point]) -> (usize, usize) {
    let n = points.len();
    if n == 0 {
        return (0, 0);
    }
    if n == 1 {
        return (0, 0);
    }
    let hull = convex_hull_indices_points(points);
    if hull.len() == 1 {
        return (0, 1);
    }
    if hull.len() == 2 {
        return (hull[0], hull[1]);
    }
    diameter_indices_points(points, &hull)
}

fn farthest_pair_with_scratch(points: &[Point<f32>], scratch: &mut RdpScratchF32) -> (usize, usize) {
    let n = points.len();
    if n == 0 {
        return (0, 0);
    }
    if n == 1 {
        return (0, 0);
    }
    let hull_len = convex_hull_indices_f32_into(points, &mut scratch.idxs, &mut scratch.uniq, &mut scratch.hull);
    if hull_len == 1 {
        return (0, 1);
    }
    if hull_len == 2 {
        return (scratch.hull[0], scratch.hull[1]);
    }
    diameter_indices_f32(points, &scratch.hull[..hull_len])
}

fn convex_hull_indices_points(points: &[crate::Point]) -> Vec<usize> {
    let n = points.len();
    if n <= 1 {
        return (0..n).collect();
    }
    let mut idxs: Vec<usize> = (0..n).collect();
    idxs.sort_by(|&a, &b| {
        let pa = &points[a];
        let pb = &points[b];
        match pa.x.partial_cmp(&pb.x).unwrap_or(Ordering::Equal) {
            Ordering::Equal => match pa.y.partial_cmp(&pb.y).unwrap_or(Ordering::Equal) {
                Ordering::Equal => a.cmp(&b),
                other => other,
            },
            other => other,
        }
    });
    let mut uniq = Vec::with_capacity(n);
    for &i in &idxs {
        if let Some(&last) = uniq.last()
            && points[i] == points[last]
        {
            continue;
        }
        uniq.push(i);
    }
    if uniq.len() <= 1 {
        return uniq;
    }

    let mut hull = Vec::with_capacity(uniq.len() * 2);
    for &i in &uniq {
        while hull.len() >= 2 {
            let len = hull.len();
            let o = hull[len - 2];
            let a = hull[len - 1];
            if cross_points(points, o, a, i) <= 0.0 {
                hull.pop();
            } else {
                break;
            }
        }
        hull.push(i);
    }
    let lower_len = hull.len();
    for &i in uniq.iter().rev().skip(1) {
        while hull.len() > lower_len {
            let len = hull.len();
            let o = hull[len - 2];
            let a = hull[len - 1];
            if cross_points(points, o, a, i) <= 0.0 {
                hull.pop();
            } else {
                break;
            }
        }
        hull.push(i);
    }
    hull.pop();
    if hull.is_empty() {
        hull.push(uniq[0]);
    }
    hull
}

fn convex_hull_indices_f32_into(points: &[Point<f32>], idxs: &mut Vec<usize>, uniq: &mut Vec<usize>, hull: &mut Vec<usize>) -> usize {
    let n = points.len();
    if n <= 1 {
        hull.clear();
        if n == 1 {
            hull.push(0);
        }
        return hull.len();
    }
    idxs.clear();
    idxs.resize(n, 0);
    for (i, slot) in idxs.iter_mut().enumerate() {
        *slot = i;
    }
    idxs.sort_by(|&a, &b| {
        let pa = &points[a];
        let pb = &points[b];
        match pa.x.partial_cmp(&pb.x).unwrap_or(Ordering::Equal) {
            Ordering::Equal => match pa.y.partial_cmp(&pb.y).unwrap_or(Ordering::Equal) {
                Ordering::Equal => a.cmp(&b),
                other => other,
            },
            other => other,
        }
    });
    uniq.clear();
    uniq.reserve(n);
    for &i in idxs.iter() {
        if let Some(&last) = uniq.last()
            && points[i] == points[last]
        {
            continue;
        }
        uniq.push(i);
    }
    if uniq.len() <= 1 {
        hull.clear();
        hull.extend_from_slice(uniq);
        return hull.len();
    }

    hull.clear();
    hull.reserve(uniq.len() * 2);
    for &i in uniq.iter() {
        while hull.len() >= 2 {
            let len = hull.len();
            let o = hull[len - 2];
            let a = hull[len - 1];
            if cross_f32(points, o, a, i) <= 0.0 {
                hull.pop();
            } else {
                break;
            }
        }
        hull.push(i);
    }
    let lower_len = hull.len();
    for &i in uniq.iter().rev().skip(1) {
        while hull.len() > lower_len {
            let len = hull.len();
            let o = hull[len - 2];
            let a = hull[len - 1];
            if cross_f32(points, o, a, i) <= 0.0 {
                hull.pop();
            } else {
                break;
            }
        }
        hull.push(i);
    }
    hull.pop();
    if hull.is_empty() {
        hull.push(uniq[0]);
    }
    hull.len()
}

fn diameter_indices_points(points: &[crate::Point], hull: &[usize]) -> (usize, usize) {
    let m = hull.len();
    if m == 0 {
        return (0, 0);
    }
    if m == 1 {
        return (hull[0], hull[0]);
    }
    if m == 2 {
        return (hull[0], hull[1]);
    }

    let mut j = 1usize;
    let mut best = normalize_pair(hull[0], hull[1]);
    let mut best_dist = dist2_points(points, best.0, best.1);

    for i in 0..m {
        let ni = (i + 1) % m;
        loop {
            let next = (j + 1) % m;
            let area_next = area2_points(points, hull[i], hull[ni], hull[next]);
            let area_cur = area2_points(points, hull[i], hull[ni], hull[j]);
            if area_next > area_cur {
                j = next;
            } else {
                break;
            }
        }
        update_best(points, hull[i], hull[j], &mut best, &mut best_dist);
        update_best(points, hull[ni], hull[j], &mut best, &mut best_dist);
    }

    best
}

fn diameter_indices_f32(points: &[Point<f32>], hull: &[usize]) -> (usize, usize) {
    let m = hull.len();
    if m == 0 {
        return (0, 0);
    }
    if m == 1 {
        return (hull[0], hull[0]);
    }
    if m == 2 {
        return (hull[0], hull[1]);
    }

    let mut j = 1usize;
    let mut best = normalize_pair(hull[0], hull[1]);
    let mut best_dist = dist2_f32(points, best.0, best.1);

    for i in 0..m {
        let ni = (i + 1) % m;
        loop {
            let next = (j + 1) % m;
            let area_next = area2_f32(points, hull[i], hull[ni], hull[next]);
            let area_cur = area2_f32(points, hull[i], hull[ni], hull[j]);
            if area_next > area_cur {
                j = next;
            } else {
                break;
            }
        }
        update_best_f32(points, hull[i], hull[j], &mut best, &mut best_dist);
        update_best_f32(points, hull[ni], hull[j], &mut best, &mut best_dist);
    }

    best
}

fn update_best(points: &[crate::Point], a: usize, b: usize, best: &mut (usize, usize), best_dist: &mut f64) {
    let (i, j) = normalize_pair(a, b);
    let dist = dist2_points(points, i, j);
    if dist > *best_dist || (dist == *best_dist && (i, j) < *best) {
        *best_dist = dist;
        *best = (i, j);
    }
}

fn update_best_f32(points: &[Point<f32>], a: usize, b: usize, best: &mut (usize, usize), best_dist: &mut f64) {
    let (i, j) = normalize_pair(a, b);
    let dist = dist2_f32(points, i, j);
    if dist > *best_dist || (dist == *best_dist && (i, j) < *best) {
        *best_dist = dist;
        *best = (i, j);
    }
}

fn normalize_pair(a: usize, b: usize) -> (usize, usize) {
    if a <= b { (a, b) } else { (b, a) }
}

fn dist2_points(points: &[crate::Point], a: usize, b: usize) -> f64 {
    let dx = points[a].x - points[b].x;
    let dy = points[a].y - points[b].y;
    dx * dx + dy * dy
}

fn dist2_f32(points: &[Point<f32>], a: usize, b: usize) -> f64 {
    let dx = points[a].x as f64 - points[b].x as f64;
    let dy = points[a].y as f64 - points[b].y as f64;
    dx * dx + dy * dy
}

fn area2_points(points: &[crate::Point], a: usize, b: usize, c: usize) -> f64 {
    let abx = points[b].x - points[a].x;
    let aby = points[b].y - points[a].y;
    let acx = points[c].x - points[a].x;
    let acy = points[c].y - points[a].y;
    (abx * acy - aby * acx).abs()
}

fn area2_f32(points: &[Point<f32>], a: usize, b: usize, c: usize) -> f64 {
    let abx = points[b].x as f64 - points[a].x as f64;
    let aby = points[b].y as f64 - points[a].y as f64;
    let acx = points[c].x as f64 - points[a].x as f64;
    let acy = points[c].y as f64 - points[a].y as f64;
    (abx * acy - aby * acx).abs()
}

fn cross_points(points: &[crate::Point], o: usize, a: usize, b: usize) -> f64 {
    let ox = points[o].x;
    let oy = points[o].y;
    (points[a].x - ox) * (points[b].y - oy) - (points[a].y - oy) * (points[b].x - ox)
}

fn cross_f32(points: &[Point<f32>], o: usize, a: usize, b: usize) -> f32 {
    let ox = points[o].x;
    let oy = points[o].y;
    (points[a].x - ox) * (points[b].y - oy) - (points[a].y - oy) * (points[b].x - ox)
}

// pub fn approx_poly_dp(curve: &[Point<u32>], epsilon: f64, closed: bool) -> Vec<Point<u32>> {
//     if curve.is_empty() {
//         return Vec::new();
//     }

//     if closed && curve.len() < 3 {
//         return curve.to_vec();
//     }

//     let curve_f64: Vec<(f64, f64)> = curve.iter().map(|p| (p.x as f64, p.y as f64)).collect();
//     let simplified_indices = douglas_peucker(&curve_f64, epsilon);
//     let mut approx = simplified_indices
//         .iter()
//         .map(|&i| Point::new(curve_f64[i].0.round() as u32, curve_f64[i].1.round() as u32))
//         .collect::<Vec<_>>();
//     approx
// }

// fn point_line_distance(
//     point: (f64, f64),
//     line_start: (f64, f64),
//     line_end: (f64, f64),
// ) -> f64 {
//     let (x0, y0) = point;
//     let (x1, y1) = line_start;
//     let (x2, y2) = line_end;

//     let numerator = ((y2 - y1) * x0 - (x2 - x1) * y0 + x2 * y1 - y2 * x1).abs();
//     let denominator = ((y2 - y1).powi(2) + (x2 - x1).powi(2)).sqrt();

//     if denominator == 0.0 {
//         ((x0 - x1).powi(2) + (y0 - y1).powi(2)).sqrt()
//     } else {
//         numerator / denominator
//     }
// }

// /// Implements the cyclic Douglas-Peucker algorithm.
// pub fn cyclic_douglas_peucker(curve: &[(f64, f64)], epsilon: f64) -> Vec<usize> {
//     if curve.len() < 3 {
//         // Not enough points to simplify
//         return (0..curve.len()).collect();
//     }

//     // Initialize a vector to keep track of points to keep
//     let mut keep = vec![false; curve.len()];
//     let mut stack = vec![(0, curve.len() - 1)];

//     // Always keep the first and last points
//     keep[0] = true;
//     keep[curve.len() - 1] = true;

//     while let Some((start, end)) = stack.pop() {
//         if end <= start + 1 {
//             continue;
//         }

//         let line_start = curve[start];
//         let line_end = curve[end];

//         let mut max_dist = 0.0;
//         let mut index = start;

//         for i in (start + 1)..end {
//             let dist = point_line_distance(curve[i], line_start, line_end);
//             if dist > max_dist {
//                 max_dist = dist;
//                 index = i;
//             }
//         }

//         if max_dist > epsilon {
//             keep[index] = true;
//             stack.push((start, index));
//             stack.push((index, end));
//         }
//     }

//     // To handle the cyclic nature, check the distance between the last and first kept points
//     let mut cyclic_keep = keep.clone();
//     let mut last_kept = 0;
//     for i in 1..keep.len() {
//         if keep[i] {
//             last_kept = i;
//             break;
//         }
//     }

//     let mut first_kept = 0;
//     for i in (0..keep.len()).rev() {
//         if keep[i] {
//             first_kept = i;
//             break;
//         }
//     }

//     let distance = point_line_distance(curve[first_kept], curve[last_kept], curve[first_kept]);
//     if distance > epsilon {
//         cyclic_keep[first_kept] = true;
//         cyclic_keep[last_kept] = true;
//     }

//     // Collect the indices of kept points
//     cyclic_keep
//         .iter()
//         .enumerate()
//         .filter_map(|(i, &k)| if k { Some(i) } else { None })
//         .collect()
// }

// /// Rotates the curve so that the point with the maximum distance is first.
// fn rotate_curve_to_max_distance(curve: &[(f64, f64)], epsilon: f64) -> Vec<(f64, f64)> {
//     // Find the point with the maximum distance from the baseline (first to last)
//     let (start, end) = (curve[0], curve[curve.len() - 1]);
//     let mut max_dist = 0.0;
//     let mut index = 0;

//     for (i, &point) in curve.iter().enumerate().skip(1).take(curve.len() - 2) {
//         let dist = point_line_distance(point, start, end);
//         if dist > max_dist {
//             max_dist = dist;
//             index = i;
//         }
//     }

//     // If no point exceeds epsilon, return the original curve
//     if max_dist <= epsilon {
//         return curve.to_vec();
//     }

//     // Rotate the curve so that the point with maximum distance is first
//     let mut rotated = curve[index..].to_vec();
//     rotated.extend_from_slice(&curve[..index]);
//     rotated
// }

// /// Rotates the approximated curve back to the original orientation.
// fn rotate_back(original: &[(f64, f64)], simplified: &[Point<u32>]) -> Vec<Point<u32>> {
//     if simplified.is_empty() {
//         return Vec::new();
//     }

//     // Find the starting point in the original curve
//     let start_point = (simplified[0].x as f64, simplified[0].y as f64);
//     let start_index = original.iter().position(|&p| p == start_point).unwrap_or(0);

//     // Rotate back
//     let mut rotated_back = Vec::new();
//     for point in simplified.iter().take(simplified.len() - 1) { // exclude the duplicate last point
//         rotated_back.push(point.clone());
//     }

//     rotated_back.rotate_right(start_index);
//     rotated_back.push(rotated_back[0].clone()); // close the curve

//     rotated_back
// }

// /// Standard Douglas-Peucker algorithm for open curves.
// pub fn douglas_peucker(points: &[(f64, f64)], epsilon: f64) -> Vec<usize> {
//     if points.is_empty() {
//         return Vec::new();
//     }

//     let start = 0;
//     let end = points.len() - 1;

//     let mut stack: Vec<(usize, usize)> = Vec::new();
//     let mut keep: Vec<bool> = vec![false; points.len()];
//     keep[start] = true;
//     keep[end] = true;

//     stack.push((start, end));

//     while let Some((s, e)) = stack.pop() {
//         if e <= s + 1 {
//             continue;
//         }

//         let line_start = points[s];
//         let line_end = points[e];

//         let mut max_dist = 0.0;
//         let mut index = s;

//         for i in (s + 1)..e {
//             let dist = point_line_distance(points[i], line_start, line_end);
//             if dist > max_dist {
//                 max_dist = dist;
//                 index = i;
//             }
//         }

//         if max_dist > epsilon {
//             keep[index] = true;
//             stack.push((s, index));
//             stack.push((index, e));
//         }
//     }

//     points
//         .iter()
//         .enumerate()
//         .filter_map(|(i, _)| if keep[i] { Some(i) } else { None })
//         .collect()
// }
