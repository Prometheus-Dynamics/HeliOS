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
/// foreground. A running sum per column over the window's rows, plus a
/// prefix sum along each row, gives every window sum in O(1). The test
/// `pixel + offset < sum / count` is done as `(pixel + offset) * count < sum`;
/// away from the borders `count` is constant, so the inner loop vectorises.
pub fn adaptive_threshold(gray: &GrayImage, config: &ThresholdConfig) -> BinaryImage {
    let (width, height) = (gray.width(), gray.height());
    let radius = config.window.max(3) / 2;
    let offset = config.offset;
    let data = gray.data();
    let mut out = vec![0u8; width * height];
    if width == 0 || height == 0 {
        return BinaryImage::new(width, height, out).expect("size matches");
    }

    // Column sums over rows [0, radius] to start with.
    let mut columns = vec![0u32; width];
    for row in data.chunks_exact(width).take(radius.min(height - 1) + 1) {
        for (sum, &p) in columns.iter_mut().zip(row) {
            *sum += p as u32;
        }
    }
    let mut prefix = vec![0u32; width + 1];
    for y in 0..height {
        if y > 0 {
            if let Some(add) = (y + radius < height).then(|| &data[(y + radius) * width..(y + radius + 1) * width]) {
                for (sum, &p) in columns.iter_mut().zip(add) {
                    *sum += p as u32;
                }
            }
            if y > radius {
                let remove = &data[(y - radius - 1) * width..(y - radius) * width];
                for (sum, &p) in columns.iter_mut().zip(remove) {
                    *sum -= p as u32;
                }
            }
        }
        let rows = ((y + radius).min(height - 1) + 1 - y.saturating_sub(radius)) as i32;
        let mut running = 0u32;
        for (x, &column) in columns.iter().enumerate() {
            running += column;
            prefix[x + 1] = running;
        }

        let pixels = &data[y * width..(y + 1) * width];
        let out_row = &mut out[y * width..(y + 1) * width];
        let test = |x: usize| {
            let (x0, x1) = (x.saturating_sub(radius), (x + radius + 1).min(width));
            let sum = (prefix[x1] - prefix[x0]) as i32;
            u8::from((pixels[x] as i32 + offset) * rows * ((x1 - x0) as i32) < sum)
        };
        let full = 2 * radius + 1;
        if width <= full {
            for (x, value) in out_row.iter_mut().enumerate() {
                *value = test(x);
            }
            continue;
        }
        let count = rows * full as i32;
        for x in (0..radius).chain(width - radius..width) {
            out_row[x] = test(x);
        }
        let interior = radius..width - radius;
        let sums = prefix[interior.start + radius + 1..].iter().zip(&prefix[interior.start - radius..]).map(|(hi, lo)| (hi - lo) as i32);
        for ((value, &p), sum) in out_row[interior.clone()].iter_mut().zip(&pixels[interior]).zip(sums) {
            *value = u8::from((p as i32 + offset) * count < sum);
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

    /// The straightforward definition, for comparison.
    fn reference(gray: &GrayImage, config: &ThresholdConfig) -> Vec<u8> {
        let radius = config.window.max(3) / 2;
        let (w, h) = (gray.width(), gray.height());
        let mut out = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                let (mut sum, mut count) = (0i32, 0i32);
                for yy in y.saturating_sub(radius)..(y + radius + 1).min(h) {
                    for xx in x.saturating_sub(radius)..(x + radius + 1).min(w) {
                        sum += gray.get(xx, yy) as i32;
                        count += 1;
                    }
                }
                out[y * w + x] = u8::from((gray.get(x, y) as i32 + config.offset) * count < sum);
            }
        }
        out
    }

    #[test]
    fn matches_the_reference_definition() {
        let mut state = 7u32;
        for (w, h) in [(64, 48), (23, 31), (5, 7), (1, 1), (40, 3)] {
            let data = (0..w * h)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    (state % 256) as u8
                })
                .collect();
            let gray = GrayImage::new(w, h, data).unwrap();
            for config in [ThresholdConfig::default(), ThresholdConfig { window: 3, offset: -2 }, ThresholdConfig { window: 9, offset: 0 }] {
                assert_eq!(adaptive_threshold(&gray, &config).data(), reference(&gray, &config).as_slice(), "{w}x{h} {config:?}");
            }
        }
    }

    #[test]
    fn flat_image_has_no_foreground() {
        let gray = GrayImage::filled(30, 20, 128);
        assert_eq!(adaptive_threshold(&gray, &ThresholdConfig::default()).count_set(), 0);
    }
}
