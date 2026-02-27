use std::error::Error;
use std::path::Path;

use image::DynamicImage;
use imageproc::point::Point as CvPoint;
use serde_json::{Value as JsonValue, json};

use lib_cv::modules::aruco::detect::{ArucoTagDecodeConfig, DecodeQuadOutcome, decode_quad_debug};

pub(crate) fn dump_detections_json(detections: &[lib_cv::modules::aruco::ArucoDetection2D], path: &Path) -> Result<(), Box<dyn Error>> {
    let mut out: Vec<JsonValue> = Vec::with_capacity(detections.len());
    for det in detections {
        let corners = det.corners.iter().map(|p| [p.x, p.y]).collect::<Vec<[f64; 2]>>();
        let obj = json!({
            "id": det.id,
            "rotation": det.rotation,
            "corners": corners,
            "score": det.score,
            "best_distance": det.best_distance,
            "second_distance": det.second_distance,
            "border_mismatches": det.border_mismatches,
            "contrast_range": det.contrast_range
        });
        out.push(obj);
    }
    let file = std::fs::File::create(path)?;
    serde_json::to_writer_pretty(file, &out)?;
    Ok(())
}

pub(crate) fn dump_decode_stats_json(stats: &lib_cv::modules::aruco::detect::DecodeQuadsStats, warp_only: Option<usize>, path: &Path) -> Result<(), Box<dyn Error>> {
    let obj = json!({
        "input_quads": stats.input_quads,
        "output_markers": stats.output_markers,
        "decoded_sampled": stats.decoded_sampled,
        "decoded_warp": stats.decoded_warp,
        "sampled_fail_too_small": stats.sampled_fail_too_small,
        "sampled_fail_projection": stats.sampled_fail_projection,
        "sampled_fail_invalid_input": stats.sampled_fail_invalid_input,
        "sampled_fail_low_contrast": stats.sampled_fail_low_contrast,
        "sampled_fail_border_mismatch": stats.sampled_fail_border_mismatch,
        "sampled_fail_quiet_zone": stats.sampled_fail_quiet_zone,
        "sampled_fail_bit_delta_too_low": stats.sampled_fail_bit_delta_too_low,
        "sampled_fail_hamming_too_high": stats.sampled_fail_hamming_too_high,
        "sampled_fail_hamming_margin_too_low": stats.sampled_fail_hamming_margin_too_low,
        "sampled_fail_low_score": stats.sampled_fail_low_score,
        "verify_warp_attempted": stats.verify_warp_attempted,
        "verify_warp_passed": stats.verify_warp_passed,
        "verify_warp_reject_mismatch": stats.verify_warp_reject_mismatch,
        "verify_warp_reject_failed": stats.verify_warp_reject_failed,
        "warp_attempted": stats.warp_attempted,
        "warp_failed": stats.warp_failed,
        "warp_fail_projection": stats.warp_fail_projection,
        "warp_fail_low_contrast": stats.warp_fail_low_contrast,
        "warp_fail_grid": stats.warp_fail_grid,
        "warp_fail_family_decode": stats.warp_fail_family_decode,
        "warp_fail_low_score": stats.warp_fail_low_score,
        "warp_only_decoded": warp_only,
    });
    let file = std::fs::File::create(path)?;
    serde_json::to_writer_pretty(file, &obj)?;
    Ok(())
}

pub(crate) fn decode_outcome_label(outcome: DecodeQuadOutcome) -> &'static str {
    match outcome {
        DecodeQuadOutcome::DecodedSampled => "decoded_sampled",
        DecodeQuadOutcome::DecodedWarp => "decoded_warp",
        DecodeQuadOutcome::Failed(failure) => match failure {
            lib_cv::modules::aruco::detect::DecodeQuadFailure::SampledProjection => "sampled_projection",
            lib_cv::modules::aruco::detect::DecodeQuadFailure::SampledInvalidInput => "sampled_invalid_input",
            lib_cv::modules::aruco::detect::DecodeQuadFailure::SampledLowContrast => "sampled_low_contrast",
            lib_cv::modules::aruco::detect::DecodeQuadFailure::SampledBorderMismatch => "sampled_border_mismatch",
            lib_cv::modules::aruco::detect::DecodeQuadFailure::SampledQuietZoneDeltaTooLow => "sampled_quiet_zone_delta_too_low",
            lib_cv::modules::aruco::detect::DecodeQuadFailure::SampledBitDeltaTooLow => "sampled_bit_delta_too_low",
            lib_cv::modules::aruco::detect::DecodeQuadFailure::SampledHammingMarginTooLow => "sampled_hamming_margin_too_low",
            lib_cv::modules::aruco::detect::DecodeQuadFailure::SampledHammingTooHigh => "sampled_hamming_too_high",
            lib_cv::modules::aruco::detect::DecodeQuadFailure::SampledDecodeScoreTooLow => "sampled_decode_score_too_low",
            lib_cv::modules::aruco::detect::DecodeQuadFailure::SampledVerifyWarpMismatch => "sampled_verify_warp_mismatch",
            lib_cv::modules::aruco::detect::DecodeQuadFailure::SampledVerifyWarpFailed => "sampled_verify_warp_failed",
            lib_cv::modules::aruco::detect::DecodeQuadFailure::WarpProjection => "warp_projection",
            lib_cv::modules::aruco::detect::DecodeQuadFailure::WarpLowContrast => "warp_low_contrast",
            lib_cv::modules::aruco::detect::DecodeQuadFailure::WarpGrid => "warp_grid",
            lib_cv::modules::aruco::detect::DecodeQuadFailure::WarpFamilyDecode => "warp_family_decode",
            lib_cv::modules::aruco::detect::DecodeQuadFailure::WarpLowScore => "warp_low_score",
        },
    }
}

pub(crate) fn dump_decode_quad_errors(
    frame: &DynamicImage,
    quads: &[[CvPoint<f32>; 4]],
    sample_scale: u32,
    family: &lib_cv::modules::aruco::tag::ArucoTagFamily,
    config: &ArucoTagDecodeConfig,
    path: &Path,
) -> Result<(), Box<dyn Error>> {
    let mut out = Vec::with_capacity(quads.len());
    for (idx, quad) in quads.iter().enumerate() {
        let outcome = decode_quad_debug(frame, quad, sample_scale, family, config);
        let corners: Vec<[f32; 2]> = quad.iter().map(|p| [p.x, p.y]).collect();
        out.push(json!({
            "index": idx,
            "corners": corners,
            "outcome": decode_outcome_label(outcome),
        }));
    }
    let file = std::fs::File::create(path)?;
    serde_json::to_writer_pretty(file, &out)?;
    Ok(())
}
