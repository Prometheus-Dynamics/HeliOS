//! Utilities for working with 1-bit images. [`BinaryImage`] can be converted to
//! and from `GrayImage`.
//!
//! ```
//! use lib_cv::BinaryImage;
//!
//! let mut img = BinaryImage::new(2, 1);
//! img.set(1, 0, true);
//! assert!(img.get(1, 0));
//! ```

use image::GrayImage;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Packed representation of a binary (black and white) image.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct BinaryImage {
    pub width: u32,
    pub height: u32,
    #[serde(with = "serde_bytes")]
    data: Vec<u8>,
}

impl BinaryImage {
    /// Create a new empty binary image with all pixels set to 0.
    pub fn new(width: u32, height: u32) -> Self {
        let len = (width as usize * height as usize).div_ceil(8);
        Self { width, height, data: vec![0; len] }
    }

    /// Set the pixel at `(x, y)`.
    pub fn set(&mut self, x: u32, y: u32, value: bool) {
        let idx = (y * self.width + x) as usize;
        let byte = idx / 8;
        let bit = idx % 8;
        if value {
            self.data[byte] |= 1 << bit;
        } else {
            self.data[byte] &= !(1 << bit);
        }
    }

    /// Get the pixel value at `(x, y)`.
    pub fn get(&self, x: u32, y: u32) -> bool {
        let idx = (y * self.width + x) as usize;
        let byte = idx / 8;
        let bit = idx % 8;
        (self.data[byte] & (1 << bit)) != 0
    }

    /// Convert from a `GrayImage`. Any non-zero value becomes `true`.
    pub fn from_gray(image: &GrayImage) -> Self {
        let (width, height) = (image.width(), image.height());
        let mut out = Self::new(width, height);
        for (i, &pix) in image.as_raw().iter().enumerate() {
            if pix != 0 {
                let byte = i / 8;
                let bit = i % 8;
                out.data[byte] |= 1 << bit;
            }
        }
        out
    }

    /// Convert this binary image back into a `GrayImage` with pixels 0 or 255.
    pub fn to_gray(&self) -> GrayImage {
        let mut img = GrayImage::new(self.width, self.height);
        for i in 0..(self.width as usize * self.height as usize) {
            let byte = i / 8;
            let bit = i % 8;
            img.as_mut()[i] = if (self.data[byte] & (1 << bit)) != 0 { 255 } else { 0 };
        }
        img
    }
}

impl From<&GrayImage> for BinaryImage {
    fn from(value: &GrayImage) -> Self {
        Self::from_gray(value)
    }
}

impl From<BinaryImage> for GrayImage {
    fn from(value: BinaryImage) -> Self {
        value.to_gray()
    }
}
