use super::*;

#[derive(Debug, Clone, Copy)]
pub enum DecodeQuadFailure {
    SampledProjection,
    SampledInvalidInput,
    SampledLowContrast,
    SampledBorderMismatch,
    SampledQuietZoneDeltaTooLow,
    SampledBitDeltaTooLow,
    SampledHammingMarginTooLow,
    SampledHammingTooHigh,
    SampledDecodeScoreTooLow,
    SampledVerifyWarpMismatch,
    SampledVerifyWarpFailed,
    WarpProjection,
    WarpLowContrast,
    WarpGrid,
    WarpFamilyDecode,
    WarpLowScore,
}

#[derive(Debug, Clone, Copy)]
pub enum DecodeQuadOutcome {
    DecodedSampled,
    DecodedWarp,
    Failed(DecodeQuadFailure),
}

/// Decode a single quad and return a concise outcome reason for debug comparisons.
pub fn decode_quad_debug(frame: &DynamicImage, corners: &[Point<f32>; 4], sample_scale: u32, family: &ArucoTagFamily, config: &ArucoTagDecodeConfig) -> DecodeQuadOutcome {
    with_luma8_frame(frame, |gray| {
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
                return DecodeQuadOutcome::Failed(DecodeQuadFailure::SampledInvalidInput);
            }
        }

        match decode_quad_sampled(gray, corners, sample_scale, family, config) {
            Ok((_marker, _verified)) => return DecodeQuadOutcome::DecodedSampled,
            Err(err) => {
                let mapped = match err {
                    SampledDecodeError::Projection => DecodeQuadFailure::SampledProjection,
                    SampledDecodeError::InvalidInput => DecodeQuadFailure::SampledInvalidInput,
                    SampledDecodeError::LowContrast => DecodeQuadFailure::SampledLowContrast,
                    SampledDecodeError::BorderMismatch { .. } => DecodeQuadFailure::SampledBorderMismatch,
                    SampledDecodeError::QuietZoneDeltaTooLow => DecodeQuadFailure::SampledQuietZoneDeltaTooLow,
                    SampledDecodeError::BitDeltaTooLow => DecodeQuadFailure::SampledBitDeltaTooLow,
                    SampledDecodeError::HammingMarginTooLow => DecodeQuadFailure::SampledHammingMarginTooLow,
                    SampledDecodeError::HammingTooHigh { .. } => DecodeQuadFailure::SampledHammingTooHigh,
                    SampledDecodeError::DecodeScoreTooLow => DecodeQuadFailure::SampledDecodeScoreTooLow,
                    SampledDecodeError::VerifyWarpMismatch => DecodeQuadFailure::SampledVerifyWarpMismatch,
                    SampledDecodeError::VerifyWarpFailed => DecodeQuadFailure::SampledVerifyWarpFailed,
                };

                if !config.warp_fallback_on_decode_fail {
                    return DecodeQuadOutcome::Failed(mapped);
                }
                if !should_warp_fallback(&err, config) {
                    return DecodeQuadOutcome::Failed(mapped);
                }
            }
        }

        match decode_quad_warp_result(gray, corners, sample_scale, family, config) {
            Ok(_marker) => DecodeQuadOutcome::DecodedWarp,
            Err(err) => {
                let mapped = match err {
                    WarpDecodeError::Projection => DecodeQuadFailure::WarpProjection,
                    WarpDecodeError::LowContrast => DecodeQuadFailure::WarpLowContrast,
                    WarpDecodeError::BorderMismatch | WarpDecodeError::InvalidInput => DecodeQuadFailure::WarpGrid,
                    WarpDecodeError::HammingTooHigh => DecodeQuadFailure::WarpFamilyDecode,
                    WarpDecodeError::LowScore => DecodeQuadFailure::WarpLowScore,
                };
                DecodeQuadOutcome::Failed(mapped)
            }
        }
    })
}
