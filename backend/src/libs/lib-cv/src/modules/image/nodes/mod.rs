use crate::Pixel;
use daedalus::core::compute::ComputeAffinity;
use daedalus::macros::node;
use daedalus::runtime::NodeError;
use image::{DynamicImage, GenericImageView, GrayImage, RgbImage, Rgba, RgbaImage};
use imageproc::distance_transform::Norm as ImageprocNorm;
use imageproc::geometric_transformations::{Interpolation, Projection, warp_into};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, OnceLock, RwLock};

#[cfg(not(feature = "gpu"))]
use crate::modules::image::convolution::{emboss, sharpen};
#[cfg(not(feature = "gpu"))]
use crate::modules::image::morphology as morph_ops;
use crate::modules::image::{
    binary::otsu_level,
    blur::nodes::cv_blur,
    clahe::{apply_clahe_with_tiles, prepare_clahe},
    components::{ComponentFeature, component_features as extract_component_features, remove_small_components_in_place},
    convolution::{canny_prep, convolve_gray, sobel_edges},
    guided::guided_filter_gray,
    luma::with_luma8_frame,
    morphology::skeleton,
    resize::{downscale_luma8_in_place, resize_fast},
    rotate::{Rotation, rotate_fast},
};
use crate::plugin::ExecMode;

#[cfg(feature = "gpu")]
use crate::modules::image::binary::binary_image_simd;
#[cfg(feature = "gpu")]
use bytemuck::{Pod, Zeroable};
use daedalus::gpu::Compute;
#[cfg(feature = "gpu")]
use daedalus::gpu::shader::{ShaderContext, TextureOut, Uniform};
#[cfg(feature = "gpu")]
use daedalus::macros::GpuBindings;
use daedalus::runtime::state::ExecutionContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MorphNorm {
    L1,
    L2,
    Linf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BorderMode {
    Zero,
    Clamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UndistortZoomMode {
    /// Use the provided `zoom` value.
    Manual,
    /// Compute the smallest zoom that keeps the output fully filled (no out-of-bounds samples)
    /// and then apply `fill_margin`.
    Fill,
}

impl BorderMode {
    fn is_clamp(self) -> bool {
        matches!(self, Self::Clamp)
    }
}

impl MorphNorm {
    fn to_imageproc(self) -> ImageprocNorm {
        match self {
            MorphNorm::L1 => ImageprocNorm::L1,
            MorphNorm::L2 => ImageprocNorm::L2,
            MorphNorm::Linf => ImageprocNorm::LInf,
        }
    }
}

fn expect_cpu(frame: Compute<DynamicImage>, label: &str, _exec_ctx: Option<&ExecutionContext>) -> Result<DynamicImage, NodeError> {
    #[cfg(feature = "gpu")]
    {
        match frame {
            Compute::Cpu(img) => Ok(img),
            Compute::Gpu(handle) => {
                let ctx = _exec_ctx.and_then(|ctx| ctx.gpu.as_ref()).ok_or_else(|| NodeError::Handler(format!("{label}: gpu payload missing context")))?;
                let bytes = ctx.read_texture(&handle).map_err(|e| NodeError::Handler(format!("{label}: {e}")))?;
                let rgba = RgbaImage::from_raw(handle.width, handle.height, bytes).ok_or_else(|| NodeError::Handler(format!("{label}: invalid image dimensions")))?;
                Ok(DynamicImage::ImageRgba8(rgba))
            }
        }
    }
    #[cfg(not(feature = "gpu"))]
    {
        match frame {
            Compute::Cpu(img) => Ok(img),
            Compute::Gpu(_) => Err(NodeError::Handler(format!("{label}: GPU payload unsupported (insert cpu convert)"))),
        }
    }
}

#[cfg(feature = "gpu")]
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct BinaryParams {
    threshold: f32,
    _pad: [f32; 3],
}

#[cfg(feature = "gpu")]
#[derive(GpuBindings)]
#[gpu(spec(src = "src/gpu/shaders/binary.wgsl", entry = "binary_main"))]
struct BinaryShaderBindings<'a> {
    #[gpu(binding = 0, texture2d(format = "rgba8unorm"))]
    input: &'a Compute<DynamicImage>,
    #[gpu(binding = 1, texture2d(format = "rgba8unorm", write))]
    output: TextureOut,
    #[gpu(binding = 2, uniform)]
    params: Uniform<BinaryParams>,
}

#[cfg(feature = "gpu")]
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct BoxBlurParams {
    width: u32,
    height: u32,
    radius: u32,
    inv_kernel: f32,
}

#[cfg(feature = "gpu")]
#[derive(GpuBindings)]
#[gpu(spec(src = "src/gpu/shaders/adaptive_box.wgsl", entry = "box_horizontal_main"))]
struct BoxBlurHorizontalBindings<'a> {
    #[gpu(binding = 0, texture2d(format = "rgba8unorm"))]
    input: &'a Compute<DynamicImage>,
    #[gpu(binding = 1, texture2d(format = "rgba8unorm", write))]
    output: TextureOut,
    #[gpu(binding = 2, uniform)]
    params: Uniform<BoxBlurParams>,
}

#[cfg(feature = "gpu")]
#[derive(GpuBindings)]
#[gpu(spec(src = "src/gpu/shaders/adaptive_box.wgsl", entry = "box_vertical_main"))]
struct BoxBlurVerticalBindings<'a> {
    #[gpu(binding = 0, texture2d(format = "rgba8unorm"))]
    input: &'a Compute<DynamicImage>,
    #[gpu(binding = 1, texture2d(format = "rgba8unorm", write))]
    output: TextureOut,
    #[gpu(binding = 2, uniform)]
    params: Uniform<BoxBlurParams>,
}

#[cfg(feature = "gpu")]
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct MaskThresholdParams {
    offset: f32,
    invert: u32,
    _pad: [u32; 2],
}

#[cfg(feature = "gpu")]
#[derive(GpuBindings)]
#[gpu(spec(src = "src/gpu/shaders/aruco_mask_threshold.wgsl", entry = "mask_threshold_r8_main"))]
struct MaskThresholdShaderBindings<'a> {
    #[gpu(binding = 0, texture2d(format = "rgba8unorm"))]
    input: &'a Compute<DynamicImage>,
    #[gpu(binding = 1, texture2d(format = "rgba8unorm"))]
    mean: &'a Compute<DynamicImage>,
    #[gpu(binding = 2, texture2d(format = "rgba8unorm", write))]
    output: TextureOut,
    #[gpu(binding = 3, uniform)]
    params: Uniform<MaskThresholdParams>,
}

#[cfg(feature = "gpu")]
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct DownscaleParams {
    in_width: u32,
    in_height: u32,
    out_width: u32,
    out_height: u32,
    factor: u32,
    _pad: [u32; 3],
}

#[cfg(feature = "gpu")]
#[derive(GpuBindings)]
#[gpu(spec(src = "src/gpu/shaders/downscale.wgsl", entry = "downscale_main"))]
struct DownscaleShaderBindings<'a> {
    #[gpu(binding = 0, texture2d(format = "rgba8unorm"))]
    input: &'a Compute<DynamicImage>,
    #[gpu(binding = 1, texture2d(format = "rgba8unorm", write))]
    output: TextureOut,
    #[gpu(binding = 2, uniform)]
    params: Uniform<DownscaleParams>,
}

#[cfg(feature = "gpu")]
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct SobelParams {
    width: u32,
    height: u32,
    _pad: [u32; 2],
}

#[cfg(feature = "gpu")]
#[derive(GpuBindings)]
#[gpu(spec(src = "src/gpu/shaders/sobel.wgsl", entry = "sobel_main"))]
struct SobelShaderBindings<'a> {
    #[gpu(binding = 0, texture2d(format = "rgba8unorm"))]
    input: &'a Compute<DynamicImage>,
    #[gpu(binding = 1, texture2d(format = "rgba8unorm", write))]
    output: TextureOut,
    #[gpu(binding = 2, uniform)]
    params: Uniform<SobelParams>,
}

#[cfg(feature = "gpu")]
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct ConvolutionParams {
    factor: f32,
    bias: f32,
    _pad: [f32; 2],
    kernel: [[f32; 4]; 3],
}

#[cfg(feature = "gpu")]
#[derive(GpuBindings)]
#[gpu(spec(src = "src/gpu/shaders/convolution.wgsl", entry = "convolution_main"))]
struct ConvolutionShaderBindings<'a> {
    #[gpu(binding = 0, texture2d(format = "rgba8unorm"))]
    input: &'a Compute<DynamicImage>,
    #[gpu(binding = 1, texture2d(format = "rgba8unorm", write))]
    output: TextureOut,
    #[gpu(binding = 2, uniform)]
    params: Uniform<ConvolutionParams>,
}

#[cfg(feature = "gpu")]
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct GuidedParams {
    radius: u32,
    epsilon: f32,
    _pad: [f32; 2],
}

#[cfg(feature = "gpu")]
#[derive(GpuBindings)]
#[gpu(spec(src = "src/gpu/shaders/guided_filter_coeff.wgsl", entry = "guided_coeff_main"))]
struct GuidedCoeffShaderBindings<'a> {
    #[gpu(binding = 0, texture2d(format = "rgba8unorm"))]
    input: &'a Compute<DynamicImage>,
    #[gpu(binding = 1, texture2d(format = "rgba16float", write))]
    output: TextureOut,
    #[gpu(binding = 2, uniform)]
    params: Uniform<GuidedParams>,
}

#[cfg(feature = "gpu")]
#[derive(GpuBindings)]
#[gpu(spec(src = "src/gpu/shaders/guided_filter_resolve.wgsl", entry = "guided_resolve_main"))]
struct GuidedResolveShaderBindings<'a> {
    #[gpu(binding = 0, texture2d(format = "rgba8unorm"))]
    input: &'a Compute<DynamicImage>,
    #[gpu(binding = 1, texture2d(format = "rgba16float"))]
    coeff: &'a Compute<DynamicImage>,
    #[gpu(binding = 2, uniform)]
    params: Uniform<GuidedParams>,
    #[gpu(binding = 3, texture2d(format = "rgba8unorm", write))]
    output: TextureOut,
}

#[cfg(feature = "gpu")]
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct MorphParams {
    radius: u32,
    norm: i32,
    op: u32,
    _pad: u32,
}

#[cfg(feature = "gpu")]
#[derive(GpuBindings)]
#[gpu(spec(src = "src/gpu/shaders/morph.wgsl", entry = "morph_main"))]
struct MorphShaderBindings<'a> {
    #[gpu(binding = 0, texture2d(format = "rgba8unorm"))]
    input: &'a Compute<DynamicImage>,
    #[gpu(binding = 1, texture2d(format = "rgba8unorm", write))]
    output: TextureOut,
    #[gpu(binding = 2, uniform)]
    params: Uniform<MorphParams>,
}

#[cfg(feature = "gpu")]
#[derive(GpuBindings)]
#[gpu(spec(src = "src/gpu/shaders/morph_diff.wgsl", entry = "diff_main"))]
struct MorphDiffShaderBindings<'a> {
    #[gpu(binding = 0, texture2d(format = "rgba8unorm"))]
    a: &'a Compute<DynamicImage>,
    #[gpu(binding = 1, texture2d(format = "rgba8unorm"))]
    b: &'a Compute<DynamicImage>,
    #[gpu(binding = 3, texture2d(format = "rgba8unorm", write))]
    output: TextureOut,
}

#[cfg(feature = "gpu")]
#[derive(GpuBindings)]
#[gpu(spec(src = "src/gpu/shaders/clahe.wgsl", entry = "clahe_main"))]
struct ClaheShaderBindings<'a> {
    #[gpu(binding = 0, texture2d(format = "rgba8unorm"))]
    input: &'a Compute<DynamicImage>,
    #[gpu(binding = 1, texture2d(format = "rgba8unorm", write))]
    output: TextureOut,
    #[gpu(binding = 2, uniform)]
    params: Uniform<ClaheParams>,
}

#[cfg(feature = "gpu")]
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct ClaheParams {
    width: u32,
    height: u32,
    tile_grid: u32,
    _pad0: u32,
    clip_limit: f32,
    _pad1: [u32; 3],
}

mod edges;
mod filters;
mod geometry;
mod helpers;
mod morph;
mod undistort;

// Lift helper functions into module scope so submodules can keep calling them unqualified.
#[allow(unused_imports)]
use helpers::*;

use daedalus::runtime::plugins::{Plugin, PluginRegistry};

#[derive(Clone, Debug, Default)]
pub struct CvImagePlugin;

impl CvImagePlugin {
    pub fn install(&self, registry: &mut PluginRegistry) -> Result<(), &'static str> {
        registry.merge::<morph::cv_erode>()?;
        registry.merge::<morph::cv_dilate>()?;
        registry.merge::<morph::cv_open>()?;
        registry.merge::<morph::cv_close>()?;
        registry.merge::<morph::cv_tophat>()?;
        registry.merge::<morph::cv_blackhat>()?;
        registry.merge::<morph::cv_gradient>()?;
        registry.merge::<morph::cv_ingradient>()?;
        registry.merge::<morph::cv_exgradient>()?;
        registry.merge::<morph::cv_outline>()?;
        registry.merge::<morph::cv_skeleton>()?;
        registry.merge::<morph::cv_component_filter>()?;
        registry.merge::<morph::cv_component_features>()?;

        registry.merge::<cv_blur>()?;

        registry.merge::<filters::cv_binary>()?;
        registry.merge::<filters::cv_binary_mask>()?;
        registry.merge::<filters::cv_otsu>()?;
        registry.merge::<filters::cv_otsu_level>()?;
        registry.merge::<filters::cv_adaptive_threshold>()?;
        registry.merge::<filters::cv_clahe>()?;
        registry.merge::<filters::cv_equalize>()?;
        registry.merge::<filters::cv_gamma>()?;
        registry.merge::<filters::cv_sobel>()?;
        registry.merge::<filters::cv_convolution3x3>()?;
        registry.merge::<filters::cv_guided_filter>()?;

        registry.merge::<undistort::cv_undistort>()?;
        registry.merge::<undistort::cv_undistort_optional>()?;

        registry.merge::<edges::cv_canny_prep>()?;
        registry.merge::<edges::cv_emboss>()?;
        registry.merge::<edges::cv_sharpen>()?;
        registry.merge::<edges::cv_laplacian>()?;
        registry.merge::<edges::cv_weighted_blur3>()?;

        registry.merge::<geometry::cv_resize>()?;
        registry.merge::<geometry::cv_downscale>()?;
        registry.merge::<geometry::cv_invert>()?;
        registry.merge::<geometry::cv_rotate90>()?;
        registry.merge::<geometry::cv_crop>()?;
        registry.merge::<geometry::cv_crop_roi>()?;
        registry.merge::<geometry::cv_passthrough_roi>()?;
        registry.merge::<geometry::cv_passthrough_roi_gray>()?;
        registry.merge::<geometry::cv_crop_roi_gray>()?;
        registry.merge::<geometry::cv_roi_offsets>()?;
        registry.merge::<geometry::cv_roi>()?;
        registry.merge::<geometry::cv_skew>()?;
        registry.merge::<geometry::cv_setpixel>()?;
        registry.merge::<geometry::cv_getpixel>()?;
        registry.merge::<geometry::cv_tap_contours>()?;
        registry.merge::<geometry::cv_tap_json>()?;

        Ok(())
    }
}

impl Plugin for CvImagePlugin {
    fn id(&self) -> &'static str {
        "image"
    }

    fn install(&self, registry: &mut PluginRegistry) -> Result<(), &'static str> {
        self.install(registry)
    }
}
