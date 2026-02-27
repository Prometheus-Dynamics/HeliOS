use super::*;

#[node(
    id = "decode_quads",
    inputs(
        "frame",
        port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()),
        port(name = "dictionary", default = "apriltag_16h5"),
        port(name = "sample_scale", default = 2i64, meta(ui_min = 1, ui_max = 8, ui_step = 1)),
        config = ArucoTagDecodeTuningConfig
    ),
    outputs(port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()))
)]
fn cv_aruco_decode_quads(
    frame: Payload<DynamicImage>,
    quads: &Vec<Quad>,
    dictionary: ArucoDictionaryKind,
    sample_scale: i64,
    cfg: ArucoTagDecodeTuningConfig,
    exec_ctx: &ExecutionContext,
) -> Result<Vec<ArucoDetection2D>, NodeError> {
    let sample_scale = u32::try_from(sample_scale).unwrap_or(1).max(1);
    let frame = expect_cpu_frame(frame, "decode_quads", Some(exec_ctx))?;

    let cv_quads: Vec<[CvPoint<f32>; 4]> = quads
        .iter()
        .map(|q| [CvPoint::new(q[0].x as f32, q[0].y as f32), CvPoint::new(q[1].x as f32, q[1].y as f32), CvPoint::new(q[2].x as f32, q[2].y as f32), CvPoint::new(q[3].x as f32, q[3].y as f32)])
        .collect();

    if let Some(dict_name) = dictionary.as_aruco_name() {
        let Some(dict) = aruco_dictionary_from_name(dict_name) else {
            return Err(NodeError::InvalidInput(format!("unknown ArUco dictionary '{dict_name}'")));
        };
        let mut decode_cfg = decode_tuning_to_aruco_config(&cfg);
        if dict.marker_size() <= 4 {
            decode_cfg.min_warped_patch_contrast_range = decode_cfg.min_warped_patch_contrast_range.max(8);
            decode_cfg.min_cell_means_contrast_range = decode_cfg.min_cell_means_contrast_range.max(10.0);
            decode_cfg.min_quad_side_px = decode_cfg.min_quad_side_px.max(10.0);
            decode_cfg.max_border_error_rate = decode_cfg.max_border_error_rate.min(0.2);
            decode_cfg.error_correction_rate = decode_cfg.error_correction_rate.min(0.4);
        }
        let detections = decode_quads_aruco_with_config(&frame, &cv_quads, sample_scale, &dict, &decode_cfg);
        return Ok(detections);
    }

    let Some(family) = dictionary.as_apriltag_family() else {
        return Err(NodeError::InvalidInput("unknown tag dictionary".into()));
    };
    let family_impl = family.into_family();
    let decode_cfg = decode_tuning_to_config(&cfg);
    Ok(decode_quads_with_config(&frame, &cv_quads, sample_scale, &family_impl, &decode_cfg))
}

#[node(
    id = "decode_quads_warp",
        inputs(
            "frame",
            port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()),
            port(name = "dictionary", default = "apriltag_16h5"),
            port(name = "sample_scale", default = 2i64, meta(ui_min = 1, ui_max = 8, ui_step = 1)),
            config = ArucoTagDecodeTuningConfig
        ),
        outputs(port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()))
    )]
fn cv_aruco_decode_quads_warp(
    frame: Payload<DynamicImage>,
    quads: &Vec<Quad>,
    dictionary: ArucoDictionaryKind,
    sample_scale: i64,
    cfg: ArucoTagDecodeTuningConfig,
    exec_ctx: &ExecutionContext,
) -> Result<Vec<ArucoDetection2D>, NodeError> {
    let sample_scale = u32::try_from(sample_scale).unwrap_or(1).max(1);
    let frame = expect_cpu_frame(frame, "decode_quads_warp", Some(exec_ctx))?;

    let cv_quads: Vec<[CvPoint<f32>; 4]> = quads
        .iter()
        .map(|q| [CvPoint::new(q[0].x as f32, q[0].y as f32), CvPoint::new(q[1].x as f32, q[1].y as f32), CvPoint::new(q[2].x as f32, q[2].y as f32), CvPoint::new(q[3].x as f32, q[3].y as f32)])
        .collect();

    if let Some(dict_name) = dictionary.as_aruco_name() {
        let Some(dict) = aruco_dictionary_from_name(dict_name) else {
            return Err(NodeError::InvalidInput(format!("unknown ArUco dictionary '{dict_name}'")));
        };
        let decode_cfg = decode_tuning_to_aruco_config(&cfg);
        let detections = decode_quads_aruco_with_config(&frame, &cv_quads, sample_scale, &dict, &decode_cfg);
        return Ok(detections);
    }

    let Some(family) = dictionary.as_apriltag_family() else {
        return Err(NodeError::InvalidInput("unknown tag dictionary".into()));
    };
    let family_impl = family.into_family();
    let decode_cfg = decode_tuning_to_config(&cfg);
    Ok(decode_quads_warp_with_config(&frame, &cv_quads, sample_scale, &family_impl, &decode_cfg))
}

#[derive(Clone, Debug, NodeConfig)]
struct ArucoTagDecodeQuadsCalibratedConfig {
    #[port(default = "4x4_1000")]
    dictionary: ArucoDictionaryKind,
    #[port(default = 2i64, meta(ui_min = 1, ui_max = 8, ui_step = 1))]
    sample_scale: i64,
    // Set to 0 to require perfect code matches (reduces false positives).
    #[port(default = -1i64, meta(ui_min = -1, ui_max = 4, ui_step = 1))]
    max_hamming: i64,
    // Higher divisor => stricter border check (fewer false positives).
    #[port(default = -1i64, meta(ui_min = -1, ui_max = 50, ui_step = 1))]
    border_error_divisor: i64,
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 5000.0, ui_step = 1.0))]
    fx: f64,
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 5000.0, ui_step = 1.0))]
    fy: f64,
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 5000.0, ui_step = 1.0))]
    cx: f64,
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 5000.0, ui_step = 1.0))]
    cy: f64,
    #[port(default = 0.0f64, meta(ui_min = -1.0, ui_max = 1.0, ui_step = 0.001))]
    k1: f64,
    #[port(default = 0.0f64, meta(ui_min = -1.0, ui_max = 1.0, ui_step = 0.001))]
    k2: f64,
    #[port(default = 0.0f64, meta(ui_min = -1.0, ui_max = 1.0, ui_step = 0.001))]
    p1: f64,
    #[port(default = 0.0f64, meta(ui_min = -1.0, ui_max = 1.0, ui_step = 0.001))]
    p2: f64,
    #[port(default = 0.0f64, meta(ui_min = -1.0, ui_max = 1.0, ui_step = 0.001))]
    k3: f64,
    #[port(default = "pinhole")]
    lens_model: LensModel,
    #[port(default = 5i64, meta(ui_min = 0, ui_max = 10, ui_step = 1))]
    undistort_iters: i64,
    #[port(default = 8i64, meta(ui_min = 0, ui_max = 255, ui_step = 1))]
    min_warped_patch_contrast_range: i64,
    #[port(default = true)]
    warp_fallback_on_decode_fail: bool,
    #[port(default = 2i64, meta(ui_min = 0, ui_max = 32, ui_step = 1))]
    warp_fallback_max_hamming_extra: i64,
    #[port(default = 6i64, meta(ui_min = 0, ui_max = 32, ui_step = 1))]
    warp_fallback_border_slack: i64,
    #[port(default = true)]
    warp_fallback_on_low_contrast: bool,
    #[port(default = 4i64, meta(ui_min = 1, ui_max = 32, ui_step = 1))]
    warp_min_sample_scale: i64,
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 200.0, ui_step = 0.5))]
    min_quad_side_px: f64,
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 64.0, ui_step = 0.5))]
    min_quiet_zone_delta: f64,
    #[port(default = 12.0f64, meta(ui_min = 0.0, ui_max = 64.0, ui_step = 0.5))]
    quiet_zone_texture_penalty: f64,
    #[port(default = 99i64, meta(ui_min = 0, ui_max = 99, ui_step = 1))]
    verify_warp_min_best_distance: i64,
    #[port(default = true)]
    verify_warp_only_if_border_mismatch: bool,
    #[port(default = false)]
    verify_warp_reject_on_fail: bool,
    #[port(default = -1.0f64, meta(ui_min = -100.0, ui_max = 100.0, ui_step = 1.0))]
    min_decode_score: f64,
    #[port(default = 0i64, meta(ui_min = 0, ui_max = 7, ui_step = 1))]
    cell_sample_grid: i64,
    #[port(default = 0.25f64, meta(ui_min = 0.0, ui_max = 0.45, ui_step = 0.01))]
    cell_sample_margin: f64,
    #[port(default = 20.0f64, meta(ui_min = 0.0, ui_max = 255.0, ui_step = 1.0))]
    min_cell_means_contrast_range: f64,
    #[port(default = 0i64, meta(ui_min = 0, ui_max = 32, ui_step = 1))]
    min_hamming_margin: i64,
    #[port(default = 0i64, meta(ui_min = 0, ui_max = 32, ui_step = 1))]
    min_hamming_margin_min_dist: i64,
    #[port(default = false)]
    min_hamming_margin_only_if_border_mismatch: bool,
    #[port(default = 6.0f64, meta(ui_min = 0.0, ui_max = 255.0, ui_step = 1.0))]
    min_bit_delta: f64,
    #[port(default = -1i64, meta(ui_min = -1, ui_max = 10000, ui_step = 1))]
    min_id: i64,
    #[port(default = -1i64, meta(ui_min = -1, ui_max = 10000, ui_step = 1))]
    max_id: i64,
}

fn decode_tuning_from_calibrated(cfg: &ArucoTagDecodeQuadsCalibratedConfig) -> ArucoTagDecodeConfig {
    ArucoTagDecodeConfig {
        min_warped_patch_contrast_range: u8::try_from(cfg.min_warped_patch_contrast_range.clamp(0, 255)).unwrap_or(8),
        warp_fallback_on_decode_fail: cfg.warp_fallback_on_decode_fail,
        warp_fallback_max_hamming_extra: u32::try_from(cfg.warp_fallback_max_hamming_extra.max(0)).unwrap_or(2),
        warp_fallback_border_slack: usize::try_from(cfg.warp_fallback_border_slack.max(0)).unwrap_or(6),
        warp_fallback_on_low_contrast: cfg.warp_fallback_on_low_contrast,
        warp_min_sample_scale: u32::try_from(cfg.warp_min_sample_scale.clamp(1, 32)).unwrap_or(4),
        min_quad_side_px: cfg.min_quad_side_px.max(0.0) as f32,
        min_quiet_zone_delta: cfg.min_quiet_zone_delta.max(0.0) as f32,
        quiet_zone_texture_penalty: cfg.quiet_zone_texture_penalty.max(0.0) as f32,
        verify_warp_min_best_distance: u32::try_from(cfg.verify_warp_min_best_distance.max(0)).unwrap_or(99),
        verify_warp_only_if_border_mismatch: cfg.verify_warp_only_if_border_mismatch,
        verify_warp_reject_on_fail: cfg.verify_warp_reject_on_fail,
        min_decode_score: cfg.min_decode_score as f32,
        cell_sample_grid: u8::try_from(cfg.cell_sample_grid.clamp(0, 7)).unwrap_or(0),
        cell_sample_margin: cfg.cell_sample_margin.clamp(0.0, 0.45) as f32,
        cell_decode: crate::modules::aruco::tag::ArucoTagDecodeTuning {
            min_cell_means_contrast_range: cfg.min_cell_means_contrast_range.clamp(0.0, 255.0) as f32,
            min_hamming_margin: u32::try_from(cfg.min_hamming_margin.max(0)).unwrap_or(0).min(32),
            min_hamming_margin_min_dist: u32::try_from(cfg.min_hamming_margin_min_dist.max(0)).unwrap_or(0).min(32),
            min_hamming_margin_only_if_border_mismatch: cfg.min_hamming_margin_only_if_border_mismatch,
            min_bit_delta: cfg.min_bit_delta.clamp(0.0, 255.0) as f32,
        },
    }
}

fn decode_tuning_from_calibrated_aruco(cfg: &ArucoTagDecodeQuadsCalibratedConfig) -> ArucoDecodeConfig {
    let mut out = ArucoDecodeConfig::default();
    out.min_warped_patch_contrast_range = u8::try_from(cfg.min_warped_patch_contrast_range.clamp(0, 255)).unwrap_or(out.min_warped_patch_contrast_range);
    out.min_cell_means_contrast_range = cfg.min_cell_means_contrast_range.clamp(0.0, 255.0) as f32;
    out.min_quad_side_px = cfg.min_quad_side_px.max(0.0) as f32;
    out.min_quiet_zone_delta = cfg.min_quiet_zone_delta.max(0.0) as f32;
    out.cell_sample_grid = u8::try_from(cfg.cell_sample_grid.clamp(0, 7)).unwrap_or(out.cell_sample_grid);
    out.cell_sample_margin = cfg.cell_sample_margin.clamp(0.0, 0.45) as f32;
    out.min_decode_score = cfg.min_decode_score as f32;
    out.min_hamming_margin = u32::try_from(cfg.min_hamming_margin.max(0)).unwrap_or(0).min(32);
    out.min_hamming_margin_min_dist = u32::try_from(cfg.min_hamming_margin_min_dist.max(0)).unwrap_or(0).min(32);
    out.min_hamming_margin_only_if_border_mismatch = cfg.min_hamming_margin_only_if_border_mismatch;
    out.min_bit_delta = cfg.min_bit_delta.clamp(0.0, 255.0) as f32;
    out
}

#[node(
    id = "decode_quads_calibrated",
    inputs(
        "frame",
        port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()),
        config = ArucoTagDecodeQuadsCalibratedConfig
    ),
    outputs(port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()))
)]
fn cv_aruco_decode_quads_calibrated(
    frame: Payload<DynamicImage>,
    quads: &Vec<Quad>,
    cfg: ArucoTagDecodeQuadsCalibratedConfig,
    exec_ctx: &ExecutionContext,
) -> Result<Vec<ArucoDetection2D>, NodeError> {
    let sample_scale = u32::try_from(cfg.sample_scale).unwrap_or(1).max(1);
    let frame = expect_cpu_frame(frame, "decode_quads_calibrated", Some(exec_ctx))?;

    let cv_quads: Vec<[CvPoint<f32>; 4]> = quads
        .iter()
        .map(|q| [CvPoint::new(q[0].x as f32, q[0].y as f32), CvPoint::new(q[1].x as f32, q[1].y as f32), CvPoint::new(q[2].x as f32, q[2].y as f32), CvPoint::new(q[3].x as f32, q[3].y as f32)])
        .collect();

    let calib = CameraCalibration {
        fx: cfg.fx as f32,
        fy: cfg.fy as f32,
        cx: cfg.cx as f32,
        cy: cfg.cy as f32,
        k1: cfg.k1 as f32,
        k2: cfg.k2 as f32,
        p1: cfg.p1 as f32,
        p2: cfg.p2 as f32,
        k3: cfg.k3 as f32,
        undistort_iters: u8::try_from(cfg.undistort_iters).unwrap_or(5).max(1),
        lens_model: cfg.lens_model,
    };

    if let Some(dict_name) = cfg.dictionary.as_aruco_name() {
        let Some(dict) = aruco_dictionary_from_name(dict_name) else {
            return Err(NodeError::InvalidInput(format!("unknown ArUco dictionary '{dict_name}'")));
        };
        let mut decode_cfg = decode_tuning_from_calibrated_aruco(&cfg);
        if cfg.max_hamming >= 0 {
            let max_hamming = u32::try_from(cfg.max_hamming).unwrap_or(0);
            let max_corr = dict.max_correction_bits() as u32;
            decode_cfg.error_correction_rate = if max_corr == 0 { 0.0 } else { (max_hamming as f32 / max_corr as f32).clamp(0.0, 1.0) };
        }
        if cfg.border_error_divisor > 0 {
            let div = (cfg.border_error_divisor as f32).max(1.0);
            decode_cfg.max_border_error_rate = decode_cfg.max_border_error_rate.min((1.0 / div).clamp(0.0, 1.0));
            decode_cfg.min_hamming_margin_only_if_border_mismatch = false;
        }
        let detections = decode_quads_aruco_calibrated_with_config(&frame, &cv_quads, sample_scale, &dict, calib, &decode_cfg);
        return Ok(filter_detections_id_range(&detections, cfg.min_id, cfg.max_id));
    }

    let Some(family) = cfg.dictionary.as_apriltag_family() else {
        return Err(NodeError::InvalidInput("unknown tag dictionary".into()));
    };
    let mut family_impl = family.into_family();
    if cfg.max_hamming >= 0 {
        family_impl = family_impl.with_max_hamming(u8::try_from(cfg.max_hamming).unwrap_or(0));
    }
    if cfg.border_error_divisor > 0 {
        family_impl = family_impl.with_border_error_divisor(u8::try_from(cfg.border_error_divisor).unwrap_or(10));
    }
    let decode_cfg = decode_tuning_from_calibrated(&cfg);
    let detections = decode_quads_calibrated_with_config(&frame, &cv_quads, sample_scale, &family_impl, calib, &decode_cfg);
    Ok(filter_detections_id_range(&detections, cfg.min_id, cfg.max_id))
}
