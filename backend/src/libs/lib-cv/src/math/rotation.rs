use crate::units::AngleUnit;
use core::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Sub, SubAssign};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, Default, PartialEq)]
/// 2D rotation expressed in the active [`AngleUnit`](crate::units::AngleUnit).
pub struct Rotation2 {
    /// rotation around Z axis in radians
    pub yaw: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema, Default, PartialEq)]
/// 3D rotation with roll/pitch/yaw using [`AngleUnit`](crate::units::AngleUnit).
pub struct Rotation3 {
    pub roll: f64,
    pub pitch: f64,
    pub yaw: f64,
}

impl Add for Rotation2 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self { yaw: self.yaw + rhs.yaw }
    }
}

impl AddAssign for Rotation2 {
    fn add_assign(&mut self, rhs: Self) {
        self.yaw += rhs.yaw;
    }
}

impl Sub for Rotation2 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        Self { yaw: self.yaw - rhs.yaw }
    }
}

impl SubAssign for Rotation2 {
    fn sub_assign(&mut self, rhs: Self) {
        self.yaw -= rhs.yaw;
    }
}

impl Mul<f64> for Rotation2 {
    type Output = Self;

    fn mul(self, rhs: f64) -> Self {
        Self { yaw: self.yaw * rhs }
    }
}

impl MulAssign<f64> for Rotation2 {
    fn mul_assign(&mut self, rhs: f64) {
        self.yaw *= rhs;
    }
}

impl Div<f64> for Rotation2 {
    type Output = Self;

    fn div(self, rhs: f64) -> Self {
        Self { yaw: self.yaw / rhs }
    }
}

impl DivAssign<f64> for Rotation2 {
    fn div_assign(&mut self, rhs: f64) {
        self.yaw /= rhs;
    }
}

impl Add for Rotation3 {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self { roll: self.roll + rhs.roll, pitch: self.pitch + rhs.pitch, yaw: self.yaw + rhs.yaw }
    }
}

impl AddAssign for Rotation3 {
    fn add_assign(&mut self, rhs: Self) {
        self.roll += rhs.roll;
        self.pitch += rhs.pitch;
        self.yaw += rhs.yaw;
    }
}

impl Sub for Rotation3 {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        Self { roll: self.roll - rhs.roll, pitch: self.pitch - rhs.pitch, yaw: self.yaw - rhs.yaw }
    }
}

impl SubAssign for Rotation3 {
    fn sub_assign(&mut self, rhs: Self) {
        self.roll -= rhs.roll;
        self.pitch -= rhs.pitch;
        self.yaw -= rhs.yaw;
    }
}

impl Mul<f64> for Rotation3 {
    type Output = Self;

    fn mul(self, rhs: f64) -> Self {
        Self { roll: self.roll * rhs, pitch: self.pitch * rhs, yaw: self.yaw * rhs }
    }
}

impl MulAssign<f64> for Rotation3 {
    fn mul_assign(&mut self, rhs: f64) {
        self.roll *= rhs;
        self.pitch *= rhs;
        self.yaw *= rhs;
    }
}

impl Div<f64> for Rotation3 {
    type Output = Self;

    fn div(self, rhs: f64) -> Self {
        Self { roll: self.roll / rhs, pitch: self.pitch / rhs, yaw: self.yaw / rhs }
    }
}

impl DivAssign<f64> for Rotation3 {
    fn div_assign(&mut self, rhs: f64) {
        self.roll /= rhs;
        self.pitch /= rhs;
        self.yaw /= rhs;
    }
}

impl Rotation2 {
    pub fn convert(self, from: AngleUnit, to: AngleUnit) -> Self {
        Self { yaw: to.convert_from_radians(from.to_radians(self.yaw)) }
    }
}

impl Rotation3 {
    pub fn convert(self, from: AngleUnit, to: AngleUnit) -> Self {
        Self { roll: to.convert_from_radians(from.to_radians(self.roll)), pitch: to.convert_from_radians(from.to_radians(self.pitch)), yaw: to.convert_from_radians(from.to_radians(self.yaw)) }
    }
}

impl From<f64> for Rotation2 {
    fn from(value: f64) -> Self {
        Self { yaw: value }
    }
}

impl From<(f64, f64, f64)> for Rotation3 {
    fn from(value: (f64, f64, f64)) -> Self {
        Self { roll: value.0, pitch: value.1, yaw: value.2 }
    }
}

impl From<[f64; 3]> for Rotation3 {
    fn from(value: [f64; 3]) -> Self {
        Self { roll: value[0], pitch: value[1], yaw: value[2] }
    }
}
