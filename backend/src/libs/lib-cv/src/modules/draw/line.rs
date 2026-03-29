use image::{DynamicImage, ImageBuffer, Luma, Rgb, Rgba};
use imageproc::{
    drawing::{Canvas, draw_line_segment_mut, draw_polygon_mut},
    point::Point,
};
use num::NumCast;

use super::{point::draw_point_x_y, utils::overlay_dynamic};

pub fn draw_line<C: Canvas, T: NumCast + Copy, T2: NumCast + Copy, T3: NumCast + Copy>(image: &mut C, start: &Point<T>, end: &Point<T2>, thickness: T3, colour: C::Pixel) {
    draw_line_x_y(image, (start.x, start.y), (end.x, end.y), thickness, colour);
}

pub fn overlay_line(image: &mut DynamicImage, start: &Point<u32>, end: &Point<u32>, thickness: u32, colour: Rgba<u8>) {
    overlay_line_x_y(image, (start.x, start.y), (end.x, end.y), thickness, colour);
}

pub fn draw_line_x_y<C: Canvas, T: NumCast, T2: NumCast, T3: NumCast + Clone>(image: &mut C, start: (T, T), end: (T2, T2), thickness: T3, colour: C::Pixel) {
    let thickness = thickness.to_f32().unwrap();
    let (x1, y1) = (start.0.to_f32().unwrap(), start.1.to_f32().unwrap());
    let (x2, y2) = (end.0.to_f32().unwrap(), end.1.to_f32().unwrap());

    if thickness <= 1.0 {
        draw_line_segment_mut(image, (x1, y1), (x2, y2), colour);
        return;
    }

    // Calculate the direction vector of the line
    let dx = x2 - x1;
    let dy = y2 - y1;
    let length = (dx.powi(2) + dy.powi(2)).sqrt();

    if length == 0.0 {
        // If the two points are the same, draw a filled circle
        draw_point_x_y(image, (x1, y1), thickness, colour);
        return;
    }

    // Calculate the perpendicular vector (normalized)
    let perp_x = -dy / length;
    let perp_y = dx / length;

    // Scale the perpendicular vector by half the thickness
    let offset_x = perp_x * thickness / 2.0;
    let offset_y = perp_y * thickness / 2.0;

    // Define the four corners of the thick line rectangle
    let top_left = (x1 + offset_x, y1 + offset_y);
    let top_right = (x2 + offset_x, y2 + offset_y);
    let bottom_right = (x2 - offset_x, y2 - offset_y);
    let bottom_left = (x1 - offset_x, y1 - offset_y);

    // Create a convex polygon (rectangle) representing the thick line
    let polygon_points = [
        Point::new(top_left.0.round() as i32, top_left.1.round() as i32),
        Point::new(top_right.0.round() as i32, top_right.1.round() as i32),
        Point::new(bottom_right.0.round() as i32, bottom_right.1.round() as i32),
        Point::new(bottom_left.0.round() as i32, bottom_left.1.round() as i32),
    ];

    draw_polygon_mut(image, &polygon_points, colour);
}

pub fn overlay_line_x_y(image: &mut DynamicImage, start: (u32, u32), end: (u32, u32), thickness: u32, colour: Rgba<u8>) {
    if colour.0[3] == 255 {
        match image {
            DynamicImage::ImageRgba8(buf) => {
                draw_line_x_y(buf, start, end, thickness.max(1), colour);
                return;
            }
            DynamicImage::ImageRgb8(buf) => {
                draw_line_x_y(buf, start, end, thickness.max(1), Rgb([colour.0[0], colour.0[1], colour.0[2]]));
                return;
            }
            DynamicImage::ImageLuma8(buf) => {
                let lum = (0.299 * colour.0[0] as f32 + 0.587 * colour.0[1] as f32 + 0.114 * colour.0[2] as f32).round().clamp(0.0, 255.0) as u8;
                draw_line_x_y(buf, start, end, thickness.max(1), Luma([lum]));
                return;
            }
            _ => {}
        }
    }

    let min_x = start.0.min(end.0).saturating_sub(thickness / 2);
    let min_y = start.1.min(end.1).saturating_sub(thickness / 2);
    let max_x = start.0.max(end.0).saturating_add(thickness / 2);
    let max_y = start.1.max(end.1).saturating_add(thickness / 2);
    let width = (max_x - min_x + 1).max(1);
    let height = (max_y - min_y + 1).max(1);
    let mut overlay_img = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(width, height, Rgba([0, 0, 0, 0])));
    draw_line_x_y(&mut overlay_img, (start.0 - min_x, start.1 - min_y), (end.0 - min_x, end.1 - min_y), thickness, colour);

    overlay_dynamic(image, &overlay_img, min_x as i64, min_y as i64);
}

fn fill_rect_luma8(image: &mut image::GrayImage, x0: u32, y0: u32, x1: u32, y1: u32, value: u8) {
    let stride = image.width() as usize;
    let buf = image.as_flat_samples_mut().samples;
    let x0 = x0 as usize;
    let x1 = x1 as usize;
    for y in y0..=y1 {
        let row = y as usize * stride;
        buf[row + x0..row + x1 + 1].fill(value);
    }
}

fn fill_rect_rgb8(image: &mut image::RgbImage, x0: u32, y0: u32, x1: u32, y1: u32, value: [u8; 3]) {
    let stride = image.width() as usize * 3;
    let buf = image.as_flat_samples_mut().samples;
    let x0 = x0 as usize;
    let x1 = x1 as usize;
    for y in y0..=y1 {
        let row = y as usize * stride;
        let span = &mut buf[row + x0 * 3..row + (x1 + 1) * 3];
        for px in span.chunks_exact_mut(3) {
            px.copy_from_slice(&value);
        }
    }
}

fn fill_rect_rgba8(image: &mut image::RgbaImage, x0: u32, y0: u32, x1: u32, y1: u32, value: [u8; 4]) {
    let stride = image.width() as usize * 4;
    let buf = image.as_flat_samples_mut().samples;
    let x0 = x0 as usize;
    let x1 = x1 as usize;
    for y in y0..=y1 {
        let row = y as usize * stride;
        let span = &mut buf[row + x0 * 4..row + (x1 + 1) * 4];
        for px in span.chunks_exact_mut(4) {
            px.copy_from_slice(&value);
        }
    }
}

pub fn overlay_crosshair_x_y(image: &mut DynamicImage, center: (u32, u32), arm: u32, thickness: u32, colour: Rgba<u8>) {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return;
    }

    let thickness = thickness.max(1);
    let cx = center.0.min(width - 1);
    let cy = center.1.min(height - 1);
    let left = cx.saturating_sub(arm);
    let right = cx.saturating_add(arm).min(width - 1);
    let top = cy.saturating_sub(arm);
    let bottom = cy.saturating_add(arm).min(height - 1);
    let band_lo = thickness.saturating_sub(1) / 2;
    let band_hi = thickness / 2;
    let hx0 = left;
    let hx1 = right;
    let hy0 = cy.saturating_sub(band_lo);
    let hy1 = cy.saturating_add(band_hi).min(height - 1);
    let vx0 = cx.saturating_sub(band_lo);
    let vx1 = cx.saturating_add(band_hi).min(width - 1);
    let vy0 = top;
    let vy1 = bottom;

    if colour.0[3] == 255 {
        match image {
            DynamicImage::ImageLuma8(buf) => {
                let lum = (0.299 * colour.0[0] as f32 + 0.587 * colour.0[1] as f32 + 0.114 * colour.0[2] as f32).round().clamp(0.0, 255.0) as u8;
                fill_rect_luma8(buf, hx0, hy0, hx1, hy1, lum);
                fill_rect_luma8(buf, vx0, vy0, vx1, vy1, lum);
                return;
            }
            DynamicImage::ImageRgb8(buf) => {
                let rgb = [colour.0[0], colour.0[1], colour.0[2]];
                fill_rect_rgb8(buf, hx0, hy0, hx1, hy1, rgb);
                fill_rect_rgb8(buf, vx0, vy0, vx1, vy1, rgb);
                return;
            }
            DynamicImage::ImageRgba8(buf) => {
                fill_rect_rgba8(buf, hx0, hy0, hx1, hy1, colour.0);
                fill_rect_rgba8(buf, vx0, vy0, vx1, vy1, colour.0);
                return;
            }
            _ => {}
        }
    }

    overlay_line_x_y(image, (left, cy), (right, cy), thickness, colour);
    overlay_line_x_y(image, (cx, top), (cx, bottom), thickness, colour);
}
