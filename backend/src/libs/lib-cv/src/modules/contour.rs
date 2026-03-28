pub mod douglas_peucker;
pub mod scaling;
pub mod suzuki_abe;

pub(crate) fn compact_runtime_scratch_after_frame() {
    douglas_peucker::compact_rdp_scratch_after_frame();
    suzuki_abe::compact_suzuki_scratch_after_frame();
}

pub(crate) fn release_runtime_scratch_on_idle() {
    compact_runtime_scratch_after_frame();
    suzuki_abe::release_suzuki_scratch_on_idle();
}

#[cfg(feature = "engine")]
pub mod nodes {
    #![allow(clippy::ptr_arg)]

    use memchr::{memchr, memrchr};

    use crate::Point;
    use crate::contour::suzuki_abe::suzuki_abe_i32;
    use crate::modules::contour::douglas_peucker::approx_poly_dp_points;
    use crate::plugin::ExecMode;
    #[cfg(feature = "gpu")]
    use bytemuck::{Pod, Zeroable};
    #[cfg(feature = "gpu")]
    use daedalus::ComputeAffinity;
    use daedalus::declare_plugin;
    use daedalus::gpu::Compute;
    #[cfg(feature = "gpu")]
    use daedalus::gpu::shader::{BufferOut, ShaderContext, Uniform};
    #[cfg(feature = "gpu")]
    use daedalus::macros::GpuBindings;
    use daedalus::macros::node;
    use daedalus::runtime::NodeError;
    use daedalus::runtime::state::ExecutionContext;
    #[cfg(feature = "gpu")]
    use image::GenericImageView;
    use image::{DynamicImage, GrayImage, Luma, RgbaImage};

    #[cfg_attr(
        feature = "gpu",
        node(
            id = "contours",
            summary = "Extract contours from a binary mask.",
            description = "Runs Suzuki–Abe contour tracing over the input grayscale mask (non-zero treated as foreground). In GPU mode, performs compact ROI readback before CPU tracing.",
            compute(ComputeAffinity::GpuPreferred),
            inputs(port(name = "mask", description = "Binary/grayscale mask image (foreground > 0).")),
            outputs(port(
                name = "contours",
                source = "Contours",
                ty = crate::daedalus_types::contours(),
                description = "List of contours; each contour is a list of points."
            )),
            shaders(MaskTileReduceBindings, MaskPackBindings)
        )
    )]
    #[cfg_attr(
        not(feature = "gpu"),
        node(
            id = "contours",
            summary = "Extract contours from a binary mask.",
            description = "Runs Suzuki–Abe contour tracing over the input grayscale mask (non-zero treated as foreground).",
            inputs(port(name = "mask", description = "Binary/grayscale mask image (foreground > 0).")),
            outputs(port(
                name = "contours",
                source = "Contours",
                ty = crate::daedalus_types::contours(),
                description = "List of contours; each contour is a list of points."
            ))
        )
    )]
    fn cv_find_contours(mask: Compute<DynamicImage>, #[cfg(feature = "gpu")] ctx: ShaderContext, exec_ctx: &ExecutionContext) -> Result<Vec<Vec<Point>>, NodeError> {
        #[cfg(feature = "gpu")]
        {
            let input_is_gpu = matches!(&mask, Compute::Gpu(_));
            let want_gpu = ctx.gpu.is_some() && input_is_gpu;
            if want_gpu {
                if let Some((gray, roi_x, roi_y)) = read_mask_roi_compact(&mask, &ctx, 16)? {
                    let mut contours = find_contours_gray(&gray);
                    if roi_x != 0 || roi_y != 0 {
                        for contour in &mut contours {
                            for p in contour {
                                p.x += roi_x as f64;
                                p.y += roi_y as f64;
                            }
                        }
                    }
                    return Ok(contours);
                }
                return Ok(Vec::new());
            }
        }

        let gray = match mask {
            Compute::Cpu(img) => img.to_luma8(),
            Compute::Gpu(handle) => {
                let gpu = exec_ctx.gpu.as_ref().ok_or_else(|| NodeError::Handler("contours: gpu payload missing context".into()))?;
                let bytes = gpu.read_texture(&handle).map_err(|e| NodeError::Handler(format!("contours: {e}")))?;
                let rgba = RgbaImage::from_raw(handle.width, handle.height, bytes).ok_or_else(|| NodeError::Handler("contours: invalid image dimensions".into()))?;
                DynamicImage::ImageRgba8(rgba).to_luma8()
            }
        };
        Ok(find_contours_gray(&gray))
    }

    fn find_contours_gray(mask: &image::GrayImage) -> Vec<Vec<Point>> {
        let w = mask.width() as usize;
        let h = mask.height() as usize;
        if w == 0 || h == 0 {
            return Vec::new();
        }

        // Fast-path: compute a tight bounding box around non-zero pixels and run contour tracing
        // only within that ROI. This can massively reduce work on typical masks where the
        // foreground occupies a small portion of the frame (e.g. a board in view).
        let buf = mask.as_raw();
        let mut any = false;
        let mut min_x = w;
        let mut max_x = 0usize;
        let mut min_y = h;
        let mut max_y = 0usize;

        for y in 0..h {
            let row = &buf[y * w..(y + 1) * w];
            let Some(x0) = memchr(255, row) else { continue };
            let x1 = memrchr(255, row).unwrap_or(x0);
            any = true;
            min_y = min_y.min(y);
            max_y = max_y.max(y);
            min_x = min_x.min(x0);
            max_x = max_x.max(x1);
        }

        if !any {
            return Vec::new();
        }

        // Expand by 1px to preserve contours that touch the ROI boundary.
        min_x = min_x.saturating_sub(1);
        min_y = min_y.saturating_sub(1);
        max_x = (max_x + 1).min(w.saturating_sub(1));
        max_y = (max_y + 1).min(h.saturating_sub(1));

        let roi_w = max_x.saturating_sub(min_x) + 1;
        let roi_h = max_y.saturating_sub(min_y) + 1;
        let full_area = w.saturating_mul(h);
        let roi_area = roi_w.saturating_mul(roi_h);

        // Only pay the ROI copy cost when it saves a meaningful amount of work.
        let use_roi = roi_area > 0 && full_area > 0 && roi_area * 100 < full_area * 85;

        let (contours, off_x, off_y) = if use_roi {
            let mut roi = image::GrayImage::new(roi_w as u32, roi_h as u32);
            let dst = roi.as_mut();
            for yy in 0..roi_h {
                let src_row = &buf[(min_y + yy) * w + min_x..(min_y + yy) * w + min_x + roi_w];
                dst[yy * roi_w..yy * roi_w + roi_w].copy_from_slice(src_row);
            }
            (suzuki_abe_i32(&roi), min_x as i32, min_y as i32)
        } else {
            (suzuki_abe_i32(mask), 0, 0)
        };

        let mut out: Vec<Vec<Point>> = Vec::with_capacity(contours.len());
        for contour in contours {
            let mut points: Vec<Point> = Vec::with_capacity(contour.points.len());
            for p in contour.points {
                points.push(Point { x: (p.x + off_x) as f64, y: (p.y + off_y) as f64 });
            }
            out.push(points);
        }
        out
    }

    fn polygon_area(contour: &Vec<Point>) -> f64 {
        if contour.len() < 3 {
            return 0.0;
        }
        let mut sum = 0.0;
        for i in 0..contour.len() {
            let j = (i + 1) % contour.len();
            sum += contour[i].x * contour[j].y - contour[j].x * contour[i].y;
        }
        sum.abs() * 0.5
    }

    #[node(
	        id = "largest",
	        summary = "Select the largest contour by area.",
	        inputs(port(
	            name = "contours",
	            source = "Contours",
	            ty = crate::daedalus_types::contours(),
	            description = "Candidate contours."
	        )),
	        outputs(port(
	            name = "contour",
	            source = "Contour",
	            ty = crate::daedalus_types::contour(),
	            description = "Largest contour, or empty if none found."
	        ))
	    )]
    fn cv_select_largest_contour(contours: &Vec<Vec<Point>>) -> Result<Vec<Point>, NodeError> {
        Ok(contours.iter().max_by(|a, b| polygon_area(a).partial_cmp(&polygon_area(b)).unwrap_or(std::cmp::Ordering::Equal)).cloned().unwrap_or_default())
    }

    #[node(
	        id = "douglas",
	        summary = "Simplify a contour using Douglas–Peucker.",
	        inputs(
	            port(name = "contour", source = "Contour", ty = crate::daedalus_types::contour(), description = "Input contour points."),
	            port(name = "epsilon", description = "Simplification tolerance in pixels.", meta(ui_min = 0.0, ui_max = 20.0, ui_step = 0.5))
	        ),
	        outputs(port(name = "contour", source = "Contour", ty = crate::daedalus_types::contour(), description = "Simplified contour."))
	    )]
    fn cv_approx_poly_dp(contour: &Vec<Point>, epsilon: f64) -> Result<Vec<Point>, NodeError> {
        if contour.len() < 4 || epsilon <= 0.0 {
            return Ok(contour.clone());
        }
        Ok(approx_poly_dp_points(contour, true, epsilon))
    }

    #[node(
	        id = "approx_contours_dp",
	        summary = "Simplify contours using Douglas–Peucker.",
	        description = "Applies Douglas–Peucker simplification to each contour in the list to reduce point count for downstream processing (drawing/quad fitting).",
	        inputs(
	            port(name = "contours", source = "Contours", ty = crate::daedalus_types::contours(), description = "Input contours."),
	            port(name = "epsilon", description = "Simplification tolerance in pixels.", meta(ui_min = 0.0, ui_max = 20.0, ui_step = 0.5))
	        ),
	        outputs(port(name = "contours", source = "Contours", ty = crate::daedalus_types::contours(), description = "Simplified contours."))
	    )]
    fn cv_approx_contours_dp(contours: &Vec<Vec<Point>>, epsilon: f64) -> Result<Vec<Vec<Point>>, NodeError> {
        let eps = epsilon.max(0.0) as f32;
        if eps <= 0.0 {
            return Ok(contours.to_vec());
        }

        let mut out = Vec::with_capacity(contours.len());
        for contour in contours.iter() {
            if contour.len() < 4 {
                out.push(contour.clone());
                continue;
            }
            out.push(approx_poly_dp_points(contour, true, eps as f64));
        }
        Ok(out)
    }

    #[node(
        id = "contours_frame",
        summary = "Convert contours into a tiny debug frame.",
        description = "Produces a 1×1 grayscale image with intensity proportional to the contour count. Useful for forcing contour computation in graphs without drawing overhead.",
        inputs(port(name = "contours", source = "Contours", ty = crate::daedalus_types::contours())),
        outputs("frame")
    )]
    fn cv_contours_to_frame(contours: &Vec<Vec<Point>>) -> Result<DynamicImage, NodeError> {
        if contours.is_empty() {
            let img: GrayImage = GrayImage::from_pixel(1, 1, Luma([0]));
            return Ok(DynamicImage::ImageLuma8(img));
        }
        Ok(contours_to_frame(contours))
    }

    fn contours_to_frame(contours: &Vec<Vec<Point>>) -> DynamicImage {
        let v = (contours.len().min(255)) as u8;
        let img: GrayImage = GrayImage::from_pixel(1, 1, Luma([v]));
        DynamicImage::ImageLuma8(img)
    }

    #[cfg(feature = "gpu")]
    #[repr(C)]
    #[derive(Copy, Clone, Pod, Zeroable)]
    struct MaskTileParams {
        width: u32,
        height: u32,
        tile: u32,
        tiles_w: u32,
        tiles_h: u32,
        // WGSL uniform layout uses 16-byte alignment; the shader declares `_pad: vec3<u32>`,
        // which forces the struct to 48 bytes (vec3 is padded to 16 bytes, and it is itself
        // placed at the next 16-byte boundary). Keep the Rust struct size in sync to avoid
        // wgpu validation errors.
        _pad: [u32; 7],
    }

    #[cfg(feature = "gpu")]
    #[repr(C)]
    #[derive(Copy, Clone, Pod, Zeroable)]
    struct MaskPackParams {
        width: u32,
        height: u32,
        roi_x: u32,
        roi_y: u32,
        roi_w: u32,
        roi_h: u32,
        packs_w: u32,
        _pad: u32,
    }

    #[cfg(feature = "gpu")]
    #[derive(GpuBindings)]
    #[gpu(spec(src = "src/gpu/shaders/mask_tile_reduce.wgsl", entry = "tile_reduce_main"))]
    struct MaskTileReduceBindings<'a> {
        #[gpu(binding = 0, texture2d(format = "rgba8unorm"))]
        input: &'a Compute<DynamicImage>,
        #[gpu(binding = 1, storage(read_write), zeroed, readback)]
        tiles: BufferOut,
        #[gpu(binding = 2, uniform)]
        params: Uniform<MaskTileParams>,
    }

    #[cfg(feature = "gpu")]
    #[derive(GpuBindings)]
    #[gpu(spec(src = "src/gpu/shaders/mask_pack.wgsl", entry = "pack_mask_main"))]
    struct MaskPackBindings<'a> {
        #[gpu(binding = 0, texture2d(format = "rgba8unorm"))]
        input: &'a Compute<DynamicImage>,
        #[gpu(binding = 1, storage(read_write), zeroed, readback)]
        packed: BufferOut,
        #[gpu(binding = 2, uniform)]
        params: Uniform<MaskPackParams>,
    }

    #[cfg(feature = "gpu")]
    fn read_mask_roi_compact(mask: &Compute<DynamicImage>, ctx: &ShaderContext, tile: u32) -> Result<Option<(GrayImage, u32, u32)>, NodeError> {
        let (width, height) = mask.dimensions();
        if width == 0 || height == 0 {
            return Ok(None);
        }

        let tile = tile.max(1);
        if tile == 1 {
            let gray = read_mask_pack(mask, ctx, 0, 0, width, height)?;
            return Ok(Some((gray, 0, 0)));
        }
        let tiles_w = width.div_ceil(tile);
        let tiles_h = height.div_ceil(tile);
        let tiles_len = (tiles_w as u64).saturating_mul(tiles_h as u64).saturating_mul(4);

        let tiles = BufferOut::write_bytes(tiles_len);
        let params = MaskTileParams { width, height, tile, tiles_w, tiles_h, _pad: [0; 7] };
        let bindings = MaskTileReduceBindings { input: mask, tiles, params: Uniform::new(params) };
        let out = ctx.dispatch_bindings(&bindings, None, None, Some([tiles_w, tiles_h, 1])).map_err(|e| NodeError::Handler(format!("contours_compact (gpu tile): {e}")))?;
        let bytes = out.buffers.get(&1).ok_or_else(|| NodeError::Handler("contours_compact (gpu tile): missing readback".into()))?;
        if bytes.len() < tiles_len as usize {
            return Err(NodeError::Handler("contours_compact (gpu tile): short readback".into()));
        }

        let mut min_x = tiles_w;
        let mut min_y = tiles_h;
        let mut max_x = 0u32;
        let mut max_y = 0u32;
        let mut any = false;
        for ty in 0..tiles_h {
            for tx in 0..tiles_w {
                let idx = (ty * tiles_w + tx) as usize;
                let offset = idx * 4;
                let v = u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]);
                if v != 0 {
                    any = true;
                    min_x = min_x.min(tx);
                    min_y = min_y.min(ty);
                    max_x = max_x.max(tx);
                    max_y = max_y.max(ty);
                }
            }
        }

        if !any {
            return Ok(None);
        }

        let roi_x = min_x.saturating_mul(tile).min(width);
        let roi_y = min_y.saturating_mul(tile).min(height);
        let roi_x2 = (max_x.saturating_add(1).saturating_mul(tile)).min(width);
        let roi_y2 = (max_y.saturating_add(1).saturating_mul(tile)).min(height);
        let roi_w = roi_x2.saturating_sub(roi_x).max(1);
        let roi_h = roi_y2.saturating_sub(roi_y).max(1);

        let gray = read_mask_pack(mask, ctx, roi_x, roi_y, roi_w, roi_h)?;
        Ok(Some((gray, roi_x, roi_y)))
    }

    #[cfg(feature = "gpu")]
    fn read_mask_pack(mask: &Compute<DynamicImage>, ctx: &ShaderContext, roi_x: u32, roi_y: u32, roi_w: u32, roi_h: u32) -> Result<GrayImage, NodeError> {
        let (width, height) = mask.dimensions();
        let packs_w = roi_w.div_ceil(4);
        let packed_len = (packs_w as u64).saturating_mul(roi_h as u64).saturating_mul(4);
        let packed = BufferOut::write_bytes(packed_len);
        let pack_params = MaskPackParams { width, height, roi_x, roi_y, roi_w, roi_h, packs_w, _pad: 0 };
        let pack_bindings = MaskPackBindings { input: mask, packed, params: Uniform::new(pack_params) };
        let packed_out = ctx.dispatch_bindings(&pack_bindings, None, None, Some([packs_w, roi_h, 1])).map_err(|e| NodeError::Handler(format!("contours_compact (gpu pack): {e}")))?;
        let packed_bytes = packed_out.buffers.get(&1).ok_or_else(|| NodeError::Handler("contours_compact (gpu pack): missing readback".into()))?;
        if packed_bytes.len() < packed_len as usize {
            return Err(NodeError::Handler("contours_compact (gpu pack): short readback".into()));
        }

        let mut out = Vec::with_capacity((roi_w * roi_h) as usize);
        for y in 0..roi_h {
            for pack_x in 0..packs_w {
                let idx = (y * packs_w + pack_x) as usize;
                let offset = idx * 4;
                let word = u32::from_le_bytes([packed_bytes[offset], packed_bytes[offset + 1], packed_bytes[offset + 2], packed_bytes[offset + 3]]);
                for i in 0..4u32 {
                    let px = pack_x * 4 + i;
                    if px >= roi_w {
                        break;
                    }
                    let v = ((word >> (i * 8)) & 0xFF) as u8;
                    out.push(v);
                }
            }
        }

        GrayImage::from_raw(roi_w, roi_h, out).ok_or_else(|| NodeError::Handler("contours_compact (gpu pack): invalid image dimensions".into()))
    }

    #[cfg_attr(
        feature = "gpu",
        node(
            id = "contours_compact",
            summary = "Extract contours from a mask with compact GPU readback.",
            description = "Uses a GPU tile-reduce pass to find an active ROI, then packs only that region into a 1-byte-per-pixel buffer before CPU contour tracing.",
            compute(ComputeAffinity::GpuPreferred),
            inputs(
                port(name = "mask", description = "Binary/grayscale mask image (foreground > 0)."),
                port(name = "tile", default = 16i64, meta(ui_min = 1, ui_max = 64, ui_step = 1)),
                port(name = "mode", default = "auto")
            ),
            outputs(port(
                name = "contours",
                source = "Contours",
                ty = crate::daedalus_types::contours(),
                description = "List of contours; each contour is a list of points."
            )),
            shaders(MaskTileReduceBindings, MaskPackBindings)
        )
    )]
    #[cfg_attr(
        not(feature = "gpu"),
        node(
            id = "contours_compact",
            summary = "Extract contours from a mask with compact GPU readback.",
            description = "Uses a GPU tile-reduce pass to find an active ROI, then packs only that region into a 1-byte-per-pixel buffer before CPU contour tracing.",
            inputs(
                port(name = "mask", description = "Binary/grayscale mask image (foreground > 0)."),
                port(name = "tile", default = 16i64, meta(ui_min = 1, ui_max = 64, ui_step = 1)),
                port(name = "mode", default = "auto")
            ),
            outputs(port(
                name = "contours",
                source = "Contours",
                ty = crate::daedalus_types::contours(),
                description = "List of contours; each contour is a list of points."
            ))
        )
    )]
    fn cv_find_contours_compact(mask: Compute<DynamicImage>, tile: i64, mode: ExecMode, #[cfg(feature = "gpu")] ctx: ShaderContext, exec_ctx: &ExecutionContext) -> Result<Vec<Vec<Point>>, NodeError> {
        #[cfg(feature = "gpu")]
        {
            if matches!(mode, ExecMode::Gpu) && ctx.gpu.is_none() {
                return Err(NodeError::Handler("contours_compact: GPU requested but unavailable".into()));
            }
            let input_is_gpu = matches!(&mask, Compute::Gpu(_));
            let want_gpu = match mode {
                ExecMode::Gpu => ctx.gpu.is_some(),
                ExecMode::Auto => ctx.gpu.is_some() && input_is_gpu,
                ExecMode::Cpu => false,
            };
            if want_gpu {
                let tile = u32::try_from(tile).unwrap_or(16).max(1);
                if let Some((gray, roi_x, roi_y)) = read_mask_roi_compact(&mask, &ctx, tile)? {
                    let mut contours = find_contours_gray(&gray);
                    if roi_x != 0 || roi_y != 0 {
                        for contour in &mut contours {
                            for p in contour {
                                p.x += roi_x as f64;
                                p.y += roi_y as f64;
                            }
                        }
                    }
                    return Ok(contours);
                }
                return Ok(Vec::new());
            }
        }
        #[cfg(not(feature = "gpu"))]
        {
            let _ = mode;
            let _ = tile;
        }

        let gray = match mask {
            Compute::Cpu(img) => img.to_luma8(),
            Compute::Gpu(handle) => {
                let gpu = exec_ctx.gpu.as_ref().ok_or_else(|| NodeError::Handler("contours_compact: gpu payload missing context".into()))?;
                let bytes = gpu.read_texture(&handle).map_err(|e| NodeError::Handler(format!("contours_compact: {e}")))?;
                let rgba = RgbaImage::from_raw(handle.width, handle.height, bytes).ok_or_else(|| NodeError::Handler("contours_compact: invalid image dimensions".into()))?;
                DynamicImage::ImageRgba8(rgba).to_luma8()
            }
        };
        Ok(find_contours_gray(&gray))
    }

    declare_plugin!(CvContourPlugin, "contour", [cv_find_contours, cv_find_contours_compact, cv_select_largest_contour, cv_approx_poly_dp, cv_approx_contours_dp, cv_contours_to_frame]);
}

// use imageproc::point::Point;

// fn is_convex(polygon: &[Point<u32>]) -> bool {
//     if polygon.len() < 4 {
//         return false;
//     }

//     let mut sign = 0.0;
//     let n = polygon.len();

//     for i in 0..n {
//         let dx1 = (polygon[(i + 2) % n].x - polygon[(i + 1) % n].x) as i32;
//         let dy1 = (polygon[(i + 2) % n].y - polygon[(i + 1) % n].y) as i32;

//         let dx2 = (polygon[i].x - polygon[(i + 1) % n].x) as i32;
//         let dy2 = (polygon[i].y - polygon[(i + 1) % n].y) as i32;

//         let z_cross_product = (dx1 * dy2 - dy1 * dx2) as f32;

//         if i == 0 {
//             sign = z_cross_product;
//         } else {
//             if sign * z_cross_product < 0.0 {
//                 return false;
//             }
//         }
//     }
//     true
// }
