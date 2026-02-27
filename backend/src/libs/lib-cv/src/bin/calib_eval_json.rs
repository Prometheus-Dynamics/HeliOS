use std::path::PathBuf;

use lib_cv::Point;
use lib_cv::calibration::{CalibrationPointPair, CalibrationSolveConfig, CalibrationView, solve_camera_intrinsics_with_hint};
use lib_cv::modules::calibration::LensModel;
use lib_cv::modules::localization::CameraIntrinsics;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Input {
    width: u32,
    height: u32,
    views: Vec<InputView>,
}

#[derive(Debug, Deserialize)]
struct InputView {
    image: String,
    pairs: Vec<InputPair>,
}

#[derive(Debug, Deserialize)]
struct InputPair {
    obj: [f64; 2],
    img: [f64; 2],
}

fn main() {
    let path: PathBuf = std::env::args_os().nth(1).map(PathBuf::from).expect("usage: calib_eval_json <views.json>");

    let bytes = std::fs::read(&path).expect("read json");
    let input: Input = serde_json::from_slice(&bytes).expect("parse json");

    let mut views = Vec::new();
    for view in input.views {
        let mut pairs = Vec::with_capacity(view.pairs.len());
        for pair in view.pairs {
            pairs.push(CalibrationPointPair::new(Point { x: pair.obj[0], y: pair.obj[1] }, Point { x: pair.img[0], y: pair.img[1] }));
        }
        println!("view {} points={}", view.image, pairs.len());
        views.push(CalibrationView::new(pairs));
    }

    let cfg = CalibrationSolveConfig { lens_model: LensModel::Fisheye, ..Default::default() };
    let hint = Some(CameraIntrinsics { fx: (input.width as f64) * 0.5, fy: (input.width as f64) * 0.5, cx: (input.width as f64) * 0.5, cy: (input.height as f64) * 0.5 });

    let solved = solve_camera_intrinsics_with_hint(&views, cfg, hint).expect("solve");
    println!("reproj_px={:.4} views_used={} points_used={} lens={:?}", solved.reprojection_error_px, solved.views_used, solved.points_used, solved.calibration.lens_model);
    println!(
        "fx={:.3} fy={:.3} cx={:.3} cy={:.3} k1={:.6} k2={:.6} k3={:.6} p1={:.6} p2={:.6}",
        solved.calibration.intrinsics.fx,
        solved.calibration.intrinsics.fy,
        solved.calibration.intrinsics.cx,
        solved.calibration.intrinsics.cy,
        solved.calibration.distortion.k1,
        solved.calibration.distortion.k2,
        solved.calibration.distortion.k3,
        solved.calibration.distortion.p1,
        solved.calibration.distortion.p2
    );
}
