use super::*;

pub(super) fn decode_quad(gray: &GrayImage, corners: &[Point<f32>; 4], sample_scale: u32, family: &ArucoTagFamily, config: &ArucoTagDecodeConfig) -> Option<ArucoDetectionF32> {
    let min_quad_side_px = config.min_quad_side_px.max(0.0);
    if min_quad_side_px > 0.0 {
        let mut min_side = f32::INFINITY;
        for i in 0..4 {
            let a = corners[i];
            let b = corners[(i + 1) % 4];
            let dx = a.x - b.x;
            let dy = a.y - b.y;
            min_side = min_side.min((dx * dx + dy * dy).sqrt());
        }
        if !min_side.is_finite() || min_side < min_quad_side_px {
            return None;
        }
    }

    match decode_quad_sampled(gray, corners, sample_scale, family, config) {
        Ok((marker, _verified)) => return Some(marker),
        Err(err) => {
            if !config.warp_fallback_on_decode_fail {
                return None;
            }
            if !should_warp_fallback(&err, config) {
                return None;
            }
        }
    }

    decode_quad_warp(gray, corners, sample_scale, family, config)
}

pub(super) fn decode_quad_with_stats(
    gray: &GrayImage,
    corners: &[Point<f32>; 4],
    sample_scale: u32,
    family: &ArucoTagFamily,
    config: &ArucoTagDecodeConfig,
    stats: &mut DecodeQuadsStats,
) -> Option<ArucoDetectionF32> {
    let min_quad_side_px = config.min_quad_side_px.max(0.0);
    if min_quad_side_px > 0.0 {
        let mut min_side = f32::INFINITY;
        for i in 0..4 {
            let a = corners[i];
            let b = corners[(i + 1) % 4];
            let dx = a.x - b.x;
            let dy = a.y - b.y;
            min_side = min_side.min((dx * dx + dy * dy).sqrt());
        }
        if !min_side.is_finite() || min_side < min_quad_side_px {
            stats.sampled_fail_too_small += 1;
            return None;
        }
    }

    match decode_quad_sampled(gray, corners, sample_scale, family, config) {
        Ok((marker, verified)) => {
            stats.decoded_sampled += 1;
            if verified {
                stats.verify_warp_attempted += 1;
                stats.verify_warp_passed += 1;
            }
            return Some(marker);
        }
        Err(err) => {
            match err {
                SampledDecodeError::Projection => stats.sampled_fail_projection += 1,
                SampledDecodeError::InvalidInput => stats.sampled_fail_invalid_input += 1,
                SampledDecodeError::LowContrast => stats.sampled_fail_low_contrast += 1,
                SampledDecodeError::BorderMismatch { .. } => stats.sampled_fail_border_mismatch += 1,
                SampledDecodeError::QuietZoneDeltaTooLow => stats.sampled_fail_quiet_zone += 1,
                SampledDecodeError::BitDeltaTooLow => stats.sampled_fail_bit_delta_too_low += 1,
                SampledDecodeError::HammingMarginTooLow => stats.sampled_fail_hamming_margin_too_low += 1,
                SampledDecodeError::HammingTooHigh { .. } => stats.sampled_fail_hamming_too_high += 1,
                SampledDecodeError::DecodeScoreTooLow => stats.sampled_fail_low_score += 1,
                SampledDecodeError::VerifyWarpMismatch => {
                    stats.verify_warp_attempted += 1;
                    stats.verify_warp_reject_mismatch += 1;
                }
                SampledDecodeError::VerifyWarpFailed => {
                    stats.verify_warp_attempted += 1;
                    stats.verify_warp_reject_failed += 1;
                }
            }

            if !config.warp_fallback_on_decode_fail {
                return None;
            }
            if !should_warp_fallback(&err, config) {
                return None;
            }
        }
    }

    stats.warp_attempted += 1;
    match decode_quad_warp_result(gray, corners, sample_scale, family, config) {
        Ok(marker) => {
            stats.decoded_warp += 1;
            Some(marker)
        }
        Err(err) => {
            stats.warp_failed += 1;
            match err {
                WarpDecodeError::Projection => stats.warp_fail_projection += 1,
                WarpDecodeError::LowContrast => stats.warp_fail_low_contrast += 1,
                WarpDecodeError::BorderMismatch | WarpDecodeError::InvalidInput => stats.warp_fail_grid += 1,
                WarpDecodeError::HammingTooHigh => stats.warp_fail_family_decode += 1,
                WarpDecodeError::LowScore => stats.warp_fail_low_score += 1,
            }
            None
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum SampledDecodeError {
    Projection,
    InvalidInput,
    LowContrast,
    BorderMismatch { mismatches: usize, allowed: usize },
    QuietZoneDeltaTooLow,
    BitDeltaTooLow,
    HammingMarginTooLow,
    HammingTooHigh { best_distance: u32, max_hamming: u32 },
    DecodeScoreTooLow,
    VerifyWarpMismatch,
    VerifyWarpFailed,
}

pub(super) fn should_warp_fallback(err: &SampledDecodeError, config: &ArucoTagDecodeConfig) -> bool {
    match *err {
        SampledDecodeError::Projection | SampledDecodeError::InvalidInput => false,
        SampledDecodeError::LowContrast => config.warp_fallback_on_low_contrast,
        SampledDecodeError::BorderMismatch { mismatches, allowed } => mismatches <= allowed.saturating_add(config.warp_fallback_border_slack),
        SampledDecodeError::QuietZoneDeltaTooLow => false,
        SampledDecodeError::BitDeltaTooLow => config.warp_fallback_on_low_contrast,
        SampledDecodeError::HammingMarginTooLow => config.warp_fallback_on_decode_fail,
        SampledDecodeError::HammingTooHigh { best_distance, max_hamming } => best_distance <= max_hamming.saturating_add(config.warp_fallback_max_hamming_extra),
        SampledDecodeError::DecodeScoreTooLow => config.warp_fallback_on_decode_fail,
        SampledDecodeError::VerifyWarpMismatch | SampledDecodeError::VerifyWarpFailed => false,
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum WarpDecodeError {
    Projection,
    LowContrast,
    BorderMismatch,
    HammingTooHigh,
    LowScore,
    InvalidInput,
}

pub(super) fn decode_quad_warp(gray: &GrayImage, corners: &[Point<f32>; 4], sample_scale: u32, family: &ArucoTagFamily, config: &ArucoTagDecodeConfig) -> Option<ArucoDetectionF32> {
    decode_quad_warp_result(gray, corners, sample_scale, family, config).ok()
}

pub(super) fn decode_quad_warp_result(
    gray: &GrayImage,
    corners: &[Point<f32>; 4],
    sample_scale: u32,
    family: &ArucoTagFamily,
    config: &ArucoTagDecodeConfig,
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
    }

    fn warp_and_decode(gray: &GrayImage, corners: &[Point<f32>; 4], warp_scale: u32, family: &ArucoTagFamily, config: &ArucoTagDecodeConfig) -> Result<WarpDecodeCandidate, WarpDecodeError> {
        DECODE_SCRATCH.with(|scratch| {
            let mut scratch = scratch.try_borrow_mut().map_err(|_| WarpDecodeError::InvalidInput)?;

            let from = [(corners[0].x, corners[0].y), (corners[1].x, corners[1].y), (corners[2].x, corners[2].y), (corners[3].x, corners[3].y)];
            let side = family.total_width() as u32 * warp_scale;
            if side == 0 {
                return Err(WarpDecodeError::InvalidInput);
            }
            let to = [(0.0, 0.0), (side as f32, 0.0), (side as f32, side as f32), (0.0, side as f32)];

            let projection = Projection::from_control_points(from, to).ok_or(WarpDecodeError::Projection)?;
            if scratch.warped_side != side {
                scratch.warped_side = side;
            }
            let side_usize = side as usize;
            let warped_len = side_usize.saturating_mul(side_usize);
            if scratch.warped_buf.len() != warped_len {
                scratch.warped_buf.resize(warped_len, 0);
                report_decode_scratch(&scratch);
            }
            let mut warped = GrayImage::from_raw(side, side, std::mem::take(&mut scratch.warped_buf)).unwrap_or_else(|| GrayImage::new(side, side));
            let result = (|| {
                let interpolation = if warp_scale <= 2 { Interpolation::Nearest } else { Interpolation::Bilinear };
                warp_into(gray, &projection, interpolation, Luma([0u8]), &mut warped);
                if !normalize_warped_patch(&mut warped, config.min_warped_patch_contrast_range) {
                    return Err(WarpDecodeError::LowContrast);
                }

                let tw = family.total_width() as usize;
                let required = tw.saturating_mul(tw);
                if tw == 0 || required > 100 {
                    return Err(WarpDecodeError::InvalidInput);
                }

                let cell_px = warp_scale.max(1) as usize;
                let buf = warped.as_raw();
                if buf.len() != side_usize.saturating_mul(side_usize) {
                    return Err(WarpDecodeError::InvalidInput);
                }

                let margin_ratio = config.cell_sample_margin.clamp(0.0, 0.45);
                let margin_px = ((cell_px as f32) * margin_ratio).round() as usize;
                let margin = margin_px.min(cell_px / 2);
                let span = cell_px.saturating_sub(margin * 2).max(1);
                let inv_area = 1.0f32 / ((span.saturating_mul(span)) as f32);
                let mut row_offsets = [0usize; 1024];
                let use_row_offsets = side_usize <= row_offsets.len();
                if use_row_offsets {
                    for (yy, slot) in row_offsets.iter_mut().enumerate().take(side_usize) {
                        *slot = yy * side_usize;
                    }
                }
                let mut x_ranges = [(0usize, 0usize); 16];
                let mut y_ranges = [(0usize, 0usize); 16];
                for (col, slot) in x_ranges.iter_mut().enumerate().take(tw) {
                    let x0 = col * cell_px;
                    let x1 = x0 + cell_px;
                    let xs = x0 + margin;
                    let xe = x1.saturating_sub(margin).max(x0 + 1);
                    *slot = (xs, xe);
                }
                for (row, slot) in y_ranges.iter_mut().enumerate().take(tw) {
                    let y0 = row * cell_px;
                    let y1 = y0 + cell_px;
                    let ys = y0 + margin;
                    let ye = y1.saturating_sub(margin).max(y0 + 1);
                    *slot = (ys, ye);
                }

                let mut idx = 0usize;
                for &(ys, ye) in y_ranges.iter().take(tw) {
                    for &(xs, xe) in x_ranges.iter().take(tw) {
                        let mut sum = 0u32;
                        if use_row_offsets {
                            for base in row_offsets.iter().take(ye).skip(ys) {
                                let base = *base;
                                for xx in xs..xe {
                                    sum += buf[base + xx] as u32;
                                }
                            }
                        } else {
                            for yy in ys..ye {
                                let base = yy * side_usize;
                                for xx in xs..xe {
                                    sum += buf[base + xx] as u32;
                                }
                            }
                        }
                        scratch.cell_means[idx] = sum as f32 * inv_area;
                        idx += 1;
                    }
                }

                let decoded = match decode_from_cell_means_result_detailed_with_tuning(&scratch.cell_means[..required], family, &config.cell_decode) {
                    Ok(out) => out,
                    Err(DecodeFromCellMeansError::LowContrast) => return Err(WarpDecodeError::LowContrast),
                    Err(DecodeFromCellMeansError::BorderMismatch { .. }) => return Err(WarpDecodeError::BorderMismatch),
                    Err(DecodeFromCellMeansError::BitDeltaTooLow { .. }) => return Err(WarpDecodeError::HammingTooHigh),
                    Err(DecodeFromCellMeansError::HammingMarginTooLow { .. }) => return Err(WarpDecodeError::HammingTooHigh),
                    Err(DecodeFromCellMeansError::HammingTooHigh { .. }) => return Err(WarpDecodeError::HammingTooHigh),
                    Err(DecodeFromCellMeansError::InvalidInput) => return Err(WarpDecodeError::InvalidInput),
                };

                let margin = decoded.second_distance.saturating_sub(decoded.best_distance) as f32;
                let score = margin * 10.0 + decoded.contrast_range - (decoded.border_mismatches as f32) * 2.0;
                if config.min_decode_score > 0.0 && score < config.min_decode_score {
                    return Err(WarpDecodeError::LowScore);
                }
                let marker = ArucoDetectionF32 {
                    id: decoded.marker.id,
                    rotation: decoded.marker.rotation,
                    border_width: decoded.marker.border_width,
                    data_width: decoded.marker.data_width,
                    // Keep decode geometry and reported geometry identical.
                    // Warp corner post-processing can move corners into the tag interior on skewed/far tags.
                    corners: Some(corners.to_vec()),
                    score: Some(score),
                    best_distance: Some(decoded.best_distance),
                    second_distance: Some(decoded.second_distance),
                    border_mismatches: Some(decoded.border_mismatches),
                    contrast_range: Some(decoded.contrast_range),
                };
                Ok(WarpDecodeCandidate { marker })
            })();
            scratch.warped_buf = warped.into_raw();
            result
        })
    }

    let adaptive_scale = adaptive_sample_scale(corners, family, sample_scale);
    let warp_scale = adaptive_scale.max(config.warp_min_sample_scale).clamp(1, 32);

    match warp_and_decode(gray, corners, warp_scale, family, config) {
        Ok(candidate) => Ok(candidate.marker),
        Err(err) => Err(err),
    }
}
