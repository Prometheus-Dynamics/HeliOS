use super::types::*;
use nalgebra::{DMatrix, Matrix3};

pub(super) fn v_ij(h: &Matrix3<f64>, i: usize, j: usize) -> Vec<f64> {
    let hi = h.column(i);
    let hj = h.column(j);
    let h1i = hi[0];
    let h2i = hi[1];
    let h3i = hi[2];
    let h1j = hj[0];
    let h2j = hj[1];
    let h3j = hj[2];
    vec![h1i * h1j, h1i * h2j + h2i * h1j, h2i * h2j, h3i * h1j + h1i * h3j, h3i * h2j + h2i * h3j, h3i * h3j]
}

pub(super) fn homography_dlt(obj: &[Point2], img: &[Point2]) -> Option<Matrix3<f64>> {
    if obj.len() != img.len() || obj.len() < 4 {
        return None;
    }
    let (t_obj, norm_obj) = normalize_points(obj)?;
    let (t_img, norm_img) = normalize_points(img)?;

    let n = obj.len();
    let mut a = DMatrix::<f64>::zeros(2 * n, 9);
    for (k, ((x, y), (u, v))) in norm_obj.iter().zip(norm_img.iter()).enumerate() {
        let row1 = 2 * k;
        let row2 = row1 + 1;
        a[(row1, 0)] = -*x;
        a[(row1, 1)] = -*y;
        a[(row1, 2)] = -1.0;
        a[(row1, 6)] = u * x;
        a[(row1, 7)] = u * y;
        a[(row1, 8)] = *u;

        a[(row2, 3)] = -*x;
        a[(row2, 4)] = -*y;
        a[(row2, 5)] = -1.0;
        a[(row2, 6)] = v * x;
        a[(row2, 7)] = v * y;
        a[(row2, 8)] = *v;
    }

    let svd = a.svd(true, true);
    let vt = svd.v_t?;
    let h = vt.row(vt.nrows() - 1).transpose();
    if h.len() != 9 {
        return None;
    }
    // DLT solves for `h` in row-major order: [h11, h12, h13, h21, ... , h33].
    // `Matrix3::from_column_slice` would interpret this as column-major and effectively transpose/scramble H.
    let hn = Matrix3::from_row_slice(h.as_slice());
    let h = t_img.try_inverse()? * hn * t_obj;
    let scale = if h[(2, 2)].abs() > 1e-12 { 1.0 / h[(2, 2)] } else { 1.0 };
    Some(h * scale)
}

pub(super) fn normalize_points(points: &[Point2]) -> Option<(Matrix3<f64>, Points2)> {
    if points.is_empty() {
        return None;
    }
    let (mut cx, mut cy) = (0.0, 0.0);
    for (x, y) in points {
        cx += *x;
        cy += *y;
    }
    cx /= points.len() as f64;
    cy /= points.len() as f64;

    let mut mean_dist = 0.0;
    for (x, y) in points {
        mean_dist += ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
    }
    mean_dist /= points.len() as f64;
    if !mean_dist.is_finite() || mean_dist <= 1e-12 {
        let t = Matrix3::new(1.0, 0.0, -cx, 0.0, 1.0, -cy, 0.0, 0.0, 1.0);
        let out = points.iter().map(|(x, y)| (x - cx, y - cy)).collect();
        return Some((t, out));
    }
    let s = (2.0f64).sqrt() / mean_dist;
    let t = Matrix3::new(s, 0.0, -s * cx, 0.0, s, -s * cy, 0.0, 0.0, 1.0);
    let out = points.iter().map(|(x, y)| (s * (x - cx), s * (y - cy))).collect();
    Some((t, out))
}
