use super::*;

#[cfg_attr(
    feature = "gpu",
    node(
        id = "brightness",
        compute(ComputeAffinity::GpuPreferred),
        inputs("frame", port(name = "value", meta(ui_min = -255, ui_max = 255, ui_step = 1)), port(name = "mode", default = "auto")),
        outputs("frame"),
        shaders(BrightnessShaderBindings)
    )
)]
#[cfg_attr(
    not(feature = "gpu"),
    node(id = "brightness", compute(ComputeAffinity::GpuPreferred), inputs("frame", port(name = "value", meta(ui_min = -255, ui_max = 255, ui_step = 1)), port(name = "mode", default = "auto")), outputs("frame"))
)]
fn cv_brightness(frame: Payload<DynamicImage>, value: i32, mode: ExecMode, #[cfg(feature = "gpu")] ctx: ShaderContext, _exec_ctx: &ExecutionContext) -> Result<Payload<DynamicImage>, NodeError> {
    if value == 0 {
        return Ok(frame);
    }
    #[cfg(feature = "gpu")]
    {
        let cpu_fallback = || -> Result<Payload<DynamicImage>, NodeError> {
            Ok(Payload::Cpu(
                frame
                    .to_rgba_bytes(ctx.gpu.as_ref())
                    .map_err(|e| NodeError::Handler(format!("brightness: {e}")))
                    .and_then(|(bytes, w, h)| RgbaImage::from_raw(w, h, bytes).ok_or_else(|| NodeError::Handler("brightness: invalid image dimensions".into())))
                    .map(DynamicImage::ImageRgba8)?
                    .brighten(value),
            ))
        };

        let (width, height) = frame.dimensions();
        if width == 0 || height == 0 {
            return Ok(Payload::Cpu(DynamicImage::new_rgba8(width, height)));
        }

        let want_gpu = matches!(mode, ExecMode::Gpu | ExecMode::Auto) && ctx.gpu.is_some();
        if want_gpu {
            let params = ColorParam { value: (value as f32) / 255.0, _pad: [0.0; 3] };
            let bindings = BrightnessShaderBindings { input: &frame, output: TextureOut::from_input_ctx(&frame, &ctx), params: Uniform::new(params) };
            return ctx
                .single(&bindings)
                .dispatch_auto()
                .and_then(|out| out.into_payload_with_ctx(1, &ctx, width, height))
                .map_err(|e| NodeError::Handler(format!("brightness (gpu): {e}")))
                .or_else(|_| cpu_fallback());
        }

        return match mode {
            ExecMode::Cpu => cpu_fallback(),
            ExecMode::Gpu => Err(NodeError::Handler("brightness: GPU requested but unavailable".into())),
            ExecMode::Auto => cpu_fallback(),
        };
    }

    #[cfg(not(feature = "gpu"))]
    {
        let _ = mode;
        let frame = expect_cpu(frame, "brightness", Some(_exec_ctx))?;
        Ok(Payload::Cpu(frame.brighten(value)))
    }
}

#[cfg_attr(
    feature = "gpu",
    node(
        id = "contrast",
        compute(ComputeAffinity::GpuPreferred),
        inputs("frame", port(name = "value", meta(ui_min = -100.0, ui_max = 100.0, ui_step = 1.0)), port(name = "mode", default = "auto")),
        outputs("frame"),
        shaders(ContrastShaderBindings)
    )
)]
#[cfg_attr(
    not(feature = "gpu"),
    node(id = "contrast", compute(ComputeAffinity::GpuPreferred), inputs("frame", port(name = "value", meta(ui_min = -100.0, ui_max = 100.0, ui_step = 1.0)), port(name = "mode", default = "auto")), outputs("frame"))
)]
fn cv_contrast(frame: Payload<DynamicImage>, value: f32, mode: ExecMode, #[cfg(feature = "gpu")] ctx: ShaderContext, _exec_ctx: &ExecutionContext) -> Result<Payload<DynamicImage>, NodeError> {
    if value == 0.0 {
        return Ok(frame);
    }
    #[cfg(feature = "gpu")]
    {
        let cpu_fallback = || -> Result<Payload<DynamicImage>, NodeError> {
            Ok(Payload::Cpu(
                frame
                    .to_rgba_bytes(ctx.gpu.as_ref())
                    .map_err(|e| NodeError::Handler(format!("contrast: {e}")))
                    .and_then(|(bytes, w, h)| RgbaImage::from_raw(w, h, bytes).ok_or_else(|| NodeError::Handler("contrast: invalid image dimensions".into())))
                    .map(DynamicImage::ImageRgba8)?
                    .adjust_contrast(value),
            ))
        };

        let (width, height) = frame.dimensions();
        if width == 0 || height == 0 {
            return Ok(Payload::Cpu(DynamicImage::new_rgba8(width, height)));
        }

        let want_gpu = matches!(mode, ExecMode::Gpu | ExecMode::Auto) && ctx.gpu.is_some();
        if want_gpu {
            let params = ColorParam { value, _pad: [0.0; 3] };
            let bindings = ContrastShaderBindings { input: &frame, output: TextureOut::from_input_ctx(&frame, &ctx), params: Uniform::new(params) };
            return ctx
                .single(&bindings)
                .dispatch_auto()
                .and_then(|out| out.into_payload_with_ctx(1, &ctx, width, height))
                .map_err(|e| NodeError::Handler(format!("contrast (gpu): {e}")))
                .or_else(|_| cpu_fallback());
        }

        return match mode {
            ExecMode::Cpu => cpu_fallback(),
            ExecMode::Gpu => Err(NodeError::Handler("contrast: GPU requested but unavailable".into())),
            ExecMode::Auto => cpu_fallback(),
        };
    }

    #[cfg(not(feature = "gpu"))]
    {
        let _ = mode;
        let frame = expect_cpu(frame, "contrast", Some(_exec_ctx))?;
        Ok(Payload::Cpu(frame.adjust_contrast(value)))
    }
}

#[cfg_attr(
    feature = "gpu",
    node(
        id = "hue",
        compute(ComputeAffinity::GpuPreferred),
        inputs("frame", port(name = "degrees", meta(ui_min = -180, ui_max = 180, ui_step = 1)), port(name = "mode", default = "auto")),
        outputs("frame"),
        shaders(HueShaderBindings)
    )
)]
#[cfg_attr(
    not(feature = "gpu"),
    node(id = "hue", compute(ComputeAffinity::GpuPreferred), inputs("frame", port(name = "degrees", meta(ui_min = -180, ui_max = 180, ui_step = 1)), port(name = "mode", default = "auto")), outputs("frame"))
)]
fn cv_hue(frame: Payload<DynamicImage>, degrees: i32, mode: ExecMode, #[cfg(feature = "gpu")] ctx: ShaderContext, _exec_ctx: &ExecutionContext) -> Result<Payload<DynamicImage>, NodeError> {
    if degrees == 0 {
        return Ok(frame);
    }
    #[cfg(feature = "gpu")]
    {
        let cpu_fallback = || -> Result<Payload<DynamicImage>, NodeError> {
            Ok(Payload::Cpu(
                frame
                    .to_rgba_bytes(ctx.gpu.as_ref())
                    .map_err(|e| NodeError::Handler(format!("hue: {e}")))
                    .and_then(|(bytes, w, h)| RgbaImage::from_raw(w, h, bytes).ok_or_else(|| NodeError::Handler("hue: invalid image dimensions".into())))
                    .map(DynamicImage::ImageRgba8)?
                    .huerotate(degrees),
            ))
        };

        let (width, height) = frame.dimensions();
        if width == 0 || height == 0 {
            return Ok(Payload::Cpu(DynamicImage::new_rgba8(width, height)));
        }

        let want_gpu = matches!(mode, ExecMode::Gpu | ExecMode::Auto) && ctx.gpu.is_some();
        if want_gpu {
            let params = ColorParam { value: degrees as f32, _pad: [0.0; 3] };
            let bindings = HueShaderBindings { input: &frame, output: TextureOut::from_input_ctx(&frame, &ctx), params: Uniform::new(params) };
            return ctx
                .single(&bindings)
                .dispatch_auto()
                .and_then(|out| out.into_payload_with_ctx(1, &ctx, width, height))
                .map_err(|e| NodeError::Handler(format!("hue (gpu): {e}")))
                .or_else(|_| cpu_fallback());
        }

        return match mode {
            ExecMode::Cpu => cpu_fallback(),
            ExecMode::Gpu => Err(NodeError::Handler("hue: GPU requested but unavailable".into())),
            ExecMode::Auto => cpu_fallback(),
        };
    }

    #[cfg(not(feature = "gpu"))]
    {
        let _ = mode;
        let frame = expect_cpu(frame, "hue", Some(_exec_ctx))?;
        Ok(Payload::Cpu(frame.huerotate(degrees)))
    }
}

#[cfg_attr(
    feature = "gpu",
    node(
        id = "saturation",
        compute(ComputeAffinity::GpuPreferred),
        inputs("frame", port(name = "factor", meta(ui_min = 0.0, ui_max = 3.0, ui_step = 0.05)), port(name = "mode", default = "auto")),
        outputs("frame"),
        shaders(SaturationShaderBindings)
    )
)]
#[cfg_attr(
    not(feature = "gpu"),
    node(
        id = "saturation",
        compute(ComputeAffinity::GpuPreferred),
        inputs("frame", port(name = "factor", meta(ui_min = 0.0, ui_max = 3.0, ui_step = 0.05)), port(name = "mode", default = "auto")),
        outputs("frame")
    )
)]
fn cv_saturation(frame: Payload<DynamicImage>, factor: f32, mode: ExecMode, #[cfg(feature = "gpu")] ctx: ShaderContext, _exec_ctx: &ExecutionContext) -> Result<Payload<DynamicImage>, NodeError> {
    if (factor - 1.0).abs() <= f32::EPSILON {
        return Ok(frame);
    }
    #[cfg(feature = "gpu")]
    {
        let cpu_fallback = || -> Result<Payload<DynamicImage>, NodeError> {
            let mut img: RgbaImage = frame
                .to_rgba_bytes(ctx.gpu.as_ref())
                .map_err(|e| NodeError::Handler(format!("saturation: {e}")))
                .and_then(|(bytes, w, h)| RgbaImage::from_raw(w, h, bytes).ok_or_else(|| NodeError::Handler("saturation: invalid image dimensions".into())))?;
            let buf = img.as_flat_samples_mut().samples;
            const LANES: usize = 8;
            let simd_width = 4 * LANES;
            let factor_v = f32x8::splat(factor.max(0.0));
            let r_c = f32x8::splat(0.299);
            let g_c = f32x8::splat(0.587);
            let b_c = f32x8::splat(0.114);

            buf.par_chunks_mut(simd_width).for_each(|chunk| {
                if chunk.len() < simd_width {
                    for px in chunk.chunks_mut(4) {
                        let r = px[0] as f32;
                        let g = px[1] as f32;
                        let b = px[2] as f32;
                        let gray = 0.299 * r + 0.587 * g + 0.114 * b;
                        px[0] = ((gray + (r - gray) * factor).clamp(0.0, 255.0)) as u8;
                        px[1] = ((gray + (g - gray) * factor).clamp(0.0, 255.0)) as u8;
                        px[2] = ((gray + (b - gray) * factor).clamp(0.0, 255.0)) as u8;
                    }
                    return;
                }

                let mut r = [0.0; LANES];
                let mut g = [0.0; LANES];
                let mut b = [0.0; LANES];
                for lane in 0..LANES {
                    r[lane] = chunk[4 * lane] as f32;
                    g[lane] = chunk[4 * lane + 1] as f32;
                    b[lane] = chunk[4 * lane + 2] as f32;
                }

                let rv = f32x8::from(r);
                let gv = f32x8::from(g);
                let bv = f32x8::from(b);
                let gray = rv * r_c + gv * g_c + bv * b_c;

                let rr = (gray + (rv - gray) * factor_v).max(f32x8::splat(0.0)).min(f32x8::splat(255.0));
                let gg = (gray + (gv - gray) * factor_v).max(f32x8::splat(0.0)).min(f32x8::splat(255.0));
                let bb = (gray + (bv - gray) * factor_v).max(f32x8::splat(0.0)).min(f32x8::splat(255.0));

                let rr_a = rr.to_array();
                let gg_a = gg.to_array();
                let bb_a = bb.to_array();
                for lane in 0..LANES {
                    chunk[4 * lane] = rr_a[lane].round() as u8;
                    chunk[4 * lane + 1] = gg_a[lane].round() as u8;
                    chunk[4 * lane + 2] = bb_a[lane].round() as u8;
                }
            });

            Ok(Payload::Cpu(DynamicImage::ImageRgba8(img)))
        };

        let (width, height) = frame.dimensions();
        if width == 0 || height == 0 {
            return Ok(Payload::Cpu(DynamicImage::new_rgba8(width, height)));
        }

        let want_gpu = matches!(mode, ExecMode::Gpu | ExecMode::Auto) && ctx.gpu.is_some();
        if want_gpu {
            let params = ColorParam { value: factor.max(0.0), _pad: [0.0; 3] };
            let bindings = SaturationShaderBindings { input: &frame, output: TextureOut::from_input_ctx(&frame, &ctx), params: Uniform::new(params) };
            return ctx
                .single(&bindings)
                .dispatch_auto()
                .and_then(|out| out.into_payload_with_ctx(1, &ctx, width, height))
                .map_err(|e| NodeError::Handler(format!("saturation (gpu): {e}")))
                .or_else(|_| cpu_fallback());
        }

        return match mode {
            ExecMode::Cpu => cpu_fallback(),
            ExecMode::Gpu => Err(NodeError::Handler("saturation: GPU requested but unavailable".into())),
            ExecMode::Auto => cpu_fallback(),
        };
    }

    #[cfg(not(feature = "gpu"))]
    {
        let _ = mode;
        let frame = expect_cpu(frame, "saturation", Some(_exec_ctx))?;
        let mut img: RgbaImage = frame.to_rgba8();
        let buf = img.as_flat_samples_mut().samples;
        const LANES: usize = 8;
        let simd_width = 4 * LANES;
        let factor_v = f32x8::splat(factor.max(0.0));
        let r_c = f32x8::splat(0.299);
        let g_c = f32x8::splat(0.587);
        let b_c = f32x8::splat(0.114);

        buf.par_chunks_mut(simd_width).for_each(|chunk| {
            if chunk.len() < simd_width {
                for px in chunk.chunks_mut(4) {
                    let r = px[0] as f32;
                    let g = px[1] as f32;
                    let b = px[2] as f32;
                    let gray = 0.299 * r + 0.587 * g + 0.114 * b;
                    px[0] = ((gray + (r - gray) * factor).clamp(0.0, 255.0)) as u8;
                    px[1] = ((gray + (g - gray) * factor).clamp(0.0, 255.0)) as u8;
                    px[2] = ((gray + (b - gray) * factor).clamp(0.0, 255.0)) as u8;
                }
                return;
            }

            let mut r = [0.0; LANES];
            let mut g = [0.0; LANES];
            let mut b = [0.0; LANES];
            for lane in 0..LANES {
                r[lane] = chunk[4 * lane] as f32;
                g[lane] = chunk[4 * lane + 1] as f32;
                b[lane] = chunk[4 * lane + 2] as f32;
            }

            let rv = f32x8::from(r);
            let gv = f32x8::from(g);
            let bv = f32x8::from(b);
            let gray = rv * r_c + gv * g_c + bv * b_c;

            let rr = (gray + (rv - gray) * factor_v).max(f32x8::splat(0.0)).min(f32x8::splat(255.0));
            let gg = (gray + (gv - gray) * factor_v).max(f32x8::splat(0.0)).min(f32x8::splat(255.0));
            let bb = (gray + (bv - gray) * factor_v).max(f32x8::splat(0.0)).min(f32x8::splat(255.0));

            let rr_a = rr.to_array();
            let gg_a = gg.to_array();
            let bb_a = bb.to_array();
            for lane in 0..LANES {
                chunk[4 * lane] = rr_a[lane].round() as u8;
                chunk[4 * lane + 1] = gg_a[lane].round() as u8;
                chunk[4 * lane + 2] = bb_a[lane].round() as u8;
            }
        });

        Ok(Payload::Cpu(DynamicImage::ImageRgba8(img)))
    }
}

#[cfg_attr(
    feature = "gpu",
    node(
        id = "grayscale",
        summary = "Convert the input frame to a grayscale mask.",
        description = "Produces a single-channel image suitable for thresholding and contour detection. Use GPU mode when available for maximum throughput.",
        compute(ComputeAffinity::GpuPreferred),
        inputs(port(name = "frame", description = "Input image frame."), port(name = "mode", default = "auto", description = "Execution mode hint (auto/cpu/gpu).")),
        outputs(port(name = "mask", description = "Grayscale output image.")),
        shaders(GrayscaleShaderBindings)
    )
)]
#[cfg_attr(
    not(feature = "gpu"),
    node(
        id = "grayscale",
        summary = "Convert the input frame to a grayscale mask.",
        description = "Produces a single-channel image suitable for thresholding and contour detection.",
        compute(ComputeAffinity::GpuPreferred),
        inputs(port(name = "frame", description = "Input image frame."), port(name = "mode", default = "auto", description = "Execution mode hint (auto/cpu/gpu).")),
        outputs(port(name = "mask", description = "Grayscale output image."))
    )
)]
fn cv_grayscale(frame: Payload<DynamicImage>, mode: ExecMode, #[cfg(feature = "gpu")] ctx: ShaderContext, _exec_ctx: &ExecutionContext) -> Result<Payload<DynamicImage>, NodeError> {
    #[cfg(feature = "gpu")]
    {
        fn to_luma8_fast(img: &DynamicImage) -> GrayImage {
            match img {
                DynamicImage::ImageLuma8(gray) => gray.clone(),
                DynamicImage::ImageRgb8(rgb) => rgb8_to_luma8_exact(rgb),
                DynamicImage::ImageRgba8(rgba) => rgba8_to_luma8_exact(rgba),
                other => other.to_luma8(),
            }
        }

        let cpu_fallback = || -> Result<Payload<DynamicImage>, NodeError> {
            match &frame {
                Payload::Cpu(img) => {
                    let out = match img {
                        DynamicImage::ImageLuma8(_) => img.clone(),
                        other => DynamicImage::ImageLuma8(to_luma8_fast(other)),
                    };
                    Ok(Payload::Cpu(out))
                }
                Payload::Gpu(handle) => {
                    let gpu = ctx.gpu.as_ref().ok_or_else(|| NodeError::Handler("grayscale: gpu payload but no gpu context".into()))?;
                    let bytes = gpu.read_texture(handle).map_err(|e| NodeError::Handler(format!("grayscale: read_texture: {e}")))?;
                    let rgba = RgbaImage::from_raw(handle.width, handle.height, bytes).ok_or_else(|| NodeError::Handler("grayscale: invalid image dimensions".into()))?;
                    Ok(Payload::Cpu(DynamicImage::ImageLuma8(rgba8_to_luma8_exact(&rgba))))
                }
            }
        };

        let (width, height) = frame.dimensions();
        if width == 0 || height == 0 {
            return Ok(Payload::Cpu(DynamicImage::new_rgba8(width, height)));
        }

        let want_gpu = matches!(mode, ExecMode::Gpu | ExecMode::Auto) && ctx.gpu.is_some();
        if want_gpu {
            let params = ColorParam { value: 0.0, _pad: [0.0; 3] };
            let bindings = GrayscaleShaderBindings { input: &frame, output: TextureOut::from_input_ctx(&frame, &ctx), params: Uniform::new(params) };
            return ctx
                .single(&bindings)
                .dispatch_auto()
                .and_then(|out| out.into_payload_with_ctx(1, &ctx, width, height))
                .map_err(|e| NodeError::Handler(format!("grayscale (gpu): {e}")))
                .or_else(|_| cpu_fallback());
        }

        return match mode {
            ExecMode::Cpu => cpu_fallback(),
            ExecMode::Gpu => Err(NodeError::Handler("grayscale: GPU requested but unavailable".into())),
            ExecMode::Auto => cpu_fallback(),
        };
    }

    #[cfg(not(feature = "gpu"))]
    {
        let _ = mode;
        let frame = expect_cpu(frame, "grayscale", Some(_exec_ctx))?;
        let out = match frame {
            DynamicImage::ImageLuma8(_) => frame,
            other => DynamicImage::ImageLuma8(match &other {
                DynamicImage::ImageRgb8(rgb) => rgb8_to_luma8_exact(rgb),
                DynamicImage::ImageRgba8(rgba) => rgba8_to_luma8_exact(rgba),
                _ => other.to_luma8(),
            }),
        };
        Ok(Payload::Cpu(out))
    }
}

// Exact sRGB -> luma conversion matching `image` crate `to_luma8()`:
// y = floor((0.2126*r + 0.7152*g + 0.0722*b) * 255) using integer weights / 10000.
#[inline(always)]
fn div_10000_u32_exact_for_luma(sum: u32) -> u32 {
    // `sum` here is bounded by 10000*255 (= 2,550,000).
    // For this range, the following multiply+shift is exactly equal to `sum / 10000`,
    // but avoids a hardware integer division on AArch64.
    //
    // Verified for all sum in [0, 2_550_000]:
    //   (sum * 1_717_987) >> 34 == sum / 10000
    ((sum as u64 * 1_717_987u64) >> 34) as u32
}

#[inline]
fn rgb8_to_luma8_exact(rgb: &RgbImage) -> GrayImage {
    let (w, h) = rgb.dimensions();
    let mut out = GrayImage::new(w, h);
    let src = rgb.as_raw();
    let dst = out.as_flat_samples_mut().samples;
    debug_assert_eq!(src.len(), dst.len().saturating_mul(3));

    for (i, px) in src.chunks_exact(3).enumerate() {
        let r = px[0] as u32;
        let g = px[1] as u32;
        let b = px[2] as u32;
        let sum = 2126u32 * r + 7152u32 * g + 722u32 * b;
        dst[i] = div_10000_u32_exact_for_luma(sum) as u8;
    }
    out
}

#[inline]
fn rgba8_to_luma8_exact(rgba: &RgbaImage) -> GrayImage {
    let (w, h) = rgba.dimensions();
    let mut out = GrayImage::new(w, h);
    let src = rgba.as_raw();
    let dst = out.as_flat_samples_mut().samples;
    debug_assert_eq!(src.len(), dst.len().saturating_mul(4));

    for (i, px) in src.chunks_exact(4).enumerate() {
        let r = px[0] as u32;
        let g = px[1] as u32;
        let b = px[2] as u32;
        let sum = 2126u32 * r + 7152u32 * g + 722u32 * b;
        dst[i] = div_10000_u32_exact_for_luma(sum) as u8;
    }
    out
}
