use image::{DynamicImage, GrayImage, RgbImage, RgbaImage, imageops::overlay};
use rayon::prelude::*;
use wide::f32x8;

fn overlay_alpha_rgb8(base: &mut RgbImage, top: &RgbaImage, x: i64, y: i64) {
    let bw = base.width() as i64;
    let bh = base.height() as i64;
    let tw = top.width() as i64;
    let th = top.height() as i64;

    let start_x = x.max(0);
    let start_y = y.max(0);
    let end_x = (x + tw).min(bw);
    let end_y = (y + th).min(bh);
    if start_x >= end_x || start_y >= end_y {
        return;
    }

    let width = (end_x - start_x) as usize;
    let rows = (end_y - start_y) as usize;
    let b_stride = base.width() as usize * 3;
    let t_stride = top.width() as usize * 4;
    let slice_start = start_y as usize * base.width() as usize * 3;
    let slice_end = end_y as usize * base.width() as usize * 3;
    let start_off = start_x as usize * 3;
    let t_off = ((start_y - y) as usize * top.width() as usize + (start_x - x) as usize) * 4;

    let b_buf = base.as_flat_samples_mut().samples;
    let t_buf = top.as_flat_samples().samples;

    let blend_row = |(row, b_row): (usize, &mut [u8])| {
        let t_row = &t_buf[t_off + row * t_stride..t_off + row * t_stride + width * 4];
        let mut i = 0;
        const LANES: usize = 8;
        while i + LANES <= width {
            let mut br = [0.0; LANES];
            let mut bg = [0.0; LANES];
            let mut bb = [0.0; LANES];
            let mut tr = [0.0; LANES];
            let mut tg = [0.0; LANES];
            let mut tb = [0.0; LANES];
            let mut a = [0.0; LANES];
            for lane in 0..LANES {
                let bi = 3 * (i + lane);
                let ti = 4 * (i + lane);
                br[lane] = b_row[bi] as f32;
                bg[lane] = b_row[bi + 1] as f32;
                bb[lane] = b_row[bi + 2] as f32;
                tr[lane] = t_row[ti] as f32;
                tg[lane] = t_row[ti + 1] as f32;
                tb[lane] = t_row[ti + 2] as f32;
                a[lane] = t_row[ti + 3] as f32 / 255.0;
            }
            let brv = f32x8::from(br);
            let bgv = f32x8::from(bg);
            let bbv = f32x8::from(bb);
            let trv = f32x8::from(tr);
            let tgv = f32x8::from(tg);
            let tbv = f32x8::from(tb);
            let av = f32x8::from(a);
            let inv_a = f32x8::splat(1.0) - av;
            let rr = brv * inv_a + trv * av;
            let gg = bgv * inv_a + tgv * av;
            let bb = bbv * inv_a + tbv * av;
            let rr_a = rr.to_array();
            let gg_a = gg.to_array();
            let bb_a = bb.to_array();
            for lane in 0..LANES {
                let bi = 3 * (i + lane);
                b_row[bi] = rr_a[lane].round() as u8;
                b_row[bi + 1] = gg_a[lane].round() as u8;
                b_row[bi + 2] = bb_a[lane].round() as u8;
            }
            i += LANES;
        }
        for px in i..width {
            let bi = px * 3;
            let ti = px * 4;
            let alpha = t_row[ti + 3] as f32 / 255.0;
            if alpha == 0.0 {
                continue;
            }
            for c in 0..3 {
                let bc = b_row[bi + c] as f32;
                let tc = t_row[ti + c] as f32;
                b_row[bi + c] = (bc * (1.0 - alpha) + tc * alpha).round() as u8;
            }
        }
    };

    if rows > 32 && width > 32 {
        b_buf[slice_start..slice_end].par_chunks_mut(b_stride).enumerate().for_each(|(r, row)| blend_row((r, &mut row[start_off..start_off + width * 3])));
    } else {
        for (r, row) in b_buf[slice_start..slice_end].chunks_mut(b_stride).enumerate() {
            blend_row((r, &mut row[start_off..start_off + width * 3]));
        }
    }
}

fn overlay_alpha_luma8(base: &mut GrayImage, top: &RgbaImage, x: i64, y: i64) {
    let bw = base.width() as i64;
    let bh = base.height() as i64;
    let tw = top.width() as i64;
    let th = top.height() as i64;

    let start_x = x.max(0);
    let start_y = y.max(0);
    let end_x = (x + tw).min(bw);
    let end_y = (y + th).min(bh);
    if start_x >= end_x || start_y >= end_y {
        return;
    }

    let width = (end_x - start_x) as usize;
    let rows = (end_y - start_y) as usize;
    let b_stride = base.width() as usize;
    let t_stride = top.width() as usize * 4;
    let slice_start = start_y as usize * base.width() as usize;
    let slice_end = end_y as usize * base.width() as usize;
    let start_off = start_x as usize;
    let t_off = ((start_y - y) as usize * top.width() as usize + (start_x - x) as usize) * 4;

    let b_buf = base.as_flat_samples_mut().samples;
    let t_buf = top.as_flat_samples().samples;

    let blend_row = |(row, b_row): (usize, &mut [u8])| {
        let t_row = &t_buf[t_off + row * t_stride..t_off + row * t_stride + width * 4];
        let mut i = 0;
        const LANES: usize = 8;
        while i + LANES <= width {
            let mut base_v = [0.0; LANES];
            let mut lum_v = [0.0; LANES];
            let mut a_v = [0.0; LANES];
            for lane in 0..LANES {
                let bi = i + lane;
                let ti = 4 * (i + lane);
                base_v[lane] = b_row[bi] as f32;
                lum_v[lane] = 0.299 * t_row[ti] as f32 + 0.587 * t_row[ti + 1] as f32 + 0.114 * t_row[ti + 2] as f32;
                a_v[lane] = t_row[ti + 3] as f32 / 255.0;
            }
            let bv = f32x8::from(base_v);
            let lv = f32x8::from(lum_v);
            let av = f32x8::from(a_v);
            let inv_a = f32x8::splat(1.0) - av;
            let res = bv * inv_a + lv * av;
            let res_a = res.to_array();
            for lane in 0..LANES {
                b_row[i + lane] = res_a[lane].round() as u8;
            }
            i += LANES;
        }
        for px in i..width {
            let bi = px;
            let ti = px * 4;
            let alpha = t_row[ti + 3] as f32 / 255.0;
            if alpha == 0.0 {
                continue;
            }
            let lum = 0.299 * t_row[ti] as f32 + 0.587 * t_row[ti + 1] as f32 + 0.114 * t_row[ti + 2] as f32;
            let bc = b_row[bi] as f32;
            b_row[bi] = (bc * (1.0 - alpha) + lum * alpha).round() as u8;
        }
    };

    if rows > 32 && width > 32 {
        b_buf[slice_start..slice_end].par_chunks_mut(b_stride).enumerate().for_each(|(r, row)| blend_row((r, &mut row[start_off..start_off + width])));
    } else {
        for (r, row) in b_buf[slice_start..slice_end].chunks_mut(b_stride).enumerate() {
            blend_row((r, &mut row[start_off..start_off + width]));
        }
    }
}

/// Overlay `top` onto `base` converting formats as needed.
pub fn overlay_dynamic(base: &mut DynamicImage, top: &DynamicImage, x: i64, y: i64) {
    match base {
        DynamicImage::ImageRgba8(buf) => overlay(buf, &top.to_rgba8(), x, y),
        DynamicImage::ImageLumaA8(buf) => overlay(buf, &top.to_luma_alpha8(), x, y),
        DynamicImage::ImageRgba16(buf) => overlay(buf, &top.to_rgba16(), x, y),
        DynamicImage::ImageLumaA16(buf) => overlay(buf, &top.to_luma_alpha16(), x, y),
        DynamicImage::ImageRgba32F(buf) => overlay(buf, &top.to_rgba32f(), x, y),
        DynamicImage::ImageLuma8(buf) => {
            let top_rgba = top.to_rgba8();
            overlay_alpha_luma8(buf, &top_rgba, x, y);
        }
        DynamicImage::ImageRgb8(buf) => {
            let top_rgba = top.to_rgba8();
            overlay_alpha_rgb8(buf, &top_rgba, x, y);
        }
        DynamicImage::ImageLuma16(_) => {
            let mut rgba = base.to_rgba16();
            overlay(&mut rgba, &top.to_rgba16(), x, y);
            let gray = DynamicImage::ImageRgba16(rgba).to_luma16();
            *base = DynamicImage::ImageLuma16(gray);
        }
        DynamicImage::ImageRgb16(_) => {
            let mut rgba = base.to_rgba16();
            overlay(&mut rgba, &top.to_rgba16(), x, y);
            let rgb = DynamicImage::ImageRgba16(rgba).to_rgb16();
            *base = DynamicImage::ImageRgb16(rgb);
        }
        DynamicImage::ImageRgb32F(_) => {
            let mut rgba = base.to_rgba32f();
            overlay(&mut rgba, &top.to_rgba32f(), x, y);
            let rgb = DynamicImage::ImageRgba32F(rgba).to_rgb32f();
            *base = DynamicImage::ImageRgb32F(rgb);
        }
        _ => {
            let mut rgba = base.to_rgba8();
            overlay(&mut rgba, &top.to_rgba8(), x, y);
            *base = DynamicImage::ImageRgba8(rgba);
        }
    }
}

/// Overlay an RGBA image onto a dynamic image without reallocating `top` when possible.
pub fn overlay_rgba_image(base: &mut DynamicImage, top: &RgbaImage, x: i64, y: i64) {
    match base {
        DynamicImage::ImageRgba8(buf) => overlay(buf, top, x, y),
        DynamicImage::ImageLuma8(buf) => overlay_alpha_luma8(buf, top, x, y),
        DynamicImage::ImageRgb8(buf) => overlay_alpha_rgb8(buf, top, x, y),
        _ => {
            let top_dyn = DynamicImage::ImageRgba8(top.clone());
            overlay_dynamic(base, &top_dyn, x, y);
        }
    }
}
