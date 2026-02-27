use image::{DynamicImage, GenericImageView, GrayImage, RgbaImage};
use rayon::prelude::*;

#[cfg(feature = "engine")]
pub mod nodes;

/// Convert an RGB pixel to HSV components.
pub fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let r = r as f32 / 255.0;
    let g = g as f32 / 255.0;
    let b = b as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let h = if delta == 0.0 {
        0.0
    } else if (max - r).abs() < f32::EPSILON {
        60.0 * (((g - b) / delta) % 6.0)
    } else if (max - g).abs() < f32::EPSILON {
        60.0 * (((b - r) / delta) + 2.0)
    } else {
        60.0 * (((r - g) / delta) + 4.0)
    };
    let h = if h < 0.0 { h + 360.0 } else { h };
    let s = if max == 0.0 { 0.0 } else { delta / max };
    let v = max;
    (h, s, v)
}

// Create a mask where pixels fall within the provided RGB range.
// (Single-range variants removed: use the multi-range helpers below.)

/// Apply a grayscale mask to an image returning a new RGBA image with masked pixels set to black.
pub fn apply_mask(frame: &DynamicImage, mask: &GrayImage) -> DynamicImage {
    if mask.as_flat_samples().samples.iter().all(|&v| v == 0) {
        let (w, h) = frame.dimensions();
        return DynamicImage::ImageRgba8(RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 255])));
    }
    if mask.as_flat_samples().samples.iter().all(|&v| v == 255) {
        return frame.clone();
    }
    let mut rgba = frame.to_rgba8();
    let buf = rgba.as_flat_samples_mut().samples;
    let mask_buf = mask.as_flat_samples().samples;
    buf.par_chunks_mut(4).zip(mask_buf.par_iter()).for_each(|(px, &m)| {
        if m == 0 {
            px[0] = 0;
            px[1] = 0;
            px[2] = 0;
        }
    });
    DynamicImage::ImageRgba8(rgba)
}

/// Merge multiple masks by taking the maximum value at each pixel.
pub fn merge_masks(masks: &[GrayImage]) -> GrayImage {
    assert!(!masks.is_empty());
    if masks.len() == 1 {
        return masks[0].clone();
    }
    let width = masks[0].width();
    let height = masks[0].height();
    let len = (width * height) as usize;

    let mut out = vec![0u8; len];
    out.par_iter_mut().enumerate().for_each(|(idx, v)| {
        let mut max_val = 0u8;
        for mask in masks {
            let buf = mask.as_flat_samples().samples;
            let val = buf[idx];
            if val > max_val {
                max_val = val;
                if max_val == 255 {
                    break;
                }
            }
        }
        *v = max_val;
    });

    GrayImage::from_raw(width, height, out).unwrap()
}

/// Create a mask from multiple RGB ranges in a single pass.
pub fn rgb_multi_range_mask(frame: &DynamicImage, ranges: &[(i32, i32, i32, i32, i32, i32)]) -> GrayImage {
    if ranges.is_empty() {
        let (w, h) = frame.dimensions();
        return GrayImage::new(w, h);
    }
    if ranges.iter().any(|&(r_min, r_max, g_min, g_max, b_min, b_max)| r_min <= 0 && r_max >= 255 && g_min <= 0 && g_max >= 255 && b_min <= 0 && b_max >= 255) {
        let (w, h) = frame.dimensions();
        return GrayImage::from_pixel(w, h, image::Luma([255u8]));
    }
    let rgb = frame.to_rgb8();
    let buf = rgb.as_flat_samples().samples;
    let len = rgb.width() * rgb.height();
    let mut mask = vec![0u8; len as usize];

    mask.par_iter_mut().enumerate().for_each(|(i, m)| {
        let r = buf[3 * i];
        let g = buf[3 * i + 1];
        let b = buf[3 * i + 2];
        for &(r_min, r_max, g_min, g_max, b_min, b_max) in ranges {
            if (r as i32) >= r_min && (r as i32) <= r_max && (g as i32) >= g_min && (g as i32) <= g_max && (b as i32) >= b_min && (b as i32) <= b_max {
                *m = 255;
                return;
            }
        }
    });

    GrayImage::from_raw(rgb.width(), rgb.height(), mask).unwrap()
}

/// Create a mask from multiple HSV ranges in a single pass.
pub fn hsv_multi_range_mask(frame: &DynamicImage, ranges: &[(f32, f32, f32, f32, f32, f32)]) -> GrayImage {
    if ranges.is_empty() {
        let (w, h) = frame.dimensions();
        return GrayImage::new(w, h);
    }
    if ranges.iter().any(|&(h_min, h_max, s_min, s_max, v_min, v_max)| h_min <= 0.0 && h_max >= 360.0 && s_min <= 0.0 && s_max >= 1.0 && v_min <= 0.0 && v_max >= 1.0) {
        let (w, h) = frame.dimensions();
        return GrayImage::from_pixel(w, h, image::Luma([255u8]));
    }
    let rgb = frame.to_rgb8();
    let buf = rgb.as_flat_samples().samples;
    let len = rgb.width() * rgb.height();
    let mut mask = vec![0u8; len as usize];

    mask.par_iter_mut().enumerate().for_each(|(i, m)| {
        let r = buf[3 * i];
        let g = buf[3 * i + 1];
        let b = buf[3 * i + 2];
        let (h, s, v) = rgb_to_hsv(r, g, b);
        for &(h_min, h_max, s_min, s_max, v_min, v_max) in ranges {
            if h >= h_min && h <= h_max && s >= s_min && s <= s_max && v >= v_min && v <= v_max {
                *m = 255;
                return;
            }
        }
    });

    GrayImage::from_raw(rgb.width(), rgb.height(), mask).unwrap()
}
