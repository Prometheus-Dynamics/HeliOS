use super::utils::overlay_dynamic;
use image::{DynamicImage, ImageBuffer, Luma, Rgb, Rgba};
use imageproc::{
    drawing::{Canvas, draw_filled_circle_mut, draw_filled_ellipse_mut, draw_filled_rect_mut, draw_hollow_circle_mut, draw_hollow_ellipse_mut, draw_hollow_rect_mut},
    point::Point,
    rect::Rect,
};
use num::ToPrimitive;

pub fn draw_rect<C, T, T2>(image: &mut C, top_left: Point<T>, bottom_right: Point<T2>, filled: bool, colour: C::Pixel)
where
    C: Canvas,
    T: ToPrimitive + Copy,
    T2: ToPrimitive + Copy,
{
    let x1 = top_left.x.to_i32().unwrap();
    let y1 = top_left.y.to_i32().unwrap();
    let x2 = bottom_right.x.to_i32().unwrap();
    let y2 = bottom_right.y.to_i32().unwrap();
    let rect = Rect::at(x1.min(x2), y1.min(y2)).of_size((x1.max(x2) - x1.min(x2)) as u32, (y1.max(y2) - y1.min(y2)) as u32);
    if filled {
        draw_filled_rect_mut(image, rect, colour);
    } else {
        draw_hollow_rect_mut(image, rect, colour);
    }
}

pub fn overlay_rect(image: &mut DynamicImage, top_left: (u32, u32), bottom_right: (u32, u32), filled: bool, colour: Rgba<u8>) {
    let (min_x, max_x) = if top_left.0 <= bottom_right.0 { (top_left.0, bottom_right.0) } else { (bottom_right.0, top_left.0) };
    let (min_y, max_y) = if top_left.1 <= bottom_right.1 { (top_left.1, bottom_right.1) } else { (bottom_right.1, top_left.1) };
    let width = (max_x - min_x).max(1);
    let height = (max_y - min_y).max(1);
    let mut overlay_img = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(width, height, Rgba([0, 0, 0, 0])));
    let tl = Point::new(0u32, 0u32);
    let br = Point::new(width - 1, height - 1);
    draw_rect(&mut overlay_img, tl, br, filled, colour);

    overlay_dynamic(image, &overlay_img, min_x as i64, min_y as i64);
}

pub fn draw_circle<C, T>(image: &mut C, center: Point<T>, radius: u32, filled: bool, colour: C::Pixel)
where
    C: Canvas,
    T: ToPrimitive + Copy,
{
    let (x, y) = (center.x.to_i32().unwrap(), center.y.to_i32().unwrap());
    if filled {
        draw_filled_circle_mut(image, (x, y), radius as i32, colour);
    } else {
        draw_hollow_circle_mut(image, (x, y), radius as i32, colour);
    }
}

pub fn overlay_circle(image: &mut DynamicImage, center: (u32, u32), radius: u32, filled: bool, colour: Rgba<u8>) {
    let cx = center.0.min(i32::MAX as u32) as i32;
    let cy = center.1.min(i32::MAX as u32) as i32;
    match image {
        DynamicImage::ImageRgba8(buf) => {
            if filled {
                draw_filled_circle_mut(buf, (cx, cy), radius as i32, colour);
            } else {
                draw_hollow_circle_mut(buf, (cx, cy), radius as i32, colour);
            }
            return;
        }
        DynamicImage::ImageRgb8(buf) => {
            let rgb = Rgb([colour.0[0], colour.0[1], colour.0[2]]);
            if filled {
                draw_filled_circle_mut(buf, (cx, cy), radius as i32, rgb);
            } else {
                draw_hollow_circle_mut(buf, (cx, cy), radius as i32, rgb);
            }
            return;
        }
        DynamicImage::ImageLuma8(buf) => {
            let lum = (0.299 * colour.0[0] as f32 + 0.587 * colour.0[1] as f32 + 0.114 * colour.0[2] as f32).round().clamp(0.0, 255.0) as u8;
            let luma = Luma([lum]);
            if filled {
                draw_filled_circle_mut(buf, (cx, cy), radius as i32, luma);
            } else {
                draw_hollow_circle_mut(buf, (cx, cy), radius as i32, luma);
            }
            return;
        }
        _ => {}
    }

    let size = radius * 2 + 3;
    let mut overlay_img = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(size, size, Rgba([0, 0, 0, 0])));
    let offset = (size / 2) as i32;
    let c = Point::new(offset, offset);
    draw_circle(&mut overlay_img, c, radius, filled, colour);

    overlay_dynamic(image, &overlay_img, center.0.saturating_sub(radius + 1) as i64, center.1.saturating_sub(radius + 1) as i64);
}

pub fn draw_ellipse<C, T>(image: &mut C, center: Point<T>, radius_x: u32, radius_y: u32, filled: bool, colour: C::Pixel)
where
    C: Canvas,
    T: ToPrimitive + Copy,
{
    let (x, y) = (center.x.to_i32().unwrap(), center.y.to_i32().unwrap());
    if filled {
        draw_filled_ellipse_mut(image, (x, y), radius_x as i32, radius_y as i32, colour);
    } else {
        draw_hollow_ellipse_mut(image, (x, y), radius_x as i32, radius_y as i32, colour);
    }
}

pub fn overlay_ellipse(image: &mut DynamicImage, center: (u32, u32), radius_x: u32, radius_y: u32, filled: bool, colour: Rgba<u8>) {
    let size_x = radius_x * 2 + 3;
    let size_y = radius_y * 2 + 3;
    let mut overlay_img = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(size_x, size_y, Rgba([0, 0, 0, 0])));
    let c = Point::new((size_x / 2) as i32, (size_y / 2) as i32);
    draw_ellipse(&mut overlay_img, c, radius_x, radius_y, filled, colour);

    overlay_dynamic(image, &overlay_img, center.0.saturating_sub(radius_x + 1) as i64, center.1.saturating_sub(radius_y + 1) as i64);
}
