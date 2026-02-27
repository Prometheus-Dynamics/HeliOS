use image::{GrayImage, Luma};

use super::super::ArucoBitGrid;

pub trait ArucoTagDecoding: Sync + Send {
    fn decode(&self, code: Vec<Vec<u8>>) -> Option<ArucoTagDecode>;
    fn max_hamming_distance(&self) -> u8;
    fn data_bits_location(&self) -> &[(u8, u8)];
    fn border_size(&self) -> u8;
    fn data_width(&self) -> u8;
    fn total_width(&self) -> u8;
    fn codes(&self) -> &[u64];

    fn border_error_divisor(&self) -> u8 {
        10
    }

    fn bit_grid(&self, id: usize) -> Option<ArucoBitGrid> {
        if id >= self.codes().len() {
            return None;
        }

        let total_width = self.total_width();
        let border = self.border_size();
        let data_width = self.data_width();
        if total_width == 0 || data_width == 0 || total_width < data_width {
            return None;
        }

        let code = self.codes()[id];
        let bits_loc = self.data_bits_location();
        let nbits = bits_loc.len();
        if nbits == 0 || nbits > 64 {
            return None;
        }

        let mut grid = vec![vec![0u8; data_width as usize]; data_width as usize];
        for (i, &(x, y)) in bits_loc.iter().enumerate() {
            let bit = ((code >> (nbits - 1 - i)) & 1) as u8;
            let row = grid.get_mut(y as usize)?;
            let cell = row.get_mut(x as usize)?;
            *cell = bit;
        }

        let mut rows: Vec<String> = Vec::with_capacity(total_width as usize);
        for y in 0..total_width {
            let mut row = String::with_capacity(total_width as usize);
            for x in 0..total_width {
                let on_border = x < border || y < border || x + border >= total_width || y + border >= total_width;
                let bit = if on_border {
                    0u8
                } else {
                    let gx = (x - border) as usize;
                    let gy = (y - border) as usize;
                    grid.get(gy)?.get(gx).copied()?
                };
                row.push(if bit == 0 { '0' } else { '1' });
            }
            rows.push(row);
        }

        Some(ArucoBitGrid { width: total_width, border, rows })
    }

    fn draw_marker(&self, id: usize, side_px: u32) -> Option<GrayImage> {
        if id >= self.codes().len() {
            return None;
        }

        let total_width = self.total_width() as u32;
        let border = self.border_size() as u32;
        let data_width = self.data_width() as u32;
        let cell_base = side_px / total_width;
        if cell_base == 0 {
            return None;
        }
        let remainder = side_px % total_width;

        let code = self.codes()[id];
        let bits_loc = self.data_bits_location();
        let mut grid = vec![vec![0u8; data_width as usize]; data_width as usize];
        let nbits = bits_loc.len();
        for (i, &(x, y)) in bits_loc.iter().enumerate() {
            let bit = ((code >> (nbits - 1 - i)) & 1) as u8;
            grid[y as usize][x as usize] = bit;
        }

        let mut cell_offsets = Vec::with_capacity((total_width + 1) as usize);
        cell_offsets.push(0u32);
        for i in 0..total_width {
            let extra = if i < remainder { 1 } else { 0 };
            let next = cell_offsets[i as usize] + cell_base + extra;
            cell_offsets.push(next);
        }

        let mut img = GrayImage::new(side_px, side_px);
        for y in 0..total_width {
            for x in 0..total_width {
                let bit = if x < border || x >= total_width - border || y < border || y >= total_width - border { 0u8 } else { grid[(y - border) as usize][(x - border) as usize] };
                let val = if bit == 0 { 0 } else { 255 };
                let x0 = cell_offsets[x as usize];
                let x1 = cell_offsets[(x + 1) as usize];
                let y0 = cell_offsets[y as usize];
                let y1 = cell_offsets[(y + 1) as usize];
                for py in y0..y1 {
                    for px in x0..x1 {
                        img.put_pixel(px, py, Luma([val]));
                    }
                }
            }
        }

        Some(img)
    }

    fn generate_marker_set(&self, side_px: u32) -> Vec<(usize, GrayImage)> {
        (0..self.codes().len()).filter_map(|id| self.draw_marker(id, side_px).map(|img| (id, img))).collect()
    }

    fn draw_charuco_board(&self, squares_x: u32, squares_y: u32, square_length: u32, marker_length: u32) -> GrayImage {
        let width = squares_x * square_length;
        let height = squares_y * square_length;
        let mut img = GrayImage::new(width, height);
        let mut id = 0usize;
        for j in 0..squares_y {
            for i in 0..squares_x {
                let x0 = i * square_length;
                let y0 = j * square_length;
                let white = (i + j) % 2 == 0;
                let val = if white { 255 } else { 0 };
                for y in y0..y0 + square_length {
                    for x in x0..x0 + square_length {
                        img.put_pixel(x, y, Luma([val as u8]));
                    }
                }
                if white {
                    if let Some(marker) = self.draw_marker(id % self.codes().len(), marker_length) {
                        let mw = marker.width();
                        let mh = marker.height();
                        if mw == 0 || mh == 0 || mw > square_length || mh > square_length {
                            id += 1;
                            continue;
                        }
                        let off_x = (square_length - mw) / 2;
                        let off_y = (square_length - mh) / 2;
                        for y in 0..mh {
                            for x in 0..mw {
                                img.put_pixel(x0 + off_x + x, y0 + off_y + y, *marker.get_pixel(x, y));
                            }
                        }
                    }
                    id += 1;
                }
            }
        }
        img
    }
}

/// Output of the tag-family bit decoder.
///
/// This is *not* a geometric detection: it does not include image-space corners. It is purely the
/// decoded ID/rotation plus decoder metadata that can be combined with a quad to form a full
/// `ArucoDetection2D`.
#[derive(Clone, Debug)]
pub struct ArucoTagDecode {
    pub id: u32,
    pub rotation: u8,
    pub border_width: u8,
    pub data_width: u8,
    pub score: Option<f32>,
    pub best_distance: Option<u32>,
    pub second_distance: Option<u32>,
    pub border_mismatches: Option<usize>,
    pub contrast_range: Option<f32>,
}
