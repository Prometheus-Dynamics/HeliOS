//! Rendering markers into images, for tests, calibration targets and tools.

use super::Dictionary;
use crate::geometry::{Homography, Point};
use crate::image::GrayImage;

/// The marker's cells including its black border, row-major, `true` = white.
/// Returns `None` for an id outside the dictionary.
pub fn marker_cells(dict: &Dictionary, id: usize) -> Option<Vec<bool>> {
    let code = *dict.codes.get(id)?;
    let n = dict.total_width() as usize;
    let nbits = dict.bits.len();
    let mut cells = vec![false; n * n];
    for (i, &(x, y)) in dict.bits.iter().enumerate() {
        let bit = (code >> (nbits - 1 - i)) & 1 == 1;
        cells[(y as usize + 1) * n + x as usize + 1] = bit;
    }
    Some(cells)
}

/// Render a marker with `cell_px` pixels per cell and a white quiet zone of
/// `quiet_cells` cells on every side.
pub fn render_marker(dict: &Dictionary, id: usize, cell_px: usize, quiet_cells: usize) -> Option<GrayImage> {
    let cells = marker_cells(dict, id)?;
    let n = dict.total_width() as usize;
    let side = (n + 2 * quiet_cells) * cell_px;
    let mut image = GrayImage::filled(side, side, 255);
    for cy in 0..n {
        for cx in 0..n {
            if cells[cy * n + cx] {
                continue;
            }
            for y in 0..cell_px {
                for x in 0..cell_px {
                    image.set((cx + quiet_cells) * cell_px + x, (cy + quiet_cells) * cell_px + y, 0);
                }
            }
        }
    }
    Some(image)
}

/// Draw `src` into `canvas` so that its corners (top-left first, clockwise)
/// land on `dst`, using bilinear sampling. Pixels outside the warped source
/// are left untouched.
pub fn paste_warped(canvas: &mut GrayImage, src: &GrayImage, dst: [Point; 4]) -> bool {
    let (w, h) = (src.width() as f32, src.height() as f32);
    let src_corners = [[-0.5, -0.5], [w - 0.5, -0.5], [w - 0.5, h - 0.5], [-0.5, h - 0.5]];
    let Some(back) = Homography::from_points(dst, src_corners) else { return false };
    let min_x = dst.iter().map(|p| p[0]).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
    let min_y = dst.iter().map(|p| p[1]).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
    let max_x = (dst.iter().map(|p| p[0]).fold(f32::MIN, f32::max).ceil() as usize).min(canvas.width().saturating_sub(1));
    let max_y = (dst.iter().map(|p| p[1]).fold(f32::MIN, f32::max).ceil() as usize).min(canvas.height().saturating_sub(1));
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let p = back.map([x as f32, y as f32]);
            if p[0] >= -0.5 && p[1] >= -0.5 && p[0] <= w - 0.5 && p[1] <= h - 0.5 {
                canvas.set(x, y, src.sample(p[0], p[1]).round() as u8);
            }
        }
    }
    true
}
