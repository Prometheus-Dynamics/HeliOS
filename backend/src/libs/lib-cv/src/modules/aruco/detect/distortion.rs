use super::*;

fn distort_norm_pinhole(x: f32, y: f32, calib: CameraCalibration) -> (f32, f32) {
    let r2 = x * x + y * y;
    let r4 = r2 * r2;
    let r6 = r4 * r2;
    let radial = 1.0 + calib.k1 * r2 + calib.k2 * r4 + calib.k3 * r6;
    let xy2 = 2.0 * x * y;
    let x2 = x * x;
    let y2 = y * y;
    let xt = calib.p1 * xy2 + calib.p2 * (r2 + 2.0 * x2);
    let yt = calib.p1 * (r2 + 2.0 * y2) + calib.p2 * xy2;
    (x * radial + xt, y * radial + yt)
}

fn distort_norm_fisheye(x: f32, y: f32, calib: CameraCalibration) -> (f32, f32) {
    let r = (x * x + y * y).sqrt();
    if !r.is_finite() || r <= 1e-6 {
        return (x, y);
    }
    let theta = r.atan();
    let theta2 = theta * theta;
    let theta4 = theta2 * theta2;
    let theta6 = theta4 * theta2;
    let theta8 = theta4 * theta4;
    let theta10 = theta8 * theta2;
    let k4 = calib.p1;
    let k5 = calib.p2;
    let theta_d = theta * (1.0 + calib.k1 * theta2 + calib.k2 * theta4 + calib.k3 * theta6 + k4 * theta8 + k5 * theta10);
    let scale = theta_d / r;
    (x * scale, y * scale)
}

pub(super) fn distort_norm(x: f32, y: f32, calib: CameraCalibration) -> (f32, f32) {
    match calib.lens_model {
        LensModel::Pinhole => distort_norm_pinhole(x, y, calib),
        LensModel::Fisheye => distort_norm_fisheye(x, y, calib),
    }
}

pub(super) fn undistort_norm(xd: f32, yd: f32, calib: CameraCalibration) -> (f32, f32) {
    let mut x = xd;
    let mut y = yd;
    let iters = calib.undistort_iters.clamp(1, 32);
    for _ in 0..iters {
        let (px, py) = distort_norm(x, y, calib);
        x -= px - xd;
        y -= py - yd;
    }
    (x, y)
}
