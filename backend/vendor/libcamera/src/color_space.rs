use std::ffi::CString;

use libcamera_sys::*;

/// Color primaries
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Primaries {
    Raw,
    Smpte170m,
    Rec709,
    Rec2020,
}

/// Transfer function
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferFunction {
    Linear,
    Srgb,
    Rec709,
}

/// YCbCr encoding
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YcbcrEncoding {
    None,
    Rec601,
    Rec709,
    Rec2020,
}

/// Color range
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    Full,
    Limited,
}

/// Represents `libcamera::ColorSpace`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColorSpace {
    pub primaries: Primaries,
    pub transfer_function: TransferFunction,
    pub ycbcr_encoding: YcbcrEncoding,
    pub range: Range,
}

impl ColorSpace {
    pub const fn new(
        primaries: Primaries,
        transfer_function: TransferFunction,
        ycbcr_encoding: YcbcrEncoding,
        range: Range,
    ) -> Self {
        Self {
            primaries,
            transfer_function,
            ycbcr_encoding,
            range,
        }
    }

    // Predefined color spaces from libcamera
    pub fn raw() -> Self {
        unsafe { libcamera_color_space_raw() }.into()
    }
    pub fn srgb() -> Self {
        unsafe { libcamera_color_space_srgb() }.into()
    }
    pub fn sycc() -> Self {
        unsafe { libcamera_color_space_sycc() }.into()
    }
    pub fn smpte170m() -> Self {
        unsafe { libcamera_color_space_smpte170m() }.into()
    }
    pub fn rec709() -> Self {
        unsafe { libcamera_color_space_rec709() }.into()
    }
    pub fn rec2020() -> Self {
        unsafe { libcamera_color_space_rec2020() }.into()
    }

    /// Returns libcamera string representation (e.g. "Smpte170m/Rec709/Full").
    pub fn to_repr(&self) -> String {
        unsafe {
            let ptr = libcamera_color_space_to_string(&(*self).into());
            if ptr.is_null() {
                return String::new();
            }
            let s = std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned();
            libc::free(ptr.cast());
            s
        }
    }

    /// Parse color space from libcamera string representation. Returns None on failure.
    pub fn from_string(s: &str) -> Option<Self> {
        let cstr = CString::new(s).ok()?;
        let mut cs = libcamera_color_space_t {
            primaries: libcamera_color_space_primaries::LIBCAMERA_COLOR_SPACE_PRIMARIES_RAW,
            transfer_function: libcamera_color_space_transfer_function::LIBCAMERA_COLOR_SPACE_TRANSFER_FUNCTION_LINEAR,
            ycbcr_encoding: libcamera_color_space_ycbcr_encoding::LIBCAMERA_COLOR_SPACE_YCBCR_ENCODING_NONE,
            range: libcamera_color_space_range::LIBCAMERA_COLOR_SPACE_RANGE_FULL,
        };
        let ok = unsafe { libcamera_color_space_from_string(cstr.as_ptr(), &mut cs) };
        if ok {
            Some(ColorSpace::from(cs))
        } else {
            None
        }
    }

    /// Adjust this color space for a given pixel format.
    ///
    /// Returns `true` if the color space was modified to make it compatible with the pixel format,
    /// or `false` if it was already compatible.
    pub fn adjust_for_format(&mut self, pixel_format: crate::pixel_format::PixelFormat) -> bool {
        let mut cs = (*self).into();
        let adjusted = unsafe { libcamera_color_space_adjust(&mut cs, &pixel_format.0) };
        *self = cs.into();
        adjusted
    }

    /// Returns a clone of this color space adjusted for the given pixel format.
    ///
    /// libcamera always treats the color space as compatible (possibly changing it), so this
    /// will always return `Some`.
    pub fn with_adjusted_for_format(&self, pixel_format: crate::pixel_format::PixelFormat) -> Option<Self> {
        let mut clone = *self;
        let _ = clone.adjust_for_format(pixel_format);
        Some(clone)
    }
}

impl core::fmt::Display for ColorSpace {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.to_repr())
    }
}

impl From<ColorSpace> for libcamera_color_space_t {
    fn from(cs: ColorSpace) -> Self {
        unsafe {
            libcamera_color_space_make(
                match cs.primaries {
                    Primaries::Raw => libcamera_color_space_primaries::LIBCAMERA_COLOR_SPACE_PRIMARIES_RAW,
                    Primaries::Smpte170m => libcamera_color_space_primaries::LIBCAMERA_COLOR_SPACE_PRIMARIES_SMPTE170M,
                    Primaries::Rec709 => libcamera_color_space_primaries::LIBCAMERA_COLOR_SPACE_PRIMARIES_REC709,
                    Primaries::Rec2020 => libcamera_color_space_primaries::LIBCAMERA_COLOR_SPACE_PRIMARIES_REC2020,
                },
                match cs.transfer_function {
                    TransferFunction::Linear => {
                        libcamera_color_space_transfer_function::LIBCAMERA_COLOR_SPACE_TRANSFER_FUNCTION_LINEAR
                    }
                    TransferFunction::Srgb => {
                        libcamera_color_space_transfer_function::LIBCAMERA_COLOR_SPACE_TRANSFER_FUNCTION_SRGB
                    }
                    TransferFunction::Rec709 => {
                        libcamera_color_space_transfer_function::LIBCAMERA_COLOR_SPACE_TRANSFER_FUNCTION_REC709
                    }
                },
                match cs.ycbcr_encoding {
                    YcbcrEncoding::None => {
                        libcamera_color_space_ycbcr_encoding::LIBCAMERA_COLOR_SPACE_YCBCR_ENCODING_NONE
                    }
                    YcbcrEncoding::Rec601 => {
                        libcamera_color_space_ycbcr_encoding::LIBCAMERA_COLOR_SPACE_YCBCR_ENCODING_REC601
                    }
                    YcbcrEncoding::Rec709 => {
                        libcamera_color_space_ycbcr_encoding::LIBCAMERA_COLOR_SPACE_YCBCR_ENCODING_REC709
                    }
                    YcbcrEncoding::Rec2020 => {
                        libcamera_color_space_ycbcr_encoding::LIBCAMERA_COLOR_SPACE_YCBCR_ENCODING_REC2020
                    }
                },
                match cs.range {
                    Range::Full => libcamera_color_space_range::LIBCAMERA_COLOR_SPACE_RANGE_FULL,
                    Range::Limited => libcamera_color_space_range::LIBCAMERA_COLOR_SPACE_RANGE_LIMITED,
                },
            )
        }
    }
}

impl From<libcamera_color_space_t> for ColorSpace {
    fn from(cs: libcamera_color_space_t) -> Self {
        let primaries = match cs.primaries {
            libcamera_color_space_primaries::LIBCAMERA_COLOR_SPACE_PRIMARIES_RAW => Primaries::Raw,
            libcamera_color_space_primaries::LIBCAMERA_COLOR_SPACE_PRIMARIES_SMPTE170M => Primaries::Smpte170m,
            libcamera_color_space_primaries::LIBCAMERA_COLOR_SPACE_PRIMARIES_REC709 => Primaries::Rec709,
            libcamera_color_space_primaries::LIBCAMERA_COLOR_SPACE_PRIMARIES_REC2020 => Primaries::Rec2020,
            _ => Primaries::Raw,
        };
        let transfer_function = match cs.transfer_function {
            libcamera_color_space_transfer_function::LIBCAMERA_COLOR_SPACE_TRANSFER_FUNCTION_LINEAR => {
                TransferFunction::Linear
            }
            libcamera_color_space_transfer_function::LIBCAMERA_COLOR_SPACE_TRANSFER_FUNCTION_SRGB => {
                TransferFunction::Srgb
            }
            libcamera_color_space_transfer_function::LIBCAMERA_COLOR_SPACE_TRANSFER_FUNCTION_REC709 => {
                TransferFunction::Rec709
            }
            _ => TransferFunction::Linear,
        };
        let ycbcr_encoding = match cs.ycbcr_encoding {
            libcamera_color_space_ycbcr_encoding::LIBCAMERA_COLOR_SPACE_YCBCR_ENCODING_NONE => YcbcrEncoding::None,
            libcamera_color_space_ycbcr_encoding::LIBCAMERA_COLOR_SPACE_YCBCR_ENCODING_REC601 => YcbcrEncoding::Rec601,
            libcamera_color_space_ycbcr_encoding::LIBCAMERA_COLOR_SPACE_YCBCR_ENCODING_REC709 => YcbcrEncoding::Rec709,
            libcamera_color_space_ycbcr_encoding::LIBCAMERA_COLOR_SPACE_YCBCR_ENCODING_REC2020 => {
                YcbcrEncoding::Rec2020
            }
            _ => YcbcrEncoding::None,
        };
        let range = match cs.range {
            libcamera_color_space_range::LIBCAMERA_COLOR_SPACE_RANGE_FULL => Range::Full,
            libcamera_color_space_range::LIBCAMERA_COLOR_SPACE_RANGE_LIMITED => Range::Limited,
            _ => Range::Full,
        };

        ColorSpace {
            primaries,
            transfer_function,
            ycbcr_encoding,
            range,
        }
    }
}
