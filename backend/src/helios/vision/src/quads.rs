//! Candidate quadrilaterals from a binary image.

use crate::contour::{ComponentFilter, outer_contours};
use crate::geometry::{Point, approx_closed, distance, is_convex, perimeter, signed_area2};
use crate::image::BinaryImage;

/// A convex quadrilateral, corners clockwise in image coordinates (y down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quad {
    pub corners: [Point; 4],
}

/// Parameters for [`find_quads`]. Rates are relative to the larger image side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct QuadConfig {
    /// Minimum quad perimeter as a fraction of the larger image side.
    pub min_perimeter_rate: f32,
    /// Maximum quad perimeter as a fraction of the larger image side.
    pub max_perimeter_rate: f32,
    /// Polygon approximation tolerance as a fraction of the contour perimeter.
    pub approx_accuracy_rate: f32,
    /// Minimum side length as a fraction of the quad perimeter.
    pub min_side_rate: f32,
    /// Minimum distance of every corner from the image border, in pixels.
    pub min_border_distance: f32,
}

impl Default for QuadConfig {
    fn default() -> Self {
        Self { min_perimeter_rate: 0.03, max_perimeter_rate: 4.0, approx_accuracy_rate: 0.03, min_side_rate: 0.05, min_border_distance: 3.0 }
    }
}

/// Find convex quadrilaterals outlining foreground components.
pub fn find_quads(binary: &BinaryImage, config: &QuadConfig) -> Vec<Quad> {
    let (width, height) = (binary.width() as f32, binary.height() as f32);
    let side = width.max(height);
    let min_perimeter = config.min_perimeter_rate * side;
    let max_perimeter = config.max_perimeter_rate * side;
    // A component's bounding-box perimeter bounds its outline from below
    // (and is within a factor of sqrt(2) of a square's perimeter).
    let filter = ComponentFilter { min_box_perimeter: (min_perimeter / 1.5) as usize, max_box_perimeter: (max_perimeter * 1.5) as usize };

    let mut quads = Vec::new();
    for contour in outer_contours(binary, &filter) {
        if contour.len() < 8 {
            continue;
        }
        let contour_perimeter = contour.len() as f32;
        let poly = approx_closed(&contour, config.approx_accuracy_rate * contour_perimeter);
        if poly.len() != 4 || !is_convex(&poly) {
            continue;
        }
        let quad_perimeter = perimeter(&poly);
        if quad_perimeter < min_perimeter || quad_perimeter > max_perimeter {
            continue;
        }
        let min_side = config.min_side_rate * quad_perimeter;
        if (0..4).any(|i| distance(poly[i], poly[(i + 1) % 4]) < min_side) {
            continue;
        }
        let border = config.min_border_distance;
        if poly.iter().any(|p| p[0] < border || p[1] < border || p[0] > width - 1.0 - border || p[1] > height - 1.0 - border) {
            continue;
        }
        let mut corners = [poly[0], poly[1], poly[2], poly[3]];
        if signed_area2(&corners) < 0.0 {
            corners.reverse();
        }
        quads.push(Quad { corners });
    }
    quads
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binary_with_square(x0: usize, y0: usize, size: usize) -> BinaryImage {
        let (w, h) = (120, 100);
        let mut data = vec![0u8; w * h];
        for y in y0..y0 + size {
            for x in x0..x0 + size {
                data[y * w + x] = 1;
            }
        }
        BinaryImage::new(w, h, data).unwrap()
    }

    #[test]
    fn finds_square_as_clockwise_quad() {
        let quads = find_quads(&binary_with_square(30, 20, 40), &QuadConfig::default());
        assert_eq!(quads.len(), 1);
        let corners = quads[0].corners;
        assert!(signed_area2(&corners) > 0.0);
        for expected in [[30.0, 20.0], [69.0, 20.0], [69.0, 59.0], [30.0, 59.0]] {
            assert!(corners.iter().any(|c| distance(*c, expected) < 1.5), "{expected:?} not in {corners:?}");
        }
    }

    #[test]
    fn rejects_quads_touching_the_border() {
        assert!(find_quads(&binary_with_square(0, 0, 40), &QuadConfig::default()).is_empty());
    }

    #[test]
    fn rejects_tiny_components() {
        assert!(find_quads(&binary_with_square(50, 50, 2), &QuadConfig::default()).is_empty());
    }
}
