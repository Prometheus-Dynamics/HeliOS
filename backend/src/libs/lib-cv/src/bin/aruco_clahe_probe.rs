use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use image::{DynamicImage, GrayImage};

use lib_cv::aruco::ArucoTagFamilyKind;
use lib_cv::modules::aruco::adaptive::{AdaptiveDetectorConfig, adaptive_quads_from_mask};
use lib_cv::modules::aruco::detect::{ArucoTagDecodeConfig, DecodeQuadOutcome, decode_quad_debug, decode_quads_with_config_no_bits};
use lib_cv::modules::image::binary::adaptive_mean_threshold_fast_with_invert;
use lib_cv::modules::image::clahe::apply_clahe;

#[derive(Clone, Debug)]
struct ProbeConfig {
    tile_size: u32,
    clip_limit: f32,
    gamma: f32,
    adaptive_window: u32,
    adaptive_offset: f32,
    threshold_offset: f32,
    invert: bool,
    min_perimeter_rate: f32,
    max_perimeter_rate: f32,
    epsilon: f32,
    min_area: f32,
    max_area: Option<f32>,
    min_angle_deg: f32,
    max_angle_deg: f32,
    max_side_cv: f32,
    fallback_max_contours: usize,
    sample_scale: u32,
    max_hamming: i32,
    border_error_divisor: i32,
    min_warped_patch_contrast_range: u8,
    warp_fallback_max_hamming_extra: u32,
    warp_fallback_border_slack: usize,
    warp_min_sample_scale: u32,
    min_quad_side_px: f32,
    min_quiet_zone_delta: f32,
    quiet_zone_texture_penalty: f32,
    verify_warp_min_best_distance: u32,
    verify_warp_only_if_border_mismatch: bool,
    verify_warp_reject_on_fail: bool,
    min_decode_score: f32,
    cell_sample_grid: u8,
    cell_sample_margin: f32,
    min_cell_means_contrast_range: f32,
    min_hamming_margin: u32,
    min_hamming_margin_min_dist: u32,
    min_hamming_margin_only_if_border_mismatch: bool,
    min_bit_delta: f32,
}

type DetectionQuality = (u32, Option<u32>, Option<usize>, Option<f32>);

impl Default for ProbeConfig {
    fn default() -> Self {
        Self {
            tile_size: env_u32("PROBE_TILE_SIZE", 16),
            clip_limit: env_f32("PROBE_CLIP_LIMIT", 2.5),
            gamma: env_f32("PROBE_GAMMA", 0.9),
            adaptive_window: env_u32("PROBE_ADAPTIVE_WINDOW", 11),
            adaptive_offset: env_f32("PROBE_ADAPTIVE_OFFSET", 6.0),
            threshold_offset: env_f32("PROBE_THRESHOLD_OFFSET", 0.0),
            invert: env_bool("PROBE_INVERT", true),
            min_perimeter_rate: env_f32("PROBE_MIN_PERIMETER_RATE", 0.035),
            max_perimeter_rate: env_f32("PROBE_MAX_PERIMETER_RATE", 4.5),
            epsilon: env_f32("PROBE_EPSILON", 7.0),
            min_area: env_f32("PROBE_MIN_AREA", 160.0),
            max_area: env_f32_opt("PROBE_MAX_AREA").or(Some(0.0)).filter(|v| *v > 0.0),
            min_angle_deg: env_f32("PROBE_MIN_ANGLE_DEG", 0.0),
            max_angle_deg: env_f32("PROBE_MAX_ANGLE_DEG", 180.0),
            max_side_cv: env_f32("PROBE_MAX_SIDE_CV", 4.5),
            fallback_max_contours: env_usize("PROBE_FALLBACK_MAX_CONTOURS", 32),
            sample_scale: env_u32("PROBE_SAMPLE_SCALE", 2).max(1),
            max_hamming: env_i32("PROBE_MAX_HAMMING", 4),
            border_error_divisor: env_i32("PROBE_BORDER_ERROR_DIVISOR", 6),
            min_warped_patch_contrast_range: env_u8("PROBE_MIN_WARPED_PATCH_CONTRAST_RANGE", 6),
            warp_fallback_max_hamming_extra: env_u32("PROBE_WARP_FALLBACK_MAX_HAMMING_EXTRA", 7),
            warp_fallback_border_slack: env_usize("PROBE_WARP_FALLBACK_BORDER_SLACK", 14),
            warp_min_sample_scale: env_u32("PROBE_WARP_MIN_SAMPLE_SCALE", 4).max(1),
            min_quad_side_px: env_f32("PROBE_MIN_QUAD_SIDE_PX", 0.0),
            min_quiet_zone_delta: env_f32("PROBE_MIN_QUIET_ZONE_DELTA", 0.0),
            quiet_zone_texture_penalty: env_f32("PROBE_QUIET_ZONE_TEXTURE_PENALTY", 4.0),
            verify_warp_min_best_distance: env_u32("PROBE_VERIFY_WARP_MIN_BEST_DISTANCE", 99),
            verify_warp_only_if_border_mismatch: env_bool("PROBE_VERIFY_WARP_ONLY_IF_BORDER_MISMATCH", true),
            verify_warp_reject_on_fail: env_bool("PROBE_VERIFY_WARP_REJECT_ON_FAIL", true),
            min_decode_score: env_f32("PROBE_MIN_DECODE_SCORE", -1.0),
            cell_sample_grid: env_u8("PROBE_CELL_SAMPLE_GRID", 0),
            cell_sample_margin: env_f32("PROBE_CELL_SAMPLE_MARGIN", 0.25),
            min_cell_means_contrast_range: env_f32("PROBE_MIN_CELL_MEANS_CONTRAST_RANGE", 4.0),
            min_hamming_margin: env_u32("PROBE_MIN_HAMMING_MARGIN", 0),
            min_hamming_margin_min_dist: env_u32("PROBE_MIN_HAMMING_MARGIN_MIN_DIST", 0),
            min_hamming_margin_only_if_border_mismatch: env_bool("PROBE_MIN_HAMMING_MARGIN_ONLY_IF_BORDER_MISMATCH", false),
            min_bit_delta: env_f32("PROBE_MIN_BIT_DELTA", 0.0),
        }
    }
}

fn env_str(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.trim().is_empty())
}

fn env_bool(key: &str, default: bool) -> bool {
    match env_str(key).as_deref() {
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES") | Some("on") | Some("ON") => true,
        Some("0") | Some("false") | Some("FALSE") | Some("no") | Some("NO") | Some("off") | Some("OFF") => false,
        _ => default,
    }
}

fn env_f32(key: &str, default: f32) -> f32 {
    env_str(key).and_then(|v| v.parse::<f32>().ok()).unwrap_or(default)
}

fn env_f32_opt(key: &str) -> Option<f32> {
    env_str(key).and_then(|v| v.parse::<f32>().ok())
}

fn env_u32(key: &str, default: u32) -> u32 {
    env_str(key).and_then(|v| v.parse::<u32>().ok()).unwrap_or(default)
}

fn env_u8(key: &str, default: u8) -> u8 {
    env_str(key).and_then(|v| v.parse::<u8>().ok()).unwrap_or(default)
}

fn env_usize(key: &str, default: usize) -> usize {
    env_str(key).and_then(|v| v.parse::<usize>().ok()).unwrap_or(default)
}

fn env_i32(key: &str, default: i32) -> i32 {
    env_str(key).and_then(|v| v.parse::<i32>().ok()).unwrap_or(default)
}

fn build_decode_config(cfg: &ProbeConfig) -> ArucoTagDecodeConfig {
    ArucoTagDecodeConfig {
        min_warped_patch_contrast_range: cfg.min_warped_patch_contrast_range,
        warp_fallback_on_decode_fail: true,
        warp_fallback_max_hamming_extra: cfg.warp_fallback_max_hamming_extra,
        warp_fallback_border_slack: cfg.warp_fallback_border_slack,
        warp_fallback_on_low_contrast: true,
        warp_min_sample_scale: cfg.warp_min_sample_scale,
        min_quad_side_px: cfg.min_quad_side_px,
        min_quiet_zone_delta: cfg.min_quiet_zone_delta,
        quiet_zone_texture_penalty: cfg.quiet_zone_texture_penalty,
        verify_warp_min_best_distance: cfg.verify_warp_min_best_distance,
        verify_warp_only_if_border_mismatch: cfg.verify_warp_only_if_border_mismatch,
        verify_warp_reject_on_fail: cfg.verify_warp_reject_on_fail,
        min_decode_score: cfg.min_decode_score,
        cell_sample_grid: cfg.cell_sample_grid,
        cell_sample_margin: cfg.cell_sample_margin,
        cell_decode: lib_cv::modules::aruco::tag::ArucoTagDecodeTuning {
            min_cell_means_contrast_range: cfg.min_cell_means_contrast_range,
            min_hamming_margin: cfg.min_hamming_margin,
            min_hamming_margin_min_dist: cfg.min_hamming_margin_min_dist,
            min_hamming_margin_only_if_border_mismatch: cfg.min_hamming_margin_only_if_border_mismatch,
            min_bit_delta: cfg.min_bit_delta,
        },
    }
}

fn apply_gamma_in_place(gray: &mut GrayImage, gamma: f32) {
    if (gamma - 1.0).abs() <= f32::EPSILON {
        return;
    }
    let gamma = gamma.clamp(0.01, 10.0);
    let mut lut = [0u8; 256];
    for (i, dst) in lut.iter_mut().enumerate() {
        let x = (i as f32) / 255.0;
        *dst = (x.powf(gamma) * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    for px in gray.as_mut().iter_mut() {
        *px = lut[*px as usize];
    }
}

fn preprocess_mask(frame: &DynamicImage, cfg: &ProbeConfig) -> (GrayImage, DynamicImage) {
    let gray = frame.to_luma8();
    let mut enhanced = apply_clahe(&gray, cfg.tile_size.max(1), cfg.clip_limit.max(0.0));
    apply_gamma_in_place(&mut enhanced, cfg.gamma);
    let mask = adaptive_mean_threshold_fast_with_invert(&enhanced, cfg.adaptive_window.max(3), cfg.adaptive_offset + cfg.threshold_offset, cfg.invert);
    (mask, DynamicImage::ImageLuma8(enhanced))
}

fn load_inputs(path: &Path) -> Result<Vec<PathBuf>, String> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    if !path.is_dir() {
        return Err(format!("input path not found: {}", path.display()));
    }
    let mut files: Vec<PathBuf> = fs::read_dir(path)
        .map_err(|e| format!("read_dir {} failed: {e}", path.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| {
            matches!(
                p.extension().and_then(OsStr::to_str).map(|s| s.to_ascii_lowercase()),
                Some(ref ext) if ext == "jpg" || ext == "jpeg" || ext == "png" || ext == "bmp"
            )
        })
        .collect();
    files.sort();
    Ok(files)
}

fn outcome_label(outcome: DecodeQuadOutcome) -> &'static str {
    match outcome {
        DecodeQuadOutcome::DecodedSampled => "decoded_sampled",
        DecodeQuadOutcome::DecodedWarp => "decoded_warp",
        DecodeQuadOutcome::Failed(lib_cv::modules::aruco::detect::DecodeQuadFailure::SampledProjection) => "sampled_projection",
        DecodeQuadOutcome::Failed(lib_cv::modules::aruco::detect::DecodeQuadFailure::SampledInvalidInput) => "sampled_invalid_input",
        DecodeQuadOutcome::Failed(lib_cv::modules::aruco::detect::DecodeQuadFailure::SampledLowContrast) => "sampled_low_contrast",
        DecodeQuadOutcome::Failed(lib_cv::modules::aruco::detect::DecodeQuadFailure::SampledBorderMismatch) => "sampled_border_mismatch",
        DecodeQuadOutcome::Failed(lib_cv::modules::aruco::detect::DecodeQuadFailure::SampledQuietZoneDeltaTooLow) => "sampled_quiet_zone_delta_too_low",
        DecodeQuadOutcome::Failed(lib_cv::modules::aruco::detect::DecodeQuadFailure::SampledBitDeltaTooLow) => "sampled_bit_delta_too_low",
        DecodeQuadOutcome::Failed(lib_cv::modules::aruco::detect::DecodeQuadFailure::SampledHammingMarginTooLow) => "sampled_hamming_margin_too_low",
        DecodeQuadOutcome::Failed(lib_cv::modules::aruco::detect::DecodeQuadFailure::SampledHammingTooHigh) => "sampled_hamming_too_high",
        DecodeQuadOutcome::Failed(lib_cv::modules::aruco::detect::DecodeQuadFailure::SampledDecodeScoreTooLow) => "sampled_decode_score_too_low",
        DecodeQuadOutcome::Failed(lib_cv::modules::aruco::detect::DecodeQuadFailure::SampledVerifyWarpMismatch) => "sampled_verify_warp_mismatch",
        DecodeQuadOutcome::Failed(lib_cv::modules::aruco::detect::DecodeQuadFailure::SampledVerifyWarpFailed) => "sampled_verify_warp_failed",
        DecodeQuadOutcome::Failed(lib_cv::modules::aruco::detect::DecodeQuadFailure::WarpProjection) => "warp_projection",
        DecodeQuadOutcome::Failed(lib_cv::modules::aruco::detect::DecodeQuadFailure::WarpLowContrast) => "warp_low_contrast",
        DecodeQuadOutcome::Failed(lib_cv::modules::aruco::detect::DecodeQuadFailure::WarpGrid) => "warp_grid",
        DecodeQuadOutcome::Failed(lib_cv::modules::aruco::detect::DecodeQuadFailure::WarpFamilyDecode) => "warp_family_decode",
        DecodeQuadOutcome::Failed(lib_cv::modules::aruco::detect::DecodeQuadFailure::WarpLowScore) => "warp_low_score",
    }
}

fn node_like_filter(detections: &[lib_cv::modules::aruco::ArucoDetection2D], width: u32, height: u32) -> Vec<lib_cv::modules::aruco::ArucoDetection2D> {
    #[inline(always)]
    fn orient(a: &lib_cv::Point, b: &lib_cv::Point, c: &lib_cv::Point) -> f64 {
        (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
    }

    #[inline(always)]
    fn proper_segment_intersection(a: &lib_cv::Point, b: &lib_cv::Point, c: &lib_cv::Point, d: &lib_cv::Point) -> bool {
        let o1 = orient(a, b, c);
        let o2 = orient(a, b, d);
        let o3 = orient(c, d, a);
        let o4 = orient(c, d, b);
        (o1 * o2) < 0.0 && (o3 * o4) < 0.0
    }

    #[inline(always)]
    fn quad_has_crossed_edges(c: &[lib_cv::Point; 4]) -> bool {
        proper_segment_intersection(&c[0], &c[1], &c[2], &c[3]) || proper_segment_intersection(&c[1], &c[2], &c[3], &c[0])
    }

    #[inline(always)]
    fn quad_is_strictly_convex(c: &[lib_cv::Point; 4]) -> bool {
        let mut sign = 0.0f64;
        for i in 0..4usize {
            let a = &c[i];
            let b = &c[(i + 1) & 3];
            let d = &c[(i + 2) & 3];
            let cross = orient(a, b, d);
            if cross.abs() <= f64::EPSILON {
                return false;
            }
            if sign == 0.0 {
                sign = cross.signum();
            } else if sign * cross < 0.0 {
                return false;
            }
        }
        true
    }

    let w = width.max(1) as f64;
    let h = height.max(1) as f64;
    let margin = 4.0f64;
    detections
        .iter()
        .filter(|det| {
            if let Some(border_mismatches) = det.border_mismatches {
                let best_distance = det.best_distance.unwrap_or(0);
                if border_mismatches >= 3 {
                    return false;
                }
                if border_mismatches == 2 && best_distance >= 2 {
                    return false;
                }
                if border_mismatches == 1 && best_distance >= 3 {
                    return false;
                }
            }

            let mut area2 = 0.0f64;
            let mut min_edge_sq = f64::MAX;
            for i in 0..4usize {
                let a = det.corners[i];
                let b = det.corners[(i + 1) % 4];
                if !a.x.is_finite() || !a.y.is_finite() {
                    return false;
                }
                if a.x < -margin || a.y < -margin || a.x > (w - 1.0 + margin) || a.y > (h - 1.0 + margin) {
                    return false;
                }
                let dx = b.x - a.x;
                let dy = b.y - a.y;
                min_edge_sq = min_edge_sq.min(dx * dx + dy * dy);
                area2 += a.x * b.y - b.x * a.y;
            }
            if min_edge_sq < 4.0 {
                return false;
            }
            if area2.abs() < 4.0 {
                return false;
            }
            if quad_has_crossed_edges(&det.corners) {
                return false;
            }
            quad_is_strictly_convex(&det.corners)
        })
        .cloned()
        .collect()
}

fn main() -> Result<(), String> {
    let input = std::env::args().nth(1).unwrap_or_else(|| "/tmp/stream3f_imgs".to_string());
    let cfg = ProbeConfig::default();
    let files = load_inputs(Path::new(&input))?;
    if files.is_empty() {
        return Err(format!("no input images in {}", input));
    }

    let mut family = ArucoTagFamilyKind::Tag36H11.into_family();
    if cfg.max_hamming >= 0 {
        family = family.with_max_hamming(cfg.max_hamming as u8);
    }
    if cfg.border_error_divisor > 0 {
        family = family.with_border_error_divisor(cfg.border_error_divisor as u8);
    }
    let decode_cfg = build_decode_config(&cfg);

    println!(
        "config offset={} epsilon={} min_area={} fallback={} sample_scale={} max_hamming={} border_div={} decode_min_warp_contrast={}",
        cfg.adaptive_offset, cfg.epsilon, cfg.min_area, cfg.fallback_max_contours, cfg.sample_scale, cfg.max_hamming, cfg.border_error_divisor, cfg.min_warped_patch_contrast_range
    );

    let mut total_quads = 0usize;
    let mut total_det_orig = 0usize;
    let mut total_det_enh = 0usize;
    let mut outcome_hist: BTreeMap<&'static str, usize> = BTreeMap::new();

    let image_count = files.len();
    for path in files {
        let frame = image::open(&path).map_err(|e| format!("open {} failed: {e}", path.display()))?;
        let (mask, enhanced_frame) = preprocess_mask(&frame, &cfg);
        let aq_cfg = AdaptiveDetectorConfig {
            adaptive_window: cfg.adaptive_window,
            adaptive_offset: cfg.adaptive_offset,
            threshold_offset: cfg.threshold_offset,
            invert: cfg.invert,
            open_k: 0,
            min_perimeter_rate: cfg.min_perimeter_rate,
            max_perimeter_rate: cfg.max_perimeter_rate,
            epsilon: cfg.epsilon,
            min_area: cfg.min_area,
            max_area: cfg.max_area,
            min_angle: cfg.min_angle_deg,
            max_angle: cfg.max_angle_deg,
            max_side_cv: cfg.max_side_cv,
            min_corner_distance_rate: 0.05,
            min_distance_to_border: 3,
            min_side_px: 0.0,
            fallback_max_contours: cfg.fallback_max_contours,
            max_quads: 0,
        };
        let quads = adaptive_quads_from_mask(&mask, &aq_cfg);

        for q in &quads {
            let outcome = decode_quad_debug(&frame, q, cfg.sample_scale, &family, &decode_cfg);
            let key = outcome_label(outcome);
            *outcome_hist.entry(key).or_insert(0) += 1;
        }

        let detections_orig = decode_quads_with_config_no_bits(&frame, &quads, cfg.sample_scale, &family, &decode_cfg);
        let detections_enh = decode_quads_with_config_no_bits(&enhanced_frame, &quads, cfg.sample_scale, &family, &decode_cfg);
        let mut detections_orig_canon: Vec<_> = detections_orig.iter().cloned().map(|d| d.canonicalize()).collect();
        let mut detections_enh_canon: Vec<_> = detections_enh.iter().cloned().map(|d| d.canonicalize()).collect();
        detections_orig_canon = node_like_filter(&detections_orig_canon, frame.width(), frame.height());
        detections_enh_canon = node_like_filter(&detections_enh_canon, frame.width(), frame.height());
        let mut ids_orig: Vec<u32> = detections_orig.iter().map(|d| d.id).collect();
        let mut ids_enh: Vec<u32> = detections_enh.iter().map(|d| d.id).collect();
        let quality_orig: Vec<DetectionQuality> = detections_orig.iter().map(|d| (d.id, d.best_distance, d.border_mismatches, d.score)).collect();
        let quality_enh: Vec<DetectionQuality> = detections_enh.iter().map(|d| (d.id, d.best_distance, d.border_mismatches, d.score)).collect();
        let ids_node_like_orig: Vec<u32> = detections_orig_canon.iter().map(|d| d.id).collect();
        let ids_node_like_enh: Vec<u32> = detections_enh_canon.iter().map(|d| d.id).collect();
        ids_orig.sort_unstable();
        ids_enh.sort_unstable();

        total_quads += quads.len();
        total_det_orig += detections_orig.len();
        total_det_enh += detections_enh.len();

        println!(
            "{} quads={} det_orig={} ids_orig={:?} quality_orig={:?} node_like_orig={} node_ids_orig={:?} det_enh={} ids_enh={:?} quality_enh={:?} node_like_enh={} node_ids_enh={:?}",
            path.file_name().and_then(OsStr::to_str).unwrap_or("<unknown>"),
            quads.len(),
            detections_orig.len(),
            ids_orig,
            quality_orig,
            detections_orig_canon.len(),
            ids_node_like_orig,
            detections_enh.len(),
            ids_enh,
            quality_enh,
            detections_enh_canon.len(),
            ids_node_like_enh
        );
    }

    println!("summary images={} total_quads={} total_det_orig={} total_det_enh={}", image_count, total_quads, total_det_orig, total_det_enh);
    println!("outcome_hist {:?}", outcome_hist);
    Ok(())
}
