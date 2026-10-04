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

    /// A borrowed view of this image.
    pub fn view(&self) -> GrayView<'_> {
        GrayView { width: self.width, height: self.height, stride: self.width, data: &self.data }
    }

    /// Halve both dimensions by averaging 2x2 blocks; see [`GrayView::downscale2`].
    pub fn downscale2(&self) -> GrayImage {
        self.view().downscale2()
    }

    /// Bilinear sample; see [`GrayView::sample`].
    pub fn sample(&self, x: f32, y: f32) -> f32 {
        self.view().sample(x, y)
    }
}

/// A borrowed 8-bit grayscale image with a row stride, e.g. the luma plane
/// of a camera frame read in place.
#[derive(Clone, Copy, Debug)]
pub struct GrayView<'a> {
    width: usize,
    height: usize,
    stride: usize,
    data: &'a [u8],
}

impl<'a> GrayView<'a> {
    /// A view of `height` rows of `width` pixels, `stride` bytes apart.
    pub fn new(width: usize, height: usize, stride: usize, data: &'a [u8]) -> Result<Self, ImageError> {
        if stride < width {
            return Err(ImageError::StrideTooSmall { width, stride });
        }
        let needed = if height == 0 { 0 } else { stride * (height - 1) + width };
        if data.len() < needed {
            return Err(ImageError::SizeMismatch { width, height, expected: needed, actual: data.len() });
        }
        Ok(Self { width, height, stride, data })
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// Row `y` (exactly `width` pixels).
    pub fn row(&self, y: usize) -> &'a [u8] {
        &self.data[y * self.stride..][..self.width]
    }

    pub fn get(&self, x: usize, y: usize) -> u8 {
        self.data[y * self.stride + x]
    }

    /// Copy into an owned, unpadded image.
    pub fn to_image(&self) -> GrayImage {
        let mut data = Vec::with_capacity(self.width * self.height);
        for y in 0..self.height {
            data.extend_from_slice(self.row(y));
        }
        GrayImage { width: self.width, height: self.height, data }
    }

    /// Halve both dimensions by averaging 2x2 blocks (an odd last row or
    /// column is dropped). Pixel centres map as `full = 2 * half + 0.5`.
    pub fn downscale2(&self) -> GrayImage {
        let (w, h) = (self.width / 2, self.height / 2);
        let mut data = vec![0u8; w * h];
        for (y, out) in data.chunks_exact_mut(w.max(1)).enumerate().take(h) {
            let top = &self.row(2 * y)[..2 * w];
            let bottom = &self.row(2 * y + 1)[..2 * w];
            // Indexed form so LLVM emits de-interleaving vector loads.
            for x in 0..w {
                let sum = top[2 * x] as u16 + top[2 * x + 1] as u16 + bottom[2 * x] as u16 + bottom[2 * x + 1] as u16;
                out[x] = ((sum + 2) >> 2) as u8;
            }
        }
        GrayImage { width: w, height: h, data }
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
    fn downscale2_averages_blocks() {
        let image = GrayImage::new(4, 2, vec![0, 4, 8, 8, 4, 0, 8, 8]).unwrap();
        let half = image.downscale2();
        assert_eq!((half.width(), half.height()), (2, 1));
        assert_eq!(half.data(), &[2, 8]);
    }

    #[test]
    fn sample_interpolates_between_pixels() {
        let image = GrayImage::new(2, 1, vec![0, 100]).unwrap();
        assert_eq!(image.sample(0.5, 0.0), 50.0);
        assert_eq!(image.sample(-3.0, 0.0), 0.0);
    }
}
