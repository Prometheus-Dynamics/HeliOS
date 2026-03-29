use super::*;

#[derive(Clone, Debug, NodeConfig)]
struct ArucoTagOverlayConfig {
    #[port(default = "apriltag_16h5")]
    dictionary: ArucoDictionaryKind,
    // Override max Hamming distance accepted by the family decoder.
    // A negative value uses the family default.
    #[port(default = -1i64, meta(ui_min = -1, ui_max = 4, ui_step = 1))]
    max_hamming: i64,
    #[port(default = 1i64, meta(ui_min = 1, ui_max = 8, ui_step = 1))]
    downscale: i64,
    #[port(default = 1i64, meta(ui_min = 1, ui_max = 8, ui_step = 1))]
    sample_scale: i64,
    #[port(default = 31i64, meta(ui_min = 3, ui_max = 101, ui_step = 2))]
    threshold_window: i64,
    #[port(default = 10.0f64, meta(ui_min = -50.0, ui_max = 50.0, ui_step = 1.0))]
    threshold_offset: f64,
    // Threshold mode: "adaptive_mean" (default) or "otsu" (fast, lighting-dependent).
    #[port(default = "adaptive_mean")]
    threshold_mode: crate::modules::aruco::ArucoMaskMode,
    #[port(default = true)]
    invert_mask: bool,
    // Gamma correction applied to grayscale before thresholding/decoding. <1 brightens shadows.
    #[port(default = 1.0f64, meta(ui_min = 0.1, ui_max = 5.0, ui_step = 0.1))]
    gamma: f64,
    // Optional blur to reduce mask noise (set to 0 for speed).
    #[port(default = 1.0f64, meta(ui_min = 0.0, ui_max = 10.0, ui_step = 0.1))]
    blur_sigma: f64,
    // For robustness we can try both polarities and keep whichever yields more markers.
    // Disable for speed if lighting is stable.
    #[port(default = true)]
    try_opposite_polarity: bool,
    // For throughput tuning, allow skipping per-tag overlay drawing.
    #[port(default = true)]
    draw_markers: bool,
    // Control the on-frame "TAGS: N" HUD overlay.
    #[port(default = true)]
    draw_hud: bool,
    // Run the full (expensive) detection every N frames and reuse the last successful
    // detections in-between. Set to 1 to disable caching.
    #[port(default = 1i64, meta(ui_min = 1, ui_max = 120, ui_step = 1))]
    detect_every_n: i64,
    #[port(default = 0.03f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01))]
    epsilon: f64,
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 180.0, ui_step = 1.0))]
    min_angle_deg: f64,
    #[port(default = 180.0f64, meta(ui_min = 0.0, ui_max = 180.0, ui_step = 1.0))]
    max_angle_deg: f64,
    #[port(default = 10.0f64, meta(ui_min = 0.1, ui_max = 10.0, ui_step = 0.1))]
    max_side_cv: f64,
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 1000000.0, ui_step = 1000.0))]
    min_area: f64,
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 1000000.0, ui_step = 1000.0))]
    max_area: f64,
}

fn overlay_count_label(image: &mut DynamicImage, label: &str, count: usize) {
    const HUD_W: u32 = 240;
    const HUD_H: u32 = 72;
    const PAD_X: u32 = 8;
    const PAD_Y: u32 = 8;
    const SCALE: u32 = 3;

    fn buf_len_ok(len: usize, width: usize, height: usize, channels: usize) -> bool {
        width.checked_mul(height).and_then(|pixels| pixels.checked_mul(channels)).map(|expected| len >= expected).unwrap_or(false)
    }

    fn glyph_rows(c: char) -> [u8; 7] {
        match c {
            'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
            'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
            'D' => [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
            'G' => [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110],
            'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
            'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
            'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
            ':' => [0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000],
            ' ' => [0, 0, 0, 0, 0, 0, 0],
            '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
            '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
            '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
            '3' => [0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110],
            '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
            '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
            '6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
            '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
            '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
            '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
            _ => [0, 0, 0, 0, 0, 0, 0],
        }
    }

    fn draw_hud_luma(dst: &mut image::GrayImage, text: &str) {
        let w = dst.width().min(HUD_W) as usize;
        let h = dst.height().min(HUD_H) as usize;
        let dst_w = dst.width() as usize;
        let dst_h = dst.height() as usize;
        let stride = dst_w;
        let buf = dst.as_mut();
        if !buf_len_ok(buf.len(), dst_w, dst_h, 1) {
            return;
        }
        for y in 0..h {
            let start = y * stride;
            buf[start..start + w].fill(0);
        }

        let mut x0 = PAD_X as usize;
        let y0 = PAD_Y as usize;
        for ch in text.chars() {
            let rows = glyph_rows(ch);
            for (gy, row) in rows.iter().enumerate() {
                for gx in 0..5usize {
                    if ((row >> (4 - gx)) & 1) == 0 {
                        continue;
                    }
                    for sy in 0..SCALE as usize {
                        let py = y0 + gy * (SCALE as usize) + sy;
                        if py >= dst_h {
                            continue;
                        }
                        let base = py * stride;
                        for sx in 0..SCALE as usize {
                            let px = x0 + gx * (SCALE as usize) + sx;
                            if px < dst_w {
                                buf[base + px] = 255;
                            }
                        }
                    }
                }
            }
            x0 += 6 * SCALE as usize;
        }
    }

    fn draw_hud_rgba(dst: &mut image::RgbaImage, text: &str) {
        let w = dst.width().min(HUD_W) as usize;
        let h = dst.height().min(HUD_H) as usize;
        let dst_w = dst.width() as usize;
        let dst_h = dst.height() as usize;
        let stride_px = dst_w;
        let stride = stride_px.saturating_mul(4);
        let buf = dst.as_mut();
        if !buf_len_ok(buf.len(), dst_w, dst_h, 4) {
            return;
        }
        for y in 0..h {
            let row = &mut buf[y * stride..y * stride + w * 4];
            for px in row.chunks_exact_mut(4) {
                px[0] = 0;
                px[1] = 0;
                px[2] = 0;
                px[3] = 255;
            }
        }

        let mut x0 = PAD_X as usize;
        let y0 = PAD_Y as usize;
        for ch in text.chars() {
            let rows = glyph_rows(ch);
            for (gy, row) in rows.iter().enumerate() {
                for gx in 0..5usize {
                    if ((row >> (4 - gx)) & 1) == 0 {
                        continue;
                    }
                    for sy in 0..SCALE as usize {
                        let py = y0 + gy * (SCALE as usize) + sy;
                        if py >= dst_h {
                            continue;
                        }
                        let base = py * stride;
                        for sx in 0..SCALE as usize {
                            let px = x0 + gx * (SCALE as usize) + sx;
                            if px < dst_w {
                                let off = base + px * 4;
                                buf[off] = 255;
                                buf[off + 1] = 255;
                                buf[off + 2] = 255;
                                buf[off + 3] = 255;
                            }
                        }
                    }
                }
            }
            x0 += 6 * SCALE as usize;
        }
    }

    fn draw_hud_rgb(dst: &mut RgbImage, text: &str) {
        let w = dst.width().min(HUD_W) as usize;
        let h = dst.height().min(HUD_H) as usize;
        let dst_w = dst.width() as usize;
        let dst_h = dst.height() as usize;
        let stride = dst_w.saturating_mul(3);
        let buf = dst.as_mut();
        if !buf_len_ok(buf.len(), dst_w, dst_h, 3) {
            return;
        }
        for y in 0..h {
            let row = &mut buf[y * stride..y * stride + w * 3];
            row.fill(0);
        }

        let mut x0 = PAD_X as usize;
        let y0 = PAD_Y as usize;
        for ch in text.chars() {
            let rows = glyph_rows(ch);
            for (gy, row) in rows.iter().enumerate() {
                for gx in 0..5usize {
                    if ((row >> (4 - gx)) & 1) == 0 {
                        continue;
                    }
                    for sy in 0..SCALE as usize {
                        let py = y0 + gy * (SCALE as usize) + sy;
                        if py >= dst_h {
                            continue;
                        }
                        let base = py * stride;
                        for sx in 0..SCALE as usize {
                            let px = x0 + gx * (SCALE as usize) + sx;
                            if px < dst_w {
                                let off = base + px * 3;
                                buf[off] = 255;
                                buf[off + 1] = 255;
                                buf[off + 2] = 255;
                            }
                        }
                    }
                }
            }
            x0 += 6 * SCALE as usize;
        }
    }

    let text = format!("{label}: {count}");
    match image {
        DynamicImage::ImageLuma8(gray) => draw_hud_luma(gray, &text),
        DynamicImage::ImageRgb8(rgb) => draw_hud_rgb(rgb, &text),
        DynamicImage::ImageRgba8(rgba) => draw_hud_rgba(rgba, &text),
        _ => {
            let mut rgba = image.to_rgba8();
            draw_hud_rgba(&mut rgba, &text);
            *image = DynamicImage::ImageRgba8(rgba);
        }
    }
}

fn overlay_tags_count(image: &mut DynamicImage, count: usize) {
    // Keep this stable and OCR-friendly for `tools/daedalus_tag_sweep.py`.
    // The python tool crops `img[0:90, 0:240]` and expects "TAGS: <n>".
    //
    // Avoid using the general font/text renderer here because it can trigger expensive
    // full-frame colorspace conversions (especially when the camera is decoded as GREY).
    // This small bitmap font draws directly into the existing buffer.

    overlay_count_label(image, "TAGS", count);
}

fn overlay_id_label_centered(image: &mut DynamicImage, id: u32, center_x: i64, y: i64) {
    const OVERLAY_ID_CACHE_MAX: usize = 128;
    OVERLAY_ID_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        // Guard against unbounded growth from noisy/false IDs over long runs.
        if cache.len() >= OVERLAY_ID_CACHE_MAX && !cache.contains_key(&id) {
            cache.clear();
        }
        {
            cache.entry(id).or_insert_with(|| {
                let text = id.to_string();
                let width = (text.len() as u32 * 14).max(14);
                let height = 14 + 5;
                let mut img = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 0]));
                draw::text::draw_text_x_y(&mut img, text.as_str(), (0u32, 0u32), &draw::font::FontType::SavedByZero, 14, Rgba([0, 255, 0, 255]));
                img
            });
        }
        let total = cache.values().map(|image| image.as_raw().capacity() * std::mem::size_of::<u8>()).sum::<usize>();
        report_overlay_id_cache_bytes(total);
        let entry = cache.get(&id).expect("overlay id cache entry inserted");
        let x = center_x - (entry.width() as i64 / 2);
        draw::utils::overlay_rgba_image(image, entry, x, y);
    });
}

fn overlay_detection_crosshair(image: &mut DynamicImage, cx: i64, cy: i64, size_px: i64, thickness: u32, color: Rgba<u8>) {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return;
    }
    let cx = cx.clamp(0, i64::from(width.saturating_sub(1))) as u32;
    let cy = cy.clamp(0, i64::from(height.saturating_sub(1))) as u32;
    let arm = (size_px.max(2) / 2) as u32;
    draw::line::overlay_crosshair_x_y(image, (cx, cy), arm, thickness.max(1), color);
}

#[derive(Clone, Debug, NodeConfig)]
struct ArucoOverlayDetectionsConfig {
    #[port(default = 3i64, meta(ui_min = 1, ui_max = 16, ui_step = 1))]
    thickness: i64,
    #[port(default = true)]
    draw_boxes: bool,
    #[port(default = true)]
    draw_corners: bool,
    #[port(default = true)]
    draw_ids: bool,
    #[port(default = true)]
    draw_hud: bool,
    #[port(default = false)]
    draw_crosshair: bool,
    #[port(default = 0.25f64, meta(ui_min = 0.05, ui_max = 1.5, ui_step = 0.05))]
    crosshair_scale: f64,
    #[port(default = 0i64, meta(ui_min = 0, ui_max = 16, ui_step = 1))]
    crosshair_thickness: i64,
}

#[node(
    id = "overlay_detections",
    inputs(
        port(name = "frame", ty = TypeExpr::opaque("image:dynamic")),
        port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()),
        config = ArucoOverlayDetectionsConfig
    ),
    outputs(port(name = "frame", ty = TypeExpr::opaque("image:dynamic")))
)]
fn cv_aruco_overlay(
    frame: Compute<DynamicImage>,
    detections: Option<std::sync::Arc<Vec<ArucoDetection2D>>>,
    cfg: ArucoOverlayDetectionsConfig,
    exec_ctx: &ExecutionContext,
) -> Result<Compute<DynamicImage>, NodeError> {
    let thickness = u32::try_from(cfg.thickness).unwrap_or(3).clamp(1, 32);
    let detections = detections.as_deref().map(Vec::as_slice).unwrap_or(&[]);
    if (!cfg.draw_boxes && !cfg.draw_corners && !cfg.draw_ids && !cfg.draw_hud && !cfg.draw_crosshair) || (detections.is_empty() && !cfg.draw_hud) {
        return Ok(frame);
    }
    let mut out = expect_cpu_frame(frame, "overlay_detections", Some(exec_ctx))?;
    let preserve_luma_output = matches!(out, DynamicImage::ImageLuma8(_) | DynamicImage::ImageLumaA8(_));
    let crosshair_scale = cfg.crosshair_scale.clamp(0.05, 1.5);
    let crosshair_thickness = if cfg.crosshair_thickness <= 0 { thickness } else { u32::try_from(cfg.crosshair_thickness).unwrap_or(thickness).clamp(1, 32) };

    for det in detections {
        let corners = det.corners;
        let bbox = [
            CvPoint::new(corners[0].x as f32, corners[0].y as f32),
            CvPoint::new(corners[1].x as f32, corners[1].y as f32),
            CvPoint::new(corners[2].x as f32, corners[2].y as f32),
            CvPoint::new(corners[3].x as f32, corners[3].y as f32),
        ];
        if cfg.draw_boxes {
            draw::contour::overlay_contour_points(&mut out, &bbox, thickness, Rgba([0, 255, 255, 255]));
        }

        if cfg.draw_crosshair {
            let min_x = corners.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
            let max_x = corners.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
            let min_y = corners.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
            let max_y = corners.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);
            if min_x.is_finite() && max_x.is_finite() && min_y.is_finite() && max_y.is_finite() {
                let bbox_w = (max_x - min_x).abs().max(1.0);
                let bbox_h = (max_y - min_y).abs().max(1.0);
                let base = bbox_w.min(bbox_h).max(2.0);
                let size = (base * crosshair_scale).round().max(2.0) as i64;
                let cx = ((min_x + max_x) * 0.5).round() as i64;
                let cy = ((min_y + max_y) * 0.5).round() as i64;
                overlay_detection_crosshair(&mut out, cx, cy, size, crosshair_thickness, Rgba([0, 255, 0, 255]));
            }
        }

        let dot_radius = (thickness + 2).clamp(3, 16);
        let mut max_y = 0.0f64;
        let mut center_x = 0i64;
        if cfg.draw_ids {
            let mut sum_x = 0.0f64;
            max_y = corners[0].y;
            for c in &corners {
                max_y = max_y.max(c.y);
                sum_x += c.x;
            }
            center_x = (sum_x / 4.0).round() as i64;
        }

        if cfg.draw_corners {
            for corner in corners {
                draw::shape::overlay_circle(&mut out, (corner.x.max(0.0) as u32, corner.y.max(0.0) as u32), dot_radius, true, Rgba([255, 64, 0, 255]));
            }
        }

        if cfg.draw_ids {
            let label_y = (max_y.round() as i64 + (dot_radius as i64) + 4).max(0);
            overlay_id_label_centered(&mut out, det.id, center_x.max(0), label_y);
        }
    }

    if cfg.draw_hud {
        overlay_tags_count(&mut out, detections.len());
    }

    if preserve_luma_output && !matches!(out, DynamicImage::ImageLuma8(_)) {
        out = DynamicImage::ImageLuma8(out.to_luma8());
    }

    Ok(Compute::Cpu(out))
}

#[node(
    id = "overlay_quads",
    inputs(
        "frame",
        port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()),
        port(name = "thickness", default = 2i64, meta(ui_min = 1, ui_max = 16, ui_step = 1)),
        port(name = "max_quads", default = 128i64, meta(ui_min = 1, ui_max = 1024, ui_step = 1))
    ),
    outputs(port(name = "frame", ty = TypeExpr::opaque("image:dynamic")))
)]
fn cv_aruco_overlay_quads(frame: Compute<DynamicImage>, quads: &Vec<Quad>, thickness: i64, max_quads: i64, exec_ctx: &ExecutionContext) -> Result<Compute<DynamicImage>, NodeError> {
    let thickness = u32::try_from(thickness).unwrap_or(2).clamp(1, 32);
    let max_quads = usize::try_from(max_quads).unwrap_or(128).clamp(1, 4096);
    let mut out = expect_cpu_frame(frame, "overlay_quads", Some(exec_ctx))?;

    for quad in quads.iter().take(max_quads) {
        let bbox = [
            CvPoint::new(quad[0].x as f32, quad[0].y as f32),
            CvPoint::new(quad[1].x as f32, quad[1].y as f32),
            CvPoint::new(quad[2].x as f32, quad[2].y as f32),
            CvPoint::new(quad[3].x as f32, quad[3].y as f32),
        ];
        draw::contour::overlay_contour_points(&mut out, &bbox, thickness, Rgba([255, 0, 255, 255]));
    }

    Ok(Compute::Cpu(out))
}

#[node(
    id = "overlay_quads_count",
    inputs(
        "frame",
        port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads())
    ),
    outputs(port(name = "frame", ty = TypeExpr::opaque("image:dynamic")))
)]
fn cv_aruco_overlay_quads_count(frame: Compute<DynamicImage>, quads: &Vec<Quad>, exec_ctx: &ExecutionContext) -> Result<Compute<DynamicImage>, NodeError> {
    let mut out = expect_cpu_frame(frame, "overlay_quads_count", Some(exec_ctx))?;

    overlay_count_label(&mut out, "QUADS", quads.len());

    Ok(Compute::Cpu(out))
}

#[node(
    id = "overlay_tags_count",
    inputs(
        "frame",
        port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d())
    ),
    outputs(port(name = "frame", ty = TypeExpr::opaque("image:dynamic")))
)]
fn cv_overlay_tags_count(frame: Compute<DynamicImage>, detections: &Vec<ArucoDetection2D>, exec_ctx: &ExecutionContext) -> Result<Compute<DynamicImage>, NodeError> {
    let _ = exec_ctx;
    match frame {
        Compute::Cpu(mut out) => {
            if !detections.is_empty() {
                overlay_tags_count(&mut out, detections.len());
            }
            Ok(Compute::Cpu(out))
        }
        Compute::Gpu(handle) => Ok(Compute::Gpu(handle)),
    }
}

#[node(
        id = "detect_overlay",
        inputs("frame", config = ArucoTagOverlayConfig),
        outputs(port(name = "frame", ty = TypeExpr::opaque("image:dynamic")))
		    )]
fn cv_detect_aruco_overlay(frame: Compute<DynamicImage>, cfg: ArucoTagOverlayConfig, exec_ctx: &ExecutionContext) -> Result<Compute<DynamicImage>, NodeError> {
    static CALLS: AtomicU64 = AtomicU64::new(0);
    let call_idx = CALLS.fetch_add(1, Ordering::Relaxed);
    if call_idx < 3 {
        let (w, h) = frame.dimensions();
        tracing::info!(call_idx, w, h, "cv:aruco:detect_overlay received frame");
    }

    #[derive(Default)]
    struct Cached {
        markers: Option<Arc<Vec<ArucoDetection2D>>>,
    }
    static CACHE: LazyLock<Mutex<Cached>> = LazyLock::new(|| Mutex::new(Cached::default()));

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

    // Hard requirements for this pipeline: full-res and no frame skipping.
    if downscale != 1 {
        return Err(NodeError::InvalidInput("downscale must be 1 (full-res required)".into()));
    }
    if detect_every_n != 1 {
        return Err(NodeError::InvalidInput("detect_every_n must be 1 (no frame skipping)".into()));
    }

    let do_profile = call_idx.is_multiple_of(120) && tracing::level_enabled!(tracing::Level::INFO);
    let t_total = do_profile.then(Instant::now);

    let mut out = expect_cpu_frame(frame, "aruco_detect_overlay", Some(exec_ctx))?;

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

    // Fast path: reuse cached detections to avoid the expensive mask/contour work.
    if detect_every_n > 1 && !call_idx.is_multiple_of(detect_every_n as u64) {
        let cached = CACHE.lock().ok().and_then(|c| c.markers.clone());
        if let Some(markers) = cached {
            if cfg.draw_markers {
                for marker in markers.iter() {
                    draw::aruco::overlay_marker(&mut out, marker, Rgba([0, 255, 0, 255]));
                }
            }
            if cfg.draw_hud {
                overlay_tags_count(&mut out, markers.len());
            }
            return Ok(Compute::Cpu(out));
        }
    }

    let processing = &out;
    let (pw, ph) = processing.dimensions();

    let detect_cfg = {
        // Contours are extracted from the downscaled mask; scale area thresholds accordingly.
        let scale = downscale as f32;
        let area_scale = (scale * scale).max(1.0);
        ArucoTagDetectorConfig {
            epsilon: cfg.epsilon.clamp(0.01, 1000.0) as f32,
            min_area: (min_area / area_scale).max(0.0),
            max_area: if max_area > 0.0 { Some((max_area / area_scale).max(0.0)) } else { None },
            // Keep the main pass strict for speed; use a fallback pass (below) for extreme
            // perspective when needed.
            min_angle_deg: cfg.min_angle_deg.clamp(0.0, 180.0) as f32,
            max_angle_deg: cfg.max_angle_deg.clamp(0.0, 180.0) as f32,
            max_side_cv,
            max_side_ratio: 0.0,
            max_diag_ratio: 0.0,
            angle_cos_min: -1.0,
            angle_cos_max: 1.0,
        }
        .with_angle_cos_bounds()
    };

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
        return Ok(Compute::Cpu(out));
    }

    #[derive(Default, Debug, Clone, Copy)]
    struct DetectStats {
        threshold_time: Duration,
        contour_time: Duration,
        candidate_time: Duration,
        decode_time: Duration,
        contours: usize,
        quads: usize,
        markers: usize,
    }

    let mut stats_preferred = DetectStats::default();
    let mut stats_opposite = DetectStats::default();

    let detect_once = |mask: &GrayImage, cfg: &ArucoTagDetectorConfig, mut stats: Option<&mut DetectStats>| -> Vec<ArucoDetection2D> {
        // Use the i32 Suzuki–Abe pass to avoid per-point `NumCast` overhead. Convert to f32
        // only for contours that pass a cheap bounding-box area check. This reduces allocations
        // substantially on noisy masks and improves throughput.
        let t_contours = do_profile.then(Instant::now);
        let contours = crate::contour::suzuki_abe::suzuki_abe_i32(mask);
        let contours_len = contours.len();
        let mut contour_points: Vec<Vec<imageproc::point::Point<f32>>> = Vec::with_capacity(contours.len());
        let min_area = cfg.min_area.max(0.0);
        let max_area = cfg.max_area.unwrap_or(f32::INFINITY);
        for c in contours {
            let pts = c.points;
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
            let mut scaled: Vec<imageproc::point::Point<f32>> = Vec::with_capacity(pts.len());
            for p in pts {
                scaled.push(imageproc::point::Point::new(p.x as f32, p.y as f32));
            }
            contour_points.push(scaled);
        }

        if let Some(start) = t_contours
            && let Some(s) = stats.as_deref_mut()
        {
            s.contour_time = start.elapsed();
            s.contours = contours_len;
        }

        let t_candidates = do_profile.then(Instant::now);
        let mut quads = filter_candidates(&contour_points, cfg);
        if let Some(start) = t_candidates
            && let Some(s) = stats.as_deref_mut()
        {
            s.candidate_time = start.elapsed();
            s.quads = quads.len();
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

        let t_decode = do_profile.then(Instant::now);
        let markers = if let Some(dict) = aruco_dict.as_ref() {
            decode_quads_aruco_with_config(&out, &quads, sample_scale, dict, &ArucoDecodeConfig::default())
        } else if let Some(family_impl) = family_impl.as_ref() {
            decode_quads_with_config(&out, &quads, sample_scale, family_impl, &ArucoTagDecodeConfig::default())
        } else {
            Vec::new()
        };
        if let Some(start) = t_decode
            && let Some(s) = stats.as_deref_mut()
        {
            s.decode_time = start.elapsed();
            s.markers = markers.len();
        }
        markers
    };

    // Try both polarities and keep whichever yields more markers (optional for speed).
    // Use a thread-local mask buffer to avoid per-frame allocations.
    let markers = with_luma8_frame(processing, |gray_ref| {
        let gray_blurred;
        let gray = if blur_sigma > 0.0 {
            gray_blurred = crate::modules::image::blur::blur_gray_image(gray_ref.clone(), blur_sigma);
            &gray_blurred
        } else {
            gray_ref
        };
        // Otsu thresholding is extremely sensitive to lighting falloff and tends to drop tags
        // under real-world exposure/contrast. Use adaptive mean thresholding instead.
        let threshold_window = threshold_window.max(3);
        let threshold_offset = threshold_offset as f32;

        let preferred_markers = if use_otsu_threshold {
            let threshold = crate::modules::image::binary::otsu_level_gray(gray);
            crate::modules::image::binary::with_binary_threshold_mask(gray, threshold, cfg.invert_mask, |mask| {
                detect_once(mask, &detect_cfg, if do_profile { Some(&mut stats_preferred) } else { None })
            })
        } else if do_profile {
            let (markers, threshold_time) = crate::modules::image::binary::with_adaptive_mean_threshold_fast_timed(gray, threshold_window, threshold_offset, cfg.invert_mask, |mask| {
                detect_once(mask, &detect_cfg, Some(&mut stats_preferred))
            });
            stats_preferred.threshold_time = threshold_time;
            markers
        } else {
            crate::modules::image::binary::with_adaptive_mean_threshold_fast(gray, threshold_window, threshold_offset, cfg.invert_mask, |mask| detect_once(mask, &detect_cfg, None))
        };

        let mut raw_markers = preferred_markers;
        if cfg.try_opposite_polarity {
            let opposite_markers = if use_otsu_threshold {
                let threshold = crate::modules::image::binary::otsu_level_gray(gray);
                crate::modules::image::binary::with_binary_threshold_mask(gray, threshold, !cfg.invert_mask, |mask| {
                    detect_once(mask, &detect_cfg, if do_profile { Some(&mut stats_opposite) } else { None })
                })
            } else if do_profile {
                let (markers, threshold_time) = crate::modules::image::binary::with_adaptive_mean_threshold_fast_timed(gray, threshold_window, threshold_offset, !cfg.invert_mask, |mask| {
                    detect_once(mask, &detect_cfg, Some(&mut stats_opposite))
                });
                stats_opposite.threshold_time = threshold_time;
                markers
            } else {
                crate::modules::image::binary::with_adaptive_mean_threshold_fast(gray, threshold_window, threshold_offset, !cfg.invert_mask, |mask| detect_once(mask, &detect_cfg, None))
            };
            raw_markers.extend(opposite_markers);
        }

        // When the family has a small codebook (e.g. 16h5) duplicate ids are expected, so we
        // must not deduplicate by id here. Instead, merge only near-identical detections that
        // overlap in image space.
        merge_detections_spatial_impl(raw_markers, 8.0, 0.0, 2.5, 0.0, 0.0)
    });

    if let Ok(mut cached) = CACHE.lock() {
        cached.markers = Some(Arc::new(markers.clone()));
    }

    if cfg.draw_markers {
        for marker in &markers {
            draw::aruco::overlay_marker(&mut out, marker, Rgba([0, 255, 0, 255]));
            let corners = &marker.corners;
            let min_x = corners.iter().map(|p| p.x as f32).fold(f32::INFINITY, f32::min);
            let max_x = corners.iter().map(|p| p.x as f32).fold(f32::NEG_INFINITY, f32::max);
            let min_y = corners.iter().map(|p| p.y as f32).fold(f32::INFINITY, f32::min);
            let max_y = corners.iter().map(|p| p.y as f32).fold(f32::NEG_INFINITY, f32::max);
            let bbox = [CvPoint::new(min_x, min_y), CvPoint::new(max_x, min_y), CvPoint::new(max_x, max_y), CvPoint::new(min_x, max_y)];
            draw::contour::overlay_contour_points(&mut out, &bbox, 3, Rgba([0, 255, 255, 255]));
        }
    }

    if cfg.draw_hud {
        overlay_tags_count(&mut out, markers.len());
    }

    if do_profile {
        let total = t_total.map(|t| t.elapsed()).unwrap_or_default();
        tracing::info!(
            w = pw,
            h = ph,
            tags = markers.len(),
            preferred = ?stats_preferred,
            opposite = ?stats_opposite,
            total_ms = total.as_secs_f64() * 1000.0,
            "cv:aruco:detect_overlay profile"
        );
    }

    Ok(Compute::Cpu(out))
}

fn build_detector_config(epsilon: f64, min_area: f32, max_area: f32, min_angle_deg: f64, max_angle_deg: f64, max_side_cv: f32, downscale: u32) -> ArucoTagDetectorConfig {
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

fn contour_points_from_binary_mask(mask: &GrayImage, cfg: &ArucoTagDetectorConfig) -> Vec<Vec<CvPoint<f32>>> {
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

fn contour_points_from_shared_contours(contours: &[Vec<Point>], cfg: &ArucoTagDetectorConfig) -> Vec<Vec<CvPoint<f32>>> {
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

fn decode_detections_from_contour_points(
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
fn detect_detections_from_shared_contours_impl(
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
fn cv_detect_aruco_detections(frame: Compute<DynamicImage>, cfg: ArucoTagOverlayConfig, exec_ctx: &ExecutionContext) -> Result<Vec<ArucoDetection2D>, NodeError> {
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
fn cv_detect_aruco_detections_from_contours(
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
        overlay_tags_count(&mut out, 1);

        match out {
            DynamicImage::ImageLuma8(image) => {
                assert_eq!(image.dimensions(), (96, 96));
            }
            other => panic!("expected luma8 overlay output, got {other:?}"),
        }
    }
}
