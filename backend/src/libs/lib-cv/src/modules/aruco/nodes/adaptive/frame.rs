use super::*;

#[node(
    id = "clahe_gray",
    summary = "Apply CLAHE directly to a grayscale frame.",
    inputs(
        port(name = "mask", source = "Frame", ty = crate::daedalus_types::image_gray8()),
        port(name = "tile_size", default = 2i64, meta(ui_min = 1, ui_max = 64, ui_step = 1)),
        port(name = "clip_limit", default = 3.5f64, meta(ui_min = 0.0, ui_max = 10.0, ui_step = 0.1)),
        port(name = "mix", default = 1.0f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01)),
        port(name = "mode", default = "cpu")
    ),
    outputs(port(name = "mask", source = "Frame", ty = crate::daedalus_types::image_gray8()))
)]
pub(super) fn cv_aruco_clahe_gray(mask: &GrayImage, tile_size: i64, clip_limit: f64, mix: f64, _mode: Option<crate::plugin::ExecMode>, exec_ctx: &ExecutionContext) -> Result<GrayImage, NodeError> {
    let gray = mask;
    let tile_size = tile_size.clamp(1, u32::MAX as i64) as u32;
    let clip_limit = (clip_limit.max(0.0) as f32).max(0.0);
    let mix = (mix as f32).clamp(0.0, 1.0);
    let (width, height) = gray.dimensions();
    if width == 0 || height == 0 {
        return Ok(GrayImage::new(width, height));
    }
    if mix <= 0.001 {
        return Ok(gray.clone());
    }

    let mut out = crate::modules::image::clahe::alloc_gray_image_for_overwrite(width, height);
    if mix >= 0.999 {
        with_adaptive_frame_node_scratch(exec_ctx, |scratch| {
            apply_cached_clahe_into(gray, tile_size, clip_limit, &mut scratch.clahe_tiles, &mut out);
        })
        .map_err(NodeError::Handler)?;
        return Ok(out);
    }

    with_adaptive_frame_node_scratch(exec_ctx, |scratch| {
        apply_cached_clahe_into(gray, tile_size, clip_limit, &mut scratch.clahe_tiles, &mut scratch.clahe);
        crate::modules::image::clahe::blend_clahe_with_base_into(gray, &scratch.clahe, mix, &mut out);
    })
    .map_err(NodeError::Handler)?;
    Ok(out)
}

fn adaptive_border_guarded_gray<'a>(gray: &'a GrayImage, border_guard_px: u32) -> std::borrow::Cow<'a, GrayImage> {
    let width = gray.width() as usize;
    let guard = (border_guard_px as usize).min(width / 2);
    if guard == 0 || width <= 2 {
        return std::borrow::Cow::Borrowed(gray);
    }

    let mut guarded = gray.clone();
    for row in guarded.as_mut().chunks_mut(width) {
        let left_src = row[guard];
        row[..guard].fill(left_src);

        let right_src_idx = width - guard - 1;
        let right_src = row[right_src_idx];
        row[(width - guard)..].fill(right_src);
    }
    std::borrow::Cow::Owned(guarded)
}

#[node(
    id = "adaptive_threshold_gray",
    summary = "Adaptive threshold directly on a grayscale frame.",
    inputs(
        port(name = "frame", source = "Frame", ty = crate::daedalus_types::image_gray8()),
        port(name = "window", default = 9i64, meta(ui_min = 3, ui_max = 101, ui_step = 2)),
        port(name = "offset", default = 0.0f64, meta(ui_min = -50.0, ui_max = 50.0, ui_step = 1.0)),
        port(name = "threshold_offset", default = 0.0f64, meta(ui_min = -50.0, ui_max = 50.0, ui_step = 1.0)),
        port(name = "border_guard_px", default = 0i64, meta(ui_min = 0, ui_max = 32, ui_step = 1)),
        port(name = "invert", default = false),
        port(name = "mode", default = "cpu")
    ),
    outputs(port(name = "mask", source = "Frame", ty = crate::daedalus_types::image_gray8()))
)]
pub(super) fn cv_aruco_adaptive_threshold_gray(
    frame: &GrayImage,
    window: i64,
    offset: f64,
    threshold_offset: f64,
    border_guard_px: i64,
    invert: bool,
    _mode: Option<crate::plugin::ExecMode>,
    exec_ctx: &ExecutionContext,
) -> Result<GrayImage, NodeError> {
    let window = u32::try_from(window).unwrap_or(0);
    let window = if window < 3 {
        window
    } else if window.is_multiple_of(2) {
        window.saturating_add(1)
    } else {
        window
    };
    if window < 3 || frame.width() == 0 || frame.height() == 0 {
        return Ok(GrayImage::new(frame.width(), frame.height()));
    }
    let combined_offset = offset.clamp(-50.0, 50.0) as f32 + threshold_offset.clamp(-50.0, 50.0) as f32;
    let guarded = adaptive_border_guarded_gray(frame, u32::try_from(border_guard_px.max(0)).unwrap_or(0));
    crate::modules::image::binary::adaptive_mean_threshold_fast_with_invert_in(exec_ctx, guarded.as_ref(), window, combined_offset, invert).map_err(NodeError::Handler)
}

#[node(
    id = "adaptive_window_select",
    inputs(
        port(name = "enabled", default = true),
        port(name = "pass_idx", default = 0i64, meta(ui_min = 0, ui_max = 64, ui_step = 1)),
        port(name = "adaptive_window", default = 15i64, meta(ui_min = 3, ui_max = 101, ui_step = 2)),
        port(name = "use_window_sweep", default = false),
        port(name = "adaptive_window_min", default = 3i64, meta(ui_min = 3, ui_max = 101, ui_step = 2)),
        port(name = "adaptive_window_max", default = 23i64, meta(ui_min = 3, ui_max = 101, ui_step = 2)),
        port(name = "adaptive_window_step", default = 10i64, meta(ui_min = 1, ui_max = 50, ui_step = 1))
    ),
    outputs(port(name = "window", default = 0i64))
)]
pub(super) fn cv_aruco_adaptive_window_select(
    enabled: bool,
    pass_idx: i64,
    adaptive_window: i64,
    use_window_sweep: bool,
    adaptive_window_min: i64,
    adaptive_window_max: i64,
    adaptive_window_step: i64,
) -> Result<i64, NodeError> {
    if !enabled {
        return Ok(0);
    }
    let pass_idx_u32 = u32::try_from(pass_idx.max(0)).unwrap_or(0);
    let window = if use_window_sweep {
        let mut window_min = u32::try_from(adaptive_window_min).unwrap_or(3).max(3);
        if window_min.is_multiple_of(2) {
            window_min = window_min.saturating_add(1);
        }
        let mut window_max = u32::try_from(adaptive_window_max).unwrap_or(window_min).max(window_min);
        if window_max.is_multiple_of(2) {
            window_max = window_max.saturating_add(1);
        }
        let step = u32::try_from(adaptive_window_step).unwrap_or(1).max(1);
        let mut w = window_min;
        for _ in 0..pass_idx_u32 {
            let next = w.saturating_add(step);
            w = if next.is_multiple_of(2) { next.saturating_add(1) } else { next };
        }
        if w > window_max { 0 } else { w }
    } else {
        if pass_idx_u32 != 0 {
            return Ok(0);
        }
        let mut w = u32::try_from(adaptive_window).unwrap_or(15).max(3);
        if w.is_multiple_of(2) {
            w = w.saturating_add(1);
        }
        w
    };
    Ok(window as i64)
}

#[node(
    id = "adaptive_threshold_mask",
    inputs(
        port(name = "enabled", default = true),
        port(name = "gray"),
        port(name = "adaptive_window", default = 15i64, meta(ui_min = 0, ui_max = 101, ui_step = 1)),
        port(name = "adaptive_offset", default = 7.0f64, meta(ui_min = 0.0, ui_max = 64.0, ui_step = 1.0)),
        port(name = "threshold_offset", default = 0.0f64, meta(ui_min = -32.0, ui_max = 32.0, ui_step = 1.0)),
        port(name = "invert", default = true),
        port(name = "invert_flip", default = false)
    ),
    outputs(port(name = "mask"))
)]
pub(super) fn cv_aruco_adaptive_threshold_mask(
    enabled: bool,
    gray: &GrayImage,
    adaptive_window: i64,
    adaptive_offset: f64,
    threshold_offset: f64,
    invert: bool,
    invert_flip: bool,
    exec_ctx: &ExecutionContext,
) -> Result<GrayImage, NodeError> {
    if !enabled {
        return Ok(GrayImage::new(0, 0));
    }
    let window = u32::try_from(adaptive_window).unwrap_or(0);
    if window < 3 || gray.width() == 0 || gray.height() == 0 {
        return Ok(GrayImage::new(0, 0));
    }
    let offset = adaptive_offset.clamp(0.0, 64.0) as f32 + threshold_offset.clamp(-32.0, 32.0) as f32;
    let invert = invert ^ invert_flip;
    crate::modules::image::binary::adaptive_mean_threshold_fast_with_invert_in(exec_ctx, gray, window, offset, invert).map_err(NodeError::Handler)
}

struct AdaptiveFrameNodeScratch {
    clahe: GrayImage,
    blended: GrayImage,
    clahe_tiles: crate::modules::image::clahe::ClaheTiles,
}

impl Default for AdaptiveFrameNodeScratch {
    fn default() -> Self {
        Self { clahe: GrayImage::new(0, 0), blended: GrayImage::new(0, 0), clahe_tiles: crate::modules::image::clahe::ClaheTiles::default() }
    }
}

#[inline]
fn adaptive_frame_node_scratch_bytes(scratch: &AdaptiveFrameNodeScratch) -> usize {
    scratch.clahe.as_raw().capacity() + scratch.blended.as_raw().capacity() + scratch.clahe_tiles.luts.capacity() * std::mem::size_of::<[u8; 256]>()
}

#[inline]
fn adaptive_frame_node_scratch_live_bytes(scratch: &AdaptiveFrameNodeScratch) -> usize {
    scratch.clahe.as_raw().len() + scratch.blended.as_raw().len() + scratch.clahe_tiles.luts.len() * std::mem::size_of::<[u8; 256]>()
}

#[inline]
fn clear_gray_image_live(image: &mut GrayImage) {
    let mut buf = std::mem::take(image).into_raw();
    buf.clear();
    *image = GrayImage::from_raw(0, 0, buf).expect("zero-sized gray image");
}

#[inline]
fn release_gray_image(image: &mut GrayImage) {
    let mut buf = std::mem::take(image).into_raw();
    buf.clear();
    buf.shrink_to_fit();
    *image = GrayImage::from_raw(0, 0, buf).expect("zero-sized gray image");
}

#[inline]
fn release_clahe_tiles(tiles: &mut crate::modules::image::clahe::ClaheTiles) {
    *tiles = crate::modules::image::clahe::ClaheTiles::default();
}

impl daedalus::runtime::state::ManagedResource for AdaptiveFrameNodeScratch {
    fn live_bytes(&self) -> u64 {
        adaptive_frame_node_scratch_live_bytes(self) as u64
    }

    fn retained_bytes(&self) -> u64 {
        adaptive_frame_node_scratch_bytes(self) as u64
    }

    fn touched_bytes(&self) -> u64 {
        self.live_bytes()
    }

    fn after_frame(&mut self) {
        clear_gray_image_live(&mut self.clahe);
        clear_gray_image_live(&mut self.blended);
    }

    fn on_memory_pressure(&mut self) {
        release_gray_image(&mut self.clahe);
        release_gray_image(&mut self.blended);
        release_clahe_tiles(&mut self.clahe_tiles);
    }

    fn on_idle(&mut self) {
        self.on_memory_pressure();
    }

    fn on_stop(&mut self) {
        self.on_memory_pressure();
    }
}

fn with_adaptive_frame_node_scratch<R>(exec_ctx: &ExecutionContext, f: impl FnOnce(&mut AdaptiveFrameNodeScratch) -> R) -> Result<R, String> {
    exec_ctx.with_frame_scratch("aruco.adaptive.frame", AdaptiveFrameNodeScratch::default, f)
}

pub(crate) fn compact_adaptive_frame_node_scratch_after_frame() {}

#[inline]
fn apply_cached_clahe_into(gray: &GrayImage, tile_size: u32, clip_limit: f32, tiles: &mut crate::modules::image::clahe::ClaheTiles, output: &mut GrayImage) {
    crate::modules::image::clahe::prepare_clahe_into(gray, tile_size, clip_limit, tiles);
    crate::modules::image::clahe::apply_clahe_with_tiles_into(gray, tiles, output);
}

#[allow(clippy::too_many_arguments)]
fn collect_adaptive_quads(
    mask: &GrayImage,
    min_perimeter_rate: f64,
    max_perimeter_rate: f64,
    epsilon: f64,
    min_area: f64,
    max_area: f64,
    min_angle_deg: f64,
    max_angle_deg: f64,
    max_side_cv: f64,
    min_corner_distance_rate: f64,
    min_distance_to_border: i64,
    min_side_px: f64,
    fallback_max_contours: i64,
    max_quads: i64,
    expand_inner_scale: f64,
    expand_inner_max_side_px: f64,
) -> Result<Vec<Quad>, NodeError> {
    if mask.width() == 0 || mask.height() == 0 {
        return Ok(Vec::new());
    }

    let cfg = crate::modules::aruco::adaptive::AdaptiveDetectorConfig {
        adaptive_window: 15,
        adaptive_offset: 7.0,
        threshold_offset: 0.0,
        invert: false,
        open_k: 0,
        min_perimeter_rate: min_perimeter_rate.max(0.0) as f32,
        max_perimeter_rate: max_perimeter_rate.max(min_perimeter_rate + f64::EPSILON) as f32,
        epsilon: epsilon.max(0.01) as f32,
        min_area: min_area.max(0.0) as f32,
        max_area: if max_area > 0.0 { Some(max_area as f32) } else { None },
        min_angle: min_angle_deg.max(0.0) as f32,
        max_angle: max_angle_deg.min(180.0) as f32,
        max_side_cv: max_side_cv.max(0.05) as f32,
        min_corner_distance_rate: min_corner_distance_rate.max(0.0) as f32,
        min_distance_to_border: u32::try_from(min_distance_to_border.max(0)).unwrap_or(0),
        min_side_px: min_side_px.max(0.0) as f32,
        fallback_max_contours: usize::try_from(fallback_max_contours.clamp(0, 1000)).unwrap_or(120),
        max_quads: usize::try_from(max_quads.clamp(0, 512)).unwrap_or(0),
    };

    let expand_inner_scale = expand_inner_scale.max(1.0);
    let expand_inner_max_side_px = expand_inner_max_side_px.max(0.0);
    let max_x = (mask.width().saturating_sub(1)) as f64;
    let max_y = (mask.height().saturating_sub(1)) as f64;

    let mut out = Vec::new();
    for quad in crate::modules::aruco::adaptive::adaptive_quads_from_mask(mask, &cfg) {
        let base_quad = [
            Point { x: quad[0].x as f64, y: quad[0].y as f64 },
            Point { x: quad[1].x as f64, y: quad[1].y as f64 },
            Point { x: quad[2].x as f64, y: quad[2].y as f64 },
            Point { x: quad[3].x as f64, y: quad[3].y as f64 },
        ];
        out.push(base_quad);

        if expand_inner_scale > 1.0 && expand_inner_max_side_px > 0.0 {
            let mut max_side = 0.0f64;
            for i in 0..4usize {
                let a = base_quad[i];
                let b = base_quad[(i + 1) % 4];
                let dx = a.x - b.x;
                let dy = a.y - b.y;
                max_side = max_side.max((dx * dx + dy * dy).sqrt());
            }
            if max_side <= expand_inner_max_side_px {
                let cx = (base_quad[0].x + base_quad[1].x + base_quad[2].x + base_quad[3].x) * 0.25;
                let cy = (base_quad[0].y + base_quad[1].y + base_quad[2].y + base_quad[3].y) * 0.25;
                let mut expanded = base_quad;
                for p in &mut expanded {
                    p.x = (cx + (p.x - cx) * expand_inner_scale).clamp(0.0, max_x);
                    p.y = (cy + (p.y - cy) * expand_inner_scale).clamp(0.0, max_y);
                }
                out.push(expanded);
            }
        }
    }
    Ok(out)
}

#[node(
    id = "adaptive_quads_from_frame",
    summary = "Extract adaptive quads from a frame without materializing intermediate graph images.",
    inputs(
        port(name = "frame", source = "Frame", ty = crate::daedalus_types::image_gray8()),
        port(name = "enabled", default = true),
        port(name = "tile_size", default = 2i64, meta(ui_min = 1, ui_max = 64, ui_step = 1)),
        port(name = "clip_limit", default = 3.5f64, meta(ui_min = 0.0, ui_max = 10.0, ui_step = 0.1)),
        port(name = "mix", default = 1.0f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01)),
        port(name = "adaptive_window", default = 61i64, meta(ui_min = 3, ui_max = 101, ui_step = 2)),
        port(name = "adaptive_offset", default = 10.0f64, meta(ui_min = -50.0, ui_max = 50.0, ui_step = 1.0)),
        port(name = "threshold_offset", default = 0.0f64, meta(ui_min = -32.0, ui_max = 32.0, ui_step = 1.0)),
        port(name = "invert", default = true),
        port(name = "min_perimeter_rate", default = 0.03f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01)),
        port(name = "max_perimeter_rate", default = 4.0f64, meta(ui_min = 0.0, ui_max = 20.0, ui_step = 0.1)),
        port(name = "epsilon", default = 5.0f64, meta(ui_min = 0.01, ui_max = 1000.0, ui_step = 0.1)),
        port(name = "min_area", default = 120.0f64, meta(ui_min = 0.0, ui_max = 1000000.0, ui_step = 1000.0)),
        port(name = "max_area", default = 0.0f64, meta(ui_min = 0.0, ui_max = 1000000.0, ui_step = 1000.0)),
        port(name = "min_angle_deg", default = 5.0f64, meta(ui_min = 0.0, ui_max = 180.0, ui_step = 1.0)),
        port(name = "max_angle_deg", default = 175.0f64, meta(ui_min = 0.0, ui_max = 180.0, ui_step = 1.0)),
        port(name = "max_side_cv", default = 4.0f64, meta(ui_min = 0.05, ui_max = 20.0, ui_step = 0.1)),
        port(name = "min_corner_distance_rate", default = 0.05f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01)),
        port(name = "min_distance_to_border", default = 3i64, meta(ui_min = 0, ui_max = 128, ui_step = 1)),
        port(name = "min_side_px", default = 0.0f64, meta(ui_min = 0.0, ui_max = 128.0, ui_step = 0.5)),
        port(name = "fallback_max_contours", default = 120i64, meta(ui_min = 0, ui_max = 1000, ui_step = 1)),
        port(name = "max_quads", default = 0i64, meta(ui_min = 0, ui_max = 512, ui_step = 1)),
        port(name = "expand_inner_scale", default = 1.0f64, meta(ui_min = 1.0, ui_max = 4.0, ui_step = 0.05)),
        port(name = "expand_inner_max_side_px", default = 0.0f64, meta(ui_min = 0.0, ui_max = 512.0, ui_step = 1.0))
    ),
    outputs(port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()))
)]
#[allow(clippy::too_many_arguments)]
pub(super) fn cv_aruco_adaptive_quads_from_frame(
    frame: &GrayImage,
    enabled: bool,
    tile_size: i64,
    clip_limit: f64,
    mix: f64,
    adaptive_window: i64,
    adaptive_offset: f64,
    threshold_offset: f64,
    invert: bool,
    min_perimeter_rate: f64,
    max_perimeter_rate: f64,
    epsilon: f64,
    min_area: f64,
    max_area: f64,
    min_angle_deg: f64,
    max_angle_deg: f64,
    max_side_cv: f64,
    min_corner_distance_rate: f64,
    min_distance_to_border: i64,
    min_side_px: f64,
    fallback_max_contours: i64,
    max_quads: i64,
    expand_inner_scale: f64,
    expand_inner_max_side_px: f64,
    exec_ctx: &ExecutionContext,
) -> Result<Vec<Quad>, NodeError> {
    collect_adaptive_quads_from_gray_frame(
        frame,
        enabled,
        tile_size,
        clip_limit,
        mix,
        adaptive_window,
        adaptive_offset,
        threshold_offset,
        invert,
        min_perimeter_rate,
        max_perimeter_rate,
        epsilon,
        min_area,
        max_area,
        min_angle_deg,
        max_angle_deg,
        max_side_cv,
        min_corner_distance_rate,
        min_distance_to_border,
        min_side_px,
        fallback_max_contours,
        max_quads,
        expand_inner_scale,
        expand_inner_max_side_px,
        exec_ctx,
    )
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

fn collect_adaptive_quads_from_gray_frame(
    frame: &GrayImage,
    enabled: bool,
    tile_size: i64,
    clip_limit: f64,
    mix: f64,
    adaptive_window: i64,
    adaptive_offset: f64,
    threshold_offset: f64,
    invert: bool,
    min_perimeter_rate: f64,
    max_perimeter_rate: f64,
    epsilon: f64,
    min_area: f64,
    max_area: f64,
    min_angle_deg: f64,
    max_angle_deg: f64,
    max_side_cv: f64,
    min_corner_distance_rate: f64,
    min_distance_to_border: i64,
    min_side_px: f64,
    fallback_max_contours: i64,
    max_quads: i64,
    expand_inner_scale: f64,
    expand_inner_max_side_px: f64,
    exec_ctx: &ExecutionContext,
) -> Result<Vec<Quad>, NodeError> {
    if !enabled {
        return Ok(Vec::new());
    }

    let window = u32::try_from(adaptive_window).unwrap_or(0);
    if window < 3 || frame.width() == 0 || frame.height() == 0 {
        return Ok(Vec::new());
    }

    let tile_size = tile_size.clamp(1, u32::MAX as i64) as u32;
    let clip_limit = (clip_limit.max(0.0) as f32).max(0.0);
    let mix = (mix as f32).clamp(0.0, 1.0);
    let offset = adaptive_offset.clamp(0.0, 64.0) as f32 + threshold_offset.clamp(-32.0, 32.0) as f32;

    with_adaptive_frame_node_scratch(exec_ctx, |scratch| {
        let AdaptiveFrameNodeScratch { clahe, blended, clahe_tiles } = &mut *scratch;

        let threshold_input: &GrayImage = if mix <= 0.001 {
            frame
        } else {
            apply_cached_clahe_into(frame, tile_size, clip_limit, clahe_tiles, clahe);
            if mix >= 0.999 {
                clahe
            } else {
                crate::modules::image::clahe::blend_clahe_with_base_into(frame, clahe, mix, blended);
                blended
            }
        };

        crate::modules::image::binary::with_adaptive_mean_threshold_fast(threshold_input, window, offset, invert, |mask| {
            collect_adaptive_quads(
                mask,
                min_perimeter_rate,
                max_perimeter_rate,
                epsilon,
                min_area,
                max_area,
                min_angle_deg,
                max_angle_deg,
                max_side_cv,
                min_corner_distance_rate,
                min_distance_to_border,
                min_side_px,
                fallback_max_contours,
                max_quads,
                expand_inner_scale,
                expand_inner_max_side_px,
            )
        })
    })
    .map_err(NodeError::Handler)?
}

#[node(
    id = "adaptive_quads_from_roi_frame",
    summary = "Extract adaptive quads directly from an input frame plus ROI inputs.",
    inputs(
        port(name = "frame", source = "Frame", ty = crate::daedalus_types::image_dynamic()),
        port(name = "roi_x", default = 0i64, meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
        port(name = "roi_y", default = 0i64, meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
        port(name = "roi_w", default = 0i64, meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
        port(name = "roi_h", default = 0i64, meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
        port(name = "enabled", default = true),
        port(name = "tile_size", default = 2i64, meta(ui_min = 1, ui_max = 64, ui_step = 1)),
        port(name = "clip_limit", default = 3.5f64, meta(ui_min = 0.0, ui_max = 10.0, ui_step = 0.1)),
        port(name = "mix", default = 1.0f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01)),
        port(name = "adaptive_window", default = 61i64, meta(ui_min = 3, ui_max = 101, ui_step = 2)),
        port(name = "adaptive_offset", default = 10.0f64, meta(ui_min = -50.0, ui_max = 50.0, ui_step = 1.0)),
        port(name = "threshold_offset", default = 0.0f64, meta(ui_min = -32.0, ui_max = 32.0, ui_step = 1.0)),
        port(name = "invert", default = true),
        port(name = "min_perimeter_rate", default = 0.03f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01)),
        port(name = "max_perimeter_rate", default = 4.0f64, meta(ui_min = 0.0, ui_max = 20.0, ui_step = 0.1)),
        port(name = "epsilon", default = 5.0f64, meta(ui_min = 0.01, ui_max = 1000.0, ui_step = 0.1)),
        port(name = "min_area", default = 120.0f64, meta(ui_min = 0.0, ui_max = 1000000.0, ui_step = 1000.0)),
        port(name = "max_area", default = 0.0f64, meta(ui_min = 0.0, ui_max = 1000000.0, ui_step = 1000.0)),
        port(name = "min_angle_deg", default = 5.0f64, meta(ui_min = 0.0, ui_max = 180.0, ui_step = 1.0)),
        port(name = "max_angle_deg", default = 175.0f64, meta(ui_min = 0.0, ui_max = 180.0, ui_step = 1.0)),
        port(name = "max_side_cv", default = 4.0f64, meta(ui_min = 0.05, ui_max = 20.0, ui_step = 0.1)),
        port(name = "min_corner_distance_rate", default = 0.05f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01)),
        port(name = "min_distance_to_border", default = 3i64, meta(ui_min = 0, ui_max = 128, ui_step = 1)),
        port(name = "min_side_px", default = 0.0f64, meta(ui_min = 0.0, ui_max = 128.0, ui_step = 0.5)),
        port(name = "fallback_max_contours", default = 120i64, meta(ui_min = 0, ui_max = 1000, ui_step = 1)),
        port(name = "max_quads", default = 0i64, meta(ui_min = 0, ui_max = 512, ui_step = 1)),
        port(name = "expand_inner_scale", default = 1.0f64, meta(ui_min = 1.0, ui_max = 4.0, ui_step = 0.05)),
        port(name = "expand_inner_max_side_px", default = 0.0f64, meta(ui_min = 0.0, ui_max = 512.0, ui_step = 1.0))
    ),
    outputs(port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()))
)]
#[allow(clippy::too_many_arguments)]
pub(super) fn cv_aruco_adaptive_quads_from_roi_frame(
    frame: &DynamicImage,
    roi_x: i64,
    roi_y: i64,
    roi_w: i64,
    roi_h: i64,
    enabled: bool,
    tile_size: i64,
    clip_limit: f64,
    mix: f64,
    adaptive_window: i64,
    adaptive_offset: f64,
    threshold_offset: f64,
    invert: bool,
    min_perimeter_rate: f64,
    max_perimeter_rate: f64,
    epsilon: f64,
    min_area: f64,
    max_area: f64,
    min_angle_deg: f64,
    max_angle_deg: f64,
    max_side_cv: f64,
    min_corner_distance_rate: f64,
    min_distance_to_border: i64,
    min_side_px: f64,
    fallback_max_contours: i64,
    max_quads: i64,
    expand_inner_scale: f64,
    expand_inner_max_side_px: f64,
    exec_ctx: &ExecutionContext,
) -> Result<Vec<Quad>, NodeError> {
    let (fw, fh) = frame.dimensions();
    let (x, y, w, h) = roi_bounds_or_full(fw, fh, roi_x, roi_y, roi_w, roi_h);
    crate::modules::image::luma::with_cropped_luma8_frame(frame, x, y, w, h, |gray| {
        collect_adaptive_quads_from_gray_frame(
            gray,
            enabled,
            tile_size,
            clip_limit,
            mix,
            adaptive_window,
            adaptive_offset,
            threshold_offset,
            invert,
            min_perimeter_rate,
            max_perimeter_rate,
            epsilon,
            min_area,
            max_area,
            min_angle_deg,
            max_angle_deg,
            max_side_cv,
            min_corner_distance_rate,
            min_distance_to_border,
            min_side_px,
            fallback_max_contours,
            max_quads,
            expand_inner_scale,
            expand_inner_max_side_px,
            exec_ctx,
        )
    })
}

#[node(
    id = "adaptive_quads_from_mask",
    summary = "Extract quads from a binary mask.",
    inputs(
        port(name = "enabled", default = true),
        port(name = "mask"),
        port(name = "min_perimeter_rate", default = 0.03f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01)),
        port(name = "max_perimeter_rate", default = 4.0f64, meta(ui_min = 0.0, ui_max = 20.0, ui_step = 0.1)),
        port(name = "epsilon", default = 5.0f64, meta(ui_min = 0.01, ui_max = 1000.0, ui_step = 0.1)),
        port(name = "min_area", default = 120.0f64, meta(ui_min = 0.0, ui_max = 1000000.0, ui_step = 1000.0)),
        port(name = "max_area", default = 0.0f64, meta(ui_min = 0.0, ui_max = 1000000.0, ui_step = 1000.0)),
        port(name = "min_angle_deg", default = 5.0f64, meta(ui_min = 0.0, ui_max = 180.0, ui_step = 1.0)),
        port(name = "max_angle_deg", default = 175.0f64, meta(ui_min = 0.0, ui_max = 180.0, ui_step = 1.0)),
        port(name = "max_side_cv", default = 4.0f64, meta(ui_min = 0.05, ui_max = 20.0, ui_step = 0.1)),
        port(name = "min_corner_distance_rate", default = 0.05f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01)),
        port(name = "min_distance_to_border", default = 3i64, meta(ui_min = 0, ui_max = 128, ui_step = 1)),
        port(name = "min_side_px", default = 0.0f64, meta(ui_min = 0.0, ui_max = 128.0, ui_step = 0.5)),
        port(name = "fallback_max_contours", default = 120i64, meta(ui_min = 0, ui_max = 1000, ui_step = 1)),
        port(name = "max_quads", default = 0i64, meta(ui_min = 0, ui_max = 512, ui_step = 1)),
        port(name = "expand_inner_scale", default = 1.0f64, meta(ui_min = 1.0, ui_max = 4.0, ui_step = 0.05)),
        port(name = "expand_inner_max_side_px", default = 0.0f64, meta(ui_min = 0.0, ui_max = 512.0, ui_step = 1.0))
    ),
    outputs(port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()))
)]
#[allow(clippy::too_many_arguments)]
pub(super) fn cv_aruco_adaptive_quads_from_mask(
    enabled: bool,
    mask: &GrayImage,
    min_perimeter_rate: f64,
    max_perimeter_rate: f64,
    epsilon: f64,
    min_area: f64,
    max_area: f64,
    min_angle_deg: f64,
    max_angle_deg: f64,
    max_side_cv: f64,
    min_corner_distance_rate: f64,
    min_distance_to_border: i64,
    min_side_px: f64,
    fallback_max_contours: i64,
    max_quads: i64,
    expand_inner_scale: f64,
    expand_inner_max_side_px: f64,
) -> Result<Vec<Quad>, NodeError> {
    if !enabled {
        return Ok(Vec::new());
    }
    collect_adaptive_quads(
        mask,
        min_perimeter_rate,
        max_perimeter_rate,
        epsilon,
        min_area,
        max_area,
        min_angle_deg,
        max_angle_deg,
        max_side_cv,
        min_corner_distance_rate,
        min_distance_to_border,
        min_side_px,
        fallback_max_contours,
        max_quads,
        expand_inner_scale,
        expand_inner_max_side_px,
    )
}
