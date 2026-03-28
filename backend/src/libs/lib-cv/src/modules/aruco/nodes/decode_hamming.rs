use super::*;

#[node(
    id = "decode_quads_hamming_decode_detections",
    inputs(
        "frame",
        port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()),
        port(name = "dictionary", default = "4x4_1000"),
        port(name = "sample_scale", default = 2i64, meta(ui_min = 1, ui_max = 8, ui_step = 1)),
        port(name = "max_hamming", default = -1i64, meta(ui_min = -1, ui_max = 4, ui_step = 1)),
        port(name = "border_error_divisor", default = -1i64, meta(ui_min = -1, ui_max = 50, ui_step = 1)),
        config = ArucoTagDecodeTuningConfig
    ),
    outputs(port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()))
)]
#[allow(clippy::too_many_arguments)]
fn cv_aruco_decode_quads_hamming_decode_detections(
    frame: &DynamicImage,
    quads: &Vec<Quad>,
    dictionary: ArucoDictionaryKind,
    sample_scale: i64,
    max_hamming: i64,
    border_error_divisor: i64,
    cfg: ArucoTagDecodeTuningConfig,
) -> Result<Vec<ArucoDetection2D>, NodeError> {
    let _scratch_guard = ArucoDecodeFrameScratchGuard::new();
    let sample_scale = u32::try_from(sample_scale).unwrap_or(1).max(1);
    if quads.is_empty() {
        return Ok(Vec::new());
    }

    if let Some(dict_name) = dictionary.as_aruco_name() {
        let Some(dict) = aruco_dictionary_from_name(dict_name) else {
            return Err(NodeError::InvalidInput(format!("unknown ArUco dictionary '{dict_name}'")));
        };
        let mut decode_cfg = decode_tuning_to_aruco_config(&cfg);
        if max_hamming >= 0 {
            let max_hamming = u32::try_from(max_hamming).unwrap_or(0);
            let max_corr = dict.max_correction_bits() as u32;
            decode_cfg.error_correction_rate = if max_corr == 0 { 0.0 } else { (max_hamming as f32 / max_corr as f32).clamp(0.0, 1.0) };
        }
        if border_error_divisor > 0 {
            let div = (border_error_divisor as f32).max(1.0);
            decode_cfg.max_border_error_rate = decode_cfg.max_border_error_rate.min((1.0 / div).clamp(0.0, 1.0));
            if border_error_divisor >= 32 {
                decode_cfg.max_border_error_rate = 0.0;
            }
            decode_cfg.min_hamming_margin_only_if_border_mismatch = false;
        }
        if dict.marker_size() <= 4 {
            decode_cfg.min_warped_patch_contrast_range = decode_cfg.min_warped_patch_contrast_range.max(8);
            decode_cfg.min_cell_means_contrast_range = decode_cfg.min_cell_means_contrast_range.max(10.0);
            decode_cfg.min_quad_side_px = decode_cfg.min_quad_side_px.max(10.0);
            decode_cfg.max_border_error_rate = decode_cfg.max_border_error_rate.min(0.2);
            decode_cfg.error_correction_rate = decode_cfg.error_correction_rate.min(0.4);
        }

        let detections = DECODE_QUAD_SCRATCH.with(|quad_scratch| {
            let mut quad_scratch = quad_scratch.borrow_mut();
            let cv_quads = &mut quad_scratch.quads;
            cv_quads.resize(quads.len(), [CvPoint::new(0.0, 0.0); 4]);
            for (i, q) in quads.iter().enumerate() {
                cv_quads[i] =
                    [CvPoint::new(q[0].x as f32, q[0].y as f32), CvPoint::new(q[1].x as f32, q[1].y as f32), CvPoint::new(q[2].x as f32, q[2].y as f32), CvPoint::new(q[3].x as f32, q[3].y as f32)];
            }
            decode_quads_aruco_with_config_no_bits(frame, cv_quads, sample_scale, &dict, &decode_cfg)
        });
        return Ok(detections);
    }

    let Some(family) = dictionary.as_apriltag_family() else {
        return Err(NodeError::InvalidInput("unknown tag dictionary".into()));
    };
    let decode_cfg = decode_tuning_to_config(&cfg);
    let detections = DECODE_FAMILY_SCRATCH.with(|family_scratch| {
        let mut family_scratch = family_scratch.borrow_mut();
        if family_scratch.kind != Some(family) || family_scratch.max_hamming != max_hamming || family_scratch.border_error_divisor != border_error_divisor {
            let mut family_impl = family.into_family();
            if max_hamming >= 0 {
                family_impl = family_impl.with_max_hamming(u8::try_from(max_hamming).unwrap_or(0));
            }
            if border_error_divisor > 0 {
                family_impl = family_impl.with_border_error_divisor(u8::try_from(border_error_divisor).unwrap_or(10));
            }
            family_scratch.family = family_impl;
            family_scratch.kind = Some(family);
            family_scratch.max_hamming = max_hamming;
            family_scratch.border_error_divisor = border_error_divisor;
        }

        DECODE_QUAD_SCRATCH.with(|quad_scratch| {
            let mut quad_scratch = quad_scratch.borrow_mut();
            let cv_quads = &mut quad_scratch.quads;
            cv_quads.resize(quads.len(), [CvPoint::new(0.0, 0.0); 4]);
            for (i, q) in quads.iter().enumerate() {
                cv_quads[i] =
                    [CvPoint::new(q[0].x as f32, q[0].y as f32), CvPoint::new(q[1].x as f32, q[1].y as f32), CvPoint::new(q[2].x as f32, q[2].y as f32), CvPoint::new(q[3].x as f32, q[3].y as f32)];
            }
            decode_quads_with_config_no_bits(frame, cv_quads, sample_scale, &family_scratch.family, &decode_cfg)
        })
    });
    Ok(detections)
}

#[node(
    id = "decode_quads_hamming_refine_detections",
    inputs(
        "frame",
        port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()),
        port(name = "dictionary", default = "4x4_1000"),
        port(name = "sample_scale", default = 2i64, meta(ui_min = 1, ui_max = 8, ui_step = 1)),
        port(name = "max_hamming", default = -1i64, meta(ui_min = -1, ui_max = 4, ui_step = 1)),
        port(name = "border_error_divisor", default = -1i64, meta(ui_min = -1, ui_max = 50, ui_step = 1)),
        port(name = "refine_corners_warp", default = false),
        port(name = "refine_corners_warp_scale", default = 0i64, meta(ui_min = 0, ui_max = 8, ui_step = 1)),
        config = ArucoTagDecodeTuningConfig
    ),
    outputs(port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()))
)]
#[allow(clippy::too_many_arguments)]
fn cv_aruco_decode_quads_hamming_refine_detections(
    frame: &DynamicImage,
    detections: std::sync::Arc<Vec<ArucoDetection2D>>,
    dictionary: ArucoDictionaryKind,
    sample_scale: i64,
    max_hamming: i64,
    border_error_divisor: i64,
    refine_corners_warp: bool,
    refine_corners_warp_scale: i64,
    cfg: ArucoTagDecodeTuningConfig,
) -> Result<Vec<ArucoDetection2D>, NodeError> {
    let _scratch_guard = ArucoDecodeFrameScratchGuard::new();
    let _ = (frame, dictionary, sample_scale, max_hamming, border_error_divisor, refine_corners_warp, refine_corners_warp_scale, cfg);
    // Corner refinement is currently disabled: existing refinement paths can degrade corner
    // stability on slight blur/perspective (including occasional crossed corners).
    //
    // Keep decoded corners as-is until a replacement refine method is implemented.
    Ok(std::sync::Arc::unwrap_or_clone(detections))
}

#[node(
    id = "decode_quads_hamming_finalize_detections",
    inputs(
        "frame",
        port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()),
        port(name = "dictionary", default = "4x4_1000"),
        port(name = "max_hamming", default = -1i64, meta(ui_min = -1, ui_max = 4, ui_step = 1)),
        port(name = "canonicalize", default = true),
        port(name = "min_id", default = -1i64, meta(ui_min = -1, ui_max = 10000, ui_step = 1)),
        port(name = "max_id", default = -1i64, meta(ui_min = -1, ui_max = 10000, ui_step = 1))
    ),
    outputs(port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()))
)]
fn cv_aruco_decode_quads_hamming_finalize_detections(
    frame: &DynamicImage,
    detections: std::sync::Arc<Vec<ArucoDetection2D>>,
    dictionary: ArucoDictionaryKind,
    max_hamming: i64,
    canonicalize: bool,
    min_id: i64,
    max_id: i64,
) -> Result<Vec<ArucoDetection2D>, NodeError> {
    let _scratch_guard = ArucoDecodeFrameScratchGuard::new();
    let (frame_w, frame_h) = (frame.width(), frame.height());
    let mut detections = std::sync::Arc::unwrap_or_clone(detections);

    if detections.is_empty() {
        return Ok(Vec::new());
    }

    if canonicalize {
        for det in &mut detections {
            det.canonicalize_in_place();
        }
    }
    filter_detections_id_range_in_place(&mut detections, min_id, max_id);
    filter_detections_in_frame_in_place(&mut detections, frame_w, frame_h);

    if let Some(dict_name) = dictionary.as_aruco_name() {
        let Some(dict) = aruco_dictionary_from_name(dict_name) else {
            return Err(NodeError::InvalidInput(format!("unknown ArUco dictionary '{dict_name}'")));
        };
        let max_hamming_u32 = if max_hamming >= 0 { u32::try_from(max_hamming).unwrap_or(0) } else { u32::MAX };
        let include_bits = aruco_include_bits_enabled();
        detections.retain(|det| !(max_hamming_u32 != u32::MAX && det.best_distance.is_some_and(|best| best > max_hamming_u32)));
        if include_bits {
            for det in &mut detections {
                if det.bits.is_none() {
                    det.bits = dict.bit_grid(det.id as usize);
                }
            }
        }
        return Ok(detections);
    }

    let Some(family) = dictionary.as_apriltag_family() else {
        return Err(NodeError::InvalidInput("unknown tag dictionary".into()));
    };
    let family_impl = family.into_family();
    let include_bits = aruco_include_bits_enabled();
    if include_bits {
        for det in &mut detections {
            if det.bits.is_none() {
                det.bits = family_impl.bit_grid(det.id as usize);
            }
        }
    }
    Ok(detections)
}

#[node(
    id = "decode_quads_hamming",
    summary = "Decode quad candidates into marker detections.",
    description = "Stage-split node-group version of the historical `decode_quads_hamming` implementation. Exposes decode/finalization stages as independent profiles.",
    inputs(
        port(name = "frame", source = "Frame", ty = daedalus::data::model::TypeExpr::opaque("image:gray8")),
        port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()),
        port(name = "dictionary", default = "4x4_1000"),
        port(name = "sample_scale", default = 2i64, meta(ui_min = 1, ui_max = 8, ui_step = 1)),
        port(name = "max_hamming", default = -1i64, meta(ui_min = -1, ui_max = 4, ui_step = 1)),
        port(name = "border_error_divisor", default = -1i64, meta(ui_min = -1, ui_max = 50, ui_step = 1)),
        port(name = "refine_corners_warp", default = false),
        port(name = "refine_corners_warp_scale", default = 0i64, meta(ui_min = 0, ui_max = 8, ui_step = 1)),
        port(name = "min_id", default = -1i64, meta(ui_min = -1, ui_max = 10000, ui_step = 1)),
        port(name = "max_id", default = -1i64, meta(ui_min = -1, ui_max = 10000, ui_step = 1)),
        port(name = "min_warped_patch_contrast_range", default = 8i64, meta(ui_min = 0, ui_max = 255, ui_step = 1)),
        port(name = "warp_fallback_on_decode_fail", default = true),
        port(name = "warp_fallback_max_hamming_extra", default = 2i64, meta(ui_min = 0, ui_max = 32, ui_step = 1)),
        port(name = "warp_fallback_border_slack", default = 6i64, meta(ui_min = 0, ui_max = 32, ui_step = 1)),
        port(name = "warp_fallback_on_low_contrast", default = true),
        port(name = "warp_min_sample_scale", default = 4i64, meta(ui_min = 1, ui_max = 32, ui_step = 1)),
        port(name = "min_quad_side_px", default = 0.0f64, meta(ui_min = 0.0, ui_max = 200.0, ui_step = 0.5)),
        port(name = "min_quiet_zone_delta", default = 0.0f64, meta(ui_min = 0.0, ui_max = 64.0, ui_step = 0.5)),
        port(name = "quiet_zone_texture_penalty", default = 12.0f64, meta(ui_min = 0.0, ui_max = 64.0, ui_step = 0.5)),
        port(name = "verify_warp_min_best_distance", default = 99i64, meta(ui_min = 0, ui_max = 99, ui_step = 1)),
        port(name = "verify_warp_only_if_border_mismatch", default = true),
        port(name = "verify_warp_reject_on_fail", default = false),
        port(name = "min_decode_score", default = -1.0f64, meta(ui_min = -100.0, ui_max = 100.0, ui_step = 1.0)),
        port(name = "cell_sample_grid", default = 0i64, meta(ui_min = 0, ui_max = 7, ui_step = 1)),
        port(name = "cell_sample_margin", default = 0.25f64, meta(ui_min = 0.0, ui_max = 0.45, ui_step = 0.01)),
        port(name = "min_cell_means_contrast_range", default = 10.0f64, meta(ui_min = 0.0, ui_max = 255.0, ui_step = 1.0)),
        port(name = "min_hamming_margin", default = 2i64, meta(ui_min = 0, ui_max = 32, ui_step = 1)),
        port(name = "min_hamming_margin_min_dist", default = 1i64, meta(ui_min = 0, ui_max = 32, ui_step = 1)),
        port(name = "min_hamming_margin_only_if_border_mismatch", default = false),
        port(name = "min_bit_delta", default = 2.0f64, meta(ui_min = 0.0, ui_max = 255.0, ui_step = 1.0))
    ),
    outputs(port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()))
)]
#[allow(clippy::too_many_arguments)]
fn cv_aruco_decode_quads_hamming(
    frame: &GrayImage,
    quads: std::sync::Arc<Vec<Quad>>,
    dictionary: ArucoDictionaryKind,
    sample_scale: i64,
    max_hamming: i64,
    border_error_divisor: i64,
    refine_corners_warp: bool,
    refine_corners_warp_scale: i64,
    min_id: i64,
    max_id: i64,
    min_warped_patch_contrast_range: i64,
    warp_fallback_on_decode_fail: bool,
    warp_fallback_max_hamming_extra: i64,
    warp_fallback_border_slack: i64,
    warp_fallback_on_low_contrast: bool,
    warp_min_sample_scale: i64,
    min_quad_side_px: f64,
    min_quiet_zone_delta: f64,
    quiet_zone_texture_penalty: f64,
    verify_warp_min_best_distance: i64,
    verify_warp_only_if_border_mismatch: bool,
    verify_warp_reject_on_fail: bool,
    min_decode_score: f64,
    cell_sample_grid: i64,
    cell_sample_margin: f64,
    min_cell_means_contrast_range: f64,
    min_hamming_margin: i64,
    min_hamming_margin_min_dist: i64,
    min_hamming_margin_only_if_border_mismatch: bool,
    min_bit_delta: f64,
    exec_ctx: &ExecutionContext,
) -> Result<Vec<ArucoDetection2D>, NodeError> {
    let _scratch_guard = ArucoDecodeFrameScratchGuard::new();
    let _ = exec_ctx;
    decode_quads_hamming_gray_inner(
        frame,
        quads.as_slice(),
        dictionary,
        sample_scale,
        max_hamming,
        border_error_divisor,
        refine_corners_warp,
        refine_corners_warp_scale,
        min_id,
        max_id,
        min_warped_patch_contrast_range,
        warp_fallback_on_decode_fail,
        warp_fallback_max_hamming_extra,
        warp_fallback_border_slack,
        warp_fallback_on_low_contrast,
        warp_min_sample_scale,
        min_quad_side_px,
        min_quiet_zone_delta,
        quiet_zone_texture_penalty,
        verify_warp_min_best_distance,
        verify_warp_only_if_border_mismatch,
        verify_warp_reject_on_fail,
        min_decode_score,
        cell_sample_grid,
        cell_sample_margin,
        min_cell_means_contrast_range,
        min_hamming_margin,
        min_hamming_margin_min_dist,
        min_hamming_margin_only_if_border_mismatch,
        min_bit_delta,
    )
}

#[allow(clippy::too_many_arguments)]
fn decode_quads_hamming_gray_inner(
    frame: &GrayImage,
    quads: &[Quad],
    dictionary: ArucoDictionaryKind,
    sample_scale: i64,
    max_hamming: i64,
    border_error_divisor: i64,
    refine_corners_warp: bool,
    refine_corners_warp_scale: i64,
    min_id: i64,
    max_id: i64,
    min_warped_patch_contrast_range: i64,
    warp_fallback_on_decode_fail: bool,
    warp_fallback_max_hamming_extra: i64,
    warp_fallback_border_slack: i64,
    warp_fallback_on_low_contrast: bool,
    warp_min_sample_scale: i64,
    min_quad_side_px: f64,
    min_quiet_zone_delta: f64,
    quiet_zone_texture_penalty: f64,
    verify_warp_min_best_distance: i64,
    verify_warp_only_if_border_mismatch: bool,
    verify_warp_reject_on_fail: bool,
    min_decode_score: f64,
    cell_sample_grid: i64,
    cell_sample_margin: f64,
    min_cell_means_contrast_range: f64,
    min_hamming_margin: i64,
    min_hamming_margin_min_dist: i64,
    min_hamming_margin_only_if_border_mismatch: bool,
    min_bit_delta: f64,
) -> Result<Vec<ArucoDetection2D>, NodeError> {
    let _scratch_guard = ArucoDecodeFrameScratchGuard::new();

    #[inline(always)]
    fn quad_min_side_px(quad: &Quad) -> f64 {
        let mut min_side_sq = f64::MAX;
        for i in 0..4usize {
            let a = quad[i];
            let b = quad[(i + 1) % 4];
            let dx = a.x - b.x;
            let dy = a.y - b.y;
            let d2 = dx * dx + dy * dy;
            min_side_sq = min_side_sq.min(d2);
        }
        min_side_sq.sqrt()
    }

    let (frame_w, frame_h) = frame.dimensions();
    let _ = (refine_corners_warp, refine_corners_warp_scale);
    let tuning = ArucoTagDecodeTuningConfig {
        min_warped_patch_contrast_range,
        warp_fallback_on_decode_fail,
        warp_fallback_max_hamming_extra,
        warp_fallback_border_slack,
        warp_fallback_on_low_contrast,
        warp_min_sample_scale,
        min_quad_side_px,
        min_quiet_zone_delta,
        quiet_zone_texture_penalty,
        verify_warp_min_best_distance,
        verify_warp_only_if_border_mismatch,
        verify_warp_reject_on_fail,
        min_decode_score,
        cell_sample_grid,
        cell_sample_margin,
        min_cell_means_contrast_range,
        min_hamming_margin,
        min_hamming_margin_min_dist,
        min_hamming_margin_only_if_border_mismatch,
        min_bit_delta,
    };

    let min_quad_side_px = min_quad_side_px.max(0.0);
    let filtered_quads: Option<Vec<Quad>> = if min_quad_side_px > 0.0 { Some(quads.iter().copied().filter(|q| quad_min_side_px(q) >= min_quad_side_px).collect()) } else { None };
    let quads = filtered_quads.as_deref().unwrap_or(quads);

    let sample_scale_u32 = u32::try_from(sample_scale).unwrap_or(1).max(1);
    let decoded = if quads.is_empty() {
        Vec::new()
    } else if let Some(dict_name) = dictionary.as_aruco_name() {
        let Some(dict) = aruco_dictionary_from_name(dict_name) else {
            return Err(NodeError::InvalidInput(format!("unknown ArUco dictionary '{dict_name}'")));
        };
        let mut decode_cfg = decode_tuning_to_aruco_config(&tuning);
        if max_hamming >= 0 {
            let max_hamming = u32::try_from(max_hamming).unwrap_or(0);
            let max_corr = dict.max_correction_bits() as u32;
            decode_cfg.error_correction_rate = if max_corr == 0 { 0.0 } else { (max_hamming as f32 / max_corr as f32).clamp(0.0, 1.0) };
        }
        if border_error_divisor > 0 {
            let div = (border_error_divisor as f32).max(1.0);
            decode_cfg.max_border_error_rate = decode_cfg.max_border_error_rate.min((1.0 / div).clamp(0.0, 1.0));
            if border_error_divisor >= 32 {
                decode_cfg.max_border_error_rate = 0.0;
            }
            decode_cfg.min_hamming_margin_only_if_border_mismatch = false;
        }
        if dict.marker_size() <= 4 {
            decode_cfg.min_warped_patch_contrast_range = decode_cfg.min_warped_patch_contrast_range.max(8);
            decode_cfg.min_cell_means_contrast_range = decode_cfg.min_cell_means_contrast_range.max(10.0);
            decode_cfg.min_quad_side_px = decode_cfg.min_quad_side_px.max(10.0);
            decode_cfg.max_border_error_rate = decode_cfg.max_border_error_rate.min(0.2);
            decode_cfg.error_correction_rate = decode_cfg.error_correction_rate.min(0.4);
        }
        DECODE_QUAD_SCRATCH.with(|quad_scratch| {
            let mut quad_scratch = quad_scratch.borrow_mut();
            let cv_quads = &mut quad_scratch.quads;
            cv_quads.resize(quads.len(), [CvPoint::new(0.0, 0.0); 4]);
            for (i, q) in quads.iter().enumerate() {
                cv_quads[i] =
                    [CvPoint::new(q[0].x as f32, q[0].y as f32), CvPoint::new(q[1].x as f32, q[1].y as f32), CvPoint::new(q[2].x as f32, q[2].y as f32), CvPoint::new(q[3].x as f32, q[3].y as f32)];
            }
            decode_quads_aruco_with_config_no_bits_gray(frame, cv_quads, sample_scale_u32, &dict, &decode_cfg)
        })
    } else {
        let Some(family) = dictionary.as_apriltag_family() else {
            return Err(NodeError::InvalidInput("unknown tag dictionary".into()));
        };
        let decode_cfg = decode_tuning_to_config(&tuning);
        DECODE_FAMILY_SCRATCH.with(|family_scratch| {
            let mut family_scratch = family_scratch.borrow_mut();
            if family_scratch.kind != Some(family) || family_scratch.max_hamming != max_hamming || family_scratch.border_error_divisor != border_error_divisor {
                let mut family_impl = family.into_family();
                if max_hamming >= 0 {
                    family_impl = family_impl.with_max_hamming(u8::try_from(max_hamming).unwrap_or(0));
                }
                if border_error_divisor > 0 {
                    family_impl = family_impl.with_border_error_divisor(u8::try_from(border_error_divisor).unwrap_or(10));
                }
                family_scratch.family = family_impl;
                family_scratch.kind = Some(family);
                family_scratch.max_hamming = max_hamming;
                family_scratch.border_error_divisor = border_error_divisor;
            }
            DECODE_QUAD_SCRATCH.with(|quad_scratch| {
                let mut quad_scratch = quad_scratch.borrow_mut();
                let cv_quads = &mut quad_scratch.quads;
                cv_quads.resize(quads.len(), [CvPoint::new(0.0, 0.0); 4]);
                for (i, q) in quads.iter().enumerate() {
                    cv_quads[i] = [
                        CvPoint::new(q[0].x as f32, q[0].y as f32),
                        CvPoint::new(q[1].x as f32, q[1].y as f32),
                        CvPoint::new(q[2].x as f32, q[2].y as f32),
                        CvPoint::new(q[3].x as f32, q[3].y as f32),
                    ];
                }
                decode_quads_with_config_no_bits_gray(frame, cv_quads, sample_scale_u32, &family_scratch.family, &decode_cfg)
            })
        })
    };

    let refined = decoded;

    let finalized = if refined.is_empty() {
        Vec::new()
    } else if let Some(dict_name) = dictionary.as_aruco_name() {
        let Some(dict) = aruco_dictionary_from_name(dict_name) else {
            return Err(NodeError::InvalidInput(format!("unknown ArUco dictionary '{dict_name}'")));
        };
        let max_hamming_u32 = if max_hamming >= 0 { u32::try_from(max_hamming).unwrap_or(0) } else { u32::MAX };
        let include_bits = aruco_include_bits_enabled();
        let mut detections = refined;
        detections.retain(|det| !(max_hamming_u32 != u32::MAX && det.best_distance.is_some_and(|best| best > max_hamming_u32)));
        filter_detections_id_range_in_place(&mut detections, min_id, max_id);
        filter_detections_in_frame_in_place(&mut detections, frame_w, frame_h);
        if include_bits {
            for det in &mut detections {
                if det.bits.is_none() {
                    det.bits = dict.bit_grid(det.id as usize);
                }
            }
        }
        for det in &mut detections {
            det.canonicalize_in_place();
        }
        detections
    } else {
        let Some(family) = dictionary.as_apriltag_family() else {
            return Err(NodeError::InvalidInput("unknown tag dictionary".into()));
        };
        let family_impl = family.into_family();
        let include_bits = aruco_include_bits_enabled();
        let mut detections = refined;
        filter_detections_id_range_in_place(&mut detections, min_id, max_id);
        filter_detections_in_frame_in_place(&mut detections, frame_w, frame_h);
        for det in &mut detections {
            if include_bits && det.bits.is_none() {
                det.bits = family_impl.bit_grid(det.id as usize);
            }
            det.canonicalize_in_place();
        }
        detections
    };

    Ok(finalized)
}

fn roi_bounds_or_full(fw: u32, fh: u32, roi_x: i64, roi_y: i64, roi_w: i64, roi_h: i64) -> (u32, u32, u32, u32) {
    if fw == 0 || fh == 0 {
        return (0, 0, 0, 0);
    }
    if roi_w <= 0 || roi_h <= 0 {
        return (0, 0, fw, fh);
    }

    let x = roi_x.max(0);
    let y = roi_y.max(0);
    if x >= i64::from(fw) || y >= i64::from(fh) {
        return (0, 0, fw, fh);
    }

    let max_w = i64::from(fw) - x;
    let max_h = i64::from(fh) - y;
    if max_w <= 0 || max_h <= 0 {
        return (0, 0, fw, fh);
    }

    let w = roi_w.max(1).min(max_w) as u32;
    let h = roi_h.max(1).min(max_h) as u32;
    let x = x as u32;
    let y = y as u32;
    if x == 0 && y == 0 && w == fw && h == fh { (0, 0, fw, fh) } else { (x, y, w, h) }
}

#[node(
    id = "decode_quads_hamming_from_roi_frame",
    summary = "Decode quad candidates from an input frame plus ROI inputs.",
    inputs(
        port(name = "frame", source = "Frame", ty = daedalus::data::model::TypeExpr::opaque("image:dynamic")),
        port(name = "roi_x", default = 0i64, meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
        port(name = "roi_y", default = 0i64, meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
        port(name = "roi_w", default = 0i64, meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
        port(name = "roi_h", default = 0i64, meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
        port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()),
        port(name = "dictionary", default = "4x4_1000"),
        port(name = "sample_scale", default = 2i64, meta(ui_min = 1, ui_max = 8, ui_step = 1)),
        port(name = "max_hamming", default = -1i64, meta(ui_min = -1, ui_max = 4, ui_step = 1)),
        port(name = "border_error_divisor", default = -1i64, meta(ui_min = -1, ui_max = 50, ui_step = 1)),
        port(name = "refine_corners_warp", default = false),
        port(name = "refine_corners_warp_scale", default = 0i64, meta(ui_min = 0, ui_max = 8, ui_step = 1)),
        port(name = "min_id", default = -1i64, meta(ui_min = -1, ui_max = 10000, ui_step = 1)),
        port(name = "max_id", default = -1i64, meta(ui_min = -1, ui_max = 10000, ui_step = 1)),
        port(name = "min_warped_patch_contrast_range", default = 8i64, meta(ui_min = 0, ui_max = 255, ui_step = 1)),
        port(name = "warp_fallback_on_decode_fail", default = true),
        port(name = "warp_fallback_max_hamming_extra", default = 2i64, meta(ui_min = 0, ui_max = 32, ui_step = 1)),
        port(name = "warp_fallback_border_slack", default = 6i64, meta(ui_min = 0, ui_max = 32, ui_step = 1)),
        port(name = "warp_fallback_on_low_contrast", default = true),
        port(name = "warp_min_sample_scale", default = 4i64, meta(ui_min = 1, ui_max = 32, ui_step = 1)),
        port(name = "min_quad_side_px", default = 0.0f64, meta(ui_min = 0.0, ui_max = 200.0, ui_step = 0.5)),
        port(name = "min_quiet_zone_delta", default = 0.0f64, meta(ui_min = 0.0, ui_max = 64.0, ui_step = 0.5)),
        port(name = "quiet_zone_texture_penalty", default = 12.0f64, meta(ui_min = 0.0, ui_max = 64.0, ui_step = 0.5)),
        port(name = "verify_warp_min_best_distance", default = 99i64, meta(ui_min = 0, ui_max = 99, ui_step = 1)),
        port(name = "verify_warp_only_if_border_mismatch", default = true),
        port(name = "verify_warp_reject_on_fail", default = false),
        port(name = "min_decode_score", default = -1.0f64, meta(ui_min = -100.0, ui_max = 100.0, ui_step = 1.0)),
        port(name = "cell_sample_grid", default = 0i64, meta(ui_min = 0, ui_max = 7, ui_step = 1)),
        port(name = "cell_sample_margin", default = 0.25f64, meta(ui_min = 0.0, ui_max = 0.45, ui_step = 0.01)),
        port(name = "min_cell_means_contrast_range", default = 10.0f64, meta(ui_min = 0.0, ui_max = 255.0, ui_step = 1.0)),
        port(name = "min_hamming_margin", default = 2i64, meta(ui_min = 0, ui_max = 32, ui_step = 1)),
        port(name = "min_hamming_margin_min_dist", default = 1i64, meta(ui_min = 0, ui_max = 32, ui_step = 1)),
        port(name = "min_hamming_margin_only_if_border_mismatch", default = false),
        port(name = "min_bit_delta", default = 2.0f64, meta(ui_min = 0.0, ui_max = 255.0, ui_step = 1.0))
    ),
    outputs(port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()))
)]
#[allow(clippy::too_many_arguments)]
fn cv_aruco_decode_quads_hamming_from_roi_frame(
    frame: &DynamicImage,
    roi_x: i64,
    roi_y: i64,
    roi_w: i64,
    roi_h: i64,
    quads: std::sync::Arc<Vec<Quad>>,
    dictionary: ArucoDictionaryKind,
    sample_scale: i64,
    max_hamming: i64,
    border_error_divisor: i64,
    refine_corners_warp: bool,
    refine_corners_warp_scale: i64,
    min_id: i64,
    max_id: i64,
    min_warped_patch_contrast_range: i64,
    warp_fallback_on_decode_fail: bool,
    warp_fallback_max_hamming_extra: i64,
    warp_fallback_border_slack: i64,
    warp_fallback_on_low_contrast: bool,
    warp_min_sample_scale: i64,
    min_quad_side_px: f64,
    min_quiet_zone_delta: f64,
    quiet_zone_texture_penalty: f64,
    verify_warp_min_best_distance: i64,
    verify_warp_only_if_border_mismatch: bool,
    verify_warp_reject_on_fail: bool,
    min_decode_score: f64,
    cell_sample_grid: i64,
    cell_sample_margin: f64,
    min_cell_means_contrast_range: f64,
    min_hamming_margin: i64,
    min_hamming_margin_min_dist: i64,
    min_hamming_margin_only_if_border_mismatch: bool,
    min_bit_delta: f64,
    _exec_ctx: &ExecutionContext,
) -> Result<Vec<ArucoDetection2D>, NodeError> {
    let (fw, fh) = frame.dimensions();
    let (x, y, w, h) = roi_bounds_or_full(fw, fh, roi_x, roi_y, roi_w, roi_h);
    crate::modules::image::luma::with_cropped_luma8_frame(frame, x, y, w, h, |gray| {
        decode_quads_hamming_gray_inner(
            gray,
            quads.as_slice(),
            dictionary,
            sample_scale,
            max_hamming,
            border_error_divisor,
            refine_corners_warp,
            refine_corners_warp_scale,
            min_id,
            max_id,
            min_warped_patch_contrast_range,
            warp_fallback_on_decode_fail,
            warp_fallback_max_hamming_extra,
            warp_fallback_border_slack,
            warp_fallback_on_low_contrast,
            warp_min_sample_scale,
            min_quad_side_px,
            min_quiet_zone_delta,
            quiet_zone_texture_penalty,
            verify_warp_min_best_distance,
            verify_warp_only_if_border_mismatch,
            verify_warp_reject_on_fail,
            min_decode_score,
            cell_sample_grid,
            cell_sample_margin,
            min_cell_means_contrast_range,
            min_hamming_margin,
            min_hamming_margin_min_dist,
            min_hamming_margin_only_if_border_mismatch,
            min_bit_delta,
        )
    })
}
