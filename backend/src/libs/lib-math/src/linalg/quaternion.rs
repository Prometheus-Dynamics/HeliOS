use core::ops::{Add, Mul};

use crate::linalg::{LinalgError, Mat3, Vec3};

/// Quaternion representing 3D rotations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quaternion {
    pub w: f32,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Quaternion {
    pub const IDENTITY: Self = Self { w: 1.0, x: 0.0, y: 0.0, z: 0.0 };

    pub fn new(w: f32, x: f32, y: f32, z: f32) -> Self {
        Self { w, x, y, z }
    }

    pub fn from_axis_angle(axis: Vec3, angle_radians: f32) -> Result<Self, LinalgError> {
        let normalized = axis.normalized().ok_or(LinalgError::ZeroMagnitude)?;
        let half = angle_radians * 0.5;
        let (s, c) = half.sin_cos();
        Ok(Self { w: c, x: normalized.x * s, y: normalized.y * s, z: normalized.z * s })
    }

    pub fn normalized(self) -> Result<Self, LinalgError> {
        let norm_sq = self.w * self.w + self.x * self.x + self.y * self.y + self.z * self.z;
        if norm_sq <= f32::EPSILON {
            return Err(LinalgError::ZeroMagnitude);
        }
        let inv = 1.0 / norm_sq.sqrt();
        Ok(Self { w: self.w * inv, x: self.x * inv, y: self.y * inv, z: self.z * inv })
    }

    pub fn conjugate(self) -> Self {
        Self { w: self.w, x: -self.x, y: -self.y, z: -self.z }
    }

    pub fn to_mat3(self) -> Result<Mat3, LinalgError> {
        let q = self.normalized()?;
        let (w, x, y, z) = (q.w, q.x, q.y, q.z);
        let rows = [
            [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - z * w), 2.0 * (x * z + y * w)],
            [2.0 * (x * y + z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - x * w)],
            [2.0 * (x * z - y * w), 2.0 * (y * z + x * w), 1.0 - 2.0 * (x * x + y * y)],
        ];
        Ok(Mat3::from_rows(rows))
    }

    pub fn rotate_vec3(self, v: Vec3) -> Result<Vec3, LinalgError> {
        let rot = self.to_mat3()?;
        Ok(rot.transform_vec3(v))
    }
}

impl Mul for Quaternion {
    type Output = Self;

    fn mul(self, rhs: Self) -> Self::Output {
        Self {
            w: self.w * rhs.w - self.x * rhs.x - self.y * rhs.y - self.z * rhs.z,
            x: self.w * rhs.x + self.x * rhs.w + self.y * rhs.z - self.z * rhs.y,
            y: self.w * rhs.y - self.x * rhs.z + self.y * rhs.w + self.z * rhs.x,
            z: self.w * rhs.z + self.x * rhs.y - self.y * rhs.x + self.z * rhs.w,
        }
    }
}

impl Add for Quaternion {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self { w: self.w + rhs.w, x: self.x + rhs.x, y: self.y + rhs.y, z: self.z + rhs.z }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_angle_round_trip() {
        let q = Quaternion::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), core::f32::consts::FRAC_PI_2).unwrap();
        let rotated = q.rotate_vec3(Vec3::new(1.0, 0.0, 0.0)).unwrap();
        assert!((rotated.x).abs() < 1e-5);
        assert!((rotated.y - 1.0).abs() < 1e-5);
    }

    #[test]
    fn quaternion_multiply_combines_rotations() {
        let q1 = Quaternion::from_axis_angle(Vec3::new(1.0, 0.0, 0.0), core::f32::consts::FRAC_PI_2).unwrap();
        let q2 = Quaternion::from_axis_angle(Vec3::new(0.0, 1.0, 0.0), core::f32::consts::FRAC_PI_2).unwrap();
        let combined = (q2 * q1).normalized().unwrap();
        let mat = combined.to_mat3().unwrap();
        // Combined rotation should be finite and orthonormal-ish.
        for row in mat.rows {
            let len = (row[0] * row[0] + row[1] * row[1] + row[2] * row[2]).sqrt();
            assert!((len - 1.0).abs() < 1e-4);
        }
    }
}
