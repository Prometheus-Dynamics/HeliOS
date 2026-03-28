#[cfg(feature = "gpu")]
use super::*;

#[cfg(feature = "gpu")]
pub(super) fn run_box_blur(frame: &Compute<DynamicImage>, radius: u32, ctx: &ShaderContext) -> Result<Compute<DynamicImage>, NodeError> {
    let (width, height) = frame.dimensions();
    if width == 0 || height == 0 {
        return Ok(Compute::Cpu(DynamicImage::new_rgba8(width, height)));
    }
    let radius = radius.max(1);
    let inv_kernel = 1.0 / ((radius * 2 + 1) as f32);
    let params = BoxBlurParams { width, height, radius, inv_kernel };
    let horiz = BoxBlurHorizontalBindings { input: frame, output: TextureOut::from_input_ctx(frame, ctx), params: Uniform::new(params) };
    let temp = ctx.single(&horiz).dispatch_auto().and_then(|out| out.into_payload_with_ctx(1, ctx, width, height)).map_err(|e| NodeError::Handler(format!("box_blur (gpu horizontal): {e}")))?;

    let vert = BoxBlurVerticalBindings { input: &temp, output: TextureOut::from_input_ctx(frame, ctx), params: Uniform::new(params) };
    ctx.single(&vert).dispatch_auto().and_then(|out| out.into_payload_with_ctx(1, ctx, width, height)).map_err(|e| NodeError::Handler(format!("box_blur (gpu vertical): {e}")))
}

#[cfg(feature = "gpu")]
pub(super) fn run_sobel_gpu(frame: &Compute<DynamicImage>, ctx: &ShaderContext) -> Result<Compute<DynamicImage>, NodeError> {
    let (width, height) = frame.dimensions();
    if width == 0 || height == 0 {
        return Ok(Compute::Cpu(DynamicImage::new_rgba8(width, height)));
    }
    let params = SobelParams { width, height, _pad: [0; 2] };
    let bindings = SobelShaderBindings { input: frame, output: TextureOut::from_input_ctx(frame, ctx), params: Uniform::new(params) };
    ctx.single(&bindings).dispatch_auto().and_then(|out| out.into_payload_with_ctx(1, ctx, width, height)).map_err(|e| NodeError::Handler(format!("sobel (gpu): {e}")))
}

#[cfg(feature = "gpu")]
pub(super) fn gpu_convolution(frame: &Compute<DynamicImage>, kernel: [f32; 9], factor: f32, bias: f32, ctx: &ShaderContext) -> Result<Compute<DynamicImage>, NodeError> {
    let (width, height) = frame.dimensions();
    if width == 0 || height == 0 {
        return Ok(Compute::Cpu(DynamicImage::new_rgba8(width, height)));
    }
    let kernel_rows = [[kernel[0], kernel[1], kernel[2], 0.0], [kernel[3], kernel[4], kernel[5], 0.0], [kernel[6], kernel[7], kernel[8], 0.0]];
    let params = ConvolutionParams { factor, bias, _pad: [0.0; 2], kernel: kernel_rows };
    let bindings = ConvolutionShaderBindings { input: frame, output: TextureOut::from_input_ctx(frame, ctx), params: Uniform::new(params) };

    ctx.single(&bindings).dispatch_auto().and_then(|out| out.into_payload_with_ctx(1, ctx, width, height)).map_err(|e| NodeError::Handler(format!("convolution (gpu): {e}")))
}

#[cfg(feature = "gpu")]
pub(super) fn run_morph(mask: &Compute<DynamicImage>, norm: MorphNorm, k: u32, op: u32, ctx: &ShaderContext) -> Result<Compute<DynamicImage>, NodeError> {
    let (width, height) = mask.dimensions();
    if width == 0 || height == 0 {
        return Ok(Compute::Cpu(DynamicImage::new_rgba8(width, height)));
    }
    let params = MorphParams {
        radius: k.max(1),
        norm: match norm {
            MorphNorm::L1 => 1,
            MorphNorm::L2 => 2,
            MorphNorm::Linf => 0,
        },
        op,
        _pad: 0,
    };
    let bindings = MorphShaderBindings { input: mask, output: TextureOut::from_input_ctx(mask, ctx), params: Uniform::new(params) };
    ctx.single(&bindings).dispatch_auto().and_then(|out| out.into_payload_with_ctx(1, ctx, width, height)).map_err(|e| NodeError::Handler(format!("morph (gpu): {e}")))
}

#[cfg(feature = "gpu")]
pub(super) fn run_morph_diff(a: &Compute<DynamicImage>, b: &Compute<DynamicImage>, ctx: &ShaderContext) -> Result<Compute<DynamicImage>, NodeError> {
    let (width, height) = a.dimensions();
    if (width, height) != b.dimensions() {
        return Err(NodeError::InvalidInput("morph diff: size mismatch".into()));
    }
    let bindings = MorphDiffShaderBindings { a, b, output: TextureOut::from_input_ctx(a, ctx) };
    ctx.single(&bindings).dispatch_auto().and_then(|out| out.into_payload_with_ctx(2, ctx, width, height)).map_err(|e| NodeError::Handler(format!("morph diff (gpu): {e}")))
}

#[cfg(feature = "gpu")]
pub(super) fn cpu_convolution_from_payload(frame: &Compute<DynamicImage>, kernel: [f32; 9], factor: f32, bias: f32, label: &str, ctx: &ShaderContext) -> Result<Compute<DynamicImage>, NodeError> {
    let (bytes, w, h) = frame.to_rgba_bytes(ctx.gpu.as_ref()).map_err(|e| NodeError::Handler(format!("{label}: {e}")))?;
    let rgba = RgbaImage::from_raw(w, h, bytes).ok_or_else(|| NodeError::Handler(format!("{label}: invalid image dimensions")))?;
    let gray = DynamicImage::ImageRgba8(rgba).to_luma8();
    Ok(Compute::Cpu(DynamicImage::ImageLuma8(convolve_gray(&gray, kernel, factor, bias))))
}

#[cfg(feature = "gpu")]
pub(super) fn dispatch_convolution(frame: &Compute<DynamicImage>, kernel: [f32; 9], factor: f32, bias: f32, mode: ExecMode, ctx: &ShaderContext) -> Result<Compute<DynamicImage>, NodeError> {
    let cpu_fallback = || cpu_convolution_from_payload(frame, kernel, factor, bias, "convolution", ctx);
    let want_gpu = matches!(mode, ExecMode::Gpu | ExecMode::Auto) && ctx.gpu.is_some();
    if want_gpu {
        return gpu_convolution(frame, kernel, factor, bias, ctx).or_else(|_| cpu_fallback());
    }
    match mode {
        ExecMode::Cpu => cpu_fallback(),
        ExecMode::Gpu => Err(NodeError::Handler("convolution: GPU requested but unavailable".into())),
        ExecMode::Auto => cpu_fallback(),
    }
}

pub(super) fn ensure_odd_window(value: u32) -> u32 {
    let candidate = if value < 3 { 3 } else { value };
    if candidate % 2 == 0 { candidate + 1 } else { candidate }
}

pub(super) fn to_kernel(values: &[f32]) -> Option<[f32; 9]> {
    if values.len() != 9 {
        return None;
    }
    let mut out = [0f32; 9];
    out.copy_from_slice(&values[..9]);
    Some(out)
}
