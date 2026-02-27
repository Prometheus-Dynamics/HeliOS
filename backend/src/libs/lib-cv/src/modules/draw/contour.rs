use image::{DynamicImage, ImageBuffer, Luma, Rgb, Rgba};
use imageproc::{contours::Contour, drawing::Canvas, point::Point};
use num::{NumCast, ToPrimitive};

use super::{
    line::{draw_line, draw_line_x_y},
    point::{draw_point, overlay_point_x_y},
    utils::overlay_dynamic,
};

pub fn draw_contour<C: Canvas, T: NumCast + Copy, T2: NumCast + Copy>(image: &mut C, contour: &Contour<T>, thickness: T2, colour: C::Pixel) {
    draw_contour_points(image, &contour.points, thickness, colour);
}

pub fn overlay_contour(image: &mut DynamicImage, contour: &Contour<u32>, thickness: u32, colour: Rgba<u8>) {
    overlay_contour_points(image, &contour.points, thickness, colour);
}

pub fn draw_contour_points<C: Canvas, T: NumCast + Copy, T2: NumCast + Copy>(image: &mut C, contour: &[Point<T>], thickness: T2, colour: C::Pixel) {
    if contour.len() >= 2 {
        // Draw lines between consecutive points
        for i in 0..contour.len() - 1 {
            let p1 = &contour[i];
            let p2 = &contour[i + 1];

            draw_line(image, p1, p2, thickness, colour);
        }
        let start = contour.first().unwrap();
        let end = contour.last().unwrap();
        draw_line(image, start, end, thickness, colour);
    } else if contour.len() == 1 {
        // Draw a single point
        draw_point(image, &contour[0], thickness, colour);
    }
}

pub fn overlay_contour_points<T: ToPrimitive + Copy>(image: &mut DynamicImage, contour: &[Point<T>], thickness: u32, colour: Rgba<u8>) {
    if contour.is_empty() {
        return;
    }
    let img_w = image.width();
    let img_h = image.height();
    if img_w == 0 || img_h == 0 {
        return;
    }

    if contour.len() == 1 {
        let x = contour[0].x.to_f64().unwrap_or(0.0);
        let y = contour[0].y.to_f64().unwrap_or(0.0);
        let x = x.round().clamp(0.0, (img_w - 1) as f64) as u32;
        let y = y.round().clamp(0.0, (img_h - 1) as f64) as u32;
        overlay_point_x_y(image, (x, y), thickness, colour);
        return;
    }

    let solid_alpha = colour.0[3] == 255;
    if solid_alpha && contour.len() <= 8 {
        let mut points = [(0u32, 0u32); 8];
        let mut count = 0usize;
        for p in contour {
            let Some(x) = p.x.to_f64() else { continue };
            let Some(y) = p.y.to_f64() else { continue };
            if !x.is_finite() || !y.is_finite() {
                continue;
            }
            let cur = (x.round().clamp(0.0, (img_w - 1) as f64) as u32, y.round().clamp(0.0, (img_h - 1) as f64) as u32);
            if count > 0 && points[count - 1] == cur {
                continue;
            }
            points[count] = cur;
            count += 1;
        }
        if count == 1 {
            overlay_point_x_y(image, points[0], thickness, colour);
            return;
        }
        if count >= 2 {
            let points = &points[..count];
            match image {
                DynamicImage::ImageRgba8(buf) => {
                    for i in 0..points.len() - 1 {
                        let a = points[i];
                        let b = points[i + 1];
                        draw_line_x_y(buf, (a.0, a.1), (b.0, b.1), thickness.max(1), colour);
                    }
                    if points.len() >= 3 {
                        let last = points[points.len() - 1];
                        let first = points[0];
                        draw_line_x_y(buf, (last.0, last.1), (first.0, first.1), thickness.max(1), colour);
                    }
                    return;
                }
                DynamicImage::ImageRgb8(buf) => {
                    let rgb = Rgb([colour.0[0], colour.0[1], colour.0[2]]);
                    for i in 0..points.len() - 1 {
                        let a = points[i];
                        let b = points[i + 1];
                        draw_line_x_y(buf, (a.0, a.1), (b.0, b.1), thickness.max(1), rgb);
                    }
                    if points.len() >= 3 {
                        let last = points[points.len() - 1];
                        let first = points[0];
                        draw_line_x_y(buf, (last.0, last.1), (first.0, first.1), thickness.max(1), rgb);
                    }
                    return;
                }
                DynamicImage::ImageLuma8(buf) => {
                    let lum = (0.299 * colour.0[0] as f32 + 0.587 * colour.0[1] as f32 + 0.114 * colour.0[2] as f32).round().clamp(0.0, 255.0) as u8;
                    let luma = Luma([lum]);
                    for i in 0..points.len() - 1 {
                        let a = points[i];
                        let b = points[i + 1];
                        draw_line_x_y(buf, (a.0, a.1), (b.0, b.1), thickness.max(1), luma);
                    }
                    if points.len() >= 3 {
                        let last = points[points.len() - 1];
                        let first = points[0];
                        draw_line_x_y(buf, (last.0, last.1), (first.0, first.1), thickness.max(1), luma);
                    }
                    return;
                }
                _ => {}
            }
        }
    }

    // Collect points as u32 for bounding box calculations; drop consecutive duplicates so
    // we don't render a polyline as a "cloud of points" when the trace contains repeats.
    let mut points: Vec<(u32, u32)> = Vec::with_capacity(contour.len());
    for p in contour {
        let Some(x) = p.x.to_f64() else { continue };
        let Some(y) = p.y.to_f64() else { continue };
        if !x.is_finite() || !y.is_finite() {
            continue;
        }
        let cur = (x.round().clamp(0.0, (img_w - 1) as f64) as u32, y.round().clamp(0.0, (img_h - 1) as f64) as u32);
        if points.last().copied() == Some(cur) {
            continue;
        }
        points.push(cur);
    }
    if points.len() == 1 {
        overlay_point_x_y(image, points[0], thickness, colour);
        return;
    }
    if points.len() < 2 {
        return;
    }

    let min_x = points.iter().map(|p| p.0).min().unwrap().saturating_sub(thickness / 2);
    let min_y = points.iter().map(|p| p.1).min().unwrap().saturating_sub(thickness / 2);
    let max_x = points.iter().map(|p| p.0).max().unwrap().saturating_add(thickness / 2);
    let max_y = points.iter().map(|p| p.1).max().unwrap().saturating_add(thickness / 2);

    let width = (max_x - min_x + 1).max(1);
    let height = (max_y - min_y + 1).max(1);

    let mut overlay_img = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(width, height, Rgba([0, 0, 0, 0])));

    // Draw the contour as a closed polyline. This is more robust than treating arbitrary contour
    // traces as a polygon (which can self-intersect), and fixes the "points only" appearance.
    for w in points.windows(2) {
        let (x1, y1) = w[0];
        let (x2, y2) = w[1];
        draw_line_x_y(&mut overlay_img, ((x1 as f32) - (min_x as f32), (y1 as f32) - (min_y as f32)), ((x2 as f32) - (min_x as f32), (y2 as f32) - (min_y as f32)), thickness.max(1), colour);
    }
    // Close the contour.
    if points.len() >= 3 {
        let (x1, y1) = points[points.len() - 1];
        let (x2, y2) = points[0];
        draw_line_x_y(&mut overlay_img, ((x1 as f32) - (min_x as f32), (y1 as f32) - (min_y as f32)), ((x2 as f32) - (min_x as f32), (y2 as f32) - (min_y as f32)), thickness.max(1), colour);
    }

    overlay_dynamic(image, &overlay_img, min_x as i64, min_y as i64);
}
