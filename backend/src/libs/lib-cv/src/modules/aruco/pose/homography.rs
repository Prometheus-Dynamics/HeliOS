use super::*;

pub(super) fn homography_dlt_v1(object_xy: &[(f64, f64); 4], image_uv: &[(f64, f64); 4]) -> Option<Matrix3<f64>> {
    // Solve for the planar homography from 4 correspondences by fixing h33 = 1 and
    // solving an 8×8 linear system. This avoids an SVD dependency (and works well on
    // embedded targets).
    //
    // u = (h11*x + h12*y + h13) / (h31*x + h32*y + 1)
    // v = (h21*x + h22*y + h23) / (h31*x + h32*y + 1)
    //
    // Unknowns: [h11 h12 h13 h21 h22 h23 h31 h32]
    let mut a = SMatrix::<f64, 8, 8>::zeros();
    let mut b = SVector::<f64, 8>::zeros();

    for i in 0..4 {
        let (x, y) = object_xy[i];
        let (u, v) = image_uv[i];
        if !x.is_finite() || !y.is_finite() || !u.is_finite() || !v.is_finite() {
            return None;
        }

        let r0 = 2 * i;
        let r1 = r0 + 1;

        a[(r0, 0)] = x;
        a[(r0, 1)] = y;
        a[(r0, 2)] = 1.0;
        a[(r0, 6)] = -u * x;
        a[(r0, 7)] = -u * y;
        b[r0] = u;

        a[(r1, 3)] = x;
        a[(r1, 4)] = y;
        a[(r1, 5)] = 1.0;
        a[(r1, 6)] = -v * x;
        a[(r1, 7)] = -v * y;
        b[r1] = v;
    }

    let params = a.lu().solve(&b)?;
    Some(Matrix3::new(params[0], params[1], params[2], params[3], params[4], params[5], params[6], params[7], 1.0))
}

type HomographyPointSet = [(f64, f64); 4];
type HomographyNormalized = (HomographyPointSet, Matrix3<f64>);

fn homography_point_normalization(points: &HomographyPointSet) -> Option<HomographyNormalized> {
    let mut centroid = Vector2::zeros();
    for (x, y) in points {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        centroid.x += x;
        centroid.y += y;
    }
    centroid /= points.len() as f64;

    let mut mean_dist = 0.0f64;
    for (x, y) in points {
        let dx = x - centroid.x;
        let dy = y - centroid.y;
        mean_dist += (dx * dx + dy * dy).sqrt();
    }
    mean_dist /= points.len() as f64;
    if !mean_dist.is_finite() || mean_dist.abs() < 1e-9 {
        return None;
    }

    let scale = (2.0f64).sqrt() / mean_dist;
    let t = Matrix3::new(scale, 0.0, -scale * centroid.x, 0.0, scale, -scale * centroid.y, 0.0, 0.0, 1.0);
    let normalized = points.map(|(x, y)| (scale * (x - centroid.x), scale * (y - centroid.y)));
    Some((normalized, t))
}

pub(super) fn homography_dlt_v2(object_xy: &[(f64, f64); 4], image_uv: &[(f64, f64); 4]) -> Option<Matrix3<f64>> {
    let (object_norm, t_object) = homography_point_normalization(object_xy)?;
    let (image_norm, t_image) = homography_point_normalization(image_uv)?;
    let h_norm = homography_dlt_v1(&object_norm, &image_norm)?;
    let t_image_inv = t_image.try_inverse()?;
    let mut h = t_image_inv * h_norm * t_object;
    let h33 = h[(2, 2)];
    if h33.is_finite() && h33.abs() > 1e-15 {
        h /= h33;
    }
    Some(h)
}

pub(super) fn decompose_planar_homography_v1(h: Matrix3<f64>, calib: TagPoseCalibration) -> Option<(Vector3<f64>, Matrix3<f64>)> {
    let k = Matrix3::new(calib.fx, 0.0, calib.cx, 0.0, calib.fy, calib.cy, 0.0, 0.0, 1.0);
    let k_inv = k.try_inverse()?;
    let b = k_inv * h;

    let b1 = b.column(0).into_owned();
    let b2 = b.column(1).into_owned();
    let b3 = b.column(2).into_owned();

    let n1 = b1.norm();
    let n2 = b2.norm();
    if !n1.is_finite() || !n2.is_finite() {
        return None;
    }
    let scale = (n1 + n2) * 0.5;
    if !scale.is_finite() || scale.abs() < 1e-15 {
        return None;
    }
    let lambda = 1.0 / scale;

    let r1 = b1 * lambda;
    let r2 = b2 * lambda;
    let t = b3 * lambda;
    let r3 = r1.cross(&r2);
    let r_raw = Matrix3::from_columns(&[r1, r2, r3]);

    // Orthonormalize rotation.
    let svd = r_raw.svd(true, true);
    let u = svd.u?;
    let v_t = svd.v_t?;
    let mut r = u * v_t;
    if r.determinant() < 0.0 {
        // Fix improper rotation.
        r.column_mut(2).neg_mut();
    }

    Some((t, r))
}

pub(super) fn decompose_planar_homography_v2(h: Matrix3<f64>, calib: TagPoseCalibration) -> Option<(Vector3<f64>, Matrix3<f64>)> {
    let k = Matrix3::new(calib.fx, 0.0, calib.cx, 0.0, calib.fy, calib.cy, 0.0, 0.0, 1.0);
    let k_inv = k.try_inverse()?;
    let b = k_inv * h;

    let b1 = b.column(0).into_owned();
    let b2 = b.column(1).into_owned();
    let b3 = b.column(2).into_owned();

    if !b1.iter().all(|v| v.is_finite()) || !b2.iter().all(|v| v.is_finite()) || !b3.iter().all(|v| v.is_finite()) {
        return None;
    }

    let r3 = b1.cross(&b2);
    let r_raw = Matrix3::from_columns(&[b1, b2, r3]);

    // Orthonormalize rotation.
    let svd = r_raw.svd(true, true);
    let u = svd.u?;
    let v_t = svd.v_t?;
    let mut r = u * v_t;
    if r.determinant() < 0.0 {
        r.column_mut(2).neg_mut();
    }

    let r1 = r.column(0).into_owned();
    let r2 = r.column(1).into_owned();
    let mut scale = (r1.dot(&b1) + r2.dot(&b2)) * 0.5;
    if !scale.is_finite() || scale.abs() < 1e-15 {
        return None;
    }
    // Homographies are defined up to scale; prefer a positive scale for consistent depths.
    if scale < 0.0 {
        scale = -scale;
        r.column_mut(0).neg_mut();
        r.column_mut(1).neg_mut();
    }

    let mut t = b3 / scale;
    if t.z.is_finite() && t.z < 0.0 {
        // Flip planar axes (equivalent homography scale) so the tag is in front of the camera.
        r.column_mut(0).neg_mut();
        r.column_mut(1).neg_mut();
        t = -t;
    }

    Some((t, r))
}

fn distort_norm_pinhole(x: f64, y: f64, calib: TagPoseCalibration) -> (f64, f64) {
    let r2 = x * x + y * y;
    let r4 = r2 * r2;
    let r6 = r4 * r2;
    let radial = 1.0 + calib.k1 * r2 + calib.k2 * r4 + calib.k3 * r6;
    let xy2 = 2.0 * x * y;
    let x2 = x * x;
    let y2 = y * y;
    let xt = calib.p1 * xy2 + calib.p2 * (r2 + 2.0 * x2);
    let yt = calib.p1 * (r2 + 2.0 * y2) + calib.p2 * xy2;
    (x * radial + xt, y * radial + yt)
}

fn distort_norm_fisheye(x: f64, y: f64, calib: TagPoseCalibration) -> (f64, f64) {
    let r = (x * x + y * y).sqrt();
    if !r.is_finite() || r <= 1e-9 {
        return (x, y);
    }
    let theta = r.atan();
    let theta2 = theta * theta;
    let theta4 = theta2 * theta2;
    let theta6 = theta4 * theta2;
    let theta8 = theta4 * theta4;
    let theta10 = theta8 * theta2;
    let k4 = calib.p1;
    let k5 = calib.p2;
    let theta_d = theta * (1.0 + calib.k1 * theta2 + calib.k2 * theta4 + calib.k3 * theta6 + k4 * theta8 + k5 * theta10);
    let scale = theta_d / r;
    (x * scale, y * scale)
}

fn distort_norm(x: f64, y: f64, calib: TagPoseCalibration) -> (f64, f64) {
    match calib.lens_model {
        LensModel::Pinhole => distort_norm_pinhole(x, y, calib),
        LensModel::Fisheye => distort_norm_fisheye(x, y, calib),
    }
}

pub(super) fn undistort_norm(xd: f64, yd: f64, calib: TagPoseCalibration) -> Option<(f64, f64)> {
    let mut x = xd;
    let mut y = yd;
    if !x.is_finite() || !y.is_finite() {
        return None;
    }

    let iters = calib.undistort_iters.clamp(1, 32) as usize;
    for _ in 0..iters {
        let (px, py) = distort_norm(x, y, calib);
        x -= px - xd;
        y -= py - yd;
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
    }
    Some((x, y))
}
