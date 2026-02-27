use super::utils::overlay_dynamic;
use image::{DynamicImage, ImageBuffer, Rgba};
use imageproc::{
    drawing::{Canvas, draw_filled_circle_mut},
    point::Point,
};
use num::{NumCast, ToPrimitive};

pub fn draw_points<C: Canvas, T: NumCast + Copy, T2: NumCast + Copy>(image: &mut C, points: &[Point<T>], thickness: T2, colour: C::Pixel) {
    points.iter().for_each(|p| draw_point(image, p, thickness, colour));
}

pub fn overlay_points(image: &mut DynamicImage, points: &[Point<u32>], thickness: u32, colour: Rgba<u8>) {
    points.iter().for_each(|p| overlay_point_x_y(image, (p.x, p.y), thickness, colour));
}

pub fn draw_point<C: Canvas, T: NumCast + Copy, T2: NumCast + Copy>(image: &mut C, point: &Point<T>, thickness: T2, colour: C::Pixel) {
    draw_point_x_y(image, (point.x, point.y), thickness, colour);
}

pub fn overlay_point(image: &mut DynamicImage, point: &Point<u32>, thickness: u32, colour: Rgba<u8>) {
    overlay_point_x_y(image, (point.x, point.y), thickness, colour);
}

pub fn draw_point_x_y<C: Canvas, T: NumCast + Copy, T2: NumCast + Copy>(image: &mut C, point: (T, T), thickness: T2, colour: C::Pixel) {
    let thickness = thickness.to_f32().unwrap();
    let (x1, y1) = (point.0.to_u32().unwrap(), point.1.to_u32().unwrap());

    if thickness <= 1.0 {
        image.draw_pixel(x1, y1, colour);
        return;
    }

    draw_filled_circle_mut(image, (x1.to_i32().unwrap(), y1.to_i32().unwrap()), (thickness / 2.0) as i32, colour);
}

pub fn overlay_point_x_y(image: &mut DynamicImage, point: (u32, u32), thickness: u32, colour: Rgba<u8>) {
    let size = thickness.max(1) + 2;
    let offset = size / 2;
    let mut overlay_img = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(size, size, Rgba([0, 0, 0, 0])));
    draw_point_x_y(&mut overlay_img, (offset, offset), thickness, colour);

    overlay_dynamic(image, &overlay_img, point.0.saturating_sub(offset) as i64, point.1.saturating_sub(offset) as i64);
}
