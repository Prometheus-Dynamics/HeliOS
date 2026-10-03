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
/// window size.
pub fn adaptive_threshold(gray: &GrayImage, config: &ThresholdConfig) -> BinaryImage {
    let (width, height) = (gray.width(), gray.height());
    let radius = (config.window.max(3) / 2) as isize;
    let stride = width + 1;
    let mut integral = vec![0u32; stride * (height + 1)];
    for y in 0..height {
        let mut row_sum = 0u32;
        let row = &gray.data()[y * width..(y + 1) * width];
        for (x, &value) in row.iter().enumerate() {
            row_sum += value as u32;
            integral[(y + 1) * stride + x + 1] = integral[y * stride + x + 1] + row_sum;
        }
    }

    let mut out = vec![0u8; width * height];
    for y in 0..height {
        let y0 = (y as isize - radius).max(0) as usize;
        let y1 = ((y as isize + radius + 1) as usize).min(height);
        for x in 0..width {
            let x0 = (x as isize - radius).max(0) as usize;
            let x1 = ((x as isize + radius + 1) as usize).min(width);
            let sum = integral[y1 * stride + x1] + integral[y0 * stride + x0] - integral[y0 * stride + x1] - integral[y1 * stride + x0];
            let count = ((x1 - x0) * (y1 - y0)) as u32;
            let mean = (sum / count) as i32;
            if (gray.get(x, y) as i32) < mean - config.offset {
                out[y * width + x] = 1;
            }
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
