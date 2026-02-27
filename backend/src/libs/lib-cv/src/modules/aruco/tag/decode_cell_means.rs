#![allow(unsafe_code)]

use super::decode_grid::hamming_distance;
use super::decoding::{ArucoTagDecode, ArucoTagDecoding};
use super::family::ArucoTagDecodeTuning;

#[cfg(target_arch = "aarch64")]
use core::arch::aarch64::*;

/// Decode an ArUco tag directly from per-cell means (row-major), without constructing an
/// intermediate `Vec<Vec<u8>>` grid.
///
/// This is intended for high-throughput pipelines where the caller samples each cell directly
/// from the source image. It avoids per-quad heap allocations and a full patch warp.
pub fn decode_from_cell_means(cell_means: &[f32], family: &dyn ArucoTagDecoding) -> Option<ArucoTagDecode> {
    decode_from_cell_means_result(cell_means, family).ok()
}

#[derive(Clone)]
pub struct DecodeFromCellMeansOk {
    pub marker: ArucoTagDecode,
    pub best_distance: u32,
    pub second_distance: u32,
    pub border_mismatches: usize,
    pub threshold: f32,
    pub contrast_range: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeFromCellMeansError {
    InvalidInput,
    LowContrast,
    BorderMismatch { mismatches: usize, allowed: usize },
    BitDeltaTooLow { min_delta: u8, required: u8 },
    HammingMarginTooLow { best_distance: u32, second_distance: u32, min_margin: u32 },
    HammingTooHigh { best_distance: u32, max_hamming: u32 },
}

pub fn decode_from_cell_means_result_detailed(cell_means: &[f32], family: &dyn ArucoTagDecoding) -> Result<DecodeFromCellMeansOk, DecodeFromCellMeansError> {
    decode_from_cell_means_result_detailed_with_tuning(cell_means, family, &ArucoTagDecodeTuning::default())
}

pub fn decode_from_cell_means_result(cell_means: &[f32], family: &dyn ArucoTagDecoding) -> Result<ArucoTagDecode, DecodeFromCellMeansError> {
    decode_from_cell_means_result_detailed_with_tuning(cell_means, family, &ArucoTagDecodeTuning::default()).map(|out| out.marker)
}

pub fn decode_from_cell_means_result_detailed_with_tuning(cell_means: &[f32], family: &dyn ArucoTagDecoding, tuning: &ArucoTagDecodeTuning) -> Result<DecodeFromCellMeansOk, DecodeFromCellMeansError> {
    decode_from_cell_means_result_detailed_impl(cell_means, family, tuning)
}

fn decode_from_cell_means_result_detailed_impl(cell_means: &[f32], family: &dyn ArucoTagDecoding, tuning: &ArucoTagDecodeTuning) -> Result<DecodeFromCellMeansOk, DecodeFromCellMeansError> {
    let total_width = family.total_width() as usize;
    let border_width = family.border_size() as usize;
    let data_width = family.data_width() as usize;

    if total_width == 0 || cell_means.len() != total_width * total_width {
        return Err(DecodeFromCellMeansError::InvalidInput);
    }
    if border_width * 2 + data_width != total_width {
        // Family shape mismatch.
        return Err(DecodeFromCellMeansError::InvalidInput);
    }

    let n = cell_means.len();
    if n > 100 {
        // Current built-in families are <= 9x9; keep this bounded for stack storage.
        return Err(DecodeFromCellMeansError::InvalidInput);
    }

    // Copy into a small fixed buffer so we can sort without allocating.
    let mut flat = [0.0f32; 100];
    for (dst, &src) in flat.iter_mut().zip(cell_means.iter()) {
        *dst = src;
    }
    let flat = &mut flat[..n];
    flat.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let min_val = *flat.first().unwrap_or(&0.0);
    let max_val = *flat.last().unwrap_or(&0.0);
    if !min_val.is_finite() || !max_val.is_finite() {
        return Err(DecodeFromCellMeansError::InvalidInput);
    }
    let contrast_range = max_val - min_val;
    let min_range = tuning.min_cell_means_contrast_range.clamp(0.0, 255.0);
    if contrast_range < min_range {
        return Err(DecodeFromCellMeansError::LowContrast);
    }

    let trim = (n / 6).clamp(1, (n / 2).max(1));
    let low_mean = flat.iter().take(trim).copied().sum::<f32>() / trim as f32;
    let high_mean = flat.iter().rev().take(trim).copied().sum::<f32>() / trim as f32;
    let threshold_trimmed = (low_mean + high_mean) * 0.5;

    let candidates = [threshold_trimmed];
    let candidate_count = 1usize;

    let mut border_indices = [0usize; 100];
    let mut border_total = 0usize;
    if border_width > 0 {
        for row in 0..total_width {
            for col in 0..total_width {
                let on_border = row < border_width || col < border_width || row + border_width >= total_width || col + border_width >= total_width;
                if on_border {
                    border_indices[border_total] = row * total_width + col;
                    border_total += 1;
                }
            }
        }
    }
    let border_indices = &border_indices[..border_total];

    // Try multiple plausible thresholds and keep the best decode result.
    //
    // Minimizing "border ones" is a good first approximation, but under glare/blur a slightly
    // shifted threshold can flip a handful of bits and turn an otherwise-failing candidate into a
    // clean decode. Since we already sampled all cell means, testing a small number of threshold
    // variants is cheap compared to re-warping/re-sampling.
    let codes = family.codes();
    let max_hamming = family.max_hamming_distance() as u32;
    let data_bits_location = family.data_bits_location();
    let dw = data_width as i32;
    if dw <= 0 {
        return Err(DecodeFromCellMeansError::InvalidInput);
    }

    let border_cells = border_total;
    let divisor = family.border_error_divisor().max(1) as usize;
    let allowed = if border_cells > 0 { (border_cells / divisor).min(border_cells) } else { 0 };

    let nudges: [f32; 1] = [0.0];

    let mut best_marker: Option<ArucoTagDecode> = None;
    let mut best_distance = u32::MAX;
    let mut best_second_distance = u32::MAX;
    let mut best_threshold = 0.0f32;
    let mut best_border_mismatches_for_marker = 0usize;
    let mut best_hamming_fail = u32::MAX;
    let mut best_margin_fail: Option<(u32, u32)> = None;
    let mut saw_border_ok = false;
    let mut best_border_mismatches = usize::MAX;
    let mut bit_buf = [0u8; 100];

    for &base in &candidates[..candidate_count] {
        for &delta in &nudges {
            let threshold = (base + delta).clamp(min_val, max_val);
            let bits = &mut bit_buf[..n];
            #[cfg(target_arch = "aarch64")]
            let use_neon = crate::simd::neon_enabled();
            #[cfg(not(target_arch = "aarch64"))]
            let use_neon = false;
            if use_neon {
                threshold_bits_neon_checked(cell_means, threshold, bits);
            } else {
                for (out, &mean) in bits.iter_mut().zip(cell_means.iter()) {
                    *out = if mean <= threshold { 0 } else { 1 };
                }
            }
            let mut mismatches_for_threshold = 0usize;

            // Border validation in bit-space (same as `is_valid_border`).
            if border_width > 0 {
                let mut mismatches = 0usize;
                for &idx in border_indices {
                    if bits[idx] != 0 {
                        mismatches += 1;
                        if mismatches > best_border_mismatches {
                            // Can't beat the best "closest-to-valid" border seen so far.
                            break;
                        }
                    }
                }
                best_border_mismatches = best_border_mismatches.min(mismatches);
                mismatches_for_threshold = mismatches;
                if mismatches > allowed {
                    continue;
                }
            }

            saw_border_ok = true;

            // Decode by scanning rotations (no allocation / grid rotation).
            let mut threshold_best_id: Option<u32> = None;
            let mut threshold_best_rotation: u8 = 0;
            let mut threshold_best_distance = u32::MAX;
            let mut threshold_best_second_distance = u32::MAX;

            for rot in 0..4u8 {
                // Extract data bits in the family-defined order.
                let mut code_u64 = 0u64;
                for &(x, y) in data_bits_location {
                    let x = x as i32;
                    let y = y as i32;
                    if x < 0 || y < 0 || x >= dw || y >= dw {
                        return Err(DecodeFromCellMeansError::InvalidInput);
                    }
                    let (rx, ry) = match rot {
                        0 => (x, y),
                        // Rotate grid clockwise (matches `rotate_grid_90` used in the warp decode path).
                        1 => (y, dw - 1 - x),
                        2 => (dw - 1 - x, dw - 1 - y),
                        _ => (dw - 1 - y, x),
                    };
                    let gx = (border_width as i32 + rx) as usize;
                    let gy = (border_width as i32 + ry) as usize;
                    let bit = bits[gy * total_width + gx] as u64;
                    code_u64 = (code_u64 << 1) | bit;
                }

                let mut min_distance = u32::MAX;
                let mut second_distance = u32::MAX;
                let mut rot_best_id: Option<u32> = None;
                for (id, &family_code) in codes.iter().enumerate() {
                    let d = hamming_distance(code_u64, family_code);
                    if d < min_distance {
                        second_distance = min_distance;
                        min_distance = d;
                        rot_best_id = Some(id as u32);
                        if d == 0 {
                            break;
                        }
                    } else if d < second_distance {
                        second_distance = d;
                    }
                }

                let better = min_distance < threshold_best_distance || (min_distance == threshold_best_distance && second_distance > threshold_best_second_distance);
                if better {
                    threshold_best_distance = min_distance;
                    threshold_best_second_distance = second_distance;
                    threshold_best_id = rot_best_id;
                    threshold_best_rotation = rot;
                    if threshold_best_distance == 0 {
                        break;
                    }
                }
            }

            best_hamming_fail = best_hamming_fail.min(threshold_best_distance);

            let min_margin = tuning.min_hamming_margin.min(32);
            let min_margin_dist = tuning.min_hamming_margin_min_dist.min(32);
            if threshold_best_distance <= max_hamming {
                if min_margin > 0 && threshold_best_distance >= min_margin_dist {
                    let only_if_border_mismatch = tuning.min_hamming_margin_only_if_border_mismatch;
                    if !only_if_border_mismatch || mismatches_for_threshold > 0 {
                        let margin = threshold_best_second_distance.saturating_sub(threshold_best_distance);
                        if margin < min_margin {
                            best_margin_fail = match best_margin_fail {
                                Some((best_d, best_second)) => {
                                    let better = threshold_best_distance < best_d || (threshold_best_distance == best_d && threshold_best_second_distance > best_second);
                                    if better { Some((threshold_best_distance, threshold_best_second_distance)) } else { Some((best_d, best_second)) }
                                }
                                None => Some((threshold_best_distance, threshold_best_second_distance)),
                            };
                            continue;
                        }
                    }
                }

                let Some(id) = threshold_best_id else {
                    return Err(DecodeFromCellMeansError::InvalidInput);
                };

                let better_than_best = threshold_best_distance < best_distance || (threshold_best_distance == best_distance && threshold_best_second_distance > best_second_distance);
                if better_than_best {
                    best_distance = threshold_best_distance;
                    best_second_distance = threshold_best_second_distance;
                    best_threshold = threshold;
                    best_border_mismatches_for_marker = mismatches_for_threshold;
                    best_marker = Some(ArucoTagDecode {
                        id,
                        rotation: threshold_best_rotation,
                        border_width: border_width as u8,
                        data_width: data_width as u8,
                        score: None,
                        best_distance: None,
                        second_distance: None,
                        border_mismatches: None,
                        contrast_range: None,
                    });
                }
            }

            if best_distance == 0 {
                break;
            }
        }
        if best_distance == 0 {
            break;
        }
    }

    if let Some(marker) = best_marker {
        let min_bit_delta = tuning.min_bit_delta.clamp(0.0, 255.0);
        if min_bit_delta > 0.0 {
            let threshold = best_threshold;

            // Use a percentile of per-bit deltas (instead of a strict minimum) so a single blurred
            // cell doesn't invalidate an otherwise-good decode.
            let mut deltas = [0.0f32; 64];
            let mut nd = 0usize;

            for &(x, y) in data_bits_location {
                if nd >= deltas.len() {
                    break;
                }
                let x = x as i32;
                let y = y as i32;
                let (rx, ry) = match marker.rotation {
                    0 => (x, y),
                    1 => (y, dw - 1 - x),
                    2 => (dw - 1 - x, dw - 1 - y),
                    _ => (dw - 1 - y, x),
                };
                if rx < 0 || ry < 0 || rx >= dw || ry >= dw {
                    return Err(DecodeFromCellMeansError::InvalidInput);
                }
                let gx = (border_width as i32 + rx) as usize;
                let gy = (border_width as i32 + ry) as usize;
                let mean = cell_means[gy * total_width + gx];
                deltas[nd] = (mean - threshold).abs();
                nd += 1;
            }

            if nd >= 4 {
                let deltas = &mut deltas[..nd];
                deltas.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let p50 = deltas[nd / 2];
                let required = min_bit_delta.round().clamp(0.0, 255.0) as u8;
                if !p50.is_finite() || p50 < required as f32 {
                    return Err(DecodeFromCellMeansError::BitDeltaTooLow { min_delta: p50.round().clamp(0.0, 255.0) as u8, required });
                }
            }
        }

        return Ok(DecodeFromCellMeansOk {
            marker,
            best_distance,
            second_distance: best_second_distance,
            border_mismatches: best_border_mismatches_for_marker,
            threshold: best_threshold,
            contrast_range,
        });
    }

    if saw_border_ok {
        if let Some((best_d, second_d)) = best_margin_fail
            && best_d <= max_hamming
        {
            return Err(DecodeFromCellMeansError::HammingMarginTooLow { best_distance: best_d, second_distance: second_d, min_margin: tuning.min_hamming_margin.min(32) });
        }
        return Err(DecodeFromCellMeansError::HammingTooHigh { best_distance: best_hamming_fail, max_hamming });
    }

    Err(DecodeFromCellMeansError::BorderMismatch { mismatches: best_border_mismatches.min(border_cells), allowed })
}

#[cfg(target_arch = "aarch64")]
#[allow(unsafe_op_in_unsafe_fn)]
#[target_feature(enable = "neon")]
unsafe fn threshold_bits_neon(cell_means: &[f32], threshold: f32, out: &mut [u8]) {
    debug_assert_eq!(cell_means.len(), out.len());
    let n = cell_means.len();
    let thr = vdupq_n_f32(threshold);
    let mut i = 0usize;

    while i + 8 <= n {
        let v0 = vld1q_f32(cell_means.as_ptr().add(i));
        let v1 = vld1q_f32(cell_means.as_ptr().add(i + 4));
        let m0 = vcgtq_f32(v0, thr);
        let m1 = vcgtq_f32(v1, thr);
        let b0 = vshrq_n_u32(m0, 31);
        let b1 = vshrq_n_u32(m1, 31);
        let b0_u16 = vqmovn_u32(b0);
        let b1_u16 = vqmovn_u32(b1);
        let packed_u16 = vcombine_u16(b0_u16, b1_u16);
        let packed_u8 = vmovn_u16(packed_u16);
        vst1_u8(out.as_mut_ptr().add(i), packed_u8);
        i += 8;
    }

    for idx in i..n {
        out[idx] = if cell_means[idx] <= threshold { 0 } else { 1 };
    }
}

#[cfg(target_arch = "aarch64")]
#[allow(unsafe_code)]
#[inline]
fn threshold_bits_neon_checked(cell_means: &[f32], threshold: f32, out: &mut [u8]) {
    // SAFETY: caller validates NEON availability and buffer sizes.
    unsafe { threshold_bits_neon(cell_means, threshold, out) }
}

#[cfg(not(target_arch = "aarch64"))]
#[inline]
fn threshold_bits_neon_checked(cell_means: &[f32], threshold: f32, out: &mut [u8]) {
    for (dst, &mean) in out.iter_mut().zip(cell_means.iter()) {
        *dst = if mean <= threshold { 0 } else { 1 };
    }
}

#[cfg(test)]
mod tests {
    use super::super::ArucoTagFamily;
    use super::*;
    use image::DynamicImage;
    use image::GrayImage;
    use imageproc::point::Point as CvPoint;

    fn rotate90_cw(img: &GrayImage) -> GrayImage {
        let w = img.width();
        let h = img.height();
        let mut out = GrayImage::new(h, w);
        for y in 0..h {
            for x in 0..w {
                let px = *img.get_pixel(x, y);
                out.put_pixel(h - 1 - y, x, px);
            }
        }
        out
    }

    fn rotate180(img: &GrayImage) -> GrayImage {
        let w = img.width();
        let h = img.height();
        let mut out = GrayImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let px = *img.get_pixel(x, y);
                out.put_pixel(w - 1 - x, h - 1 - y, px);
            }
        }
        out
    }

    fn rotate270_cw(img: &GrayImage) -> GrayImage {
        let w = img.width();
        let h = img.height();
        let mut out = GrayImage::new(h, w);
        for y in 0..h {
            for x in 0..w {
                let px = *img.get_pixel(x, y);
                out.put_pixel(y, w - 1 - x, px);
            }
        }
        out
    }

    #[test]
    fn self_generated_family16h5_markers_roundtrip() {
        let family = ArucoTagFamily::new_family16h5();
        let side_px = 96u32;
        let markers = family.generate_marker_set(side_px);
        assert!(!markers.is_empty());

        for (expected_id, img) in markers.into_iter().take(8) {
            let frame = DynamicImage::ImageLuma8(img);
            let w = frame.width() as f32;
            let h = frame.height() as f32;
            let quad = [CvPoint::new(0.0, 0.0), CvPoint::new(w - 1.0, 0.0), CvPoint::new(w - 1.0, h - 1.0), CvPoint::new(0.0, h - 1.0)];
            let decoded = crate::modules::aruco::detect::decode_quads(&frame, &[quad], 1, &family);
            assert_eq!(decoded.len(), 1, "no detection for marker id {expected_id}");
            assert_eq!(decoded[0].id, expected_id as u32, "wrong decoded id for marker id {expected_id}");
        }
    }

    #[test]
    fn self_generated_family16h5_markers_roundtrip_rotations() {
        let family = ArucoTagFamily::new_family16h5();
        let side_px = 96u32;
        let markers = family.generate_marker_set(side_px);
        assert!(!markers.is_empty());

        for (expected_id, img) in markers.into_iter().take(8) {
            let variants: [(u8, GrayImage); 4] = [(0, img.clone()), (1, rotate90_cw(&img)), (2, rotate180(&img)), (3, rotate270_cw(&img))];

            for (rot, rotated) in variants {
                let frame = DynamicImage::ImageLuma8(rotated);
                let w = frame.width() as f32;
                let h = frame.height() as f32;
                let quad = [CvPoint::new(0.0, 0.0), CvPoint::new(w - 1.0, 0.0), CvPoint::new(w - 1.0, h - 1.0), CvPoint::new(0.0, h - 1.0)];
                let decoded = crate::modules::aruco::detect::decode_quads(&frame, &[quad], 1, &family);
                assert_eq!(decoded.len(), 1, "no detection for marker id {expected_id} (img rot {rot})");
                assert_eq!(decoded[0].id, expected_id as u32, "wrong decoded id for marker id {expected_id} (img rot {rot})");

                let expected_rotation = (4 - (rot & 3)) & 3;
                assert_eq!(decoded[0].rotation, expected_rotation, "unexpected code rotation for marker id {expected_id} (img rot {rot})");
            }
        }
    }
}
