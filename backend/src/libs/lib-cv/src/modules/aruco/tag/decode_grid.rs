use std::cell::RefCell;

use image::GrayImage;
use imageproc::integral_image::{integral_image, sum_image_pixels};
use tracing::error;

use super::decoding::{ArucoTagDecode, ArucoTagDecoding};

#[derive(Default)]
struct ArucoTagScratch {
    flat: Vec<f32>,
    border_vals: Vec<f32>,
    inner_vals: Vec<f32>,
    candidates: Vec<f32>,
}

thread_local! {
    static ARUCO_TAG_SCRATCH: RefCell<ArucoTagScratch> = RefCell::new(ArucoTagScratch::default());
}

const TAG_SCRATCH_FLAT_RETAIN_CAP: usize = 256;
const TAG_SCRATCH_BORDER_RETAIN_CAP: usize = 128;
const TAG_SCRATCH_INNER_RETAIN_CAP: usize = 128;
const TAG_SCRATCH_CANDIDATE_RETAIN_CAP: usize = 128;

fn trim_retained_vec<T>(vec: &mut Vec<T>, retain_cap: usize) {
    vec.clear();
    if vec.capacity() > retain_cap {
        vec.shrink_to(retain_cap);
    }
}

pub(super) fn compact_tag_scratch_after_frame() {
    ARUCO_TAG_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        trim_retained_vec(&mut scratch.flat, TAG_SCRATCH_FLAT_RETAIN_CAP);
        trim_retained_vec(&mut scratch.border_vals, TAG_SCRATCH_BORDER_RETAIN_CAP);
        trim_retained_vec(&mut scratch.inner_vals, TAG_SCRATCH_INNER_RETAIN_CAP);
        trim_retained_vec(&mut scratch.candidates, TAG_SCRATCH_CANDIDATE_RETAIN_CAP);
    });
}

pub(super) fn hamming_distance(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// Utility function to convert a slice of bits to a u64 value.
/// Returns `None` if the bit slice exceeds 64 bits or contains invalid bits.
fn bits_to_u64(bits: &[u8]) -> Option<u64> {
    if bits.len() > 64 {
        return None;
    }
    bits.iter().try_fold(0u64, |val, &bit| if bit == 0 || bit == 1 { Some((val << 1) | u64::from(bit)) } else { None })
}

fn rotate_grid_90(grid: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let rows = grid.len();
    let cols = grid[0].len();
    let mut rotated = vec![vec![0u8; rows]; cols];

    for (y, row) in grid.iter().enumerate() {
        for (x, &value) in row.iter().enumerate() {
            rotated[x][rows - 1 - y] = value;
        }
    }

    rotated
}

fn is_valid_border(grid: &[Vec<u8>], border_width: u8, error_divisor: u8) -> bool {
    let bw = border_width as usize;
    if bw == 0 {
        return true;
    }
    if grid.is_empty() || grid[0].is_empty() {
        return false;
    }

    let n = grid.len();
    let mut border_cells = 0usize;
    let mut mismatches = 0usize;
    for (y, row) in grid.iter().enumerate() {
        if row.len() != n {
            return false;
        }
        for (x, &v) in row.iter().enumerate() {
            let on_border = x < bw || x + bw >= n || y < bw || y + bw >= n;
            if !on_border {
                continue;
            }
            border_cells += 1;
            if v != 0 {
                mismatches += 1;
            }
        }
    }

    // Warping + thresholding can introduce a small number of border flips; allow a configurable
    // error budget rather than rejecting outright.
    let divisor = error_divisor.max(1) as usize;
    let allowed = (border_cells / divisor).max(1).min(border_cells);
    mismatches <= allowed
}

/// Helper function to remove the border and return the inner data grid
fn remove_border(grid: &[Vec<u8>], border_width: u8) -> Vec<Vec<u8>> {
    let n = grid.len();
    let bw = border_width as usize;
    grid.iter().skip(bw).take(n - 2 * bw).map(|row| row[bw..(n - bw)].to_vec()).collect()
}
pub(super) fn decode(code: &[Vec<u8>], family: &dyn ArucoTagDecoding) -> Option<ArucoTagDecode> {
    let border_width = family.border_size();
    let data_width = family.data_width();
    // Ensure the input grid includes the correct size, accounting for the border
    let expected_size = (data_width + 2 * border_width) as usize;
    if code.len() != expected_size || code[0].len() != expected_size {
        error!("Invalid grid size: expected {}x{}, got {}x{}", expected_size, expected_size, code.len(), code[0].len());
        return None;
    }

    // Verify that the border is solid (all 0s) on all sides
    if !is_valid_border(code, border_width, family.border_error_divisor()) {
        // eprintln!("Invalid border: Border should be solid (all 0s).");
        return None;
    }

    // Remove the border to extract the inner data grid
    let mut data_grid = remove_border(code, border_width);

    let mut min_distance = u32::MAX;
    let mut best_id = None;
    let mut best_rotation = 0;

    // Attempt to decode the grid in all 4 rotations
    for rotation in 0..4 {
        if let Some((id, distance)) = decode_without_rotation(&data_grid, family)
            && distance < min_distance
        {
            min_distance = distance;
            best_id = Some(id);
            best_rotation = rotation;

            if min_distance == 0 {
                break;
            }
        }
        data_grid = rotate_grid_90(&data_grid);
    }

    best_id.map(|id| ArucoTagDecode { id, rotation: best_rotation, border_width, data_width, score: None, best_distance: None, second_distance: None, border_mismatches: None, contrast_range: None })
}

fn decode_without_rotation(code: &[Vec<u8>], family: &dyn ArucoTagDecoding) -> Option<(u32, u32)> {
    // Ensure the inner grid has the correct dimensions (DATA_WIDTH x DATA_WIDTH)

    let data_width = family.data_width();
    let data_bits_location = family.data_bits_location();
    let codes = family.codes();
    let hamming = family.max_hamming_distance();
    // let hamming = 0;

    if code.len() != data_width as usize || code[0].len() != data_width as usize {
        error!("Invalid inner grid size: expected {}x{}, got {}x{}", data_width, data_width, code.len(), code[0].len());
        return None;
    }

    // Extract bits based on the DATA_BITS_LOCATION
    let mut extracted_bits = Vec::new();
    for &(x, y) in data_bits_location.iter() {
        if let Some(row) = code.get(y as usize) {
            if let Some(&bit) = row.get(x as usize) {
                extracted_bits.push(bit);
            } else {
                error!("Invalid bit location: ({}, {})", x, y);
                return None;
            }
        } else {
            error!("Invalid row index: {}", y);
            return None;
        }
    }

    // Convert extracted bits to u64
    let code_u64 = bits_to_u64(&extracted_bits)?;

    // Iterate through the CODES array to find the closest match
    let mut min_distance = u32::MAX;
    let mut best_id = None;

    for (id, &family_code) in codes.iter().enumerate() {
        let distance = hamming_distance(code_u64, family_code);
        if distance < min_distance {
            min_distance = distance;
            best_id = Some(id as u32);
        }
    }

    // Return the ID and the Hamming distance if within the allowable limit
    if min_distance <= hamming as u32 { Some((best_id?, min_distance)) } else { None }
}

pub fn decode_marker_grid(image: &GrayImage, family: &dyn ArucoTagDecoding) -> Option<Vec<Vec<u8>>> {
    let total_width = family.total_width();
    let border = family.border_size();

    if image.width() < total_width as u32 || image.height() < total_width as u32 {
        eprintln!("Image dimensions are smaller than the expected grid size. Expected at least: {}x{}, Got: {}x{}", total_width, total_width, image.width(), image.height());
        return None;
    }

    let cell_width = image.width() as f32 / total_width as f32;
    let cell_height = image.height() as f32 / total_width as f32;

    // Compute cell means with an integral image, then derive a robust global threshold
    // from the distribution of per-cell means.
    //
    // Otsu over the full patch can be unstable for small / low-contrast tags and under
    // heavy illumination gradients, especially after warping + normalization.
    let integral = integral_image::<_, u32>(image);

    let mut means = vec![vec![0.0f32; total_width as usize]; total_width as usize];
    let mut flat = ARUCO_TAG_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        scratch.flat.clear();
        scratch.flat.reserve((total_width as usize) * (total_width as usize));
        std::mem::take(&mut scratch.flat)
    });
    for row in 0..total_width {
        for col in 0..total_width {
            let x_start = (col as f32 * cell_width).floor() as u32;
            let y_start = (row as f32 * cell_height).floor() as u32;
            let x_end = ((col as f32 + 1.0) * cell_width).ceil() as u32;
            let y_end = ((row as f32 + 1.0) * cell_height).ceil() as u32;

            let x_end = x_end.min(image.width()).saturating_sub(1);
            let y_end = y_end.min(image.height()).saturating_sub(1);

            if x_start > x_end || y_start > y_end {
                continue;
            }

            let sum = sum_image_pixels(&integral, x_start, y_start, x_end, y_end)[0];
            let area = (x_end - x_start + 1) * (y_end - y_start + 1);
            if area == 0 {
                continue;
            }
            let mean = sum as f32 / area as f32;
            means[row as usize][col as usize] = mean;
            flat.push(mean);
        }
    }

    if flat.is_empty() {
        ARUCO_TAG_SCRATCH.with(|scratch| {
            let mut scratch = scratch.borrow_mut();
            scratch.flat = flat;
        });
        return None;
    }
    flat.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = flat.len();

    // Robust thresholding on *cell means*.
    //
    // Using raw min/max is extremely sensitive to outliers (e.g. partial occlusion, strong
    // shadows, warped borders) and can introduce bit flips that reduce decode reliability.
    //
    // We combine:
    // - trimmed means at both ends (reduces outlier influence)
    // - a "largest gap" split (works well when means are bimodal)
    let trim = (n / 6).clamp(1, (n / 2).max(1));
    let low_mean = flat.iter().take(trim).copied().sum::<f32>() / trim as f32;
    let high_mean = flat.iter().rev().take(trim).copied().sum::<f32>() / trim as f32;
    let threshold_trimmed = (low_mean + high_mean) * 0.5;

    let mut best_gap = 0.0f32;
    let mut best_gap_threshold = threshold_trimmed;
    if n >= 4 {
        let start = trim.min(n.saturating_sub(2));
        let end = n.saturating_sub(trim).saturating_sub(1);
        if start < end {
            for i in start..end {
                let a = flat[i];
                let b = flat[i + 1];
                let gap = b - a;
                if gap > best_gap {
                    best_gap = gap;
                    best_gap_threshold = (a + b) * 0.5;
                }
            }
        }
    }

    let mut candidates = ARUCO_TAG_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        scratch.candidates.clear();
        scratch.candidates.push(threshold_trimmed);
        std::mem::take(&mut scratch.candidates)
    });
    if best_gap >= 2.0 {
        candidates.push(best_gap_threshold);
    }

    // Leverage the known black border to stabilize thresholding: border cells should be 0.
    // Compute a black reference from the border ring and a white reference from the brightest
    // cells, then include that as a candidate threshold.
    if border > 0 && border * 2 < total_width {
        let (mut border_vals, mut inner_vals) = ARUCO_TAG_SCRATCH.with(|scratch| {
            let mut scratch = scratch.borrow_mut();
            scratch.border_vals.clear();
            scratch.inner_vals.clear();
            (std::mem::take(&mut scratch.border_vals), std::mem::take(&mut scratch.inner_vals))
        });
        for row in 0..total_width {
            for col in 0..total_width {
                let mean = means[row as usize][col as usize];
                let is_border = row < border || col < border || row >= total_width.saturating_sub(border) || col >= total_width.saturating_sub(border);
                if is_border {
                    border_vals.push(mean);
                } else {
                    inner_vals.push(mean);
                }
            }
        }

        if !border_vals.is_empty() && !inner_vals.is_empty() {
            border_vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            inner_vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let k_border = (border_vals.len() / 3).max(1);
            let k_white = (inner_vals.len() / 4).max(1);
            let black_ref = border_vals.iter().take(k_border).copied().sum::<f32>() / k_border as f32;
            let white_ref = inner_vals.iter().rev().take(k_white).copied().sum::<f32>() / k_white as f32;
            candidates.push((black_ref + white_ref) * 0.5);
        }

        ARUCO_TAG_SCRATCH.with(|scratch| {
            let mut scratch = scratch.borrow_mut();
            scratch.border_vals = border_vals;
            scratch.inner_vals = inner_vals;
        });
    }

    let border_total = (0..total_width)
        .flat_map(|row| (0..total_width).map(move |col| (row, col)))
        .filter(|(row, col)| border > 0 && (*row < border || *col < border || *row >= total_width.saturating_sub(border) || *col >= total_width.saturating_sub(border)))
        .count();

    let mut best_threshold = threshold_trimmed;
    let mut best_border_ones = usize::MAX;
    for &t in &candidates {
        let mut ones = 0usize;
        if border_total > 0 {
            for row in 0..total_width {
                for col in 0..total_width {
                    let is_border = row < border || col < border || row >= total_width.saturating_sub(border) || col >= total_width.saturating_sub(border);
                    if !is_border {
                        continue;
                    }
                    if means[row as usize][col as usize] > t {
                        ones += 1;
                    }
                }
            }
        }
        if ones < best_border_ones {
            best_border_ones = ones;
            best_threshold = t;
        }
    }

    // In practice the upstream pipeline should already ensure "black is dark" patches; trying
    // to auto-detect inversion here can cause catastrophic bit flips under glare/shadows, so we
    // keep binarization polarity fixed.
    let invert = false;
    let threshold = best_threshold;

    let mut binary_grid: Vec<Vec<u8>> = Vec::with_capacity(total_width as usize);
    for row in 0..total_width {
        let mut binary_row = Vec::with_capacity(total_width as usize);
        for col in 0..total_width {
            let mean = means[row as usize][col as usize];
            binary_row.push(if invert {
                if mean <= threshold { 1u8 } else { 0u8 }
            } else if mean <= threshold {
                0u8
            } else {
                1u8
            });
        }
        binary_grid.push(binary_row);
    }

    ARUCO_TAG_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        scratch.flat = flat;
        scratch.candidates = candidates;
    });

    Some(binary_grid)
}

/// Decode a marker grid from already-computed per-cell grayscale means (row-major).
///
/// This avoids building an integral image over a full warped patch when the caller can sample
/// each cell directly (e.g., via a homography into the source frame).
pub fn decode_marker_grid_from_cell_means(cell_means: &[f32], family: &dyn ArucoTagDecoding) -> Option<Vec<Vec<u8>>> {
    let total_width = family.total_width() as usize;
    let border = family.border_size() as usize;
    if total_width == 0 {
        return None;
    }
    if cell_means.len() != total_width * total_width {
        return None;
    }

    let mut flat = ARUCO_TAG_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        scratch.flat.clear();
        scratch.flat.extend_from_slice(cell_means);
        std::mem::take(&mut scratch.flat)
    });
    flat.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    if flat.is_empty() {
        ARUCO_TAG_SCRATCH.with(|scratch| {
            let mut scratch = scratch.borrow_mut();
            scratch.flat = flat;
        });
        return None;
    }
    let n = flat.len();

    let min_val = *flat.first().unwrap_or(&0.0);
    let max_val = *flat.last().unwrap_or(&0.0);
    if !min_val.is_finite() || !max_val.is_finite() {
        ARUCO_TAG_SCRATCH.with(|scratch| {
            let mut scratch = scratch.borrow_mut();
            scratch.flat = flat;
        });
        return None;
    }
    // Early reject: extremely low contrast across cell samples leads to random bit flips.
    if max_val - min_val < 20.0 {
        ARUCO_TAG_SCRATCH.with(|scratch| {
            let mut scratch = scratch.borrow_mut();
            scratch.flat = flat;
        });
        return None;
    }

    let trim = (n / 6).clamp(1, (n / 2).max(1));
    let low_mean = flat.iter().take(trim).copied().sum::<f32>() / trim as f32;
    let high_mean = flat.iter().rev().take(trim).copied().sum::<f32>() / trim as f32;
    let threshold_trimmed = (low_mean + high_mean) * 0.5;

    let mut best_gap = 0.0f32;
    let mut best_gap_threshold = threshold_trimmed;
    if n >= 4 {
        let start = trim.min(n.saturating_sub(2));
        let end = n.saturating_sub(trim).saturating_sub(1);
        if start < end {
            for i in start..end {
                let a = flat[i];
                let b = flat[i + 1];
                let gap = b - a;
                if gap > best_gap {
                    best_gap = gap;
                    best_gap_threshold = (a + b) * 0.5;
                }
            }
        }
    }

    let mut candidates = ARUCO_TAG_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        scratch.candidates.clear();
        scratch.candidates.push(threshold_trimmed);
        std::mem::take(&mut scratch.candidates)
    });
    if best_gap >= 2.0 {
        candidates.push(best_gap_threshold);
    }

    // Leverage the known black border: compute a black ref from the border ring and a white ref
    // from the brightest inner cells, then include that as a candidate threshold.
    if border > 0 && border * 2 < total_width {
        let (mut border_vals, mut inner_vals) = ARUCO_TAG_SCRATCH.with(|scratch| {
            let mut scratch = scratch.borrow_mut();
            scratch.border_vals.clear();
            scratch.inner_vals.clear();
            (std::mem::take(&mut scratch.border_vals), std::mem::take(&mut scratch.inner_vals))
        });
        for row in 0..total_width {
            for col in 0..total_width {
                let mean = cell_means[row * total_width + col];
                let is_border = row < border || col < border || row + border >= total_width || col + border >= total_width;
                if is_border {
                    border_vals.push(mean);
                } else {
                    inner_vals.push(mean);
                }
            }
        }

        if !border_vals.is_empty() && !inner_vals.is_empty() {
            border_vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            inner_vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let k_border = (border_vals.len() / 3).max(1);
            let k_white = (inner_vals.len() / 4).max(1);
            let black_ref = border_vals.iter().take(k_border).copied().sum::<f32>() / k_border as f32;
            let white_ref = inner_vals.iter().rev().take(k_white).copied().sum::<f32>() / k_white as f32;
            candidates.push((black_ref + white_ref) * 0.5);
        }

        ARUCO_TAG_SCRATCH.with(|scratch| {
            let mut scratch = scratch.borrow_mut();
            scratch.border_vals = border_vals;
            scratch.inner_vals = inner_vals;
        });
    }

    let border_total = if border > 0 {
        (0..total_width)
            .flat_map(|row| (0..total_width).map(move |col| (row, col)))
            .filter(|(row, col)| *row < border || *col < border || *row + border >= total_width || *col + border >= total_width)
            .count()
    } else {
        0
    };

    let mut best_threshold = threshold_trimmed;
    let mut best_border_ones = usize::MAX;
    for &t in &candidates {
        let mut ones = 0usize;
        if border_total > 0 {
            for row in 0..total_width {
                for col in 0..total_width {
                    let is_border = row < border || col < border || row + border >= total_width || col + border >= total_width;
                    if !is_border {
                        continue;
                    }
                    if cell_means[row * total_width + col] > t {
                        ones += 1;
                    }
                }
            }
        }
        if ones < best_border_ones {
            best_border_ones = ones;
            best_threshold = t;
        }
    }

    let threshold = best_threshold;
    let mut binary_grid: Vec<Vec<u8>> = Vec::with_capacity(total_width);
    for row in 0..total_width {
        let mut binary_row = Vec::with_capacity(total_width);
        for col in 0..total_width {
            let mean = cell_means[row * total_width + col];
            binary_row.push(if mean <= threshold { 0u8 } else { 1u8 });
        }
        binary_grid.push(binary_row);
    }
    ARUCO_TAG_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        scratch.flat = flat;
        scratch.candidates = candidates;
    });
    Some(binary_grid)
}
