//! Frames and rotations. WPILib (X forward, Y left, Z up, metres) to glTF (X right, Y up, Z back,
//! metres), and the camera's rotation in each.
//!
//! A WPILib point `(x, y, z)` is the glTF point `(x, z, -y)`. A WPILib camera body is
//! `+X` forward, `+Y` left, `+Z` up. glTF's camera looks along its `-Z` with `+Y` up, so its
//! `+X` is the body's right (`-Y`), its `+Y` is the body's up (`+Z`) and its `+Z` is the body's
//! back (`-X`). [`camera_node_rotation`] is the glTF rotation of the camera node.

/// A 3x3 matrix, row-major: `m[row][column]`.
pub type Mat3 = [[f64; 3]; 3];
pub type Vec3 = [f64; 3];

/// WPILib to glTF for a vector: `(x, y, z) -> (x, z, -y)`.
pub const GLTF_FROM_WPILIB: Mat3 = [[1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]];

/// glTF camera axes in the WPILib camera body: the columns are the body's `(-Y, +Z, -X)`.
const BODY_FROM_GLTF_CAMERA: Mat3 = [[0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];

pub fn mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut out = [[0.0; 3]; 3];
    for (r, row) in out.iter_mut().enumerate() {
        for (c, value) in row.iter_mut().enumerate() {
            *value = (0..3).map(|k| a[r][k] * b[k][c]).sum();
        }
    }
    out
}

pub fn mul_v(a: &Mat3, v: Vec3) -> Vec3 {
    [0, 1, 2].map(|r| a[r][0] * v[0] + a[r][1] * v[1] + a[r][2] * v[2])
}

pub fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// WPILib's point to glTF's.
pub fn to_gltf(p: Vec3) -> Vec3 {
    mul_v(&GLTF_FROM_WPILIB, p)
}

/// A WPILib rotation: roll about X, then pitch about Y, then yaw about Z, about the fixed axes
/// (`Rz(yaw) * Ry(pitch) * Rx(roll)`), in degrees.
pub fn rotation_from_rpy_degrees(roll: f64, pitch: f64, yaw: f64) -> Mat3 {
    let (sr, cr) = roll.to_radians().sin_cos();
    let (sp, cp) = pitch.to_radians().sin_cos();
    let (sy, cy) = yaw.to_radians().sin_cos();
    let rx = [[1.0, 0.0, 0.0], [0.0, cr, -sr], [0.0, sr, cr]];
    let ry = [[cp, 0.0, sp], [0.0, 1.0, 0.0], [-sp, 0.0, cp]];
    let rz = [[cy, -sy, 0.0], [sy, cy, 0.0], [0.0, 0.0, 1.0]];
    mul(&rz, &mul(&ry, &rx))
}

/// The glTF rotation of the camera node, from the camera body's rotation in the WPILib world
/// (its axes as columns, as [`rotation_from_rpy_degrees`] gives them).
pub fn camera_node_rotation(wpilib_body_rotation: &Mat3) -> Mat3 {
    mul(&GLTF_FROM_WPILIB, &mul(wpilib_body_rotation, &BODY_FROM_GLTF_CAMERA))
}

/// A rotation matrix's quaternion `[x, y, z, w]` (glTF's order), with `w >= 0`.
pub fn matrix_to_quat(m: &Mat3) -> [f64; 4] {
    let trace = m[0][0] + m[1][1] + m[2][2];
    let q = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [(m[2][1] - m[1][2]) / s, (m[0][2] - m[2][0]) / s, (m[1][0] - m[0][1]) / s, 0.25 * s]
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
        [0.25 * s, (m[0][1] + m[1][0]) / s, (m[0][2] + m[2][0]) / s, (m[2][1] - m[1][2]) / s]
    } else if m[1][1] > m[2][2] {
        let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
        [(m[0][1] + m[1][0]) / s, 0.25 * s, (m[1][2] + m[2][1]) / s, (m[0][2] - m[2][0]) / s]
    } else {
        let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
        [(m[0][2] + m[2][0]) / s, (m[1][2] + m[2][1]) / s, 0.25 * s, (m[1][0] - m[0][1]) / s]
    };
    let sign = if q[3] < 0.0 { -1.0 } else { 1.0 };
    let norm = q.iter().map(|v| v * v).sum::<f64>().sqrt();
    q.map(|v| sign * v / norm)
}

/// A unit quaternion `(w, x, y, z)` as a rotation matrix.
pub fn quat_to_matrix(w: f64, x: f64, y: f64, z: f64) -> Mat3 {
    [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - w * z), 2.0 * (x * z + w * y)],
        [2.0 * (x * y + w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - w * x)],
        [2.0 * (x * z - w * y), 2.0 * (y * z + w * x), 1.0 - 2.0 * (x * x + y * y)],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        (0..3).all(|i| (a[i] - b[i]).abs() < 1e-9)
    }

    #[test]
    fn points_convert_from_wpilib_to_gltf() {
        assert_eq!(to_gltf([1.0, 2.0, 3.0]), [1.0, 3.0, -2.0]);
        // Forward (+X) is glTF's +X; left (+Y) is glTF's -Z (its "forward" for a glTF camera).
        assert_eq!(to_gltf([1.0, 0.0, 0.0]), [1.0, 0.0, 0.0]);
        assert_eq!(to_gltf([0.0, 1.0, 0.0]), [0.0, 0.0, -1.0]);
    }

    #[test]
    fn an_unrotated_camera_looks_forward_with_up_and_right_as_gltf_has_them() {
        let r = camera_node_rotation(&rotation_from_rpy_degrees(0.0, 0.0, 0.0));
        // glTF camera: forward is -Z, up is +Y, right is +X.
        assert!(close(mul_v(&r, [0.0, 0.0, -1.0]), to_gltf([1.0, 0.0, 0.0])), "forward");
        assert!(close(mul_v(&r, [0.0, 1.0, 0.0]), to_gltf([0.0, 0.0, 1.0])), "up");
        assert!(close(mul_v(&r, [1.0, 0.0, 0.0]), to_gltf([0.0, -1.0, 0.0])), "right");
    }

    #[test]
    fn a_yawed_camera_keeps_its_axes_in_the_body_frame() {
        // Yaw 90: the body looks along WPILib +Y (glTF -Z is then the body's forward).
        let body = rotation_from_rpy_degrees(0.0, 0.0, 90.0);
        let r = camera_node_rotation(&body);
        assert!(close(mul_v(&r, [0.0, 0.0, -1.0]), to_gltf(mul_v(&body, [1.0, 0.0, 0.0]))), "forward follows the body");
        assert!(close(mul_v(&r, [0.0, 1.0, 0.0]), to_gltf(mul_v(&body, [0.0, 0.0, 1.0]))), "up follows the body");
        assert!(close(mul_v(&r, [1.0, 0.0, 0.0]), to_gltf(mul_v(&body, [0.0, -1.0, 0.0]))), "right follows the body");
    }

    #[test]
    fn quaternions_round_trip_and_are_normalised() {
        for (roll, pitch, yaw) in [(0.0, 0.0, 0.0), (10.0, -20.0, 110.0), (179.0, 45.0, -170.0), (0.0, 90.0, 0.0)] {
            let m = camera_node_rotation(&rotation_from_rpy_degrees(roll, pitch, yaw));
            let [x, y, z, w] = matrix_to_quat(&m);
            assert!(w >= 0.0);
            assert!((x * x + y * y + z * z + w * w - 1.0).abs() < 1e-12);
            let back = quat_to_matrix(w, x, y, z);
            for r in 0..3 {
                for c in 0..3 {
                    assert!((back[r][c] - m[r][c]).abs() < 1e-9, "{roll} {pitch} {yaw}");
                }
            }
        }
    }
}
