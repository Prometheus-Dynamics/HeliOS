use lib_cv::Point;
use lib_cv::calibration::{CalibrationPointPair, CalibrationSolveConfig, CalibrationView, solve_camera_intrinsics, solve_camera_intrinsics_with_hint};
use lib_cv::localization::CameraIntrinsics;
use nalgebra::{Rotation3, Vector3};

fn project_point(obj: (f64, f64), rotation: &Rotation3<f64>, translation: &Vector3<f64>, intrinsics: CameraIntrinsics) -> Point {
    let p = Vector3::new(obj.0, obj.1, 0.0);
    let cam = rotation * p + translation;
    let u = intrinsics.fx * (cam.x / cam.z) + intrinsics.cx;
    let v = intrinsics.fy * (cam.y / cam.z) + intrinsics.cy;
    Point { x: u, y: v }
}

fn distort_norm_fisheye(x: f64, y: f64, k1: f64, k2: f64, k3: f64, k4: f64) -> (f64, f64) {
    let r = (x * x + y * y).sqrt();
    if !r.is_finite() || r <= 1.0e-12 {
        return (x, y);
    }
    let theta = r.atan();
    let theta2 = theta * theta;
    let theta4 = theta2 * theta2;
    let theta6 = theta4 * theta2;
    let theta8 = theta4 * theta4;
    let theta_d = theta * (1.0 + k1 * theta2 + k2 * theta4 + k3 * theta6 + k4 * theta8);
    let scale = theta_d / r;
    (x * scale, y * scale)
}

fn project_point_fisheye(obj: (f64, f64), rotation: &Rotation3<f64>, translation: &Vector3<f64>, intrinsics: CameraIntrinsics, d: (f64, f64, f64, f64)) -> Point {
    let p = Vector3::new(obj.0, obj.1, 0.0);
    let cam = rotation * p + translation;
    let x = cam.x / cam.z;
    let y = cam.y / cam.z;
    let (xd, yd) = distort_norm_fisheye(x, y, d.0, d.1, d.2, d.3);
    let u = intrinsics.fx * xd + intrinsics.cx;
    let v = intrinsics.fy * yd + intrinsics.cy;
    Point { x: u, y: v }
}

fn build_view(obj_points: &[(f64, f64)], rotation: Rotation3<f64>, translation: Vector3<f64>, intrinsics: CameraIntrinsics) -> CalibrationView {
    let points = obj_points
        .iter()
        .map(|&obj| {
            let img = project_point(obj, &rotation, &translation, intrinsics);
            CalibrationPointPair::new(Point { x: obj.0, y: obj.1 }, img)
        })
        .collect();
    CalibrationView::new(points)
}

fn build_view_fisheye(obj_points: &[(f64, f64)], rotation: Rotation3<f64>, translation: Vector3<f64>, intrinsics: CameraIntrinsics, d: (f64, f64, f64, f64)) -> CalibrationView {
    let points = obj_points
        .iter()
        .map(|&obj| {
            let img = project_point_fisheye(obj, &rotation, &translation, intrinsics, d);
            CalibrationPointPair::new(Point { x: obj.0, y: obj.1 }, img)
        })
        .collect();
    CalibrationView::new(points)
}

#[test]
fn solve_intrinsics_from_planar_views() {
    let intrinsics = CameraIntrinsics::new(820.0, 810.0, 320.0, 240.0);

    let mut obj_points = Vec::new();
    for y in 0..6 {
        for x in 0..8 {
            obj_points.push((x as f64 * 0.04, y as f64 * 0.04));
        }
    }

    let views = vec![
        build_view(&obj_points, Rotation3::from_euler_angles(0.12, -0.05, 0.08), Vector3::new(0.05, -0.02, 2.2), intrinsics),
        build_view(&obj_points, Rotation3::from_euler_angles(-0.18, 0.15, -0.1), Vector3::new(-0.12, 0.06, 2.0), intrinsics),
        build_view(&obj_points, Rotation3::from_euler_angles(0.05, -0.22, 0.18), Vector3::new(0.0, 0.12, 2.4), intrinsics),
    ];

    let result = solve_camera_intrinsics(&views, CalibrationSolveConfig::default()).expect("solve failed");
    let solved = result.calibration.intrinsics;

    assert!((solved.fx - intrinsics.fx).abs() < 3.0);
    assert!((solved.fy - intrinsics.fy).abs() < 3.0);
    assert!((solved.cx - intrinsics.cx).abs() < 2.0);
    assert!((solved.cy - intrinsics.cy).abs() < 2.0);
    assert!(result.reprojection_error_px < 1e-2);

    let dist = result.calibration.distortion;
    assert!(dist.k1.abs() < 1e-2);
    assert!(dist.k2.abs() < 1e-2);
    assert!(dist.k3.abs() < 1e-2);
    assert!(dist.p1.abs() < 1e-2);
    assert!(dist.p2.abs() < 1e-2);
}

#[test]
fn solve_intrinsics_from_planar_views_fisheye() {
    // Synthetic fisheye dataset: ensure the multi-start + BA path converges to sane parameters.
    let intrinsics = CameraIntrinsics::new(570.0, 571.0, 640.0, 400.0);
    let d = (-0.007, 0.007, 0.004, -0.003); // OpenCV fisheye k1..k4

    let mut obj_points = Vec::new();
    for y in 0..13 {
        for x in 0..9 {
            obj_points.push((x as f64 * 0.02, y as f64 * 0.02));
        }
    }

    let poses = [
        (0.12, -0.05, 0.08, 0.05, -0.02, 1.2),
        (-0.18, 0.15, -0.10, -0.12, 0.06, 1.1),
        (0.05, -0.22, 0.18, 0.0, 0.12, 1.35),
        (0.28, 0.10, -0.05, 0.18, -0.09, 1.05),
        (-0.25, -0.12, 0.22, -0.22, 0.03, 1.4),
        (0.10, 0.30, 0.12, 0.12, 0.18, 1.25),
        (-0.32, 0.05, -0.18, -0.18, -0.14, 1.15),
        (0.22, -0.28, 0.04, 0.26, 0.05, 1.3),
    ];
    let views =
        poses.into_iter().map(|(rx, ry, rz, tx, ty, tz)| build_view_fisheye(&obj_points, Rotation3::from_euler_angles(rx, ry, rz), Vector3::new(tx, ty, tz), intrinsics, d)).collect::<Vec<_>>();

    let cfg = CalibrationSolveConfig { lens_model: lib_cv::calibration::LensModel::Fisheye, refine_undistort_iters: 10, ..CalibrationSolveConfig::default() };
    let result = solve_camera_intrinsics_with_hint(&views, cfg, Some(intrinsics)).expect("fisheye solve failed");
    let solved = result.calibration.intrinsics;
    let dist = result.calibration.distortion;

    assert!((solved.fx - intrinsics.fx).abs() < 10.0);
    assert!((solved.fy - intrinsics.fy).abs() < 10.0);
    assert!((solved.cx - intrinsics.cx).abs() < 10.0);
    assert!((solved.cy - intrinsics.cy).abs() < 10.0);

    // For fisheye we store k4 in `p1` and ignore `p2`.
    assert!((dist.k1 - d.0).abs() < 0.05);
    assert!((dist.k2 - d.1).abs() < 0.05);
    assert!((dist.k3 - d.2).abs() < 0.05);
    assert!((dist.p1 - d.3).abs() < 0.05);
    assert!(dist.p2.abs() < 1e-6);

    assert!(result.reprojection_error_px < 1.0);
}
