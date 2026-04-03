use super::*;

pub(in crate::services::calibration) type Point2 = (f64, f64);

pub(in crate::services::calibration) fn quad_center(quad: &[Point; 4]) -> Point {
    let mut x = 0.0;
    let mut y = 0.0;
    for p in quad {
        x += p.x;
        y += p.y;
    }
    Point { x: x / 4.0, y: y / 4.0 }
}

pub(in crate::services::calibration) fn project_homography(h: &Matrix3<f64>, p: Point2) -> Option<Point2> {
    let x = h[(0, 0)] * p.0 + h[(0, 1)] * p.1 + h[(0, 2)];
    let y = h[(1, 0)] * p.0 + h[(1, 1)] * p.1 + h[(1, 2)];
    let z = h[(2, 0)] * p.0 + h[(2, 1)] * p.1 + h[(2, 2)];
    if !z.is_finite() || z.abs() <= 1e-12 {
        return None;
    }
    Some((x / z, y / z))
}

pub(in crate::services::calibration) fn normalize_points(points: &[Point2]) -> Option<(Matrix3<f64>, Vec<Point2>)> {
    if points.is_empty() {
        return None;
    }
    let mut cx = 0.0;
    let mut cy = 0.0;
    for (x, y) in points {
        cx += *x;
        cy += *y;
    }
    cx /= points.len() as f64;
    cy /= points.len() as f64;

    let mut mean_dist = 0.0;
    for (x, y) in points {
        mean_dist += ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
    }
    mean_dist /= points.len() as f64;
    if !mean_dist.is_finite() || mean_dist <= 1e-12 {
        return None;
    }
    let s = (2.0f64).sqrt() / mean_dist;
    let t = Matrix3::new(s, 0.0, -s * cx, 0.0, s, -s * cy, 0.0, 0.0, 1.0);
    let out = points.iter().map(|(x, y)| (s * (x - cx), s * (y - cy))).collect();
    Some((t, out))
}

pub(in crate::services::calibration) fn solve_homography(obj: &[Point2], img: &[Point2]) -> Option<Matrix3<f64>> {
    if obj.len() != img.len() || obj.len() < 4 {
        return None;
    }
    let (t_obj, norm_obj) = normalize_points(obj)?;
    let (t_img, norm_img) = normalize_points(img)?;

    let n = obj.len();
    let mut a = DMatrix::<f64>::zeros(2 * n, 9);
    for (k, ((x, y), (u, v))) in norm_obj.iter().zip(norm_img.iter()).enumerate() {
        let row1 = 2 * k;
        let row2 = row1 + 1;
        a[(row1, 0)] = -*x;
        a[(row1, 1)] = -*y;
        a[(row1, 2)] = -1.0;
        a[(row1, 6)] = u * x;
        a[(row1, 7)] = u * y;
        a[(row1, 8)] = *u;

        a[(row2, 3)] = -*x;
        a[(row2, 4)] = -*y;
        a[(row2, 5)] = -1.0;
        a[(row2, 6)] = v * x;
        a[(row2, 7)] = v * y;
        a[(row2, 8)] = *v;
    }

    let svd = a.svd(true, true);
    let vt = svd.v_t?;
    let h = vt.row(vt.nrows() - 1).transpose();
    if h.len() != 9 {
        return None;
    }
    // DLT solves for `h` in row-major order: [h11, h12, h13, h21, ... , h33].
    // `Matrix3::from_column_slice` would interpret this as column-major and effectively transpose/scramble H.
    let hn = Matrix3::from_row_slice(h.as_slice());
    let h = t_img.try_inverse()? * hn * t_obj;
    let scale = if h[(2, 2)].abs() > 1e-12 { 1.0 / h[(2, 2)] } else { 1.0 };
    Some(h * scale)
}

pub(in crate::services::calibration) fn robust_homography_pairs(pairs: &[(Point2, Point2)], min_threshold_px: f64) -> Option<(Matrix3<f64>, Vec<bool>, f64)> {
    if pairs.len() < 4 {
        return None;
    }
    let mut nn_dists = Vec::with_capacity(pairs.len());
    for (i, (_, img_a)) in pairs.iter().enumerate() {
        let mut best = f64::INFINITY;
        for (j, (_, img_b)) in pairs.iter().enumerate() {
            if i == j {
                continue;
            }
            let dx = img_a.0 - img_b.0;
            let dy = img_a.1 - img_b.1;
            let d = (dx * dx + dy * dy).sqrt();
            if d < best {
                best = d;
            }
        }
        if best.is_finite() {
            nn_dists.push(best);
        }
    }
    let mut nn_sorted = nn_dists.clone();
    let median_nn = median_in_place(&mut nn_sorted).unwrap_or(0.0);
    let min_threshold_px = if min_threshold_px.is_finite() { min_threshold_px.max(0.0) } else { 0.0 };
    let threshold = if median_nn.is_finite() && median_nn > 0.0 { (median_nn * 0.2).max(min_threshold_px).max(6.0) } else { 8.0f64.max(min_threshold_px) };

    let mut seed = (pairs.len() as u64).wrapping_mul(1_664_525_295).wrapping_add(1_013_904_223);
    let mut best_inliers: Vec<bool> = vec![false; pairs.len()];
    let mut best_count = 0usize;
    let mut best_error = f64::INFINITY;
    let mut best_h: Option<Matrix3<f64>> = None;

    let iterations = 64usize.max(pairs.len().saturating_mul(2));
    for _ in 0..iterations {
        let mut idx = [0usize; 4];
        let mut used = [false; 4];
        let mut count = 0usize;
        while count < 4 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let cand = (seed % (pairs.len() as u64)) as usize;
            if idx[..count].iter().all(|v| *v != cand) {
                idx[count] = cand;
                used[count] = true;
                count += 1;
            }
        }
        if !used.iter().all(|v| *v) {
            continue;
        }
        let mut obj = Vec::with_capacity(4);
        let mut img = Vec::with_capacity(4);
        for &i in &idx {
            obj.push(pairs[i].0);
            img.push(pairs[i].1);
        }
        let Some(h) = solve_homography(&obj, &img) else {
            continue;
        };
        let mut inliers = vec![false; pairs.len()];
        let mut inlier_count = 0usize;
        let mut total_error = 0.0;
        for (i, (o, img)) in pairs.iter().enumerate() {
            let err = match project_homography(&h, *o) {
                Some((u, v)) => {
                    let dx = u - img.0;
                    let dy = v - img.1;
                    (dx * dx + dy * dy).sqrt()
                }
                None => f64::INFINITY,
            };
            if err <= threshold {
                inliers[i] = true;
                inlier_count += 1;
                total_error += err;
            }
        }
        if inlier_count > best_count || (inlier_count == best_count && total_error < best_error) {
            best_count = inlier_count;
            best_error = total_error;
            best_inliers = inliers;
            best_h = Some(h);
        }
    }

    if best_count < 4 {
        return None;
    }

    let mut obj = Vec::with_capacity(best_count);
    let mut img = Vec::with_capacity(best_count);
    for (idx, ok) in best_inliers.iter().enumerate() {
        if *ok {
            obj.push(pairs[idx].0);
            img.push(pairs[idx].1);
        }
    }
    let h = solve_homography(&obj, &img).or(best_h)?;
    Some((h, best_inliers, threshold))
}

pub(in crate::services::calibration) fn order_marker_corners(rotation: u8, quad: [Point; 4]) -> [Point; 4] {
    let r = (rotation & 3) as usize;
    // `ArucoDetection2D` corners are canonicalized to an image-based ordering (TL/TR/BR/BL).
    //
    // The decoder's `rotation` is the "code rotation": how many 90-degree rotations are
    // required to align the sampled grid with the dictionary's canonical orientation.
    //
    // OpenCV's `detectMarkers()` returns corners in tag-canonical order (tag-local TL/TR/BR/BL),
    // so we must rotate image-ordered corners *back* by the code rotation.
    let shift = (4 - r) & 3;
    [quad[shift], quad[(shift + 1) & 3], quad[(shift + 2) & 3], quad[(shift + 3) & 3]]
}

pub(in crate::services::calibration) fn median_in_place(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.total_cmp(b));
    Some(values[values.len() / 2])
}

pub(in crate::services::calibration) fn refine_charuco_corner_harris(gray: &GrayImage, u: f64, v: f64) -> Option<(f64, f64)> {
    if !u.is_finite() || !v.is_finite() {
        return None;
    }
    let w = gray.width() as i32;
    let h = gray.height() as i32;
    if w < 16 || h < 16 {
        return None;
    }

    // Search a small window around the predicted corner for a strong 2D corner response.
    // This is a lightweight substitute for OpenCV's cornerSubPix and dramatically reduces
    // ChArUco reprojection error versus pure homography projection, especially on fisheye lenses.
    // The homography-projected intersections can be off by ~10-30px on strong fisheye lenses.
    // Use a larger window to reliably snap to the true black/white intersection.
    let search = 24i32;
    let tensor_r = 2i32; // 5x5 tensor window
    let k = 0.04f64;

    let cx = u.round() as i32;
    let cy = v.round() as i32;

    let mut best: Option<(i32, i32, f64)> = None;
    for yy in (cy - search)..=(cy + search) {
        for xx in (cx - search)..=(cx + search) {
            // Need a 1px border for gradients, plus tensor radius.
            if xx <= tensor_r + 1 || yy <= tensor_r + 1 || xx >= w - (tensor_r + 2) || yy >= h - (tensor_r + 2) {
                continue;
            }
            let r = harris_response(gray, xx, yy, tensor_r, k)?;
            if !r.is_finite() {
                continue;
            }
            match best {
                Some((_, _, best_r)) if best_r >= r => {}
                _ => best = Some((xx, yy, r)),
            }
        }
    }

    let (bx, by, _br) = best?;

    // Simple subpixel tweak: compute a response-weighted centroid in a 3x3 neighborhood.
    let mut sum_w = 0.0f64;
    let mut sum_x = 0.0f64;
    let mut sum_y = 0.0f64;
    for yy in (by - 1)..=(by + 1) {
        for xx in (bx - 1)..=(bx + 1) {
            if xx <= tensor_r + 1 || yy <= tensor_r + 1 || xx >= w - (tensor_r + 2) || yy >= h - (tensor_r + 2) {
                continue;
            }
            let r = harris_response(gray, xx, yy, tensor_r, k).unwrap_or(0.0);
            let wgt = r.max(0.0);
            sum_w += wgt;
            sum_x += (xx as f64) * wgt;
            sum_y += (yy as f64) * wgt;
        }
    }
    if sum_w > 0.0 {
        Some((sum_x / sum_w, sum_y / sum_w))
    } else {
        Some((bx as f64, by as f64))
    }
}

pub(in crate::services::calibration) fn harris_response(gray: &GrayImage, x: i32, y: i32, r: i32, k: f64) -> Option<f64> {
    let w = gray.width() as i32;
    let h = gray.height() as i32;
    if x - r - 1 < 0 || y - r - 1 < 0 || x + r + 1 >= w || y + r + 1 >= h {
        return None;
    }

    let mut sxx = 0.0f64;
    let mut syy = 0.0f64;
    let mut sxy = 0.0f64;
    for yy in (y - r)..=(y + r) {
        for xx in (x - r)..=(x + r) {
            let xm1 = (xx - 1) as u32;
            let xp1 = (xx + 1) as u32;
            let ym1 = (yy - 1) as u32;
            let yp1 = (yy + 1) as u32;

            let i_xp1 = gray.get_pixel(xp1, yy as u32).0[0] as i32;
            let i_xm1 = gray.get_pixel(xm1, yy as u32).0[0] as i32;
            let i_yp1 = gray.get_pixel(xx as u32, yp1).0[0] as i32;
            let i_ym1 = gray.get_pixel(xx as u32, ym1).0[0] as i32;
            let ix = (i_xp1 - i_xm1) as f64;
            let iy = (i_yp1 - i_ym1) as f64;

            sxx += ix * ix;
            syy += iy * iy;
            sxy += ix * iy;
        }
    }

    let det = sxx * syy - sxy * sxy;
    let trace = sxx + syy;
    Some(det - k * trace * trace)
}

pub(in crate::services::calibration) fn tag_area_stats(entries: &[DebugEntry], layout: &BoardLayout, base_min_area: f64, min_points: usize, min_views: usize) -> TagAreaStats {
    let mut areas: Vec<f64> = Vec::new();
    let mut total_views = 0usize;
    for entry in entries {
        let DebugEntry::PendingImage(image) = entry else { continue };
        total_views = total_views.saturating_add(1);
        for det in &image.detections {
            if !layout.tag_corners_by_id.contains_key(&det.id) {
                continue;
            }
            let area = quad_area_px2(&det.corners);
            if !area.is_finite() || area <= 0.0 {
                continue;
            }
            areas.push(area);
        }
    }

    let count = areas.len();
    let mut stats = TagAreaStats { median: None, min_area: base_min_area, count };
    if areas.is_empty() {
        return stats;
    }

    areas.sort_by(|a, b| a.total_cmp(b));
    stats.median = Some(areas[areas.len() / 2]);

    let min_views_required = min_views.max((total_views / 2).max(4));
    let percentiles = [0.95, 0.9, 0.85, 0.8, 0.7, 0.6, 0.5, 0.4, 0.0];
    let mut chosen = base_min_area;
    let ratio = calibration_min_tag_area_median_ratio();
    for p in percentiles {
        let idx = ((areas.len().saturating_sub(1)) as f64 * p).round() as usize;
        let q = areas[idx.min(areas.len() - 1)];
        let threshold = base_min_area.max(q * ratio);
        let usable = count_views_above_area(entries, layout, threshold, min_points);
        if usable >= min_views_required {
            chosen = threshold;
            break;
        }
    }
    stats.min_area = chosen;
    stats
}

pub(in crate::services::calibration) fn count_views_above_area(entries: &[DebugEntry], layout: &BoardLayout, min_area: f64, min_points: usize) -> usize {
    let mut usable = 0usize;
    for entry in entries {
        let DebugEntry::PendingImage(image) = entry else { continue };
        let mut ids: HashSet<u32> = HashSet::new();
        for det in &image.detections {
            if !layout.tag_corners_by_id.contains_key(&det.id) {
                continue;
            }
            let area = quad_area_px2(&det.corners);
            if !area.is_finite() || area <= 0.0 || area < min_area {
                continue;
            }
            ids.insert(det.id);
        }
        if ids.len().saturating_mul(4) >= min_points {
            usable = usable.saturating_add(1);
        }
    }
    usable
}
