use super::*;
use serde::{Deserialize, Serialize};

#[cfg_attr(
    feature = "gpu",
    node(
        id = "binary",
        compute(ComputeAffinity::GpuPreferred),
        inputs("frame", port(name = "threshold", meta(ui_min = 0, ui_max = 255, ui_step = 1)), port(name = "mode", default = "auto")),
        outputs("mask"),
        shaders(BinaryShaderBindings)
    )
)]
#[cfg_attr(
    not(feature = "gpu"),
    node(
        id = "binary",
        compute(ComputeAffinity::GpuPreferred),
        inputs("frame", port(name = "threshold", meta(ui_min = 0, ui_max = 255, ui_step = 1)), port(name = "mode", default = "auto")),
        outputs("mask")
    )
)]
pub(super) fn cv_binary(
    frame: Compute<DynamicImage>,
    threshold: u8,
    mode: ExecMode,
    #[cfg(feature = "gpu")] ctx: ShaderContext,
    _exec_ctx: &ExecutionContext,
) -> Result<Compute<DynamicImage>, NodeError> {
    #[cfg(feature = "gpu")]
    {
        let cpu_fallback = || -> Result<Compute<DynamicImage>, NodeError> {
            let (bytes, w, h) = frame.to_rgba_bytes(ctx.gpu.as_ref()).map_err(|e| NodeError::Handler(format!("binary: {e}")))?;
            let rgba = RgbaImage::from_raw(w, h, bytes).ok_or_else(|| NodeError::Handler("binary: invalid image dimensions".into()))?;
            let img = DynamicImage::ImageRgba8(rgba);
            let mask = binary_image_simd(&img, threshold);
            Ok(Compute::Cpu(DynamicImage::ImageLuma8(mask)))
        };

        let (width, height) = frame.dimensions();
        if width == 0 || height == 0 {
            return Ok(Compute::Cpu(DynamicImage::new_rgba8(width, height)));
        }

        let want_gpu = matches!(mode, ExecMode::Gpu | ExecMode::Auto) && ctx.gpu.is_some();
        if want_gpu {
            let bindings =
                BinaryShaderBindings { input: &frame, output: TextureOut::from_input_ctx(&frame, &ctx), params: Uniform::new(BinaryParams { threshold: (threshold as f32) / 255.0, _pad: [0.0; 3] }) };

            return ctx
                .single(&bindings)
                .dispatch_auto()
                .and_then(|out| out.into_payload_with_ctx(1, &ctx, width, height))
                .map_err(|e| NodeError::Handler(format!("binary (gpu): {e}")))
                .or_else(|_| cpu_fallback());
        }

        return match mode {
            ExecMode::Cpu => cpu_fallback(),
            ExecMode::Gpu => Err(NodeError::Handler("binary: GPU requested but unavailable".into())),
            ExecMode::Auto => cpu_fallback(),
        };
    }

    #[cfg(not(feature = "gpu"))]
    {
        let _ = mode;
        let frame = expect_cpu(frame, "binary", Some(_exec_ctx))?;
        let gray_owned;
        let gray = match &frame {
            DynamicImage::ImageLuma8(gray) => gray,
            _ => {
                gray_owned = frame.to_luma8();
                &gray_owned
            }
        };
        let mask = crate::modules::image::binary::binary_image_gray_simd(gray, threshold);
        Ok(Compute::Cpu(DynamicImage::ImageLuma8(mask)))
    }
}

#[node(
    id = "binary_mask",
    summary = "Fast global threshold (CPU) producing a GrayImage mask.",
    description = "Converts the input frame to grayscale (if needed) and applies a global threshold. This avoids adaptive threshold cost for high-FPS preprocessing pipelines.",
    inputs("frame", port(name = "threshold", default = 90, meta(ui_min = 0, ui_max = 255, ui_step = 1))),
    outputs("mask")
)]
pub(super) fn cv_binary_mask(frame: Compute<DynamicImage>, threshold: i64, _exec_ctx: &ExecutionContext) -> Result<GrayImage, NodeError> {
    let img: DynamicImage = expect_cpu(frame, "binary_mask", Some(_exec_ctx))?;

    let threshold = (threshold.clamp(0, 255)) as u8;
    let gray_owned;
    let gray = match &img {
        DynamicImage::ImageLuma8(gray) => gray,
        _ => {
            gray_owned = img.to_luma8();
            &gray_owned
        }
    };
    #[cfg(target_arch = "aarch64")]
    {
        Ok(crate::modules::image::binary::binary_image_gray_simd(gray, threshold))
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        Ok(crate::modules::image::binary::binary_image_gray_simd(gray, threshold))
    }
}

#[node(id = "otsu", compute(ComputeAffinity::GpuPreferred), inputs("frame"), outputs("mask"))]
pub(super) fn cv_otsu(frame: DynamicImage, exec_ctx: &ExecutionContext) -> Result<GrayImage, NodeError> {
    const STATE_KEY: &str = "cv:image:otsu:state";
    const RECOMPUTE_INTERVAL: u64 = 4;
    const MAX_SIG_DELTA: f64 = 0.03;

    #[derive(Clone, Default, Serialize, Deserialize)]
    struct OtsuState {
        frame_idx: u64,
        last_threshold: u8,
        last_signature_8x8: Option<Vec<u8>>,
    }

    fn signature_8x8(gray: &GrayImage) -> [u8; 64] {
        let (w, h) = gray.dimensions();
        let w = w.max(1);
        let h = h.max(1);
        let raw = gray.as_raw();
        let stride = w as usize;
        let mut sig = [0u8; 64];
        for gy in 0..8u32 {
            for gx in 0..8u32 {
                let x = (((gx * 2 + 1) * w) / 16).min(w - 1) as usize;
                let y = (((gy * 2 + 1) * h) / 16).min(h - 1) as usize;
                sig[(gy * 8 + gx) as usize] = raw[y * stride + x];
            }
        }
        sig
    }

    fn signature_delta_norm(a: &[u8], b: &[u8; 64]) -> f64 {
        if a.len() != 64 {
            return 1.0;
        }
        let mut acc = 0.0f64;
        for i in 0..64usize {
            acc += (f64::from(a[i]) - f64::from(b[i])).abs();
        }
        acc / (64.0 * 255.0)
    }

    let mut state: OtsuState = exec_ctx.state.take_native::<OtsuState>(STATE_KEY).or_else(|_| exec_ctx.state.get_checked::<OtsuState>(STATE_KEY)).map_err(NodeError::Handler)?.unwrap_or_default();
    state.frame_idx = state.frame_idx.saturating_add(1);

    let mask = crate::modules::image::luma::with_luma8_frame(&frame, |gray| {
        let sig = signature_8x8(gray);
        let can_reuse = state.last_signature_8x8.as_ref().map(|prev| signature_delta_norm(prev, &sig) <= MAX_SIG_DELTA).unwrap_or(false);
        let periodic_refresh = state.frame_idx.is_multiple_of(RECOMPUTE_INTERVAL);
        let threshold = if can_reuse && !periodic_refresh { state.last_threshold } else { crate::modules::image::binary::otsu_level_gray(gray) };
        state.last_threshold = threshold;
        match state.last_signature_8x8.as_mut() {
            Some(prev) if prev.len() == 64 => prev.copy_from_slice(&sig),
            _ => state.last_signature_8x8 = Some(sig.to_vec()),
        }
        crate::modules::image::binary::binary_image_gray_simd(gray, threshold)
    });

    exec_ctx.state.set_native(STATE_KEY, state).map_err(NodeError::Handler)?;
    Ok(mask)
}

fn adaptive_border_guarded<'a>(gray: &'a GrayImage, border_guard_px: u32) -> std::borrow::Cow<'a, GrayImage> {
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

#[cfg(feature = "gpu")]
fn adaptive_threshold_impl(
    frame: Compute<DynamicImage>,
    window: u32,
    offset: f32,
    threshold_offset: f32,
    border_guard_px: u32,
    invert: bool,
    mode: ExecMode,
    ctx: ShaderContext,
    _exec_ctx: &ExecutionContext,
) -> Result<Compute<DynamicImage>, NodeError> {
    let window = ensure_odd_window(window);
    let radius = window.saturating_sub(1) / 2;
    let combined_offset = offset + threshold_offset;
    let cpu_fallback = || -> Result<Compute<DynamicImage>, NodeError> {
        match &frame {
            Compute::Cpu(DynamicImage::ImageLuma8(gray)) => {
                let guarded = adaptive_border_guarded(gray, border_guard_px);
                Ok(Compute::Cpu(DynamicImage::ImageLuma8(crate::modules::image::binary::adaptive_mean_threshold_fast_with_invert(guarded.as_ref(), window, combined_offset, invert))))
            }
            Compute::Cpu(img) => {
                let gray = img.to_luma8();
                let guarded = adaptive_border_guarded(&gray, border_guard_px);
                Ok(Compute::Cpu(DynamicImage::ImageLuma8(crate::modules::image::binary::adaptive_mean_threshold_fast_with_invert(guarded.as_ref(), window, combined_offset, invert))))
            }
            Compute::Gpu(_) => {
                let (bytes, w, h) = frame.to_rgba_bytes(ctx.gpu.as_ref()).map_err(|e| NodeError::Handler(format!("adaptive_threshold: {e}")))?;
                let rgba = RgbaImage::from_raw(w, h, bytes).ok_or_else(|| NodeError::Handler("adaptive_threshold: invalid image dimensions".into()))?;
                let gray = DynamicImage::ImageRgba8(rgba).to_luma8();
                let guarded = adaptive_border_guarded(&gray, border_guard_px);
                Ok(Compute::Cpu(DynamicImage::ImageLuma8(crate::modules::image::binary::adaptive_mean_threshold_fast_with_invert(guarded.as_ref(), window, combined_offset, invert))))
            }
        }
    };

    let (width, height) = frame.dimensions();
    if width == 0 || height == 0 {
        return Ok(Compute::Cpu(DynamicImage::new_rgba8(width, height)));
    }

    let want_gpu = matches!(mode, ExecMode::Gpu | ExecMode::Auto) && ctx.gpu.is_some();
    if want_gpu {
        let mean = run_box_blur(&frame, radius, &ctx).map_err(|e| NodeError::Handler(format!("adaptive_threshold (gpu blur): {e}"))).or_else(|_| cpu_fallback())?;
        let params = MaskThresholdParams { offset: combined_offset, invert: u32::from(invert), _pad: [0; 2] };
        let bindings = MaskThresholdShaderBindings { input: &frame, mean: &mean, output: TextureOut::from_input_ctx(&frame, &ctx), params: Uniform::new(params) };

        return ctx
            .single(&bindings)
            .dispatch_auto()
            .and_then(|out| out.into_payload_with_ctx(2, &ctx, width, height))
            .map_err(|e| NodeError::Handler(format!("adaptive_threshold (gpu): {e}")))
            .or_else(|_| cpu_fallback());
    }

    match mode {
        ExecMode::Cpu => cpu_fallback(),
        ExecMode::Gpu => Err(NodeError::Handler("adaptive_threshold: GPU requested but unavailable".into())),
        ExecMode::Auto => cpu_fallback(),
    }
}

#[cfg(not(feature = "gpu"))]
fn adaptive_threshold_impl(
    frame: Compute<DynamicImage>,
    window: u32,
    offset: f32,
    threshold_offset: f32,
    border_guard_px: u32,
    invert: bool,
    mode: ExecMode,
    _exec_ctx: &ExecutionContext,
) -> Result<Compute<DynamicImage>, NodeError> {
    let window = ensure_odd_window(window);
    let _ = mode;
    let frame = expect_cpu(frame, "adaptive_threshold", Some(_exec_ctx))?;
    let combined_offset = offset + threshold_offset;
    let mask = with_luma8_frame(&frame, |gray| {
        let guarded = adaptive_border_guarded(gray, border_guard_px);
        crate::modules::image::binary::adaptive_mean_threshold_fast_with_invert(guarded.as_ref(), window, combined_offset, invert)
    });
    Ok(Compute::Cpu(DynamicImage::ImageLuma8(mask)))
}

#[cfg_attr(
    feature = "gpu",
    node(
        id = "adaptive_threshold",
        compute(ComputeAffinity::GpuPreferred),
        inputs(
            "frame",
            port(name = "window", default = 9, meta(ui_min = 3, ui_max = 101, ui_step = 2)),
            port(name = "offset", default = 0.0, meta(ui_min = -50.0, ui_max = 50.0, ui_step = 1.0)),
            port(name = "threshold_offset", default = 0.0, meta(ui_min = -50.0, ui_max = 50.0, ui_step = 1.0)),
            port(name = "border_guard_px", default = 0, meta(ui_min = 0, ui_max = 32, ui_step = 1)),
            port(name = "invert", default = false),
            port(name = "mode", default = "auto")
        ),
        outputs("mask"),
        shaders(BoxBlurHorizontalBindings, BoxBlurVerticalBindings, MaskThresholdShaderBindings)
    )
)]
#[cfg_attr(
    not(feature = "gpu"),
    node(
        id = "adaptive_threshold",
        compute(ComputeAffinity::GpuPreferred),
        inputs(
            "frame",
            port(name = "window", default = 9, meta(ui_min = 3, ui_max = 101, ui_step = 2)),
            port(name = "offset", default = 0.0, meta(ui_min = -50.0, ui_max = 50.0, ui_step = 1.0)),
            port(name = "threshold_offset", default = 0.0, meta(ui_min = -50.0, ui_max = 50.0, ui_step = 1.0)),
            port(name = "border_guard_px", default = 0, meta(ui_min = 0, ui_max = 32, ui_step = 1)),
            port(name = "invert", default = false),
            port(name = "mode", default = "auto")
        ),
        outputs("mask")
    )
)]
pub(super) fn cv_adaptive_threshold(
    frame: Compute<DynamicImage>,
    window: u32,
    offset: f32,
    threshold_offset: f32,
    border_guard_px: u32,
    invert: bool,
    mode: ExecMode,
    #[cfg(feature = "gpu")] ctx: ShaderContext,
    _exec_ctx: &ExecutionContext,
) -> Result<Compute<DynamicImage>, NodeError> {
    #[cfg(feature = "gpu")]
    {
        adaptive_threshold_impl(frame, window, offset, threshold_offset, border_guard_px, invert, mode, ctx, _exec_ctx)
    }
    #[cfg(not(feature = "gpu"))]
    {
        adaptive_threshold_impl(frame, window, offset, threshold_offset, border_guard_px, invert, mode, _exec_ctx)
    }
}

#[cfg_attr(feature = "gpu", node(id = "sobel_edge", compute(ComputeAffinity::GpuPreferred), inputs("frame", port(name = "mode", default = "auto")), outputs("mask"), shaders(SobelShaderBindings)))]
#[cfg_attr(not(feature = "gpu"), node(id = "sobel_edge", compute(ComputeAffinity::GpuPreferred), inputs("frame", port(name = "mode", default = "auto")), outputs("mask")))]
pub(super) fn cv_sobel(frame: Compute<DynamicImage>, mode: ExecMode, #[cfg(feature = "gpu")] ctx: ShaderContext, _exec_ctx: &ExecutionContext) -> Result<Compute<DynamicImage>, NodeError> {
    #[cfg(feature = "gpu")]
    {
        let cpu_fallback = || -> Result<Compute<DynamicImage>, NodeError> {
            let (bytes, w, h) = frame.to_rgba_bytes(ctx.gpu.as_ref()).map_err(|e| NodeError::Handler(format!("sobel: {e}")))?;
            let rgba = RgbaImage::from_raw(w, h, bytes).ok_or_else(|| NodeError::Handler("sobel: invalid image dimensions".into()))?;
            let tmp = DynamicImage::ImageRgba8(rgba);
            let mask = with_luma8_frame(&tmp, sobel_edges);
            Ok(Compute::Cpu(DynamicImage::ImageLuma8(mask)))
        };

        let (width, height) = frame.dimensions();
        if width == 0 || height == 0 {
            return Ok(Compute::Cpu(DynamicImage::new_rgba8(width, height)));
        }

        let want_gpu = matches!(mode, ExecMode::Gpu | ExecMode::Auto) && ctx.gpu.is_some();
        if want_gpu {
            let params = SobelParams { width, height, _pad: [0; 2] };
            let bindings = SobelShaderBindings { input: &frame, output: TextureOut::from_input_ctx(&frame, &ctx), params: Uniform::new(params) };

            return ctx
                .single(&bindings)
                .dispatch_auto()
                .and_then(|out| out.into_payload_with_ctx(1, &ctx, width, height))
                .map_err(|e| NodeError::Handler(format!("sobel (gpu): {e}")))
                .or_else(|_| cpu_fallback());
        }

        return match mode {
            ExecMode::Cpu => cpu_fallback(),
            ExecMode::Gpu => Err(NodeError::Handler("sobel: GPU requested but unavailable".into())),
            ExecMode::Auto => cpu_fallback(),
        };
    }

    #[cfg(not(feature = "gpu"))]
    {
        let _ = mode;
        let frame = expect_cpu(frame, "sobel", Some(_exec_ctx))?;
        let mask = with_luma8_frame(&frame, sobel_edges);
        Ok(Compute::Cpu(DynamicImage::ImageLuma8(mask)))
    }
}

#[cfg_attr(
    feature = "gpu",
    node(
        id = "convolution3x3",
        compute(ComputeAffinity::GpuPreferred),
        inputs(
            "frame",
            "kernel",
            port(name = "factor", meta(ui_min = 0.0, ui_max = 5.0, ui_step = 0.1)),
            port(name = "bias", meta(ui_min = -128.0, ui_max = 128.0, ui_step = 1.0)),
            port(name = "mode", default = "auto")
        ),
        outputs("mask"),
        shaders(ConvolutionShaderBindings)
    )
)]
#[cfg_attr(
    not(feature = "gpu"),
    node(
        id = "convolution3x3",
        compute(ComputeAffinity::GpuPreferred),
        inputs(
            "frame",
            "kernel",
            port(name = "factor", meta(ui_min = 0.0, ui_max = 5.0, ui_step = 0.1)),
            port(name = "bias", meta(ui_min = -128.0, ui_max = 128.0, ui_step = 1.0)),
            port(name = "mode", default = "auto")
        ),
        outputs("mask")
    )
)]
pub(super) fn cv_convolution3x3(
    frame: Compute<DynamicImage>,
    kernel: Vec<f32>,
    factor: f32,
    bias: f32,
    mode: ExecMode,
    #[cfg(feature = "gpu")] ctx: ShaderContext,
    _exec_ctx: &ExecutionContext,
) -> Result<Compute<DynamicImage>, NodeError> {
    let kernel_arr: [f32; 9] = to_kernel(&kernel).ok_or_else(|| NodeError::InvalidInput("kernel must have 9 values".into()))?;

    #[cfg(feature = "gpu")]
    {
        return dispatch_convolution(&frame, kernel_arr, factor, bias, mode, &ctx);
    }

    #[cfg(not(feature = "gpu"))]
    {
        let _ = mode;
        let frame = expect_cpu(frame, "convolution3x3", Some(_exec_ctx))?;
        let gray = frame.to_luma8();
        Ok(Compute::Cpu(DynamicImage::ImageLuma8(convolve_gray(&gray, kernel_arr, factor, bias))))
    }
}

#[cfg_attr(
    feature = "gpu",
    node(
        id = "guided_filter_gray",
        compute(ComputeAffinity::GpuPreferred),
        inputs(
            "mask",
            port(name = "radius", meta(ui_min = 1, ui_max = 64, ui_step = 1)),
            port(name = "epsilon", meta(ui_min = 0.0, ui_max = 0.1, ui_step = 0.001)),
            port(name = "mode", default = "auto")
        ),
        outputs("mask"),
        shaders(GuidedCoeffShaderBindings, GuidedResolveShaderBindings)
    )
)]
#[cfg_attr(
    not(feature = "gpu"),
    node(
        id = "guided_filter_gray",
        compute(ComputeAffinity::GpuPreferred),
        inputs(
            "mask",
            port(name = "radius", meta(ui_min = 1, ui_max = 64, ui_step = 1)),
            port(name = "epsilon", meta(ui_min = 0.0, ui_max = 0.1, ui_step = 0.001)),
            port(name = "mode", default = "auto")
        ),
        outputs("mask")
    )
)]
pub(super) fn cv_guided_filter(
    mask: Compute<DynamicImage>,
    radius: u32,
    epsilon: f32,
    mode: ExecMode,
    #[cfg(feature = "gpu")] ctx: ShaderContext,
    _exec_ctx: &ExecutionContext,
) -> Result<Compute<DynamicImage>, NodeError> {
    #[cfg(feature = "gpu")]
    {
        let r = radius.max(1);
        let cpu_fallback = || -> Result<Compute<DynamicImage>, NodeError> {
            let (bytes, w, h) = mask.to_rgba_bytes(ctx.gpu.as_ref()).map_err(|e| NodeError::Handler(format!("guided_filter_gray: {e}")))?;
            let rgba = RgbaImage::from_raw(w, h, bytes).ok_or_else(|| NodeError::Handler("guided_filter_gray: invalid image dimensions".into()))?;
            let gray = DynamicImage::ImageRgba8(rgba).to_luma8();
            Ok(Compute::Cpu(DynamicImage::ImageLuma8(guided_filter_gray(&gray, r, epsilon))))
        };

        let (width, height) = mask.dimensions();
        if width == 0 || height == 0 {
            return Ok(Compute::Cpu(DynamicImage::new_rgba8(width, height)));
        }

        let want_gpu = matches!(mode, ExecMode::Gpu | ExecMode::Auto) && ctx.gpu.is_some();
        if want_gpu {
            let params = GuidedParams { radius: r, epsilon, _pad: [0.0; 2] };
            let coeff_bindings = GuidedCoeffShaderBindings { input: &mask, output: TextureOut::write(width, height), params: Uniform::new(params) };

            let coeff_out = match ctx.single(&coeff_bindings).dispatch_auto() {
                Ok(out) => out,
                Err(e) => {
                    return cpu_fallback().map_err(|_| NodeError::Handler(format!("guided_filter_gray (gpu coeff): {e}")));
                }
            };

            if let Some(coeff_handle) = coeff_out.texture_handle(1) {
                let coeff_payload = Compute::Gpu(coeff_handle);
                let resolve_bindings = GuidedResolveShaderBindings { input: &mask, coeff: &coeff_payload, output: TextureOut::from_input_ctx(&mask, &ctx), params: Uniform::new(params) };

                return ctx
                    .single(&resolve_bindings)
                    .dispatch_auto()
                    .and_then(|out| out.into_payload_with_ctx(2, &ctx, width, height))
                    .map_err(|e| NodeError::Handler(format!("guided_filter_gray (gpu resolve): {e}")))
                    .or_else(|_| cpu_fallback());
            }
        }

        return match mode {
            ExecMode::Cpu => cpu_fallback(),
            ExecMode::Gpu => Err(NodeError::Handler("guided_filter_gray: GPU requested but unavailable".into())),
            ExecMode::Auto => cpu_fallback(),
        };
    }

    #[cfg(not(feature = "gpu"))]
    {
        let _ = mode;
        let r = radius.max(1);
        let mask = expect_cpu(mask, "guided_filter_gray", Some(_exec_ctx))?;
        let gray = mask.to_luma8();
        Ok(Compute::Cpu(DynamicImage::ImageLuma8(guided_filter_gray(&gray, r, epsilon))))
    }
}

#[node(id = "otsu_level", inputs("frame"), outputs("threshold"))]
pub(super) fn cv_otsu_level(frame: DynamicImage) -> Result<u32, NodeError> {
    Ok(otsu_level(&frame) as u32)
}
