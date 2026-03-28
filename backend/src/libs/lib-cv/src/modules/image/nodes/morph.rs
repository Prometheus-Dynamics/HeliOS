use super::*;

#[cfg_attr(
    feature = "gpu",
    node(
        id = "erode",
        compute(ComputeAffinity::GpuPreferred),
        inputs("mask", port(name = "norm", default = "l1"), port(name = "k", meta(ui_min = 1, ui_max = 31, ui_step = 2))),
        outputs("mask"),
        shaders(MorphShaderBindings)
    )
)]
#[cfg_attr(
    not(feature = "gpu"),
    node(id = "erode", compute(ComputeAffinity::GpuPreferred), inputs("mask", port(name = "norm", default = "l1"), port(name = "k", meta(ui_min = 1, ui_max = 31, ui_step = 2))), outputs("mask"))
)]
fn cv_erode(mask: Compute<DynamicImage>, norm: MorphNorm, k: u32, #[cfg(feature = "gpu")] ctx: ShaderContext, _exec_ctx: &ExecutionContext) -> Result<Compute<DynamicImage>, NodeError> {
    #[cfg(feature = "gpu")]
    {
        return run_morph(&mask, norm, k, 0, &ctx);
    }
    #[cfg(not(feature = "gpu"))]
    {
        let k = k.max(1).min(u8::MAX as u32) as u8;
        let mask = expect_cpu(mask, "erode", Some(_exec_ctx))?;
        let gray = mask.to_luma8();
        Ok(Compute::Cpu(DynamicImage::ImageLuma8(morph_ops::erode(&gray, norm.to_imageproc(), k))))
    }
}

#[cfg_attr(
    feature = "gpu",
    node(
        id = "dilate",
        compute(ComputeAffinity::GpuPreferred),
        inputs("mask", port(name = "norm", default = "l1"), port(name = "k", meta(ui_min = 1, ui_max = 31, ui_step = 2))),
        outputs("mask"),
        shaders(MorphShaderBindings)
    )
)]
#[cfg_attr(
    not(feature = "gpu"),
    node(id = "dilate", compute(ComputeAffinity::GpuPreferred), inputs("mask", port(name = "norm", default = "l1"), port(name = "k", meta(ui_min = 1, ui_max = 31, ui_step = 2))), outputs("mask"))
)]
fn cv_dilate(mask: Compute<DynamicImage>, norm: MorphNorm, k: u32, #[cfg(feature = "gpu")] ctx: ShaderContext, _exec_ctx: &ExecutionContext) -> Result<Compute<DynamicImage>, NodeError> {
    #[cfg(feature = "gpu")]
    {
        return run_morph(&mask, norm, k, 1, &ctx);
    }
    #[cfg(not(feature = "gpu"))]
    {
        let k = k.max(1).min(u8::MAX as u32) as u8;
        let mask = expect_cpu(mask, "dilate", Some(_exec_ctx))?;
        let gray = mask.to_luma8();
        Ok(Compute::Cpu(DynamicImage::ImageLuma8(morph_ops::dilate(&gray, norm.to_imageproc(), k))))
    }
}

#[cfg_attr(
    feature = "gpu",
    node(
        id = "open",
        compute(ComputeAffinity::GpuPreferred),
        inputs("mask", port(name = "norm", default = "l1"), port(name = "k", meta(ui_min = 1, ui_max = 31, ui_step = 2))),
        outputs("mask"),
        shaders(MorphShaderBindings, MorphDiffShaderBindings)
    )
)]
#[cfg_attr(
    not(feature = "gpu"),
    node(id = "open", compute(ComputeAffinity::GpuPreferred), inputs("mask", port(name = "norm", default = "l1"), port(name = "k", meta(ui_min = 1, ui_max = 31, ui_step = 2))), outputs("mask"))
)]
fn cv_open(mask: Compute<DynamicImage>, norm: MorphNorm, k: u32, #[cfg(feature = "gpu")] ctx: ShaderContext, _exec_ctx: &ExecutionContext) -> Result<Compute<DynamicImage>, NodeError> {
    if k == 0 || payload_is_empty(&mask) {
        return Ok(mask);
    }
    #[cfg(feature = "gpu")]
    {
        let eroded = run_morph(&mask, norm, k, 0, &ctx)?;
        return run_morph(&eroded, norm, k, 1, &ctx);
    }
    #[cfg(not(feature = "gpu"))]
    {
        let k = k.max(1).min(u8::MAX as u32) as u8;
        let mask = expect_cpu(mask, "open", Some(_exec_ctx))?;
        let gray = mask.to_luma8();
        Ok(Compute::Cpu(DynamicImage::ImageLuma8(morph_ops::open(&gray, norm.to_imageproc(), k))))
    }
}

#[cfg_attr(
    feature = "gpu",
    node(
        id = "close",
        compute(ComputeAffinity::GpuPreferred),
        inputs("mask", port(name = "norm", default = "l1"), port(name = "k", meta(ui_min = 1, ui_max = 31, ui_step = 2))),
        outputs("mask"),
        shaders(MorphShaderBindings, MorphDiffShaderBindings)
    )
)]
#[cfg_attr(
    not(feature = "gpu"),
    node(id = "close", compute(ComputeAffinity::GpuPreferred), inputs("mask", port(name = "norm", default = "l1"), port(name = "k", meta(ui_min = 1, ui_max = 31, ui_step = 2))), outputs("mask"))
)]
fn cv_close(mask: Compute<DynamicImage>, norm: MorphNorm, k: u32, #[cfg(feature = "gpu")] ctx: ShaderContext, _exec_ctx: &ExecutionContext) -> Result<Compute<DynamicImage>, NodeError> {
    if k == 0 || payload_is_empty(&mask) {
        return Ok(mask);
    }
    #[cfg(feature = "gpu")]
    {
        let dilated = run_morph(&mask, norm, k, 1, &ctx)?;
        return run_morph(&dilated, norm, k, 0, &ctx);
    }
    #[cfg(not(feature = "gpu"))]
    {
        let k = k.max(1).min(u8::MAX as u32) as u8;
        let mask = expect_cpu(mask, "close", Some(_exec_ctx))?;
        let gray = mask.to_luma8();
        Ok(Compute::Cpu(DynamicImage::ImageLuma8(morph_ops::close(&gray, norm.to_imageproc(), k))))
    }
}

#[cfg_attr(
    feature = "gpu",
    node(
        id = "tophat",
        compute(ComputeAffinity::GpuPreferred),
        inputs("mask", port(name = "norm", default = "l1"), port(name = "k", meta(ui_min = 1, ui_max = 31, ui_step = 2))),
        outputs("mask"),
        shaders(MorphShaderBindings, MorphDiffShaderBindings)
    )
)]
#[cfg_attr(
    not(feature = "gpu"),
    node(id = "tophat", compute(ComputeAffinity::GpuPreferred), inputs("mask", port(name = "norm", default = "l1"), port(name = "k", meta(ui_min = 1, ui_max = 31, ui_step = 2))), outputs("mask"))
)]
fn cv_tophat(mask: Compute<DynamicImage>, norm: MorphNorm, k: u32, #[cfg(feature = "gpu")] ctx: ShaderContext, _exec_ctx: &ExecutionContext) -> Result<Compute<DynamicImage>, NodeError> {
    if k == 0 || payload_is_empty(&mask) {
        return Ok(mask);
    }
    #[cfg(feature = "gpu")]
    {
        let opened = {
            let eroded = run_morph(&mask, norm, k, 0, &ctx)?;
            run_morph(&eroded, norm, k, 1, &ctx)?
        };
        return run_morph_diff(&mask, &opened, &ctx);
    }
    #[cfg(not(feature = "gpu"))]
    {
        let k = k.max(1).min(u8::MAX as u32) as u8;
        let mask = expect_cpu(mask, "tophat", Some(_exec_ctx))?;
        let gray = mask.to_luma8();
        let out = if matches!(norm, MorphNorm::L1) { morph_ops::tophat_simd(&gray, norm.to_imageproc(), k) } else { morph_ops::tophat(&gray, norm.to_imageproc(), k) };
        Ok(Compute::Cpu(DynamicImage::ImageLuma8(out)))
    }
}

#[cfg_attr(
    feature = "gpu",
    node(
        id = "blackhat",
        compute(ComputeAffinity::GpuPreferred),
        inputs("mask", port(name = "norm", default = "l1"), port(name = "k", meta(ui_min = 1, ui_max = 31, ui_step = 2))),
        outputs("mask"),
        shaders(MorphShaderBindings, MorphDiffShaderBindings)
    )
)]
#[cfg_attr(
    not(feature = "gpu"),
    node(id = "blackhat", compute(ComputeAffinity::GpuPreferred), inputs("mask", port(name = "norm", default = "l1"), port(name = "k", meta(ui_min = 1, ui_max = 31, ui_step = 2))), outputs("mask"))
)]
fn cv_blackhat(mask: Compute<DynamicImage>, norm: MorphNorm, k: u32, #[cfg(feature = "gpu")] ctx: ShaderContext, _exec_ctx: &ExecutionContext) -> Result<Compute<DynamicImage>, NodeError> {
    if k == 0 || payload_is_empty(&mask) {
        return Ok(mask);
    }
    #[cfg(feature = "gpu")]
    {
        let closed = {
            let dil = run_morph(&mask, norm, k, 1, &ctx)?;
            run_morph(&dil, norm, k, 0, &ctx)?
        };
        return run_morph_diff(&closed, &mask, &ctx);
    }
    #[cfg(not(feature = "gpu"))]
    {
        let k = k.max(1).min(u8::MAX as u32) as u8;
        let mask = expect_cpu(mask, "blackhat", Some(_exec_ctx))?;
        let gray = mask.to_luma8();
        let out = if matches!(norm, MorphNorm::L1) { morph_ops::blackhat_simd(&gray, norm.to_imageproc(), k) } else { morph_ops::blackhat(&gray, norm.to_imageproc(), k) };
        Ok(Compute::Cpu(DynamicImage::ImageLuma8(out)))
    }
}

#[cfg_attr(
    feature = "gpu",
    node(
        id = "gradient",
        compute(ComputeAffinity::GpuPreferred),
        inputs("mask", port(name = "norm", default = "l1"), port(name = "k", meta(ui_min = 1, ui_max = 31, ui_step = 2))),
        outputs("mask"),
        shaders(MorphShaderBindings, MorphDiffShaderBindings)
    )
)]
#[cfg_attr(
    not(feature = "gpu"),
    node(id = "gradient", compute(ComputeAffinity::GpuPreferred), inputs("mask", port(name = "norm", default = "l1"), port(name = "k", meta(ui_min = 1, ui_max = 31, ui_step = 2))), outputs("mask"))
)]
fn cv_gradient(mask: Compute<DynamicImage>, norm: MorphNorm, k: u32, #[cfg(feature = "gpu")] ctx: ShaderContext, _exec_ctx: &ExecutionContext) -> Result<Compute<DynamicImage>, NodeError> {
    if k == 0 || payload_is_empty(&mask) {
        return Ok(mask);
    }
    #[cfg(feature = "gpu")]
    {
        let dil = run_morph(&mask, norm, k, 1, &ctx)?;
        let ero = run_morph(&mask, norm, k, 0, &ctx)?;
        return run_morph_diff(&dil, &ero, &ctx);
    }
    #[cfg(not(feature = "gpu"))]
    {
        let k = k.max(1).min(u8::MAX as u32) as u8;
        let mask = expect_cpu(mask, "gradient", Some(_exec_ctx))?;
        let gray = mask.to_luma8();
        let out = if matches!(norm, MorphNorm::L1) { morph_ops::gradient_simd(&gray, norm.to_imageproc(), k) } else { morph_ops::gradient(&gray, norm.to_imageproc(), k) };
        Ok(Compute::Cpu(DynamicImage::ImageLuma8(out)))
    }
}

#[cfg_attr(
    feature = "gpu",
    node(
        id = "ingradient",
        compute(ComputeAffinity::GpuPreferred),
        inputs("mask", port(name = "norm", default = "l1"), port(name = "k", meta(ui_min = 1, ui_max = 31, ui_step = 2))),
        outputs("mask"),
        shaders(MorphShaderBindings, MorphDiffShaderBindings)
    )
)]
#[cfg_attr(
    not(feature = "gpu"),
    node(
        id = "ingradient",
        compute(ComputeAffinity::GpuPreferred),
        inputs("mask", port(name = "norm", default = "l1"), port(name = "k", meta(ui_min = 1, ui_max = 31, ui_step = 2))),
        outputs("mask")
    )
)]
fn cv_ingradient(mask: Compute<DynamicImage>, norm: MorphNorm, k: u32, #[cfg(feature = "gpu")] ctx: ShaderContext, _exec_ctx: &ExecutionContext) -> Result<Compute<DynamicImage>, NodeError> {
    if k == 0 || payload_is_empty(&mask) {
        return Ok(mask);
    }
    #[cfg(feature = "gpu")]
    {
        let ero = run_morph(&mask, norm, k, 0, &ctx)?;
        return run_morph_diff(&mask, &ero, &ctx);
    }
    #[cfg(not(feature = "gpu"))]
    {
        let k = k.max(1).min(u8::MAX as u32) as u8;
        let mask = expect_cpu(mask, "ingradient", Some(_exec_ctx))?;
        let gray = mask.to_luma8();
        Ok(Compute::Cpu(DynamicImage::ImageLuma8(morph_ops::internal_gradient(&gray, norm.to_imageproc(), k))))
    }
}

#[cfg_attr(
    feature = "gpu",
    node(
        id = "exgradient",
        compute(ComputeAffinity::GpuPreferred),
        inputs("mask", port(name = "norm", default = "l1"), port(name = "k", meta(ui_min = 1, ui_max = 31, ui_step = 2))),
        outputs("mask"),
        shaders(MorphShaderBindings, MorphDiffShaderBindings)
    )
)]
#[cfg_attr(
    not(feature = "gpu"),
    node(
        id = "exgradient",
        compute(ComputeAffinity::GpuPreferred),
        inputs("mask", port(name = "norm", default = "l1"), port(name = "k", meta(ui_min = 1, ui_max = 31, ui_step = 2))),
        outputs("mask")
    )
)]
fn cv_exgradient(mask: Compute<DynamicImage>, norm: MorphNorm, k: u32, #[cfg(feature = "gpu")] ctx: ShaderContext, _exec_ctx: &ExecutionContext) -> Result<Compute<DynamicImage>, NodeError> {
    if k == 0 || payload_is_empty(&mask) {
        return Ok(mask);
    }
    #[cfg(feature = "gpu")]
    {
        let dil = run_morph(&mask, norm, k, 1, &ctx)?;
        return run_morph_diff(&dil, &mask, &ctx);
    }
    #[cfg(not(feature = "gpu"))]
    {
        let k = k.max(1).min(u8::MAX as u32) as u8;
        let mask = expect_cpu(mask, "exgradient", Some(_exec_ctx))?;
        let gray = mask.to_luma8();
        Ok(Compute::Cpu(DynamicImage::ImageLuma8(morph_ops::external_gradient(&gray, norm.to_imageproc(), k))))
    }
}

#[cfg_attr(
    feature = "gpu",
    node(
        id = "outline",
        compute(ComputeAffinity::GpuPreferred),
        inputs("mask", port(name = "norm", default = "l1"), port(name = "k", meta(ui_min = 1, ui_max = 31, ui_step = 2))),
        outputs("mask"),
        shaders(MorphShaderBindings, MorphDiffShaderBindings)
    )
)]
#[cfg_attr(
    not(feature = "gpu"),
    node(id = "outline", compute(ComputeAffinity::GpuPreferred), inputs("mask", port(name = "norm", default = "l1"), port(name = "k", meta(ui_min = 1, ui_max = 31, ui_step = 2))), outputs("mask"))
)]
fn cv_outline(mask: Compute<DynamicImage>, norm: MorphNorm, k: u32, #[cfg(feature = "gpu")] ctx: ShaderContext, _exec_ctx: &ExecutionContext) -> Result<Compute<DynamicImage>, NodeError> {
    if k == 0 || payload_is_empty(&mask) {
        return Ok(mask);
    }
    #[cfg(feature = "gpu")]
    {
        let ero = run_morph(&mask, norm, k, 0, &ctx)?;
        return run_morph_diff(&mask, &ero, &ctx);
    }
    #[cfg(not(feature = "gpu"))]
    {
        let k = k.max(1).min(u8::MAX as u32) as u8;
        let mask = expect_cpu(mask, "outline", Some(_exec_ctx))?;
        let gray = mask.to_luma8();
        Ok(Compute::Cpu(DynamicImage::ImageLuma8(morph_ops::outline(&gray, norm.to_imageproc(), k))))
    }
}

#[node(id = "skeleton", inputs("mask", port(name = "norm", default = "l1")), outputs("mask"))]
fn cv_skeleton(mask: GrayImage, norm: MorphNorm) -> Result<GrayImage, NodeError> {
    if mask.as_raw().iter().all(|&v| v == 0) {
        return Ok(mask);
    }
    Ok(skeleton(&mask, norm.to_imageproc()))
}

#[node(id = "component_filter", inputs("mask", port(name = "min_area", meta(ui_min = 0, ui_max = 2000000, ui_step = 1000))), outputs("mask"))]
fn cv_component_filter(mut mask: GrayImage, min_area: u32) -> Result<GrayImage, NodeError> {
    if min_area == 0 || mask.as_raw().iter().all(|&v| v == 0) {
        return Ok(mask);
    }
    remove_small_components_in_place(&mut mask, min_area);
    Ok(mask)
}

#[node(id = "component_features", inputs("mask", port(name = "min_area", meta(ui_min = 0, ui_max = 2000000, ui_step = 1000))), outputs("features"))]
fn cv_component_features(mask: GrayImage, min_area: u32) -> Result<Vec<ComponentFeature>, NodeError> {
    if mask.as_raw().iter().all(|&v| v == 0) {
        return Ok(Vec::new());
    }
    Ok(extract_component_features(&mask, min_area))
}

fn payload_is_empty(mask: &Compute<DynamicImage>) -> bool {
    match mask {
        Compute::Cpu(DynamicImage::ImageLuma8(gray)) => gray.as_raw().iter().all(|&v| v == 0),
        _ => false,
    }
}

// NOTE: Conversion-only nodes (e.g. DynamicImage <-> GrayImage) are intentionally avoided.
// Daedalus handles CPU coercions via the conversion registry and GPU<->CPU transfers via
// GpuSendable + segment boundaries.
