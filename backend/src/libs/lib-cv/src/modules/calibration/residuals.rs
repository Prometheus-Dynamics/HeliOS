use super::distortion;
use super::projection;
use super::rodrigues;
use super::types::*;
use nalgebra::Vector3;

pub(super) fn params_valid(params: &[f64], lens_model: LensModel) -> bool {
    let dist_n = distortion::distortion_param_count(lens_model);
    if params.len() < 4 + dist_n {
        return false;
    }
    let fx = params[0];
    let fy = params[1];
    let cx = params[2];
    let cy = params[3];
    if !fx.is_finite() || !fy.is_finite() || !cx.is_finite() || !cy.is_finite() {
        return false;
    }
    if fx <= 0.0 || fy <= 0.0 {
        return false;
    }
    if lens_model == LensModel::Fisheye {
        let ratio = fx / fy;
        if !ratio.is_finite() || !(0.5..=2.0).contains(&ratio) {
            return false;
        }
        let dist = distortion::distortion_from_params(params, 4, lens_model);
        if !distortion::distortion_is_reasonable(dist, lens_model) {
            return false;
        }
    }
    params.iter().all(|v| v.is_finite())
}

pub(super) fn params_valid_fixed(params: &[f64], lens_model: LensModel) -> bool {
    let dist_n = distortion::distortion_param_count(lens_model);
    if params.len() < dist_n {
        return false;
    }
    if lens_model == LensModel::Fisheye {
        let dist = distortion::distortion_from_params(params, 0, lens_model);
        if !distortion::distortion_is_reasonable(dist, lens_model) {
            return false;
        }
    }
    params.iter().all(|v| v.is_finite())
}

pub(super) fn residuals_rms(residuals: &[f64]) -> f64 {
    if residuals.is_empty() {
        return 0.0;
    }
    let mut sum = 0.0;
    for v in residuals {
        sum += v * v;
    }
    (sum / (residuals.len() as f64)).sqrt()
}

pub(super) fn compute_residuals(params: &[f64], views: &[&ViewObservations], lens_model: LensModel) -> Vec<f64> {
    let dist_n = distortion::distortion_param_count(lens_model);
    let fx = params.first().copied().unwrap_or(1.0);
    let fy = params.get(1).copied().unwrap_or(1.0);
    let cx = params.get(2).copied().unwrap_or(0.0);
    let cy = params.get(3).copied().unwrap_or(0.0);
    let dist = distortion::distortion_from_params(params, 4, lens_model);
    let mut residuals = Vec::new();
    let base = 4 + dist_n;
    for (view_idx, view) in views.iter().enumerate() {
        let off = base + view_idx * 6;
        if off + 5 >= params.len() {
            break;
        }
        let rvec = Vector3::new(params[off], params[off + 1], params[off + 2]);
        let tvec = Vector3::new(params[off + 3], params[off + 4], params[off + 5]);
        let rmat = rodrigues::rodrigues_to_matrix(rvec);
        for (obj, img) in &view.points {
            let p = rmat * Vector3::new(obj.0, obj.1, 0.0) + tvec;
            if !p.z.is_finite() || p.z <= 1e-9 {
                residuals.push(1.0e6);
                residuals.push(1.0e6);
                continue;
            }
            let x = p.x / p.z;
            let y = p.y / p.z;
            let (xd, yd) = projection::distort_norm(x, y, dist, lens_model);
            let u = fx * xd + cx;
            let v = fy * yd + cy;
            residuals.push(u - img.0);
            residuals.push(v - img.1);
        }
    }
    residuals
}

pub(super) fn compute_residuals_fisheye_fixed_pp(params: &[f64], views: &[&ViewObservations], cx: f64, cy: f64) -> Vec<f64> {
    let dist_n = distortion::distortion_param_count(LensModel::Fisheye);
    let fx = params.first().copied().unwrap_or(1.0);
    let fy = params.get(1).copied().unwrap_or(1.0);
    let dist = distortion::distortion_from_params(params, 2, LensModel::Fisheye);
    let mut residuals = Vec::new();
    let base = 2 + dist_n;
    for (view_idx, view) in views.iter().enumerate() {
        let off = base + view_idx * 6;
        if off + 5 >= params.len() {
            break;
        }
        let rvec = Vector3::new(params[off], params[off + 1], params[off + 2]);
        let tvec = Vector3::new(params[off + 3], params[off + 4], params[off + 5]);
        let rmat = rodrigues::rodrigues_to_matrix(rvec);
        for (obj, img) in &view.points {
            let p = rmat * Vector3::new(obj.0, obj.1, 0.0) + tvec;
            if !p.z.is_finite() || p.z <= 1e-9 {
                residuals.push(1.0e6);
                residuals.push(1.0e6);
                continue;
            }
            let x = p.x / p.z;
            let y = p.y / p.z;
            let (xd, yd) = projection::distort_norm(x, y, dist, LensModel::Fisheye);
            let u = fx * xd + cx;
            let v = fy * yd + cy;
            residuals.push(u - img.0);
            residuals.push(v - img.1);
        }
    }
    residuals
}

pub(super) fn params_valid_fisheye_fixed_pp(params: &[f64]) -> bool {
    let dist_n = distortion::distortion_param_count(LensModel::Fisheye);
    if params.len() < 2 + dist_n {
        return false;
    }
    let fx = params[0];
    let fy = params[1];
    if !fx.is_finite() || !fy.is_finite() || fx <= 0.0 || fy <= 0.0 {
        return false;
    }
    let ratio = fx / fy;
    if !ratio.is_finite() || !(0.5..=2.0).contains(&ratio) {
        return false;
    }
    let dist = distortion::distortion_from_params(params, 2, LensModel::Fisheye);
    if !distortion::distortion_is_reasonable(dist, LensModel::Fisheye) {
        return false;
    }
    params.iter().all(|v| v.is_finite())
}

pub(super) fn solve_state_from_params_fisheye_fixed_pp(params: &[f64], views: &[&ViewObservations], view_indices: &[usize], cx: f64, cy: f64) -> Result<SolveState, CalibrationSolveError> {
    let dist_n = distortion::distortion_param_count(LensModel::Fisheye);
    if params.len() < 2 + dist_n {
        return Err(CalibrationSolveError::IntrinsicsSolveFailed);
    }
    let intrinsics = IntrinsicsSolve { fx: params[0], fy: params[1], skew: 0.0, cx, cy };
    let distortion = distortion::distortion_from_params(params, 2, LensModel::Fisheye);
    if !distortion::distortion_is_reasonable(distortion, LensModel::Fisheye) {
        return Err(CalibrationSolveError::IntrinsicsSolveFailed);
    }

    let base = 2 + dist_n;
    let mut extrinsics = Vec::new();
    for i in 0..views.len() {
        let off = base + i * 6;
        if off + 5 >= params.len() {
            break;
        }
        let rvec = Vector3::new(params[off], params[off + 1], params[off + 2]);
        let tvec = Vector3::new(params[off + 3], params[off + 4], params[off + 5]);
        let rmat = rodrigues::rodrigues_to_matrix(rvec);
        extrinsics.push(Extrinsics { r: rmat, t: tvec });
    }
    if extrinsics.len() < 2 {
        return Err(CalibrationSolveError::ExtrinsicsSolveFailed);
    }

    let reproj = projection::reprojection_error(&views.iter().map(|v| (*v).clone()).collect::<Vec<_>>(), &extrinsics, intrinsics, distortion, LensModel::Fisheye);
    let points_used = views.iter().map(|view| view.points.len()).sum();

    Ok(SolveState {
        intrinsics,
        distortion,
        reprojection_error_px: reproj,
        warnings: Vec::new(),
        views_used: views.len(),
        points_used,
        intrinsics_degenerate: false,
        extrinsics,
        view_indices: view_indices.to_vec(),
        lens_model: LensModel::Fisheye,
    })
}

pub(super) fn compute_residuals_fixed(params: &[f64], views: &[&ViewObservations], intrinsics: IntrinsicsSolve, lens_model: LensModel) -> Vec<f64> {
    let dist_n = distortion::distortion_param_count(lens_model);
    let dist = distortion::distortion_from_params(params, 0, lens_model);
    let mut residuals = Vec::new();
    let base = dist_n;
    for (view_idx, view) in views.iter().enumerate() {
        let off = base + view_idx * 6;
        if off + 5 >= params.len() {
            break;
        }
        let rvec = Vector3::new(params[off], params[off + 1], params[off + 2]);
        let tvec = Vector3::new(params[off + 3], params[off + 4], params[off + 5]);
        let rmat = rodrigues::rodrigues_to_matrix(rvec);
        for (obj, img) in &view.points {
            let p = rmat * Vector3::new(obj.0, obj.1, 0.0) + tvec;
            if !p.z.is_finite() || p.z <= 1e-9 {
                residuals.push(1.0e6);
                residuals.push(1.0e6);
                continue;
            }
            let x = p.x / p.z;
            let y = p.y / p.z;
            let (xd, yd) = projection::distort_norm(x, y, dist, lens_model);
            let u = intrinsics.fx * xd + intrinsics.cx + intrinsics.skew * yd;
            let v = intrinsics.fy * yd + intrinsics.cy;
            residuals.push(u - img.0);
            residuals.push(v - img.1);
        }
    }
    residuals
}

pub(super) fn solve_state_from_params(params: &[f64], views: &[&ViewObservations], view_indices: &[usize], lens_model: LensModel) -> Result<SolveState, CalibrationSolveError> {
    let dist_n = distortion::distortion_param_count(lens_model);
    if params.len() < 4 + dist_n {
        return Err(CalibrationSolveError::IntrinsicsSolveFailed);
    }
    let intrinsics = IntrinsicsSolve { fx: params[0], fy: params[1], skew: 0.0, cx: params[2], cy: params[3] };
    let distortion = distortion::distortion_from_params(params, 4, lens_model);
    if !distortion::distortion_is_reasonable(distortion, lens_model) {
        return Err(CalibrationSolveError::IntrinsicsSolveFailed);
    }

    let base = 4 + dist_n;
    let mut extrinsics = Vec::new();
    for i in 0..views.len() {
        let off = base + i * 6;
        if off + 5 >= params.len() {
            break;
        }
        let rvec = Vector3::new(params[off], params[off + 1], params[off + 2]);
        let tvec = Vector3::new(params[off + 3], params[off + 4], params[off + 5]);
        let rmat = rodrigues::rodrigues_to_matrix(rvec);
        extrinsics.push(Extrinsics { r: rmat, t: tvec });
    }

    if extrinsics.len() < 2 {
        return Err(CalibrationSolveError::ExtrinsicsSolveFailed);
    }

    let reproj = projection::reprojection_error(&views.iter().map(|v| (*v).clone()).collect::<Vec<_>>(), &extrinsics, intrinsics, distortion, lens_model);
    let points_used = views.iter().map(|view| view.points.len()).sum();

    Ok(SolveState {
        intrinsics,
        distortion,
        reprojection_error_px: reproj,
        warnings: Vec::new(),
        views_used: views.len(),
        points_used,
        intrinsics_degenerate: false,
        extrinsics,
        view_indices: view_indices.to_vec(),
        lens_model,
    })
}
