use crate::linalg::{LinalgError, Vec3, Vec4};

/// 3x3 matrix supporting the rotation math we rely on in pose estimation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat3 {
    pub rows: [[f32; 3]; 3],
}

impl Mat3 {
    pub const IDENTITY: Self = Self { rows: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]] };

    pub const fn from_rows(rows: [[f32; 3]; 3]) -> Self {
        Self { rows }
    }

    pub fn transpose(self) -> Self {
        let mut out = [[0.0; 3]; 3];
        for (i, row) in out.iter_mut().enumerate() {
            for (j, value) in row.iter_mut().enumerate() {
                *value = self.rows[j][i];
            }
        }
        Self { rows: out }
    }

    pub fn determinant(self) -> f32 {
        let m = self.rows;
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    }

    pub fn try_inverse(self) -> Result<Self, LinalgError> {
        let det = self.determinant();
        if det.abs() <= 1e-6 {
            return Err(LinalgError::SingularMatrix);
        }
        let m = self.rows;
        let adj = [
            [m[1][1] * m[2][2] - m[1][2] * m[2][1], m[0][2] * m[2][1] - m[0][1] * m[2][2], m[0][1] * m[1][2] - m[0][2] * m[1][1]],
            [m[1][2] * m[2][0] - m[1][0] * m[2][2], m[0][0] * m[2][2] - m[0][2] * m[2][0], m[0][2] * m[1][0] - m[0][0] * m[1][2]],
            [m[1][0] * m[2][1] - m[1][1] * m[2][0], m[0][1] * m[2][0] - m[0][0] * m[2][1], m[0][0] * m[1][1] - m[0][1] * m[1][0]],
        ];
        Ok(Self { rows: adj } / det)
    }

    pub fn mul_mat3(self, rhs: Self) -> Self {
        let mut out = [[0.0; 3]; 3];
        for (i, row) in out.iter_mut().enumerate() {
            for (j, value) in row.iter_mut().enumerate() {
                *value = (0..3).map(|k| self.rows[i][k] * rhs.rows[k][j]).sum::<f32>();
            }
        }
        Self { rows: out }
    }

    pub fn transform_vec3(self, v: Vec3) -> Vec3 {
        let data = self.rows;
        Vec3::new(data[0][0] * v.x + data[0][1] * v.y + data[0][2] * v.z, data[1][0] * v.x + data[1][1] * v.y + data[1][2] * v.z, data[2][0] * v.x + data[2][1] * v.y + data[2][2] * v.z)
    }
}

impl core::ops::Mul<f32> for Mat3 {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        let mut out = self.rows;
        for row in &mut out {
            for val in row {
                *val *= rhs;
            }
        }
        Self { rows: out }
    }
}

impl core::ops::Div<f32> for Mat3 {
    type Output = Self;

    fn div(self, rhs: f32) -> Self::Output {
        let mut out = self.rows;
        for row in &mut out {
            for val in row {
                *val /= rhs;
            }
        }
        Self { rows: out }
    }
}

/// 4x4 matrix used for homogeneous transformations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4 {
    pub rows: [[f32; 4]; 4],
}

impl Mat4 {
    pub const IDENTITY: Self = Self { rows: [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]] };

    pub fn from_translation(tx: f32, ty: f32, tz: f32) -> Self {
        let mut m = Self::IDENTITY;
        m.rows[0][3] = tx;
        m.rows[1][3] = ty;
        m.rows[2][3] = tz;
        m
    }

    pub fn from_basis(rotation: Mat3, translation: Vec3) -> Self {
        let mut rows = [[0.0; 4]; 4];
        for (i, row) in rows.iter_mut().take(3).enumerate() {
            row[..3].copy_from_slice(&rotation.rows[i]);
        }
        rows[0][3] = translation.x;
        rows[1][3] = translation.y;
        rows[2][3] = translation.z;
        rows[3] = [0.0, 0.0, 0.0, 1.0];
        Self { rows }
    }

    pub fn transform_vec4(self, v: Vec4) -> Vec4 {
        let data = self.rows;
        Vec4 {
            x: data[0][0] * v.x + data[0][1] * v.y + data[0][2] * v.z + data[0][3] * v.w,
            y: data[1][0] * v.x + data[1][1] * v.y + data[1][2] * v.z + data[1][3] * v.w,
            z: data[2][0] * v.x + data[2][1] * v.y + data[2][2] * v.z + data[2][3] * v.w,
            w: data[3][0] * v.x + data[3][1] * v.y + data[3][2] * v.z + data[3][3] * v.w,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mat3_inverse_round_trip() {
        let m = Mat3::from_rows([[3.0, 0.0, 2.0], [2.0, 0.0, -2.0], [0.0, 1.0, 1.0]]);
        let inv = m.try_inverse().unwrap();
        let identity = m.mul_mat3(inv);
        for i in 0..3 {
            for j in 0..3 {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!((identity.rows[i][j] - expected).abs() < 1e-4);
            }
        }
    }

    #[test]
    fn mat4_transform_translation() {
        let m = Mat4::from_translation(1.0, 2.0, 3.0);
        let v = Vec4::new(0.0, 0.0, 0.0, 1.0);
        let result = m.transform_vec4(v);
        assert_eq!(result, Vec4::new(1.0, 2.0, 3.0, 1.0));
    }
}
