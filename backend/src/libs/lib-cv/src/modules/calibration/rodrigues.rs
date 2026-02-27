use nalgebra::{Matrix3, Vector3};

pub(super) fn matrix_to_rodrigues(r: &Matrix3<f64>) -> Vector3<f64> {
    let trace = r[(0, 0)] + r[(1, 1)] + r[(2, 2)];
    let cos_theta = ((trace - 1.0) * 0.5).clamp(-1.0, 1.0);
    let theta = cos_theta.acos();
    if theta.abs() < 1e-9 {
        return Vector3::new(0.0, 0.0, 0.0);
    }
    let denom = 2.0 * theta.sin();
    let rx = (r[(2, 1)] - r[(1, 2)]) / denom;
    let ry = (r[(0, 2)] - r[(2, 0)]) / denom;
    let rz = (r[(1, 0)] - r[(0, 1)]) / denom;
    Vector3::new(rx * theta, ry * theta, rz * theta)
}

pub(super) fn rodrigues_to_matrix(rvec: Vector3<f64>) -> Matrix3<f64> {
    let theta = rvec.norm();
    if theta.abs() < 1e-9 {
        return Matrix3::identity();
    }
    let axis = rvec / theta;
    let (x, y, z) = (axis.x, axis.y, axis.z);
    let cos_t = theta.cos();
    let sin_t = theta.sin();
    let one_minus = 1.0 - cos_t;

    Matrix3::new(
        cos_t + x * x * one_minus,
        x * y * one_minus - z * sin_t,
        x * z * one_minus + y * sin_t,
        y * x * one_minus + z * sin_t,
        cos_t + y * y * one_minus,
        y * z * one_minus - x * sin_t,
        z * x * one_minus - y * sin_t,
        z * y * one_minus + x * sin_t,
        cos_t + z * z * one_minus,
    )
}
