use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

use image::Rgba;

use lib_cv::aruco::ArucoTagFamilyKind;
use lib_cv::modules::aruco::detect::{ArucoTagDecodeConfig, decode_quads_warp_with_config, decode_quads_with_config, decode_quads_with_stats_config, refine_detection_corners_warp};
use lib_cv::modules::aruco::tag::ArucoTagDecodeTuning;
use lib_cv::modules::draw::aruco::overlay_marker;
use lib_cv::modules::draw::contour::overlay_contour_points;
use lib_cv::modules::image::binary::adaptive_mean_threshold_fast;
use lib_cv::modules::image::blur::blur_image;
use lib_cv::modules::image::resize::resize_fast;

mod contours;
mod decode;
mod env_util;
mod image_io;
mod mask;
mod quads;
mod types;

use contours::contours_from_mask;
use decode::{dump_decode_quad_errors, dump_decode_stats_json, dump_detections_json};
use env_util::*;
use image_io::{load_opencv_gray, save_gray, to_gray_opencv_bgr};
use mask::{adaptive_mean_threshold_reference, compare_reference_with_opencv, compare_two_masks, compare_with_opencv, invert_gray};
use quads::{candidate_quads, dump_quads_json, dump_stage_quads};
use types::{CandidateQuadsConfig, CompareSummary};

fn main() -> Result<(), Box<dyn Error>> {
    let input_path = env::args().nth(1).or_else(|| env::var("LIBCV_INPUT_IMAGE").ok()).unwrap_or_else(|| {
        eprintln!("missing input image path: pass as arg1 or set LIBCV_INPUT_IMAGE");
        std::process::exit(2);
    });
    let output_dir = env::var("LIBCV_COMPARE_DIR").unwrap_or_else(|_| "/tmp/libcv_compare_stage".to_string());
    let opencv_dir = env::var("OPENCV_COMPARE_DIR").unwrap_or_else(|_| "/tmp/opencv_compare".to_string());

    fs::create_dir_all(&output_dir)?;

    let input = image::open(&input_path)?;
    let downscale = env_u32("LIBCV_DOWNSCALE_FACTOR", 1).max(1);
    let blur_sigma = env_f32("LIBCV_BLUR_SIGMA", 0.0);

    let frame_for_mask = if downscale > 1 {
        let width = (input.width() / downscale).max(1);
        let height = (input.height() / downscale).max(1);
        resize_fast(&input, width, height)
    } else {
        input.clone()
    };
    let blurred = if blur_sigma > 0.0 { blur_image(frame_for_mask, blur_sigma) } else { frame_for_mask };

    let mut adaptive_window = env_u32("LIBCV_ADAPTIVE_WINDOW", 13);
    let sweep_min = env_u32_opt("LIBCV_ADAPTIVE_WINDOW_MIN");
    let sweep_max = env_u32_opt("LIBCV_ADAPTIVE_WINDOW_MAX");
    let sweep_step = env_u32("LIBCV_ADAPTIVE_WINDOW_STEP", 10).max(1);
    let sweep_windows = if sweep_min.is_some() || sweep_max.is_some() {
        let min = mask::ensure_odd_window(sweep_min.unwrap_or(adaptive_window).max(3));
        let max = mask::ensure_odd_window(sweep_max.unwrap_or(min).max(min));
        adaptive_window = min;
        let mut out = Vec::new();
        let mut window = min;
        while window <= max {
            out.push(window);
            let next = window.saturating_add(sweep_step);
            window = if next.is_multiple_of(2) { next + 1 } else { next };
        }
        out
    } else {
        vec![mask::ensure_odd_window(adaptive_window)]
    };
    let adaptive_offset = env_f32("LIBCV_ADAPTIVE_OFFSET", 7.0);
    let adaptive_threshold_offset = env_f32("LIBCV_ADAPTIVE_THRESHOLD_OFFSET", 0.0);
    let gray = blurred.to_luma8();
    let gray_opencv = to_gray_opencv_bgr(&blurred);
    let gray_from_opencv_file = load_opencv_gray(&opencv_dir);
    let use_opencv_gray = env_bool("LIBCV_USE_OPENCV_GRAY", false);
    let gray_source = if use_opencv_gray { gray_from_opencv_file.as_ref().unwrap_or(&gray) } else { &gray };
    let mask = adaptive_mean_threshold_fast(gray_source, adaptive_window, adaptive_offset + adaptive_threshold_offset);
    let mask_ref = adaptive_mean_threshold_reference(gray_source, adaptive_window, adaptive_offset + adaptive_threshold_offset);
    let mask_opencv_ref = adaptive_mean_threshold_reference(&gray_opencv, adaptive_window, adaptive_offset + adaptive_threshold_offset);
    let mask_from_opencv_gray = gray_from_opencv_file.as_ref().map(|g| adaptive_mean_threshold_reference(g, adaptive_window, adaptive_offset + adaptive_threshold_offset));
    let invert_mask = env_bool("LIBCV_INVERT_MASK", true);
    let mask_inverted = if invert_mask { invert_gray(&mask) } else { mask.clone() };
    let mask_ref_inverted = if invert_mask { invert_gray(&mask_ref) } else { mask_ref.clone() };
    let mask_opencv_ref_inverted = if invert_mask { invert_gray(&mask_opencv_ref) } else { mask_opencv_ref.clone() };
    let mask_from_opencv_gray_inverted = mask_from_opencv_gray.as_ref().map(|m| if invert_mask { invert_gray(m) } else { m.clone() });

    let mut contours = Vec::new();
    let mut contour_meta = Vec::new();
    if sweep_windows.len() == 1 {
        let (c, m) = contours_from_mask(&mask_inverted);
        contours = c;
        contour_meta = m;
    } else {
        for window in &sweep_windows {
            let sweep_mask = adaptive_mean_threshold_fast(gray_source, *window, adaptive_offset + adaptive_threshold_offset);
            let sweep_mask = if invert_mask { invert_gray(&sweep_mask) } else { sweep_mask };
            let (mut c, mut m) = contours_from_mask(&sweep_mask);
            let base_id = contour_meta.len();
            for meta in &mut m {
                meta.id += base_id;
                meta.parent = None;
            }
            contours.append(&mut c);
            contour_meta.append(&mut m);
        }
    }
    if env_bool("LIBCV_DUMP_CONTOURS", false) {
        let contour_path = PathBuf::from(&output_dir).join("libcv_contours.json");
        contours::dump_contours_json(&contours, &contour_path)?;
    }
    if env_bool("LIBCV_DUMP_CONTOUR_META", false) {
        let contour_path = PathBuf::from(&output_dir).join("libcv_contours_meta.json");
        contours::dump_contour_meta_json(&contour_meta, &contour_path)?;
    }
    if env_bool("LIBCV_DUMP_APPROX", false) {
        let contour_path = PathBuf::from(&output_dir).join("libcv_contours_f32.json");
        contours::dump_contours_json_f32(&contours, &contour_path)?;
    }
    let candidate_cfg = CandidateQuadsConfig {
        downscale,
        frame_width: mask_inverted.width(),
        frame_height: mask_inverted.height(),
        epsilon: env_f32("LIBCV_QUADS_EPSILON", 0.03),
        min_area: env_f32("LIBCV_QUADS_MIN_AREA", 0.0),
        max_area: env_f32("LIBCV_QUADS_MAX_AREA", 0.0),
        min_angle_deg: env_f32("LIBCV_QUADS_MIN_ANGLE", 0.0),
        max_angle_deg: env_f32("LIBCV_QUADS_MAX_ANGLE", 180.0),
        max_side_cv: env_f32("LIBCV_QUADS_MAX_SIDE_CV", 10.0),
        max_side_ratio: env_f32("LIBCV_QUADS_MAX_SIDE_RATIO", 0.0),
        max_diag_ratio: env_f32("LIBCV_QUADS_MAX_DIAG_RATIO", 0.0),
        mode_fast: env_str("LIBCV_QUADS_MODE", "robust") == "fast",
        refine_corners: env_bool("LIBCV_QUADS_REFINE_CORNERS", false),
        refine_edge_dist: env_f32("LIBCV_QUADS_REFINE_EDGE_DIST", 1.5),
        refine_min_points: env_usize("LIBCV_QUADS_REFINE_MIN_POINTS", 8),
        min_perimeter_rate: env_f32("LIBCV_QUADS_MIN_PERIMETER_RATE", 0.03),
        max_perimeter_rate: env_f32("LIBCV_QUADS_MAX_PERIMETER_RATE", 4.0),
        min_corner_distance_rate: env_f32("LIBCV_QUADS_MIN_CORNER_DISTANCE_RATE", 0.05),
        min_distance_to_border_px: env_f32("LIBCV_QUADS_MIN_DISTANCE_TO_BORDER", 3.0),
        min_marker_distance_rate: env_f32("LIBCV_QUADS_MIN_MARKER_DISTANCE_RATE", 0.125),
        simplify_contours: env_bool("LIBCV_CONTOUR_SIMPLE", false),
    };

    let (quads, filter_stats, quad_stages) = candidate_quads(&contours, &candidate_cfg);
    if env_bool("LIBCV_DUMP_QUADS", false) {
        let quad_path = PathBuf::from(&output_dir).join("libcv_quads.json");
        dump_quads_json(&quads, &quad_path)?;
    }
    if env_bool("LIBCV_DUMP_QUAD_STAGES", false) {
        let base = PathBuf::from(&output_dir);
        contours::dump_stage_contours(&input, &quad_stages.contours_after_perimeter, &base, "contours_after_perimeter")?;
        dump_stage_quads(&input, &quad_stages.quads_after_approx, &base, "quads_after_approx")?;
        dump_stage_quads(&input, &quad_stages.quads_after_convex, &base, "quads_after_convex")?;
        dump_stage_quads(&input, &quad_stages.quads_after_angle, &base, "quads_after_angle")?;
        dump_stage_quads(&input, &quad_stages.quads_after_area, &base, "quads_after_area")?;
        dump_stage_quads(&input, &quad_stages.quads_after_side_cv, &base, "quads_after_side_cv")?;
        dump_stage_quads(&input, &quad_stages.quads_after_side_ratio, &base, "quads_after_side_ratio")?;
        dump_stage_quads(&input, &quad_stages.quads_after_diag_ratio, &base, "quads_after_diag_ratio")?;
        dump_stage_quads(&input, &quad_stages.quads_after_candidate, &base, "quads_after_candidate")?;
        dump_stage_quads(&input, &quad_stages.quads_after_min_corner, &base, "quads_after_min_corner")?;
        dump_stage_quads(&input, &quad_stages.quads_after_min_border, &base, "quads_after_min_border")?;
        dump_stage_quads(&input, &quad_stages.quads_after_marker_distance, &base, "quads_after_marker_distance")?;
    }
    if env_bool("LIBCV_DUMP_APPROX", false) {
        let approx_path = PathBuf::from(&output_dir).join("libcv_approx.json");
        quads::dump_approx_json(&quad_stages.approx_polys, &approx_path)?;
    }

    let family_label = env_str("LIBCV_FAMILY", "16h5");
    let family_kind = ArucoTagFamilyKind::from_str(&family_label).unwrap_or(ArucoTagFamilyKind::Tag16H5);
    let sample_scale = env_u32("LIBCV_SAMPLE_SCALE", 4).max(1);
    let max_hamming = env_i32("LIBCV_MAX_HAMMING", 2);
    let border_error_divisor = env_i32("LIBCV_BORDER_ERROR_DIVISOR", 6);
    let mut family = family_kind.into_family();
    if max_hamming >= 0 {
        family = family.with_max_hamming(max_hamming as u8);
    }
    if border_error_divisor > 0 {
        family = family.with_border_error_divisor(border_error_divisor as u8);
    }

    let mut cell_decode = ArucoTagDecodeTuning::default();
    cell_decode.min_cell_means_contrast_range = env_f32("LIBCV_MIN_CELL_MEANS_CONTRAST_RANGE", cell_decode.min_cell_means_contrast_range);
    cell_decode.min_bit_delta = env_f32("LIBCV_MIN_BIT_DELTA", cell_decode.min_bit_delta);
    cell_decode.min_hamming_margin = env_u32("LIBCV_MIN_HAMMING_MARGIN", cell_decode.min_hamming_margin);
    cell_decode.min_hamming_margin_min_dist = env_u32("LIBCV_MIN_HAMMING_MARGIN_MIN_DIST", cell_decode.min_hamming_margin_min_dist);
    cell_decode.min_hamming_margin_only_if_border_mismatch = env_bool("LIBCV_MIN_HAMMING_MARGIN_ONLY_IF_BORDER_MISMATCH", cell_decode.min_hamming_margin_only_if_border_mismatch);
    let decode_cfg = ArucoTagDecodeConfig {
        min_warped_patch_contrast_range: env_u8("LIBCV_MIN_WARPED_PATCH_CONTRAST_RANGE", 8),
        min_quiet_zone_delta: env_f32("LIBCV_MIN_QUIET_ZONE_DELTA", 0.0),
        quiet_zone_texture_penalty: env_f32("LIBCV_QUIET_ZONE_TEXTURE_PENALTY", 12.0),
        warp_fallback_max_hamming_extra: env_u32("LIBCV_WARP_FALLBACK_MAX_HAMMING_EXTRA", 2),
        warp_fallback_border_slack: env_usize("LIBCV_WARP_FALLBACK_BORDER_SLACK", 6),
        warp_fallback_on_low_contrast: env_bool("LIBCV_WARP_FALLBACK_ON_LOW_CONTRAST", true),
        verify_warp_min_best_distance: env_u32("LIBCV_VERIFY_WARP_MIN_BEST_DISTANCE", 99),
        verify_warp_only_if_border_mismatch: env_bool("LIBCV_VERIFY_WARP_ONLY_IF_BORDER_MISMATCH", true),
        verify_warp_reject_on_fail: env_bool("LIBCV_VERIFY_WARP_REJECT_ON_FAIL", false),
        min_decode_score: env_f32("LIBCV_MIN_DECODE_SCORE", -1.0),
        cell_sample_grid: env_u8("LIBCV_CELL_SAMPLE_GRID", 3),
        cell_sample_margin: env_f32("LIBCV_CELL_SAMPLE_MARGIN", 0.13),
        cell_decode,
        ..Default::default()
    };
    let decode_stats_enabled = env_bool("LIBCV_DECODE_STATS", false);
    let force_warp_decode = env_bool("LIBCV_FORCE_WARP_DECODE", false);
    let (mut detections, decode_stats) = if force_warp_decode {
        (decode_quads_warp_with_config(&input, &quads, sample_scale, &family, &decode_cfg), None)
    } else if decode_stats_enabled {
        let (markers, stats) = decode_quads_with_stats_config(&input, &quads, sample_scale, &family, &decode_cfg);
        (markers, Some(stats))
    } else {
        (decode_quads_with_config(&input, &quads, sample_scale, &family, &decode_cfg), None)
    };

    let refine_warp = env_bool("LIBCV_REFINE_CORNERS_WARP", false);
    if refine_warp {
        let refine_scale = env_u32("LIBCV_REFINE_CORNERS_WARP_SCALE", sample_scale);
        refine_detection_corners_warp(&input, &mut detections, refine_scale.max(1), &family, &decode_cfg);
    }
    if env_bool("LIBCV_DUMP_DETECTIONS", false) {
        let det_path = PathBuf::from(&output_dir).join("libcv_detections.json");
        dump_detections_json(&detections, &det_path)?;
    }
    if env_bool("LIBCV_DUMP_DECODE_QUAD_ERRORS", false) {
        let errors_path = PathBuf::from(&output_dir).join("libcv_decode_quad_errors.json");
        dump_decode_quad_errors(&input, &quads, sample_scale, &family, &decode_cfg, &errors_path)?;
    }
    if let Some(stats) = decode_stats {
        let warp_only = if env_bool("LIBCV_DECODE_WARP_ONLY", false) {
            let warped = decode_quads_warp_with_config(&input, &quads, sample_scale, &family, &decode_cfg);
            Some(warped.len())
        } else {
            None
        };
        let stats_path = PathBuf::from(&output_dir).join("libcv_decode_stats.json");
        dump_decode_stats_json(&stats, warp_only, &stats_path)?;
    }

    let output_dir = PathBuf::from(output_dir);
    save_gray(&mask, &output_dir.join("libcv_mask.png"))?;
    save_gray(&mask_inverted, &output_dir.join("libcv_mask_inverted.png"))?;
    save_gray(&mask_ref, &output_dir.join("libcv_ref_mask.png"))?;
    save_gray(&mask_ref_inverted, &output_dir.join("libcv_ref_mask_inverted.png"))?;
    save_gray(&mask_opencv_ref, &output_dir.join("libcv_ref_mask_opencv_gray.png"))?;
    save_gray(&mask_opencv_ref_inverted, &output_dir.join("libcv_ref_mask_opencv_gray_inverted.png"))?;
    if let Some(mask_from_opencv_gray) = &mask_from_opencv_gray {
        save_gray(mask_from_opencv_gray, &output_dir.join("libcv_ref_mask_opencv_gray_input.png"))?;
    }
    if let Some(mask_from_opencv_gray_inverted) = &mask_from_opencv_gray_inverted {
        save_gray(mask_from_opencv_gray_inverted, &output_dir.join("libcv_ref_mask_opencv_gray_input_inverted.png"))?;
    }

    let mut quads_overlay = input.clone();
    for quad in &quads {
        overlay_contour_points(&mut quads_overlay, quad, 2, Rgba([0, 170, 255, 255]));
    }
    quads_overlay.save(output_dir.join("libcv_quads.png"))?;

    let mut detections_overlay = input.clone();
    for marker in &detections {
        overlay_marker(&mut detections_overlay, marker, Rgba([0, 255, 90, 255]));
    }
    detections_overlay.save(output_dir.join("libcv_detections.png"))?;

    let (mad_mask, mad_mask_inverted, diff_pixels_mask, diff_pixels_mask_inverted, diff_border_mask, diff_interior_mask, diff_border_mask_inverted, diff_interior_mask_inverted) =
        compare_with_opencv(&opencv_dir, &mask, &mask_inverted, adaptive_window)?;
    let (ref_mad_vs_libcv, ref_diff_pixels_vs_libcv) = compare_two_masks(&mask_ref, &mask);
    let (ref_mad_vs_opencv, ref_diff_pixels_vs_opencv) = compare_reference_with_opencv(&opencv_dir, &mask_ref, &mask_ref_inverted)?;
    let (opencv_gray_mad_vs_opencv, opencv_gray_diff_pixels_vs_opencv) = compare_reference_with_opencv(&opencv_dir, &mask_opencv_ref, &mask_opencv_ref_inverted)?;
    let (opencv_gray_input_mad_vs_opencv, opencv_gray_input_diff_pixels_vs_opencv) =
        if let (Some(mask_from_opencv_gray), Some(mask_from_opencv_gray_inverted)) = (mask_from_opencv_gray.as_ref(), mask_from_opencv_gray_inverted.as_ref()) {
            compare_reference_with_opencv(&opencv_dir, mask_from_opencv_gray, mask_from_opencv_gray_inverted)?
        } else {
            (None, None)
        };
    let summary = CompareSummary {
        candidates: quads.len(),
        detections: detections.len(),
        mad_mask,
        mad_mask_inverted,
        diff_pixels_mask,
        diff_pixels_mask_inverted,
        diff_border_mask,
        diff_interior_mask,
        diff_border_mask_inverted,
        diff_interior_mask_inverted,
        ref_mad_vs_libcv,
        ref_diff_pixels_vs_libcv,
        ref_mad_vs_opencv,
        ref_diff_pixels_vs_opencv,
        opencv_gray_mad_vs_opencv,
        opencv_gray_diff_pixels_vs_opencv,
        opencv_gray_input_mad_vs_opencv,
        opencv_gray_input_diff_pixels_vs_opencv,
        filters: filter_stats,
    };

    let summary_path = output_dir.join("summary.json");
    fs::write(&summary_path, serde_json::to_vec_pretty(&summary)?)?;

    println!("libcv candidates: {}", summary.candidates);
    println!("libcv detections: {}", summary.detections);
    if let Some(mad) = summary.mad_mask {
        println!("mask MAD vs OpenCV: {:.3}", mad);
    }
    if let Some(mad) = summary.mad_mask_inverted {
        println!("mask_inverted MAD vs OpenCV: {:.3}", mad);
    }
    if let Some(diff) = summary.diff_pixels_mask {
        println!("mask diff pixels vs OpenCV: {}", diff);
    }
    if let Some(diff) = summary.diff_pixels_mask_inverted {
        println!("mask_inverted diff pixels vs OpenCV: {}", diff);
    }
    if let (Some(border), Some(interior)) = (summary.diff_border_mask, summary.diff_interior_mask) {
        println!("mask diff border/interior: {} / {}", border, interior);
    }
    if let (Some(border), Some(interior)) = (summary.diff_border_mask_inverted, summary.diff_interior_mask_inverted) {
        println!("mask_inverted diff border/interior: {} / {}", border, interior);
    }
    if let Some(mad) = summary.ref_mad_vs_libcv {
        println!("ref MAD vs libcv: {:.3}", mad);
    }
    if let Some(diff) = summary.ref_diff_pixels_vs_libcv {
        println!("ref diff pixels vs libcv: {}", diff);
    }
    if let Some(mad) = summary.ref_mad_vs_opencv {
        println!("ref MAD vs opencv: {:.3}", mad);
    }
    if let Some(diff) = summary.ref_diff_pixels_vs_opencv {
        println!("ref diff pixels vs opencv: {}", diff);
    }
    if let Some(mad) = summary.opencv_gray_mad_vs_opencv {
        println!("opencv-gray MAD vs opencv: {:.3}", mad);
    }
    if let Some(diff) = summary.opencv_gray_diff_pixels_vs_opencv {
        println!("opencv-gray diff pixels vs opencv: {}", diff);
    }
    if let Some(mad) = summary.opencv_gray_input_mad_vs_opencv {
        println!("opencv-gray-input MAD vs opencv: {:.3}", mad);
    }
    if let Some(diff) = summary.opencv_gray_input_diff_pixels_vs_opencv {
        println!("opencv-gray-input diff pixels vs opencv: {}", diff);
    }
    println!("filter stats: {}", serde_json::to_string_pretty(&summary.filters)?);

    Ok(())
}
