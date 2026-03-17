use image::{DynamicImage, GrayImage};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::{Mutex, OnceLock};

#[cfg(feature = "gpu")]
use daedalus::gpu::{GpuContextHandle, GpuError, GpuImageHandle, GpuSendable, upload_r8_texture, upload_rgba8_texture};

thread_local! {
    static LUMA_SCRATCH: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

const POOLED_GRAY_IMAGE_MAX_PER_SIZE: usize = 1;

fn pooled_gray_image_store() -> &'static Mutex<HashMap<usize, Vec<Vec<u8>>>> {
    static STORE: OnceLock<Mutex<HashMap<usize, Vec<Vec<u8>>>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn take_pooled_gray_bytes(len: usize) -> Vec<u8> {
    let Some(bytes) = pooled_gray_image_store().lock().ok().and_then(|mut guard| guard.get_mut(&len).and_then(Vec::pop)) else {
        return vec![0u8; len];
    };
    let mut bytes = bytes;
    if bytes.len() != len {
        bytes.resize(len, 0);
    } else {
        bytes.fill(0);
    }
    bytes
}

fn recycle_pooled_gray_bytes(mut bytes: Vec<u8>) {
    let len = bytes.len();
    if len == 0 {
        return;
    }
    if bytes.capacity() > len.saturating_mul(2) {
        bytes.shrink_to(len);
    }
    if let Ok(mut guard) = pooled_gray_image_store().lock() {
        let slot = guard.entry(len).or_default();
        if slot.len() < POOLED_GRAY_IMAGE_MAX_PER_SIZE {
            slot.push(bytes);
        }
    }
}

pub struct PooledGrayImage {
    image: Option<GrayImage>,
}

impl PooledGrayImage {
    pub fn new(width: u32, height: u32) -> Self {
        let len = (width as usize).saturating_mul(height as usize);
        let bytes = take_pooled_gray_bytes(len);
        let image = GrayImage::from_raw(width, height, bytes).unwrap_or_else(|| GrayImage::new(width, height));
        Self { image: Some(image) }
    }

    pub fn as_gray(&self) -> &GrayImage {
        self.image.as_ref().expect("pooled gray image missing inner image")
    }

    pub fn as_mut_gray(&mut self) -> &mut GrayImage {
        self.image.as_mut().expect("pooled gray image missing inner image")
    }

    pub fn into_inner(mut self) -> GrayImage {
        self.image.take().expect("pooled gray image missing inner image")
    }
}

impl Clone for PooledGrayImage {
    fn clone(&self) -> Self {
        let gray = self.as_gray();
        let mut out = Self::new(gray.width(), gray.height());
        out.as_mut_gray().as_mut().copy_from_slice(gray.as_raw());
        out
    }
}

impl Deref for PooledGrayImage {
    type Target = GrayImage;

    fn deref(&self) -> &Self::Target {
        self.as_gray()
    }
}

impl AsRef<GrayImage> for PooledGrayImage {
    fn as_ref(&self) -> &GrayImage {
        self.as_gray()
    }
}

impl Drop for PooledGrayImage {
    fn drop(&mut self) {
        let Some(image) = self.image.take() else {
            return;
        };
        recycle_pooled_gray_bytes(image.into_raw());
    }
}

#[cfg(feature = "gpu")]
impl GpuSendable for PooledGrayImage {
    type GpuRepr = GpuImageHandle;

    fn upload(self, ctx: &GpuContextHandle) -> Result<Self::GpuRepr, GpuError> {
        let gray = self.as_gray();
        let (width, height) = gray.dimensions();
        if ctx.capabilities().supported_formats.iter().any(|f| matches!(f, daedalus::gpu::GpuFormat::R8Unorm)) {
            return upload_r8_texture(ctx, width, height, gray.as_raw());
        }

        let mut rgba = Vec::with_capacity((width as usize).saturating_mul(height as usize).saturating_mul(4));
        for &v in gray.as_raw() {
            rgba.extend_from_slice(&[v, v, v, 255]);
        }
        upload_rgba8_texture(ctx, width, height, &rgba)
    }

    fn download(gpu: &Self::GpuRepr, ctx: &GpuContextHandle) -> Result<Self, GpuError> {
        let bytes = ctx.read_texture(gpu)?;
        let mut out = PooledGrayImage::new(gpu.width, gpu.height);
        let dst = out.as_mut_gray().as_mut();
        match gpu.format {
            daedalus::gpu::GpuFormat::R8Unorm => {
                if dst.len() != bytes.len() {
                    return Err(GpuError::AllocationFailed);
                }
                dst.copy_from_slice(&bytes);
            }
            _ => {
                if dst.len().saturating_mul(4) != bytes.len() {
                    return Err(GpuError::AllocationFailed);
                }
                for (dst_px, rgba) in dst.iter_mut().zip(bytes.chunks_exact(4)) {
                    *dst_px = rgba[0];
                }
            }
        }
        Ok(out)
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
    LUMA_SCRATCH.with(|scratch| match scratch.try_borrow_mut() {
        Ok(mut buf) => {
            if buf.len() != needed {
                buf.resize(needed, 0);
                crate::diagnostics::report_scratch_high_water(label, buf.capacity());
            }
            fill(&mut buf);
            let img = GrayImage::from_raw(width, height, std::mem::take(&mut *buf)).unwrap_or_else(|| GrayImage::new(width, height));
            let out = f(&img);
            *buf = img.into_raw();
            out
        }
        Err(_) => {
            let mut local = vec![0u8; needed];
            fill(&mut local);
            let img = GrayImage::from_raw(width, height, local).unwrap_or_else(|| GrayImage::new(width, height));
            f(&img)
        }
    })
}

pub(crate) fn crop_luma8_frame(frame: &DynamicImage, x: u32, y: u32, width: u32, height: u32) -> PooledGrayImage {
    if width == 0 || height == 0 {
        return PooledGrayImage::new(0, 0);
    }

    match frame {
        DynamicImage::ImageLuma8(gray) => {
            let raw = gray.as_raw();
            let stride = gray.width() as usize;
            let x = x as usize;
            let y = y as usize;
            let width_usize = width as usize;
            let height_usize = height as usize;
            let mut out = PooledGrayImage::new(width, height);
            let out_raw = out.as_mut_gray().as_mut();
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
            let mut out = PooledGrayImage::new(width, height);
            let out_raw = out.as_mut_gray().as_mut();
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
            let mut out = PooledGrayImage::new(width, height);
            let out_raw = out.as_mut_gray().as_mut();
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
            let mut out = PooledGrayImage::new(width, height);
            let out_raw = out.as_mut_gray().as_mut();
            for row in 0..height_usize {
                let src_row = ((y + row) * stride + x) * 4;
                let dst_row = row * width_usize;
                rgba_to_luma_into(&mut out_raw[dst_row..dst_row + width_usize], &raw[src_row..src_row + width_usize * 4]);
            }
            out
        }
        other => {
            let gray = other.crop_imm(x, y, width, height).to_luma8();
            let mut out = PooledGrayImage::new(width, height);
            out.as_mut_gray().as_mut().copy_from_slice(gray.as_raw());
            out
        }
    }
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
