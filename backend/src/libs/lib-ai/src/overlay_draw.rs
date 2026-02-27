use std::sync::LazyLock;

use ab_glyph::{FontArc, PxScale};
use image::{DynamicImage, GenericImageView, Rgba, RgbaImage};
use imageproc::{
    drawing::{draw_filled_rect_mut, draw_text_mut, text_size},
    rect::Rect,
};
use lib_cv::Pixel;

use crate::{NormalizedBoundingBox, VisionDetection2D};

const LABEL_PADDING: i32 = 4;
const LABEL_MARGIN: i32 = 2;
const LABEL_BG_ALPHA: u8 = 180;
const MIN_LABEL_SCALE: f32 = 10.0;
const MAX_LABEL_SCALE: f32 = 42.0;
const MIN_CROSSHAIR_SIZE_PX: u32 = 2;

pub static LABEL_FONT: LazyLock<FontArc> = LazyLock::new(|| FontArc::try_from_slice(include_bytes!("../assets/fonts/Roboto-Bold.ttf")).expect("Failed to load overlay label font"));

#[derive(Debug, Clone, Copy)]
pub struct OverlayOptions {
    pub color: Pixel,
    pub thickness: u32,
    pub show_label: bool,
    pub show_score: bool,
    pub show_class_id: bool,
    pub show_crosshair: bool,
    pub label_scale: f32,
    pub crosshair_scale: f32,
}

impl Default for OverlayOptions {
    fn default() -> Self {
        Self { color: Pixel { r: 0, g: 255, b: 255, a: 255 }, thickness: 2, show_label: true, show_score: true, show_class_id: true, show_crosshair: false, label_scale: 1.0, crosshair_scale: 0.25 }
    }
}

pub fn draw_detections(image: &mut DynamicImage, detections: &[VisionDetection2D], color: Pixel, thickness: u32) {
    let options = OverlayOptions { color, thickness: thickness.max(1), ..Default::default() };
    draw_detections_with_options(image, detections, &options);
}

pub fn draw_detections_with_options(image: &mut DynamicImage, detections: &[VisionDetection2D], options: &OverlayOptions) {
    if detections.is_empty() {
        return;
    }

    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return;
    }

    let mut buffer = image.to_rgba8();
    let rgba: Rgba<u8> = options.color.into();

    for detection in detections {
        draw_box(&mut buffer, detection, width, height, rgba, options);
    }

    *image = DynamicImage::ImageRgba8(buffer);
}

fn draw_box(image: &mut RgbaImage, detection: &VisionDetection2D, width: u32, height: u32, color: Rgba<u8>, options: &OverlayOptions) {
    let NormalizedBoundingBox { ymin, xmin, ymax, xmax } = detection.bbox;
    let x1 = (xmin * width as f32).clamp(0.0, width as f32 - 1.0) as u32;
    let x2 = (xmax * width as f32).clamp(0.0, width as f32 - 1.0) as u32;
    let y1 = (ymin * height as f32).clamp(0.0, height as f32 - 1.0) as u32;
    let y2 = (ymax * height as f32).clamp(0.0, height as f32 - 1.0) as u32;

    let thickness = options.thickness.max(1);
    for t in 0..thickness {
        let top = y1.saturating_add(t);
        let bottom = y2.saturating_sub(t);
        let left = x1.saturating_add(t);
        let right = x2.saturating_sub(t);

        if top >= height || left >= width {
            continue;
        }

        for x in left..=right.min(width - 1) {
            image.put_pixel(x, top, color);
            image.put_pixel(x, bottom.min(height - 1), color);
        }
        for y in top..=bottom.min(height - 1) {
            image.put_pixel(left, y, color);
            image.put_pixel(right.min(width - 1), y, color);
        }
    }

    draw_label(image, detection, x1, y1, y2, color, options);
    if options.show_crosshair {
        draw_crosshair(image, x1, y1, x2, y2, color, options);
    }
}

fn draw_label(image: &mut RgbaImage, detection: &VisionDetection2D, x1: u32, y1: u32, y2: u32, color: Rgba<u8>, options: &OverlayOptions) {
    let text = match format_detection_label(detection, options) {
        Some(text) => text,
        None => return,
    };

    let box_height = y2.saturating_sub(y1).max(1) as f32;
    let scale_value = (box_height * 0.25 * options.label_scale.max(0.1)).clamp(MIN_LABEL_SCALE, MAX_LABEL_SCALE);
    let font = &*LABEL_FONT;
    let scale = PxScale { x: scale_value, y: scale_value };
    let (text_width, text_height) = text_size(scale, font, &text);
    let padded_width = (text_width as i32 + LABEL_PADDING * 2).max(1);
    let padded_height = (text_height as i32 + LABEL_PADDING * 2).max(1);
    let background = Rgba([0, 0, 0, LABEL_BG_ALPHA]);

    let mut text_color = color;
    text_color.0[3] = 255;

    let max_top = (image.height() as i32 - padded_height).max(0);
    let desired_top = y1 as i32 - padded_height - LABEL_MARGIN;
    let fallback_top = (y1 as i32 + LABEL_MARGIN).min(max_top);
    let top = if desired_top >= 0 { desired_top.min(max_top) } else { fallback_top.max(0) };

    let label_x = x1 as i32 + LABEL_MARGIN;
    let rect = Rect::at(label_x, top).of_size(padded_width as u32, padded_height as u32);
    draw_filled_rect_mut(image, rect, background);
    draw_text_mut(image, text_color, label_x + LABEL_PADDING, top + LABEL_PADDING, scale, font, &text);
}

fn format_detection_label(detection: &VisionDetection2D, options: &OverlayOptions) -> Option<String> {
    let mut parts = Vec::new();
    if options.show_label
        && let Some(label) = detection.label.as_ref().filter(|value| !value.is_empty())
    {
        parts.push(label.clone());
    }
    if options.show_class_id
        && let Some(id) = detection.class_id
    {
        parts.push(format!("#{id}"));
    }
    if options.show_score && detection.score.is_finite() {
        parts.push(format!("{:.2}", detection.score));
    }

    if parts.is_empty() { None } else { Some(parts.join(" ")) }
}

fn draw_crosshair(image: &mut RgbaImage, x1: u32, y1: u32, x2: u32, y2: u32, color: Rgba<u8>, options: &OverlayOptions) {
    let width = image.width();
    let height = image.height();
    if width == 0 || height == 0 {
        return;
    }

    let box_w = x2.saturating_sub(x1).max(1);
    let box_h = y2.saturating_sub(y1).max(1);
    let scale = options.crosshair_scale.clamp(0.0, 1.0);
    let size = ((box_w.min(box_h) as f32) * scale).round() as u32;
    let size = size.max(MIN_CROSSHAIR_SIZE_PX);
    let half = size / 2;
    let cx = x1.saturating_add(box_w / 2);
    let cy = y1.saturating_add(box_h / 2);

    let thickness = options.thickness.max(1);
    for t in 0..thickness {
        let y = cy.saturating_add(t);
        if y < height {
            let start_x = cx.saturating_sub(half);
            let end_x = (cx + half).min(width - 1);
            for x in start_x..=end_x {
                image.put_pixel(x, y, color);
            }
        }
        let x = cx.saturating_add(t);
        if x < width {
            let start_y = cy.saturating_sub(half);
            let end_y = (cy + half).min(height - 1);
            for yy in start_y..=end_y {
                image.put_pixel(x, yy, color);
            }
        }
    }
}
