#![allow(unsafe_code)]

use super::*;

fn majority_filter_mask(mask: &mut GrayImage, radius: u32, min_on_override: u32) {
    if radius == 0 {
        return;
    }
    let (w, h) = mask.dimensions();
    if w == 0 || h == 0 {
        return;
    }

    let win = 2 * radius + 1;
    let win_area = win.saturating_mul(win);
    let min_on = if min_on_override > 0 { min_on_override.min(win_area) } else { (win_area / 2) + 1 };

    // Integral image over a binary 0/1 mask.
    let iw = (w as usize).saturating_add(1);
    let ih = (h as usize).saturating_add(1);
    with_integral_scratch(iw.saturating_mul(ih), |integral| {
        {
            let src = mask.as_raw();
            for y in 0..h as usize {
                let row_sum_idx = (y + 1) * iw;
                let prev_row_idx = y * iw;
                let mut row_sum = 0u32;
                for x in 0..w as usize {
                    row_sum += if src[y * w as usize + x] > 0 { 1 } else { 0 };
                    integral[row_sum_idx + x + 1] = integral[prev_row_idx + x + 1] + row_sum;
                }
            }
        }

        let dst = mask.as_mut();
        for y in 0..h as i32 {
            let y0 = (y as i64 - radius as i64).max(0) as usize;
            let y1 = (y as i64 + radius as i64).min((h as i64) - 1) as usize;
            let iy0 = y0;
            let iy1 = y1 + 1;
            for x in 0..w as i32 {
                let x0 = (x as i64 - radius as i64).max(0) as usize;
                let x1 = (x as i64 + radius as i64).min((w as i64) - 1) as usize;
                let ix0 = x0;
                let ix1 = x1 + 1;

                let a = integral[iy0 * iw + ix0];
                let b = integral[iy0 * iw + ix1];
                let c = integral[iy1 * iw + ix0];
                let d = integral[iy1 * iw + ix1];
                let sum = d + a - b - c;

                dst[y as usize * w as usize + x as usize] = if sum >= min_on { 255 } else { 0 };
            }
        }
    });
}

fn prune_sparse_mask(mask: &mut GrayImage, radius: u32, max_on: u32) {
    if radius == 0 {
        return;
    }
    let (w, h) = mask.dimensions();
    if w == 0 || h == 0 {
        return;
    }
    let win = 2 * radius + 1;
    let win_area = win.saturating_mul(win);
    let max_on = max_on.clamp(1, win_area);

    let iw = (w as usize).saturating_add(1);
    let ih = (h as usize).saturating_add(1);
    with_integral_scratch(iw.saturating_mul(ih), |integral| {
        {
            let src = mask.as_raw();
            for y in 0..h as usize {
                let row_sum_idx = (y + 1) * iw;
                let prev_row_idx = y * iw;
                let mut row_sum = 0u32;
                for x in 0..w as usize {
                    row_sum += if src[y * w as usize + x] > 0 { 1 } else { 0 };
                    integral[row_sum_idx + x + 1] = integral[prev_row_idx + x + 1] + row_sum;
                }
            }
        }

        let dst = mask.as_mut();
        for y in 0..h as i32 {
            let y0 = (y as i64 - radius as i64).max(0) as usize;
            let y1 = (y as i64 + radius as i64).min((h as i64) - 1) as usize;
            let iy0 = y0;
            let iy1 = y1 + 1;
            for x in 0..w as i32 {
                let idx = y as usize * w as usize + x as usize;
                if dst[idx] == 0 {
                    continue;
                }
                let x0 = (x as i64 - radius as i64).max(0) as usize;
                let x1 = (x as i64 + radius as i64).min((w as i64) - 1) as usize;
                let ix0 = x0;
                let ix1 = x1 + 1;

                let a = integral[iy0 * iw + ix0];
                let b = integral[iy0 * iw + ix1];
                let c = integral[iy1 * iw + ix0];
                let d = integral[iy1 * iw + ix1];
                let sum = d + a - b - c;

                // Remove isolated/small speckle that survives thresholding, but keep edges/regions.
                if sum <= max_on {
                    dst[idx] = 0;
                }
            }
        }
    });
}

fn gradient_gate_mask(mask: &mut GrayImage, gray: &GrayImage, k: u8, min_grad: u8) {
    if k == 0 || min_grad == 0 {
        return;
    }
    let (w, h) = mask.dimensions();
    if w == 0 || h == 0 {
        return;
    }
    if gray.dimensions() != (w, h) {
        return;
    }

    if !mask.as_raw().iter().any(|&v| v != 0) {
        return;
    }
    let grad = crate::ops::morphology::gradient_simd(gray, imageproc::distance_transform::Norm::L1, k);
    let dst = mask.as_mut();
    let g = grad.as_raw();
    for i in 0..dst.len() {
        if dst[i] > 0 && g[i] < min_grad {
            dst[i] = 0;
        }
    }
}

fn downscale_gray_box(gray: &GrayImage, factor: u32) -> GrayImage {
    let factor = factor.max(1);
    if factor == 1 {
        return gray.clone();
    }
    if factor == 2 && gray.width() >= 2 && gray.height() >= 2 {
        return downscale_gray_box2(gray);
    }

    let src_w = gray.width();
    let src_h = gray.height();
    let dst_w = (src_w / factor).max(1);
    let dst_h = (src_h / factor).max(1);

    let src_stride = src_w as usize;
    let src_buf = gray.as_raw();
    let mut out = GrayImage::new(dst_w, dst_h);
    let dst_stride = dst_w as usize;
    let dst_buf = out.as_mut();

    let f = factor as usize;
    for y in 0..dst_h as usize {
        let sy0 = y * f;
        for x in 0..dst_w as usize {
            let sx0 = x * f;
            let mut sum: u32 = 0;
            let mut count: u32 = 0;
            for dy in 0..f {
                let sy = sy0 + dy;
                if sy >= src_h as usize {
                    break;
                }
                let row = sy * src_stride;
                for dx in 0..f {
                    let sx = sx0 + dx;
                    if sx >= src_w as usize {
                        break;
                    }
                    sum += src_buf[row + sx] as u32;
                    count += 1;
                }
            }
            let v = if count > 0 { (sum / count) as u8 } else { 0 };
            dst_buf[y * dst_stride + x] = v;
        }
    }

    out
}

fn downscale_gray_box2(gray: &GrayImage) -> GrayImage {
    let src_w = gray.width() as usize;
    let src_h = gray.height() as usize;
    let dst_w = (src_w / 2).max(1);
    let dst_h = (src_h / 2).max(1);

    let src_buf = gray.as_raw();
    let mut out = GrayImage::new(dst_w as u32, dst_h as u32);
    let dst_buf = out.as_mut();

    #[cfg(target_arch = "aarch64")]
    {
        if crate::simd::neon_enabled() {
            use std::arch::aarch64::{uint8x8_t, uint8x16_t, uint16x8_t, vaddq_u16, vld1q_u8, vmovn_u16, vpaddlq_u8, vshrq_n_u16, vst1_u8};

            unsafe {
                for y in 0..dst_h {
                    let row0 = &src_buf[(2 * y) * src_w..(2 * y + 1) * src_w];
                    let row1 = &src_buf[(2 * y + 1) * src_w..(2 * y + 2) * src_w];
                    let dst_row = &mut dst_buf[y * dst_w..(y + 1) * dst_w];

                    let mut x = 0usize;
                    while x + 8 <= dst_w {
                        let src_x = 2 * x;
                        let p0: uint8x16_t = vld1q_u8(row0.as_ptr().add(src_x));
                        let p1: uint8x16_t = vld1q_u8(row1.as_ptr().add(src_x));

                        let sum0: uint16x8_t = vpaddlq_u8(p0);
                        let sum1: uint16x8_t = vpaddlq_u8(p1);
                        let sum: uint16x8_t = vaddq_u16(sum0, sum1);
                        let avg: uint16x8_t = vshrq_n_u16(sum, 2);
                        let outv: uint8x8_t = vmovn_u16(avg);

                        vst1_u8(dst_row.as_mut_ptr().add(x), outv);
                        x += 8;
                    }

                    for x in x..dst_w {
                        let sx = 2 * x;
                        let sum = row0[sx] as u16 + row0[sx + 1] as u16 + row1[sx] as u16 + row1[sx + 1] as u16;
                        dst_row[x] = (sum / 4) as u8;
                    }
                }
            }
            return out;
        }
    }

    {
        for y in 0..dst_h {
            let row0 = &src_buf[(2 * y) * src_w..(2 * y + 1) * src_w];
            let row1 = &src_buf[(2 * y + 1) * src_w..(2 * y + 2) * src_w];
            let dst_row = &mut dst_buf[y * dst_w..(y + 1) * dst_w];
            for (x, dst_cell) in dst_row.iter_mut().enumerate().take(dst_w) {
                let sx = 2 * x;
                let sum = row0[sx] as u16 + row0[sx + 1] as u16 + row1[sx] as u16 + row1[sx + 1] as u16;
                *dst_cell = (sum / 4) as u8;
            }
        }
    }

    out
}

#[node(id = "mask_downscale_gray", inputs("mask", port(name = "factor", default = 2i64, meta(ui_min = 1, ui_max = 8, ui_step = 1))), outputs("mask"))]
fn cv_mask_downscale_gray(mask: GrayImage, factor: i64) -> Result<GrayImage, NodeError> {
    let factor = u32::try_from(factor).unwrap_or(1).max(1);
    if factor == 1 {
        return Ok(mask);
    }
    Ok(downscale_gray_box(&mask, factor))
}

#[node(id = "mask_blur_gray", inputs("mask", port(name = "sigma", default = 0.0f64, meta(ui_min = 0.0, ui_max = 10.0, ui_step = 0.1))), outputs("mask"))]
fn cv_mask_blur_gray(mut mask: GrayImage, sigma: f64) -> Result<GrayImage, NodeError> {
    if sigma <= 0.0 {
        return Ok(mask);
    }
    if mask.as_raw().iter().all(|&v| v == 0) {
        return Ok(mask);
    }
    crate::modules::image::blur::blur_gray_image_in_place(&mut mask, sigma as f32);
    Ok(mask)
}

#[node(
    id = "mask_gradient_gate",
    inputs("mask", "gray", port(name = "k", default = 0i64, meta(ui_min = 0, ui_max = 255, ui_step = 1)), port(name = "min", default = 0i64, meta(ui_min = 0, ui_max = 255, ui_step = 1))),
    outputs("mask")
)]
fn cv_mask_gradient_gate(mut mask: GrayImage, gray: &GrayImage, k: i64, min: i64) -> Result<GrayImage, NodeError> {
    let k = k.max(0).min(u8::MAX as i64) as u8;
    let min = min.max(0).min(u8::MAX as i64) as u8;
    if k == 0 || min == 0 || gray.width() == 0 || gray.height() == 0 {
        return Ok(mask);
    }
    gradient_gate_mask(&mut mask, gray, k, min);
    Ok(mask)
}

#[node(
    id = "mask_prune_sparse",
    inputs(
        "mask",
        port(name = "radius", default = 0i64, meta(ui_min = 0, ui_max = 8, ui_step = 1)),
        port(name = "iters", default = 0i64, meta(ui_min = 0, ui_max = 4, ui_step = 1)),
        port(name = "max_on", default = 0i64, meta(ui_min = 0, ui_max = 255, ui_step = 1))
    ),
    outputs("mask")
)]
fn cv_mask_prune_sparse(mut mask: GrayImage, radius: i64, iters: i64, max_on: i64) -> Result<GrayImage, NodeError> {
    let radius = radius.clamp(0, 8) as u32;
    let iters = iters.clamp(0, 4) as u32;
    let max_on = max_on.max(0) as u32;
    if radius == 0 || iters == 0 || max_on == 0 {
        return Ok(mask);
    }
    if mask.as_raw().iter().all(|&v| v == 0) {
        return Ok(mask);
    }
    for _ in 0..iters {
        prune_sparse_mask(&mut mask, radius, max_on);
    }
    Ok(mask)
}

#[node(id = "mask_component_filter", inputs("mask", port(name = "min_area", default = 0i64, meta(ui_min = 0, ui_max = 100000, ui_step = 1))), outputs("mask"))]
fn cv_mask_component_filter(mut mask: GrayImage, min_area: i64) -> Result<GrayImage, NodeError> {
    let min_area = min_area.max(0) as u32;
    if min_area == 0 {
        return Ok(mask);
    }
    crate::modules::image::components::remove_small_components_in_place(&mut mask, min_area);
    Ok(mask)
}

#[node(id = "mask_open", inputs("mask", port(name = "k", default = 0i64, meta(ui_min = 0, ui_max = 255, ui_step = 1))), outputs("mask"))]
fn cv_mask_open(mask: GrayImage, k: i64) -> Result<GrayImage, NodeError> {
    let k = k.max(0) as u32;
    if k == 0 {
        return Ok(mask);
    }
    if mask.as_raw().iter().all(|&v| v == 0) {
        return Ok(mask);
    }
    let k = k.min(u8::MAX as u32) as u8;
    Ok(crate::ops::morphology::open(&mask, imageproc::distance_transform::Norm::L1, k))
}

// Internal-stage variant used by node-groups. Distinct input/output names avoid
// ambiguous port aliasing in embedded graph expansion when a node uses the same
// identifier for both directions.
#[node(id = "mask_open_stage", inputs(port(name = "in_mask"), port(name = "k", default = 0i64, meta(ui_min = 0, ui_max = 255, ui_step = 1))), outputs(port(name = "out_mask")))]
fn cv_mask_open_stage(in_mask: GrayImage, k: i64) -> Result<GrayImage, NodeError> {
    let k = k.max(0) as u32;
    if k == 0 {
        return Ok(in_mask);
    }
    if in_mask.as_raw().iter().all(|&v| v == 0) {
        return Ok(in_mask);
    }
    let k = k.min(u8::MAX as u32) as u8;
    Ok(crate::ops::morphology::open(&in_mask, imageproc::distance_transform::Norm::L1, k))
}

#[node(
    id = "mask_majority",
    inputs("mask", port(name = "radius", default = 0i64, meta(ui_min = 0, ui_max = 8, ui_step = 1)), port(name = "min_on", default = 0i64, meta(ui_min = 0, ui_max = 255, ui_step = 1))),
    outputs("mask")
)]
fn cv_mask_majority(mut mask: GrayImage, radius: i64, min_on: i64) -> Result<GrayImage, NodeError> {
    let radius = radius.clamp(0, 8) as u32;
    let min_on = min_on.max(0) as u32;
    if radius == 0 {
        return Ok(mask);
    }
    if mask.as_raw().iter().all(|&v| v == 0) {
        return Ok(mask);
    }
    majority_filter_mask(&mut mask, radius, min_on);
    Ok(mask)
}
