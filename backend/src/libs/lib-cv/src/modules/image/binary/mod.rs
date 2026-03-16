use image::{DynamicImage, GrayImage, ImageBuffer, Rgb};
use rayon::prelude::*;
use std::cell::RefCell;
use std::mem::size_of;
use wide::{CmpGt, i16x16, u8x16};

thread_local! {
    static ADAPTIVE_BUFFERS: RefCell<AdaptiveBuffers> = RefCell::new(AdaptiveBuffers::default());
    static ADAPTIVE_COL_SUM_SCRATCH: RefCell<Vec<i32>> = const { RefCell::new(Vec::new()) };
    static MASK_BUFFER: RefCell<MaskBuffer> = RefCell::new(MaskBuffer::default());
}

#[cfg(target_arch = "aarch64")]
mod neon;

mod adaptive;
pub use adaptive::{adaptive_mean_threshold_fast, adaptive_mean_threshold_fast_with_invert, with_adaptive_mean_threshold_fast, with_adaptive_mean_threshold_fast_timed};

#[derive(Default)]
struct AdaptiveBuffers {
    hsum: Vec<u16>,
    col_sum: Vec<i32>,
}

#[derive(Default)]
struct MaskBuffer {
    width: u32,
    height: u32,
    buf: Vec<u8>,
}

pub fn binary_image(image: &DynamicImage, threshold: u8) -> GrayImage {
    match image {
        DynamicImage::ImageLuma8(gray) => binary_image_gray(gray, threshold),
        _ => binary_image_gray(&image.to_luma8(), threshold),
    }
}

pub fn binary_image_simd(image: &DynamicImage, threshold: u8) -> GrayImage {
    match image {
        DynamicImage::ImageLuma8(gray) => binary_image_gray_simd(gray, threshold),
        _ => binary_image_gray_simd(&image.to_luma8(), threshold),
    }
}

pub fn binary_image_gray(image: &GrayImage, threshold: u8) -> GrayImage {
    let (width, height) = (image.width(), image.height());
    let src_pixels = image.as_raw();

    let mut output = GrayImage::new(width, height);
    output.as_mut().par_iter_mut().zip(src_pixels.par_iter()).for_each(|(dst, &src)| {
        *dst = if src > threshold { 255u8 } else { 0u8 };
    });
    output
}

pub fn binary_image_gray_with_invert(image: &GrayImage, threshold: u8, invert: bool) -> GrayImage {
    let (width, height) = (image.width(), image.height());
    let src_pixels = image.as_raw();

    let mut output = GrayImage::new(width, height);
    threshold_into(output.as_mut(), src_pixels, threshold, invert);
    output
}

pub fn binary_image_slice(buffer: &ImageBuffer<Rgb<u8>, &[u8]>, threshold: u8) -> GrayImage {
    let (width, height) = (buffer.width(), buffer.height());
    let src_pixels = buffer.as_raw();

    let mut output = GrayImage::new(width, height);
    output.as_mut().par_iter_mut().zip(src_pixels.par_iter()).for_each(|(dst, &src)| {
        *dst = if src > threshold { 255u8 } else { 0u8 };
    });
    output
}

pub fn binary_image_gray_simd(image: &GrayImage, threshold: u8) -> GrayImage {
    let (width, height) = (image.width(), image.height());
    let src_pixels = image.as_raw();
    let mut output = GrayImage::new(width, height);
    let dst_pixels = output.as_mut();

    #[cfg(target_arch = "aarch64")]
    if crate::simd::neon_enabled() {
        // A single NEON pass avoids Rayon scheduling overhead on 1MP-class frames.
        // SAFETY: guarded by runtime feature detection.
        unsafe {
            neon::threshold_to_mask_neon(dst_pixels, src_pixels, threshold);
        }
        return output;
    }

    let threshold_simd = i16x16::splat(threshold as i16);
    let on = i16x16::splat(255);
    let off = i16x16::splat(0);

    const SIMD_WIDTH: usize = 16;
    let simd_len = src_pixels.len() / SIMD_WIDTH * SIMD_WIDTH;

    dst_pixels[..simd_len].par_chunks_mut(SIMD_WIDTH * 256).zip(src_pixels[..simd_len].par_chunks(SIMD_WIDTH * 256)).for_each(|(dst_chunk, src_chunk)| {
        let mut i = 0;
        while i + SIMD_WIDTH <= src_chunk.len() {
            let v = i16x16::from(u8x16::new(src_chunk[i..i + SIMD_WIDTH].try_into().unwrap()));
            let mask = v.simd_gt(threshold_simd);
            let out = mask.blend(on, off).to_array().map(|v| v as u8);
            dst_chunk[i..i + SIMD_WIDTH].copy_from_slice(&out);
            i += SIMD_WIDTH;
        }
    });

    for idx in simd_len..src_pixels.len() {
        dst_pixels[idx] = if src_pixels[idx] > threshold { 255 } else { 0 };
    }

    output
}

pub fn otsu_binarization(image: &DynamicImage) -> GrayImage {
    crate::modules::image::luma::with_luma8_frame(image, otsu_binarization_gray)
}

pub fn otsu_level(image: &DynamicImage) -> u8 {
    crate::modules::image::luma::with_luma8_frame(image, otsu_level_gray)
}

pub fn otsu_level_gray(image: &GrayImage) -> u8 {
    let src = image.as_raw();
    if src.is_empty() {
        return 0;
    }

    let mut hist = [0u32; 256];
    let mut chunks = src.chunks_exact(16);
    for chunk in &mut chunks {
        hist[chunk[0] as usize] += 1;
        hist[chunk[1] as usize] += 1;
        hist[chunk[2] as usize] += 1;
        hist[chunk[3] as usize] += 1;
        hist[chunk[4] as usize] += 1;
        hist[chunk[5] as usize] += 1;
        hist[chunk[6] as usize] += 1;
        hist[chunk[7] as usize] += 1;
        hist[chunk[8] as usize] += 1;
        hist[chunk[9] as usize] += 1;
        hist[chunk[10] as usize] += 1;
        hist[chunk[11] as usize] += 1;
        hist[chunk[12] as usize] += 1;
        hist[chunk[13] as usize] += 1;
        hist[chunk[14] as usize] += 1;
        hist[chunk[15] as usize] += 1;
    }
    for &v in chunks.remainder() {
        hist[v as usize] += 1;
    }

    otsu_level_from_hist(&hist, src.len() as u64)
}

fn otsu_level_from_hist(hist: &[u32; 256], total_count: u64) -> u8 {
    if total_count == 0 {
        return 0;
    }

    let mut sum_all = 0u64;
    for (i, &count) in hist.iter().enumerate() {
        sum_all += (i as u64) * (count as u64);
    }

    let mut best_threshold = 0u8;
    let mut max_between = -1.0f64;
    let mut weight_bg = 0u64;
    let mut sum_bg = 0u64;

    for (i, &count) in hist.iter().enumerate() {
        let c = count as u64;
        weight_bg += c;
        if weight_bg == 0 {
            continue;
        }

        let weight_fg = total_count.saturating_sub(weight_bg);
        if weight_fg == 0 {
            break;
        }

        sum_bg += (i as u64) * c;
        let lhs = total_count.saturating_mul(sum_bg);
        let rhs = sum_all.saturating_mul(weight_bg);
        let diff = lhs.abs_diff(rhs) as f64;
        let between = (diff * diff) / ((weight_bg as f64) * (weight_fg as f64));

        if between > max_between {
            max_between = between;
            best_threshold = i as u8;
        }
    }

    best_threshold
}

pub fn otsu_binarization_gray(image: &GrayImage) -> GrayImage {
    let threshold = otsu_level_gray(image);
    binary_image_gray_simd(image, threshold)
}

pub fn otsu_binarization_gray_with_invert(image: &GrayImage, invert: bool) -> GrayImage {
    let threshold = otsu_level_gray(image);
    binary_image_gray_with_invert(image, threshold, invert)
}

pub fn otsu_binarization_simd(image: &DynamicImage) -> GrayImage {
    match image {
        DynamicImage::ImageLuma8(gray) => otsu_binarization_gray_simd(gray),
        _ => otsu_binarization_gray_simd(&image.to_luma8()),
    }
}

pub fn otsu_binarization_gray_simd(image: &GrayImage) -> GrayImage {
    let threshold = otsu_level_gray(image);
    binary_image_gray_simd(image, threshold)
}

fn ensure_mask_buffer(mask: &mut MaskBuffer, width: u32, height: u32) -> &mut [u8] {
    let needed = (width as usize).saturating_mul(height as usize);
    if mask.width != width || mask.height != height || mask.buf.len() != needed {
        mask.width = width;
        mask.height = height;
        mask.buf.resize(needed, 0);
        crate::diagnostics::report_scratch_high_water("image.binary_mask", mask.buf.capacity() * size_of::<u8>());
    }
    &mut mask.buf[..]
}

fn threshold_into(dst: &mut [u8], src: &[u8], threshold: u8, invert: bool) {
    #[cfg(target_arch = "aarch64")]
    if crate::simd::neon_enabled() {
        // SAFETY: guarded by runtime feature detection.
        unsafe {
            neon::threshold_to_mask_with_invert_neon(dst, src, threshold, invert);
        }
        return;
    }
    if invert {
        dst.par_iter_mut().zip(src.par_iter()).for_each(|(d, &s)| {
            *d = if s > threshold { 0 } else { 255 };
        });
    } else {
        dst.par_iter_mut().zip(src.par_iter()).for_each(|(d, &s)| {
            *d = if s > threshold { 255 } else { 0 };
        });
    }
}

pub fn with_binary_threshold_mask<R>(gray: &GrayImage, threshold: u8, invert: bool, f: impl FnOnce(&GrayImage) -> R) -> R {
    let (width, height) = gray.dimensions();
    let src = gray.as_raw();
    MASK_BUFFER.with(|mask| {
        let mut mask = mask.borrow_mut();
        let dst = ensure_mask_buffer(&mut mask, width, height);
        threshold_into(dst, src, threshold, invert);

        let img = GrayImage::from_raw(width, height, std::mem::take(&mut mask.buf)).expect("mask buffer size must match image dimensions");
        let out = f(&img);
        mask.buf = img.into_raw();
        out
    })
}
