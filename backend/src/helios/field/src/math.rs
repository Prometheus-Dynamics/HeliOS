//! The rigid-transform math a field layout needs, without Eidos: rotations as row-major 3x3
//! matrices, quaternions to and from them, and composition. The formulas are Eidos's
//! (`eidos_calibration::pose`), so a layout's values are the same with or without the `eidos`
//! feature.

use crate::{Quat, Vec3};

/// A 3x3 matrix, row-major (`m[row][column]`).
pub type Mat3 = [[f64; 3]; 3];

/// A rigid transform `a_from_b`: a point `p` of frame `b` is `rotation * p + translation` in `a`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rigid {
    pub rotation: Mat3,
    pub translation: Vec3,
}

pub(crate) fn matmul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut m = [[0.0; 3]; 3];
    for (i, row) in m.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
        }
    }
    m
}

fn mul_vec(m: &Mat3, v: Vec3) -> Vec3 {
    Vec3 { x: m[0][0] * v.x + m[0][1] * v.y + m[0][2] * v.z, y: m[1][0] * v.x + m[1][1] * v.y + m[1][2] * v.z, z: m[2][0] * v.x + m[2][1] * v.y + m[2][2] * v.z }
}

/// `a * b`: `b`'s frame through `a`.
pub fn compose(a: Rigid, b: Rigid) -> Rigid {
    let bt = mul_vec(&a.rotation, b.translation);
    Rigid { rotation: matmul(&a.rotation, &b.rotation), translation: Vec3 { x: a.translation.x + bt.x, y: a.translation.y + bt.y, z: a.translation.z + bt.z } }
}

/// `q` at unit length; the identity for a zero or non-finite `q`.
fn normalized(q: Quat) -> Quat {
    let norm = (q.w * q.w + q.x * q.x + q.y * q.y + q.z * q.z).sqrt();
    if !norm.is_finite() || norm <= 1e-12 {
        return Quat::default();
    }
    Quat { w: q.w / norm, x: q.x / norm, y: q.y / norm, z: q.z / norm }
}

pub(crate) fn rotation_from_quaternion(q: Quat) -> Mat3 {
    let q = normalized(q);
    let (xx, yy, zz) = (q.x * q.x, q.y * q.y, q.z * q.z);
    let (xy, xz, yz) = (q.x * q.y, q.x * q.z, q.y * q.z);
    let (wx, wy, wz) = (q.w * q.x, q.w * q.y, q.w * q.z);
    [[1.0 - 2.0 * (yy + zz), 2.0 * (xy - wz), 2.0 * (xz + wy)], [2.0 * (xy + wz), 1.0 - 2.0 * (xx + zz), 2.0 * (yz - wx)], [2.0 * (xz - wy), 2.0 * (yz + wx), 1.0 - 2.0 * (xx + yy)]]
}

pub(crate) fn quaternion_from_rotation(m: &Mat3) -> Quat {
    let trace = m[0][0] + m[1][1] + m[2][2];
    let (w, x, y, z) = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        (0.25 * s, (m[2][1] - m[1][2]) / s, (m[0][2] - m[2][0]) / s, (m[1][0] - m[0][1]) / s)
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
        ((m[2][1] - m[1][2]) / s, 0.25 * s, (m[0][1] + m[1][0]) / s, (m[0][2] + m[2][0]) / s)
    } else if m[1][1] > m[2][2] {
        let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
        ((m[0][2] - m[2][0]) / s, (m[0][1] + m[1][0]) / s, 0.25 * s, (m[1][2] + m[2][1]) / s)
    } else {
        let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
        ((m[1][0] - m[0][1]) / s, (m[0][2] + m[2][0]) / s, (m[1][2] + m[2][1]) / s, 0.25 * s)
    };
    normalized(Quat { w, x, y, z })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quaternion_survives_the_matrix_and_back() {
        let q = normalized(Quat { w: 0.3, x: -0.5, y: 0.7, z: 0.2 });
        let back = quaternion_from_rotation(&rotation_from_quaternion(q));
        let same = |a: Quat, b: Quat| (a.w - b.w).abs() + (a.x - b.x).abs() + (a.y - b.y).abs() + (a.z - b.z).abs() < 1e-12;
        assert!(same(back, q) || same(back, Quat { w: -q.w, x: -q.x, y: -q.y, z: -q.z }), "{back:?} vs {q:?}");
    }

    #[test]
    fn compose_turns_then_moves() {
        let quarter = Rigid { rotation: [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]], translation: Vec3 { x: 1.0, y: 0.0, z: 0.0 } };
        let step = Rigid { rotation: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], translation: Vec3 { x: 2.0, y: 0.0, z: 0.0 } };
        assert_eq!(compose(quarter, step).translation, Vec3 { x: 1.0, y: 2.0, z: 0.0 });
    }
}
