use crate::units::LengthUnit;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use core::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Sub, SubAssign};

#[cfg_attr(feature = "engine", derive(daedalus_macros::DaedalusTypeExpr, daedalus_macros::DaedalusToValue))]
#[cfg_attr(feature = "engine", daedalus(type_key = "cv:translation2"))]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, Default, PartialEq)]
/// 2D translation whose components obey the active [`LengthUnit`](crate::units::LengthUnit).
///
/// ```
/// use lib_cv::math::translation::Translation2;
/// use lib_cv::units::LengthUnit;
///
/// let camera_offset = Translation2 { x: 150.0, y: -20.0 };
/// let metres = camera_offset.convert(LengthUnit::Millimeters, LengthUnit::Meters);
/// assert!((metres.x - 0.15).abs() < f64::EPSILON);
/// ```
pub struct Translation2 {
    pub x: f64,
    pub y: f64,
}

#[cfg_attr(feature = "engine", derive(daedalus_macros::DaedalusTypeExpr, daedalus_macros::DaedalusToValue))]
#[cfg_attr(feature = "engine", daedalus(type_key = "cv:translation3"))]
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, Default, PartialEq)]
/// 3D translation (x, y, z) in the selected [`LengthUnit`](crate::units::LengthUnit).
pub struct Translation3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Add for Translation2 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self { x: self.x + rhs.x, y: self.y + rhs.y }
    }
}

impl AddAssign for Translation2 {
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
    }
}

impl Sub for Translation2 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        Self { x: self.x - rhs.x, y: self.y - rhs.y }
    }
}

impl SubAssign for Translation2 {
    fn sub_assign(&mut self, rhs: Self) {
        self.x -= rhs.x;
        self.y -= rhs.y;
    }
}

impl Mul<f64> for Translation2 {
    type Output = Self;

    fn mul(self, rhs: f64) -> Self {
        Self { x: self.x * rhs, y: self.y * rhs }
    }
}

impl MulAssign<f64> for Translation2 {
    fn mul_assign(&mut self, rhs: f64) {
        self.x *= rhs;
        self.y *= rhs;
    }
}

impl Div<f64> for Translation2 {
    type Output = Self;

    fn div(self, rhs: f64) -> Self {
        Self { x: self.x / rhs, y: self.y / rhs }
    }
}

impl DivAssign<f64> for Translation2 {
    fn div_assign(&mut self, rhs: f64) {
        self.x /= rhs;
        self.y /= rhs;
    }
}

impl Add for Translation3 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self { x: self.x + rhs.x, y: self.y + rhs.y, z: self.z + rhs.z }
    }
}

impl AddAssign for Translation3 {
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
        self.z += rhs.z;
    }
}

impl Sub for Translation3 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        Self { x: self.x - rhs.x, y: self.y - rhs.y, z: self.z - rhs.z }
    }
}

impl SubAssign for Translation3 {
    fn sub_assign(&mut self, rhs: Self) {
        self.x -= rhs.x;
        self.y -= rhs.y;
        self.z -= rhs.z;
    }
}

impl Mul<f64> for Translation3 {
    type Output = Self;

    fn mul(self, rhs: f64) -> Self {
        Self { x: self.x * rhs, y: self.y * rhs, z: self.z * rhs }
    }
}

impl MulAssign<f64> for Translation3 {
    fn mul_assign(&mut self, rhs: f64) {
        self.x *= rhs;
        self.y *= rhs;
        self.z *= rhs;
    }
}

impl Div<f64> for Translation3 {
    type Output = Self;

    fn div(self, rhs: f64) -> Self {
        Self { x: self.x / rhs, y: self.y / rhs, z: self.z / rhs }
    }
}

impl DivAssign<f64> for Translation3 {
    fn div_assign(&mut self, rhs: f64) {
        self.x /= rhs;
        self.y /= rhs;
        self.z /= rhs;
    }
}

impl Translation2 {
    pub fn convert(self, from: LengthUnit, to: LengthUnit) -> Self {
        Self { x: to.convert_from_meters(from.to_meters(self.x)), y: to.convert_from_meters(from.to_meters(self.y)) }
    }
}

impl Translation3 {
    pub fn convert(self, from: LengthUnit, to: LengthUnit) -> Self {
        Self { x: to.convert_from_meters(from.to_meters(self.x)), y: to.convert_from_meters(from.to_meters(self.y)), z: to.convert_from_meters(from.to_meters(self.z)) }
    }
}

impl From<(f64, f64)> for Translation2 {
    fn from(value: (f64, f64)) -> Self {
        Self { x: value.0, y: value.1 }
    }
}

impl From<[f64; 2]> for Translation2 {
    fn from(value: [f64; 2]) -> Self {
        Self { x: value[0], y: value[1] }
    }
}

impl From<(f64, f64, f64)> for Translation3 {
    fn from(value: (f64, f64, f64)) -> Self {
        Self { x: value.0, y: value.1, z: value.2 }
    }
}

impl From<[f64; 3]> for Translation3 {
    fn from(value: [f64; 3]) -> Self {
        Self { x: value[0], y: value[1], z: value[2] }
    }
}
