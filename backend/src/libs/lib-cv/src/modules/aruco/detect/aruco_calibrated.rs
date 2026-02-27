use super::*;

const ARUCO_DECODE_PAR_MIN_QUADS: usize = 96;

fn decode_quad_aruco_calibrated(
    gray: &GrayImage,
    corners: &[Point<f32>; 4],
    sample_scale: u32,
    dict: &ArucoDictionary,
    calib: CameraCalibration,
    cfg: &ArucoDecodeConfig,
) -> Option<ArucoDetectionF32> {
    if cfg.min_quad_side_px > 0.0 {
        let min_edge =
            [distance(&corners[0], &corners[1]), distance(&corners[1], &corners[2]), distance(&corners[2], &corners[3]), distance(&corners[3], &corners[0])].into_iter().fold(f32::INFINITY, f32::min);
        if min_edge < cfg.min_quad_side_px {
            return None;
        }
    }
    if calib.fx <= f32::EPSILON || calib.fy <= f32::EPSILON {
        return decode_quad_aruco_warp(gray, corners, sample_scale, dict, cfg);
    }

    let mut undistorted = [Point::new(0.0, 0.0); 4];
    for (dst, corner) in undistorted.iter_mut().zip(corners.iter()) {
        let xd = (corner.x - calib.cx) / calib.fx;
        let yd = (corner.y - calib.cy) / calib.fy;
        let (xu, yu) = undistort_norm(xd, yd, calib);
        dst.x = calib.fx * xu + calib.cx;
        dst.y = calib.fy * yu + calib.cy;
    }

    let total_width = (dict.marker_size() as usize).saturating_add(2);
    let edge_lengths = [distance(&corners[0], &corners[1]), distance(&corners[1], &corners[2]), distance(&corners[2], &corners[3]), distance(&corners[3], &corners[0])];
    let avg_edge = edge_lengths.iter().copied().sum::<f32>() / edge_lengths.len() as f32;
    let warp_scale = adaptive_sample_scale_from_avg_edge(avg_edge, total_width as f32, sample_scale).max(1);
    let side = (total_width as u32).saturating_mul(warp_scale);

    let quiet_delta_req = cfg.min_quiet_zone_delta.max(0.0);
    let quiet_bits = if quiet_delta_req > 0.0 { 1usize } else { 0usize };
    let warp_side = if quiet_bits == 0 { side } else { ((total_width + 2 * quiet_bits) as u32).saturating_mul(warp_scale).max(1) };

    let centroid = Point::new((undistorted[0].x + undistorted[1].x + undistorted[2].x + undistorted[3].x) * 0.25, (undistorted[0].y + undistorted[1].y + undistorted[2].y + undistorted[3].y) * 0.25);
    let from = if quiet_bits == 0 {
        [(undistorted[0].x, undistorted[0].y), (undistorted[1].x, undistorted[1].y), (undistorted[2].x, undistorted[2].y), (undistorted[3].x, undistorted[3].y)]
    } else {
        let scale = (warp_side as f32 / side.max(1) as f32).clamp(1.0, 2.0);
        let mut expanded = [(0.0f32, 0.0f32); 4];
        for (dst, p) in expanded.iter_mut().zip(undistorted.iter()) {
            dst.0 = centroid.x + (p.x - centroid.x) * scale;
            dst.1 = centroid.y + (p.y - centroid.y) * scale;
        }
        expanded
    };
    let to = [(0.0, 0.0), (warp_side as f32, 0.0), (warp_side as f32, warp_side as f32), (0.0, warp_side as f32)];
    let projection = Projection::from_control_points(from, to)?;
    let inv = projection.invert();

    let mut warped_full = GrayImage::new(warp_side, warp_side);
    let src_width_i32 = gray.width() as i32;
    let src_height_i32 = gray.height() as i32;
    let src_stride = src_width_i32.max(0) as usize;
    let src_pixels = gray.as_raw();
    for y in 0..warp_side {
        for x in 0..warp_side {
            let (ux, uy) = inv * (x as f32, y as f32);
            let xu = (ux - calib.cx) / calib.fx;
            let yu = (uy - calib.cy) / calib.fy;
            let (xd, yd) = distort_norm(xu, yu, calib);
            let sx = calib.fx * xd + calib.cx;
            let sy = calib.fy * yd + calib.cy;
            let v = sample_bilinear_gray_raw(src_pixels, src_width_i32, src_height_i32, src_stride, sx, sy);
            warped_full.put_pixel(x, y, Luma([v]));
        }
    }

    let mut warped = if quiet_bits == 0 {
        warped_full
    } else {
        let tw_full = total_width + 2 * quiet_bits;
        let cell_px = warp_scale.max(1) as usize;
        let margin_ratio = cfg.cell_sample_margin.clamp(0.0, 0.45);
        let margin_px = ((cell_px as f32) * margin_ratio).round() as usize;
        let margin = margin_px.min(cell_px / 2);
        let span = cell_px.saturating_sub(margin * 2).max(1);
        let inv_area = 1.0f32 / ((span.saturating_mul(span)) as f32);
        let buf = warped_full.as_raw();
        let side_usize = warp_side as usize;

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

        let quiet_p25 = quiet_slice[quiet_slice.len() / 4];
        let border_p75 = border_slice[(border_slice.len() * 3) / 4];
        if !quiet_p25.is_finite() || !border_p75.is_finite() {
            return None;
        }
        if (quiet_p25 - border_p75) < quiet_delta_req {
            return None;
        }

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
    let canonical = [(0.0f32, 0.0f32), (side as f32, 0.0f32), (side as f32, side as f32), (0.0f32, side as f32)];
    let mut refined = Vec::with_capacity(4);
    for (x, y) in canonical {
        let (ux, uy) = inv * (x, y);
        let xu = (ux - calib.cx) / calib.fx;
        let yu = (uy - calib.cy) / calib.fy;
        let (xd, yd) = distort_norm(xu, yu, calib);
        refined.push(Point::new(calib.fx * xd + calib.cx, calib.fy * yd + calib.cy));
    }

    let mut marker = ArucoDetectionF32 {
        id: summary.id,
        rotation: summary.rotation,
        border_width: 1,
        data_width: dict.marker_size(),
        corners: Some(refined),
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

pub fn decode_quads_aruco_with_config(frame: &DynamicImage, quads: &[[Point<f32>; 4]], sample_scale: u32, dict: &ArucoDictionary, cfg: &ArucoDecodeConfig) -> Vec<ArucoDetection2D> {
    decode_quads_aruco_with_config_bits(frame, quads, sample_scale, dict, cfg, true)
}

pub fn decode_quads_aruco_with_config_no_bits(frame: &DynamicImage, quads: &[[Point<f32>; 4]], sample_scale: u32, dict: &ArucoDictionary, cfg: &ArucoDecodeConfig) -> Vec<ArucoDetection2D> {
    decode_quads_aruco_with_config_bits(frame, quads, sample_scale, dict, cfg, false)
}

fn decode_quads_aruco_with_config_bits(
    frame: &DynamicImage,
    quads: &[[Point<f32>; 4]],
    sample_scale: u32,
    dict: &ArucoDictionary,
    cfg: &ArucoDecodeConfig,
    include_bits: bool,
) -> Vec<ArucoDetection2D> {
    if quads.is_empty() {
        return Vec::new();
    }
    with_luma8_frame(frame, |gray| {
        let decoded: Vec<ArucoDetectionF32> = if quads.len() < ARUCO_DECODE_PAR_MIN_QUADS {
            quads.iter().filter_map(|quad| decode_quad_aruco_warp(gray, quad, sample_scale, dict, cfg)).collect()
        } else {
            quads.par_iter().filter_map(|quad| decode_quad_aruco_warp(gray, quad, sample_scale, dict, cfg)).collect()
        };
        decoded
            .into_iter()
            .filter_map(|m| {
                let bits = if include_bits { dict.bit_grid(m.id as usize) } else { None };
                marker_f32_to_detection_2d(m, bits)
            })
            .collect()
    })
}

pub fn decode_quads_aruco_calibrated_with_config(
    frame: &DynamicImage,
    quads: &[[Point<f32>; 4]],
    sample_scale: u32,
    dict: &ArucoDictionary,
    calib: CameraCalibration,
    cfg: &ArucoDecodeConfig,
) -> Vec<ArucoDetection2D> {
    decode_quads_aruco_calibrated_with_config_bits(frame, quads, sample_scale, dict, calib, cfg, true)
}

pub fn decode_quads_aruco_calibrated_with_config_no_bits(
    frame: &DynamicImage,
    quads: &[[Point<f32>; 4]],
    sample_scale: u32,
    dict: &ArucoDictionary,
    calib: CameraCalibration,
    cfg: &ArucoDecodeConfig,
) -> Vec<ArucoDetection2D> {
    decode_quads_aruco_calibrated_with_config_bits(frame, quads, sample_scale, dict, calib, cfg, false)
}

fn decode_quads_aruco_calibrated_with_config_bits(
    frame: &DynamicImage,
    quads: &[[Point<f32>; 4]],
    sample_scale: u32,
    dict: &ArucoDictionary,
    calib: CameraCalibration,
    cfg: &ArucoDecodeConfig,
    include_bits: bool,
) -> Vec<ArucoDetection2D> {
    if quads.is_empty() {
        return Vec::new();
    }
    with_luma8_frame(frame, |gray| {
        let decoded: Vec<ArucoDetectionF32> = if quads.len() < ARUCO_DECODE_PAR_MIN_QUADS {
            quads.iter().filter_map(|quad| decode_quad_aruco_calibrated(gray, quad, sample_scale, dict, calib, cfg)).collect()
        } else {
            quads.par_iter().filter_map(|quad| decode_quad_aruco_calibrated(gray, quad, sample_scale, dict, calib, cfg)).collect()
        };
        decoded
            .into_iter()
            .filter_map(|m| {
                let bits = if include_bits { dict.bit_grid(m.id as usize) } else { None };
                marker_f32_to_detection_2d(m, bits)
            })
            .collect()
    })
}
