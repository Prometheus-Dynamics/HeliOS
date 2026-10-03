//! Adaptive (local mean) thresholding.

use crate::image::{BinaryImage, GrayImage};

/// Parameters for [`adaptive_threshold`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThresholdConfig {
    /// Side of the square averaging window in pixels (made odd, at least 3).
    pub window: usize,
    /// A pixel is foreground when it is darker than the local mean by more
    /// than this many grey levels.
    pub offset: i32,
}

impl Default for ThresholdConfig {
    fn default() -> Self {
        Self { window: 23, offset: 7 }
    }
}

/// Mark pixels darker than their local mean by more than `offset` as
/// foreground. Uses an integral image, so the cost is independent of the
/// window size. The comparison `pixel + offset < sum / count` is done as
/// `(pixel + offset) * count < sum` to avoid a division per pixel.
pub fn adaptive_threshold(gray: &GrayImage, config: &ThresholdConfig) -> BinaryImage {
    let (width, height) = (gray.width(), gray.height());
    let radius = config.window.max(3) / 2;
    let stride = width + 1;
    let mut integral = vec![0u32; stride * (height + 1)];
    for y in 0..height {
        let row = &gray.data()[y * width..(y + 1) * width];
        let (above, current) = integral.split_at_mut((y + 1) * stride);
        let above = &above[y * stride..];
        let mut row_sum = 0u32;
        for x in 0..width {
            row_sum += row[x] as u32;
            current[x + 1] = above[x + 1] + row_sum;
        }
    }

    let offset = config.offset as i64;
    let mut out = vec![0u8; width * height];
    for y in 0..height {
        let y0 = y.saturating_sub(radius);
        let y1 = (y + radius + 1).min(height);
        let top = &integral[y0 * stride..(y0 + 1) * stride];
        let bottom = &integral[y1 * stride..(y1 + 1) * stride];
        let rows = (y1 - y0) as i64;
        let pixels = &gray.data()[y * width..(y + 1) * width];
        let out_row = &mut out[y * width..(y + 1) * width];
        for x in 0..width {
            let x0 = x.saturating_sub(radius);
            let x1 = (x + radius + 1).min(width);
            let sum = (bottom[x1] + top[x0]) as i64 - (top[x1] + bottom[x0]) as i64;
            let count = rows * (x1 - x0) as i64;
            out_row[x] = u8::from((pixels[x] as i64 + offset) * count < sum);
        }
    }
    BinaryImage::new(width, height, out).expect("size matches")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_square_on_light_background_is_foreground() {
        let mut gray = GrayImage::filled(40, 40, 220);
        for y in 15..25 {
            for x in 15..25 {
                gray.set(x, y, 30);
            }
        }
        let binary = adaptive_threshold(&gray, &ThresholdConfig::default());
        assert!(binary.is_set(15, 15));
        assert!(binary.is_set(24, 24));
        assert!(!binary.is_set(5, 5));
    }

    #[test]
    fn flat_image_has_no_foreground() {
        let gray = GrayImage::filled(30, 20, 128);
        assert_eq!(adaptive_threshold(&gray, &ThresholdConfig::default()).count_set(), 0);
    }
}
