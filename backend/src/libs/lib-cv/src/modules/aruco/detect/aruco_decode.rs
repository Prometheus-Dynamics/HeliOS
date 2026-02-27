use super::*;
use std::cell::RefCell;
use std::collections::HashMap;

pub(super) struct ArucoDecodeSummary {
    pub(super) id: u32,
    pub(super) rotation: u8,
    pub(super) best_distance: u32,
    pub(super) second_distance: u32,
    pub(super) border_mismatches: usize,
    pub(super) contrast_range: f32,
}

fn hamming_distance_bytes(a: &[u8], b: &[u8]) -> u32 {
    a.iter().zip(b.iter()).map(|(x, y)| (*x ^ *y).count_ones()).sum()
}

#[derive(Clone)]
struct PackedDictionary {
    nbytes: usize,
    codes: Vec<u64>,
}

thread_local! {
    static PACKED_DICT_CACHE: RefCell<HashMap<&'static str, PackedDictionary>> = RefCell::new(HashMap::new());
}

#[inline]
fn pack_bytes_to_u64(bytes: &[u8]) -> u64 {
    let mut out = 0u64;
    for &b in bytes {
        out = (out << 8) | (b as u64);
    }
    out
}

fn with_packed_dictionary<R>(dict: &ArucoDictionary, f: impl FnOnce(&PackedDictionary) -> R) -> R {
    PACKED_DICT_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let entry = cache.entry(dict.name()).or_insert_with(|| {
            let nbytes = dict.nbytes().min(8);
            let mut codes = Vec::with_capacity(dict.marker_count());
            for id in 0..dict.marker_count() {
                let packed = dict.code(id).map(|code| pack_bytes_to_u64(&code[..nbytes])).unwrap_or(0);
                codes.push(packed);
            }
            PackedDictionary { nbytes, codes }
        });
        f(entry)
    })
}

fn build_rotated_bytes(bits: &[u8], size: usize, rotation: u8, out: &mut [u8]) {
    out.fill(0);
    let nbits = size * size;
    for idx in 0..nbits {
        let x = idx % size;
        let y = idx / size;
        let (rx, ry) = match rotation & 3 {
            1 => (size - 1 - y, x),
            2 => (size - 1 - x, size - 1 - y),
            3 => (y, size - 1 - x),
            _ => (x, y),
        };
        let bit = bits[ry * size + rx];
        if bit == 0 {
            continue;
        }
        let byte_idx = idx / 8;
        let shift = 7 - (idx % 8);
        if byte_idx < out.len() {
            out[byte_idx] |= 1u8 << shift;
        }
    }
}

fn decode_aruco_from_cell_means(cell_means: &[f32], total_width: usize, border_bits: usize, dict: &ArucoDictionary, cfg: &ArucoDecodeConfig) -> Option<ArucoDecodeSummary> {
    if total_width == 0 || cell_means.len() != total_width * total_width {
        return None;
    }
    let inner = total_width.saturating_sub(border_bits * 2);
    if inner == 0 {
        return None;
    }
    let data_width = dict.marker_size() as usize;
    if inner != data_width {
        return None;
    }

    let n = cell_means.len();
    if n > 100 {
        return None;
    }

    let mut flat = [0.0f32; 100];
    for (dst, &src) in flat.iter_mut().zip(cell_means.iter()) {
        *dst = src;
    }
    let flat = &mut flat[..n];
    flat.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let min_val = *flat.first().unwrap_or(&0.0);
    let max_val = *flat.last().unwrap_or(&0.0);
    if !min_val.is_finite() || !max_val.is_finite() {
        return None;
    }

    let trim = (n / 6).clamp(1, (n / 2).max(1));
    let low_mean = flat.iter().take(trim).copied().sum::<f32>() / trim as f32;
    let high_mean = flat.iter().rev().take(trim).copied().sum::<f32>() / trim as f32;
    if !low_mean.is_finite() || !high_mean.is_finite() {
        return None;
    }
    let contrast_range = high_mean - low_mean;
    if contrast_range < cfg.min_cell_means_contrast_range.max(0.0) {
        return None;
    }
    let threshold = (low_mean + high_mean) * 0.5;

    let mut border_errors = 0usize;
    if data_width.saturating_mul(data_width) > 64 {
        return None;
    }
    let mut inner_bits = [0u8; 64];
    let mut inner_len = 0usize;
    let mut min_delta = f32::INFINITY;
    let mut ones = 0usize;
    for row in 0..total_width {
        for col in 0..total_width {
            let idx = row * total_width + col;
            let bit = if cell_means[idx] <= threshold { 0u8 } else { 1u8 };
            let on_border = row < border_bits || col < border_bits || row + border_bits >= total_width || col + border_bits >= total_width;
            if on_border {
                if bit != 0 {
                    border_errors += 1;
                }
            } else {
                inner_bits[inner_len] = bit;
                inner_len += 1;
                if bit != 0 {
                    ones += 1;
                }
                let delta = (cell_means[idx] - threshold).abs();
                if delta.is_finite() && delta < min_delta {
                    min_delta = delta;
                }
            }
        }
    }

    let max_border = ((data_width * data_width) as f32 * cfg.max_border_error_rate.max(0.0)).floor() as usize;
    if border_errors > max_border {
        return None;
    }
    if inner_len == 0 || ones == 0 || ones == inner_len {
        return None;
    }
    if dict.marker_size() == 4 && inner_len == 16 {
        // Dense ChArUco boards produce a lot of high-contrast non-marker quads (e.g. chessboard
        // squares). Some of those can accidentally decode to a valid 4x4 code when sampling bleeds
        // in background pixels. Real 4x4_1000 marker codes used by our calibration boards are not
        // near-uniform, so reject extreme bit balances to suppress these false positives.
        if !(4..=12).contains(&ones) {
            return None;
        }
    }
    if cfg.min_bit_delta > 0.0 && min_delta.is_finite() && min_delta < cfg.min_bit_delta {
        return None;
    }

    let nbytes = dict.nbytes();
    let mut rotated = [[0u8; 8]; 4];
    for (rot, bytes) in rotated.iter_mut().enumerate() {
        build_rotated_bytes(&inner_bits[..inner_len], data_width, rot as u8, &mut bytes[..nbytes]);
    }

    let max_correction = ((dict.max_correction_bits() as f32) * cfg.error_correction_rate.max(0.0)).floor() as u32;
    let mut best_distance = u32::MAX;
    let mut second_distance = u32::MAX;
    let mut best_id = None;
    let mut best_rot = 0u8;
    let mut used_packed = false;
    if nbytes <= 8 {
        let mut rotated_packed = [0u64; 4];
        for (i, bytes) in rotated.iter().enumerate() {
            rotated_packed[i] = pack_bytes_to_u64(&bytes[..nbytes]);
        }
        with_packed_dictionary(dict, |packed| {
            if packed.nbytes != nbytes {
                return;
            }
            used_packed = true;
            for (id, &code) in packed.codes.iter().enumerate() {
                for (rot, probe) in rotated_packed.iter().enumerate() {
                    let dist = (probe ^ code).count_ones();
                    if dist < best_distance {
                        second_distance = best_distance;
                        best_distance = dist;
                        best_id = Some(id);
                        best_rot = rot as u8;
                    } else if dist < second_distance {
                        second_distance = dist;
                    }
                }
            }
        });
    }
    if !used_packed {
        for id in 0..dict.marker_count() {
            let Some(code) = dict.code(id) else { continue };
            for (rot, bytes) in rotated.iter().enumerate() {
                let dist = hamming_distance_bytes(&bytes[..nbytes], code);
                if dist < best_distance {
                    second_distance = best_distance;
                    best_distance = dist;
                    best_id = Some(id);
                    best_rot = rot as u8;
                } else if dist < second_distance {
                    second_distance = dist;
                }
            }
        }
    }

    let best_id = best_id?;
    if best_distance > max_correction {
        return None;
    }
    if second_distance == u32::MAX {
        second_distance = best_distance;
    }
    let margin = second_distance.saturating_sub(best_distance);
    if cfg.min_hamming_margin > 0 && best_distance >= cfg.min_hamming_margin_min_dist && (!cfg.min_hamming_margin_only_if_border_mismatch || border_errors > 0) && margin < cfg.min_hamming_margin {
        return None;
    }
    if cfg.min_decode_score > 0.0 {
        let score = (margin as f32) * 10.0 + contrast_range - (border_errors as f32) * 2.0;
        if score < cfg.min_decode_score {
            return None;
        }
    }

    // Match the AprilTag-family rotation convention used across the rest of the stack:
    // `rotation` is the code rotation (how many 90-degree rotations are required to align the
    // sampled grid with the dictionary's canonical orientation).
    let rotation = (4u8.wrapping_sub(best_rot & 3)) & 3;
    Some(ArucoDecodeSummary { id: best_id as u32, rotation, best_distance, second_distance, border_mismatches: border_errors, contrast_range })
}

pub(super) fn decode_aruco_from_warped(warped: &mut GrayImage, dict: &ArucoDictionary, cfg: &ArucoDecodeConfig, total_width: usize) -> Option<ArucoDecodeSummary> {
    if !normalize_warped_patch(warped, cfg.min_warped_patch_contrast_range) {
        return None;
    }
    let side = warped.width() as usize;
    if side == 0 || warped.width() != warped.height() {
        return None;
    }
    let cell_px = (side / total_width).max(1);
    let margin_ratio = cfg.cell_sample_margin.clamp(0.0, 0.45);
    let margin_px = ((cell_px as f32) * margin_ratio).round() as usize;
    let margin = margin_px.min(cell_px / 2);
    let span = cell_px.saturating_sub(margin * 2).max(1);
    let inv_area = 1.0f32 / ((span.saturating_mul(span)) as f32);

    let buf = warped.as_raw();
    let mut cell_means = [0.0f32; 100];
    let mut idx = 0usize;
    for row in 0..total_width {
        for col in 0..total_width {
            let x0 = col * cell_px;
            let y0 = row * cell_px;
            let xs = x0 + margin;
            let xe = (x0 + cell_px).saturating_sub(margin).max(x0 + 1);
            let ys = y0 + margin;
            let ye = (y0 + cell_px).saturating_sub(margin).max(y0 + 1);
            let mut sum = 0u32;
            for yy in ys..ye {
                let base = yy * side;
                for xx in xs..xe {
                    sum += buf[base + xx] as u32;
                }
            }
            if idx < cell_means.len() {
                cell_means[idx] = sum as f32 * inv_area;
                idx += 1;
            }
        }
    }
    let required = total_width.saturating_mul(total_width);
    decode_aruco_from_cell_means(&cell_means[..required], total_width, 1, dict, cfg)
}
