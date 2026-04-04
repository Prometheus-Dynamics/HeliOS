#[cfg(feature = "aruco")]
pub mod aruco;
#[cfg(feature = "contour")]
pub mod contour;
pub mod font;
pub mod line;
pub mod point;
pub mod shape;
pub mod text;
pub mod utils;

#[cfg(feature = "engine")]
pub mod nodes {
    use crate::{Pixel, Point};
    use daedalus::data::model::Value as DaedalusValue;
    use daedalus::declare_plugin;
    use daedalus::macros::{NodeConfig, node};
    use daedalus::runtime::NodeError;
    use daedalus::runtime::state::ExecutionContext;
    #[cfg(feature = "gpu")]
    use image::RgbaImage;
    use image::{DynamicImage, GenericImageView, Rgba};
    use imageproc::point::Point as CvPoint;

    use crate::draw::{
        contour::overlay_contour_points,
        font::FontType,
        line::{overlay_crosshair_x_y, overlay_line_x_y},
        point::{overlay_point_x_y, overlay_points},
        shape::{overlay_circle, overlay_ellipse, overlay_rect},
        text::{overlay_text_scaled, overlay_text_x_y},
        utils::overlay_dynamic,
    };

    use daedalus::gpu::Compute;

    fn expect_cpu_frame(frame: Compute<DynamicImage>, label: &str, exec_ctx: Option<&ExecutionContext>) -> Result<DynamicImage, NodeError> {
        #[cfg(feature = "gpu")]
        {
            match frame {
                Compute::Cpu(img) => Ok(img),
                Compute::Gpu(handle) => {
                    let ctx = exec_ctx.and_then(|ctx| ctx.gpu.as_ref()).ok_or_else(|| NodeError::Handler(format!("{label}: gpu payload missing context")))?;
                    let bytes = ctx.read_texture(&handle).map_err(|e| NodeError::Handler(format!("{label}: {e}")))?;
                    let rgba = RgbaImage::from_raw(handle.width, handle.height, bytes).ok_or_else(|| NodeError::Handler(format!("{label}: invalid image dimensions")))?;
                    Ok(DynamicImage::ImageRgba8(rgba))
                }
            }
        }
        #[cfg(not(feature = "gpu"))]
        {
            let _ = exec_ctx;
            match frame {
                Compute::Cpu(img) => Ok(img),
                Compute::Gpu(_) => Err(NodeError::Handler(format!("{label}: GPU payload unsupported (insert cpu convert)"))),
            }
        }
    }

    fn resolved_color(pixel: Pixel, fallback: [u8; 4]) -> Rgba<u8> {
        if pixel.r == 0 && pixel.g == 0 && pixel.b == 0 && pixel.a == 0 { Rgba(fallback) } else { pixel.into() }
    }

    fn pixel_from_value(value: &DaedalusValue) -> Option<Pixel> {
        match value {
            DaedalusValue::Struct(fields) => {
                let mut pixel = Pixel::default();
                for field in fields {
                    let v = match &field.value {
                        DaedalusValue::Int(i) => (*i).clamp(0, 255) as u8,
                        _ => continue,
                    };
                    match field.name.as_str() {
                        "r" => pixel.r = v,
                        "g" => pixel.g = v,
                        "b" => pixel.b = v,
                        "a" => pixel.a = v,
                        _ => {}
                    }
                }
                Some(pixel)
            }
            _ => None,
        }
    }

    #[node(
        id = "drawtext",
        inputs(
            "frame",
            "text",
            port(name = "x", meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
            port(name = "y", meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
            port(name = "scale", meta(ui_min = 1.0, ui_max = 64.0, ui_step = 1.0)),
            "color"
        ),
        outputs("frame")
    )]
    fn draw_text(frame: DynamicImage, text: String, x: u32, y: u32, scale: f32, color: Pixel) -> Result<DynamicImage, NodeError> {
        if text.is_empty() {
            return Ok(frame);
        }
        let mut out = frame;
        let color = resolved_color(color, [255, 255, 255, 255]);
        overlay_text_x_y(&mut out, text.as_str(), (x, y), &FontType::RobotoBlack, scale.max(0.0) as u32, color);
        Ok(out)
    }

    #[node(
        id = "drawpoint",
        inputs(
            "frame",
            port(name = "x", meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
            port(name = "y", meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
            port(name = "thickness", meta(ui_min = 1, ui_max = 16, ui_step = 1)),
            "color"
        ),
        outputs("frame")
    )]
    fn draw_point(frame: DynamicImage, x: u32, y: u32, thickness: u32, color: Pixel) -> Result<DynamicImage, NodeError> {
        let mut out = frame;
        let color = resolved_color(color, [255, 0, 0, 255]);
        overlay_point_x_y(&mut out, (x, y), thickness.max(1), color);
        Ok(out)
    }

    #[node(
        id = "drawline",
        inputs(
            "frame",
            port(name = "x1", meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
            port(name = "y1", meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
            port(name = "x2", meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
            port(name = "y2", meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
            port(name = "thickness", meta(ui_min = 1, ui_max = 16, ui_step = 1)),
            "color"
        ),
        outputs("frame")
    )]
    fn draw_line(frame: DynamicImage, x1: u32, y1: u32, x2: u32, y2: u32, thickness: u32, color: Pixel) -> Result<DynamicImage, NodeError> {
        let mut out = frame;
        let color = resolved_color(color, [0, 255, 0, 255]);
        overlay_line_x_y(&mut out, (x1, y1), (x2, y2), thickness.max(1), color);
        Ok(out)
    }

    #[node(
        id = "drawcrosshair",
        inputs("frame", port(name = "size", meta(ui_min = 1, ui_max = 2048, ui_step = 1)), port(name = "thickness", meta(ui_min = 1, ui_max = 16, ui_step = 1)), "color"),
        outputs("frame")
    )]
    fn draw_crosshair(frame: DynamicImage, size: u32, thickness: u32, color: Pixel) -> Result<DynamicImage, NodeError> {
        let mut out = frame;
        let (width, height) = out.dimensions();
        if width != 0 && height != 0 {
            let thickness = thickness.max(1);
            let arm = size.max(1) / 2;
            let cx = width / 2;
            let cy = height / 2;

            let color = resolved_color(color, [0, 255, 0, 255]);
            overlay_crosshair_x_y(&mut out, (cx, cy), arm, thickness, color);
        }
        Ok(out)
    }

    #[node(
        id = "drawcrosshairat",
        inputs(
            port(name = "frame", ty = crate::daedalus_types::image_dynamic()),
            port(name = "x", meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
            port(name = "y", meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
            port(name = "size", meta(ui_min = 1, ui_max = 2048, ui_step = 1)),
            port(name = "thickness", meta(ui_min = 1, ui_max = 16, ui_step = 1)),
            port(name = "enabled", default = true)
        ),
        outputs(port(name = "frame", ty = crate::daedalus_types::image_dynamic()))
    )]
    fn draw_crosshair_at(frame: Compute<DynamicImage>, x: i64, y: i64, size: u32, thickness: u32, enabled: bool, exec_ctx: &ExecutionContext) -> Result<Compute<DynamicImage>, NodeError> {
        if !enabled {
            return Ok(frame);
        }
        let mut out = expect_cpu_frame(frame, "drawcrosshairat", Some(exec_ctx))?;
        let preserve_luma_output = matches!(out, DynamicImage::ImageLuma8(_) | DynamicImage::ImageLumaA8(_));
        let (width, height) = out.dimensions();
        if width == 0 || height == 0 {
            return Ok(Compute::Cpu(out));
        }

        let thickness = thickness.max(1);
        let arm = size.max(1) / 2;
        let cx = x.clamp(0, i64::from(width.saturating_sub(1))) as u32;
        let cy = y.clamp(0, i64::from(height.saturating_sub(1))) as u32;

        let color = Rgba([0, 255, 0, 255]);
        overlay_crosshair_x_y(&mut out, (cx, cy), arm, thickness, color);
        if preserve_luma_output && !matches!(out, DynamicImage::ImageLuma8(_)) {
            out = DynamicImage::ImageLuma8(out.to_luma8());
        }
        Ok(Compute::Cpu(out))
    }

    #[derive(Clone, Debug, NodeConfig)]
    struct BboxAnchorPointConfig {
        #[port(default = false)]
        input_is_xyxy: bool,
        #[port(default = 0.5f64, meta(ui_min = -1.0, ui_max = 2.0, ui_step = 0.01))]
        anchor_x: f64,
        #[port(default = 0.5f64, meta(ui_min = -1.0, ui_max = 2.0, ui_step = 0.01))]
        anchor_y: f64,
        #[port(default = true)]
        clamp_anchor: bool,
    }

    #[node(
        id = "bboxanchorpoint",
        summary = "Compute an anchor point inside a bounding box.",
        description = "Returns an anchor point for a bbox. By default x/y/width/height are interpreted as XYWH; set `input_is_xyxy` to treat width/height as X2/Y2.",
        inputs(
            port(name = "x", meta(ui_min = -4096.0, ui_max = 4096.0, ui_step = 1.0)),
            port(name = "y", meta(ui_min = -4096.0, ui_max = 4096.0, ui_step = 1.0)),
            port(name = "width", meta(ui_min = -4096.0, ui_max = 4096.0, ui_step = 1.0)),
            port(name = "height", meta(ui_min = -4096.0, ui_max = 4096.0, ui_step = 1.0)),
            config = BboxAnchorPointConfig
        ),
        outputs("x", "y", "x_f", "y_f")
    )]
    fn bbox_anchor_point(x: f64, y: f64, width: f64, height: f64, cfg: BboxAnchorPointConfig) -> Result<(i64, i64, f64, f64), NodeError> {
        if !(x.is_finite() && y.is_finite() && width.is_finite() && height.is_finite()) {
            return Err(NodeError::InvalidInput("bboxanchorpoint requires finite x/y/width/height".to_string()));
        }

        let (min_x, max_x, min_y, max_y) = if cfg.input_is_xyxy {
            (x.min(width), x.max(width), y.min(height), y.max(height))
        } else {
            let x2 = x + width;
            let y2 = y + height;
            (x.min(x2), x.max(x2), y.min(y2), y.max(y2))
        };

        let ax = if cfg.clamp_anchor { cfg.anchor_x.clamp(0.0, 1.0) } else { cfg.anchor_x };
        let ay = if cfg.clamp_anchor { cfg.anchor_y.clamp(0.0, 1.0) } else { cfg.anchor_y };
        let out_x = min_x + (max_x - min_x) * ax;
        let out_y = min_y + (max_y - min_y) * ay;
        Ok((out_x.round() as i64, out_y.round() as i64, out_x, out_y))
    }

    #[node(
        id = "drawrect",
        inputs(
            "frame",
            port(name = "x", meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
            port(name = "y", meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
            port(name = "width", meta(ui_min = 1, ui_max = 4096, ui_step = 1)),
            port(name = "height", meta(ui_min = 1, ui_max = 4096, ui_step = 1)),
            "filled",
            "color"
        ),
        outputs("frame")
    )]
    fn draw_rect(frame: DynamicImage, x: u32, y: u32, width: u32, height: u32, filled: bool, color: Pixel) -> Result<DynamicImage, NodeError> {
        let mut out = frame;
        let color = resolved_color(color, [255, 0, 0, 255]);
        overlay_rect(&mut out, (x, y), (x.saturating_add(width), y.saturating_add(height)), filled, color);
        Ok(out)
    }

    #[node(
        id = "drawcircle",
        inputs(
            "frame",
            port(name = "cx", meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
            port(name = "cy", meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
            port(name = "radius", meta(ui_min = 1, ui_max = 2048, ui_step = 1)),
            "filled",
            "color"
        ),
        outputs("frame")
    )]
    fn draw_circle(frame: DynamicImage, cx: u32, cy: u32, radius: u32, filled: bool, color: Pixel) -> Result<DynamicImage, NodeError> {
        let mut out = frame;
        let color = resolved_color(color, [0, 0, 255, 255]);
        overlay_circle(&mut out, (cx, cy), radius, filled, color);
        Ok(out)
    }

    #[node(
        id = "drawellipse",
        inputs(
            "frame",
            port(name = "cx", meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
            port(name = "cy", meta(ui_min = 0, ui_max = 4096, ui_step = 1)),
            port(name = "rx", meta(ui_min = 1, ui_max = 2048, ui_step = 1)),
            port(name = "ry", meta(ui_min = 1, ui_max = 2048, ui_step = 1)),
            "filled",
            "color"
        ),
        outputs("frame")
    )]
    fn draw_ellipse(frame: DynamicImage, cx: u32, cy: u32, rx: u32, ry: u32, filled: bool, color: Pixel) -> Result<DynamicImage, NodeError> {
        let mut out = frame;
        let color = resolved_color(color, [0, 255, 255, 255]);
        overlay_ellipse(&mut out, (cx, cy), rx, ry, filled, color);
        Ok(out)
    }

    #[node(id = "drawpoints", inputs("frame", "points", port(name = "thickness", meta(ui_min = 1, ui_max = 16, ui_step = 1)), "color"), outputs("frame"))]
    #[allow(clippy::ptr_arg)]
    fn draw_points(frame: DynamicImage, points: &Vec<Point>, thickness: u32, color: Pixel) -> Result<DynamicImage, NodeError> {
        if points.is_empty() {
            return Ok(frame);
        }
        let mut out = frame;
        let pts: Vec<_> = points.iter().map(|p| CvPoint::new(p.x.round() as u32, p.y.round() as u32)).collect();
        let color = resolved_color(color, [255, 0, 0, 255]);
        overlay_points(&mut out, &pts, thickness.max(1), color);
        Ok(out)
    }

    #[node(
        id = "drawcontours",
        summary = "Draw contours on a frame.",
        description = "Overlays the provided contours on top of the input frame.",
        inputs(
            "frame",
            port(name = "contours", source = "Contours", ty = crate::daedalus_types::contours()),
            port(name = "thickness", default = 2, meta(ui_min = 1, ui_max = 16, ui_step = 1)),
            port(name = "color", source = "Color", ty = crate::daedalus_types::pixel_rgba())
        ),
        outputs("frame")
    )]
    #[allow(clippy::ptr_arg)]
    fn draw_contours(frame: Compute<DynamicImage>, contours: &Vec<Vec<Point>>, thickness: i64, color: DaedalusValue, exec_ctx: &ExecutionContext) -> Result<DynamicImage, NodeError> {
        let mut out = expect_cpu_frame(frame, "drawcontours", Some(exec_ctx))?;
        if contours.iter().all(|contour| contour.is_empty()) {
            return Ok(out);
        }
        let is_luma = matches!(out, DynamicImage::ImageLuma8(_) | DynamicImage::ImageLuma16(_) | DynamicImage::ImageLumaA8(_) | DynamicImage::ImageLumaA16(_));
        let thickness = thickness.clamp(1, 1024) as u32;
        let pixel = pixel_from_value(&color).unwrap_or_default();
        let fallback = if is_luma { [255, 255, 255, 255] } else { [0, 255, 0, 255] };
        let color = resolved_color(pixel, fallback);
        for contour in contours {
            let pts: Vec<_> = contour.iter().map(|p| CvPoint::new(p.x.round() as u32, p.y.round() as u32)).collect();
            overlay_contour_points(&mut out, &pts, thickness, color);
        }
        Ok(out)
    }

    #[node(
        id = "drawtextscaled",
        inputs(
            "frame",
            "text",
            port(name = "px", meta(ui_min = 0.0, ui_max = 4096.0, ui_step = 1.0)),
            port(name = "py", meta(ui_min = 0.0, ui_max = 4096.0, ui_step = 1.0)),
            port(name = "scale", meta(ui_min = 1.0, ui_max = 64.0, ui_step = 1.0)),
            "color"
        ),
        outputs("frame")
    )]
    fn draw_text_scaled(frame: DynamicImage, text: String, px: f32, py: f32, scale: f32, color: Pixel) -> Result<DynamicImage, NodeError> {
        if text.is_empty() {
            return Ok(frame);
        }
        let mut out = frame;
        let color = resolved_color(color, [255, 255, 255, 255]);
        overlay_text_scaled(&mut out, text.as_str(), (px, py), &FontType::RobotoBlack, scale.max(0.0), color);
        Ok(out)
    }

    #[node(
        id = "overlay",
        inputs(
            "base",
            "top",
            port(name = "x", meta(ui_min = -4096, ui_max = 4096, ui_step = 1)),
            port(name = "y", meta(ui_min = -4096, ui_max = 4096, ui_step = 1))
        ),
        outputs("frame")
    )]
    fn overlay_frames(base: DynamicImage, top: &DynamicImage, x: i32, y: i32) -> Result<DynamicImage, NodeError> {
        let mut out = base;
        overlay_dynamic(&mut out, top, x as i64, y as i64);
        Ok(out)
    }

    declare_plugin!(
        CvDrawPlugin,
        "draw",
        [draw_text, draw_point, draw_line, draw_crosshair, draw_crosshair_at, bbox_anchor_point, draw_rect, draw_circle, draw_ellipse, draw_points, draw_contours, draw_text_scaled, overlay_frames]
    );
}

#[cfg(all(test, feature = "engine"))]
mod tests {
    use super::line::overlay_crosshair_x_y;
    use image::{DynamicImage, GrayImage, Luma, Rgba};

    #[test]
    fn crosshair_overlay_preserves_luma8_frame() {
        let mut out = DynamicImage::ImageLuma8(GrayImage::from_pixel(96, 96, Luma([32])));
        overlay_crosshair_x_y(&mut out, (48, 48), 12, 2, Rgba([0, 255, 0, 255]));

        match out {
            DynamicImage::ImageLuma8(image) => {
                assert_eq!(image.dimensions(), (96, 96));
                assert_ne!(image.get_pixel(48, 48).0[0], 32);
            }
            other => panic!("expected luma8 crosshair output, got {other:?}"),
        }
    }
}
