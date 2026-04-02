use super::*;

pub(crate) fn build_detector_config(epsilon: f64, min_area: f32, max_area: f32, min_angle_deg: f64, max_angle_deg: f64, max_side_cv: f32, downscale: u32) -> ArucoTagDetectorConfig {
    let scale = downscale as f32;
    let area_scale = (scale * scale).max(1.0);
    ArucoTagDetectorConfig {
        epsilon: epsilon.clamp(0.01, 1000.0) as f32,
        min_area: (min_area / area_scale).max(0.0),
        max_area: if max_area > 0.0 { Some((max_area / area_scale).max(0.0)) } else { None },
        min_angle_deg: min_angle_deg.clamp(0.0, 180.0) as f32,
        max_angle_deg: max_angle_deg.clamp(0.0, 180.0) as f32,
        max_side_cv,
        max_side_ratio: 0.0,
        max_diag_ratio: 0.0,
        angle_cos_min: -1.0,
        angle_cos_max: 1.0,
    }
    .with_angle_cos_bounds()
}

pub(crate) fn contour_points_from_binary_mask(mask: &GrayImage, cfg: &ArucoTagDetectorConfig) -> Vec<Vec<CvPoint<f32>>> {
    let contours = crate::contour::suzuki_abe::suzuki_abe_i32(mask);
    let mut contour_points: Vec<Vec<CvPoint<f32>>> = Vec::with_capacity(contours.len());
    let min_area = cfg.min_area.max(0.0);
    let max_area = cfg.max_area.unwrap_or(f32::INFINITY);
    for contour in contours {
        let pts = contour.points;
        if pts.len() < 4 {
            continue;
        }
        let mut min_x = i32::MAX;
        let mut max_x = i32::MIN;
        let mut min_y = i32::MAX;
        let mut max_y = i32::MIN;
        for p in &pts {
            min_x = min_x.min(p.x);
            max_x = max_x.max(p.x);
            min_y = min_y.min(p.y);
            max_y = max_y.max(p.y);
        }
        let w = (max_x - min_x).max(0) as f32;
        let h = (max_y - min_y).max(0) as f32;
        let bbox_area = w * h;
        if bbox_area < min_area || bbox_area > max_area {
            continue;
        }
        let mut converted: Vec<CvPoint<f32>> = Vec::with_capacity(pts.len());
        for p in pts {
            converted.push(CvPoint::new(p.x as f32, p.y as f32));
        }
        contour_points.push(converted);
    }
    contour_points
}

pub(crate) fn contour_points_from_shared_contours(contours: &[Vec<Point>], cfg: &ArucoTagDetectorConfig) -> Vec<Vec<CvPoint<f32>>> {
    let mut contour_points: Vec<Vec<CvPoint<f32>>> = Vec::with_capacity(contours.len());
    let min_area = cfg.min_area.max(0.0);
    let max_area = cfg.max_area.unwrap_or(f32::INFINITY);
    for contour in contours {
        if contour.len() < 4 {
            continue;
        }
        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut min_y = f64::INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        for p in contour {
            min_x = min_x.min(p.x);
            max_x = max_x.max(p.x);
            min_y = min_y.min(p.y);
            max_y = max_y.max(p.y);
        }
        if !min_x.is_finite() || !max_x.is_finite() || !min_y.is_finite() || !max_y.is_finite() {
            continue;
        }
        let w = (max_x - min_x).max(0.0) as f32;
        let h = (max_y - min_y).max(0.0) as f32;
        let bbox_area = w * h;
        if bbox_area < min_area || bbox_area > max_area {
            continue;
        }
        let mut converted: Vec<CvPoint<f32>> = Vec::with_capacity(contour.len());
        for p in contour {
            converted.push(CvPoint::new(p.x as f32, p.y as f32));
        }
        contour_points.push(converted);
    }
    contour_points
}

pub(crate) fn decode_detections_from_contour_points(
    contour_points: &[Vec<CvPoint<f32>>],
    detect_cfg: &ArucoTagDetectorConfig,
    downscale: u32,
    decode_quads: impl Fn(&[[CvPoint<f32>; 4]]) -> Vec<ArucoDetection2D>,
) -> Vec<ArucoDetection2D> {
    if contour_points.is_empty() {
        return Vec::new();
    }

    let mut quads = filter_candidates(contour_points, detect_cfg);
    if quads.is_empty() {
        return Vec::new();
    }

    if downscale != 1 {
        let scale = downscale as f32;
        for quad in &mut quads {
            for corner in quad.iter_mut() {
                corner.x *= scale;
                corner.y *= scale;
            }
        }
    }

    decode_quads(&quads)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn detect_detections_from_shared_contours_impl(
    frame: Compute<DynamicImage>,
    contours: &Vec<Vec<Point>>,
    dictionary: ArucoDictionaryKind,
    max_hamming: i64,
    downscale: i64,
    sample_scale: i64,
    epsilon: f64,
    min_angle_deg: f64,
    max_angle_deg: f64,
    max_side_cv: f64,
    min_area: f64,
    max_area: f64,
    aruco_decode_cfg: &ArucoDecodeConfig,
    exec_ctx: &ExecutionContext,
    node_name: &'static str,
) -> Result<Vec<ArucoDetection2D>, NodeError> {
    let downscale = u32::try_from(downscale).unwrap_or(1).max(1);
    let sample_scale = u32::try_from(sample_scale).unwrap_or(1).max(1);
    if downscale != 1 {
        return Err(NodeError::InvalidInput("downscale must be 1 (full-res required)".into()));
    }

    let frame = expect_cpu_frame(frame, node_name, Some(exec_ctx))?;
    let detect_cfg = build_detector_config(epsilon, min_area.max(0.0) as f32, max_area.max(0.0) as f32, min_angle_deg, max_angle_deg, max_side_cv.clamp(0.05, 10.0) as f32, downscale);

    let aruco_dict = dictionary.as_aruco_name().and_then(aruco_dictionary_from_name);
    let family_impl = dictionary.as_apriltag_family().map(|family| {
        let mut impl_family = family.into_family();
        if max_hamming >= 0 {
            impl_family = impl_family.with_max_hamming(u8::try_from(max_hamming).unwrap_or(0));
        }
        impl_family
    });
    if aruco_dict.is_none() && family_impl.is_none() {
        return Err(NodeError::InvalidInput("unknown tag dictionary".into()));
    }

    let (pw, ph) = frame.dimensions();
    if detect_cfg.min_area > (pw as f32 * ph as f32) {
        return Ok(Vec::new());
    }

    let contour_points = contour_points_from_shared_contours(contours, &detect_cfg);
    if contour_points.is_empty() {
        return Ok(Vec::new());
    }
    let mut quads = filter_candidates(&contour_points, &detect_cfg);
    if quads.is_empty() {
        return Ok(Vec::new());
    }
    if downscale != 1 {
        let scale = downscale as f32;
        for quad in &mut quads {
            for corner in quad.iter_mut() {
                corner.x *= scale;
                corner.y *= scale;
            }
        }
    }

    const STRICT_DECODE_MIN_SIDE_PX: f32 = 56.0;
    let mut strict_quads: Vec<[CvPoint<f32>; 4]> = Vec::new();
    let mut relaxed_quads: Vec<[CvPoint<f32>; 4]> = Vec::new();
    for quad in quads {
        let mut side_sum = 0.0f32;
        for i in 0..4 {
            let p0 = quad[i];
            let p1 = quad[(i + 1) & 3];
            let dx = p1.x - p0.x;
            let dy = p1.y - p0.y;
            side_sum += (dx * dx + dy * dy).sqrt();
        }
        let mean_side = side_sum * 0.25;
        if mean_side >= STRICT_DECODE_MIN_SIDE_PX {
            strict_quads.push(quad);
        } else {
            relaxed_quads.push(quad);
        }
    }

    let mut markers: Vec<ArucoDetection2D> = Vec::new();
    if let Some(dict) = aruco_dict.as_ref() {
        let relaxed_cfg = *aruco_decode_cfg;
        let mut strict_cfg = relaxed_cfg;
        strict_cfg.min_quiet_zone_delta = strict_cfg.min_quiet_zone_delta.max(3.0);
        strict_cfg.min_cell_means_contrast_range = strict_cfg.min_cell_means_contrast_range.max(10.0);
        strict_cfg.min_hamming_margin = strict_cfg.min_hamming_margin.max(3);
        strict_cfg.min_hamming_margin_min_dist = strict_cfg.min_hamming_margin_min_dist.max(2);
        strict_cfg.min_hamming_margin_only_if_border_mismatch = false;
        strict_cfg.min_bit_delta = strict_cfg.min_bit_delta.max(3.0);
        strict_cfg.min_decode_score = strict_cfg.min_decode_score.max(2.0);
        if !relaxed_quads.is_empty() {
            markers.extend(decode_quads_aruco_with_config(&frame, &relaxed_quads, sample_scale, dict, &relaxed_cfg));
        }
        if !strict_quads.is_empty() {
            markers.extend(decode_quads_aruco_with_config(&frame, &strict_quads, sample_scale, dict, &strict_cfg));
        }
    } else if let Some(family_impl) = family_impl.as_ref() {
        let relaxed_cfg = ArucoTagDecodeConfig::default();
        let mut strict_cfg = relaxed_cfg;
        strict_cfg.min_quiet_zone_delta = strict_cfg.min_quiet_zone_delta.max(3.0);
        strict_cfg.quiet_zone_texture_penalty = strict_cfg.quiet_zone_texture_penalty.max(12.0);
        strict_cfg.min_decode_score = strict_cfg.min_decode_score.max(2.0);
        strict_cfg.cell_decode.min_cell_means_contrast_range = strict_cfg.cell_decode.min_cell_means_contrast_range.max(12.0);
        strict_cfg.cell_decode.min_hamming_margin = strict_cfg.cell_decode.min_hamming_margin.max(4);
        strict_cfg.cell_decode.min_hamming_margin_min_dist = strict_cfg.cell_decode.min_hamming_margin_min_dist.max(2);
        strict_cfg.cell_decode.min_hamming_margin_only_if_border_mismatch = false;
        strict_cfg.cell_decode.min_bit_delta = strict_cfg.cell_decode.min_bit_delta.max(4.0);
        if !relaxed_quads.is_empty() {
            markers.extend(decode_quads_with_config(&frame, &relaxed_quads, sample_scale, family_impl, &relaxed_cfg));
        }
        if !strict_quads.is_empty() {
            markers.extend(decode_quads_with_config(&frame, &strict_quads, sample_scale, family_impl, &strict_cfg));
        }
    }

    Ok(merge_detections_spatial_impl(markers, 8.0, 0.0, 2.5, 0.0, 0.0))
}

#[node(
        id = "detect_detections",
        inputs("frame", config = ArucoTagOverlayConfig),
        outputs(port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()))
                    )]
pub(crate) fn cv_detect_aruco_detections(frame: Compute<DynamicImage>, cfg: ArucoTagOverlayConfig, exec_ctx: &ExecutionContext) -> Result<Vec<ArucoDetection2D>, NodeError> {
    let downscale = u32::try_from(cfg.downscale).unwrap_or(1).max(1);
    let sample_scale = u32::try_from(cfg.sample_scale).unwrap_or(1).max(1);
    let threshold_window = u32::try_from(cfg.threshold_window).unwrap_or(31).max(3);
    let threshold_offset = cfg.threshold_offset.clamp(-255.0, 255.0);
    let use_otsu_threshold = matches!(cfg.threshold_mode, crate::modules::aruco::ArucoMaskMode::Otsu);
    let blur_sigma = cfg.blur_sigma.clamp(0.0, 20.0) as f32;
    let detect_every_n = u32::try_from(cfg.detect_every_n).unwrap_or(1).max(1);
    let max_side_cv = cfg.max_side_cv.clamp(0.05, 10.0) as f32;
    let min_area = cfg.min_area.max(0.0) as f32;
    let max_area = cfg.max_area.max(0.0) as f32;

    if downscale != 1 {
        return Err(NodeError::InvalidInput("downscale must be 1 (full-res required)".into()));
    }
    if detect_every_n != 1 {
        return Err(NodeError::InvalidInput("detect_every_n must be 1 (no frame skipping)".into()));
    }

    let mut out = expect_cpu_frame(frame, "aruco_detect_detections", Some(exec_ctx))?;
    let gamma = cfg.gamma.clamp(0.05, 5.0);
    if (gamma - 1.0).abs() >= 1e-3 {
        if !matches!(out, DynamicImage::ImageLuma8(_)) {
            out = DynamicImage::ImageLuma8(out.to_luma8());
        }
        if let DynamicImage::ImageLuma8(gray) = &mut out {
            let mut lut = [0u8; 256];
            for (i, out) in lut.iter_mut().enumerate() {
                let x = i as f64 / 255.0;
                *out = ((x.powf(gamma) * 255.0).round() as i64).clamp(0, 255) as u8;
            }
            for px in gray.as_mut() {
                *px = lut[*px as usize];
            }
        }
    }

    let processing = &out;
    let (pw, ph) = processing.dimensions();
    let detect_cfg = build_detector_config(cfg.epsilon, min_area, max_area, cfg.min_angle_deg, cfg.max_angle_deg, max_side_cv, downscale);

    let aruco_dict = cfg.dictionary.as_aruco_name().and_then(aruco_dictionary_from_name);
    let family_impl = cfg.dictionary.as_apriltag_family().map(|family| {
        let mut impl_family = family.into_family();
        if cfg.max_hamming >= 0 {
            impl_family = impl_family.with_max_hamming(u8::try_from(cfg.max_hamming).unwrap_or(0));
        }
        impl_family
    });
    if aruco_dict.is_none() && family_impl.is_none() {
        return Err(NodeError::InvalidInput("unknown tag dictionary".into()));
    }
    if detect_cfg.min_area > (pw as f32 * ph as f32) {
        return Ok(Vec::new());
    }

    let markers = with_luma8_frame(processing, |gray_ref| {
        let gray_blurred;
        let gray = if blur_sigma > 0.0 {
            gray_blurred = crate::modules::image::blur::blur_gray_image(gray_ref.clone(), blur_sigma);
            &gray_blurred
        } else {
            gray_ref
        };

        let decode_quads = |quads: &[[CvPoint<f32>; 4]]| -> Vec<ArucoDetection2D> {
            if let Some(dict) = aruco_dict.as_ref() {
                decode_quads_aruco_with_config(&out, quads, sample_scale, dict, &ArucoDecodeConfig::default())
            } else if let Some(family_impl) = family_impl.as_ref() {
                decode_quads_with_config(&out, quads, sample_scale, family_impl, &ArucoTagDecodeConfig::default())
            } else {
                Vec::new()
            }
        };
        let detect_once = |mask: &GrayImage| -> Vec<ArucoDetection2D> {
            let contour_points = contour_points_from_binary_mask(mask, &detect_cfg);
            decode_detections_from_contour_points(&contour_points, &detect_cfg, downscale, decode_quads)
        };

        let threshold_window = threshold_window.max(3);
        let threshold_offset = threshold_offset as f32;
        let mut raw_markers = if use_otsu_threshold {
            let threshold = crate::modules::image::binary::otsu_level_gray(gray);
            crate::modules::image::binary::with_binary_threshold_mask(gray, threshold, cfg.invert_mask, detect_once)
        } else {
            crate::modules::image::binary::with_adaptive_mean_threshold_fast(gray, threshold_window, threshold_offset, cfg.invert_mask, detect_once)
        };

        if cfg.try_opposite_polarity {
            let opposite_markers = if use_otsu_threshold {
                let threshold = crate::modules::image::binary::otsu_level_gray(gray);
                crate::modules::image::binary::with_binary_threshold_mask(gray, threshold, !cfg.invert_mask, detect_once)
            } else {
                crate::modules::image::binary::with_adaptive_mean_threshold_fast(gray, threshold_window, threshold_offset, !cfg.invert_mask, detect_once)
            };
            raw_markers.extend(opposite_markers);
        }

        merge_detections_spatial_impl(raw_markers, 8.0, 0.0, 2.5, 0.0, 0.0)
    });

    Ok(markers)
}

#[node(
    id = "detect_detections_from_contours",
    summary = "Decode detections from shared contours.",
    description = "Consumes precomputed contours and runs candidate filtering + decode. Use this when a single contour pass should be shared across branches.",
    inputs(
        port(name = "frame", ty = TypeExpr::opaque("image:dynamic")),
        port(name = "contours", source = "Contours", ty = crate::daedalus_types::contours()),
        port(name = "dictionary", default = "apriltag_16h5"),
        port(name = "max_hamming", default = -1i64, meta(ui_min = -1, ui_max = 4, ui_step = 1)),
        port(name = "downscale", default = 1i64, meta(ui_min = 1, ui_max = 8, ui_step = 1)),
        port(name = "sample_scale", default = 1i64, meta(ui_min = 1, ui_max = 8, ui_step = 1)),
        port(name = "epsilon", default = 0.03f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01)),
        port(name = "min_angle_deg", default = 0.0f64, meta(ui_min = 0.0, ui_max = 180.0, ui_step = 1.0)),
        port(name = "max_angle_deg", default = 180.0f64, meta(ui_min = 0.0, ui_max = 180.0, ui_step = 1.0)),
        port(name = "max_side_cv", default = 10.0f64, meta(ui_min = 0.1, ui_max = 10.0, ui_step = 0.1)),
        port(name = "min_area", default = 0.0f64, meta(ui_min = 0.0, ui_max = 1000000.0, ui_step = 1000.0)),
        port(name = "max_area", default = 0.0f64, meta(ui_min = 0.0, ui_max = 1000000.0, ui_step = 1000.0))
    ),
    outputs(port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()))
)]
#[allow(clippy::too_many_arguments)]
pub(crate) fn cv_detect_aruco_detections_from_contours(
    frame: Compute<DynamicImage>,
    contours: &Vec<Vec<Point>>,
    dictionary: ArucoDictionaryKind,
    max_hamming: i64,
    downscale: i64,
    sample_scale: i64,
    epsilon: f64,
    min_angle_deg: f64,
    max_angle_deg: f64,
    max_side_cv: f64,
    min_area: f64,
    max_area: f64,
    exec_ctx: &ExecutionContext,
) -> Result<Vec<ArucoDetection2D>, NodeError> {
    detect_detections_from_shared_contours_impl(
        frame,
        contours,
        dictionary,
        max_hamming,
        downscale,
        sample_scale,
        epsilon,
        min_angle_deg,
        max_angle_deg,
        max_side_cv,
        min_area,
        max_area,
        &ArucoDecodeConfig::default(),
        exec_ctx,
        "aruco_detect_detections_from_contours",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_detection() -> ArucoDetection2D {
        ArucoDetection2D {
            id: 3,
            rotation: 0,
            corners: [Point { x: 24.0, y: 24.0 }, Point { x: 72.0, y: 24.0 }, Point { x: 72.0, y: 72.0 }, Point { x: 24.0, y: 72.0 }],
            score: None,
            best_distance: None,
            second_distance: None,
            border_mismatches: None,
            contrast_range: None,
            border_width: None,
            data_width: None,
            bits: None,
        }
    }

    #[test]
    fn overlay_hot_path_preserves_luma8_frame() {
        let mut out = DynamicImage::ImageLuma8(GrayImage::from_pixel(96, 96, Luma([32])));
        let det = sample_detection();
        let bbox = [
            CvPoint::new(det.corners[0].x as f32, det.corners[0].y as f32),
            CvPoint::new(det.corners[1].x as f32, det.corners[1].y as f32),
            CvPoint::new(det.corners[2].x as f32, det.corners[2].y as f32),
            CvPoint::new(det.corners[3].x as f32, det.corners[3].y as f32),
        ];

        draw::contour::overlay_contour_points(&mut out, &bbox, 1, Rgba([0, 255, 255, 255]));
        for corner in det.corners {
            draw::shape::overlay_circle(&mut out, (corner.x.max(0.0) as u32, corner.y.max(0.0) as u32), 3, true, Rgba([255, 64, 0, 255]));
        }
        super::overlay_draw::overlay_tags_count(&mut out, 1);

        match out {
            DynamicImage::ImageLuma8(image) => {
                assert_eq!(image.dimensions(), (96, 96));
            }
            other => panic!("expected luma8 overlay output, got {other:?}"),
        }
    }
}
