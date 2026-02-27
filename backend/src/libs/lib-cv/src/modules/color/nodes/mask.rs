use super::*;

#[cfg_attr(
    feature = "gpu",
    node(id = "rgb_multi_range_mask", compute(ComputeAffinity::GpuPreferred), inputs("frame", "ranges", port(name = "mode", default = "auto")), outputs("mask"), shaders(RgbRangeShaderBindings))
)]
#[cfg_attr(not(feature = "gpu"), node(id = "rgb_multi_range_mask", compute(ComputeAffinity::GpuPreferred), inputs("frame", "ranges", port(name = "mode", default = "auto")), outputs("mask")))]
fn cv_rgb_multi_range_mask(
    frame: Payload<DynamicImage>,
    ranges: Vec<(i32, i32, i32, i32, i32, i32)>,
    mode: ExecMode,
    #[cfg(feature = "gpu")] ctx: ShaderContext,
    _exec_ctx: &ExecutionContext,
) -> Result<Payload<DynamicImage>, NodeError> {
    #[cfg(feature = "gpu")]
    {
        if ranges.is_empty() {
            return Err(NodeError::InvalidInput("rgb_multi_range_mask: at least one range required".into()));
        }
        let cpu_fallback = || -> Result<Payload<DynamicImage>, NodeError> {
            let (bytes, w, h) = frame.to_rgba_bytes(ctx.gpu.as_ref()).map_err(|e| NodeError::Handler(format!("rgb_multi_range_mask: {e}")))?;
            let rgba = RgbaImage::from_raw(w, h, bytes).ok_or_else(|| NodeError::Handler("rgb_multi_range_mask: invalid image dimensions".into()))?;
            let img = DynamicImage::ImageRgba8(rgba);
            Ok(Payload::Cpu(DynamicImage::ImageLuma8(rgb_multi_range_mask(&img, &ranges))))
        };

        let (width, height) = frame.dimensions();
        if width == 0 || height == 0 {
            return Ok(Payload::Cpu(DynamicImage::new_rgba8(width, height)));
        }

        let want_gpu = matches!(mode, ExecMode::Gpu | ExecMode::Auto) && ctx.gpu.is_some();
        if want_gpu {
            let packed: Vec<RgbRange> = ranges.iter().map(|(r0, r1, g0, g1, b0, b1)| RgbRange { r_min: *r0, r_max: *r1, g_min: *g0, g_max: *g1, b_min: *b0, b_max: *b1, _pad: [0; 2] }).collect();
            let params = RangeParams { count: packed.len() as u32, _pad: [0; 3] };
            let bindings =
                RgbRangeShaderBindings { input: &frame, output: TextureOut::from_input_ctx(&frame, &ctx), ranges: UniformBytes(bytemuck::cast_slice(packed.as_slice())), params: Uniform::new(params) };

            return ctx
                .single(&bindings)
                .dispatch_auto()
                .and_then(|out| out.into_payload_with_ctx(1, &ctx, width, height))
                .map_err(|e| NodeError::Handler(format!("rgb_multi_range_mask (gpu): {e}")))
                .or_else(|_| cpu_fallback());
        }

        return match mode {
            ExecMode::Cpu => cpu_fallback(),
            ExecMode::Gpu => Err(NodeError::Handler("rgb_multi_range_mask: GPU requested but unavailable".into())),
            ExecMode::Auto => cpu_fallback(),
        };
    }

    #[cfg(not(feature = "gpu"))]
    {
        let _ = mode;
        let frame = expect_cpu(frame, "rgb_multi_range_mask", Some(_exec_ctx))?;
        Ok(Payload::Cpu(DynamicImage::ImageLuma8(rgb_multi_range_mask(&frame, &ranges))))
    }
}

#[cfg_attr(
    feature = "gpu",
    node(id = "hsv_multi_range_mask", compute(ComputeAffinity::GpuPreferred), inputs("frame", "ranges", port(name = "mode", default = "auto")), outputs("mask"), shaders(HsvRangeShaderBindings))
)]
#[cfg_attr(not(feature = "gpu"), node(id = "hsv_multi_range_mask", compute(ComputeAffinity::GpuPreferred), inputs("frame", "ranges", port(name = "mode", default = "auto")), outputs("mask")))]
fn cv_hsv_multi_range_mask(
    frame: Payload<DynamicImage>,
    ranges: Vec<(f32, f32, f32, f32, f32, f32)>,
    mode: ExecMode,
    #[cfg(feature = "gpu")] ctx: ShaderContext,
    _exec_ctx: &ExecutionContext,
) -> Result<Payload<DynamicImage>, NodeError> {
    #[cfg(feature = "gpu")]
    {
        if ranges.is_empty() {
            return Err(NodeError::InvalidInput("hsv_multi_range_mask: at least one range required".into()));
        }
        let cpu_fallback = || -> Result<Payload<DynamicImage>, NodeError> {
            let (bytes, w, h) = frame.to_rgba_bytes(ctx.gpu.as_ref()).map_err(|e| NodeError::Handler(format!("hsv_multi_range_mask: {e}")))?;
            let rgba = RgbaImage::from_raw(w, h, bytes).ok_or_else(|| NodeError::Handler("hsv_multi_range_mask: invalid image dimensions".into()))?;
            let img = DynamicImage::ImageRgba8(rgba);
            Ok(Payload::Cpu(DynamicImage::ImageLuma8(hsv_multi_range_mask(&img, &ranges))))
        };

        let (width, height) = frame.dimensions();
        if width == 0 || height == 0 {
            return Ok(Payload::Cpu(DynamicImage::new_rgba8(width, height)));
        }

        let want_gpu = matches!(mode, ExecMode::Gpu | ExecMode::Auto) && ctx.gpu.is_some();
        if want_gpu {
            let packed: Vec<HsvRange> = ranges.iter().map(|(h0, h1, s0, s1, v0, v1)| HsvRange { h_min: *h0, h_max: *h1, s_min: *s0, s_max: *s1, v_min: *v0, v_max: *v1, _pad: [0.0; 2] }).collect();
            let params = RangeParams { count: packed.len() as u32, _pad: [0; 3] };
            let bindings =
                HsvRangeShaderBindings { input: &frame, output: TextureOut::from_input_ctx(&frame, &ctx), ranges: UniformBytes(bytemuck::cast_slice(packed.as_slice())), params: Uniform::new(params) };

            return ctx
                .single(&bindings)
                .dispatch_auto()
                .and_then(|out| out.into_payload_with_ctx(1, &ctx, width, height))
                .map_err(|e| NodeError::Handler(format!("hsv_multi_range_mask (gpu): {e}")))
                .or_else(|_| cpu_fallback());
        }

        return match mode {
            ExecMode::Cpu => cpu_fallback(),
            ExecMode::Gpu => Err(NodeError::Handler("hsv_multi_range_mask: GPU requested but unavailable".into())),
            ExecMode::Auto => cpu_fallback(),
        };
    }

    #[cfg(not(feature = "gpu"))]
    {
        let _ = mode;
        let frame = expect_cpu(frame, "hsv_multi_range_mask", Some(_exec_ctx))?;
        Ok(Payload::Cpu(DynamicImage::ImageLuma8(hsv_multi_range_mask(&frame, &ranges))))
    }
}

#[cfg_attr(
    feature = "gpu",
    node(id = "apply_mask", compute(ComputeAffinity::GpuPreferred), inputs("frame", "mask", port(name = "mode", default = "auto")), outputs("frame"), shaders(ApplyMaskShaderBindings))
)]
#[cfg_attr(not(feature = "gpu"), node(id = "apply_mask", compute(ComputeAffinity::GpuPreferred), inputs("frame", "mask", port(name = "mode", default = "auto")), outputs("frame")))]
fn cv_apply_mask(
    frame: Payload<DynamicImage>,
    mask: Payload<DynamicImage>,
    mode: ExecMode,
    #[cfg(feature = "gpu")] ctx: ShaderContext,
    _exec_ctx: &ExecutionContext,
) -> Result<Payload<DynamicImage>, NodeError> {
    #[cfg(feature = "gpu")]
    {
        let cpu_fallback = || -> Result<Payload<DynamicImage>, NodeError> {
            let (frame_bytes, fw, fh) = frame.to_rgba_bytes(ctx.gpu.as_ref()).map_err(|e| NodeError::Handler(format!("apply_mask: {e}")))?;
            let (mask_bytes, mw, mh) = mask.to_rgba_bytes(ctx.gpu.as_ref()).map_err(|e| NodeError::Handler(format!("apply_mask: {e}")))?;
            if fw != mw || fh != mh {
                return Err(NodeError::InvalidInput("mask/frame size mismatch".into()));
            }
            let frame_rgba = RgbaImage::from_raw(fw, fh, frame_bytes).ok_or_else(|| NodeError::Handler("apply_mask: invalid frame dimensions".into()))?;
            let mask_rgba = RgbaImage::from_raw(mw, mh, mask_bytes).ok_or_else(|| NodeError::Handler("apply_mask: invalid mask dimensions".into()))?;
            let mask_gray = DynamicImage::ImageRgba8(mask_rgba).to_luma8();
            Ok(Payload::Cpu(apply_mask(&DynamicImage::ImageRgba8(frame_rgba), &mask_gray)))
        };

        let (width, height) = frame.dimensions();
        let (mw, mh) = mask.dimensions();
        if width == 0 || height == 0 {
            return Ok(Payload::Cpu(DynamicImage::new_rgba8(width, height)));
        }
        if (width, height) != (mw, mh) {
            return Err(NodeError::InvalidInput("mask/frame size mismatch".into()));
        }

        let want_gpu = matches!(mode, ExecMode::Gpu | ExecMode::Auto) && ctx.gpu.is_some();
        if want_gpu {
            let bindings = ApplyMaskShaderBindings { frame: &frame, mask: &mask, output: TextureOut::from_input_ctx(&frame, &ctx) };

            return ctx
                .single(&bindings)
                .dispatch_auto()
                .and_then(|out| out.into_payload_with_ctx(2, &ctx, width, height))
                .map_err(|e| NodeError::Handler(format!("apply_mask (gpu): {e}")))
                .or_else(|_| cpu_fallback());
        }

        return match mode {
            ExecMode::Cpu => cpu_fallback(),
            ExecMode::Gpu => Err(NodeError::Handler("apply_mask: GPU requested but unavailable".into())),
            ExecMode::Auto => cpu_fallback(),
        };
    }

    #[cfg(not(feature = "gpu"))]
    {
        let _ = mode;
        let frame_cpu = expect_cpu(frame, "apply_mask", Some(_exec_ctx))?;
        let mask_cpu = expect_cpu(mask, "apply_mask", Some(_exec_ctx))?;
        let mask_gray = mask_cpu.to_luma8();
        Ok(Payload::Cpu(apply_mask(&frame_cpu, &mask_gray)))
    }
}

#[node(id = "merge_masks", inputs(port(name = "masks")), outputs("mask"))]
fn cv_merge_masks(masks: FanIn<GrayImage>) -> Result<GrayImage, NodeError> {
    let masks = masks.into_vec();
    if masks.is_empty() {
        return Err(NodeError::InvalidInput("at least one mask required".into()));
    }
    Ok(merge_masks(&masks))
}
