use std::{ffi::CStr, fmt, ptr::NonNull, str::FromStr};

use drm_fourcc::{DrmFormat, DrmFourcc, DrmModifier};
use libcamera_sys::*;

use crate::geometry::Size;

mod pixel_format_info_generated {
    include!(concat!(env!("OUT_DIR"), "/pixel_format_info.rs"));
}
use pixel_format_info_generated::{PixelFormatInfoData, PIXEL_FORMAT_INFO};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColourEncoding {
    Rgb,
    Yuv,
    Raw,
    Unknown(u32),
}

impl From<u32> for ColourEncoding {
    fn from(v: u32) -> Self {
        match v {
            0 => ColourEncoding::Rgb,
            1 => ColourEncoding::Yuv,
            2 => ColourEncoding::Raw,
            other => ColourEncoding::Unknown(other),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PixelFormatPlaneInfo {
    pub bytes_per_group: u32,
    pub vertical_sub_sampling: u32,
}

#[derive(Debug, Clone)]
pub struct PixelFormatInfo {
    pub name: String,
    pub format: PixelFormat,
    pub bits_per_pixel: u32,
    pub colour_encoding: ColourEncoding,
    pub packed: bool,
    pub pixels_per_group: u32,
    pub planes: Vec<PixelFormatPlaneInfo>,
    pub v4l2_formats: Vec<u32>,
}

impl PixelFormatInfo {
    fn from_data(fmt: PixelFormat, data: &PixelFormatInfoData) -> Self {
        let planes = data
            .planes
            .iter()
            .filter(|p| p.bytes_per_group > 0 && p.vertical_sub_sampling > 0)
            .map(|p| PixelFormatPlaneInfo {
                bytes_per_group: p.bytes_per_group,
                vertical_sub_sampling: p.vertical_sub_sampling,
            })
            .collect();
        Self {
            name: data.name.to_string(),
            format: fmt,
            bits_per_pixel: data.bits_per_pixel,
            colour_encoding: data.colour_encoding.into(),
            packed: data.packed,
            pixels_per_group: data.pixels_per_group,
            planes,
            v4l2_formats: data.v4l2_formats.to_vec(),
        }
    }
}

/// Represents `libcamera::PixelFormat`, which itself is a pair of fourcc code and u64 modifier as defined in `libdrm`.
#[derive(Clone, Copy)]
pub struct PixelFormat(pub(crate) libcamera_pixel_format_t);

impl PixelFormat {
    fn info_entry(&self) -> Option<&'static PixelFormatInfoData> {
        let (fourcc, modifier) = self.to_raw();
        PIXEL_FORMAT_INFO
            .iter()
            .find(|info| info.fourcc == fourcc && info.modifier == modifier)
    }

    /// Constructs new [PixelFormat] from given fourcc code and modifier.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use libcamera::pixel_format::PixelFormat;
    /// // Constructs MJPEG pixel format
    /// const PIXEL_FORMAT_MJPEG: PixelFormat =
    ///     PixelFormat::new(u32::from_le_bytes([b'M', b'J', b'P', b'G']), 0);
    /// ```
    pub const fn new(fourcc: u32, modifier: u64) -> Self {
        Self(libcamera_pixel_format_t { fourcc, modifier })
    }

    pub fn fourcc(&self) -> u32 {
        self.0.fourcc
    }

    pub fn set_fourcc(&mut self, fourcc: u32) {
        self.0.fourcc = fourcc;
    }

    pub fn modifier(&self) -> u64 {
        self.0.modifier
    }

    pub fn set_modifier(&mut self, modifier: u64) {
        self.0.modifier = modifier;
    }

    /// Returns true if this format has a non-zero modifier set.
    pub fn has_modifier(&self) -> bool {
        self.modifier() != 0
    }

    /// Compute the stride for a plane given width and optional alignment.
    pub fn stride(&self, width: u32, plane: u32, align: u32) -> u32 {
        self.info_entry()
            .map(|info| compute_stride(info, width, plane, align))
            .unwrap_or(0)
    }

    /// Compute plane size for the given frame size and plane index.
    pub fn plane_size(&self, size: Size, plane: u32, align: u32) -> u32 {
        self.info_entry()
            .map(|info| compute_plane_size(info, size, plane, align))
            .unwrap_or(0)
    }

    /// Compute total frame size for the given dimensions.
    pub fn frame_size(&self, size: Size, align: u32) -> u32 {
        self.info_entry()
            .map(|info| compute_frame_size(info, size, align))
            .unwrap_or(0)
    }

    /// Clears the modifier to zero.
    pub fn clear_modifier(&mut self) {
        self.0.modifier = 0;
    }

    /// Returns the raw `(fourcc, modifier)` tuple.
    pub const fn to_raw(self) -> (u32, u64) {
        (self.0.fourcc, self.0.modifier)
    }

    /// Constructs a PixelFormat from raw `(fourcc, modifier)` parts.
    pub const fn from_raw_parts(fourcc: u32, modifier: u64) -> Self {
        PixelFormat::new(fourcc, modifier)
    }

    /// Parse a PixelFormat from its string representation (e.g. "YUYV").
    pub fn parse(name: &str) -> Option<Self> {
        let cstr = std::ffi::CString::new(name).ok()?;
        let fmt = unsafe { libcamera_pixel_format_from_str(cstr.as_ptr()) };
        let pf = PixelFormat(fmt);
        if pf.is_valid() {
            Some(pf)
        } else {
            None
        }
    }

    /// Returns true if the PixelFormat represents a valid libcamera format.
    pub fn is_valid(&self) -> bool {
        unsafe { libcamera_pixel_format_is_valid(&self.0) }
    }

    pub fn info(&self) -> Option<PixelFormatInfo> {
        self.info_entry().map(|data| PixelFormatInfo::from_data(*self, data))
    }
}

impl FromStr for PixelFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        PixelFormat::parse(s).ok_or_else(|| format!("unrecognized pixel format: {s}"))
    }
}

impl PartialEq for PixelFormat {
    fn eq(&self, other: &Self) -> bool {
        self.0.fourcc.eq(&other.0.fourcc) && self.0.modifier.eq(&other.0.modifier)
    }
}

impl Eq for PixelFormat {}

impl core::fmt::Debug for PixelFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ptr = unsafe { libcamera_pixel_format_str(&self.0) };
        let out = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap();
        f.write_str(out)?;
        unsafe { libc::free(ptr.cast()) };
        Ok(())
    }
}

impl fmt::Display for PixelFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

fn compute_stride(info: &PixelFormatInfoData, width: u32, plane: u32, align: u32) -> u32 {
    if plane as usize >= info.planes.len() {
        return 0;
    }
    let plane_info = &info.planes[plane as usize];
    if plane_info.bytes_per_group == 0 || plane_info.vertical_sub_sampling == 0 {
        return 0;
    }
    let groups = (width as u64).div_ceil(info.pixels_per_group as u64);
    let mut stride = groups * plane_info.bytes_per_group as u64;
    if align > 0 {
        stride = stride.div_ceil(align as u64) * align as u64;
    }
    stride as u32
}

fn compute_plane_size(info: &PixelFormatInfoData, size: Size, plane: u32, align: u32) -> u32 {
    if plane as usize >= info.planes.len() {
        return 0;
    }
    let plane_info = &info.planes[plane as usize];
    if plane_info.vertical_sub_sampling == 0 {
        return 0;
    }
    let stride = compute_stride(info, size.width, plane, align) as u64;
    // ceil(height / vertical_sub_sampling)
    let height = (size.height as u64).div_ceil(plane_info.vertical_sub_sampling as u64);
    (stride * height) as u32
}

fn compute_frame_size(info: &PixelFormatInfoData, size: Size, align: u32) -> u32 {
    let mut total: u64 = 0;
    for p in 0..info.planes.len() {
        let plane = &info.planes[p];
        if plane.bytes_per_group == 0 || plane.vertical_sub_sampling == 0 {
            continue;
        }
        total += compute_plane_size(info, size, p as u32, align) as u64;
    }
    total as u32
}

impl From<u8> for ColourEncoding {
    fn from(v: u8) -> Self {
        match v {
            0 => ColourEncoding::Rgb,
            1 => ColourEncoding::Yuv,
            2 => ColourEncoding::Raw,
            other => ColourEncoding::Unknown(other as u32),
        }
    }
}

impl TryFrom<PixelFormat> for DrmFormat {
    type Error = drm_fourcc::UnrecognizedFourcc;

    fn try_from(value: PixelFormat) -> Result<Self, Self::Error> {
        let code = DrmFourcc::try_from(value.0.fourcc)?;
        let modifier = DrmModifier::from(value.0.modifier);
        Ok(DrmFormat { code, modifier })
    }
}

impl From<DrmFormat> for PixelFormat {
    fn from(f: DrmFormat) -> Self {
        PixelFormat::new(f.code as u32, f.modifier.into())
    }
}

/// Vector of [PixelFormat]
pub struct PixelFormats {
    ptr: NonNull<libcamera_pixel_formats_t>,
}

impl PixelFormats {
    pub(crate) unsafe fn from_ptr(ptr: NonNull<libcamera_pixel_formats_t>) -> Self {
        Self { ptr }
    }

    /// Number of [PixelFormat]
    pub fn len(&self) -> usize {
        unsafe { libcamera_pixel_formats_size(self.ptr.as_ptr()) as _ }
    }

    /// Returns `true` if there there are no pixel formats
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns [PixelFormat] at a given index.
    ///
    /// Return None if index is out of range.
    pub fn get(&self, index: usize) -> Option<PixelFormat> {
        if index >= self.len() {
            None
        } else {
            Some(unsafe { self.get_unchecked(index) })
        }
    }

    /// Returns [PixelFormat] at a given index without checking bounds.
    ///
    /// # Safety
    ///
    /// `index` must be less than [PixelFormats::len()].
    pub unsafe fn get_unchecked(&self, index: usize) -> PixelFormat {
        PixelFormat(unsafe { libcamera_pixel_formats_get(self.ptr.as_ptr(), index as _) })
    }
}

impl<'d> IntoIterator for &'d PixelFormats {
    type Item = PixelFormat;

    type IntoIter = PixelFormatsIterator<'d>;

    fn into_iter(self) -> Self::IntoIter {
        PixelFormatsIterator {
            formats: self,
            index: 0,
        }
    }
}

impl Drop for PixelFormats {
    fn drop(&mut self) {
        unsafe { libcamera_pixel_formats_destroy(self.ptr.as_ptr()) }
    }
}

pub struct PixelFormatsIterator<'d> {
    formats: &'d PixelFormats,
    index: usize,
}

impl Iterator for PixelFormatsIterator<'_> {
    type Item = PixelFormat;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(next) = self.formats.get(self.index) {
            self.index += 1;
            Some(next)
        } else {
            None
        }
    }
}
