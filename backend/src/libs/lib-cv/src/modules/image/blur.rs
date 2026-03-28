use std::num::NonZero;
use std::sync::{Mutex, OnceLock};

use image::{DynamicImage, GrayImage};
use libblur::{AnisotropicRadius, BlurImageMut, EdgeMode, EdgeMode2D, FastBlurChannels, ThreadingPolicy, fast_gaussian_next, fast_gaussian_next_blur_image};
use rayon::current_num_threads;

pub fn blur_image(image: DynamicImage, sigma: f32) -> DynamicImage {
    if sigma <= 0.0 {
        return image;
    }
    let threads = NonZero::new(current_num_threads()).unwrap_or(NonZero::new(1).unwrap());

    fast_gaussian_next_blur_image(image, AnisotropicRadius::new(sigma as u32), EdgeMode2D::new(EdgeMode::Clamp), libblur::ThreadingPolicy::Fixed(threads)).unwrap()
}

pub fn blur_gray_image(gray: GrayImage, sigma: f32) -> GrayImage {
    let mut gray = gray;
    blur_gray_image_in_place(&mut gray, sigma);
    gray
}

pub fn blur_gray_image_in_place(gray: &mut GrayImage, sigma: f32) {
    let (width, height) = gray.dimensions();
    if width == 0 || height == 0 {
        return;
    }
    let sigma = sigma.max(0.0);
    if sigma <= 0.0 {
        return;
    }
    if sigma <= 1.5 {
        let radius = sigma.round().clamp(1.0, 2.0) as u32;
        box_blur_gray_in_place(gray, radius);
        return;
    }

    let threads = NonZero::new(current_num_threads()).unwrap_or(NonZero::new(1).unwrap());
    let mut image = BlurImageMut::borrow(gray.as_mut(), width, height, FastBlurChannels::Plane);
    fast_gaussian_next(&mut image, AnisotropicRadius::new(sigma as u32), ThreadingPolicy::Fixed(threads), EdgeMode2D::new(EdgeMode::Clamp)).unwrap();
}

const BOX_BLUR_SCRATCH_RETAIN_CAP: usize = 2 * 1024 * 1024;

fn box_blur_scratch_pool() -> &'static Mutex<Option<Vec<u8>>> {
    static POOL: OnceLock<Mutex<Option<Vec<u8>>>> = OnceLock::new();
    POOL.get_or_init(|| Mutex::new(None))
}

fn with_box_blur_scratch<R>(needed: usize, f: impl FnOnce(&mut Vec<u8>) -> R) -> R {
    let mut scratch = box_blur_scratch_pool().lock().ok().and_then(|mut slot| slot.take()).unwrap_or_default();
    if scratch.len() != needed {
        scratch.resize(needed, 0);
    }
    let out = f(&mut scratch);
    scratch.clear();
    if scratch.capacity() <= BOX_BLUR_SCRATCH_RETAIN_CAP
        && let Ok(mut slot) = box_blur_scratch_pool().lock()
        && slot.is_none()
    {
        *slot = Some(scratch);
    }
    out
}

pub(crate) fn compact_blur_scratch_after_frame() {
    if let Ok(mut slot) = box_blur_scratch_pool().lock() {
        if slot.as_ref().is_some_and(|scratch| scratch.capacity() > BOX_BLUR_SCRATCH_RETAIN_CAP) {
            *slot = None;
        }
    }
}

pub(crate) fn release_blur_scratch_on_idle() {
    if let Ok(mut slot) = box_blur_scratch_pool().lock() {
        *slot = None;
    }
}

fn box_blur_gray_in_place(gray: &mut GrayImage, radius: u32) {
    if radius == 0 {
        return;
    }
    let (width, height) = gray.dimensions();
    if width == 0 || height == 0 {
        return;
    }
    let width_usize = width as usize;
    let height_usize = height as usize;
    let win = radius * 2 + 1;
    let radius_usize = radius as usize;
    let last_x = width_usize.saturating_sub(1);
    let last_y = height_usize.saturating_sub(1);

    let needed = width_usize.saturating_mul(height_usize);
    with_box_blur_scratch(needed, |scratch| {
        let tmp = scratch.as_mut_slice();
        let src = gray.as_raw();

        for y in 0..height_usize {
            let row = &src[y * width_usize..(y + 1) * width_usize];
            let tmp_row = &mut tmp[y * width_usize..(y + 1) * width_usize];
            let mut sum = row[0] as u32 * (radius_usize as u32 + 1);
            for i in 1..=radius_usize {
                sum += row[i.min(last_x)] as u32;
            }
            for (x, out_px) in tmp_row.iter_mut().enumerate() {
                *out_px = (sum / win) as u8;
                let add_x = (x + radius_usize + 1).min(last_x);
                let sub_x = x.saturating_sub(radius_usize);
                sum += row[add_x] as u32;
                sum -= row[sub_x] as u32;
            }
        }
        let out = gray.as_flat_samples_mut().samples;
        for x in 0..width_usize {
            let mut sum = tmp[x] as u32 * (radius_usize as u32 + 1);
            for i in 1..=radius_usize {
                let y = i.min(last_y);
                sum += tmp[y * width_usize + x] as u32;
            }
            for y in 0..height_usize {
                let idx = y * width_usize + x;
                out[idx] = (sum / win) as u8;
                let add_y = (y + radius_usize + 1).min(last_y);
                let sub_y = y.saturating_sub(radius_usize);
                sum += tmp[add_y * width_usize + x] as u32;
                sum -= tmp[sub_y * width_usize + x] as u32;
            }
        }
    });
}

#[cfg(feature = "engine")]
pub mod nodes {
    #![allow(clippy::needless_return, clippy::collapsible_if)]

    #[cfg(feature = "gpu")]
    use super::blur_gray_image_in_place;
    use super::blur_image;
    use daedalus::core::compute::ComputeAffinity;
    use daedalus::declare_plugin;
    use daedalus::macros::node;
    use daedalus::runtime::NodeError;
    use image::DynamicImage;
    #[cfg(feature = "gpu")]
    use image::GenericImageView;
    #[cfg(feature = "gpu")]
    use image::RgbaImage;

    use crate::plugin::ExecMode;

    #[cfg(feature = "gpu")]
    use bytemuck::{Pod, Zeroable};
    use daedalus::gpu::Compute;
    #[cfg(feature = "gpu")]
    use daedalus::gpu::shader::{ShaderContext, TextureOut, Uniform};
    #[cfg(feature = "gpu")]
    use daedalus::macros::GpuBindings;
    use daedalus::runtime::state::ExecutionContext;

    #[cfg(feature = "gpu")]
    const MAX_KERNEL_RADIUS: u32 = 12;
    #[cfg(feature = "gpu")]
    const MIN_BLUR_SIGMA: f32 = 0.3;

    #[cfg(feature = "gpu")]
    #[repr(C)]
    #[derive(Copy, Clone, Pod, Zeroable)]
    struct BlurParams {
        width: u32,
        height: u32,
        radius: u32,
        kernel_len: u32,
    }

    #[cfg(feature = "gpu")]
    #[derive(GpuBindings)]
    #[gpu(spec(src = "src/gpu/shaders/blur.wgsl", entry = "blur_horizontal_main"))]
    struct BlurHorizontalShaderBindings<'a> {
        #[gpu(binding = 0, texture2d(format = "rgba8unorm"))]
        input: &'a Compute<DynamicImage>,
        #[gpu(binding = 1, texture2d(format = "rgba8unorm", write))]
        output: TextureOut,
        #[gpu(binding = 2, storage(read))]
        weights: &'a [f32],
        #[gpu(binding = 3, uniform)]
        params: Uniform<BlurParams>,
    }

    #[cfg(feature = "gpu")]
    #[derive(GpuBindings)]
    #[gpu(spec(src = "src/gpu/shaders/blur.wgsl", entry = "blur_vertical_main"))]
    struct BlurVerticalShaderBindings<'a> {
        #[gpu(binding = 0, texture2d(format = "rgba8unorm"))]
        input: &'a Compute<DynamicImage>,
        #[gpu(binding = 1, texture2d(format = "rgba8unorm", write))]
        output: TextureOut,
        #[gpu(binding = 2, storage(read))]
        weights: &'a [f32],
        #[gpu(binding = 3, uniform)]
        params: Uniform<BlurParams>,
    }

    #[cfg_attr(
        feature = "gpu",
        node(
            id = "blur",
            compute(ComputeAffinity::GpuPreferred),
            inputs("frame", port(name = "sigma", meta(ui_min = 0.3, ui_max = 12.0, ui_step = 0.1)), port(name = "mode", default = "auto")),
            outputs("frame"),
            shaders(BlurHorizontalShaderBindings, BlurVerticalShaderBindings)
        )
    )]
    #[cfg_attr(
        not(feature = "gpu"),
        node(
            id = "blur",
            compute(ComputeAffinity::GpuPreferred),
            inputs("frame", port(name = "sigma", meta(ui_min = 0.3, ui_max = 12.0, ui_step = 0.1)), port(name = "mode", default = "auto")),
            outputs("frame")
        )
    )]
    pub fn cv_blur(frame: Compute<DynamicImage>, sigma: f32, mode: ExecMode, #[cfg(feature = "gpu")] ctx: ShaderContext, _exec_ctx: &ExecutionContext) -> Result<Compute<DynamicImage>, NodeError> {
        if sigma <= 0.0 {
            return Ok(frame);
        }
        #[cfg(feature = "gpu")]
        {
            let cpu_fallback = || -> Result<Compute<DynamicImage>, NodeError> {
                if let Some(cpu) = frame.as_cpu() {
                    if let DynamicImage::ImageLuma8(gray) = cpu {
                        let mut gray = gray.clone();
                        blur_gray_image_in_place(&mut gray, sigma);
                        return Ok(Compute::Cpu(DynamicImage::ImageLuma8(gray)));
                    }
                    return Ok(Compute::Cpu(blur_image(cpu.clone(), sigma)));
                }

                let (bytes, w, h) = frame.to_rgba_bytes(ctx.gpu.as_ref()).map_err(|e| NodeError::Handler(format!("blur: {e}")))?;
                let rgba = RgbaImage::from_raw(w, h, bytes).ok_or_else(|| NodeError::Handler("blur: invalid image dimensions".into()))?;
                Ok(Compute::Cpu(blur_image(DynamicImage::ImageRgba8(rgba), sigma)))
            };

            let (width, height) = frame.dimensions();
            if width == 0 || height == 0 {
                return Ok(Compute::Cpu(DynamicImage::new_rgba8(width, height)));
            }

            let want_gpu = matches!(mode, ExecMode::Gpu | ExecMode::Auto) && ctx.gpu.is_some();
            if want_gpu {
                if let Some((weights, radius)) = build_gaussian_kernel(sigma) {
                    let params = BlurParams { width, height, radius, kernel_len: weights.len() as u32 };
                    let horiz = BlurHorizontalShaderBindings { input: &frame, output: TextureOut::from_input_ctx(&frame, &ctx), weights: &weights, params: Uniform::new(params) };
                    let temp = ctx
                        .single(&horiz)
                        .dispatch_auto()
                        .and_then(|out| out.into_payload_with_ctx(1, &ctx, width, height))
                        .map_err(|e| NodeError::Handler(format!("blur (gpu horizontal): {e}")))
                        .or_else(|_| cpu_fallback())?;

                    let vert = BlurVerticalShaderBindings { input: &temp, output: TextureOut::from_input_ctx(&frame, &ctx), weights: &weights, params: Uniform::new(params) };

                    return ctx
                        .single(&vert)
                        .dispatch_auto()
                        .and_then(|out| out.into_payload_with_ctx(1, &ctx, width, height))
                        .map_err(|e| NodeError::Handler(format!("blur (gpu vertical): {e}")))
                        .or_else(|_| cpu_fallback());
                }
            }

            return match mode {
                ExecMode::Cpu => cpu_fallback(),
                ExecMode::Gpu => Err(NodeError::Handler("blur: GPU requested but unavailable".into())),
                ExecMode::Auto => cpu_fallback(),
            };
        }

        #[cfg(not(feature = "gpu"))]
        {
            let _ = mode;
            match frame {
                Compute::Cpu(image) => Ok(Compute::Cpu(blur_image(image, sigma))),
                Compute::Gpu(_) => Err(NodeError::Handler("blur: GPU payload unsupported in CPU-only build".into())),
            }
        }
    }

    #[cfg(feature = "gpu")]
    fn build_gaussian_kernel(sigma: f32) -> Option<(Vec<f32>, u32)> {
        let sigma = sigma.clamp(MIN_BLUR_SIGMA, 25.0);
        let mut radius = (sigma * 3.0).ceil() as u32;
        radius = radius.clamp(1, MAX_KERNEL_RADIUS);
        let kernel_len = 2 * radius + 1;
        let mut weights = Vec::with_capacity(kernel_len as usize);
        let mut sum = 0.0;
        for offset in -(radius as i32)..=(radius as i32) {
            let dist = offset as f32;
            let weight = (-dist * dist / (2.0 * sigma * sigma)).exp();
            weights.push(weight);
            sum += weight;
        }
        if sum == 0.0 {
            return None;
        }
        for w in &mut weights {
            *w /= sum;
        }
        Some((weights, radius))
    }

    declare_plugin!(CvBlurPlugin, "image.blur", [cv_blur]);
}
