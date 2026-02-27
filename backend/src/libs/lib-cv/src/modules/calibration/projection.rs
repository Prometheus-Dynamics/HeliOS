use super::types::*;
use nalgebra::Vector3;

pub(super) fn project_normalized(obj: Point2, ext: &Extrinsics) -> (f64, f64) {
    let p = Vector3::new(obj.0, obj.1, 0.0);
    let cam = ext.r * p + ext.t;
    if cam.z.abs() < 1e-12 {
        return (f64::NAN, f64::NAN);
    }
    (cam.x / cam.z, cam.y / cam.z)
}

pub(super) fn distort_norm_pinhole(x: f64, y: f64, dist: DistortionCoefficients) -> (f64, f64) {
    let r2 = x * x + y * y;
    let r4 = r2 * r2;
    let r6 = r4 * r2;
    let radial = 1.0 + dist.k1 * r2 + dist.k2 * r4 + dist.k3 * r6;
    let xy2 = 2.0 * x * y;
    let x2 = x * x;
    let y2 = y * y;
    let xt = dist.p1 * xy2 + dist.p2 * (r2 + 2.0 * x2);
    let yt = dist.p1 * (r2 + 2.0 * y2) + dist.p2 * xy2;
    (x * radial + xt, y * radial + yt)
}

pub(super) fn distort_norm_fisheye(x: f64, y: f64, dist: DistortionCoefficients) -> (f64, f64) {
    let r = (x * x + y * y).sqrt();
    if !r.is_finite() || r <= 1e-12 {
        return (x, y);
    }
    let theta = r.atan();
    let theta2 = theta * theta;
    let theta4 = theta2 * theta2;
    let theta6 = theta4 * theta2;
    let theta8 = theta4 * theta4;
    // OpenCV fisheye model:
    // theta_d = theta * (1 + k1*theta^2 + k2*theta^4 + k3*theta^6 + k4*theta^8)
    // We store k4 in `p1`. `p2` is unused for fisheye.
    let k4 = dist.p1;
    let theta_d = theta * (1.0 + dist.k1 * theta2 + dist.k2 * theta4 + dist.k3 * theta6 + k4 * theta8);
    let scale = theta_d / r;
    (x * scale, y * scale)
}

pub(super) fn distort_norm(x: f64, y: f64, dist: DistortionCoefficients, lens_model: LensModel) -> (f64, f64) {
    match lens_model {
        LensModel::Pinhole => distort_norm_pinhole(x, y, dist),
        LensModel::Fisheye => distort_norm_fisheye(x, y, dist),
    }
}

pub(super) fn reprojection_error(views: &[ViewObservations], extrinsics: &[Extrinsics], intrinsics: IntrinsicsSolve, dist: DistortionCoefficients, lens_model: LensModel) -> f64 {
    let mut sum = 0.0;
    let mut n = 0u64;
    for (view, ext) in views.iter().zip(extrinsics.iter()) {
        for (obj, img) in &view.points {
            let (x, y) = project_normalized(*obj, ext);
            if !x.is_finite() || !y.is_finite() {
                continue;
            }
            let (xd, yd) = distort_norm(x, y, dist, lens_model);
            let u = intrinsics.fx * xd + intrinsics.cx + intrinsics.skew * yd;
            let v = intrinsics.fy * yd + intrinsics.cy;
            let dx = u - img.0;
            let dy = v - img.1;
            if dx.is_finite() && dy.is_finite() {
                sum += dx * dx + dy * dy;
                n += 1;
            }
        }
    }
    if n == 0 {
        return 0.0;
    }
    (sum / (n as f64)).sqrt()
}
pub(super) fn undistort_iter(xd: f64, yd: f64, dist: DistortionCoefficients, lens_model: LensModel, iters: u8) -> (f64, f64) {
    let mut x = xd;
    let mut y = yd;
    let steps = iters.clamp(1, 32);
    for _ in 0..steps {
        let (px, py) = distort_norm(x, y, dist, lens_model);
        x -= px - xd;
        y -= py - yd;
    }
    (x, y)
}
