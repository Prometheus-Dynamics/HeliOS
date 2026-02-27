use super::bundle;
use super::distortion;
use super::homography;
use super::projection;
use super::residuals;
use super::rodrigues;
use super::types::*;
use nalgebra::{DMatrix, DVector, Matrix3, Vector3};

pub(super) fn solve_fisheye_multi_start(views: &[ViewObservations], config: CalibrationSolveConfig, hint: Option<IntrinsicsSolve>) -> Result<SolveState, CalibrationSolveError> {
    let (min_x, max_x, min_y, max_y) = image_span(views).ok_or(CalibrationSolveError::IntrinsicsSolveFailed)?;
    let span_w = (max_x - min_x).abs().max(1.0);
    let span_h = (max_y - min_y).abs().max(1.0);
    let span = span_w.max(span_h).max(1.0);
    let (cx, cy) = match hint {
        Some(h) if h.cx.is_finite() && h.cy.is_finite() => (h.cx, h.cy),
        _ => (0.5 * (min_x + max_x), 0.5 * (min_y + max_y)),
    };

    // Multi-start is the main defense against fisheye local minima. The point span can be
    // small when users only capture near-center views, so include a fairly wide focal range.
    let mut f_guesses = vec![span * 0.2, span * 0.35, span * 0.5, span * 0.75, span, span * 1.5, span * 2.0, span * 3.0, span * 4.0, span * 6.0];
    if let Some(h) = hint {
        let fh = 0.5 * (h.fx + h.fy);
        if fh.is_finite() {
            f_guesses.push(fh);
            f_guesses.push(fh * 0.75);
            f_guesses.push(fh * 1.25);
        }
    }
    f_guesses.retain(|f| f.is_finite() && (*f >= 50.0 && *f <= 50_000.0));
    f_guesses.sort_by(|a, b| a.total_cmp(b));
    f_guesses.dedup_by(|a, b| (*a - *b).abs() < 1.0);
    if f_guesses.is_empty() {
        f_guesses.push(span.max(50.0));
    }

    let mut best_state: Option<SolveState> = None;
    let mut best_warnings: Vec<String> = Vec::new();
    let mut best_error = f64::INFINITY;

    // Rank candidates with a small BA budget, then run the full BA budget on the best one.
    let rank_iters = config.refine_undistort_iters.clamp(1, 3);

    for f in f_guesses.iter().copied() {
        let intrinsics = IntrinsicsSolve { fx: f, fy: f, skew: 0.0, cx, cy };

        let mut extrinsics: Vec<Extrinsics> = Vec::new();
        let mut view_indices: Vec<usize> = Vec::new();
        for view in views {
            // For fisheye, the planar homography trick is a weak initializer; instead, run a small
            // per-view extrinsics solve under the fisheye model (distortion starts at 0).
            let ext = solve_view_extrinsics_fisheye(view, intrinsics).or_else(|| {
                initial_extrinsics_from_homography_fisheye(view, intrinsics).or_else(|| {
                    let (rvec, tvec) = initial_extrinsics_guess(view, intrinsics);
                    Some(Extrinsics { r: rodrigues::rodrigues_to_matrix(rvec), t: tvec })
                })
            });
            if let Some(ext) = ext {
                extrinsics.push(ext);
                view_indices.push(view.index);
            }
        }
        if extrinsics.len() < 2 {
            continue;
        }

        let used_views: Vec<ViewObservations> = view_indices.iter().filter_map(|idx| views.iter().find(|v| v.index == *idx).cloned()).collect();
        let points_used = used_views.iter().map(|view| view.points.len()).sum();
        let reproj = projection::reprojection_error(&used_views, &extrinsics, intrinsics, DistortionCoefficients::default(), LensModel::Fisheye);

        let mut state = SolveState {
            intrinsics,
            distortion: DistortionCoefficients::default(),
            reprojection_error_px: reproj,
            warnings: Vec::new(),
            views_used: extrinsics.len(),
            points_used,
            intrinsics_degenerate: false,
            extrinsics,
            view_indices,
            lens_model: LensModel::Fisheye,
        };

        let mut candidate_warnings = Vec::new();
        if config.refine_distortion && rank_iters > 0 {
            // Prefer free principal point refinement for fisheye to match OpenCV-style calibrations.
            // Fall back to the older fixed-PP BA if the free-PP refinement becomes unstable.
            match refine_solution_bundle_adjustment_fisheye(views, &state, rank_iters, (min_x, max_x, min_y, max_y)) {
                Ok(refined) => state = refined,
                Err(_) => match bundle::refine_solution_bundle_adjustment_fisheye_fixed_principal_point(views, &state, rank_iters) {
                    Ok(refined) => {
                        candidate_warnings.push("fisheye BA: free-PP failed; used fixed-PP fallback".into());
                        state = refined;
                    }
                    Err(_) => candidate_warnings.push("fisheye bundle adjustment failed during ranking".into()),
                },
            };
        }

        if !distortion::distortion_is_reasonable(state.distortion, state.lens_model) {
            candidate_warnings.push("fisheye distortion out of range; skipping candidate".into());
            continue;
        }
        if !state.reprojection_error_px.is_finite() {
            candidate_warnings.push("fisheye reprojection error non-finite; skipping candidate".into());
            continue;
        }

        if state.reprojection_error_px < best_error {
            best_error = state.reprojection_error_px;
            best_state = Some(state);
            best_warnings = candidate_warnings;
        }
    }

    let mut state = best_state.ok_or(CalibrationSolveError::IntrinsicsSolveFailed)?;
    let mut warnings = Vec::new();
    warnings.push(format!("intrinsics solve: fisheye multi-start ({} candidates)", f_guesses.len()));
    warnings.append(&mut best_warnings);

    if config.refine_distortion && config.refine_undistort_iters > rank_iters {
        let before = state.clone();
        match refine_solution_bundle_adjustment_fisheye(views, &state, config.refine_undistort_iters, (min_x, max_x, min_y, max_y)) {
            Ok(refined) => {
                if distortion::distortion_is_reasonable(refined.distortion, refined.lens_model) && refined.reprojection_error_px.is_finite() {
                    state = refined;
                } else {
                    warnings.push("fisheye full BA produced unreasonable parameters; using ranked solution".into());
                    state = before;
                }
            }
            Err(_) => {
                // Last-resort: keep the older behavior rather than failing the solve.
                match bundle::refine_solution_bundle_adjustment_fisheye_fixed_principal_point(views, &state, config.refine_undistort_iters) {
                    Ok(refined) => {
                        if distortion::distortion_is_reasonable(refined.distortion, refined.lens_model) && refined.reprojection_error_px.is_finite() {
                            warnings.push("fisheye full BA: free-PP failed; used fixed-PP fallback".into());
                            state = refined;
                        } else {
                            warnings.push("fisheye full BA failed; using ranked solution".into());
                            state = before;
                        }
                    }
                    Err(_) => warnings.push("fisheye full BA failed; using ranked solution".into()),
                }
            }
        };
    }

    state.warnings = warnings;
    Ok(state)
}

fn refine_solution_bundle_adjustment_fisheye(views: &[ViewObservations], initial: &SolveState, iters: u8, span_bounds: (f64, f64, f64, f64)) -> Result<SolveState, CalibrationSolveError> {
    if iters == 0 {
        return Ok(initial.clone());
    }
    if initial.lens_model != LensModel::Fisheye {
        return bundle::refine_solution_bundle_adjustment(views, initial, iters);
    }

    tracing::info!(views = views.len(), points = views.iter().map(|view| view.points.len()).sum::<usize>(), iters, "calibration: fisheye free-pp bundle adjustment start");

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

    let dist_n = distortion::distortion_param_count(LensModel::Fisheye);
    let mut params = Vec::with_capacity(4 + dist_n + initial.extrinsics.len() * 6);
    params.push(initial.intrinsics.fx);
    params.push(initial.intrinsics.fy);
    params.push(initial.intrinsics.cx);
    params.push(initial.intrinsics.cy);
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

    let (min_x, max_x, min_y, max_y) = span_bounds;
    let span_x = (max_x - min_x).abs().max(1.0);
    let span_y = (max_y - min_y).abs().max(1.0);
    let pad_x = (0.5 * span_x).max(64.0);
    let pad_y = (0.5 * span_y).max(64.0);

    let mut lambda = 1e-3f64;
    let mut best_params = params.clone();
    let mut best_residuals = residuals::compute_residuals(&best_params, &used_views, LensModel::Fisheye);
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
            let r_eps = residuals::compute_residuals(&perturbed, &used_views, LensModel::Fisheye);
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

        // Extra guardrails for free principal point: keep it near the observed image point span.
        if !residuals::params_valid(&candidate, LensModel::Fisheye) {
            lambda *= 10.0;
            continue;
        }
        let cx = candidate[2];
        let cy = candidate[3];
        if cx < (min_x - pad_x) || cx > (max_x + pad_x) || cy < (min_y - pad_y) || cy > (max_y + pad_y) {
            lambda *= 10.0;
            continue;
        }

        let candidate_residuals = residuals::compute_residuals(&candidate, &used_views, LensModel::Fisheye);
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

    residuals::solve_state_from_params(&best_params, &used_views, &initial.view_indices, LensModel::Fisheye)
}

fn solve_view_extrinsics_fisheye_with_init(view: &ViewObservations, intrinsics: IntrinsicsSolve, init: Option<Extrinsics>) -> Option<Extrinsics> {
    if view.points.len() < 4 {
        return None;
    }
    let (rvec_init, tvec_init) = match init {
        Some(ext) => (rodrigues::matrix_to_rodrigues(&ext.r), ext.t),
        None => initial_extrinsics_guess(view, intrinsics),
    };
    let params = [rvec_init.x, rvec_init.y, rvec_init.z, tvec_init.x, tvec_init.y, tvec_init.z];
    let dist = DistortionCoefficients::default();

    let mut lambda = 1e-3f64;
    let mut best_params = params;
    let mut best_residuals = compute_residuals_extrinsics(&best_params, view, intrinsics, dist, LensModel::Fisheye);
    let mut best_rms = residuals::residuals_rms(&best_residuals);
    if !best_rms.is_finite() {
        return None;
    }

    for _ in 0..25 {
        let n = best_params.len();
        let m = best_residuals.len();
        if m == 0 {
            break;
        }
        let mut j = DMatrix::<f64>::zeros(m, n);
        for i in 0..n {
            let mut perturbed = best_params;
            // Finite-difference step: too small becomes dominated by float noise and breaks LM.
            let eps = 1e-4 * (best_params[i].abs() + 1.0);
            perturbed[i] += eps;
            let r_eps = compute_residuals_extrinsics(&perturbed, view, intrinsics, dist, LensModel::Fisheye);
            for (row, (r_new, r_old)) in r_eps.iter().zip(best_residuals.iter()).enumerate() {
                j[(row, i)] = (r_new - r_old) / eps;
            }
        }

        let jt = j.transpose();
        let mut jtj = &jt * &j;
        // Levenberg-Marquardt damping: scale by diagonal to handle parameter units gracefully.
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

        let mut candidate = best_params;
        for i in 0..n {
            candidate[i] += delta[i];
        }
        if !params_valid_extrinsics(&candidate) {
            lambda *= 10.0;
            continue;
        }
        let candidate_residuals = compute_residuals_extrinsics(&candidate, view, intrinsics, dist, LensModel::Fisheye);
        let candidate_rms = residuals::residuals_rms(&candidate_residuals);
        if candidate_rms.is_finite() && candidate_rms < best_rms {
            best_rms = candidate_rms;
            best_params = candidate;
            best_residuals = candidate_residuals;
            lambda = (lambda * 0.5).max(1e-6);
        } else {
            lambda = (lambda * 10.0).min(1e6);
        }
        if delta.norm() < 1e-6 {
            break;
        }
    }

    if !params_valid_extrinsics(&best_params) {
        return None;
    }
    let rvec = Vector3::new(best_params[0], best_params[1], best_params[2]);
    let tvec = Vector3::new(best_params[3], best_params[4], best_params[5]);
    let rmat = rodrigues::rodrigues_to_matrix(rvec);
    Some(Extrinsics { r: rmat, t: tvec })
}

fn solve_view_extrinsics_fisheye(view: &ViewObservations, intrinsics: IntrinsicsSolve) -> Option<Extrinsics> {
    let mut candidates: Vec<(f64, Extrinsics)> = Vec::new();
    let dist = DistortionCoefficients::default();
    let init_guess = {
        let (rvec, tvec) = initial_extrinsics_guess(view, intrinsics);
        let rmat = rodrigues::rodrigues_to_matrix(rvec);
        Extrinsics { r: rmat, t: tvec }
    };

    let mut try_init = |init: Option<Extrinsics>| {
        let Some(ext) = solve_view_extrinsics_fisheye_with_init(view, intrinsics, init) else {
            return;
        };
        let rvec = rodrigues::matrix_to_rodrigues(&ext.r);
        let params = [rvec.x, rvec.y, rvec.z, ext.t.x, ext.t.y, ext.t.z];
        let residuals = compute_residuals_extrinsics(&params, view, intrinsics, dist, LensModel::Fisheye);
        let rms = residuals::residuals_rms(&residuals);
        if rms.is_finite() {
            candidates.push((rms, ext));
        }
    };

    let init_h = initial_extrinsics_from_homography_fisheye(view, intrinsics);
    try_init(init_h);
    try_init(Some(init_guess));

    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
    candidates.first().map(|(_, ext)| *ext)
}

fn initial_extrinsics_from_homography_fisheye(view: &ViewObservations, intrinsics: IntrinsicsSolve) -> Option<Extrinsics> {
    if view.points.len() < 4 {
        return None;
    }
    let mut obj = Vec::with_capacity(view.points.len());
    let mut img = Vec::with_capacity(view.points.len());
    for (o, i) in &view.points {
        obj.push(*o);
        let xd = (i.0 - intrinsics.cx) / intrinsics.fx;
        let yd = (i.1 - intrinsics.cy) / intrinsics.fy;
        let (x, y) = projection::undistort_iter(xd, yd, DistortionCoefficients::default(), LensModel::Fisheye, 8);
        img.push((x, y));
    }
    let h = homography::homography_dlt(&obj, &img)?;
    let h1 = h.column(0).into_owned();
    let h2 = h.column(1).into_owned();
    let h3 = h.column(2).into_owned();
    let norm1 = h1.norm();
    let norm2 = h2.norm();
    let norm = 0.5 * (norm1 + norm2);
    if !norm.is_finite() || norm <= 1e-12 {
        return None;
    }
    let scale = 1.0 / norm;
    let r1 = h1 * scale;
    let r2 = h2 * scale;
    let r3 = r1.cross(&r2);
    let r = Matrix3::from_columns(&[r1, r2, r3]);
    let svd_r = r.svd(true, true);
    let u = svd_r.u?;
    let vt = svd_r.v_t?;
    let r_ortho = u * vt;
    let t = h3 * scale;
    Some(Extrinsics { r: r_ortho, t })
}

fn initial_extrinsics_guess(view: &ViewObservations, intrinsics: IntrinsicsSolve) -> (Vector3<f64>, Vector3<f64>) {
    let mut min_ox = f64::INFINITY;
    let mut max_ox = f64::NEG_INFINITY;
    let mut min_oy = f64::INFINITY;
    let mut max_oy = f64::NEG_INFINITY;
    let mut min_ix = f64::INFINITY;
    let mut max_ix = f64::NEG_INFINITY;
    let mut min_iy = f64::INFINITY;
    let mut max_iy = f64::NEG_INFINITY;
    let mut sum_ix = 0.0;
    let mut sum_iy = 0.0;
    let mut count = 0.0;
    for (obj, img) in &view.points {
        min_ox = min_ox.min(obj.0);
        max_ox = max_ox.max(obj.0);
        min_oy = min_oy.min(obj.1);
        max_oy = max_oy.max(obj.1);
        min_ix = min_ix.min(img.0);
        max_ix = max_ix.max(img.0);
        min_iy = min_iy.min(img.1);
        max_iy = max_iy.max(img.1);
        sum_ix += img.0;
        sum_iy += img.1;
        count += 1.0;
    }
    let span_obj = (max_ox - min_ox).abs().max((max_oy - min_oy).abs()).max(1e-6);
    let span_img = (max_ix - min_ix).abs().max((max_iy - min_iy).abs()).max(1e-6);
    let f = intrinsics.fx.max(intrinsics.fy).max(1.0);
    let mut z = f * (span_obj / span_img);
    if !z.is_finite() || z <= 0.0 {
        z = f * 2.0;
    }
    let ix = if count > 0.0 { sum_ix / count } else { intrinsics.cx };
    let iy = if count > 0.0 { sum_iy / count } else { intrinsics.cy };
    let tx = (ix - intrinsics.cx) / intrinsics.fx * z;
    let ty = (iy - intrinsics.cy) / intrinsics.fy * z;
    (Vector3::zeros(), Vector3::new(tx, ty, z))
}

fn params_valid_extrinsics(params: &[f64; 6]) -> bool {
    if !params.iter().all(|v| v.is_finite()) {
        return false;
    }
    let tz = params[5];
    tz.is_finite() && tz > 1e-6
}

fn compute_residuals_extrinsics(params: &[f64; 6], view: &ViewObservations, intrinsics: IntrinsicsSolve, dist: DistortionCoefficients, lens_model: LensModel) -> Vec<f64> {
    let rvec = Vector3::new(params[0], params[1], params[2]);
    let tvec = Vector3::new(params[3], params[4], params[5]);
    let rmat = rodrigues::rodrigues_to_matrix(rvec);
    let fx = intrinsics.fx;
    let fy = intrinsics.fy;
    let cx = intrinsics.cx;
    let cy = intrinsics.cy;
    let mut residuals = Vec::with_capacity(view.points.len() * 2);
    for (obj, img) in &view.points {
        let p = rmat * Vector3::new(obj.0, obj.1, 0.0) + tvec;
        // Penalize invalid/behind-camera projections heavily to keep the optimizer in a sane basin.
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
    residuals
}

fn image_span(views: &[ViewObservations]) -> Option<(f64, f64, f64, f64)> {
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for view in views {
        for (_obj, img) in &view.points {
            min_x = min_x.min(img.0);
            max_x = max_x.max(img.0);
            min_y = min_y.min(img.1);
            max_y = max_y.max(img.1);
        }
    }
    if min_x.is_finite() && max_x.is_finite() && min_y.is_finite() && max_y.is_finite() && max_x > min_x && max_y > min_y { Some((min_x, max_x, min_y, max_y)) } else { None }
}
