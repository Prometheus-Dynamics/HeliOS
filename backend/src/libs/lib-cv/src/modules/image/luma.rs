use image::{DynamicImage, GrayImage};
use std::cell::RefCell;

thread_local! {
    static LUMA_SCRATCH: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
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

pub(crate) fn with_luma8_frame<R>(frame: &DynamicImage, f: impl FnOnce(&GrayImage) -> R) -> R {
    match frame {
        DynamicImage::ImageLuma8(gray) => f(gray),
        DynamicImage::ImageLumaA8(gray) => {
            let (width, height) = gray.dimensions();
            let src = gray.as_raw();
            LUMA_SCRATCH.with(|scratch| {
                let needed = (width as usize).saturating_mul(height as usize);
                match scratch.try_borrow_mut() {
                    Ok(mut buf) => {
                        if buf.len() != needed {
                            buf.resize(needed, 0);
                            crate::diagnostics::report_scratch_high_water("image.luma_scratch", buf.capacity());
                        }
                        for (dst, src) in buf.iter_mut().zip(src.chunks_exact(2)) {
                            *dst = src[0];
                        }
                        let img = GrayImage::from_raw(width, height, std::mem::take(&mut *buf)).unwrap_or_else(|| GrayImage::new(width, height));
                        let out = f(&img);
                        *buf = img.into_raw();
                        out
                    }
                    Err(_) => {
                        // Re-entrant conversion on this thread; avoid panicking on RefCell borrow.
                        let mut local = vec![0u8; needed];
                        for (dst, src) in local.iter_mut().zip(src.chunks_exact(2)) {
                            *dst = src[0];
                        }
                        let img = GrayImage::from_raw(width, height, local).unwrap_or_else(|| GrayImage::new(width, height));
                        f(&img)
                    }
                }
            })
        }
        DynamicImage::ImageRgb8(rgb) => {
            let (width, height) = rgb.dimensions();
            let src = rgb.as_raw();
            LUMA_SCRATCH.with(|scratch| {
                let needed = (width as usize).saturating_mul(height as usize);
                match scratch.try_borrow_mut() {
                    Ok(mut buf) => {
                        if buf.len() != needed {
                            buf.resize(needed, 0);
                            crate::diagnostics::report_scratch_high_water("image.luma_scratch", buf.capacity());
                        }
                        rgb_to_luma_into(buf.as_mut_slice(), src);
                        let img = GrayImage::from_raw(width, height, std::mem::take(&mut *buf)).unwrap_or_else(|| GrayImage::new(width, height));
                        let out = f(&img);
                        *buf = img.into_raw();
                        out
                    }
                    Err(_) => {
                        // Re-entrant conversion on this thread; avoid panicking on RefCell borrow.
                        let mut local = vec![0u8; needed];
                        rgb_to_luma_into(local.as_mut_slice(), src);
                        let img = GrayImage::from_raw(width, height, local).unwrap_or_else(|| GrayImage::new(width, height));
                        f(&img)
                    }
                }
            })
        }
        DynamicImage::ImageRgba8(rgba) => {
            let (width, height) = rgba.dimensions();
            let src = rgba.as_raw();
            LUMA_SCRATCH.with(|scratch| {
                let needed = (width as usize).saturating_mul(height as usize);
                match scratch.try_borrow_mut() {
                    Ok(mut buf) => {
                        if buf.len() != needed {
                            buf.resize(needed, 0);
                            crate::diagnostics::report_scratch_high_water("image.luma_scratch", buf.capacity());
                        }
                        rgba_to_luma_into(buf.as_mut_slice(), src);
                        let img = GrayImage::from_raw(width, height, std::mem::take(&mut *buf)).unwrap_or_else(|| GrayImage::new(width, height));
                        let out = f(&img);
                        *buf = img.into_raw();
                        out
                    }
                    Err(_) => {
                        // Re-entrant conversion on this thread; avoid panicking on RefCell borrow.
                        let mut local = vec![0u8; needed];
                        rgba_to_luma_into(local.as_mut_slice(), src);
                        let img = GrayImage::from_raw(width, height, local).unwrap_or_else(|| GrayImage::new(width, height));
                        f(&img)
                    }
                }
            })
        }
        other => {
            let gray_owned = other.to_luma8();
            f(&gray_owned)
        }
    }
}
