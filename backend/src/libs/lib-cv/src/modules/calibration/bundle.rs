use super::distortion;
use super::projection;
use super::residuals;
use super::rodrigues;
use super::types::*;
use super::zhang;
use nalgebra::{DMatrix, DVector, Vector3};
use std::time::Instant;

pub(super) fn refine_solution_with_distortion(views: &[ViewObservations], initial: &SolveState, iters: u8) -> Result<SolveState, CalibrationSolveError> {
    let refine_start = Instant::now();
    let intr = initial.intrinsics;
    let dist = initial.distortion;
    let mut undistorted_views: Vec<ViewObservations> = Vec::with_capacity(views.len());
    let mut dropped = 0usize;
    for view in views {
        let mut points = Vec::with_capacity(view.points.len());
        for (obj, img) in &view.points {
            let xd = (img.0 - intr.cx) / intr.fx;
            let yd = (img.1 - intr.cy) / intr.fy;
            let (xu, yu) = projection::undistort_iter(xd, yd, dist, initial.lens_model, iters);
            let u = intr.fx * xu + intr.cx;
            let v = intr.fy * yu + intr.cy;
            if !u.is_finite() || !v.is_finite() {
                dropped = dropped.saturating_add(1);
                continue;
            }
            points.push((*obj, (u, v)));
        }
        undistorted_views.push(ViewObservations { index: view.index, points });
    }
    tracing::info!(
        views = views.len(),
        points = views.iter().map(|view| view.points.len()).sum::<usize>(),
        dropped,
        elapsed_ms = refine_start.elapsed().as_millis(),
        "calibration: distortion undistort complete"
    );
    let zhang_start = Instant::now();
    zhang::solve_zhang(&undistorted_views, None, initial.lens_model).inspect(|_| {
        tracing::info!(elapsed_ms = zhang_start.elapsed().as_millis(), "calibration: distortion zhang complete");
    })
}

pub(super) fn refine_solution_bundle_adjustment(views: &[ViewObservations], initial: &SolveState, iters: u8) -> Result<SolveState, CalibrationSolveError> {
    if iters == 0 {
        return Ok(initial.clone());
    }
    tracing::info!(views = views.len(), points = views.iter().map(|view| view.points.len()).sum::<usize>(), iters, "calibration: bundle adjustment start");

    let mut view_map: std::collections::HashMap<usize, &ViewObservations> = std::collections::HashMap::new();
    for view in views {
        view_map.insert(view.index, view);
    }
    let mut used_views: Vec<&ViewObservations> = Vec::new();
    for idx in &initial.view_indices {
        if let Some(view) = view_map.get(idx) {
            used_views.push(*view);
        }
    }
    if used_views.len() < 2 || initial.extrinsics.len() < 2 {
        return Err(CalibrationSolveError::ExtrinsicsSolveFailed);
    }

    let dist_n = distortion::distortion_param_count(initial.lens_model);
    let mut params = Vec::with_capacity(4 + dist_n + initial.extrinsics.len() * 6);
    params.push(initial.intrinsics.fx);
    params.push(initial.intrinsics.fy);
    params.push(initial.intrinsics.cx);
    params.push(initial.intrinsics.cy);
    distortion::push_distortion_params(&mut params, initial.distortion, initial.lens_model);
    for ext in &initial.extrinsics {
        let rvec = rodrigues::matrix_to_rodrigues(&ext.r);
        params.push(rvec.x);
        params.push(rvec.y);
        params.push(rvec.z);
        params.push(ext.t.x);
        params.push(ext.t.y);
        params.push(ext.t.z);
    }

    let mut lambda = 1e-3f64;
    let mut best_params = params.clone();
    let mut best_residuals = residuals::compute_residuals(&best_params, &used_views, initial.lens_model);
    let mut best_rms = residuals::residuals_rms(&best_residuals);

    for _ in 0..iters {
        let n = best_params.len();
        let m = best_residuals.len();
        if m == 0 || n == 0 {
            break;
        }

        let mut j = DMatrix::<f64>::zeros(m, n);
        for i in 0..n {
            let mut perturbed = best_params.clone();
            let eps = 1e-4 * (best_params[i].abs() + 1.0);
            perturbed[i] += eps;
            let r_eps = residuals::compute_residuals(&perturbed, &used_views, initial.lens_model);
            for (row, (r_new, r_old)) in r_eps.iter().zip(best_residuals.iter()).enumerate() {
                j[(row, i)] = (r_new - r_old) / eps;
            }
        }

        let jt = j.transpose();
        let mut jtj = &jt * &j;
        for i in 0..n {
            let d = jtj[(i, i)].abs().max(1e-12);
            jtj[(i, i)] += lambda * d;
        }
        let jtr = &jt * DVector::from_vec(best_residuals.clone());
        let delta = if let Some(chol) = jtj.clone().cholesky() {
            chol.solve(&(-jtr))
        } else {
            let svd = jtj.svd(true, true);
            match svd.solve(&(-jtr), 1e-12) {
                Ok(sol) => sol,
                Err(_) => break,
            }
        };

        if delta.len() != n {
            break;
        }

        let mut candidate = best_params.clone();
        for i in 0..n {
            candidate[i] += delta[i];
        }
        if !residuals::params_valid(&candidate, initial.lens_model) {
            lambda *= 10.0;
            continue;
        }

        let candidate_residuals = residuals::compute_residuals(&candidate, &used_views, initial.lens_model);
        let candidate_rms = residuals::residuals_rms(&candidate_residuals);
        if candidate_rms.is_finite() && candidate_rms < best_rms {
            best_params = candidate;
            best_residuals = candidate_residuals;
            best_rms = candidate_rms;
            lambda *= 0.5;
            if delta.norm() < 1e-6 {
                break;
            }
        } else {
            lambda *= 2.0;
        }
    }

    let refined = residuals::solve_state_from_params(&best_params, &used_views, &initial.view_indices, initial.lens_model)?;
    Ok(refined)
}

pub(super) fn refine_solution_bundle_adjustment_fisheye_fixed_principal_point(views: &[ViewObservations], initial: &SolveState, iters: u8) -> Result<SolveState, CalibrationSolveError> {
    if iters == 0 {
        return Ok(initial.clone());
    }
    if initial.lens_model != LensModel::Fisheye {
        return refine_solution_bundle_adjustment(views, initial, iters);
    }
    tracing::info!(views = views.len(), points = views.iter().map(|view| view.points.len()).sum::<usize>(), iters, "calibration: fisheye fixed-pp bundle adjustment start");

    let mut view_map: std::collections::HashMap<usize, &ViewObservations> = std::collections::HashMap::new();
    for view in views {
        view_map.insert(view.index, view);
    }
    let mut used_views: Vec<&ViewObservations> = Vec::new();
    for idx in &initial.view_indices {
        if let Some(view) = view_map.get(idx) {
            used_views.push(*view);
        }
    }
    if used_views.len() < 2 || initial.extrinsics.len() < 2 {
        return Err(CalibrationSolveError::ExtrinsicsSolveFailed);
    }

    let cx = initial.intrinsics.cx;
    let cy = initial.intrinsics.cy;
    let dist_n = distortion::distortion_param_count(LensModel::Fisheye);
    let mut params = Vec::with_capacity(2 + dist_n + initial.extrinsics.len() * 6);
    params.push(initial.intrinsics.fx);
    params.push(initial.intrinsics.fy);
    distortion::push_distortion_params(&mut params, initial.distortion, LensModel::Fisheye);
    for ext in &initial.extrinsics {
        let rvec = rodrigues::matrix_to_rodrigues(&ext.r);
        params.push(rvec.x);
        params.push(rvec.y);
        params.push(rvec.z);
        params.push(ext.t.x);
        params.push(ext.t.y);
        params.push(ext.t.z);
    }

    let mut lambda = 1e-3f64;
    let mut best_params = params.clone();
    let mut best_residuals = residuals::compute_residuals_fisheye_fixed_pp(&best_params, &used_views, cx, cy);
    let mut best_rms = residuals::residuals_rms(&best_residuals);

    for _ in 0..iters {
        let n = best_params.len();
        let m = best_residuals.len();
        if m == 0 || n == 0 {
            break;
        }

        let mut j = DMatrix::<f64>::zeros(m, n);
        for i in 0..n {
            let mut perturbed = best_params.clone();
            let eps = 1e-4 * (best_params[i].abs() + 1.0);
            perturbed[i] += eps;
            let r_eps = residuals::compute_residuals_fisheye_fixed_pp(&perturbed, &used_views, cx, cy);
            for (row, (r_new, r_old)) in r_eps.iter().zip(best_residuals.iter()).enumerate() {
                j[(row, i)] = (r_new - r_old) / eps;
            }
        }

        let jt = j.transpose();
        let mut jtj = &jt * &j;
        for i in 0..n {
            let d = jtj[(i, i)].abs().max(1e-12);
            jtj[(i, i)] += lambda * d;
        }
        let jtr = &jt * DVector::from_vec(best_residuals.clone());
        let delta = if let Some(chol) = jtj.clone().cholesky() {
            chol.solve(&(-jtr))
        } else {
            let svd = jtj.svd(true, true);
            match svd.solve(&(-jtr), 1e-12) {
                Ok(sol) => sol,
                Err(_) => break,
            }
        };
        if delta.len() != n {
            break;
        }

        let mut candidate = best_params.clone();
        for i in 0..n {
            candidate[i] += delta[i];
        }
        if !residuals::params_valid_fisheye_fixed_pp(&candidate) {
            lambda *= 10.0;
            continue;
        }

        let candidate_residuals = residuals::compute_residuals_fisheye_fixed_pp(&candidate, &used_views, cx, cy);
        let candidate_rms = residuals::residuals_rms(&candidate_residuals);
        if candidate_rms.is_finite() && candidate_rms < best_rms {
            best_params = candidate;
            best_residuals = candidate_residuals;
            best_rms = candidate_rms;
            lambda *= 0.5;
            if delta.norm() < 1e-6 {
                break;
            }
        } else {
            lambda *= 2.0;
        }
    }

    residuals::solve_state_from_params_fisheye_fixed_pp(&best_params, &used_views, &initial.view_indices, cx, cy)
}

pub(super) fn refine_solution_bundle_adjustment_fixed_intrinsics(views: &[ViewObservations], initial: &SolveState, iters: u8) -> Result<SolveState, CalibrationSolveError> {
    if iters == 0 {
        return Ok(initial.clone());
    }

    tracing::info!(views = views.len(), points = views.iter().map(|view| view.points.len()).sum::<usize>(), iters, "calibration: fixed-intrinsics bundle adjustment start");

    let mut view_map: std::collections::HashMap<usize, &ViewObservations> = std::collections::HashMap::new();
    for view in views {
        view_map.insert(view.index, view);
    }
    let mut used_views: Vec<&ViewObservations> = Vec::new();
    for idx in &initial.view_indices {
        if let Some(view) = view_map.get(idx) {
            used_views.push(*view);
        }
    }
    if used_views.len() < 2 || initial.extrinsics.len() < 2 {
        return Err(CalibrationSolveError::ExtrinsicsSolveFailed);
    }

    let dist_n = distortion::distortion_param_count(initial.lens_model);
    let mut params = Vec::with_capacity(dist_n + initial.extrinsics.len() * 6);
    distortion::push_distortion_params(&mut params, initial.distortion, initial.lens_model);
    for ext in &initial.extrinsics {
        let rvec = rodrigues::matrix_to_rodrigues(&ext.r);
        params.push(rvec.x);
        params.push(rvec.y);
        params.push(rvec.z);
        params.push(ext.t.x);
        params.push(ext.t.y);
        params.push(ext.t.z);
    }

    let mut lambda = 1e-3f64;
    let mut best_params = params.clone();
    let mut best_residuals = residuals::compute_residuals_fixed(&best_params, &used_views, initial.intrinsics, initial.lens_model);
    let mut best_rms = residuals::residuals_rms(&best_residuals);

    for _ in 0..iters {
        let n = best_params.len();
        let m = best_residuals.len();
        if m == 0 || n == 0 {
            break;
        }

        let mut j = DMatrix::<f64>::zeros(m, n);
        for i in 0..n {
            let mut perturbed = best_params.clone();
            let eps = 1e-4 * (best_params[i].abs() + 1.0);
            perturbed[i] += eps;
            let r_eps = residuals::compute_residuals_fixed(&perturbed, &used_views, initial.intrinsics, initial.lens_model);
            for (row, (r_new, r_old)) in r_eps.iter().zip(best_residuals.iter()).enumerate() {
                j[(row, i)] = (r_new - r_old) / eps;
            }
        }

        let jt = j.transpose();
        let mut jtj = &jt * &j;
        for i in 0..n {
            let d = jtj[(i, i)].abs().max(1e-12);
            jtj[(i, i)] += lambda * d;
        }
        let jtr = &jt * DVector::from_vec(best_residuals.clone());
        let delta = if let Some(chol) = jtj.clone().cholesky() {
            chol.solve(&(-jtr))
        } else {
            let svd = jtj.svd(true, true);
            match svd.solve(&(-jtr), 1e-12) {
                Ok(sol) => sol,
                Err(_) => break,
            }
        };

        let mut candidate = best_params.clone();
        for i in 0..n {
            candidate[i] += delta[i];
        }
        if !residuals::params_valid_fixed(&candidate, initial.lens_model) {
            lambda *= 10.0;
            continue;
        }

        let candidate_residuals = residuals::compute_residuals_fixed(&candidate, &used_views, initial.intrinsics, initial.lens_model);
        let candidate_rms = residuals::residuals_rms(&candidate_residuals);
        if candidate_rms < best_rms {
            best_params = candidate;
            best_residuals = candidate_residuals;
            best_rms = candidate_rms;
            lambda *= 0.5;
        } else {
            lambda *= 2.0;
        }
    }

    let mut refined = initial.clone();
    refined.distortion = distortion::distortion_from_params(&best_params, 0, initial.lens_model);
    let mut extrinsics = Vec::with_capacity(used_views.len());
    let mut off = dist_n;
    for _ in 0..used_views.len() {
        if off + 5 >= best_params.len() {
            break;
        }
        let rvec = Vector3::new(best_params[off], best_params[off + 1], best_params[off + 2]);
        let tvec = Vector3::new(best_params[off + 3], best_params[off + 4], best_params[off + 5]);
        let rmat = rodrigues::rodrigues_to_matrix(rvec);
        extrinsics.push(Extrinsics { r: rmat, t: tvec });
        off += 6;
    }
    refined.extrinsics = extrinsics;
    refined.views_used = refined.extrinsics.len();
    refined.points_used = used_views.iter().map(|view| view.points.len()).sum();
    refined.reprojection_error_px = best_rms;

    tracing::info!(rms = best_rms, "calibration: fixed-intrinsics bundle adjustment complete");

    Ok(refined)
}
