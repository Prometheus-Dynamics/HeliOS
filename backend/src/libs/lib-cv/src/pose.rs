//! Pose utilities linking translation and rotation components.
//!
//! Unit conversions are delegated to [`PoseUnits`](crate::units::PoseUnits),
//! allowing callers to move between millimetres/degrees and metres/radians with
//! a single call to [`DevicePose::convert`].

use core::ops::{Add, AddAssign, Sub, SubAssign};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::math::{rotation::Rotation3, translation::Translation3};
use crate::units::PoseUnits;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, Default)]
/// Combined translation + rotation state expressed in configurable units.
pub struct DevicePose {
    #[serde(default)]
    pub translation: Translation3,
    #[serde(default)]
    pub rotation: Rotation3,
}

impl Add for DevicePose {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self { translation: self.translation + rhs.translation, rotation: self.rotation + rhs.rotation }
    }
}

impl AddAssign for DevicePose {
    fn add_assign(&mut self, rhs: Self) {
        self.translation += rhs.translation;
        self.rotation += rhs.rotation;
    }
}

impl Sub for DevicePose {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self {
        Self { translation: self.translation - rhs.translation, rotation: self.rotation - rhs.rotation }
    }
}

impl SubAssign for DevicePose {
    fn sub_assign(&mut self, rhs: Self) {
        self.translation -= rhs.translation;
        self.rotation -= rhs.rotation;
    }
}

impl DevicePose {
    pub fn convert(self, from: PoseUnits, to: PoseUnits) -> Self {
        Self { translation: self.translation.convert(from.length, to.length), rotation: self.rotation.convert(from.angle, to.angle) }
    }
}
