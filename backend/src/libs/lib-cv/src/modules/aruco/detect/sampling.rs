#![allow(unsafe_code)]

use super::*;

fn unchecked_sampling_enabled() -> bool {
    static ENABLED: Lazy<bool> = Lazy::new(|| std::env::var("HELIOS_CV_UNCHECKED_SAMPLING").map(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")).unwrap_or(false));
    *ENABLED
}

#[inline(always)]
pub(super) fn sample_bilinear_gray_raw(buf: &[u8], width: i32, height: i32, stride: usize, x: f32, y: f32) -> u8 {
    if !x.is_finite() || !y.is_finite() {
        return 0;
    }
    if width <= 1 || height <= 1 {
        return 0;
    }

    let max_x = (width - 1) as f32;
    let max_y = (height - 1) as f32;
    if x < 0.0 || y < 0.0 || x >= max_x || y >= max_y {
        return 0;
    }

    let x0 = x as i32;
    let y0 = y as i32;
    let x1 = x0 + 1;
    let y1 = y0 + 1;
    let fx = x - x0 as f32;
    let fy = y - y0 as f32;

    let x0 = x0 as usize;
    let x1 = x1 as usize;
    let y0 = y0 as usize;
    let y1 = y1 as usize;

    let row0 = y0 * stride;
    let row1 = y1 * stride;
    let (p00, p10, p01, p11) = unsafe { (*buf.get_unchecked(row0 + x0) as f32, *buf.get_unchecked(row0 + x1) as f32, *buf.get_unchecked(row1 + x0) as f32, *buf.get_unchecked(row1 + x1) as f32) };

    let a = p00 + (p10 - p00) * fx;
    let b = p01 + (p11 - p01) * fx;
    let v = a + (b - a) * fy;
    let v = v + 0.5;
    if v <= 0.0 {
        0
    } else if v >= 255.0 {
        255
    } else {
        v as u8
    }
}

#[allow(unsafe_op_in_unsafe_fn)]
#[inline(always)]
unsafe fn sample_bilinear_gray_raw_unchecked(buf: &[u8], stride: usize, x: f32, y: f32) -> u8 {
    unsafe {
        let x0 = x as i32;
        let y0 = y as i32;
        let x1 = x0 + 1;
        let y1 = y0 + 1;
        let fx = x - x0 as f32;
        let fy = y - y0 as f32;

        let x0 = x0 as usize;
        let x1 = x1 as usize;
        let y0 = y0 as usize;
        let y1 = y1 as usize;

        let row0 = y0 * stride;
        let row1 = y1 * stride;
        let p00 = *buf.get_unchecked(row0 + x0) as f32;
        let p10 = *buf.get_unchecked(row0 + x1) as f32;
        let p01 = *buf.get_unchecked(row1 + x0) as f32;
        let p11 = *buf.get_unchecked(row1 + x1) as f32;

        let a = p00 + (p10 - p00) * fx;
        let b = p01 + (p11 - p01) * fx;
        let v = a + (b - a) * fy + 0.5;
        if v <= 0.0 {
            0
        } else if v >= 255.0 {
            255
        } else {
            v as u8
        }
    }
}

#[inline(always)]
pub(super) fn projection_unit_in_bounds(inv: &Projection, width: i32, height: i32) -> bool {
    if width <= 1 || height <= 1 {
        return false;
    }
    let max_x = (width - 1) as f32;
    let max_y = (height - 1) as f32;
    let corners = [(0.0f32, 0.0f32), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
    for (u, v) in corners {
        let (x, y) = *inv * (u, v);
        if !x.is_finite() || !y.is_finite() {
            return false;
        }
        if x < 0.0 || y < 0.0 || x >= max_x || y >= max_y {
            return false;
        }
    }
    true
}

#[inline(always)]
pub(super) fn cell_sample_ratio(cell_size: f32) -> f32 {
    let mut ratio = 0.25f32;
    if cell_size.is_finite() && cell_size > 0.0 {
        let o_inner = (cell_size * 0.25).clamp(0.25, 4.0);
        ratio = (o_inner / cell_size).clamp(0.0, 0.5);
    }
    ratio
}

pub(super) fn build_sample_positions_unit(total_width: usize, cell_size: f32, grid: u8, margin: f32, positions: &mut Vec<(f32, f32)>) -> usize {
    positions.clear();
    if total_width == 0 {
        return 0;
    }

    let use_grid = grid >= 2;
    let inv_tw = 1.0 / total_width as f32;
    if use_grid {
        let g = grid as usize;
        let margin = margin.clamp(0.0, 0.45);
        let min_off = margin - 0.5;
        let step = if g > 1 { (1.0 - 2.0 * margin) / (g as f32 - 1.0) } else { 0.0 };
        let samples_per_cell = g * g;
        positions.reserve(total_width * total_width * samples_per_cell);
        for row in 0..total_width {
            let base_y = (row as f32 + 0.5) * inv_tw;
            for col in 0..total_width {
                let base_x = (col as f32 + 0.5) * inv_tw;
                for gy in 0..g {
                    let dy = min_off + step * gy as f32;
                    let uy = base_y + dy * inv_tw;
                    for gx in 0..g {
                        let dx = min_off + step * gx as f32;
                        positions.push((base_x + dx * inv_tw, uy));
                    }
                }
            }
        }
        return samples_per_cell;
    }

    let ratio = cell_sample_ratio(cell_size);
    let inner_offsets = [(-ratio, -ratio), (ratio, -ratio), (-ratio, ratio), (ratio, ratio), (0.0, 0.0)];
    positions.reserve(total_width * total_width * inner_offsets.len());
    for row in 0..total_width {
        let base_y = (row as f32 + 0.5) * inv_tw;
        for col in 0..total_width {
            let base_x = (col as f32 + 0.5) * inv_tw;
            for (dx, dy) in inner_offsets {
                positions.push((base_x + dx * inv_tw, base_y + dy * inv_tw));
            }
        }
    }
    inner_offsets.len()
}

pub(super) fn sample_cell_means_from_unit_offsets(gray: &GrayImage, inv: &Projection, total_width: usize, ratio: f32, in_bounds: bool, out: &mut [f32]) {
    if total_width == 0 {
        return;
    }
    let cells = total_width.saturating_mul(total_width);
    if out.len() != cells {
        return;
    }

    let src_width_i32 = gray.width() as i32;
    let src_height_i32 = gray.height() as i32;
    let src_stride = src_width_i32.max(0) as usize;
    let src_pixels = gray.as_raw();
    let use_unchecked = in_bounds && unchecked_sampling_enabled();

    let inv_tw = 1.0 / total_width as f32;
    let off = ratio * inv_tw;

    unsafe {
        let out_ptr = out.as_mut_ptr();
        let mut idx = 0usize;
        let mut base_y = 0.5 * inv_tw;
        if use_unchecked {
            for _ in 0..total_width {
                let mut base_x = 0.5 * inv_tw;
                for _ in 0..total_width {
                    let (sx0, sy0) = *inv * (base_x - off, base_y - off);
                    let (sx1, sy1) = *inv * (base_x + off, base_y - off);
                    let (sx2, sy2) = *inv * (base_x - off, base_y + off);
                    let (sx3, sy3) = *inv * (base_x + off, base_y + off);
                    let (sx4, sy4) = *inv * (base_x, base_y);

                    let sum = sample_bilinear_gray_raw_unchecked(src_pixels, src_stride, sx0, sy0) as f32
                        + sample_bilinear_gray_raw_unchecked(src_pixels, src_stride, sx1, sy1) as f32
                        + sample_bilinear_gray_raw_unchecked(src_pixels, src_stride, sx2, sy2) as f32
                        + sample_bilinear_gray_raw_unchecked(src_pixels, src_stride, sx3, sy3) as f32
                        + sample_bilinear_gray_raw_unchecked(src_pixels, src_stride, sx4, sy4) as f32;
                    *out_ptr.add(idx) = sum * 0.2;
                    idx += 1;
                    base_x += inv_tw;
                }
                base_y += inv_tw;
            }
        } else {
            for _ in 0..total_width {
                let mut base_x = 0.5 * inv_tw;
                for _ in 0..total_width {
                    let (sx0, sy0) = *inv * (base_x - off, base_y - off);
                    let (sx1, sy1) = *inv * (base_x + off, base_y - off);
                    let (sx2, sy2) = *inv * (base_x - off, base_y + off);
                    let (sx3, sy3) = *inv * (base_x + off, base_y + off);
                    let (sx4, sy4) = *inv * (base_x, base_y);

                    let sum = sample_bilinear_gray_raw(src_pixels, src_width_i32, src_height_i32, src_stride, sx0, sy0) as f32
                        + sample_bilinear_gray_raw(src_pixels, src_width_i32, src_height_i32, src_stride, sx1, sy1) as f32
                        + sample_bilinear_gray_raw(src_pixels, src_width_i32, src_height_i32, src_stride, sx2, sy2) as f32
                        + sample_bilinear_gray_raw(src_pixels, src_width_i32, src_height_i32, src_stride, sx3, sy3) as f32
                        + sample_bilinear_gray_raw(src_pixels, src_width_i32, src_height_i32, src_stride, sx4, sy4) as f32;
                    *out_ptr.add(idx) = sum * 0.2;
                    idx += 1;
                    base_x += inv_tw;
                }
                base_y += inv_tw;
            }
        }
    }
}

pub(super) fn sample_cell_means_from_unit_positions(gray: &GrayImage, inv: &Projection, total_width: usize, positions: &[(f32, f32)], samples_per_cell: usize, in_bounds: bool, out: &mut [f32]) {
    if total_width == 0 || samples_per_cell == 0 {
        return;
    }
    let cells = total_width.saturating_mul(total_width);
    if out.len() != cells || positions.len() != cells.saturating_mul(samples_per_cell) {
        return;
    }

    let src_width_i32 = gray.width() as i32;
    let src_height_i32 = gray.height() as i32;
    let src_stride = src_width_i32.max(0) as usize;
    let src_pixels = gray.as_raw();
    let inv_samples = 1.0 / samples_per_cell as f32;
    let use_unchecked = in_bounds && unchecked_sampling_enabled();

    // Common path: 5 samples per cell (grid disabled). Unroll to cut loop/branch overhead.
    if samples_per_cell == 5 {
        // SAFETY: positions length and out length are validated above.
        unsafe {
            let mut pos_ptr = positions.as_ptr();
            let out_ptr = out.as_mut_ptr();
            if use_unchecked {
                for cell in 0..cells {
                    let (u0, v0) = *pos_ptr;
                    pos_ptr = pos_ptr.add(1);
                    let (u1, v1) = *pos_ptr;
                    pos_ptr = pos_ptr.add(1);
                    let (u2, v2) = *pos_ptr;
                    pos_ptr = pos_ptr.add(1);
                    let (u3, v3) = *pos_ptr;
                    pos_ptr = pos_ptr.add(1);
                    let (u4, v4) = *pos_ptr;
                    pos_ptr = pos_ptr.add(1);

                    let (sx0, sy0) = *inv * (u0, v0);
                    let (sx1, sy1) = *inv * (u1, v1);
                    let (sx2, sy2) = *inv * (u2, v2);
                    let (sx3, sy3) = *inv * (u3, v3);
                    let (sx4, sy4) = *inv * (u4, v4);

                    let sum = sample_bilinear_gray_raw_unchecked(src_pixels, src_stride, sx0, sy0) as f32
                        + sample_bilinear_gray_raw_unchecked(src_pixels, src_stride, sx1, sy1) as f32
                        + sample_bilinear_gray_raw_unchecked(src_pixels, src_stride, sx2, sy2) as f32
                        + sample_bilinear_gray_raw_unchecked(src_pixels, src_stride, sx3, sy3) as f32
                        + sample_bilinear_gray_raw_unchecked(src_pixels, src_stride, sx4, sy4) as f32;
                    *out_ptr.add(cell) = sum * 0.2;
                }
            } else {
                for cell in 0..cells {
                    let (u0, v0) = *pos_ptr;
                    pos_ptr = pos_ptr.add(1);
                    let (u1, v1) = *pos_ptr;
                    pos_ptr = pos_ptr.add(1);
                    let (u2, v2) = *pos_ptr;
                    pos_ptr = pos_ptr.add(1);
                    let (u3, v3) = *pos_ptr;
                    pos_ptr = pos_ptr.add(1);
                    let (u4, v4) = *pos_ptr;
                    pos_ptr = pos_ptr.add(1);

                    let (sx0, sy0) = *inv * (u0, v0);
                    let (sx1, sy1) = *inv * (u1, v1);
                    let (sx2, sy2) = *inv * (u2, v2);
                    let (sx3, sy3) = *inv * (u3, v3);
                    let (sx4, sy4) = *inv * (u4, v4);

                    let sum = sample_bilinear_gray_raw(src_pixels, src_width_i32, src_height_i32, src_stride, sx0, sy0) as f32
                        + sample_bilinear_gray_raw(src_pixels, src_width_i32, src_height_i32, src_stride, sx1, sy1) as f32
                        + sample_bilinear_gray_raw(src_pixels, src_width_i32, src_height_i32, src_stride, sx2, sy2) as f32
                        + sample_bilinear_gray_raw(src_pixels, src_width_i32, src_height_i32, src_stride, sx3, sy3) as f32
                        + sample_bilinear_gray_raw(src_pixels, src_width_i32, src_height_i32, src_stride, sx4, sy4) as f32;
                    *out_ptr.add(cell) = sum * 0.2;
                }
            }
        }
        return;
    }

    // SAFETY: positions length and out length are validated above.
    unsafe {
        let mut pos_ptr = positions.as_ptr();
        let out_ptr = out.as_mut_ptr();
        if use_unchecked {
            for cell in 0..cells {
                let mut sum = 0.0f32;
                for _ in 0..samples_per_cell {
                    let (u, v) = *pos_ptr;
                    pos_ptr = pos_ptr.add(1);
                    let (sx, sy) = *inv * (u, v);
                    sum += sample_bilinear_gray_raw_unchecked(src_pixels, src_stride, sx, sy) as f32;
                }
                *out_ptr.add(cell) = sum * inv_samples;
            }
        } else {
            for cell in 0..cells {
                let mut sum = 0.0f32;
                for _ in 0..samples_per_cell {
                    let (u, v) = *pos_ptr;
                    pos_ptr = pos_ptr.add(1);
                    let (sx, sy) = *inv * (u, v);
                    sum += sample_bilinear_gray_raw(src_pixels, src_width_i32, src_height_i32, src_stride, sx, sy) as f32;
                }
                *out_ptr.add(cell) = sum * inv_samples;
            }
        }
    }
}
