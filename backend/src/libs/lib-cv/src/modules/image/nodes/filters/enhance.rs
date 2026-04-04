use super::*;

// NOTE: `adaptive_threshold_mask` used to exist as a separate node. It was a conversion-only
// variant; Daedalus already handles conversions, so we keep a single `adaptive_threshold`
// node with the superset of parameters.

fn apply_clahe_cached(gray: &GrayImage, tile_size: u32, clip_limit: f32) -> GrayImage {
    // Reusing prepared tiles across different frames can preserve the wrong local histogram
    // layout even when coarse frame stats look "similar". That is enough to change the
    // downstream threshold mask and break tag detection. Always prepare from the current frame.
    let tiles = prepare_clahe(gray, tile_size, clip_limit);
    let out = apply_clahe_with_tiles(gray, &tiles);
    crate::modules::image::clahe::compact_clahe_scratch_after_frame();
    out
}

#[inline]
fn blend_clahe_with_base(base: &GrayImage, enhanced: GrayImage, mix: f32) -> GrayImage {
    let alpha = mix.clamp(0.0, 1.0);
    if alpha >= 0.999 {
        return enhanced;
    }
    if alpha <= 0.001 {
        return base.clone();
    }

    let mut out = GrayImage::new(base.width(), base.height());
    let a = (alpha * 256.0).round().clamp(0.0, 256.0) as u32;
    let ia = 256u32.saturating_sub(a);
    out.as_mut().par_iter_mut().zip(base.as_raw().par_iter().zip(enhanced.as_raw().par_iter())).for_each(|(dst, (&b, &e))| {
        let mixed = (e as u32).saturating_mul(a).saturating_add((b as u32).saturating_mul(ia)).saturating_add(128) >> 8;
        *dst = mixed as u8;
    });
    out
}

#[cfg_attr(
    feature = "gpu",
    node(
        id = "clahe",
        compute(ComputeAffinity::GpuPreferred),
        inputs(
            "mask",
            port(name = "tile_size", meta(ui_min = 1, ui_max = 64, ui_step = 1)),
            port(name = "clip_limit", meta(ui_min = 0.0, ui_max = 10.0, ui_step = 0.1)),
            port(name = "mix", default = 1.0, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01)),
            port(name = "mode", default = "auto")
        ),
        outputs("mask"),
        shaders(ClaheShaderBindings)
    )
)]
#[cfg_attr(
    not(feature = "gpu"),
    node(
        id = "clahe",
        compute(ComputeAffinity::GpuPreferred),
        inputs(
            "mask",
            port(name = "tile_size", meta(ui_min = 1, ui_max = 64, ui_step = 1)),
            port(name = "clip_limit", meta(ui_min = 0.0, ui_max = 10.0, ui_step = 0.1)),
            port(name = "mix", default = 1.0, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01)),
            port(name = "mode", default = "auto")
        ),
        outputs("mask")
    )
)]
pub(super) fn cv_clahe(
    mask: Compute<DynamicImage>,
    tile_size: i64,
    clip_limit: f64,
    mix: f64,
    mode: Option<ExecMode>,
    #[cfg(feature = "gpu")] ctx: ShaderContext,
    _exec_ctx: &ExecutionContext,
) -> Result<Compute<DynamicImage>, NodeError> {
    let tile_size = tile_size.clamp(1, u32::MAX as i64) as u32;
    let clip_limit = (clip_limit.max(0.0) as f32).max(0.0);
    let mix = (mix as f32).clamp(0.0, 1.0);
    let mode = mode.unwrap_or(ExecMode::Auto);

    fn grayscale_like_rgb(buf: &[u8], channels: usize) -> bool {
        if channels < 3 {
            return false;
        }
        let pixels = buf.len() / channels;
        if pixels == 0 {
            return true;
        }
        let stride = (pixels / 256).max(1);
        let mut near_gray = 0usize;
        let mut sampled = 0usize;
        for i in (0..pixels).step_by(stride) {
            let off = i * channels;
            let r = buf[off];
            let g = buf[off + 1];
            let b = buf[off + 2];
            let rg = r.abs_diff(g);
            let rb = r.abs_diff(b);
            let gb = g.abs_diff(b);
            // Tolerate tiny codec/color-conversion drift for effectively monochrome frames.
            if rg <= 2 && rb <= 2 && gb <= 2 {
                near_gray += 1;
            }
            sampled += 1;
        }
        near_gray.saturating_mul(100) >= sampled.saturating_mul(98)
    }

    fn rgb_like_to_luma_parallel(src: &[u8], width: u32, height: u32, channels: usize) -> GrayImage {
        let mut out = GrayImage::new(width, height);
        out.as_mut().par_iter_mut().enumerate().for_each(|(i, dst)| {
            let off = i * channels;
            let r = src[off] as u32;
            let g = src[off + 1] as u32;
            let b = src[off + 2] as u32;
            *dst = (((77 * r) + (150 * g) + (29 * b) + 128) >> 8) as u8;
        });
        out
    }

    fn fast_luma_from_rgb_like(img: &DynamicImage) -> Option<GrayImage> {
        match img {
            DynamicImage::ImageRgb8(rgb) => {
                let src = rgb.as_raw();
                if grayscale_like_rgb(src, 3) {
                    let mut out = GrayImage::new(rgb.width(), rgb.height());
                    for (dst, px) in out.as_mut().iter_mut().zip(src.chunks_exact(3)) {
                        *dst = px[0];
                    }
                    return Some(out);
                }
                Some(rgb_like_to_luma_parallel(src, rgb.width(), rgb.height(), 3))
            }
            DynamicImage::ImageRgba8(rgba) => {
                let src = rgba.as_raw();
                if grayscale_like_rgb(src, 4) {
                    let mut out = GrayImage::new(rgba.width(), rgba.height());
                    for (dst, px) in out.as_mut().iter_mut().zip(src.chunks_exact(4)) {
                        *dst = px[0];
                    }
                    return Some(out);
                }
                Some(rgb_like_to_luma_parallel(src, rgba.width(), rgba.height(), 4))
            }
            _ => None,
        }
    }

    #[cfg(feature = "gpu")]
    {
        let cpu_fallback = || -> Result<Compute<DynamicImage>, NodeError> {
            match &mask {
                Compute::Cpu(img) => {
                    let out = match img {
                        DynamicImage::ImageLuma8(gray) => blend_clahe_with_base(gray, apply_clahe_cached(gray, tile_size, clip_limit), mix),
                        other => {
                            let gray = fast_luma_from_rgb_like(other).unwrap_or_else(|| other.to_luma8());
                            blend_clahe_with_base(&gray, apply_clahe_cached(&gray, tile_size, clip_limit), mix)
                        }
                    };
                    Ok(Compute::Cpu(DynamicImage::ImageLuma8(out)))
                }
                Compute::Gpu(_) => {
                    let (bytes, w, h) = mask.to_rgba_bytes(ctx.gpu.as_ref()).map_err(|e| NodeError::Handler(format!("clahe: {e}")))?;
                    let rgba = RgbaImage::from_raw(w, h, bytes).ok_or_else(|| NodeError::Handler("clahe: invalid image dimensions".into()))?;
                    let gray = DynamicImage::ImageRgba8(rgba).to_luma8();
                    let out = blend_clahe_with_base(&gray, apply_clahe_cached(&gray, tile_size, clip_limit), mix);
                    Ok(Compute::Cpu(DynamicImage::ImageLuma8(out)))
                }
            }
        };

        let (width, height) = mask.dimensions();
        if width == 0 || height == 0 {
            return Ok(Compute::Cpu(DynamicImage::new_rgba8(width, height)));
        }

        let want_gpu = matches!(mode, ExecMode::Gpu | ExecMode::Auto) && ctx.gpu.is_some() && mix >= 0.999;
        if want_gpu {
            let grid = tile_size.max(1);
            let tile_w = width.div_ceil(grid).max(4).min(width);
            let tile_h = height.div_ceil(grid).max(4).min(height);
            let tiles_x = width.div_ceil(tile_w).max(1);
            let tiles_y = height.div_ceil(tile_h).max(1);
            let params = ClaheParams { width, height, tile_grid: grid, _pad0: 0, clip_limit, _pad1: [0; 3] };
            let bindings = ClaheShaderBindings { input: &mask, output: TextureOut::from_input_ctx(&mask, &ctx), params: Uniform::new(params) };
            let gpu_result = ctx
                .dispatch_bindings(&bindings, None, Some([tiles_x, tiles_y, 1]), None)
                .and_then(|out| out.into_payload_with_ctx(1, &ctx, width, height))
                .map_err(|e| NodeError::Handler(format!("clahe (gpu): {e}")));
            return match gpu_result {
                Ok(out) => Ok(out),
                Err(err) if matches!(mode, ExecMode::Gpu) => Err(err),
                Err(_) => cpu_fallback(),
            };
        }

        return match mode {
            ExecMode::Cpu => cpu_fallback(),
            ExecMode::Gpu => Err(NodeError::Handler("clahe: GPU requested but unavailable".into())),
            ExecMode::Auto => cpu_fallback(),
        };
    }

    #[cfg(not(feature = "gpu"))]
    {
        let _ = mode;
        let mask = expect_cpu(mask, "clahe", Some(_exec_ctx))?;
        let out = match &mask {
            DynamicImage::ImageLuma8(gray) => blend_clahe_with_base(gray, apply_clahe_cached(gray, tile_size, clip_limit), mix),
            _ => {
                let gray = fast_luma_from_rgb_like(&mask).unwrap_or_else(|| mask.to_luma8());
                blend_clahe_with_base(&gray, apply_clahe_cached(&gray, tile_size, clip_limit), mix)
            }
        };
        Ok(Compute::Cpu(DynamicImage::ImageLuma8(out)))
    }
}

fn equalize_hist_gray_in_place(gray: &mut GrayImage) {
    let (w, h) = gray.dimensions();
    if w == 0 || h == 0 {
        return;
    }

    let mut hist = [0u32; 256];
    for &v in gray.as_raw() {
        hist[v as usize] = hist[v as usize].saturating_add(1);
    }

    let total = w.saturating_mul(h);
    if total == 0 {
        return;
    }

    let mut cdf = [0u32; 256];
    let mut cum = 0u32;
    for i in 0..256 {
        cum = cum.saturating_add(hist[i]);
        cdf[i] = cum;
    }

    let cdf_min = cdf.iter().copied().find(|&v| v != 0).unwrap_or(0);
    if cdf_min == 0 || cdf_min >= total {
        return;
    }

    let denom = (total - cdf_min).max(1) as f32;
    let scale = 255.0 / denom;

    for v in gray.as_mut() {
        let c = cdf[*v as usize];
        let mapped = ((c.saturating_sub(cdf_min)) as f32 * scale).round().clamp(0.0, 255.0) as u8;
        *v = mapped;
    }
}

fn build_gamma_lut(gamma: f32) -> [u8; 256] {
    let gamma = gamma.max(0.0);
    let mut lut = [0u8; 256];
    for (i, v) in lut.iter_mut().enumerate() {
        let x = i as f32 / 255.0;
        *v = (x.powf(gamma) * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    lut
}

#[inline(always)]
fn srgb_to_luma_u8(r: u8, g: u8, b: u8) -> u8 {
    // ITU-R BT.709 luma, integerized to keep conversion cheap.
    const SRGB_LUMA_R: u32 = 2126;
    const SRGB_LUMA_G: u32 = 7152;
    const SRGB_LUMA_B: u32 = 722;
    const SRGB_LUMA_DIV: u32 = 10000;
    let l = (SRGB_LUMA_R * r as u32 + SRGB_LUMA_G * g as u32 + SRGB_LUMA_B * b as u32) / SRGB_LUMA_DIV;
    l as u8
}

#[inline(always)]
fn gamma_correct_luma_in_place(gray: &mut GrayImage, lut: &[u8; 256]) {
    for v in gray.as_mut() {
        *v = lut[*v as usize];
    }
}

fn gamma_apply_to_image(img: DynamicImage, lut: &[u8; 256]) -> GrayImage {
    match img {
        DynamicImage::ImageLuma8(mut gray) => {
            gamma_correct_luma_in_place(&mut gray, lut);
            gray
        }
        DynamicImage::ImageLumaA8(gray_alpha) => {
            let (w, h) = gray_alpha.dimensions();
            let src = gray_alpha.as_raw();
            let mut out = GrayImage::new(w, h);
            for (dst, px) in out.as_mut().iter_mut().zip(src.chunks_exact(2)) {
                *dst = lut[px[0] as usize];
            }
            out
        }
        DynamicImage::ImageRgb8(rgb) => {
            let (w, h) = rgb.dimensions();
            let src = rgb.as_raw();
            let mut out = GrayImage::new(w, h);
            for (dst, px) in out.as_mut().iter_mut().zip(src.chunks_exact(3)) {
                *dst = lut[srgb_to_luma_u8(px[0], px[1], px[2]) as usize];
            }
            out
        }
        DynamicImage::ImageRgba8(rgba) => {
            let (w, h) = rgba.dimensions();
            let src = rgba.as_raw();
            let mut out = GrayImage::new(w, h);
            for (dst, px) in out.as_mut().iter_mut().zip(src.chunks_exact(4)) {
                *dst = lut[srgb_to_luma_u8(px[0], px[1], px[2]) as usize];
            }
            out
        }
        other => {
            let mut gray = other.to_luma8();
            gamma_correct_luma_in_place(&mut gray, lut);
            gray
        }
    }
}

thread_local! {
    static GAMMA_LUT_CACHE: std::cell::Cell<(u32, [u8; 256])> = const { std::cell::Cell::new((0, [0u8; 256])) };
}

fn gamma_lut(gamma: f32) -> [u8; 256] {
    let key = gamma.to_bits();
    GAMMA_LUT_CACHE.with(|cache| {
        let (cached_key, cached_lut) = cache.get();
        if cached_key == key {
            cached_lut
        } else {
            let lut = build_gamma_lut(gamma);
            cache.set((key, lut));
            lut
        }
    })
}

#[node(id = "equalize", inputs("mask"), outputs("mask"))]
pub(super) fn cv_equalize(mask: Compute<DynamicImage>, _exec_ctx: &ExecutionContext) -> Result<Compute<DynamicImage>, NodeError> {
    let img = expect_cpu(mask, "equalize", Some(_exec_ctx))?;
    let mut gray = match img {
        DynamicImage::ImageLuma8(gray) => gray,
        other => other.to_luma8(),
    };
    if gray.as_raw().iter().all(|&v| v == 0) {
        return Ok(Compute::Cpu(DynamicImage::ImageLuma8(gray)));
    }
    equalize_hist_gray_in_place(&mut gray);
    Ok(Compute::Cpu(DynamicImage::ImageLuma8(gray)))
}

#[node(id = "gamma", inputs("mask", port(name = "gamma", default = 1.0f64, meta(ui_min = 0.1, ui_max = 5.0, ui_step = 0.1))), outputs("mask"))]
pub(super) fn cv_gamma(mask: Compute<DynamicImage>, gamma: f64, _exec_ctx: &ExecutionContext) -> Result<Compute<DynamicImage>, NodeError> {
    if (gamma - 1.0).abs() <= f64::EPSILON {
        return Ok(mask);
    }
    let gamma = gamma.clamp(0.01, 10.0) as f32;
    let lut = gamma_lut(gamma);
    let img = expect_cpu(mask, "gamma", Some(_exec_ctx))?;
    let gray = gamma_apply_to_image(img, &lut);
    Ok(Compute::Cpu(DynamicImage::ImageLuma8(gray)))
}
