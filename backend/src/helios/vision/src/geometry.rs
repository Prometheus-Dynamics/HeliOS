//! Small 2-D geometry helpers: points and homographies.

/// A point in image coordinates (x right, y down).
pub type Point = [f32; 2];

pub fn distance(a: Point, b: Point) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
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
}
