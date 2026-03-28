#![allow(clippy::needless_return, clippy::too_many_arguments, clippy::collapsible_if)]

use daedalus::FanIn;
use daedalus::core::compute::ComputeAffinity;
use daedalus::macros::node;
use daedalus::runtime::NodeError;
#[cfg(feature = "gpu")]
use image::GenericImageView;
use image::{DynamicImage, GrayImage, RgbImage, RgbaImage};
use rayon::prelude::*;
use wide::f32x8;

use crate::modules::color::{apply_mask, hsv_multi_range_mask, merge_masks, rgb_multi_range_mask};
use crate::plugin::ExecMode;

#[cfg(feature = "gpu")]
use bytemuck::{Pod, Zeroable};
use daedalus::gpu::Compute;
#[cfg(feature = "gpu")]
use daedalus::gpu::shader::{ShaderContext, TextureOut, Uniform, UniformBytes};
#[cfg(feature = "gpu")]
use daedalus::macros::GpuBindings;
use daedalus::runtime::state::ExecutionContext;

#[cfg(not(feature = "gpu"))]
fn expect_cpu(frame: Compute<DynamicImage>, label: &str, _exec_ctx: Option<&ExecutionContext>) -> Result<DynamicImage, NodeError> {
    match frame {
        Compute::Cpu(img) => Ok(img),
        Compute::Gpu(_) => Err(NodeError::Handler(format!("{label}: gpu payload unsupported (insert cpu convert)"))),
    }
}

#[cfg(feature = "gpu")]
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct ColorParam {
    value: f32,
    _pad: [f32; 3],
}

#[cfg(feature = "gpu")]
#[derive(GpuBindings)]
#[gpu(spec(src = "src/gpu/shaders/color_ops.wgsl", entry = "brightness_main"))]
struct BrightnessShaderBindings<'a> {
    #[gpu(binding = 0, texture2d(format = "rgba8unorm"))]
    input: &'a Compute<DynamicImage>,
    #[gpu(binding = 1, texture2d(format = "rgba8unorm", write))]
    output: TextureOut,
    #[gpu(binding = 2, uniform)]
    params: Uniform<ColorParam>,
}

#[cfg(feature = "gpu")]
#[derive(GpuBindings)]
#[gpu(spec(src = "src/gpu/shaders/color_ops.wgsl", entry = "contrast_main"))]
struct ContrastShaderBindings<'a> {
    #[gpu(binding = 0, texture2d(format = "rgba8unorm"))]
    input: &'a Compute<DynamicImage>,
    #[gpu(binding = 1, texture2d(format = "rgba8unorm", write))]
    output: TextureOut,
    #[gpu(binding = 2, uniform)]
    params: Uniform<ColorParam>,
}

#[cfg(feature = "gpu")]
#[derive(GpuBindings)]
#[gpu(spec(src = "src/gpu/shaders/color_ops.wgsl", entry = "saturation_main"))]
struct SaturationShaderBindings<'a> {
    #[gpu(binding = 0, texture2d(format = "rgba8unorm"))]
    input: &'a Compute<DynamicImage>,
    #[gpu(binding = 1, texture2d(format = "rgba8unorm", write))]
    output: TextureOut,
    #[gpu(binding = 2, uniform)]
    params: Uniform<ColorParam>,
}

#[cfg(feature = "gpu")]
#[derive(GpuBindings)]
#[gpu(spec(src = "src/gpu/shaders/color_ops.wgsl", entry = "grayscale_main"))]
struct GrayscaleShaderBindings<'a> {
    #[gpu(binding = 0, texture2d(format = "rgba8unorm"))]
    input: &'a Compute<DynamicImage>,
    #[gpu(binding = 1, texture2d(format = "rgba8unorm", write))]
    output: TextureOut,
    #[gpu(binding = 2, uniform)]
    params: Uniform<ColorParam>,
}

#[cfg(feature = "gpu")]
#[derive(GpuBindings)]
#[gpu(spec(src = "src/gpu/shaders/color_ops.wgsl", entry = "hue_main"))]
struct HueShaderBindings<'a> {
    #[gpu(binding = 0, texture2d(format = "rgba8unorm"))]
    input: &'a Compute<DynamicImage>,
    #[gpu(binding = 1, texture2d(format = "rgba8unorm", write))]
    output: TextureOut,
    #[gpu(binding = 2, uniform)]
    params: Uniform<ColorParam>,
}

#[cfg(feature = "gpu")]
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct RangeParams {
    count: u32,
    _pad: [u32; 3],
}

#[cfg(feature = "gpu")]
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct RgbRange {
    r_min: i32,
    r_max: i32,
    g_min: i32,
    g_max: i32,
    b_min: i32,
    b_max: i32,
    _pad: [i32; 2],
}

#[cfg(feature = "gpu")]
#[derive(GpuBindings)]
#[gpu(spec(src = "src/gpu/shaders/rgb_range.wgsl", entry = "rgb_mask_main"))]
struct RgbRangeShaderBindings<'a> {
    #[gpu(binding = 0, texture2d(format = "rgba8unorm"))]
    input: &'a Compute<DynamicImage>,
    #[gpu(binding = 1, texture2d(format = "rgba8unorm", write))]
    output: TextureOut,
    #[gpu(binding = 2, storage(read))]
    ranges: UniformBytes<'a>,
    #[gpu(binding = 3, uniform)]
    params: Uniform<RangeParams>,
}

#[cfg(feature = "gpu")]
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct HsvRange {
    h_min: f32,
    h_max: f32,
    s_min: f32,
    s_max: f32,
    v_min: f32,
    v_max: f32,
    _pad: [f32; 2],
}

#[cfg(feature = "gpu")]
#[derive(GpuBindings)]
#[gpu(spec(src = "src/gpu/shaders/hsv_range.wgsl", entry = "hsv_mask_main"))]
struct HsvRangeShaderBindings<'a> {
    #[gpu(binding = 0, texture2d(format = "rgba8unorm"))]
    input: &'a Compute<DynamicImage>,
    #[gpu(binding = 1, texture2d(format = "rgba8unorm", write))]
    output: TextureOut,
    #[gpu(binding = 2, storage(read))]
    ranges: UniformBytes<'a>,
    #[gpu(binding = 3, uniform)]
    params: Uniform<RangeParams>,
}

#[cfg(feature = "gpu")]
#[derive(GpuBindings)]
#[gpu(spec(src = "src/gpu/shaders/apply_mask.wgsl", entry = "apply_mask_main"))]
struct ApplyMaskShaderBindings<'a> {
    #[gpu(binding = 0, texture2d(format = "rgba8unorm"))]
    frame: &'a Compute<DynamicImage>,
    #[gpu(binding = 1, texture2d(format = "rgba8unorm"))]
    mask: &'a Compute<DynamicImage>,
    #[gpu(binding = 2, texture2d(format = "rgba8unorm", write))]
    output: TextureOut,
}

mod mask;
mod ops;

use daedalus::runtime::plugins::{Plugin, PluginRegistry};

#[derive(Clone, Debug, Default)]
pub struct CvColorPlugin;

impl CvColorPlugin {
    pub fn install(&self, registry: &mut PluginRegistry) -> Result<(), &'static str> {
        registry.merge::<ops::cv_brightness>()?;
        registry.merge::<ops::cv_contrast>()?;
        registry.merge::<ops::cv_hue>()?;
        registry.merge::<ops::cv_saturation>()?;
        registry.merge::<ops::cv_grayscale>()?;
        registry.merge::<mask::cv_rgb_multi_range_mask>()?;
        registry.merge::<mask::cv_hsv_multi_range_mask>()?;
        registry.merge::<mask::cv_apply_mask>()?;
        registry.merge::<mask::cv_merge_masks>()?;
        Ok(())
    }
}

impl Plugin for CvColorPlugin {
    fn id(&self) -> &'static str {
        "color"
    }

    fn install(&self, registry: &mut PluginRegistry) -> Result<(), &'static str> {
        self.install(registry)
    }
}
