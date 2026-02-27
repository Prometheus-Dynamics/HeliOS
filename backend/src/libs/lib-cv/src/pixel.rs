//! RGBA pixel helpers with conversions to and from common packed formats.
//!
//! ```
//! use lib_cv::Pixel;
//!
//! let pixel = Pixel { r: 255, g: 0, b: 128, a: 200 };
//! let packed: u32 = pixel.into();
//! let unpacked = Pixel::from(packed);
//! assert_eq!(unpacked.a, 200);
//! ```

use image::Rgba;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, Default, PartialEq, Eq)]
pub struct Pixel {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl From<Rgba<u8>> for Pixel {
    fn from(value: Rgba<u8>) -> Self {
        Self { r: value[0], g: value[1], b: value[2], a: value[3] }
    }
}

impl From<Pixel> for Rgba<u8> {
    fn from(value: Pixel) -> Self {
        Rgba([value.r, value.g, value.b, value.a])
    }
}

impl From<u32> for Pixel {
    fn from(value: u32) -> Self {
        Self { r: ((value >> 24) & 0xFF) as u8, g: ((value >> 16) & 0xFF) as u8, b: ((value >> 8) & 0xFF) as u8, a: (value & 0xFF) as u8 }
    }
}

impl From<Pixel> for u32 {
    fn from(value: Pixel) -> Self {
        ((value.r as u32) << 24) | ((value.g as u32) << 16) | ((value.b as u32) << 8) | (value.a as u32)
    }
}
