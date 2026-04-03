use super::*;

pub(super) fn approximate_farthest_pair_axis_extrema(points: &[Point<f32>]) -> (usize, usize) {
    let n = points.len();
    if n == 0 {
        return (0, 0);
    }
    if n == 1 {
        return (0, 0);
    }
    if n == 2 {
        return (0, 1);
    }

    let mut min_x = 0usize;
    let mut max_x = 0usize;
    let mut min_y = 0usize;
    let mut max_y = 0usize;
    let mut min_sum = 0usize;
    let mut max_sum = 0usize;
    let mut min_diff = 0usize;
    let mut max_diff = 0usize;

    for i in 1..n {
        let p = points[i];
        let px = p.x;
        let py = p.y;
        let sum = px + py;
        let diff = px - py;

        let min_x_p = points[min_x];
        if px < min_x_p.x || (px == min_x_p.x && py < min_x_p.y) {
            min_x = i;
        }

        let max_x_p = points[max_x];
        if px > max_x_p.x || (px == max_x_p.x && py > max_x_p.y) {
            max_x = i;
        }

        let min_y_p = points[min_y];
        if py < min_y_p.y || (py == min_y_p.y && px < min_y_p.x) {
            min_y = i;
        }

        let max_y_p = points[max_y];
        if py > max_y_p.y || (py == max_y_p.y && px > max_y_p.x) {
            max_y = i;
        }

        let min_sum_p = points[min_sum];
        let min_sum_v = min_sum_p.x + min_sum_p.y;
        if sum < min_sum_v || (sum == min_sum_v && px < min_sum_p.x) {
            min_sum = i;
        }

        let max_sum_p = points[max_sum];
        let max_sum_v = max_sum_p.x + max_sum_p.y;
        if sum > max_sum_v || (sum == max_sum_v && px > max_sum_p.x) {
            max_sum = i;
        }

        let min_diff_p = points[min_diff];
        let min_diff_v = min_diff_p.x - min_diff_p.y;
        if diff < min_diff_v || (diff == min_diff_v && py < min_diff_p.y) {
            min_diff = i;
        }

        let max_diff_p = points[max_diff];
        let max_diff_v = max_diff_p.x - max_diff_p.y;
        if diff > max_diff_v || (diff == max_diff_v && py > max_diff_p.y) {
            max_diff = i;
        }
    }

    let extrema = [min_x, max_x, min_y, max_y, min_sum, max_sum, min_diff, max_diff];
    let mut unique = [usize::MAX; 8];
    let mut unique_len = 0usize;
    for idx in extrema {
        if !unique[..unique_len].contains(&idx) {
            unique[unique_len] = idx;
            unique_len += 1;
        }
    }

    if unique_len < 2 {
        return (0, 1);
    }

    let mut best = normalize_pair(unique[0], unique[1]);
    let mut best_dist = dist2_f32(points, best.0, best.1);
    for i in 0..unique_len {
        for j in (i + 1)..unique_len {
            update_best_f32(points, unique[i], unique[j], &mut best, &mut best_dist);
        }
    }
    best
}

pub(super) fn approximate_farthest_pair_axis_extrema_i32(points: &[Point<i32>]) -> (usize, usize) {
    let n = points.len();
    if n == 0 {
        return (0, 0);
    }
    if n == 1 {
        return (0, 0);
    }
    if n == 2 {
        return (0, 1);
    }

    let mut min_x = 0usize;
    let mut max_x = 0usize;
    let mut min_y = 0usize;
    let mut max_y = 0usize;
    let mut min_sum = 0usize;
    let mut max_sum = 0usize;
    let mut min_diff = 0usize;
    let mut max_diff = 0usize;

    for i in 1..n {
        let p = points[i];
        let px = p.x;
        let py = p.y;
        let sum = px + py;
        let diff = px - py;

        let min_x_p = points[min_x];
        if px < min_x_p.x || (px == min_x_p.x && py < min_x_p.y) {
            min_x = i;
        }

        let max_x_p = points[max_x];
        if px > max_x_p.x || (px == max_x_p.x && py > max_x_p.y) {
            max_x = i;
        }

        let min_y_p = points[min_y];
        if py < min_y_p.y || (py == min_y_p.y && px < min_y_p.x) {
            min_y = i;
        }

        let max_y_p = points[max_y];
        if py > max_y_p.y || (py == max_y_p.y && px > max_y_p.x) {
            max_y = i;
        }

        let min_sum_p = points[min_sum];
        let min_sum_v = min_sum_p.x + min_sum_p.y;
        if sum < min_sum_v || (sum == min_sum_v && px < min_sum_p.x) {
            min_sum = i;
        }

        let max_sum_p = points[max_sum];
        let max_sum_v = max_sum_p.x + max_sum_p.y;
        if sum > max_sum_v || (sum == max_sum_v && px > max_sum_p.x) {
            max_sum = i;
        }

        let min_diff_p = points[min_diff];
        let min_diff_v = min_diff_p.x - min_diff_p.y;
        if diff < min_diff_v || (diff == min_diff_v && py < min_diff_p.y) {
            min_diff = i;
        }

        let max_diff_p = points[max_diff];
        let max_diff_v = max_diff_p.x - max_diff_p.y;
        if diff > max_diff_v || (diff == max_diff_v && py > max_diff_p.y) {
            max_diff = i;
        }
    }

    let extrema = [min_x, max_x, min_y, max_y, min_sum, max_sum, min_diff, max_diff];
    let mut unique = [usize::MAX; 8];
    let mut unique_len = 0usize;
    for idx in extrema {
        if !unique[..unique_len].contains(&idx) {
            unique[unique_len] = idx;
            unique_len += 1;
        }
    }

    if unique_len < 2 {
        return (0, 1);
    }

    let mut best = normalize_pair(unique[0], unique[1]);
    let mut best_dist = dist2_i32(points, best.0, best.1);
    for i in 0..unique_len {
        for j in (i + 1)..unique_len {
            update_best_i32(points, unique[i], unique[j], &mut best, &mut best_dist);
        }
    }
    best
}

pub(super) fn farthest_pair_points(points: &[crate::Point]) -> (usize, usize) {
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

pub(super) fn farthest_pair_with_scratch(points: &[Point<f32>], scratch: &mut RdpScratchF32) -> (usize, usize) {
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

fn update_best_i32(points: &[Point<i32>], a: usize, b: usize, best: &mut (usize, usize), best_dist: &mut f64) {
    let (i, j) = normalize_pair(a, b);
    let dist = dist2_i32(points, i, j);
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

fn dist2_i32(points: &[Point<i32>], a: usize, b: usize) -> f64 {
    let dx = points[a].x as f64 - points[b].x as f64;
    let dy = points[a].y as f64 - points[b].y as f64;
    dx * dx + dy * dy
}
