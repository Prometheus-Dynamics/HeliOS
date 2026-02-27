use super::*;

/// Decode quads using the legacy warp-then-decode path.
///
/// This is useful as a quality baseline, but it is significantly slower than the sampled decode
/// path (which avoids constructing a full warped patch and integral image).
pub fn decode_quads_warp(frame: &DynamicImage, quads: &[[Point<f32>; 4]], sample_scale: u32, family: &ArucoTagFamily) -> Vec<ArucoDetection2D> {
    decode_quads_warp_with_config(frame, quads, sample_scale, family, &ArucoTagDecodeConfig::default())
}

pub fn decode_quads_warp_with_config(frame: &DynamicImage, quads: &[[Point<f32>; 4]], sample_scale: u32, family: &ArucoTagFamily, config: &ArucoTagDecodeConfig) -> Vec<ArucoDetection2D> {
    if quads.is_empty() {
        return Vec::new();
    }
    with_luma8_frame(frame, |gray| {
        let decoded: Vec<ArucoDetectionF32> = if quads.len() < 32 {
            quads.iter().filter_map(|quad| decode_quad_warp(gray, quad, sample_scale, family, config)).collect()
        } else {
            quads.par_iter().filter_map(|quad| decode_quad_warp(gray, quad, sample_scale, family, config)).collect()
        };
        decoded
            .into_iter()
            .filter_map(|m| {
                let bits = family.bit_grid(m.id as usize);
                marker_f32_to_detection_2d(m, bits)
            })
            .collect()
    })
}
