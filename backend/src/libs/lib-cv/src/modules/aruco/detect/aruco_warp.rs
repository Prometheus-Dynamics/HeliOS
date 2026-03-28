use super::*;

pub(super) fn decode_quad_aruco_warp(gray: &GrayImage, corners: &[Point<f32>; 4], sample_scale: u32, dict: &ArucoDictionary, cfg: &ArucoDecodeConfig) -> Option<ArucoDetectionF32> {
    if cfg.min_quad_side_px > 0.0 {
        let min_edge =
            [distance(&corners[0], &corners[1]), distance(&corners[1], &corners[2]), distance(&corners[2], &corners[3]), distance(&corners[3], &corners[0])].into_iter().fold(f32::INFINITY, f32::min);
        if min_edge < cfg.min_quad_side_px {
            return None;
        }
    }

    let total_width = (dict.marker_size() as usize).saturating_add(2);
    let edge_lengths = [distance(&corners[0], &corners[1]), distance(&corners[1], &corners[2]), distance(&corners[2], &corners[3]), distance(&corners[3], &corners[0])];
    let avg_edge = edge_lengths.iter().copied().sum::<f32>() / edge_lengths.len() as f32;
    let warp_scale = adaptive_sample_scale_from_avg_edge(avg_edge, total_width as f32, sample_scale).max(1);
    let side = (total_width as u32).saturating_mul(warp_scale);
    if side == 0 {
        return None;
    }

    // Optional quiet-zone verification: for calibration boards we expect a light "quiet" ring
    // outside the black marker border (markers are placed on white squares). This rejects many
    // false positives on plain chessboard squares.
    let quiet_delta_req = cfg.min_quiet_zone_delta.max(0.0);
    let quiet_bits = if quiet_delta_req > 0.0 { 1usize } else { 0usize };
    let warp_side = if quiet_bits == 0 { side } else { ((total_width + 2 * quiet_bits) as u32).saturating_mul(warp_scale).max(1) };

    let centroid = Point::new((corners[0].x + corners[1].x + corners[2].x + corners[3].x) * 0.25, (corners[0].y + corners[1].y + corners[2].y + corners[3].y) * 0.25);
    let from = if quiet_bits == 0 {
        [(corners[0].x, corners[0].y), (corners[1].x, corners[1].y), (corners[2].x, corners[2].y), (corners[3].x, corners[3].y)]
    } else {
        let scale = (warp_side as f32 / side.max(1) as f32).clamp(1.0, 2.0);
        let mut expanded = [(0.0f32, 0.0f32); 4];
        for (dst, p) in expanded.iter_mut().zip(corners.iter()) {
            dst.0 = centroid.x + (p.x - centroid.x) * scale;
            dst.1 = centroid.y + (p.y - centroid.y) * scale;
        }
        expanded
    };
    let to = [(0.0, 0.0), (warp_side as f32, 0.0), (warp_side as f32, warp_side as f32), (0.0, warp_side as f32)];
    let projection = Projection::from_control_points(from, to)?;

    let mut warped_full = GrayImage::new(warp_side, warp_side);
    let interpolation = if warp_scale <= 2 { Interpolation::Nearest } else { Interpolation::Bilinear };
    warp_into(gray, &projection, interpolation, Luma([0u8]), &mut warped_full);

    let mut warped = if quiet_bits == 0 {
        warped_full
    } else {
        // Quiet-zone delta check on the expanded warp.
        let tw_full = total_width + 2 * quiet_bits;
        let cell_px = warp_scale.max(1) as usize;
        let margin_ratio = cfg.cell_sample_margin.clamp(0.0, 0.45);
        let margin_px = ((cell_px as f32) * margin_ratio).round() as usize;
        let margin = margin_px.min(cell_px / 2);
        let span = cell_px.saturating_sub(margin * 2).max(1);
        let inv_area = 1.0f32 / ((span.saturating_mul(span)) as f32);
        let buf = warped_full.as_raw();
        let side_usize = warp_side as usize;

        // Store per-cell means so we can use percentile-based checks (mean can be fooled when a
        // non-marker quad straddles black/white chessboard squares).
        let mut quiet_vals = [0.0f32; 64];
        let mut quiet_n = 0usize;
        let mut border_vals = [0.0f32; 64];
        let mut border_n = 0usize;

        let marker_row0 = quiet_bits;
        let marker_col0 = quiet_bits;
        let marker_row1 = marker_row0 + total_width - 1;
        let marker_col1 = marker_col0 + total_width - 1;

        for row in 0..tw_full {
            for col in 0..tw_full {
                let on_quiet = row == 0 || col == 0 || row + 1 == tw_full || col + 1 == tw_full;
                let on_border =
                    row >= marker_row0 && row <= marker_row1 && col >= marker_col0 && col <= marker_col1 && (row == marker_row0 || row == marker_row1 || col == marker_col0 || col == marker_col1);
                if !on_quiet && !on_border {
                    continue;
                }
                let x0 = col * cell_px;
                let y0 = row * cell_px;
                let xs = x0 + margin;
                let xe = (x0 + cell_px).saturating_sub(margin).max(x0 + 1);
                let ys = y0 + margin;
                let ye = (y0 + cell_px).saturating_sub(margin).max(y0 + 1);
                let mut sum = 0u32;
                for yy in ys..ye {
                    let base = yy * side_usize;
                    for xx in xs..xe {
                        sum += buf[base + xx] as u32;
                    }
                }
                let mean = sum as f32 * inv_area;
                if on_quiet {
                    if quiet_n < quiet_vals.len() {
                        quiet_vals[quiet_n] = mean;
                    }
                    quiet_n += 1;
                } else if on_border {
                    if border_n < border_vals.len() {
                        border_vals[border_n] = mean;
                    }
                    border_n += 1;
                }
            }
        }

        if quiet_n < 4 || border_n < 4 {
            return None;
        }

        let quiet_slice_len = quiet_n.min(quiet_vals.len());
        let border_slice_len = border_n.min(border_vals.len());
        let quiet_slice = &mut quiet_vals[..quiet_slice_len];
        let border_slice = &mut border_vals[..border_slice_len];

        quiet_slice.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        border_slice.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        // Require the quiet ring to be uniformly brighter than the marker border. Using a lower
        // percentile for quiet and an upper percentile for border rejects "average looks ok"
        // cases where the ring includes both black and white chessboard squares.
        let quiet_p25 = quiet_slice[quiet_slice.len() / 4];
        let border_p75 = border_slice[(border_slice.len() * 3) / 4];
        if !quiet_p25.is_finite() || !border_p75.is_finite() {
            return None;
        }
        if (quiet_p25 - border_p75) < quiet_delta_req {
            return None;
        }

        // Crop out the quiet ring and decode only the marker region.
        let crop_off = (quiet_bits * cell_px) as u32;
        let mut cropped = GrayImage::new(side, side);
        let cropped_buf = cropped.as_mut();
        for y in 0..side {
            let src_y = (y + crop_off) as usize;
            let dst_y = y as usize;
            let src_row = src_y * side_usize;
            let dst_row = dst_y * (side as usize);
            let src = &buf[src_row + crop_off as usize..src_row + crop_off as usize + side as usize];
            cropped_buf[dst_row..dst_row + side as usize].copy_from_slice(src);
        }
        cropped
    };

    let summary = decode_aruco_from_warped(&mut warped, dict, cfg, total_width)?;
    let mut marker = ArucoDetectionF32 {
        id: summary.id,
        rotation: summary.rotation,
        border_width: 1,
        data_width: dict.marker_size(),
        corners: Some(corners.to_vec()),
        score: None,
        best_distance: Some(summary.best_distance),
        second_distance: Some(summary.second_distance),
        border_mismatches: Some(summary.border_mismatches),
        contrast_range: Some(summary.contrast_range),
    };
    let margin = summary.second_distance.saturating_sub(summary.best_distance) as f32;
    marker.score = Some(margin * 10.0 + summary.contrast_range - (summary.border_mismatches as f32) * 2.0);
    Some(marker)
}

pub(super) fn decode_quad_aruco_warp_result(
    gray: &GrayImage,
    corners: &[Point<f32>; 4],
    sample_scale: u32,
    dict: &ArucoDictionary,
    cfg: &ArucoDecodeConfig,
) -> Result<ArucoDetectionF32, WarpDecodeError> {
    let (width, height) = gray.dimensions();
    if width < 2 || height < 2 {
        return Err(WarpDecodeError::InvalidInput);
    }
    let max_dim = width.max(height) as f32;
    let min_bound = -max_dim;
    let max_bound = max_dim * 2.0;
    for p in corners.iter() {
        if !p.x.is_finite() || !p.y.is_finite() {
            return Err(WarpDecodeError::InvalidInput);
        }
        if p.x < min_bound || p.x > max_bound || p.y < min_bound || p.y > max_bound {
            return Err(WarpDecodeError::InvalidInput);
        }
    }
    if quad_area(corners) <= f32::EPSILON {
        return Err(WarpDecodeError::InvalidInput);
    }

    struct WarpDecodeCandidate {
        marker: ArucoDetectionF32,
        score: f32,
    }

    fn refine_peak_offset(prev: f32, curr: f32, next: f32) -> f32 {
        let denom = prev - 2.0 * curr + next;
        if denom.abs() < 1e-6 {
            return 0.0;
        }
        0.5 * (prev - next) / denom
    }

    fn fit_edge_line(points: &[Point<f32>]) -> Option<(f32, f32, f32)> {
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

    fn fit_edge_line_refined_with_buffers(
        points: &[Point<f32>],
        trim_ratio: f32,
        base: (f32, f32, f32),
        distances: &mut Vec<(f32, Point<f32>)>,
        trimmed: &mut Vec<Point<f32>>,
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

    fn fit_edge_line_ransac(points: &[Point<f32>], max_dist: f32, iters: usize) -> Option<(f32, f32, f32)> {
        if points.len() < 2 {
            return None;
        }
        let iters = iters.clamp(8, 128);
        with_detect_scratch(|scratch| {
            let mut inliers = std::mem::take(&mut scratch.inliers);
            let mut best_inliers = std::mem::take(&mut scratch.best_inliers);
            inliers.clear();
            best_inliers.clear();
            best_inliers.reserve(points.len());

            for i in 0..iters {
                let a = points[(i * 37) % points.len()];
                let b = points[(i * 57 + 11) % points.len()];
                let dx = b.x - a.x;
                let dy = b.y - a.y;
                let len = (dx * dx + dy * dy).sqrt();
                if !len.is_finite() || len <= f32::EPSILON {
                    continue;
                }
                let nx = -dy / len;
                let ny = dx / len;
                let c = nx * a.x + ny * a.y;

                inliers.clear();
                for &p in points {
                    let d = (nx * p.x + ny * p.y - c).abs();
                    if d <= max_dist {
                        inliers.push(p);
                    }
                }
                if inliers.len() > best_inliers.len() {
                    std::mem::swap(&mut inliers, &mut best_inliers);
                }
            }

            let result = if best_inliers.len() >= 2 {
                let base = fit_edge_line(&best_inliers)?;
                let mut trimmed = std::mem::take(&mut scratch.trimmed);
                let line = fit_edge_line_refined_with_buffers(&best_inliers, 0.2, base, &mut scratch.distances, &mut trimmed)?;
                scratch.trimmed = trimmed;
                Some(line)
            } else {
                None
            };

            scratch.inliers = inliers;
            scratch.best_inliers = best_inliers;
            result
        })
    }

    fn intersect_lines(a: (f32, f32, f32), b: (f32, f32, f32)) -> Option<Point<f32>> {
        let (a1, b1, c1) = a;
        let (a2, b2, c2) = b;
        let det = a1 * b2 - a2 * b1;
        if det.abs() < 1e-6 {
            return None;
        }
        let x = (c1 * b2 - c2 * b1) / det;
        let y = (a1 * c2 - a2 * c1) / det;
        Some(Point::new(x, y))
    }

    fn refine_warp_corners_from_edges(warped: &GrayImage, border_px: u32, min_grad: f32) -> Option<[Point<f32>; 4]> {
        let (side, side_h) = warped.dimensions();
        if side < 8 || side != side_h {
            return None;
        }
        let side_usize = side as usize;
        let buf = warped.as_raw();
        if buf.len() != side_usize.saturating_mul(side_usize) {
            return None;
        }
        let border_px = border_px.max(2).min(side / 4);
        let inner_start = border_px;
        let inner_end = side.saturating_sub(border_px);
        if inner_end <= inner_start + 4 {
            return None;
        }

        let step = (side / 64).clamp(1, 4);
        let search = (border_px.saturating_mul(2)).min(32);

        let grad_h = |x: u32, y: u32| -> f32 {
            let x0 = x.saturating_sub(1);
            let x1 = (x + 1).min(side - 1);
            let idx0 = (y as usize) * side_usize + x0 as usize;
            let idx1 = (y as usize) * side_usize + x1 as usize;
            (buf[idx0] as f32 - buf[idx1] as f32).abs()
        };
        let grad_v = |x: u32, y: u32| -> f32 {
            let y0 = y.saturating_sub(1);
            let y1 = (y + 1).min(side - 1);
            let idx0 = (y0 as usize) * side_usize + x as usize;
            let idx1 = (y1 as usize) * side_usize + x as usize;
            (buf[idx0] as f32 - buf[idx1] as f32).abs()
        };
        // Prefer the edge nearest the patch boundary; strongest-gradient-only can latch onto
        // interior border/data transitions for high-contrast tags.
        let choose_edge_peak = |start: u32, end: u32, from_end: bool, grad: &dyn Fn(u32) -> f32| -> Option<f32> {
            if start >= end {
                return None;
            }

            let mut best_pos = start;
            let mut best_grad = 0.0f32;
            for pos in start..=end {
                let g = grad(pos);
                if g > best_grad {
                    best_grad = g;
                    best_pos = pos;
                }
            }
            if best_grad < min_grad {
                return None;
            }

            let gate = (best_grad * 0.6).max(min_grad);
            let mut picked = None;
            if end > start + 1 {
                if from_end {
                    for pos in ((start + 1)..end).rev() {
                        let prev = grad(pos - 1);
                        let curr = grad(pos);
                        let next = grad(pos + 1);
                        if curr >= gate && curr >= prev && curr >= next {
                            picked = Some(pos);
                            break;
                        }
                    }
                } else {
                    for pos in (start + 1)..end {
                        let prev = grad(pos - 1);
                        let curr = grad(pos);
                        let next = grad(pos + 1);
                        if curr >= gate && curr >= prev && curr >= next {
                            picked = Some(pos);
                            break;
                        }
                    }
                }
            }

            let peak = picked.unwrap_or(best_pos);
            let mut peak_f = peak as f32;
            if peak > start && peak < end {
                let prev = grad(peak - 1);
                let curr = grad(peak);
                let next = grad(peak + 1);
                peak_f += refine_peak_offset(prev, curr, next);
            }
            Some(peak_f)
        };

        let (mut top_pts, mut bottom_pts, mut left_pts, mut right_pts) = with_detect_scratch(|scratch| {
            (std::mem::take(&mut scratch.top_pts), std::mem::take(&mut scratch.bottom_pts), std::mem::take(&mut scratch.left_pts), std::mem::take(&mut scratch.right_pts))
        });

        top_pts.clear();
        bottom_pts.clear();
        for x in (inner_start..inner_end).step_by(step as usize) {
            let y_min = 1u32;
            let y_max = search.min(side.saturating_sub(2));
            if let Some(y_f) = choose_edge_peak(y_min, y_max, false, &|y| grad_v(x, y)) {
                top_pts.push(Point::new(x as f32, y_f));
            }

            let y_min = side.saturating_sub(search).max(1);
            let y_max = side.saturating_sub(2);
            if let Some(y_f) = choose_edge_peak(y_min, y_max, true, &|y| grad_v(x, y)) {
                bottom_pts.push(Point::new(x as f32, y_f));
            }
        }
        left_pts.clear();
        right_pts.clear();
        for y in (inner_start..inner_end).step_by(step as usize) {
            let x_min = 1u32;
            let x_max = search.min(side.saturating_sub(2));
            if let Some(x_f) = choose_edge_peak(x_min, x_max, false, &|x| grad_h(x, y)) {
                left_pts.push(Point::new(x_f, y as f32));
            }

            let x_min = side.saturating_sub(search).max(1);
            let x_max = side.saturating_sub(2);
            if let Some(x_f) = choose_edge_peak(x_min, x_max, true, &|x| grad_h(x, y)) {
                right_pts.push(Point::new(x_f, y as f32));
            }
        }

        let mut result = None;
        if top_pts.len() >= 4 && bottom_pts.len() >= 4 && left_pts.len() >= 4 && right_pts.len() >= 4 {
            let max_dist = 1.5f32;
            if let (Some(top), Some(bottom), Some(left), Some(right)) = (
                fit_edge_line_ransac(&top_pts, max_dist, 64),
                fit_edge_line_ransac(&bottom_pts, max_dist, 64),
                fit_edge_line_ransac(&left_pts, max_dist, 64),
                fit_edge_line_ransac(&right_pts, max_dist, 64),
            ) && let (Some(tl), Some(tr), Some(br), Some(bl)) = (intersect_lines(top, left), intersect_lines(top, right), intersect_lines(bottom, right), intersect_lines(bottom, left))
            {
                let max_pad = search as f32 * 1.5;
                let min_bound = -max_pad;
                let max_bound = side as f32 + max_pad;
                if [tl, tr, br, bl].iter().all(|p| p.x >= min_bound && p.x <= max_bound && p.y >= min_bound && p.y <= max_bound) {
                    result = Some([tl, tr, br, bl]);
                }
            }
        }

        with_detect_scratch(|scratch| {
            scratch.top_pts = top_pts;
            scratch.bottom_pts = bottom_pts;
            scratch.left_pts = left_pts;
            scratch.right_pts = right_pts;
        });

        result
    }

    fn warp_and_decode(gray: &GrayImage, corners: &[Point<f32>; 4], warp_scale: u32, dict: &ArucoDictionary, cfg: &ArucoDecodeConfig) -> Result<WarpDecodeCandidate, WarpDecodeError> {
        let from = [(corners[0].x, corners[0].y), (corners[1].x, corners[1].y), (corners[2].x, corners[2].y), (corners[3].x, corners[3].y)];
        let total_width = (dict.marker_size() as usize).saturating_add(2);
        let side = (total_width as u32).saturating_mul(warp_scale).max(1);
        let to = [(0.0, 0.0), (side as f32, 0.0), (side as f32, side as f32), (0.0, side as f32)];
        let projection = Projection::from_control_points(from, to).ok_or(WarpDecodeError::Projection)?;

        let mut warped = GrayImage::new(side, side);
        let interpolation = if warp_scale <= 2 { Interpolation::Nearest } else { Interpolation::Bilinear };
        warp_into(gray, &projection, interpolation, Luma([0u8]), &mut warped);
        if !normalize_warped_patch(&mut warped, cfg.min_warped_patch_contrast_range) {
            return Err(WarpDecodeError::LowContrast);
        }

        let mut warped_decode = warped;
        let summary = decode_aruco_from_warped(&mut warped_decode, dict, cfg, total_width).ok_or(WarpDecodeError::HammingTooHigh)?;

        let inv = projection.invert();
        let border_px = warp_scale.max(1);
        let min_grad = (summary.contrast_range as f32 * 0.2).clamp(6.0, 32.0);
        let refined_warp = refine_warp_corners_from_edges(&warped_decode, border_px, min_grad);
        let canonical = [(0.0f32, 0.0f32), (side as f32, 0.0f32), (side as f32, side as f32), (0.0f32, side as f32)];

        let mut refined = [Point::new(0.0, 0.0); 4];
        if let Some(refined_warp) = refined_warp {
            for (idx, p) in refined_warp.iter().enumerate() {
                let (rx, ry) = inv * (p.x, p.y);
                refined[idx] = Point::new(rx, ry);
            }
        } else {
            for (idx, (x, y)) in canonical.iter().enumerate() {
                let (rx, ry) = inv * (*x, *y);
                refined[idx] = Point::new(rx, ry);
            }
        }
        if !refined_quad_is_reasonable(corners, &refined) {
            refined = *corners;
        }

        let mut marker = ArucoDetectionF32 {
            id: summary.id,
            rotation: summary.rotation,
            border_width: 1,
            data_width: dict.marker_size(),
            corners: Some(refined.to_vec()),
            score: None,
            best_distance: Some(summary.best_distance),
            second_distance: Some(summary.second_distance),
            border_mismatches: Some(summary.border_mismatches),
            contrast_range: Some(summary.contrast_range),
        };
        let margin = summary.second_distance.saturating_sub(summary.best_distance) as f32;
        let score = margin * 10.0 + summary.contrast_range - (summary.border_mismatches as f32) * 2.0;
        marker.score = Some(score);
        Ok(WarpDecodeCandidate { marker, score })
    }

    let total_width = (dict.marker_size() as usize).saturating_add(2);
    let edge_lengths = [distance(&corners[0], &corners[1]), distance(&corners[1], &corners[2]), distance(&corners[2], &corners[3]), distance(&corners[3], &corners[0])];
    let avg_edge = edge_lengths.iter().copied().sum::<f32>() / edge_lengths.len() as f32;
    let adaptive_scale = adaptive_sample_scale_from_avg_edge(avg_edge, total_width as f32, sample_scale).max(1);
    let warp_scale = adaptive_scale.clamp(1, 32);
    let centroid = Point::new((corners[0].x + corners[1].x + corners[2].x + corners[3].x) * 0.25, (corners[0].y + corners[1].y + corners[2].y + corners[3].y) * 0.25);

    // If contour corner placement is a little loose, try a small shrink toward the centroid.
    // Note: warping is relatively expensive, so we try to avoid doing multiple warps when the
    // first decode is already "obviously good".
    let shrink_factors: &[f32] = &[0.0, 0.03, 0.06, 0.09, 0.12];
    let mut last_err = None;
    let mut best: Option<WarpDecodeCandidate> = None;
    for &shrink in shrink_factors {
        let mut adjusted = *corners;
        if shrink.abs() > f32::EPSILON {
            for p in &mut adjusted {
                p.x += (centroid.x - p.x) * shrink;
                p.y += (centroid.y - p.y) * shrink;
            }
        }
        match warp_and_decode(gray, &adjusted, warp_scale, dict, cfg) {
            Ok(candidate) => {
                // If we got a perfect decode with a perfect border, additional shrink passes
                // cannot improve correctness. Bail early to save work.
                if let (Some(0), Some(0)) = (candidate.marker.best_distance, candidate.marker.border_mismatches) {
                    return Ok(candidate.marker);
                }
                if best.as_ref().map(|b| candidate.score > b.score).unwrap_or(true) {
                    best = Some(candidate);
                }
            }
            Err(err) => last_err = Some(err),
        }
    }

    if let Some(best) = best {
        return Ok(best.marker);
    }

    Err(last_err.unwrap_or(WarpDecodeError::Projection))
}
