//! Owned single-channel images used by the CPU vision stages.

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ImageError {
    #[error("image of {width}x{height} needs {expected} bytes, got {actual}")]
    SizeMismatch { width: usize, height: usize, expected: usize, actual: usize },
    #[error("row stride {stride} is smaller than the width {width}")]
    StrideTooSmall { width: usize, stride: usize },
}

/// An 8-bit grayscale image, row-major with no padding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrayImage {
    width: usize,
    height: usize,
    data: Vec<u8>,
}

impl GrayImage {
    pub fn new(width: usize, height: usize, data: Vec<u8>) -> Result<Self, ImageError> {
        let expected = width * height;
        if data.len() != expected {
            return Err(ImageError::SizeMismatch { width, height, expected, actual: data.len() });
        }
        Ok(Self { width, height, data })
    }

    /// A uniform image.
    pub fn filled(width: usize, height: usize, value: u8) -> Self {
        Self { width, height, data: vec![value; width * height] }
    }

    /// Copy the visible part of a strided plane (e.g. a camera luma plane).
    pub fn from_strided(width: usize, height: usize, stride: usize, plane: &[u8]) -> Result<Self, ImageError> {
        if stride < width {
            return Err(ImageError::StrideTooSmall { width, stride });
        }
        let needed = if height == 0 { 0 } else { stride * (height - 1) + width };
        if plane.len() < needed {
            return Err(ImageError::SizeMismatch { width, height, expected: needed, actual: plane.len() });
        }
        let mut data = Vec::with_capacity(width * height);
        for row in 0..height {
            data.extend_from_slice(&plane[row * stride..row * stride + width]);
        }
        Ok(Self { width, height, data })
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }

    pub fn get(&self, x: usize, y: usize) -> u8 {
        self.data[y * self.width + x]
    }

    pub fn set(&mut self, x: usize, y: usize, value: u8) {
        self.data[y * self.width + x] = value;
    }

    /// Bilinear sample at a sub-pixel position (pixel centres are at integer
    /// coordinates). Positions outside the image are clamped to the edge.
    pub fn sample(&self, x: f32, y: f32) -> f32 {
        if self.width == 0 || self.height == 0 {
            return 0.0;
        }
        let max_x = (self.width - 1) as f32;
        let max_y = (self.height - 1) as f32;
        let x = x.clamp(0.0, max_x);
        let y = y.clamp(0.0, max_y);
        let x0 = x.floor() as usize;
        let y0 = y.floor() as usize;
        let x1 = (x0 + 1).min(self.width - 1);
        let y1 = (y0 + 1).min(self.height - 1);
        let fx = x - x0 as f32;
        let fy = y - y0 as f32;
        let top = self.get(x0, y0) as f32 * (1.0 - fx) + self.get(x1, y0) as f32 * fx;
        let bottom = self.get(x0, y1) as f32 * (1.0 - fx) + self.get(x1, y1) as f32 * fx;
        top * (1.0 - fy) + bottom * fy
    }
}

/// A binary mask, row-major: 1 marks a foreground (dark) pixel, 0 background.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BinaryImage {
    width: usize,
    height: usize,
    data: Vec<u8>,
}

impl BinaryImage {
    pub fn new(width: usize, height: usize, data: Vec<u8>) -> Result<Self, ImageError> {
        let expected = width * height;
        if data.len() != expected {
            return Err(ImageError::SizeMismatch { width, height, expected, actual: data.len() });
        }
        Ok(Self { width, height, data })
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// Foreground test with out-of-bounds pixels treated as background.
    pub fn is_set(&self, x: isize, y: isize) -> bool {
        x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height && self.data[y as usize * self.width + x as usize] != 0
    }

    pub fn count_set(&self) -> usize {
        self.data.iter().filter(|&&v| v != 0).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_strided_drops_row_padding() {
        let plane = [1, 2, 9, 3, 4, 9];
        let image = GrayImage::from_strided(2, 2, 3, &plane).unwrap();
        assert_eq!(image.data(), &[1, 2, 3, 4]);
    }

    #[test]
    fn sample_interpolates_between_pixels() {
        let image = GrayImage::new(2, 1, vec![0, 100]).unwrap();
        assert_eq!(image.sample(0.5, 0.0), 50.0);
        assert_eq!(image.sample(-3.0, 0.0), 0.0);
    }
}
