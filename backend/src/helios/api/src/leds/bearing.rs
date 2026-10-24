//! A tag's bearing from the camera's forward axis, in degrees, positive to the right of the image
//! centre. With a calibration the ray through the tag's centre is undistorted and its horizontal
//! angle taken (pinhole: Brown-Conrady; fisheye: equidistant). Without one, the centre's x
//! fraction times the configured horizontal field of view.

use crate::camera_context::{CameraCalibration, Distortion, LensModel};

/// Undistortion steps: the fixed-point iterations converge in a handful at lens distortions.
const UNDISTORT_STEPS: usize = 12;

/// The bearing of image point `(u, v)` in a `width` by `height` frame, in degrees.
pub fn bearing_deg(u: f64, v: f64, width: f64, height: f64, calibration: Option<&CameraCalibration>, fov_deg: f64) -> f64 {
    match calibration {
        Some(calibration) => calibrated_bearing_deg(u, v, width, height, calibration),
        None => (u / width - 0.5) * fov_deg,
    }
}

fn calibrated_bearing_deg(u: f64, v: f64, width: f64, height: f64, calibration: &CameraCalibration) -> f64 {
    // The calibration may be for another size of the same image: scale its intrinsics.
    let sx = width / f64::from(calibration.width);
    let sy = height / f64::from(calibration.height);
    let xd = (u - calibration.cx * sx) / (calibration.fx * sx);
    let yd = (v - calibration.cy * sy) / (calibration.fy * sy);
    let radians = match calibration.model {
        LensModel::Pinhole => undistort_pinhole(xd, yd, &calibration.distortion).0.atan(),
        LensModel::Fisheye => fisheye_bearing(xd, yd, &calibration.distortion),
    };
    radians.to_degrees()
}

/// The undistorted normalised point (x, y) of a distorted one, by fixed-point iteration on the
/// Brown-Conrady model (OpenCV's eight coefficients).
pub fn undistort_pinhole(xd: f64, yd: f64, d: &Distortion) -> (f64, f64) {
    let (mut x, mut y) = (xd, yd);
    for _ in 0..UNDISTORT_STEPS {
        let r2 = x * x + y * y;
        let r4 = r2 * r2;
        let r6 = r4 * r2;
        let radial = (1.0 + d.k1 * r2 + d.k2 * r4 + d.k3 * r6) / (1.0 + d.k4 * r2 + d.k5 * r4 + d.k6 * r6);
        let dx = 2.0 * d.p1 * x * y + d.p2 * (r2 + 2.0 * x * x);
        let dy = d.p1 * (r2 + 2.0 * y * y) + 2.0 * d.p2 * x * y;
        x = (xd - dx) / radial;
        y = (yd - dy) / radial;
    }
    (x, y)
}

/// The horizontal bearing (radians) of a fisheye point: the equidistant model's distorted angle
/// `theta_d = theta (1 + k1 theta^2 + k2 theta^4 + k3 theta^6 + k4 theta^8)` is inverted by Newton's
/// method, and the ray's angle taken in the horizontal plane.
pub fn fisheye_bearing(xd: f64, yd: f64, d: &Distortion) -> f64 {
    let theta_d = xd.hypot(yd);
    if theta_d < 1e-12 {
        return 0.0;
    }
    let theta = invert_equidistant(theta_d, d);
    let phi = yd.atan2(xd);
    (theta.sin() * phi.cos()).atan2(theta.cos())
}

fn invert_equidistant(theta_d: f64, d: &Distortion) -> f64 {
    let mut theta = theta_d;
    for _ in 0..UNDISTORT_STEPS {
        let t2 = theta * theta;
        let value = theta * (1.0 + t2 * (d.k1 + t2 * (d.k2 + t2 * (d.k3 + t2 * d.k4)))) - theta_d;
        let slope = 1.0 + t2 * (3.0 * d.k1 + t2 * (5.0 * d.k2 + t2 * (7.0 * d.k3 + t2 * 9.0 * d.k4)));
        if slope.abs() < 1e-9 {
            break;
        }
        theta -= value / slope;
    }
    theta
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calibration(model: LensModel, distortion: Distortion) -> CameraCalibration {
        CameraCalibration { width: 640, height: 400, model, fx: 500.0, fy: 500.0, cx: 320.0, cy: 200.0, distortion, rms_px: None, uncertainty: None, source: None, orientation: Default::default(), saved_at_ms: 0, provenance: None }
    }

    #[test]
    fn without_a_calibration_the_fov_is_linear_across_the_frame() {
        assert!(bearing_deg(320.0, 200.0, 640.0, 400.0, None, 90.0).abs() < 1e-9, "the centre is forward");
        assert!((bearing_deg(640.0, 200.0, 640.0, 400.0, None, 90.0) - 45.0).abs() < 1e-9, "the right edge is half the fov");
        assert!((bearing_deg(0.0, 200.0, 640.0, 400.0, None, 90.0) + 45.0).abs() < 1e-9, "the left edge is minus half");
        assert!((bearing_deg(480.0, 0.0, 640.0, 400.0, None, 60.0) - 15.0).abs() < 1e-9, "a quarter across: 15 of 60 degrees");
    }

    #[test]
    fn a_calibrated_pinhole_gives_the_angle_of_the_undistorted_ray() {
        let cal = calibration(LensModel::Pinhole, Distortion::default());
        // u = cx + fx tan(angle): 45 degrees is one unit of tangent.
        let u = 320.0 + 500.0 * 1.0;
        assert!((bearing_deg(u, 200.0, 640.0, 400.0, Some(&cal), 90.0) - 45.0).abs() < 1e-9);
        let u = 320.0 - 500.0 * 0.5_f64.atan().tan();
        assert!((bearing_deg(u, 200.0, 640.0, 400.0, Some(&cal), 90.0) + 26.565).abs() < 1e-3);
    }

    #[test]
    fn radial_distortion_is_undone_by_the_inverse_of_the_forward_model() {
        let d = Distortion { k1: -0.2, k2: 0.05, ..Distortion::default() };
        let (xu, yu) = (0.3, -0.2);
        let r2 = xu * xu + yu * yu;
        let scale = 1.0 + d.k1 * r2 + d.k2 * r2 * r2;
        let (x, y) = undistort_pinhole(xu * scale, yu * scale, &d);
        assert!((x - xu).abs() < 1e-9 && (y - yu).abs() < 1e-9, "({x}, {y})");
    }

    #[test]
    fn a_fisheye_maps_the_equidistant_angle_back() {
        let flat = Distortion::default();
        let theta: f64 = 0.5;
        // An equidistant image point is theta from the centre along the direction phi.
        let (xd, yd) = (theta * 0.6, theta * 0.8);
        let bearing = fisheye_bearing(xd, yd, &flat);
        let expected = (theta.sin() * 0.6).atan2(theta.cos());
        assert!((bearing - expected).abs() < 1e-9);
        // Straight ahead is zero; the far side of the lens is the negative of the near side.
        assert_eq!(fisheye_bearing(0.0, 0.0, &flat), 0.0);
        assert!((fisheye_bearing(-xd, yd, &flat) + expected).abs() < 1e-9);
    }

    #[test]
    fn a_distorted_fisheye_round_trips_through_its_forward_model() {
        let d = Distortion { k1: 0.05, k2: -0.01, k3: 0.0, k4: 0.0, ..Distortion::default() };
        let theta: f64 = 0.9;
        let theta_d = theta + d.k1 * theta.powi(3) + d.k2 * theta.powi(5);
        let recovered = invert_equidistant(theta_d, &d);
        assert!((recovered - theta).abs() < 1e-9, "{recovered}");
    }

    #[test]
    fn a_calibration_for_another_size_is_scaled() {
        let half = CameraCalibration { width: 320, height: 200, fx: 250.0, fy: 250.0, cx: 160.0, cy: 100.0, ..calibration(LensModel::Pinhole, Distortion::default()) };
        let full = calibration(LensModel::Pinhole, Distortion::default());
        let a = bearing_deg(400.0, 200.0, 640.0, 400.0, Some(&half), 90.0);
        let b = bearing_deg(400.0, 200.0, 640.0, 400.0, Some(&full), 90.0);
        assert!((a - b).abs() < 1e-9, "{a} {b}");
    }
}
