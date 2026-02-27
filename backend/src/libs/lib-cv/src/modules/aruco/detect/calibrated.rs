use super::*;

pub fn decode_quads_calibrated(frame: &DynamicImage, quads: &[[Point<f32>; 4]], sample_scale: u32, family: &ArucoTagFamily, calib: CameraCalibration) -> Vec<ArucoDetection2D> {
    decode_quads_calibrated_with_config(frame, quads, sample_scale, family, calib, &ArucoTagDecodeConfig::default())
}

pub fn decode_quads_calibrated_with_config(
    frame: &DynamicImage,
    quads: &[[Point<f32>; 4]],
    sample_scale: u32,
    family: &ArucoTagFamily,
    calib: CameraCalibration,
    config: &ArucoTagDecodeConfig,
) -> Vec<ArucoDetection2D> {
    if quads.is_empty() {
        return Vec::new();
    }
    with_luma8_frame(frame, |gray| {
        let start = std::time::Instant::now();
        // Rayon overhead can dominate when we only have a handful of candidates.
        let decoded: Vec<ArucoDetectionF32> = if quads.len() < 32 {
            quads.iter().filter_map(|quad| decode_quad_calibrated(gray, quad, sample_scale, family, calib, config)).collect()
        } else {
            quads.par_iter().filter_map(|quad| decode_quad_calibrated(gray, quad, sample_scale, family, calib, config)).collect()
        };
        if tracing::enabled!(Level::TRACE) {
            let elapsed = start.elapsed();
            trace!(
                target: "cv::aruco::decode",
                quads = quads.len(),
                decoded = decoded.len(),
                ms = (elapsed.as_secs_f64() * 1000.0),
                us_per_quad = (elapsed.as_secs_f64() * 1_000_000.0) / (quads.len().max(1) as f64),
                "decode_quads_calibrated"
            );
        }
        decoded
            .into_iter()
            .filter_map(|m| {
                let bits = family.bit_grid(m.id as usize);
                marker_f32_to_detection_2d(m, bits)
            })
            .collect()
    })
}
fn decode_quad_calibrated(
    gray: &GrayImage,
    corners: &[Point<f32>; 4],
    sample_scale: u32,
    family: &ArucoTagFamily,
    calib: CameraCalibration,
    config: &ArucoTagDecodeConfig,
) -> Option<ArucoDetectionF32> {
    if calib.fx <= f32::EPSILON || calib.fy <= f32::EPSILON {
        return decode_quad(gray, corners, sample_scale, family, config);
    }

    let mut undistorted = [Point::new(0.0, 0.0); 4];
    for (dst, corner) in undistorted.iter_mut().zip(corners.iter()) {
        let xd = (corner.x - calib.cx) / calib.fx;
        let yd = (corner.y - calib.cy) / calib.fy;
        let (xu, yu) = undistort_norm(xd, yd, calib);
        dst.x = calib.fx * xu + calib.cx;
        dst.y = calib.fy * yu + calib.cy;
    }

    let from = [(undistorted[0].x, undistorted[0].y), (undistorted[1].x, undistorted[1].y), (undistorted[2].x, undistorted[2].y), (undistorted[3].x, undistorted[3].y)];
    let sample_scale = adaptive_sample_scale(corners, family, sample_scale);
    let side = family.total_width() as u32 * sample_scale;
    let to = [(0.0, 0.0), (side as f32, 0.0), (side as f32, side as f32), (0.0, side as f32)];

    let projection = Projection::from_control_points(from, to)?;
    let inv = projection.invert();

    let mut warped = GrayImage::new(side, side);
    let src_width_i32 = gray.width() as i32;
    let src_height_i32 = gray.height() as i32;
    let src_stride = src_width_i32.max(0) as usize;
    let src_pixels = gray.as_raw();
    for y in 0..side {
        for x in 0..side {
            // Map from canonical square pixel → undistorted input pixel.
            let (ux, uy) = inv * (x as f32, y as f32);
            let xu = (ux - calib.cx) / calib.fx;
            let yu = (uy - calib.cy) / calib.fy;
            // Map from undistorted → distorted, then sample from the original distorted frame.
            let (xd, yd) = distort_norm(xu, yu, calib);
            let sx = calib.fx * xd + calib.cx;
            let sy = calib.fy * yd + calib.cy;
            let v = sample_bilinear_gray_raw(src_pixels, src_width_i32, src_height_i32, src_stride, sx, sy);
            warped.put_pixel(x, y, Luma([v]));
        }
    }
    if !normalize_warped_patch(&mut warped, config.min_warped_patch_contrast_range) {
        return None;
    }

    let grid = decode_marker_grid(&warped, family)?;
    let decoded = family.decode(grid)?;

    // Refine marker corners back into the original (distorted) pixel space.
    let canonical = [(0.0f32, 0.0f32), (side as f32, 0.0f32), (side as f32, side as f32), (0.0f32, side as f32)];
    let mut refined = Vec::with_capacity(4);
    for (x, y) in canonical {
        let (ux, uy) = inv * (x, y);
        let xu = (ux - calib.cx) / calib.fx;
        let yu = (uy - calib.cy) / calib.fy;
        let (xd, yd) = distort_norm(xu, yu, calib);
        refined.push(Point::new(calib.fx * xd + calib.cx, calib.fy * yd + calib.cy));
    }
    Some(ArucoDetectionF32 {
        id: decoded.id,
        rotation: decoded.rotation,
        border_width: decoded.border_width,
        data_width: decoded.data_width,
        corners: Some(refined),
        score: decoded.score,
        best_distance: decoded.best_distance,
        second_distance: decoded.second_distance,
        border_mismatches: decoded.border_mismatches,
        contrast_range: decoded.contrast_range,
    })
}
