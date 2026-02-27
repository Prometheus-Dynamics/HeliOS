use ab_glyph::PxScale;
use image::{DynamicImage, ImageBuffer, Pixel, Rgba};
use imageproc::{
    definitions::Clamp,
    drawing::{Canvas, draw_text_mut},
    point::Point,
};
use num::NumCast;

use super::font::FontType;
use super::utils::overlay_dynamic;

pub fn draw_text<C, T, T2, T3>(image: &mut C, text: &str, point: &Point<T>, font: &FontType, scale: T3, colour: C::Pixel)
where
    C: Canvas,
    T: NumCast + Copy,
    T2: NumCast + Copy,
    T3: NumCast + Copy,
    <C::Pixel as Pixel>::Subpixel: Into<f32> + Clamp<f32>,
{
    draw_text_x_y(image, text, (point.x, point.y), font, scale, colour)
}

pub fn draw_text_x_y<C, T, T2>(image: &mut C, text: &str, point: (T, T), font: &FontType, scale: T2, colour: C::Pixel)
where
    C: Canvas,
    T: NumCast + Copy,
    T2: NumCast + Copy,
    <C::Pixel as Pixel>::Subpixel: Into<f32> + Clamp<f32>,
{
    let scale = scale.to_f32().unwrap();

    let px_scale = PxScale { x: scale, y: scale };

    draw_text_mut(image, colour, point.0.to_i32().unwrap(), point.1.to_i32().unwrap(), px_scale, font.get_font(), text);
}

pub fn overlay_text_x_y(image: &mut DynamicImage, text: &str, point: (u32, u32), font: &FontType, scale: u32, colour: Rgba<u8>) {
    let width = (text.len() as u32 * scale).max(scale);
    let height = scale + 5;
    let mut overlay_img = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(width, height, Rgba([0, 0, 0, 0])));
    draw_text_x_y(&mut overlay_img, text, (0, 0), font, scale, colour);

    overlay_dynamic(image, &overlay_img, point.0 as i64, point.1 as i64);
}

pub fn draw_text_scaled<C>(image: &mut C, text: &str, percent: (f32, f32), font: &FontType, scale: f32, colour: C::Pixel)
where
    C: Canvas,
    <C::Pixel as Pixel>::Subpixel: Into<f32> + Clamp<f32>,
{
    let (width, height) = image.dimensions();
    let x = (percent.0.clamp(0.0, 100.0) / 100.0) * width as f32;
    let y = (percent.1.clamp(0.0, 100.0) / 100.0) * height as f32;
    let base_scale = height as f32 / 100.0;
    draw_text_x_y(image, text, (x, y), font, base_scale * scale, colour)
}

pub fn draw_text_scaled_point<C>(image: &mut C, text: &str, point: &Point<f32>, font: &FontType, scale: f32, colour: C::Pixel)
where
    C: Canvas,
    <C::Pixel as Pixel>::Subpixel: Into<f32> + Clamp<f32>,
{
    draw_text_scaled(image, text, (point.x, point.y), font, scale, colour);
}

pub fn overlay_text_scaled(image: &mut DynamicImage, text: &str, percent: (f32, f32), font: &FontType, scale: f32, colour: Rgba<u8>) {
    let (width, height) = image.dimensions();
    let x = (percent.0.clamp(0.0, 100.0) / 100.0) * width as f32;
    let y = (percent.1.clamp(0.0, 100.0) / 100.0) * height as f32;
    let base_scale = height as f32 / 100.0;
    let px_scale = (base_scale * scale).round().max(1.0) as u32;
    overlay_text_x_y(image, text, (x.round() as u32, y.round() as u32), font, px_scale, colour);
}

pub fn overlay_text_scaled_point(image: &mut DynamicImage, text: &str, point: &Point<f32>, font: &FontType, scale: f32, colour: Rgba<u8>) {
    overlay_text_scaled(image, text, (point.x, point.y), font, scale, colour);
}
