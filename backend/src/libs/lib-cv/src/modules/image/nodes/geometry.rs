#![allow(unsafe_code)]

use super::*;

#[node(id = "resize", inputs("frame", port(name = "width", meta(ui_min = 1, ui_max = 4096, ui_step = 1)), port(name = "height", meta(ui_min = 1, ui_max = 4096, ui_step = 1))), outputs("frame"))]
fn cv_resize(frame: DynamicImage, width: u32, height: u32) -> Result<DynamicImage, NodeError> {
    if frame.width() == width && frame.height() == height {
        return Ok(frame);
    }
    Ok(resize_fast(&frame, width, height))
}

// Backwards-compat: older pipeline graphs reference `cv:image:to_gray`.
#[node(id = "to_gray", inputs("frame"), outputs("mask"))]
fn cv_to_gray(frame: DynamicImage) -> Result<GrayImage, NodeError> {
    Ok(frame.to_luma8())
}

#[cfg_attr(
    feature = "gpu",
    node(
        id = "downscale",
        compute(ComputeAffinity::GpuPreferred),
        inputs("frame", port(name = "factor", default = 2i64, meta(ui_min = 1, ui_max = 8, ui_step = 1)), port(name = "mode", default = "auto")),
        outputs("frame", "factor"),
        shaders(DownscaleShaderBindings)
    )
)]
#[cfg_attr(
    not(feature = "gpu"),
    node(
        id = "downscale",
        compute(ComputeAffinity::GpuPreferred),
        inputs("frame", port(name = "factor", default = 2i64, meta(ui_min = 1, ui_max = 8, ui_step = 1)), port(name = "mode", default = "auto")),
        outputs("frame", "factor")
    )
)]
fn cv_downscale(
    frame: Payload<DynamicImage>,
    factor: i64,
    mode: ExecMode,
    #[cfg(feature = "gpu")] ctx: ShaderContext,
    _exec_ctx: &ExecutionContext,
) -> Result<(Payload<DynamicImage>, i64), NodeError> {
    let factor = u32::try_from(factor).unwrap_or(1).max(1);
    let factor_out = i64::from(factor);
    if factor == 1 {
        return Ok((frame, factor_out));
    }

    #[cfg(feature = "gpu")]
    {
        let (width, height) = frame.dimensions();
        let out_width = (width / factor).max(1);
        let out_height = (height / factor).max(1);
        if width == 0 || height == 0 {
            return Ok((Payload::Cpu(DynamicImage::new_rgba8(width, height)), factor_out));
        }

        if matches!(mode, ExecMode::Gpu) && ctx.gpu.is_none() {
            return Err(NodeError::Handler("downscale: GPU requested but unavailable".into()));
        }

        let want_gpu = matches!(mode, ExecMode::Gpu | ExecMode::Auto) && ctx.gpu.is_some();
        if want_gpu {
            let params = DownscaleParams { in_width: width, in_height: height, out_width, out_height, factor, _pad: [0; 3] };
            let bindings = DownscaleShaderBindings { input: &frame, output: TextureOut::write(out_width, out_height), params: Uniform::new(params) };

            if let Ok(out) = ctx.single(&bindings).dispatch_auto().and_then(|out| out.into_payload_with_ctx(1, &ctx, out_width, out_height)) {
                return Ok((out, factor_out));
            }
        }
    }

    #[cfg(not(feature = "gpu"))]
    {
        let _ = mode;
    }

    let frame = expect_cpu(frame, "downscale", Some(_exec_ctx))?;
    let (width, height) = frame.dimensions();
    let dst_width = (width / factor).max(1);
    let dst_height = (height / factor).max(1);
    let resized = match frame {
        DynamicImage::ImageLuma8(gray) => DynamicImage::ImageLuma8(downscale_luma8_in_place(gray, dst_width, dst_height)),
        other => resize_fast(&other, dst_width, dst_height),
    };
    Ok((Payload::Cpu(resized), factor_out))
}

#[node(id = "invert", inputs("mask"), outputs("mask"))]
#[allow(unsafe_code)]
fn cv_invert(mask: image::GrayImage) -> Result<image::GrayImage, NodeError> {
    if mask.as_raw().is_empty() {
        return Ok(mask);
    }
    let mut out = mask;
    #[cfg(target_arch = "aarch64")]
    {
        if crate::simd::neon_enabled() {
            // SAFETY: guarded by runtime feature detection and aarch64 target.
            unsafe {
                use core::arch::aarch64::*;
                let buf = out.as_mut();
                let len = buf.len();
                let ptr = buf.as_mut_ptr();
                let mut i = 0usize;
                while i + 64 <= len {
                    let v0 = vld1q_u8(ptr.add(i));
                    let v1 = vld1q_u8(ptr.add(i + 16));
                    let v2 = vld1q_u8(ptr.add(i + 32));
                    let v3 = vld1q_u8(ptr.add(i + 48));
                    vst1q_u8(ptr.add(i), vmvnq_u8(v0));
                    vst1q_u8(ptr.add(i + 16), vmvnq_u8(v1));
                    vst1q_u8(ptr.add(i + 32), vmvnq_u8(v2));
                    vst1q_u8(ptr.add(i + 48), vmvnq_u8(v3));
                    i += 64;
                }
                while i + 16 <= len {
                    let v = vld1q_u8(ptr.add(i));
                    vst1q_u8(ptr.add(i), vmvnq_u8(v));
                    i += 16;
                }
                while i < len {
                    *ptr.add(i) = 255u8.wrapping_sub(*ptr.add(i));
                    i += 1;
                }
            }
            return Ok(out);
        }
    }
    let buf = out.as_mut();
    let (prefix, words, suffix) = unsafe { buf.align_to_mut::<u64>() };
    for v in prefix {
        *v = 255u8.wrapping_sub(*v);
    }
    for w in words {
        *w = !*w;
    }
    for v in suffix {
        *v = 255u8.wrapping_sub(*v);
    }
    Ok(out)
}

#[node(id = "rotate90", inputs("frame"), outputs("frame"))]
fn cv_rotate90(frame: DynamicImage) -> Result<DynamicImage, NodeError> {
    Ok(rotate_fast(frame, Rotation::Deg90))
}

#[node(
    id = "crop",
    inputs(
        "frame",
        port(name = "x", meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
        port(name = "y", meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
        port(name = "width", meta(ui_min = 1, ui_max = 4096, ui_step = 1)),
        port(name = "height", meta(ui_min = 1, ui_max = 4096, ui_step = 1))
    ),
    outputs("frame")
)]
fn cv_crop(frame: DynamicImage, x: u32, y: u32, width: u32, height: u32) -> Result<DynamicImage, NodeError> {
    let (fw, fh) = frame.dimensions();
    if width == 0 || height == 0 || x >= fw || y >= fh {
        return Ok(DynamicImage::new_rgba8(0, 0));
    }
    let w = width.min(fw.saturating_sub(x));
    let h = height.min(fh.saturating_sub(y));
    Ok(frame.crop_imm(x, y, w, h))
}

#[node(
    id = "crop_roi",
    summary = "Crop to ROI bounds with passthrough disable semantics.",
    description = "Crops the frame to roi_x/roi_y/roi_w/roi_h. If roi_w or roi_h is <= 0, ROI is treated as disabled and the full frame is passed through with zero offsets.",
    inputs(
        "frame",
        port(name = "roi_x", default = 0i64, meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
        port(name = "roi_y", default = 0i64, meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
        port(name = "roi_w", default = 0i64, meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
        port(name = "roi_h", default = 0i64, meta(ui_min = 0, ui_max = 4096, ui_step = 1))
    ),
    outputs("frame", "offset_x", "offset_y")
)]
fn cv_crop_roi(frame: DynamicImage, roi_x: i64, roi_y: i64, roi_w: i64, roi_h: i64) -> Result<(DynamicImage, i64, i64), NodeError> {
    let (fw, fh) = frame.dimensions();
    if fw == 0 || fh == 0 {
        return Ok((frame, 0, 0));
    }

    if let Some((x, y, w, h)) = roi_crop_bounds(fw, fh, roi_x, roi_y, roi_w, roi_h) { Ok((frame.crop_imm(x, y, w, h), i64::from(x), i64::from(y))) } else { Ok((frame, 0, 0)) }
}

#[node(
    id = "crop_roi_gray",
    summary = "Crop to ROI bounds as grayscale with passthrough disable semantics.",
    description = "Crops the frame to roi_x/roi_y/roi_w/roi_h and converts directly to GrayImage. If roi_w or roi_h is <= 0, ROI is treated as disabled and the full grayscale frame is returned with zero offsets.",
    inputs(
        "frame",
        port(name = "roi_x", default = 0i64, meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
        port(name = "roi_y", default = 0i64, meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
        port(name = "roi_w", default = 0i64, meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
        port(name = "roi_h", default = 0i64, meta(ui_min = 0, ui_max = 4096, ui_step = 1))
    ),
    outputs("frame", "offset_x", "offset_y")
)]
fn cv_crop_roi_gray(frame: &DynamicImage, roi_x: i64, roi_y: i64, roi_w: i64, roi_h: i64) -> Result<(crate::modules::image::luma::PooledGrayImage, i64, i64), NodeError> {
    let (fw, fh) = frame.dimensions();
    if fw == 0 || fh == 0 {
        return Ok((crate::modules::image::luma::PooledGrayImage::new(0, 0), 0, 0));
    }

    if let Some((x, y, w, h)) = roi_crop_bounds(fw, fh, roi_x, roi_y, roi_w, roi_h) {
        Ok((crop_luma8_frame(&frame, x, y, w, h), i64::from(x), i64::from(y)))
    } else {
        Ok((crop_luma8_frame(&frame, 0, 0, fw, fh), 0, 0))
    }
}

#[node(
    id = "roi_offsets",
    summary = "Resolve ROI offsets without materializing an ROI image.",
    description = "Computes the clamped ROI origin used by crop_roi/crop_roi_gray. If ROI is disabled or spans the full frame, returns zero offsets.",
    inputs(
        "frame",
        port(name = "roi_x", default = 0i64, meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
        port(name = "roi_y", default = 0i64, meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
        port(name = "roi_w", default = 0i64, meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
        port(name = "roi_h", default = 0i64, meta(ui_min = 0, ui_max = 4096, ui_step = 1))
    ),
    outputs("offset_x", "offset_y")
)]
fn cv_roi_offsets(frame: &DynamicImage, roi_x: i64, roi_y: i64, roi_w: i64, roi_h: i64) -> Result<(i64, i64), NodeError> {
    let (fw, fh) = frame.dimensions();
    if fw == 0 || fh == 0 {
        return Ok((0, 0));
    }

    if let Some((x, y, _, _)) = roi_crop_bounds(fw, fh, roi_x, roi_y, roi_w, roi_h) { Ok((i64::from(x), i64::from(y))) } else { Ok((0, 0)) }
}

fn roi_crop_bounds(fw: u32, fh: u32, roi_x: i64, roi_y: i64, roi_w: i64, roi_h: i64) -> Option<(u32, u32, u32, u32)> {
    if roi_w <= 0 || roi_h <= 0 {
        return None;
    }

    let x = roi_x.max(0);
    let y = roi_y.max(0);
    if x >= i64::from(fw) || y >= i64::from(fh) {
        return None;
    }

    let max_w = i64::from(fw) - x;
    let max_h = i64::from(fh) - y;
    if max_w <= 0 || max_h <= 0 {
        return None;
    }

    let w = roi_w.max(1).min(max_w) as u32;
    let h = roi_h.max(1).min(max_h) as u32;
    let x = x as u32;
    let y = y as u32;
    if x == 0 && y == 0 && w == fw && h == fh { None } else { Some((x, y, w, h)) }
}

#[node(
    id = "roi",
    inputs(
        "frame",
        port(name = "left_pct", default = 0.0f64, meta(ui_min = 0.0, ui_max = 100.0, ui_step = 0.5)),
        port(name = "right_pct", default = 0.0f64, meta(ui_min = 0.0, ui_max = 100.0, ui_step = 0.5)),
        port(name = "top_pct", default = 0.0f64, meta(ui_min = 0.0, ui_max = 100.0, ui_step = 0.5)),
        port(name = "bottom_pct", default = 0.0f64, meta(ui_min = 0.0, ui_max = 100.0, ui_step = 0.5))
    ),
    outputs("frame")
)]
fn cv_roi(frame: DynamicImage, left_pct: f64, right_pct: f64, top_pct: f64, bottom_pct: f64) -> Result<DynamicImage, NodeError> {
    let (fw, fh) = frame.dimensions();
    if fw == 0 || fh == 0 {
        return Ok(DynamicImage::new_rgba8(0, 0));
    }

    let left_px = ((fw as f64) * (left_pct.clamp(0.0, 100.0) / 100.0)).round() as u32;
    let right_px = ((fw as f64) * (right_pct.clamp(0.0, 100.0) / 100.0)).round() as u32;
    let top_px = ((fh as f64) * (top_pct.clamp(0.0, 100.0) / 100.0)).round() as u32;
    let bottom_px = ((fh as f64) * (bottom_pct.clamp(0.0, 100.0) / 100.0)).round() as u32;

    let w = fw.saturating_sub(left_px.saturating_add(right_px));
    let h = fh.saturating_sub(top_px.saturating_add(bottom_px));
    if w == 0 || h == 0 {
        return Ok(DynamicImage::new_rgba8(0, 0));
    }

    Ok(frame.crop_imm(left_px, top_px, w, h))
}

#[node(
    id = "skew",
    inputs(
        "frame",
        port(name = "x_skew", meta(ui_min = -1.0, ui_max = 1.0, ui_step = 0.01)),
        port(name = "y_skew", meta(ui_min = -1.0, ui_max = 1.0, ui_step = 0.01))
    ),
    outputs("frame")
)]
fn cv_skew(frame: DynamicImage, x_skew: f32, y_skew: f32) -> Result<DynamicImage, NodeError> {
    let proj = Projection::from_matrix([1.0, x_skew, 0.0, y_skew, 1.0, 0.0, 0.0, 0.0, 1.0]).ok_or_else(|| NodeError::InvalidInput("invalid projection".into()))?;
    let src = frame.to_rgba8();
    let (w, h) = src.dimensions();
    let mut out: RgbaImage = RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 0]));
    warp_into(&src, &proj, Interpolation::Bilinear, Rgba([0, 0, 0, 0]), &mut out);
    Ok(DynamicImage::ImageRgba8(out))
}

#[node(id = "setpixel", inputs("frame", port(name = "x", meta(ui_min = 0, ui_max = 4096, ui_step = 1)), port(name = "y", meta(ui_min = 0, ui_max = 4096, ui_step = 1)), "pixel"), outputs("frame"))]
fn cv_setpixel(frame: DynamicImage, x: u32, y: u32, pixel: Pixel) -> Result<DynamicImage, NodeError> {
    let mut img = frame.to_rgba8();
    if x < img.width() && y < img.height() {
        img.put_pixel(x, y, pixel.into());
    }
    Ok(DynamicImage::ImageRgba8(img))
}

#[node(id = "getpixel", inputs("frame", port(name = "x", meta(ui_min = 0, ui_max = 4096, ui_step = 1)), port(name = "y", meta(ui_min = 0, ui_max = 4096, ui_step = 1))), outputs("pixel"))]
fn cv_getpixel(frame: DynamicImage, x: u32, y: u32) -> Result<Pixel, NodeError> {
    let img = frame.to_rgba8();
    if x < img.width() && y < img.height() { Ok(Pixel::from(*img.get_pixel(x, y))) } else { Ok(Pixel::default()) }
}

#[node(
    id = "tap_contours",
    inputs(
        "frame",
        port(name = "contours", source = "Contours", ty = crate::daedalus_types::contours())
    ),
    outputs("frame")
)]
fn cv_tap_contours(frame: DynamicImage, contours: &Vec<Vec<crate::Point>>) -> Result<DynamicImage, NodeError> {
    let _ = contours;
    Ok(frame)
}

#[node(
    id = "tap_json",
    inputs(
        "frame",
        port(name = "json", source = "Json", ty = crate::daedalus_types::json_value())
    ),
    outputs("frame")
)]
fn cv_tap_json(frame: DynamicImage, json: String) -> Result<DynamicImage, NodeError> {
    let _ = json;
    Ok(frame)
}
