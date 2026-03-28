use super::*;

fn quiet_zone_delta_adjusted(gray: &GrayImage, inv: &Projection, cell_means: &[f32], total_width: usize, border_width: usize, texture_penalty_scale: f32) -> Option<f32> {
    if total_width == 0 || border_width == 0 || border_width * 2 >= total_width {
        return None;
    }
    let cell_size = 1.0 / total_width as f32;
    if !cell_size.is_finite() || cell_size <= 0.0 {
        return None;
    }
    let expected = total_width.saturating_mul(total_width);
    if cell_means.len() != expected {
        return None;
    }

    let mut border_vals = [0.0f32; 100];
    let mut nb = 0usize;
    for row in 0..total_width {
        for col in 0..total_width {
            let on_border = row < border_width || col < border_width || row + border_width >= total_width || col + border_width >= total_width;
            if !on_border {
                continue;
            }
            if nb >= border_vals.len() {
                break;
            }
            border_vals[nb] = cell_means[row * total_width + col];
            nb += 1;
        }
    }
    if nb == 0 {
        return None;
    }
    let border_vals = &mut border_vals[..nb];
    border_vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let k_border = (nb / 3).max(1);
    let black_ref = border_vals.iter().take(k_border).copied().sum::<f32>() / k_border as f32;

    let src_width_i32 = gray.width() as i32;
    let src_height_i32 = gray.height() as i32;
    let src_stride = src_width_i32.max(0) as usize;
    let src_pixels = gray.as_raw();

    let mut outside_vals = [0.0f32; 64];
    let mut outside_sum = 0.0f32;
    let mut outside_sq_sum = 0.0f32;
    let mut outside_n = 0usize;

    let tw_i32 = total_width as i32;
    for row in -1..=tw_i32 {
        for col in -1..=tw_i32 {
            let on_ring = row == -1 || col == -1 || row == tw_i32 || col == tw_i32;
            if !on_ring {
                continue;
            }

            let cx = (col as f32 + 0.5) * cell_size;
            let cy = (row as f32 + 0.5) * cell_size;
            let (sx, sy) = *inv * (cx, cy);
            if !sx.is_finite() || !sy.is_finite() {
                continue;
            }
            let x0 = sx.floor() as i32;
            let y0 = sy.floor() as i32;
            let x1 = x0 + 1;
            let y1 = y0 + 1;
            if x0 < 0 || y0 < 0 || x1 >= src_width_i32 || y1 >= src_height_i32 {
                continue;
            }
            let v = sample_bilinear_gray_raw(src_pixels, src_width_i32, src_height_i32, src_stride, sx, sy) as f32;
            if outside_n < outside_vals.len() {
                outside_vals[outside_n] = v;
            }
            outside_sum += v;
            outside_sq_sum += v * v;
            outside_n += 1;
        }
    }

    if outside_n < 8 {
        return None;
    }

    let outside_cap = outside_vals.len();
    let outside_slice_len = outside_n.min(outside_cap);
    let outside_slice = &mut outside_vals[..outside_slice_len];
    outside_slice.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    // A true quiet zone should be consistently light. Using a lower percentile is stricter than
    // the mean and helps reject textured regions (e.g. carpet/chair grain) that can accidentally
    // match a low-bit-count family like 16h5.
    let outside_p25 = outside_slice[outside_slice.len() / 4];

    let outside_mean = outside_sum / outside_n as f32;
    let outside_var = (outside_sq_sum / outside_n as f32) - (outside_mean * outside_mean);
    let outside_std = outside_var.max(0.0).sqrt();

    // Textured backgrounds can produce large "quiet zone" deltas due to local speckle.
    // Penalize high-variance rings to bias toward smooth paper backgrounds.
    let texture_penalty = (outside_std / 64.0).min(1.0) * texture_penalty_scale.max(0.0);

    // Penalize texture using the mean-based std dev, but compute the final delta from a lower
    // percentile to ensure the ring is uniformly bright.
    Some((outside_p25 - black_ref) - texture_penalty)
}

pub(super) fn decode_quad_sampled(
    gray: &GrayImage,
    corners: &[Point<f32>; 4],
    requested_scale: u32,
    family: &ArucoTagFamily,
    config: &ArucoTagDecodeConfig,
) -> Result<(ArucoDetectionF32, bool), SampledDecodeError> {
    #[allow(clippy::too_many_arguments)]
    fn decode_once_with_inv(
        gray: &GrayImage,
        inv: &Projection,
        corners: &[Point<f32>; 4],
        requested_scale: u32,
        total_width: usize,
        cell_size: f32,
        required: usize,
        family: &ArucoTagFamily,
        config: &ArucoTagDecodeConfig,
    ) -> Result<(ArucoDetectionF32, bool), SampledDecodeError> {
        if total_width == 0 || required == 0 || required > 100 || !cell_size.is_finite() || cell_size <= 0.0 {
            return Err(SampledDecodeError::InvalidInput);
        }
        let (marker, should_verify) = with_decode_scratch(|scratch| {
            let in_bounds = projection_unit_in_bounds(inv, gray.width() as i32, gray.height() as i32);
            if config.cell_sample_grid >= 2 {
                let margin_bits = config.cell_sample_margin.to_bits();
                if scratch.sample_positions_total_width != total_width || scratch.sample_positions_grid != config.cell_sample_grid || scratch.sample_positions_margin_bits != margin_bits {
                    scratch.sample_positions_per_cell = build_sample_positions_unit(total_width, cell_size, config.cell_sample_grid, config.cell_sample_margin, &mut scratch.sample_positions);
                    scratch.sample_positions_total_width = total_width;
                    scratch.sample_positions_grid = config.cell_sample_grid;
                    scratch.sample_positions_margin_bits = margin_bits;
                    report_decode_scratch(&scratch);
                }
                if scratch.sample_positions_per_cell == 0 {
                    return Err(SampledDecodeError::InvalidInput);
                }
                let positions_per_cell = scratch.sample_positions_per_cell;
                let DecodeScratch { cell_means, sample_positions, .. } = &mut *scratch;
                sample_cell_means_from_unit_positions(gray, inv, total_width, &*sample_positions, positions_per_cell, in_bounds, &mut cell_means[..required]);
            } else {
                let ratio = cell_sample_ratio(cell_size);
                let cell_means = &mut scratch.cell_means;
                sample_cell_means_from_unit_offsets(gray, inv, total_width, ratio, in_bounds, &mut cell_means[..required]);
            }
            let cell_means = &scratch.cell_means;
            let decoded = match decode_from_cell_means_result_detailed_with_tuning(&cell_means[..required], family, &config.cell_decode) {
                Ok(marker) => marker,
                Err(DecodeFromCellMeansError::InvalidInput) => return Err(SampledDecodeError::InvalidInput),
                Err(DecodeFromCellMeansError::LowContrast) => return Err(SampledDecodeError::LowContrast),
                Err(DecodeFromCellMeansError::BorderMismatch { mismatches, allowed }) => {
                    return Err(SampledDecodeError::BorderMismatch { mismatches, allowed });
                }
                Err(DecodeFromCellMeansError::BitDeltaTooLow { .. }) => {
                    return Err(SampledDecodeError::BitDeltaTooLow);
                }
                Err(DecodeFromCellMeansError::HammingMarginTooLow { .. }) => {
                    return Err(SampledDecodeError::HammingMarginTooLow);
                }
                Err(DecodeFromCellMeansError::HammingTooHigh { best_distance, max_hamming }) => {
                    return Err(SampledDecodeError::HammingTooHigh { best_distance, max_hamming });
                }
            };

            let min_quiet_zone_delta = config.min_quiet_zone_delta.max(0.0);
            if min_quiet_zone_delta > 0.0 {
                let border_width = family.border_size() as usize;
                match quiet_zone_delta_adjusted(gray, inv, &cell_means[..required], total_width, border_width, config.quiet_zone_texture_penalty) {
                    Some(delta) => {
                        if !delta.is_finite() || delta < min_quiet_zone_delta {
                            return Err(SampledDecodeError::QuietZoneDeltaTooLow);
                        }
                    }
                    None => {
                        // If we can't reliably sample the quiet zone ring (e.g. degenerate projection or
                        // mostly out-of-bounds), treat this as a failed verification when enabled.
                        return Err(SampledDecodeError::QuietZoneDeltaTooLow);
                    }
                }
            }

            let margin = decoded.second_distance.saturating_sub(decoded.best_distance) as f32;
            let score = margin * 10.0 + decoded.contrast_range - (decoded.border_mismatches as f32) * 2.0;
            if config.min_decode_score > 0.0 && score < config.min_decode_score {
                return Err(SampledDecodeError::DecodeScoreTooLow);
            }
            let marker = ArucoDetectionF32 {
                id: decoded.marker.id,
                rotation: decoded.marker.rotation,
                border_width: decoded.marker.border_width,
                data_width: decoded.marker.data_width,
                corners: Some(vec![corners[0], corners[1], corners[2], corners[3]]),
                score: Some(score),
                best_distance: Some(decoded.best_distance),
                second_distance: Some(decoded.second_distance),
                border_mismatches: Some(decoded.border_mismatches),
                contrast_range: Some(decoded.contrast_range),
            };

            let verify_min = config.verify_warp_min_best_distance.min(32);
            let should_verify = verify_min <= 32 && decoded.best_distance >= verify_min && (!config.verify_warp_only_if_border_mismatch || decoded.border_mismatches > 0);

            Ok((marker, should_verify))
        })?;

        let mut verified = false;
        if should_verify {
            match decode_quad_warp_result(gray, corners, requested_scale, family, config) {
                Ok(warp_marker) => {
                    if warp_marker.id != marker.id {
                        return Err(SampledDecodeError::VerifyWarpMismatch);
                    }
                    verified = true;
                }
                Err(_) => {
                    if config.verify_warp_reject_on_fail {
                        return Err(SampledDecodeError::VerifyWarpFailed);
                    }
                }
            }
        }

        Ok((marker, verified))
    }

    // Similar to the warp fallback, we historically tried a small corner-shrink sweep here when
    // sampled decoding failed. That can accept detections using modified quad geometry that does
    // not match the reported output corners, which causes corner-placement regressions.
    //
    // Keep only the direct decode path so accepted detections remain corner-consistent.
    let centroid = Point::new((corners[0].x + corners[1].x + corners[2].x + corners[3].x) * 0.25, (corners[0].y + corners[1].y + corners[2].y + corners[3].y) * 0.25);
    // Mirror the warp path: try a small inward shrink sweep.
    let from = [(corners[0].x, corners[0].y), (corners[1].x, corners[1].y), (corners[2].x, corners[2].y), (corners[3].x, corners[3].y)];
    let projection_unit = Projection::from_control_points(from, [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]).ok_or(SampledDecodeError::Projection)?;
    let inv_unit = projection_unit.invert();

    let total_width = family.total_width() as usize;
    let required = total_width.saturating_mul(total_width);
    if required == 0 || required > 100 {
        return Err(SampledDecodeError::InvalidInput);
    }

    let avg_edge = {
        let e0 = distance(&corners[0], &corners[1]);
        let e1 = distance(&corners[1], &corners[2]);
        let e2 = distance(&corners[2], &corners[3]);
        let e3 = distance(&corners[3], &corners[0]);
        (e0 + e1 + e2 + e3) * 0.25
    };
    let cells = family.total_width().max(1) as f32;
    let shrink_factors: &[f32] = &[0.0];
    let mut last_err: Option<SampledDecodeError> = None;
    for &shrink in shrink_factors {
        let mut adjusted = *corners;
        if shrink.abs() > f32::EPSILON {
            for p in &mut adjusted {
                p.x += (centroid.x - p.x) * shrink;
                p.y += (centroid.y - p.y) * shrink;
            }
        }
        let scaled_avg_edge = avg_edge * (1.0 - shrink);
        let sample_scale = adaptive_sample_scale_from_avg_edge(scaled_avg_edge, cells, requested_scale);
        let side = (total_width as u32).saturating_mul(sample_scale).max(1);
        let cell_size = side as f32 / total_width.max(1) as f32;
        let inv = if shrink.abs() <= f32::EPSILON {
            inv_unit
        } else {
            let scale = (1.0 - shrink).max(0.0);
            let adjust = Projection::translate(centroid.x, centroid.y) * Projection::scale(scale, scale) * Projection::translate(-centroid.x, -centroid.y);
            adjust * inv_unit
        };

        match decode_once_with_inv(gray, &inv, &adjusted, requested_scale, total_width, cell_size, required, family, config) {
            Ok((mut marker, verified)) => {
                // Prefer returning the original quad corners for downstream consumers (pose/dedup).
                marker.corners = Some(corners.to_vec());
                return Ok((marker, verified));
            }
            Err(err) => last_err = Some(err),
        }
    }

    Err(last_err.unwrap_or(SampledDecodeError::InvalidInput))
}
