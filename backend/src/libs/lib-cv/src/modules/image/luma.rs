use image::{DynamicImage, GrayImage};
use std::sync::{Mutex, OnceLock};

const LUMA_SCRATCH_RETAIN_CAP: usize = 2 * 1024 * 1024;

fn luma_scratch_pool() -> &'static Mutex<Option<Vec<u8>>> {
    static POOL: OnceLock<Mutex<Option<Vec<u8>>>> = OnceLock::new();
    POOL.get_or_init(|| Mutex::new(None))
}

fn with_luma_scratch<R>(needed: usize, label: &'static str, f: impl FnOnce(&mut Vec<u8>) -> R) -> R {
    let mut buf = luma_scratch_pool().lock().ok().and_then(|mut slot| slot.take()).unwrap_or_default();
    if buf.len() != needed {
        buf.resize(needed, 0);
        crate::diagnostics::report_scratch_high_water(label, buf.capacity());
    }
    let out = f(&mut buf);
    buf.clear();
    if buf.capacity() <= LUMA_SCRATCH_RETAIN_CAP
        && let Ok(mut slot) = luma_scratch_pool().lock()
        && slot.is_none()
    {
        *slot = Some(buf);
    }
    out
}

pub(crate) fn compact_luma_scratch_after_frame() {
    if let Ok(mut slot) = luma_scratch_pool().lock() {
        if slot.as_ref().is_some_and(|scratch| scratch.capacity() > LUMA_SCRATCH_RETAIN_CAP) {
            *slot = None;
        }
    }
}

pub(crate) fn release_luma_scratch_on_idle() {
    if let Ok(mut slot) = luma_scratch_pool().lock() {
        *slot = None;
    }
}

const SRGB_LUMA_R: u32 = 2126;
const SRGB_LUMA_G: u32 = 7152;
const SRGB_LUMA_B: u32 = 722;
const SRGB_LUMA_DIV: u32 = 10000;

#[inline(always)]
fn srgb_to_luma_u8(r: u8, g: u8, b: u8) -> u8 {
    let l = (SRGB_LUMA_R * r as u32 + SRGB_LUMA_G * g as u32 + SRGB_LUMA_B * b as u32) / SRGB_LUMA_DIV;
    l as u8
}

#[cfg(target_arch = "aarch64")]
#[allow(unsafe_op_in_unsafe_fn)]
#[target_feature(enable = "neon")]
unsafe fn rgb_to_luma_neon(dst: &mut [u8], src: &[u8]) {
    use core::arch::aarch64::*;

    debug_assert_eq!(src.len(), dst.len().saturating_mul(3));
    let mut i = 0usize;
    let len = dst.len();
    while i + 16 <= len {
        let rgb = vld3q_u8(src.as_ptr().add(i * 3));

        let r_lo = vmovl_u8(vget_low_u8(rgb.0));
        let g_lo = vmovl_u8(vget_low_u8(rgb.1));
        let b_lo = vmovl_u8(vget_low_u8(rgb.2));
        let r_hi = vmovl_u8(vget_high_u8(rgb.0));
        let g_hi = vmovl_u8(vget_high_u8(rgb.1));
        let b_hi = vmovl_u8(vget_high_u8(rgb.2));

        let lo = vshrq_n_u16(vmlaq_n_u16(vmlaq_n_u16(vmulq_n_u16(r_lo, 77), g_lo, 150), b_lo, 29), 8);
        let hi = vshrq_n_u16(vmlaq_n_u16(vmlaq_n_u16(vmulq_n_u16(r_hi, 77), g_hi, 150), b_hi, 29), 8);
        vst1q_u8(dst.as_mut_ptr().add(i), vcombine_u8(vmovn_u16(lo), vmovn_u16(hi)));
        i += 16;
    }

    while i < len {
        let base = i * 3;
        dst[i] = srgb_to_luma_u8(src[base], src[base + 1], src[base + 2]);
        i += 1;
    }
}

#[cfg(target_arch = "aarch64")]
#[allow(unsafe_op_in_unsafe_fn)]
#[target_feature(enable = "neon")]
unsafe fn rgba_to_luma_neon(dst: &mut [u8], src: &[u8]) {
    use core::arch::aarch64::*;

    debug_assert_eq!(src.len(), dst.len().saturating_mul(4));
    let mut i = 0usize;
    let len = dst.len();
    while i + 16 <= len {
        let rgba = vld4q_u8(src.as_ptr().add(i * 4));

        let r_lo = vmovl_u8(vget_low_u8(rgba.0));
        let g_lo = vmovl_u8(vget_low_u8(rgba.1));
        let b_lo = vmovl_u8(vget_low_u8(rgba.2));
        let r_hi = vmovl_u8(vget_high_u8(rgba.0));
        let g_hi = vmovl_u8(vget_high_u8(rgba.1));
        let b_hi = vmovl_u8(vget_high_u8(rgba.2));

        let lo = vshrq_n_u16(vmlaq_n_u16(vmlaq_n_u16(vmulq_n_u16(r_lo, 77), g_lo, 150), b_lo, 29), 8);
        let hi = vshrq_n_u16(vmlaq_n_u16(vmlaq_n_u16(vmulq_n_u16(r_hi, 77), g_hi, 150), b_hi, 29), 8);
        vst1q_u8(dst.as_mut_ptr().add(i), vcombine_u8(vmovn_u16(lo), vmovn_u16(hi)));
        i += 16;
    }

    while i < len {
        let base = i * 4;
        dst[i] = srgb_to_luma_u8(src[base], src[base + 1], src[base + 2]);
        i += 1;
    }
}

#[inline(always)]
fn rgb_to_luma_into(dst: &mut [u8], src: &[u8]) {
    #[cfg(target_arch = "aarch64")]
    {
        if crate::simd::neon_enabled() {
            // SAFETY: guarded by runtime feature detection.
            unsafe {
                rgb_to_luma_neon(dst, src);
            }
            return;
        }
    }
    for (px, dst_px) in src.chunks_exact(3).zip(dst.iter_mut()) {
        *dst_px = srgb_to_luma_u8(px[0], px[1], px[2]);
    }
}

#[inline(always)]
fn rgba_to_luma_into(dst: &mut [u8], src: &[u8]) {
    #[cfg(target_arch = "aarch64")]
    {
        if crate::simd::neon_enabled() {
            // SAFETY: guarded by runtime feature detection.
            unsafe {
                rgba_to_luma_neon(dst, src);
            }
            return;
        }
    }
    for (px, dst_px) in src.chunks_exact(4).zip(dst.iter_mut()) {
        *dst_px = srgb_to_luma_u8(px[0], px[1], px[2]);
    }
}

fn with_gray_scratch<R>(width: u32, height: u32, label: &'static str, fill: impl FnOnce(&mut [u8]), f: impl FnOnce(&GrayImage) -> R) -> R {
    let needed = (width as usize).saturating_mul(height as usize);
    with_luma_scratch(needed, label, |buf| {
        fill(buf.as_mut_slice());
        let img = GrayImage::from_raw(width, height, std::mem::take(buf)).unwrap_or_else(|| GrayImage::new(width, height));
        let out = f(&img);
        *buf = img.into_raw();
        out
    })
}

pub(crate) fn crop_luma8_frame(frame: &DynamicImage, x: u32, y: u32, width: u32, height: u32) -> GrayImage {
    if width == 0 || height == 0 {
        return GrayImage::new(0, 0);
    }

    match frame {
        DynamicImage::ImageLuma8(gray) => {
            if x == 0 && y == 0 && width == gray.width() && height == gray.height() {
                return gray.clone();
            }
            let raw = gray.as_raw();
            let stride = gray.width() as usize;
            let x = x as usize;
            let y = y as usize;
            let width_usize = width as usize;
            let height_usize = height as usize;
            let mut out = GrayImage::new(width, height);
            let out_raw = out.as_mut();
            for row in 0..height_usize {
                let src_start = (y + row) * stride + x;
                let src_end = src_start + width_usize;
                let dst_start = row * width_usize;
                out_raw[dst_start..dst_start + width_usize].copy_from_slice(&raw[src_start..src_end]);
            }
            out
        }
        DynamicImage::ImageLumaA8(gray) => {
            let raw = gray.as_raw();
            let stride = gray.width() as usize;
            let x = x as usize;
            let y = y as usize;
            let width_usize = width as usize;
            let height_usize = height as usize;
            let mut out = GrayImage::new(width, height);
            let out_raw = out.as_mut();
            for row in 0..height_usize {
                let src_row = ((y + row) * stride + x) * 2;
                let dst_row = row * width_usize;
                for col in 0..width_usize {
                    out_raw[dst_row + col] = raw[src_row + col * 2];
                }
            }
            out
        }
        DynamicImage::ImageRgb8(rgb) => {
            let raw = rgb.as_raw();
            let stride = rgb.width() as usize;
            let x = x as usize;
            let y = y as usize;
            let width_usize = width as usize;
            let height_usize = height as usize;
            let mut out = GrayImage::new(width, height);
            let out_raw = out.as_mut();
            for row in 0..height_usize {
                let src_row = ((y + row) * stride + x) * 3;
                let dst_row = row * width_usize;
                rgb_to_luma_into(&mut out_raw[dst_row..dst_row + width_usize], &raw[src_row..src_row + width_usize * 3]);
            }
            out
        }
        DynamicImage::ImageRgba8(rgba) => {
            let raw = rgba.as_raw();
            let stride = rgba.width() as usize;
            let x = x as usize;
            let y = y as usize;
            let width_usize = width as usize;
            let height_usize = height as usize;
            let mut out = GrayImage::new(width, height);
            let out_raw = out.as_mut();
            for row in 0..height_usize {
                let src_row = ((y + row) * stride + x) * 4;
                let dst_row = row * width_usize;
                rgba_to_luma_into(&mut out_raw[dst_row..dst_row + width_usize], &raw[src_row..src_row + width_usize * 4]);
            }
            out
        }
        other => {
            let gray = other.crop_imm(x, y, width, height).to_luma8();
            let mut out = GrayImage::new(width, height);
            out.as_mut().copy_from_slice(gray.as_raw());
            out
        }
    }
}

pub(crate) fn crop_luma8_image(frame: &DynamicImage, x: u32, y: u32, width: u32, height: u32) -> GrayImage {
    crop_luma8_frame(frame, x, y, width, height)
}

pub(crate) fn with_cropped_luma8_frame<R>(frame: &DynamicImage, x: u32, y: u32, width: u32, height: u32, f: impl FnOnce(&GrayImage) -> R) -> R {
    if width == 0 || height == 0 {
        let img = GrayImage::new(0, 0);
        return f(&img);
    }

    match frame {
        DynamicImage::ImageLuma8(gray) if x == 0 && y == 0 && width == gray.width() && height == gray.height() => f(gray),
        DynamicImage::ImageLuma8(gray) => {
            let raw = gray.as_raw();
            let stride = gray.width() as usize;
            let x = x as usize;
            let y = y as usize;
            let width_usize = width as usize;
            let height_usize = height as usize;
            with_gray_scratch(
                width,
                height,
                "image.luma_crop_scratch",
                |dst| {
                    for row in 0..height_usize {
                        let src_start = (y + row) * stride + x;
                        let src_end = src_start + width_usize;
                        let dst_start = row * width_usize;
                        dst[dst_start..dst_start + width_usize].copy_from_slice(&raw[src_start..src_end]);
                    }
                },
                f,
            )
        }
        DynamicImage::ImageLumaA8(gray) => {
            let raw = gray.as_raw();
            let stride = gray.width() as usize;
            let x = x as usize;
            let y = y as usize;
            let width_usize = width as usize;
            let height_usize = height as usize;
            with_gray_scratch(
                width,
                height,
                "image.luma_crop_scratch",
                |dst| {
                    for row in 0..height_usize {
                        let src_row = ((y + row) * stride + x) * 2;
                        let dst_row = row * width_usize;
                        for col in 0..width_usize {
                            dst[dst_row + col] = raw[src_row + col * 2];
                        }
                    }
                },
                f,
            )
        }
        DynamicImage::ImageRgb8(rgb) => {
            let raw = rgb.as_raw();
            let stride = rgb.width() as usize;
            let x = x as usize;
            let y = y as usize;
            let width_usize = width as usize;
            let height_usize = height as usize;
            with_gray_scratch(
                width,
                height,
                "image.luma_crop_scratch",
                |dst| {
                    for row in 0..height_usize {
                        let src_row = ((y + row) * stride + x) * 3;
                        let dst_row = row * width_usize;
                        rgb_to_luma_into(&mut dst[dst_row..dst_row + width_usize], &raw[src_row..src_row + width_usize * 3]);
                    }
                },
                f,
            )
        }
        DynamicImage::ImageRgba8(rgba) => {
            let raw = rgba.as_raw();
            let stride = rgba.width() as usize;
            let x = x as usize;
            let y = y as usize;
            let width_usize = width as usize;
            let height_usize = height as usize;
            with_gray_scratch(
                width,
                height,
                "image.luma_crop_scratch",
                |dst| {
                    for row in 0..height_usize {
                        let src_row = ((y + row) * stride + x) * 4;
                        let dst_row = row * width_usize;
                        rgba_to_luma_into(&mut dst[dst_row..dst_row + width_usize], &raw[src_row..src_row + width_usize * 4]);
                    }
                },
                f,
            )
        }
        other => {
            let gray = other.crop_imm(x, y, width, height).to_luma8();
            f(&gray)
        }
    }
}

pub fn with_luma8_frame<R>(frame: &DynamicImage, f: impl FnOnce(&GrayImage) -> R) -> R {
    match frame {
        DynamicImage::ImageLuma8(gray) => f(gray),
        DynamicImage::ImageLumaA8(gray) => {
            let (width, height) = gray.dimensions();
            let src = gray.as_raw();
            let needed = (width as usize).saturating_mul(height as usize);
            with_luma_scratch(needed, "image.luma_scratch", |buf| {
                for (dst, src) in buf.iter_mut().zip(src.chunks_exact(2)) {
                    *dst = src[0];
                }
                let img = GrayImage::from_raw(width, height, std::mem::take(buf)).unwrap_or_else(|| GrayImage::new(width, height));
                let out = f(&img);
                *buf = img.into_raw();
                out
            })
        }
        DynamicImage::ImageRgb8(rgb) => {
            let (width, height) = rgb.dimensions();
            let src = rgb.as_raw();
            let needed = (width as usize).saturating_mul(height as usize);
            with_luma_scratch(needed, "image.luma_scratch", |buf| {
                rgb_to_luma_into(buf.as_mut_slice(), src);
                let img = GrayImage::from_raw(width, height, std::mem::take(buf)).unwrap_or_else(|| GrayImage::new(width, height));
                let out = f(&img);
                *buf = img.into_raw();
                out
            })
        }
        DynamicImage::ImageRgba8(rgba) => {
            let (width, height) = rgba.dimensions();
            let src = rgba.as_raw();
            let needed = (width as usize).saturating_mul(height as usize);
            with_luma_scratch(needed, "image.luma_scratch", |buf| {
                rgba_to_luma_into(buf.as_mut_slice(), src);
                let img = GrayImage::from_raw(width, height, std::mem::take(buf)).unwrap_or_else(|| GrayImage::new(width, height));
                let out = f(&img);
                *buf = img.into_raw();
                out
            })
        }
        other => {
            let gray_owned = other.to_luma8();
            f(&gray_owned)
        }
    }
}
