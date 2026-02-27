use image::DynamicImage;
use imageproc::point::Point as CvPoint;
use serde::Serialize;

use lib_cv::modules::aruco::adaptive::AdaptiveDetectorConfig;
use lib_cv::modules::aruco::adaptive::adaptive_quads;
use lib_cv::modules::aruco::aruco_dictionary_from_name;
use lib_cv::modules::aruco::detect::{ArucoDecodeConfig, decode_quads_aruco_with_config, refine_detection_corners_warp_aruco};

#[derive(Serialize)]
struct OutDet {
    id: u32,
    rotation: u8,
    corners: [[f64; 2]; 4],
    best_distance: Option<u32>,
    second_distance: Option<u32>,
    border_mismatches: Option<u32>,
    contrast_range: Option<f32>,
    score: Option<f32>,
}

fn main() {
    let path = std::env::args_os().nth(1).expect("usage: aruco_detect_dump <image.jpg>");
    let img: DynamicImage = image::open(&path).expect("open image");

    let dict = aruco_dictionary_from_name("4x4_1000").expect("dict 4x4_1000");

    // Match the calibration template's adaptive quad detector defaults.
    let detector_cfg = AdaptiveDetectorConfig {
        adaptive_window: 15,
        adaptive_offset: 6.0,
        threshold_offset: 0.0,
        invert: true,
        open_k: 0,
        min_perimeter_rate: 0.06,
        max_perimeter_rate: 4.0,
        epsilon: 3.0,
        min_area: 40.0,
        max_area: None,
        min_angle: 15.0,
        max_angle: 165.0,
        max_side_cv: 2.5,
        min_corner_distance_rate: 0.05,
        min_distance_to_border: 3,
        min_side_px: 0.0,
        fallback_max_contours: 120,
        max_quads: 0,
    };
    let quads: Vec<[CvPoint<f32>; 4]> = adaptive_quads(&img, &detector_cfg);

    // Match the calibration template's ArUco decode tuning (stricter than general tagging).
    let decode_cfg = ArucoDecodeConfig {
        min_warped_patch_contrast_range: 20,
        min_cell_means_contrast_range: 25.0,
        min_quad_side_px: 8.0,
        min_quiet_zone_delta: 24.0,
        max_border_error_rate: 0.2,
        error_correction_rate: 0.0, // 4x4_1000 has no correction bits; keep strict.
        cell_sample_grid: 5,
        cell_sample_margin: 0.13,
        min_decode_score: 35.0,
        min_hamming_margin: 8,
        min_hamming_margin_min_dist: 1,
        min_hamming_margin_only_if_border_mismatch: false,
        min_bit_delta: 8.0,
    };

    let mut markers = decode_quads_aruco_with_config(&img, &quads, 8, &dict, &decode_cfg);
    let _ = refine_detection_corners_warp_aruco(&img, &mut markers, 8, &dict, &decode_cfg);

    // Calibration board IDs are contiguous starting at 0; keep the same range as the 9x13 A4 standard.
    let min_id = 0u32;
    let max_id = 58u32;
    markers.retain(|m| m.id >= min_id && m.id <= max_id);

    let out: Vec<OutDet> = markers
        .into_iter()
        .map(|m| {
            // Refinement updates corners; canonicalize again so rotation stays consistent.
            let det = m.canonicalize();
            let mut out_corners = [[0.0f64; 2]; 4];
            for (dst, src) in out_corners.iter_mut().zip(det.corners.iter()) {
                dst[0] = src.x;
                dst[1] = src.y;
            }
            OutDet {
                id: det.id,
                rotation: det.rotation,
                corners: out_corners,
                best_distance: det.best_distance,
                second_distance: det.second_distance,
                border_mismatches: det.border_mismatches.map(|v| v.min(u32::MAX as usize) as u32),
                contrast_range: det.contrast_range,
                score: det.score,
            }
        })
        .collect();

    println!("{}", serde_json::to_string_pretty(&out).unwrap());
}
