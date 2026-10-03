//! ArUco / AprilTag marker decoding.

pub mod dictionary;
pub mod render;

pub use dictionary::{DICT_4X4_50, Dictionary, TAG_36H11};

use crate::geometry::{Homography, Point, distance};
use crate::image::GrayImage;
use crate::quads::{Quad, QuadConfig, find_quads, scale_quads};
use crate::refine::refine_corners;
use crate::threshold::{ThresholdConfig, adaptive_threshold};

/// A decoded marker. `corners[0]` is the marker's top-left corner as printed;
/// the rest follow clockwise in image coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct Marker {
    pub dictionary: &'static str,
    pub id: u32,
    pub corners: [Point; 4],
    /// Bit errors corrected to match the dictionary code.
    pub hamming: u32,
}

impl Marker {
    pub fn center(&self) -> Point {
        let mut c = [0.0, 0.0];
        for p in &self.corners {
            c[0] += p[0] / 4.0;
            c[1] += p[1] / 4.0;
        }
        c
    }
}

/// Parameters for [`decode_quads`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DecodeConfig {
    pub dictionary: &'static Dictionary,
    /// Minimum difference between the brightest and darkest cell means.
    pub min_contrast: f32,
    /// Fraction of border cells allowed to read as white.
    pub max_border_error_rate: f32,
    /// Bit errors to correct; `None` uses the dictionary's default.
    pub max_correction: Option<u32>,
}

impl Default for DecodeConfig {
    fn default() -> Self {
        Self { dictionary: &DICT_4X4_50, min_contrast: 20.0, max_border_error_rate: 0.2, max_correction: None }
    }
}

/// Parameters for the whole [`detect`] pipeline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DetectorConfig {
    /// Find quads on the image downscaled by this factor (1, 2 or 4);
    /// markers are always decoded at full resolution.
    pub decimate: u32,
    pub threshold: ThresholdConfig,
    pub quads: QuadConfig,
    pub decode: DecodeConfig,
}

impl Default for DetectorConfig {
    fn default() -> Self {
        Self { decimate: 2, threshold: ThresholdConfig::default(), quads: QuadConfig::default(), decode: DecodeConfig::default() }
    }
}

/// Threshold, find quads and decode markers in one call.
pub fn detect(gray: &GrayImage, config: &DetectorConfig) -> Vec<Marker> {
    let mut small = None;
    let mut factor = 1;
    while factor < config.decimate.min(4) {
        small = Some(small.as_ref().unwrap_or(gray).downscale2());
        factor *= 2;
    }
    let binary = adaptive_threshold(small.as_ref().unwrap_or(gray), &config.threshold);
    let quads = scale_quads(&find_quads(&binary, &config.quads), factor as f32);
    decode_quads(gray, &quads, &config.decode)
}

/// Decode each candidate quad against the dictionary. Quads that are not
/// markers are dropped; duplicate detections of a marker are merged.
pub fn decode_quads(gray: &GrayImage, quads: &[Quad], config: &DecodeConfig) -> Vec<Marker> {
    let mut markers: Vec<Marker> = Vec::new();
    for quad in quads {
        let Some(marker) = decode_quad(gray, quad, config) else { continue };
        let side = distance(marker.corners[0], marker.corners[1]);
        if let Some(existing) = markers.iter_mut().find(|m| m.id == marker.id && distance(m.center(), marker.center()) < side * 0.5) {
            if marker.hamming < existing.hamming {
                *existing = marker;
            }
            continue;
        }
        markers.push(marker);
    }
    markers
}

fn decode_quad(gray: &GrayImage, quad: &Quad, config: &DecodeConfig) -> Option<Marker> {
    let dict = config.dictionary;
    let n = dict.total_width() as usize;
    // Contour corners are the centres of the outermost marker pixels; the
    // marker edge is half a pixel further out.
    let coarse = expand(quad.corners, 0.5);
    let border_ok = |corners: [Point; 4]| -> Option<(Vec<f32>, f32)> {
        let cells = sample_cells(gray, corners, n)?;
        let (min, max) = cells.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
        if max - min < config.min_contrast {
            return None;
        }
        let threshold = otsu(&cells);
        let border_cells: Vec<f32> = (0..n * n).filter(|&i| is_border(i % n, i / n, n)).map(|i| cells[i]).collect();
        let border_errors = border_cells.iter().filter(|&&v| v >= threshold).count();
        if border_errors as f32 > config.max_border_error_rate * border_cells.len() as f32 {
            return None;
        }
        Some((cells, threshold))
    };
    // Reject with the coarse corners, then move the survivors onto the
    // sub-pixel edges and sample again.
    border_ok(coarse)?;
    let corners = refine_corners(gray, coarse).unwrap_or(coarse);
    let (cells, threshold) = border_ok(corners)?;

    let max_correction = config.max_correction.unwrap_or(dict.max_correction);
    let mut best: Option<(u32, u32, usize)> = None; // (hamming, id, rotation)
    for rotation in 0..4 {
        let rotated = rotate(corners, rotation);
        let grid = if rotation == 0 { cells.clone() } else { sample_cells(gray, rotated, n)? };
        let mut code = 0u64;
        for &(x, y) in dict.bits {
            let value = grid[(y as usize + 1) * n + x as usize + 1];
            code = (code << 1) | u64::from(value >= threshold);
        }
        for (id, &candidate) in dict.codes.iter().enumerate() {
            let hamming = (code ^ candidate).count_ones();
            if hamming <= max_correction && best.is_none_or(|(h, _, _)| hamming < h) {
                best = Some((hamming, id as u32, rotation));
            }
        }
    }
    let (hamming, id, rotation) = best?;
    Some(Marker { dictionary: dict.name, id, corners: rotate(corners, rotation), hamming })
}

fn is_border(x: usize, y: usize, n: usize) -> bool {
    x == 0 || y == 0 || x == n - 1 || y == n - 1
}

/// Rotate the corner order so that corner `start` comes first.
fn rotate(corners: [Point; 4], start: usize) -> [Point; 4] {
    [corners[start % 4], corners[(start + 1) % 4], corners[(start + 2) % 4], corners[(start + 3) % 4]]
}

/// Move each corner away from the quad's centre by `amount` pixels.
fn expand(corners: [Point; 4], amount: f32) -> [Point; 4] {
    let c = [corners.iter().map(|p| p[0]).sum::<f32>() / 4.0, corners.iter().map(|p| p[1]).sum::<f32>() / 4.0];
    corners.map(|p| {
        let d = distance(p, c).max(f32::EPSILON);
        [p[0] + (p[0] - c[0]) / d * amount * std::f32::consts::SQRT_2, p[1] + (p[1] - c[1]) / d * amount * std::f32::consts::SQRT_2]
    })
}

/// Mean intensity of each of the `n x n` grid cells inside the quad,
/// row-major in the quad's own frame (corner 0 at the grid origin).
fn sample_cells(gray: &GrayImage, corners: [Point; 4], n: usize) -> Option<Vec<f32>> {
    let side = n as f32;
    let h = Homography::from_points([[0.0, 0.0], [side, 0.0], [side, side], [0.0, side]], corners)?;
    // Sample the central part of each cell, away from its edges.
    const OFFSETS: [(f32, f32); 5] = [(0.5, 0.5), (0.3, 0.3), (0.7, 0.3), (0.3, 0.7), (0.7, 0.7)];
    let mut cells = Vec::with_capacity(n * n);
    for y in 0..n {
        for x in 0..n {
            let sum: f32 = OFFSETS
                .iter()
                .map(|&(dx, dy)| {
                    let p = h.map([x as f32 + dx, y as f32 + dy]);
                    gray.sample(p[0], p[1])
                })
                .sum();
            cells.push(sum / OFFSETS.len() as f32);
        }
    }
    Some(cells)
}

/// Otsu's threshold over a small set of values.
fn otsu(values: &[f32]) -> f32 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    let total: f32 = sorted.iter().sum();
    let n = sorted.len() as f32;
    let mut best = (f32::MIN, (sorted[0] + sorted[sorted.len() - 1]) / 2.0);
    let mut low_sum = 0.0;
    for i in 0..sorted.len() - 1 {
        low_sum += sorted[i];
        let low_n = (i + 1) as f32;
        let high_n = n - low_n;
        let low_mean = low_sum / low_n;
        let high_mean = (total - low_sum) / high_n;
        let between = low_n * high_n * (low_mean - high_mean).powi(2);
        if between > best.0 {
            best = (between, (sorted[i] + sorted[i + 1]) / 2.0);
        }
    }
    best.1
}

#[cfg(test)]
mod tests;
