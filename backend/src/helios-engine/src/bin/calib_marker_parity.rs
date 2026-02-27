use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use image::DynamicImage;
use lib_cv::calibration::{solve_camera_intrinsics_with_hint, CalibrationPointPair, CalibrationSolveConfig, CalibrationView, LensModel};
use lib_cv::modules::aruco::ArucoDetection2D;
use lib_cv::Point;

const CALIBRATION_TEMPLATE_ID: &str = "daedalus_aruco";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoardParity {
    EvenSquares,
    OddSquares,
}

impl BoardParity {
    fn is_marker_square(self, x: u32, y: u32) -> bool {
        let even = (x + y).is_multiple_of(2);
        match self {
            BoardParity::EvenSquares => even,
            BoardParity::OddSquares => !even,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoardOrdering {
    RowMajor,
    ColMajor,
}

#[derive(Debug, Clone)]
struct BoardLayout {
    tag_corners_by_id: HashMap<u32, [Point; 4]>,
}

impl BoardLayout {
    fn new(squares_x: u32, squares_y: u32, square_size: f64, marker_size: f64, parity: BoardParity, ordering: BoardOrdering) -> Self {
        let mut tag_corners_by_id = HashMap::new();
        let mut id = 0u32;
        let off = ((square_size - marker_size) * 0.5).max(0.0);

        let mut visit = Vec::new();
        match ordering {
            BoardOrdering::RowMajor => {
                for j in 0..squares_y {
                    for i in 0..squares_x {
                        visit.push((i, j));
                    }
                }
            }
            BoardOrdering::ColMajor => {
                for i in 0..squares_x {
                    for j in 0..squares_y {
                        visit.push((i, j));
                    }
                }
            }
        }

        for (i, j) in visit {
            if !parity.is_marker_square(i, j) {
                continue;
            }
            let sx0 = (i as f64) * square_size;
            let sy0 = (j as f64) * square_size;
            let mx0 = sx0 + off;
            let my0 = sy0 + off;
            let mx1 = mx0 + marker_size;
            let my1 = my0 + marker_size;
            tag_corners_by_id.insert(id, [Point { x: mx0, y: my0 }, Point { x: mx1, y: my0 }, Point { x: mx1, y: my1 }, Point { x: mx0, y: my1 }]);
            id = id.wrapping_add(1);
        }

        Self { tag_corners_by_id }
    }
}

fn order_marker_corners(rotation: u8, quad: [Point; 4]) -> [Point; 4] {
    let r = (rotation & 3) as usize;
    let shift = (4 - r) & 3;
    [quad[shift], quad[(shift + 1) & 3], quad[(shift + 2) & 3], quad[(shift + 3) & 3]]
}

fn parse_detections(raw: serde_json::Value) -> Vec<ArucoDetection2D> {
    // The JSON output is either an array (raw detections) or an object with `detections`.
    if let Some(arr) = raw.as_array() {
        serde_json::from_value::<Vec<ArucoDetection2D>>(serde_json::Value::Array(arr.clone())).unwrap_or_default()
    } else if let Some(arr) = raw.get("detections").and_then(|v| v.as_array()) {
        serde_json::from_value::<Vec<ArucoDetection2D>>(serde_json::Value::Array(arr.clone())).unwrap_or_default()
    } else {
        Vec::new()
    }
}

fn list_images(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|ent| ent.ok())
        .map(|ent| ent.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("jpg")).unwrap_or(false))
        .collect();
    out.sort();
    out
}

fn main() {
    let dir: PathBuf = std::env::args_os().nth(1).map(PathBuf::from).expect("usage: calib_marker_parity <dir-with-jpgs>");
    let images = list_images(&dir);
    if images.is_empty() {
        panic!("no .jpg images found in {dir:?}");
    }

    let squares_x = 9u32;
    let squares_y = 13u32;
    let square_size = 20.0f64;
    let marker_size = 14.0f64;

    let graph_json = helios_engine::pipelines::load_template_graph_json(CALIBRATION_TEMPLATE_ID).expect("load calibration template graph");
    let graph = helios_engine::graph::GraphHandle::from_json_with_output(2, &graph_json, Some("frame")).expect("build graph");

    let cfg = CalibrationSolveConfig { lens_model: LensModel::Fisheye, min_views: 2, min_points_per_view: 12, refine_distortion: true, undistort_iters: 5, refine_undistort_iters: 8 };

    let start = Instant::now();
    let candidates = [
        (BoardOrdering::RowMajor, BoardParity::OddSquares),
        (BoardOrdering::RowMajor, BoardParity::EvenSquares),
        (BoardOrdering::ColMajor, BoardParity::OddSquares),
        (BoardOrdering::ColMajor, BoardParity::EvenSquares),
    ];

    for (ordering, parity) in candidates {
        let layout = BoardLayout::new(squares_x, squares_y, square_size, marker_size, parity, ordering);
        let mut views: Vec<CalibrationView> = Vec::new();

        for path in &images {
            let img: DynamicImage = image::open(path).expect("open image");
            let _ = graph.process(img);
            let Some(raw) = graph.sample_json_output("detections") else { continue };

            let parsed: serde_json::Value = match raw {
                serde_json::Value::String(s) => serde_json::from_str(&s).unwrap_or_else(|_| serde_json::json!({ "raw": s })),
                other => other,
            };
            let mut dets = parse_detections(parsed);
            if dets.is_empty() {
                continue;
            }

            dets.retain(|d| layout.tag_corners_by_id.contains_key(&d.id));
            let mut points = Vec::new();
            for det in dets {
                let Some(obj) = layout.tag_corners_by_id.get(&det.id).copied() else { continue };
                let ordered = order_marker_corners(det.rotation, det.corners);
                for k in 0..4 {
                    points.push(CalibrationPointPair::new(obj[k], ordered[k]));
                }
            }

            if points.len() >= cfg.min_points_per_view {
                views.push(CalibrationView::new(points));
            }
        }

        if views.len() < cfg.min_views {
            eprintln!("{ordering:?}/{parity:?}: usable_views={} (need {})", views.len(), cfg.min_views);
            continue;
        }

        let result = solve_camera_intrinsics_with_hint(&views, cfg, None).expect("solve");
        eprintln!(
            "{ordering:?}/{parity:?}: usable_views={} points_used={} reproj_px={:.3} fx={:.1} fy={:.1} cx={:.1} cy={:.1} k=[{:.4},{:.4},{:.4},{:.4}]",
            result.views_used,
            result.points_used,
            result.reprojection_error_px,
            result.calibration.intrinsics.fx,
            result.calibration.intrinsics.fy,
            result.calibration.intrinsics.cx,
            result.calibration.intrinsics.cy,
            result.calibration.distortion.k1,
            result.calibration.distortion.k2,
            result.calibration.distortion.k3,
            result.calibration.distortion.p1,
        );
    }

    eprintln!("elapsed_ms={}", start.elapsed().as_millis());
}
