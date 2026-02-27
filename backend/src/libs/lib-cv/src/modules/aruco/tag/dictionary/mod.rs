//! Utilities to generate ArUco markers and ChArUco boards.
use image::{GrayImage, Luma};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

use super::super::ArucoBitGrid;
use super::ArucoTagFamilyKind;

pub mod data;
pub mod family;

use data::dictionary_data::{ARUCO_DICTIONARIES, ArucoDictionaryDef};

#[derive(Clone, Copy)]
pub struct ArucoDictionary {
    def: &'static ArucoDictionaryDef,
}

impl ArucoDictionary {
    pub fn name(&self) -> &'static str {
        self.def.name
    }

    pub fn marker_size(&self) -> u8 {
        self.def.marker_size
    }

    pub fn marker_count(&self) -> usize {
        self.def.marker_count
    }

    pub fn max_correction_bits(&self) -> u8 {
        self.def.max_correction_bits
    }

    pub fn nbytes(&self) -> usize {
        let bits = self.def.marker_size as usize * self.def.marker_size as usize;
        bits.div_ceil(8)
    }

    pub fn code(&self, id: usize) -> Option<&'static [u8]> {
        if id >= self.def.marker_count {
            return None;
        }
        let nbytes = self.nbytes();
        let start = id.saturating_mul(nbytes);
        let end = start.saturating_add(nbytes);
        self.def.bytes.get(start..end)
    }

    pub fn bit_grid(&self, id: usize) -> Option<ArucoBitGrid> {
        let code = self.code(id)?;
        let data_width = self.def.marker_size as usize;
        if data_width == 0 {
            return None;
        }
        let total_width = data_width + 2;
        let mut rows: Vec<String> = Vec::with_capacity(total_width);
        for y in 0..total_width {
            let mut row = String::with_capacity(total_width);
            for x in 0..total_width {
                let on_border = x == 0 || y == 0 || x + 1 == total_width || y + 1 == total_width;
                let bit = if on_border {
                    0u8
                } else {
                    let ix = (y - 1) * data_width + (x - 1);
                    let byte = code[ix / 8];
                    let shift = 7 - (ix % 8);
                    (byte >> shift) & 1
                };
                row.push(if bit == 0 { '0' } else { '1' });
            }
            rows.push(row);
        }
        Some(ArucoBitGrid { width: total_width as u8, border: 1, rows })
    }

    pub fn draw_marker(&self, id: usize, side_px: u32, border_bits: u8) -> Option<GrayImage> {
        if border_bits == 0 || id >= self.def.marker_count {
            return None;
        }
        let code = self.code(id)?;
        let dim = self.def.marker_size as u32 + 2 * border_bits as u32;
        let cell = side_px / dim;
        if cell == 0 {
            return None;
        }
        let mut img = GrayImage::new(cell * dim, cell * dim);
        for y in 0..dim {
            for x in 0..dim {
                let bit = if x < border_bits as u32 || x >= dim - border_bits as u32 || y < border_bits as u32 || y >= dim - border_bits as u32 {
                    0
                } else {
                    let ix = (y - border_bits as u32) * self.def.marker_size as u32 + (x - border_bits as u32);
                    let byte = code[(ix / 8) as usize];
                    let shift = 7 - (ix % 8);
                    (byte >> shift) & 1
                };
                let val = if bit == 0 { 0 } else { 255 };
                for dy in 0..cell {
                    for dx in 0..cell {
                        img.put_pixel(x * cell + dx, y * cell + dy, Luma([val]));
                    }
                }
            }
        }
        Some(img)
    }

    pub fn draw_charuco_board(&self, squares_x: u32, squares_y: u32, square_length: u32, marker_length: u32) -> GrayImage {
        let width = squares_x * square_length;
        let height = squares_y * square_length;
        let mut img = GrayImage::new(width, height);
        let mut id = 0usize;
        for j in 0..squares_y {
            for i in 0..squares_x {
                let x0 = i * square_length;
                let y0 = j * square_length;
                let white = (i + j) % 2 == 1;
                let val = if white { 255 } else { 0 };
                for y in y0..y0 + square_length {
                    for x in x0..x0 + square_length {
                        img.put_pixel(x, y, Luma([val as u8]));
                    }
                }
                if white {
                    if let Some(marker) = self.draw_marker(id % self.def.marker_count, marker_length, 1) {
                        let marker_w = marker.width();
                        let marker_h = marker.height();
                        let off_x = square_length.saturating_sub(marker_w) / 2;
                        let off_y = square_length.saturating_sub(marker_h) / 2;
                        for y in 0..marker_h {
                            for x in 0..marker_w {
                                let px = marker.get_pixel(x, y);
                                img.put_pixel(x0 + off_x + x, y0 + off_y + y, *px);
                            }
                        }
                    }
                    id = id.wrapping_add(1);
                }
            }
        }
        img
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArucoDictionaryKind {
    Dict4x4_50,
    Dict4x4_100,
    Dict4x4_250,
    #[default]
    Dict4x4_1000,
    Dict5x5_50,
    Dict5x5_100,
    Dict5x5_250,
    Dict5x5_1000,
    Dict6x6_50,
    Dict6x6_100,
    Dict6x6_250,
    Dict6x6_1000,
    Dict7x7_50,
    Dict7x7_100,
    Dict7x7_250,
    Dict7x7_1000,
    ArucoOriginal,
    ArucoMip36h12,
    Apriltag16H5,
    Apriltag25H9,
    Apriltag36H10,
    Apriltag36H11,
}

impl ArucoDictionaryKind {
    pub fn values() -> Vec<&'static str> {
        vec![
            "4x4_50",
            "4x4_100",
            "4x4_250",
            "4x4_1000",
            "5x5_50",
            "5x5_100",
            "5x5_250",
            "5x5_1000",
            "6x6_50",
            "6x6_100",
            "6x6_250",
            "6x6_1000",
            "7x7_50",
            "7x7_100",
            "7x7_250",
            "7x7_1000",
            "aruco_original",
            "aruco_mip_36h12",
            "apriltag_16h5",
            "apriltag_25h9",
            "apriltag_36h10",
            "apriltag_36h11",
        ]
    }

    pub fn as_aruco_name(self) -> Option<&'static str> {
        match self {
            Self::Dict4x4_50 => Some("4x4_50"),
            Self::Dict4x4_100 => Some("4x4_100"),
            Self::Dict4x4_250 => Some("4x4_250"),
            Self::Dict4x4_1000 => Some("4x4_1000"),
            Self::Dict5x5_50 => Some("5x5_50"),
            Self::Dict5x5_100 => Some("5x5_100"),
            Self::Dict5x5_250 => Some("5x5_250"),
            Self::Dict5x5_1000 => Some("5x5_1000"),
            Self::Dict6x6_50 => Some("6x6_50"),
            Self::Dict6x6_100 => Some("6x6_100"),
            Self::Dict6x6_250 => Some("6x6_250"),
            Self::Dict6x6_1000 => Some("6x6_1000"),
            Self::Dict7x7_50 => Some("7x7_50"),
            Self::Dict7x7_100 => Some("7x7_100"),
            Self::Dict7x7_250 => Some("7x7_250"),
            Self::Dict7x7_1000 => Some("7x7_1000"),
            Self::ArucoOriginal => Some("aruco_original"),
            Self::ArucoMip36h12 => Some("aruco_mip_36h12"),
            Self::Apriltag16H5 | Self::Apriltag25H9 | Self::Apriltag36H10 | Self::Apriltag36H11 => None,
        }
    }

    pub fn as_apriltag_family(self) -> Option<ArucoTagFamilyKind> {
        match self {
            Self::Apriltag16H5 => Some(ArucoTagFamilyKind::Tag16H5),
            Self::Apriltag25H9 => Some(ArucoTagFamilyKind::Tag25H9),
            Self::Apriltag36H10 => Some(ArucoTagFamilyKind::Tag36H10),
            Self::Apriltag36H11 => Some(ArucoTagFamilyKind::Tag36H11),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dict4x4_50 => "4x4_50",
            Self::Dict4x4_100 => "4x4_100",
            Self::Dict4x4_250 => "4x4_250",
            Self::Dict4x4_1000 => "4x4_1000",
            Self::Dict5x5_50 => "5x5_50",
            Self::Dict5x5_100 => "5x5_100",
            Self::Dict5x5_250 => "5x5_250",
            Self::Dict5x5_1000 => "5x5_1000",
            Self::Dict6x6_50 => "6x6_50",
            Self::Dict6x6_100 => "6x6_100",
            Self::Dict6x6_250 => "6x6_250",
            Self::Dict6x6_1000 => "6x6_1000",
            Self::Dict7x7_50 => "7x7_50",
            Self::Dict7x7_100 => "7x7_100",
            Self::Dict7x7_250 => "7x7_250",
            Self::Dict7x7_1000 => "7x7_1000",
            Self::ArucoOriginal => "aruco_original",
            Self::ArucoMip36h12 => "aruco_mip_36h12",
            Self::Apriltag16H5 => "apriltag_16h5",
            Self::Apriltag25H9 => "apriltag_25h9",
            Self::Apriltag36H10 => "apriltag_36h10",
            Self::Apriltag36H11 => "apriltag_36h11",
        }
    }
}

impl FromStr for ArucoDictionaryKind {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let raw = s.trim();
        if raw.is_empty() {
            return Err(());
        }
        let normalized = normalize_dictionary_name(raw).ok_or(())?;
        match normalized.as_str() {
            "4x4_50" => Ok(Self::Dict4x4_50),
            "4x4_100" => Ok(Self::Dict4x4_100),
            "4x4_250" => Ok(Self::Dict4x4_250),
            "4x4_1000" => Ok(Self::Dict4x4_1000),
            "5x5_50" => Ok(Self::Dict5x5_50),
            "5x5_100" => Ok(Self::Dict5x5_100),
            "5x5_250" => Ok(Self::Dict5x5_250),
            "5x5_1000" => Ok(Self::Dict5x5_1000),
            "6x6_50" => Ok(Self::Dict6x6_50),
            "6x6_100" => Ok(Self::Dict6x6_100),
            "6x6_250" => Ok(Self::Dict6x6_250),
            "6x6_1000" => Ok(Self::Dict6x6_1000),
            "7x7_50" => Ok(Self::Dict7x7_50),
            "7x7_100" => Ok(Self::Dict7x7_100),
            "7x7_250" => Ok(Self::Dict7x7_250),
            "7x7_1000" => Ok(Self::Dict7x7_1000),
            "aruco_original" => Ok(Self::ArucoOriginal),
            "aruco_mip_36h12" => Ok(Self::ArucoMip36h12),
            "apriltag_16h5" => Ok(Self::Apriltag16H5),
            "apriltag_25h9" => Ok(Self::Apriltag25H9),
            "apriltag_36h10" => Ok(Self::Apriltag36H10),
            "apriltag_36h11" => Ok(Self::Apriltag36H11),
            _ => Err(()),
        }
    }
}

impl Serialize for ArucoDictionaryKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ArucoDictionaryKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        ArucoDictionaryKind::from_str(&s).map_err(|_| serde::de::Error::custom("invalid tag dictionary"))
    }
}

fn normalize_dictionary_name(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.to_ascii_lowercase())
}

pub fn aruco_dictionary_from_name(name: &str) -> Option<ArucoDictionary> {
    let normalized = normalize_dictionary_name(name)?;
    ARUCO_DICTIONARIES.iter().find(|def| def.name == normalized).map(|def| ArucoDictionary { def })
}

/// Trait implemented by each ArUco dictionary.
pub trait Dictionary {
    /// Number of bits per side of the marker.
    const SIDE_BITS: u32;

    /// Total number of markers in the dictionary.
    fn marker_count() -> usize;

    /// Return the raw code bytes for a marker id.
    fn code(id: usize) -> Option<&'static [u8]>;

    fn draw_marker(id: usize, side_px: u32, border_bits: u8) -> Option<GrayImage> {
        if border_bits == 0 || id >= Self::marker_count() {
            return None;
        }
        let code = Self::code(id)?;
        let dim = Self::SIDE_BITS + 2 * border_bits as u32;
        let cell = side_px / dim;
        if cell == 0 {
            return None;
        }
        let mut img = GrayImage::new(cell * dim, cell * dim);
        for y in 0..dim {
            for x in 0..dim {
                let bit = if x < border_bits as u32 || x >= dim - border_bits as u32 || y < border_bits as u32 || y >= dim - border_bits as u32 {
                    0
                } else {
                    let ix = (y - border_bits as u32) * Self::SIDE_BITS + (x - border_bits as u32);
                    let byte = code[(ix / 8) as usize];
                    let shift = 7 - (ix % 8);
                    (byte >> shift) & 1
                };
                let val = if bit == 0 { 0 } else { 255 };
                for dy in 0..cell {
                    for dx in 0..cell {
                        img.put_pixel(x * cell + dx, y * cell + dy, Luma([val]));
                    }
                }
            }
        }
        Some(img)
    }

    fn generate_marker_set(side_px: u32, border_bits: u8) -> Vec<(usize, GrayImage)> {
        (0..Self::marker_count()).filter_map(|id| Self::draw_marker(id, side_px, border_bits).map(|img| (id, img))).collect()
    }

    fn generate_marker_set_mm(side_mm: f32, dpi: f32, border_bits: u8) -> Option<Vec<(usize, GrayImage)>> {
        let px = mm_to_px(side_mm, dpi)?;
        Some(Self::generate_marker_set(px, border_bits))
    }

    fn draw_charuco_board(squares_x: u32, squares_y: u32, square_length: u32, marker_length: u32) -> GrayImage {
        let width = squares_x * square_length;
        let height = squares_y * square_length;
        let mut img = GrayImage::new(width, height);
        let mut id = 0usize;
        for j in 0..squares_y {
            for i in 0..squares_x {
                let x0 = i * square_length;
                let y0 = j * square_length;
                // Match OpenCV's ChArUco layout: square (0,0) is black, and markers are placed on
                // the alternating (white) squares. With an odd total square count this yields
                // floor(total/2) markers, consistent with OpenCV's generated IDs.
                let white = (i + j) % 2 == 1;
                let val = if white { 255 } else { 0 };
                for y in y0..y0 + square_length {
                    for x in x0..x0 + square_length {
                        img.put_pixel(x, y, Luma([val as u8]));
                    }
                }
                if white {
                    if let Some(marker) = Self::draw_marker(id % Self::marker_count(), marker_length, 1) {
                        let marker_w = marker.width();
                        let marker_h = marker.height();
                        let off_x = square_length.saturating_sub(marker_w) / 2;
                        let off_y = square_length.saturating_sub(marker_h) / 2;
                        for y in 0..marker_h {
                            for x in 0..marker_w {
                                let px = marker.get_pixel(x, y);
                                img.put_pixel(x0 + off_x + x, y0 + off_y + y, *px);
                            }
                        }
                    }
                    id += 1;
                }
            }
        }
        img
    }
}

/// Millimeters per inch used when converting between pixels and physical size.
const MM_PER_INCH: f32 = 25.4;

pub fn mm_to_px(mm: f32, dpi: f32) -> Option<u32> {
    if dpi <= 0.0 || mm <= 0.0 {
        return None;
    }
    Some(((mm / MM_PER_INCH) * dpi).round() as u32)
}

pub fn px_to_mm(px: u32, dpi: f32) -> Option<f32> {
    if dpi <= 0.0 {
        return None;
    }
    Some((px as f32 / dpi) * MM_PER_INCH)
}

pub use data::dict4x4_50::Dict4x4_50;
pub use data::dict5x5_100::Dict5x5_100;
pub use data::dict6x6_250::Dict6x6_250;

/// Names of the built-in ArUco dictionaries.
pub const AVAILABLE_DICTIONARIES: &[&str] = &[
    "4x4_50",
    "4x4_100",
    "4x4_250",
    "4x4_1000",
    "5x5_50",
    "5x5_100",
    "5x5_250",
    "5x5_1000",
    "6x6_50",
    "6x6_100",
    "6x6_250",
    "6x6_1000",
    "7x7_50",
    "7x7_100",
    "7x7_250",
    "7x7_1000",
    "aruco_original",
    "aruco_mip_36h12",
];

/// Return a list of available dictionary names.
pub fn list_dictionaries() -> Vec<String> {
    AVAILABLE_DICTIONARIES.iter().map(|&d| d.to_string()).collect()
}

/// Generate a ChArUco board using the specified dictionary name.
pub fn charuco_from_dict_name(name: &str, squares_x: u32, squares_y: u32, square_length: u32, marker_length: u32) -> Option<GrayImage> {
    aruco_dictionary_from_name(name).map(|dict| dict.draw_charuco_board(squares_x, squares_y, square_length, marker_length))
}
