use image::{DynamicImage, Pixel, Rgba};
use imageproc::{definitions::Clamp, drawing::Canvas};

use crate::modules::aruco::ArucoDetection2D;

use super::{
    contour::{draw_contour_points, overlay_contour_points},
    text::{draw_text_x_y, overlay_text_x_y},
};

pub fn draw_marker<C: Canvas>(image: &mut C, marker: &ArucoDetection2D, colour: C::Pixel)
where
    <C::Pixel as Pixel>::Subpixel: Into<f32> + Clamp<f32>,
{
    let contour = [
        imageproc::point::Point::new(marker.corners[0].x as f32, marker.corners[0].y as f32),
        imageproc::point::Point::new(marker.corners[1].x as f32, marker.corners[1].y as f32),
        imageproc::point::Point::new(marker.corners[2].x as f32, marker.corners[2].y as f32),
        imageproc::point::Point::new(marker.corners[3].x as f32, marker.corners[3].y as f32),
    ];

    draw_contour_points(image, &contour, 2, colour);

    let min_x = contour.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
    let max_x = contour.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max);
    let min_y = contour.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
    let max_y = contour.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);

    let height = max_y - min_y;
    let font_size = ((height * 0.2).round() as u32).max(10);

    let id_str = format!("{}", marker.id);
    let estimated_char_width = (font_size as f32).round() as u32;
    let text_width = id_str.len() as u32 * estimated_char_width;
    let center_x = ((min_x + max_x) / 2.0).round() as u32;
    let text_x = center_x.saturating_sub(text_width / 2);
    let padding = (font_size as f32 * 0.2).round() as u32;
    let mut text_y = max_y.round() as u32 + padding;
    if text_y + font_size > image.height() {
        text_y = image.height().saturating_sub(font_size);
    }
    draw_text_x_y(image, id_str.as_str(), (text_x, text_y), &super::font::FontType::SavedByZero, font_size, colour);
}

pub fn overlay_marker(image: &mut DynamicImage, marker: &ArucoDetection2D, colour: Rgba<u8>) {
    let contour = [
        imageproc::point::Point::new(marker.corners[0].x as f32, marker.corners[0].y as f32),
        imageproc::point::Point::new(marker.corners[1].x as f32, marker.corners[1].y as f32),
        imageproc::point::Point::new(marker.corners[2].x as f32, marker.corners[2].y as f32),
        imageproc::point::Point::new(marker.corners[3].x as f32, marker.corners[3].y as f32),
    ];

    overlay_contour_points(image, &contour, 2, colour);

    let min_x = contour.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
    let max_x = contour.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max);
    let min_y = contour.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
    let max_y = contour.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);

    let height = max_y - min_y;
    let font_size = ((height * 0.2).round() as u32).max(10);
    let id_str = format!("{}", marker.id);
    let estimated_char_width = (font_size as f32).round() as u32;
    let text_width = id_str.len() as u32 * estimated_char_width;
    let center_x = ((min_x + max_x) / 2.0).round() as u32;
    let text_x = center_x.saturating_sub(text_width / 2);
    let padding = (font_size as f32 * 0.2).round() as u32;
    let mut text_y = max_y.round() as u32 + padding;
    if text_y + font_size > image.height() {
        text_y = image.height().saturating_sub(font_size);
    }
    overlay_text_x_y(image, id_str.as_str(), (text_x, text_y), &super::font::FontType::SavedByZero, font_size, colour);
}

pub fn overlay_marker_outline(image: &mut DynamicImage, marker: &ArucoDetection2D, colour: Rgba<u8>) {
    let contour = [
        imageproc::point::Point::new(marker.corners[0].x as f32, marker.corners[0].y as f32),
        imageproc::point::Point::new(marker.corners[1].x as f32, marker.corners[1].y as f32),
        imageproc::point::Point::new(marker.corners[2].x as f32, marker.corners[2].y as f32),
        imageproc::point::Point::new(marker.corners[3].x as f32, marker.corners[3].y as f32),
    ];
    overlay_contour_points(image, &contour, 2, colour);
}
