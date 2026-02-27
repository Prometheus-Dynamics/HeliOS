use super::*;

#[cfg_attr(
    feature = "gpu",
    node(
        id = "canny_prep",
        compute(ComputeAffinity::GpuPreferred),
        inputs("frame", port(name = "smoothing", meta(ui_min = 0.0, ui_max = 5.0, ui_step = 0.1)), port(name = "mode", default = "auto")),
        outputs("edges"),
        shaders(SobelShaderBindings, ConvolutionShaderBindings)
    )
)]
#[cfg_attr(
    not(feature = "gpu"),
    node(
        id = "canny_prep",
        compute(ComputeAffinity::GpuPreferred),
        inputs("frame", port(name = "smoothing", meta(ui_min = 0.0, ui_max = 5.0, ui_step = 0.1)), port(name = "mode", default = "auto")),
        outputs("edges")
    )
)]
fn cv_canny_prep(frame: Payload<DynamicImage>, smoothing: f32, mode: ExecMode, #[cfg(feature = "gpu")] ctx: ShaderContext, _exec_ctx: &ExecutionContext) -> Result<Payload<DynamicImage>, NodeError> {
    #[cfg(feature = "gpu")]
    {
        let _ = smoothing;
        let cpu_fallback = || -> Result<Payload<DynamicImage>, NodeError> {
            let (bytes, w, h) = frame.to_rgba_bytes(ctx.gpu.as_ref()).map_err(|e| NodeError::Handler(format!("canny_prep: {e}")))?;
            let rgba = RgbaImage::from_raw(w, h, bytes).ok_or_else(|| NodeError::Handler("canny_prep: invalid image dimensions".into()))?;
            let gray = DynamicImage::ImageRgba8(rgba).to_luma8();
            Ok(Payload::Cpu(DynamicImage::ImageLuma8(canny_prep(&gray))))
        };

        let (width, height) = frame.dimensions();
        if width == 0 || height == 0 {
            return Ok(Payload::Cpu(DynamicImage::new_rgba8(width, height)));
        }

        let want_gpu = matches!(mode, ExecMode::Gpu | ExecMode::Auto) && ctx.gpu.is_some();
        if want_gpu {
            const GAUSSIAN_3X3: [f32; 9] = [1.0, 2.0, 1.0, 2.0, 4.0, 2.0, 1.0, 2.0, 1.0];
            let blurred = gpu_convolution(&frame, GAUSSIAN_3X3, 1.0 / 16.0, 0.0, &ctx).map_err(|e| NodeError::Handler(format!("canny_prep (gpu blur): {e}"))).or_else(|_| cpu_fallback())?;

            return run_sobel_gpu(&blurred, &ctx).map_err(|e| NodeError::Handler(format!("canny_prep (gpu sobel): {e}"))).or_else(|_| cpu_fallback());
        }

        return match mode {
            ExecMode::Cpu => cpu_fallback(),
            ExecMode::Gpu => Err(NodeError::Handler("canny_prep: GPU requested but unavailable".into())),
            ExecMode::Auto => cpu_fallback(),
        };
    }

    #[cfg(not(feature = "gpu"))]
    {
        let _ = (mode, smoothing);
        let frame = expect_cpu(frame, "canny_prep", Some(_exec_ctx))?;
        let gray = frame.to_luma8();
        Ok(Payload::Cpu(DynamicImage::ImageLuma8(canny_prep(&gray))))
    }
}

#[cfg_attr(feature = "gpu", node(id = "emboss", compute(ComputeAffinity::GpuPreferred), inputs("frame", port(name = "mode", default = "auto")), outputs("mask"), shaders(ConvolutionShaderBindings)))]
#[cfg_attr(not(feature = "gpu"), node(id = "emboss", compute(ComputeAffinity::GpuPreferred), inputs("frame", port(name = "mode", default = "auto")), outputs("mask")))]
fn cv_emboss(frame: Payload<DynamicImage>, mode: ExecMode, #[cfg(feature = "gpu")] ctx: ShaderContext, _exec_ctx: &ExecutionContext) -> Result<Payload<DynamicImage>, NodeError> {
    #[cfg(feature = "gpu")]
    const EMBOSS_KERNEL: [f32; 9] = [-2.0, -1.0, 0.0, -1.0, 1.0, 1.0, 0.0, 1.0, 2.0];

    #[cfg(feature = "gpu")]
    {
        return dispatch_convolution(&frame, EMBOSS_KERNEL, 1.0, 128.0, mode, &ctx);
    }

    #[cfg(not(feature = "gpu"))]
    {
        let _ = mode;
        let frame = expect_cpu(frame, "emboss", Some(_exec_ctx))?;
        let gray = frame.to_luma8();
        Ok(Payload::Cpu(DynamicImage::ImageLuma8(emboss(&gray))))
    }
}

#[cfg_attr(feature = "gpu", node(id = "sharpen", compute(ComputeAffinity::GpuPreferred), inputs("frame", port(name = "mode", default = "auto")), outputs("mask"), shaders(ConvolutionShaderBindings)))]
#[cfg_attr(not(feature = "gpu"), node(id = "sharpen", compute(ComputeAffinity::GpuPreferred), inputs("frame", port(name = "mode", default = "auto")), outputs("mask")))]
fn cv_sharpen(frame: Payload<DynamicImage>, mode: ExecMode, #[cfg(feature = "gpu")] ctx: ShaderContext, _exec_ctx: &ExecutionContext) -> Result<Payload<DynamicImage>, NodeError> {
    #[cfg(feature = "gpu")]
    const SHARPEN_KERNEL: [f32; 9] = [0.0, -1.0, 0.0, -1.0, 5.0, -1.0, 0.0, -1.0, 0.0];

    #[cfg(feature = "gpu")]
    {
        return dispatch_convolution(&frame, SHARPEN_KERNEL, 1.0, 0.0, mode, &ctx);
    }

    #[cfg(not(feature = "gpu"))]
    {
        let _ = mode;
        let frame = expect_cpu(frame, "sharpen", Some(_exec_ctx))?;
        let gray = frame.to_luma8();
        Ok(Payload::Cpu(DynamicImage::ImageLuma8(sharpen(&gray))))
    }
}

#[cfg_attr(
    feature = "gpu",
    node(
        id = "laplacian",
        compute(ComputeAffinity::GpuPreferred),
        inputs(
            "frame",
            port(name = "gain", meta(ui_min = 0.0, ui_max = 5.0, ui_step = 0.1)),
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
        id = "laplacian",
        compute(ComputeAffinity::GpuPreferred),
        inputs(
            "frame",
            port(name = "gain", meta(ui_min = 0.0, ui_max = 5.0, ui_step = 0.1)),
            port(name = "bias", meta(ui_min = -128.0, ui_max = 128.0, ui_step = 1.0)),
            port(name = "mode", default = "auto")
        ),
        outputs("mask")
    )
)]
fn cv_laplacian(
    frame: Payload<DynamicImage>,
    gain: f32,
    bias: f32,
    mode: ExecMode,
    #[cfg(feature = "gpu")] ctx: ShaderContext,
    _exec_ctx: &ExecutionContext,
) -> Result<Payload<DynamicImage>, NodeError> {
    let base_kernel: [f32; 9] = [-1.0, -1.0, -1.0, -1.0, 8.0, -1.0, -1.0, -1.0, -1.0];
    let mut kernel = base_kernel;
    for v in kernel.iter_mut() {
        *v *= gain;
    }

    #[cfg(feature = "gpu")]
    {
        return dispatch_convolution(&frame, kernel, 1.0, bias, mode, &ctx);
    }

    #[cfg(not(feature = "gpu"))]
    {
        let _ = mode;
        let frame = expect_cpu(frame, "laplacian", Some(_exec_ctx))?;
        let gray = frame.to_luma8();
        Ok(Payload::Cpu(DynamicImage::ImageLuma8(convolve_gray(&gray, kernel, 1.0, bias))))
    }
}

#[cfg_attr(
    feature = "gpu",
    node(id = "weighted_blur3", compute(ComputeAffinity::GpuPreferred), inputs("frame", "weights", port(name = "mode", default = "auto")), outputs("mask"), shaders(ConvolutionShaderBindings))
)]
#[cfg_attr(not(feature = "gpu"), node(id = "weighted_blur3", compute(ComputeAffinity::GpuPreferred), inputs("frame", "weights", port(name = "mode", default = "auto")), outputs("mask")))]
fn cv_weighted_blur3(
    frame: Payload<DynamicImage>,
    weights: Vec<f32>,
    mode: ExecMode,
    #[cfg(feature = "gpu")] ctx: ShaderContext,
    _exec_ctx: &ExecutionContext,
) -> Result<Payload<DynamicImage>, NodeError> {
    let kernel = to_kernel(&weights).ok_or_else(|| NodeError::InvalidInput("weights must contain 9 values".into()))?;
    let sum: f32 = kernel.iter().copied().sum();
    let factor = if sum.abs() > f32::EPSILON { 1.0 / sum } else { 1.0 };

    #[cfg(feature = "gpu")]
    {
        return dispatch_convolution(&frame, kernel, factor, 0.0, mode, &ctx);
    }

    #[cfg(not(feature = "gpu"))]
    {
        let _ = mode;
        let frame = expect_cpu(frame, "weighted_blur3", Some(_exec_ctx))?;
        let gray = frame.to_luma8();
        Ok(Payload::Cpu(DynamicImage::ImageLuma8(convolve_gray(&gray, kernel, factor, 0.0))))
    }
}
