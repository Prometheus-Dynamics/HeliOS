use std::{fmt::Debug, str::FromStr};

use serde::de::{self, Visitor};
use serde::{Deserialize, Serialize};

use super::decode_grid;
use super::decoding::{ArucoTagDecode, ArucoTagDecoding};
use super::dictionary::family::{Family16H5, Family25H9, Family36H10, Family36H11};

#[derive(Debug, Clone, Copy)]
pub struct ArucoTagDecodeTuning {
    pub min_cell_means_contrast_range: f32,
    pub min_hamming_margin: u32,
    pub min_hamming_margin_min_dist: u32,
    pub min_hamming_margin_only_if_border_mismatch: bool,
    pub min_bit_delta: f32,
}

impl Default for ArucoTagDecodeTuning {
    fn default() -> Self {
        Self {
            // OpenCV parity: lower contrast threshold to recover valid tags.
            min_cell_means_contrast_range: 10.0,
            min_hamming_margin: 2,
            min_hamming_margin_min_dist: 1,
            min_hamming_margin_only_if_border_mismatch: false,
            // OpenCV parity: lower bit delta gate to keep weaker tags.
            min_bit_delta: 2.0,
        }
    }
}

pub struct ArucoTagFamily {
    pub family: Box<dyn ArucoTagDecoding>,
    label: String,
    max_hamming_override: Option<u8>,
    border_error_divisor_override: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArucoTagFamilyKind {
    Tag16H5,
    Tag25H9,
    Tag36H10,
    Tag36H11,
}

impl ArucoTagFamilyKind {
    pub fn as_label(self) -> &'static str {
        match self {
            Self::Tag16H5 => "16h5",
            Self::Tag25H9 => "25h9",
            Self::Tag36H10 => "36h10",
            Self::Tag36H11 => "36h11",
        }
    }

    pub fn values() -> &'static [&'static str] {
        &["16h5", "25h9", "36h10", "36h11"]
    }

    pub fn into_family(self) -> ArucoTagFamily {
        match self {
            Self::Tag16H5 => ArucoTagFamily::new_family16h5(),
            Self::Tag25H9 => ArucoTagFamily::new_family25h9(),
            Self::Tag36H10 => ArucoTagFamily::new_family36h10(),
            Self::Tag36H11 => ArucoTagFamily::new_family36h11(),
        }
    }
}

impl FromStr for ArucoTagFamilyKind {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let normalized = s.trim();
        match normalized {
            "16h5" => Ok(Self::Tag16H5),
            "25h9" => Ok(Self::Tag25H9),
            "36h10" => Ok(Self::Tag36H10),
            "36h11" => Ok(Self::Tag36H11),
            _ => Err(()),
        }
    }
}

impl Serialize for ArucoTagFamilyKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_label())
    }
}

impl<'de> Deserialize<'de> for ArucoTagFamilyKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct KindVisitor;

        impl Visitor<'_> for KindVisitor {
            type Value = ArucoTagFamilyKind;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("an ArUco tag family label like \"16h5\"")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let normalized = value.trim().to_ascii_lowercase();
                match normalized.as_str() {
                    "16h5" => Ok(ArucoTagFamilyKind::Tag16H5),
                    "25h9" => Ok(ArucoTagFamilyKind::Tag25H9),
                    "36h10" => Ok(ArucoTagFamilyKind::Tag36H10),
                    "36h11" => Ok(ArucoTagFamilyKind::Tag36H11),
                    _ => Err(E::custom(format!("unknown ArUco tag family: {value}"))),
                }
            }
        }

        deserializer.deserialize_str(KindVisitor)
    }
}

impl Debug for ArucoTagFamily {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Family: {}", self.label)
    }
}

impl ArucoTagFamily {
    // Constructor methods for each family variant
    pub fn new_family16h5() -> Self {
        ArucoTagFamily { family: Box::new(Family16H5), label: "16h5".into(), max_hamming_override: None, border_error_divisor_override: None }
    }

    pub fn new_family25h9() -> Self {
        ArucoTagFamily { family: Box::new(Family25H9), label: "25h9".into(), max_hamming_override: None, border_error_divisor_override: None }
    }

    pub fn new_family36h10() -> Self {
        ArucoTagFamily { family: Box::new(Family36H10), label: "36h10".into(), max_hamming_override: None, border_error_divisor_override: None }
    }

    pub fn new_family36h11() -> Self {
        ArucoTagFamily { family: Box::new(Family36H11), label: "36h11".into(), max_hamming_override: None, border_error_divisor_override: None }
    }

    /// Names of the built-in ArUco tag families.
    pub fn available() -> &'static [&'static str] {
        ArucoTagFamilyKind::values()
    }

    /// Construct a family by its label, e.g. "16h5".
    pub fn from_label(label: &str) -> Option<Self> {
        ArucoTagFamilyKind::from_str(label).ok().map(|kind| kind.into_family())
    }

    #[must_use]
    pub fn with_max_hamming(mut self, max_hamming: u8) -> Self {
        self.max_hamming_override = Some(max_hamming);
        self
    }

    #[must_use]
    pub fn with_border_error_divisor(mut self, divisor: u8) -> Self {
        self.border_error_divisor_override = Some(divisor.max(1));
        self
    }
}

// Implement ArucoTagDecoding for ArucoTagFamily by delegating to the trait object
impl ArucoTagDecoding for ArucoTagFamily {
    fn decode(&self, code: Vec<Vec<u8>>) -> Option<ArucoTagDecode> {
        // Use the shared decode helper so overrides like max Hamming and border error divisor
        // apply consistently across both sampled and warp-based decode paths.
        decode_grid::decode(&code, self)
    }

    fn max_hamming_distance(&self) -> u8 {
        self.max_hamming_override.unwrap_or_else(|| self.family.max_hamming_distance())
    }

    fn data_bits_location(&self) -> &[(u8, u8)] {
        self.family.data_bits_location()
    }

    fn border_size(&self) -> u8 {
        self.family.border_size()
    }

    fn data_width(&self) -> u8 {
        self.family.data_width()
    }

    fn total_width(&self) -> u8 {
        self.family.total_width()
    }

    fn codes(&self) -> &[u64] {
        self.family.codes()
    }

    fn border_error_divisor(&self) -> u8 {
        self.border_error_divisor_override.unwrap_or_else(|| self.family.border_error_divisor())
    }
}
