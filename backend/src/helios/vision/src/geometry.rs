//! Small 2-D geometry helpers: polygon simplification and homographies.

/// A point in image coordinates (x right, y down).
pub type Point = [f32; 2];

pub fn distance(a: Point, b: Point) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

/// Perimeter of a closed polygon.
pub fn perimeter(points: &[Point]) -> f32 {
    (0..points.len()).map(|i| distance(points[i], points[(i + 1) % points.len()])).sum()
}

/// Twice the signed area of a polygon (shoelace). Positive for clockwise
/// order in image coordinates (y down).
pub fn signed_area2(points: &[Point]) -> f32 {
    (0..points.len())
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % points.len()]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum()
}

/// Whether a polygon is strictly convex (all turns in the same direction).
pub fn is_convex(points: &[Point]) -> bool {
    let n = points.len();
    if n < 3 {
        return false;
    }
    let mut sign = 0.0f32;
    for i in 0..n {
        let (a, b, c) = (points[i], points[(i + 1) % n], points[(i + 2) % n]);
        let cross = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
        if cross.abs() < f32::EPSILON {
            return false;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}

/// Douglas-Peucker simplification of a closed contour. Splits the contour at
/// its first point and the point farthest from it, then simplifies both
/// chains with tolerance `epsilon`.
pub fn approx_closed(contour: &[(i32, i32)], epsilon: f32) -> Vec<Point> {
    let points: Vec<Point> = contour.iter().map(|&(x, y)| [x as f32, y as f32]).collect();
    let n = points.len();
    if n < 3 {
        return points;
    }
    let far = (1..n).max_by(|&a, &b| distance(points[0], points[a]).total_cmp(&distance(points[0], points[b]))).unwrap_or(0);
    let first: Vec<Point> = points[0..=far].to_vec();
    let mut second: Vec<Point> = points[far..].to_vec();
    second.push(points[0]);

    let mut out = douglas_peucker(&first, epsilon);
    out.pop(); // `far`, repeated as the start of the second chain
    let mut tail = douglas_peucker(&second, epsilon);
    tail.pop(); // the first point again
    out.extend(tail);
    out
}

fn douglas_peucker(points: &[Point], epsilon: f32) -> Vec<Point> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let (a, b) = (points[0], points[points.len() - 1]);
    let (index, max) = points[1..points.len() - 1].iter().enumerate().map(|(i, &p)| (i + 1, point_line_distance(p, a, b))).max_by(|x, y| x.1.total_cmp(&y.1)).unwrap_or((0, 0.0));
    if max <= epsilon {
        return vec![a, b];
    }
    let mut left = douglas_peucker(&points[..=index], epsilon);
    let right = douglas_peucker(&points[index..], epsilon);
    left.pop();
    left.extend(right);
    left
}

fn point_line_distance(p: Point, a: Point, b: Point) -> f32 {
    let length = distance(a, b);
    if length < f32::EPSILON {
        return distance(p, a);
    }
    ((b[0] - a[0]) * (a[1] - p[1]) - (a[0] - p[0]) * (b[1] - a[1])).abs() / length
}

/// A 3x3 planar homography (row-major, `h[8]` normalised to 1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Homography(pub [f64; 9]);

impl Homography {
    /// The homography mapping each `src[i]` to `dst[i]`.
    pub fn from_points(src: [Point; 4], dst: [Point; 4]) -> Option<Self> {
        // Solve the 8x8 system for h0..h7 with h8 = 1.
        let mut m = [[0f64; 9]; 8];
        for i in 0..4 {
            let (x, y) = (src[i][0] as f64, src[i][1] as f64);
            let (u, v) = (dst[i][0] as f64, dst[i][1] as f64);
            m[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
            m[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
        }
        let h = solve8(m)?;
        Some(Self([h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7], 1.0]))
    }

    pub fn map(&self, p: Point) -> Point {
        let h = &self.0;
        let (x, y) = (p[0] as f64, p[1] as f64);
        let w = h[6] * x + h[7] * y + h[8];
        [((h[0] * x + h[1] * y + h[2]) / w) as f32, ((h[3] * x + h[4] * y + h[5]) / w) as f32]
    }
}

/// Gaussian elimination with partial pivoting on an 8x9 augmented matrix.
fn solve8(mut m: [[f64; 9]; 8]) -> Option<[f64; 8]> {
    for col in 0..8 {
        let pivot = (col..8).max_by(|&a, &b| m[a][col].abs().total_cmp(&m[b][col].abs()))?;
        if m[pivot][col].abs() < 1e-12 {
            return None;
        }
        m.swap(col, pivot);
        let pivot_row = m[col];
        for (row_index, row) in m.iter_mut().enumerate() {
            if row_index != col {
                let factor = row[col] / pivot_row[col];
                for (value, &pivot_value) in row[col..].iter_mut().zip(&pivot_row[col..]) {
                    *value -= factor * pivot_value;
                }
            }
        }
    }
    let mut out = [0f64; 8];
    for i in 0..8 {
        out[i] = m[i][8] / m[i][i];
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn homography_maps_corners() {
        let src = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let dst = [[10.0, 20.0], [50.0, 25.0], [55.0, 70.0], [5.0, 60.0]];
        let h = Homography::from_points(src, dst).unwrap();
        for i in 0..4 {
            let p = h.map(src[i]);
            assert!(distance(p, dst[i]) < 1e-3, "{p:?} vs {:?}", dst[i]);
        }
    }

    #[test]
    fn approx_reduces_square_contour_to_four_corners() {
        let mut contour = Vec::new();
        for x in 0..10 {
            contour.push((x, 0));
        }
        for y in 1..10 {
            contour.push((9, y));
        }
        for x in (0..9).rev() {
            contour.push((x, 9));
        }
        for y in (1..9).rev() {
            contour.push((0, y));
        }
        let poly = approx_closed(&contour, 1.0);
        assert_eq!(poly.len(), 4, "{poly:?}");
        assert!(is_convex(&poly));
        assert!(signed_area2(&poly) > 0.0);
    }

    #[test]
    fn concave_polygon_is_not_convex() {
        let poly = [[0.0, 0.0], [4.0, 0.0], [2.0, 1.0], [4.0, 4.0], [0.0, 4.0]];
        assert!(!is_convex(&poly));
    }
}
