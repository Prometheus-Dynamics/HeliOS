use std::error::Error;
use std::path::{Path, PathBuf};

use image::GrayImage;

pub(crate) type OpencvCompareStats = (Option<f32>, Option<f32>, Option<u64>, Option<u64>, Option<u64>, Option<u64>, Option<u64>, Option<u64>);

pub(crate) fn invert_gray(mask: &GrayImage) -> GrayImage {
    let mut out = mask.clone();
    for v in out.as_mut() {
        *v = 255u8.saturating_sub(*v);
    }
    out
}

pub(crate) fn compare_with_opencv(opencv_dir: &str, mask: &GrayImage, mask_inverted: &GrayImage, adaptive_window: u32) -> Result<OpencvCompareStats, Box<dyn Error>> {
    let opencv_dir = PathBuf::from(opencv_dir);
    let mask_path = opencv_dir.join("opencv_mask.png");
    let mask_inv_path = opencv_dir.join("opencv_mask_inverted.png");

    let (mad_mask, diff_pixels_mask, diff_border_mask, diff_interior_mask) = if mask_path.exists() {
        let other = image::open(mask_path)?.to_luma8();
        let (diff_total, diff_border, diff_interior) = diff_pixels_with_border(mask, &other, adaptive_window);
        write_diff_mask(mask, &other, &opencv_dir.join("mask_diff_libcv_vs_opencv.png"))?;
        (Some(mean_abs_diff(mask, &other)), Some(diff_total), Some(diff_border), Some(diff_interior))
    } else {
        (None, None, None, None)
    };
    let (mad_mask_inverted, diff_pixels_mask_inverted, diff_border_mask_inverted, diff_interior_mask_inverted) = if mask_inv_path.exists() {
        let other = image::open(mask_inv_path)?.to_luma8();
        let (diff_total, diff_border, diff_interior) = diff_pixels_with_border(mask_inverted, &other, adaptive_window);
        write_diff_mask(mask_inverted, &other, &opencv_dir.join("mask_inverted_diff_libcv_vs_opencv.png"))?;
        (Some(mean_abs_diff(mask_inverted, &other)), Some(diff_total), Some(diff_border), Some(diff_interior))
    } else {
        (None, None, None, None)
    };
    Ok((mad_mask, mad_mask_inverted, diff_pixels_mask, diff_pixels_mask_inverted, diff_border_mask, diff_interior_mask, diff_border_mask_inverted, diff_interior_mask_inverted))
}

pub(crate) fn compare_reference_with_opencv(opencv_dir: &str, mask_ref: &GrayImage, mask_ref_inverted: &GrayImage) -> Result<(Option<f32>, Option<u64>), Box<dyn Error>> {
    let opencv_dir = PathBuf::from(opencv_dir);
    let mask_path = opencv_dir.join("opencv_mask.png");
    let mask_inv_path = opencv_dir.join("opencv_mask_inverted.png");
    let mut mad = None;
    let mut diff = None;
    if mask_path.exists() {
        let other = image::open(mask_path)?.to_luma8();
        let (mad_val, diff_val) = compare_two_masks(mask_ref, &other);
        mad = mad_val;
        diff = diff_val;
    }
    if mask_inv_path.exists() {
        let other = image::open(mask_inv_path)?.to_luma8();
        let (mad_val, diff_val) = compare_two_masks(mask_ref_inverted, &other);
        if mad.is_some() {
            mad = mad.zip(mad_val).map(|(a, b)| a + b);
        } else {
            mad = mad_val;
        }
        if diff.is_some() {
            diff = diff.zip(diff_val).map(|(a, b)| a + b);
        } else {
            diff = diff_val;
        }
    }
    Ok((mad, diff))
}

pub(crate) fn mean_abs_diff(a: &GrayImage, b: &GrayImage) -> f32 {
    if a.width() != b.width() || a.height() != b.height() {
        return f32::NAN;
    }
    let mut sum = 0u64;
    for (va, vb) in a.as_raw().iter().zip(b.as_raw().iter()) {
        let diff = (*va as i16 - *vb as i16).unsigned_abs() as u64;
        sum += diff;
    }
    let denom = a.as_raw().len().max(1) as f32;
    sum as f32 / denom
}

pub(crate) fn compare_two_masks(a: &GrayImage, b: &GrayImage) -> (Option<f32>, Option<u64>) {
    if a.width() != b.width() || a.height() != b.height() {
        return (None, None);
    }
    let mad = mean_abs_diff(a, b);
    let diff = diff_pixels_total(a, b);
    (Some(mad), Some(diff))
}

fn diff_pixels_total(a: &GrayImage, b: &GrayImage) -> u64 {
    let mut diff = 0u64;
    for (va, vb) in a.as_raw().iter().zip(b.as_raw().iter()) {
        if va != vb {
            diff += 1;
        }
    }
    diff
}

fn diff_pixels_with_border(a: &GrayImage, b: &GrayImage, window: u32) -> (u64, u64, u64) {
    if a.width() != b.width() || a.height() != b.height() {
        return (0, 0, 0);
    }
    let radius = window.saturating_sub(1) / 2;
    let width = a.width();
    let height = a.height();
    let mut diff_total = 0u64;
    let mut diff_border = 0u64;
    let mut diff_interior = 0u64;
    for y in 0..height {
        for x in 0..width {
            let idx = (y * width + x) as usize;
            if a.as_raw()[idx] != b.as_raw()[idx] {
                diff_total += 1;
                let is_border = x < radius || y < radius || x + radius >= width || y + radius >= height;
                if is_border {
                    diff_border += 1;
                } else {
                    diff_interior += 1;
                }
            }
        }
    }
    (diff_total, diff_border, diff_interior)
}

fn write_diff_mask(a: &GrayImage, b: &GrayImage, path: &Path) -> Result<(), Box<dyn Error>> {
    if a.width() != b.width() || a.height() != b.height() {
        return Ok(());
    }
    let mut out = GrayImage::new(a.width(), a.height());
    for (idx, pixel) in out.as_mut().iter_mut().enumerate() {
        let diff = a.as_raw()[idx] != b.as_raw()[idx];
        *pixel = if diff { 255 } else { 0 };
    }
    out.save(path)?;
    Ok(())
}

pub(crate) fn adaptive_mean_threshold_reference(image: &GrayImage, window: u32, offset: f32) -> GrayImage {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return GrayImage::new(width, height);
    }
    let window = ensure_odd_window(window);
    let radius = (window / 2) as i32;
    let padded = pad_replicate(image, radius);
    let (padded_w, _padded_h) = padded.dimensions();
    let integral = integral_image_u32(&padded);
    let mut out = GrayImage::new(width, height);
    let area = (window as f32) * (window as f32);
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let px = x + radius;
            let py = y + radius;
            let x0 = (px - radius) as u32;
            let y0 = (py - radius) as u32;
            let x1 = (px + radius + 1) as u32;
            let y1 = (py + radius + 1) as u32;
            let sum = rect_sum(&integral, padded_w, x0, y0, x1, y1) as f32;
            let mean = sum / area;
            let src = image.get_pixel(x as u32, y as u32).0[0] as f32;
            let threshold = mean - offset;
            let is_foreground = src > threshold;
            let v = if is_foreground { 255 } else { 0 };
            out.get_pixel_mut(x as u32, y as u32).0[0] = v;
        }
    }
    out
}

pub(crate) fn ensure_odd_window(value: u32) -> u32 {
    let candidate = value.clamp(3, 181);
    if candidate.is_multiple_of(2) { candidate + 1 } else { candidate }
}

fn pad_replicate(image: &GrayImage, radius: i32) -> GrayImage {
    let (width, height) = image.dimensions();
    let padded_w = width + (radius as u32) * 2;
    let padded_h = height + (radius as u32) * 2;
    let mut out = GrayImage::new(padded_w, padded_h);
    for y in 0..padded_h as i32 {
        for x in 0..padded_w as i32 {
            let src_x = (x - radius).clamp(0, width as i32 - 1) as u32;
            let src_y = (y - radius).clamp(0, height as i32 - 1) as u32;
            let v = image.get_pixel(src_x, src_y).0[0];
            out.get_pixel_mut(x as u32, y as u32).0[0] = v;
        }
    }
    out
}

fn integral_image_u32(image: &GrayImage) -> Vec<u32> {
    let (width, height) = image.dimensions();
    let mut integral = vec![0u32; (width as usize + 1) * (height as usize + 1)];
    for y in 0..height as usize {
        let mut row_sum = 0u32;
        for x in 0..width as usize {
            let v = image.get_pixel(x as u32, y as u32).0[0] as u32;
            row_sum += v;
            let idx = (y + 1) * (width as usize + 1) + (x + 1);
            integral[idx] = integral[(y) * (width as usize + 1) + (x + 1)] + row_sum;
        }
    }
    integral
}

fn rect_sum(integral: &[u32], width: u32, x0: u32, y0: u32, x1: u32, y1: u32) -> u32 {
    let w = width as usize + 1;
    let a = integral[(y0 as usize) * w + (x0 as usize)];
    let b = integral[(y0 as usize) * w + (x1 as usize)];
    let c = integral[(y1 as usize) * w + (x0 as usize)];
    let d = integral[(y1 as usize) * w + (x1 as usize)];
    d + a - b - c
}
