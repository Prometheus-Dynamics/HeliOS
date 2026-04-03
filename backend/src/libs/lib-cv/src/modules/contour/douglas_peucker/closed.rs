use super::*;

pub(super) fn approx_poly_dp_closed_points(points: &[crate::Point], epsilon: f64) -> Vec<crate::Point> {
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

pub(super) fn approx_poly_dp_closed_with_scratch(points: &[Point<f32>], epsilon: f32, out: &mut Vec<Point<f32>>, scratch: &mut RdpScratchF32) {
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

pub(super) fn approx_poly_dp_closed_fast_with_scratch(points: &[Point<f32>], epsilon: f32, out: &mut Vec<Point<f32>>, scratch: &mut RdpScratchF32) {
    let n = points.len();
    if n < 3 || epsilon <= 0.0 {
        out.clear();
        out.extend_from_slice(points);
        return;
    }

    let (mut a, mut b) = approximate_farthest_pair_axis_extrema(points);
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

pub(super) fn approx_poly_dp_closed_fast_i32_with_scratch(points: &[Point<i32>], epsilon: f32, out: &mut Vec<Point<i32>>, scratch: &mut RdpScratchI32) {
    let n = points.len();
    if n < 3 || epsilon <= 0.0 {
        out.clear();
        out.extend_from_slice(points);
        return;
    }

    let (mut a, mut b) = approximate_farthest_pair_axis_extrema_i32(points);
    if a > b {
        std::mem::swap(&mut a, &mut b);
    }

    let RdpScratchI32 { stack, marker_epoch, marker_gen, seg1, seg2, out2 } = scratch;
    seg1.clear();
    seg1.extend_from_slice(&points[a..=b]);
    seg2.clear();
    seg2.extend_from_slice(&points[b..]);
    seg2.extend_from_slice(&points[..=a]);

    approx_poly_dp_open_with_buffers_i32_into(seg1.as_slice(), epsilon, stack, marker_epoch, marker_gen, out);
    approx_poly_dp_open_with_buffers_i32_into(seg2.as_slice(), epsilon, stack, marker_epoch, marker_gen, out2);
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
