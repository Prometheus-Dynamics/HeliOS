use super::*;

const DECODE_PAR_MIN_QUADS: usize = 512;

/// Decode a collection of quads using the supplied ArUco tag family.
pub fn decode_quads(frame: &DynamicImage, quads: &[[Point<f32>; 4]], sample_scale: u32, family: &ArucoTagFamily) -> Vec<ArucoDetection2D> {
    decode_quads_with_config(frame, quads, sample_scale, family, &ArucoTagDecodeConfig::default())
}

pub fn decode_quads_with_config(frame: &DynamicImage, quads: &[[Point<f32>; 4]], sample_scale: u32, family: &ArucoTagFamily, config: &ArucoTagDecodeConfig) -> Vec<ArucoDetection2D> {
    decode_quads_with_config_bits(frame, quads, sample_scale, family, config, true)
}

pub fn decode_quads_with_config_no_bits(frame: &DynamicImage, quads: &[[Point<f32>; 4]], sample_scale: u32, family: &ArucoTagFamily, config: &ArucoTagDecodeConfig) -> Vec<ArucoDetection2D> {
    decode_quads_with_config_bits(frame, quads, sample_scale, family, config, false)
}

pub fn decode_quads_gray(gray: &GrayImage, quads: &[[Point<f32>; 4]], sample_scale: u32, family: &ArucoTagFamily) -> Vec<ArucoDetection2D> {
    decode_quads_with_config_gray(gray, quads, sample_scale, family, &ArucoTagDecodeConfig::default())
}

pub fn decode_quads_with_config_gray(gray: &GrayImage, quads: &[[Point<f32>; 4]], sample_scale: u32, family: &ArucoTagFamily, config: &ArucoTagDecodeConfig) -> Vec<ArucoDetection2D> {
    decode_quads_with_config_bits_gray(gray, quads, sample_scale, family, config, true)
}

pub fn decode_quads_with_config_no_bits_gray(gray: &GrayImage, quads: &[[Point<f32>; 4]], sample_scale: u32, family: &ArucoTagFamily, config: &ArucoTagDecodeConfig) -> Vec<ArucoDetection2D> {
    decode_quads_with_config_bits_gray(gray, quads, sample_scale, family, config, false)
}

fn decode_quads_with_config_bits(
    frame: &DynamicImage,
    quads: &[[Point<f32>; 4]],
    sample_scale: u32,
    family: &ArucoTagFamily,
    config: &ArucoTagDecodeConfig,
    include_bits: bool,
) -> Vec<ArucoDetection2D> {
    if quads.is_empty() {
        return Vec::new();
    }
    with_luma8_frame(frame, |gray| decode_quads_with_config_bits_gray(gray, quads, sample_scale, family, config, include_bits))
}

fn decode_quads_with_config_bits_gray(
    gray: &GrayImage,
    quads: &[[Point<f32>; 4]],
    sample_scale: u32,
    family: &ArucoTagFamily,
    config: &ArucoTagDecodeConfig,
    include_bits: bool,
) -> Vec<ArucoDetection2D> {
    if quads.is_empty() {
        return Vec::new();
    }

    let start = std::time::Instant::now();
    // On CM-class devices, rayon setup/merge overhead is often larger than decode work for
    // small/medium candidate counts. Keep serial decode unless we have a large quad set.
    let detections: Vec<ArucoDetection2D> = if quads.len() < DECODE_PAR_MIN_QUADS {
        let mut out = Vec::with_capacity(quads.len().min(256));
        for quad in quads {
            let Some(marker) = decode_quad(gray, quad, sample_scale, family, config) else {
                continue;
            };
            let bits = if include_bits { family.bit_grid(marker.id as usize) } else { None };
            if let Some(detection) = marker_f32_to_detection_2d(marker, bits) {
                out.push(detection);
            }
        }
        out
    } else {
        quads
            .par_iter()
            .filter_map(|quad| {
                let marker = decode_quad(gray, quad, sample_scale, family, config)?;
                let bits = if include_bits { family.bit_grid(marker.id as usize) } else { None };
                marker_f32_to_detection_2d(marker, bits)
            })
            .collect()
    };
    if tracing::enabled!(Level::TRACE) {
        let elapsed = start.elapsed();
        trace!(
            target: "cv::aruco::decode",
            quads = quads.len(),
            decoded = detections.len(),
            ms = (elapsed.as_secs_f64() * 1000.0),
            us_per_quad = (elapsed.as_secs_f64() * 1_000_000.0) / (quads.len().max(1) as f64),
            "decode_quads"
        );
    }
    detections
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DecodeQuadsStats {
    pub input_quads: usize,
    pub output_markers: usize,

    pub decoded_sampled: usize,
    pub decoded_warp: usize,

    pub sampled_fail_too_small: usize,
    pub sampled_fail_projection: usize,
    pub sampled_fail_invalid_input: usize,
    pub sampled_fail_low_contrast: usize,
    pub sampled_fail_border_mismatch: usize,
    pub sampled_fail_quiet_zone: usize,
    pub sampled_fail_bit_delta_too_low: usize,
    pub sampled_fail_hamming_too_high: usize,
    pub sampled_fail_hamming_margin_too_low: usize,
    pub sampled_fail_low_score: usize,

    pub verify_warp_attempted: usize,
    pub verify_warp_passed: usize,
    pub verify_warp_reject_mismatch: usize,
    pub verify_warp_reject_failed: usize,

    pub warp_attempted: usize,
    pub warp_failed: usize,

    pub warp_fail_projection: usize,
    pub warp_fail_low_contrast: usize,
    pub warp_fail_grid: usize,
    pub warp_fail_family_decode: usize,
    pub warp_fail_low_score: usize,
}

/// Decode a collection of quads and return diagnostics about where candidates were rejected.
///
/// Intended for debug/telemetry (uses a serial loop to keep stats deterministic).
pub fn decode_quads_with_stats(frame: &DynamicImage, quads: &[[Point<f32>; 4]], sample_scale: u32, family: &ArucoTagFamily) -> (Vec<ArucoDetection2D>, DecodeQuadsStats) {
    decode_quads_with_stats_config(frame, quads, sample_scale, family, &ArucoTagDecodeConfig::default())
}

pub fn decode_quads_with_stats_config(
    frame: &DynamicImage,
    quads: &[[Point<f32>; 4]],
    sample_scale: u32,
    family: &ArucoTagFamily,
    config: &ArucoTagDecodeConfig,
) -> (Vec<ArucoDetection2D>, DecodeQuadsStats) {
    let mut stats = DecodeQuadsStats { input_quads: quads.len(), ..DecodeQuadsStats::default() };
    if quads.is_empty() {
        return (Vec::new(), stats);
    }
    with_luma8_frame(frame, |gray| {
        let mut out = Vec::with_capacity(quads.len().min(512));
        for quad in quads {
            if let Some(marker) = decode_quad_with_stats(gray, quad, sample_scale, family, config, &mut stats) {
                out.push(marker);
            }
        }
        stats.output_markers = out.len();
        let out2d: Vec<_> = out
            .into_iter()
            .filter_map(|m| {
                let bits = family.bit_grid(m.id as usize);
                marker_f32_to_detection_2d(m, bits)
            })
            .collect();
        (out2d, stats)
    })
}
