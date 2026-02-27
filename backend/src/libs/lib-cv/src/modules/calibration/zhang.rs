use super::distortion;
use super::homography;
use super::intrinsics;
use super::projection;
use super::types::*;
use nalgebra::{DMatrix, Matrix3};

pub(super) fn solve_zhang(views: &[ViewObservations], intrinsics_hint: Option<IntrinsicsSolve>, lens_model: LensModel) -> Result<SolveState, CalibrationSolveError> {
    let mut homographies: Vec<(Matrix3<f64>, &ViewObservations)> = Vec::new();
    let mut warnings = Vec::new();
    for view in views {
        let (obj, img): (Points2, Points2) = view.points.iter().map(|(o, i)| (*o, *i)).unzip();
        let Some(h) = homography::homography_dlt(&obj, &img) else {
            warnings.push(format!("view {}: homography solve failed", view.index));
            continue;
        };
        homographies.push((h, view));
    }
    if homographies.len() < 2 {
        return Err(CalibrationSolveError::DegenerateHomography);
    }

    let v_rows = 2 * homographies.len();
    let mut v = DMatrix::<f64>::zeros(v_rows, 6);
    for (idx, (h, _view)) in homographies.iter().enumerate() {
        let v12 = homography::v_ij(h, 0, 1);
        let v11 = homography::v_ij(h, 0, 0);
        let v22 = homography::v_ij(h, 1, 1);
        let r0 = 2 * idx;
        let r1 = r0 + 1;
        for c in 0..6 {
            v[(r0, c)] = v12[c];
            v[(r1, c)] = v11[c] - v22[c];
        }
    }

    let svd = v.clone().svd(true, true);
    let vt = svd.v_t.ok_or(CalibrationSolveError::IntrinsicsSolveFailed)?;
    let b = vt.row(vt.nrows() - 1).transpose();
    if b.nrows() != 6 {
        return Err(CalibrationSolveError::IntrinsicsSolveFailed);
    }
    let bvec: [f64; 6] = [b[0], b[1], b[2], b[3], b[4], b[5]];
    let (mut intrinsics, intrinsics_warning) = intrinsics::solve_intrinsics_from_b(bvec)?;
    if let Some(warning) = intrinsics_warning {
        warnings.push(warning);
    }
    // A warning indicates we used a fallback intrinsics reconstruction method, but that does not
    // necessarily mean the intrinsics are unusable. Decide degeneracy from the numeric values.
    let mut intrinsics_degenerate = !(intrinsics.fx.is_finite() && intrinsics.fy.is_finite() && intrinsics.cx.is_finite() && intrinsics.cy.is_finite() && intrinsics.fx > 0.0 && intrinsics.fy > 0.0);
    if intrinsics.skew.abs() > intrinsics.fx.max(intrinsics.fy) * 0.01 {
        warnings.push("intrinsics solve: large skew detected; results may be unstable".into());
    }

    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (_h, view) in &homographies {
        for (_obj, img) in &view.points {
            min_x = min_x.min(img.0);
            max_x = max_x.max(img.0);
            min_y = min_y.min(img.1);
            max_y = max_y.max(img.1);
        }
    }
    let image_span_ok = min_x.is_finite() && max_x.is_finite() && min_y.is_finite() && max_y.is_finite() && max_x > min_x && max_y > min_y;
    let cx_out_of_bounds = image_span_ok && (intrinsics.cx < min_x - (max_x - min_x) || intrinsics.cx > max_x + (max_x - min_x));
    let cy_out_of_bounds = image_span_ok && (intrinsics.cy < min_y - (max_y - min_y) || intrinsics.cy > max_y + (max_y - min_y));
    let skew_too_large = intrinsics.skew.abs() > intrinsics.fx.max(intrinsics.fy) * 0.05;

    let mut used_hint = false;
    if intrinsics_degenerate || skew_too_large || cx_out_of_bounds || cy_out_of_bounds {
        if let Some(hint) = intrinsics_hint {
            intrinsics = IntrinsicsSolve { fx: hint.fx, fy: hint.fy, skew: 0.0, cx: hint.cx, cy: hint.cy };
            intrinsics_degenerate = false;
            used_hint = true;
            warnings.push("intrinsics solve: used provided intrinsics hint".into());
        } else {
            let cx_guess = if image_span_ok { 0.5 * (min_x + max_x) } else { intrinsics.cx };
            let cy_guess = if image_span_ok { 0.5 * (min_y + max_y) } else { intrinsics.cy };
            if let Some(fallback) = intrinsics::solve_intrinsics_from_homographies(&homographies, cx_guess, cy_guess) {
                intrinsics = fallback;
                intrinsics_degenerate = false;
                warnings.push("intrinsics solve: used homography median with fixed principal point".into());
            } else if let Some((fallback, warning)) = intrinsics::solve_intrinsics_zero_skew(&v) {
                intrinsics = fallback;
                intrinsics_degenerate = true;
                if let Some(warning) = warning {
                    warnings.push(warning);
                }
            }
        }
    }

    if image_span_ok && !used_hint {
        let span_w = (max_x - min_x).abs();
        let span_h = (max_y - min_y).abs();
        let min_span = span_w.min(span_h).max(1.0);
        let max_span = span_w.max(span_h).max(1.0);
        let min_fx = min_span * 0.2;
        let max_fx = max_span * 50.0;
        let fx_bad = !intrinsics.fx.is_finite() || intrinsics.fx < min_fx || intrinsics.fx > max_fx;
        let fy_bad = !intrinsics.fy.is_finite() || intrinsics.fy < min_fx || intrinsics.fy > max_fx;
        if fx_bad || fy_bad {
            let cx_guess = 0.5 * (min_x + max_x);
            let cy_guess = 0.5 * (min_y + max_y);
            let f_guess = (max_span * 2.0).max(1.0);
            intrinsics = IntrinsicsSolve { fx: f_guess, fy: f_guess, skew: 0.0, cx: cx_guess, cy: cy_guess };
            intrinsics_degenerate = true;
            warnings.push("intrinsics solve: heuristic fallback based on image span".into());
        }
    }

    let k = Matrix3::new(intrinsics.fx, intrinsics.skew, intrinsics.cx, 0.0, intrinsics.fy, intrinsics.cy, 0.0, 0.0, 1.0);
    let inv_k = k.try_inverse().ok_or(CalibrationSolveError::IntrinsicsSolveFailed)?;

    let mut extrinsics: Vec<Extrinsics> = Vec::new();
    let mut view_indices: Vec<usize> = Vec::new();
    for (h, view) in &homographies {
        let h1 = h.column(0).into_owned();
        let h2 = h.column(1).into_owned();
        let h3 = h.column(2).into_owned();
        let invh1 = inv_k * h1;
        let invh2 = inv_k * h2;
        let norm1 = invh1.norm();
        let norm2 = invh2.norm();
        let norm = 0.5 * (norm1 + norm2);
        if !norm.is_finite() || norm <= 1e-12 {
            continue;
        }
        let scale = 1.0 / norm;
        let r1 = invh1 * scale;
        let r2 = invh2 * scale;
        let r3 = r1.cross(&r2);
        let r = Matrix3::from_columns(&[r1, r2, r3]);
        let svd_r = r.svd(true, true);
        let u = svd_r.u.ok_or(CalibrationSolveError::ExtrinsicsSolveFailed)?;
        let vt = svd_r.v_t.ok_or(CalibrationSolveError::ExtrinsicsSolveFailed)?;
        let r_ortho = u * vt;
        let t = inv_k * h3 * scale;
        extrinsics.push(Extrinsics { r: r_ortho, t });
        view_indices.push(view.index);
    }

    if extrinsics.len() < 2 {
        return Err(CalibrationSolveError::ExtrinsicsSolveFailed);
    }

    let used_views: Vec<ViewObservations> = view_indices.iter().filter_map(|idx| views.iter().find(|view| view.index == *idx).cloned()).collect();

    let mut distortion = if intrinsics_degenerate {
        DistortionCoefficients::default()
    } else if lens_model == LensModel::Pinhole {
        let (k1, k2, k3, p1, p2) = distortion::estimate_distortion(&used_views, &extrinsics, intrinsics);
        DistortionCoefficients { k1, k2, k3, p1, p2 }
    } else {
        DistortionCoefficients::default()
    };
    if !distortion::distortion_is_reasonable(distortion, lens_model) {
        warnings.push("distortion solve unstable; using zero distortion".into());
        distortion = DistortionCoefficients::default();
    }
    let reproj = projection::reprojection_error(&used_views, &extrinsics, intrinsics, distortion, lens_model);

    let points_used = used_views.iter().map(|view| view.points.len()).sum();
    Ok(SolveState { intrinsics, distortion, reprojection_error_px: reproj, warnings, views_used: extrinsics.len(), points_used, intrinsics_degenerate, extrinsics, view_indices, lens_model })
}

pub(super) fn solve_fixed_intrinsics_extrinsics(views: &[ViewObservations], intrinsics: IntrinsicsSolve) -> Result<ExtrinsicsSolveResult, CalibrationSolveError> {
    let mut homographies: Vec<(Matrix3<f64>, &ViewObservations)> = Vec::new();
    let mut warnings = Vec::new();
    for view in views {
        let (obj, img): (Points2, Points2) = view.points.iter().map(|(o, i)| (*o, *i)).unzip();
        let Some(h) = homography::homography_dlt(&obj, &img) else {
            warnings.push(format!("view {}: homography solve failed", view.index));
            continue;
        };
        homographies.push((h, view));
    }
    if homographies.len() < 2 {
        return Err(CalibrationSolveError::DegenerateHomography);
    }

    let k = Matrix3::new(intrinsics.fx, intrinsics.skew, intrinsics.cx, 0.0, intrinsics.fy, intrinsics.cy, 0.0, 0.0, 1.0);
    let inv_k = k.try_inverse().ok_or(CalibrationSolveError::IntrinsicsSolveFailed)?;

    let mut extrinsics: Vec<Extrinsics> = Vec::new();
    let mut view_indices: Vec<usize> = Vec::new();
    for (h, view) in &homographies {
        let h1 = h.column(0).into_owned();
        let h2 = h.column(1).into_owned();
        let h3 = h.column(2).into_owned();
        let invh1 = inv_k * h1;
        let invh2 = inv_k * h2;
        let norm1 = invh1.norm();
        let norm2 = invh2.norm();
        let norm = 0.5 * (norm1 + norm2);
        if !norm.is_finite() || norm <= 1e-12 {
            continue;
        }
        let scale = 1.0 / norm;
        let r1 = invh1 * scale;
        let r2 = invh2 * scale;
        let r3 = r1.cross(&r2);
        let r = Matrix3::from_columns(&[r1, r2, r3]);
        let svd_r = r.svd(true, true);
        let u = svd_r.u.ok_or(CalibrationSolveError::ExtrinsicsSolveFailed)?;
        let vt = svd_r.v_t.ok_or(CalibrationSolveError::ExtrinsicsSolveFailed)?;
        let r_ortho = u * vt;
        let t = inv_k * h3 * scale;
        extrinsics.push(Extrinsics { r: r_ortho, t });
        view_indices.push(view.index);
    }

    if extrinsics.len() < 2 {
        return Err(CalibrationSolveError::ExtrinsicsSolveFailed);
    }

    Ok((extrinsics, view_indices, warnings))
}

pub(super) fn solve_fixed_intrinsics(views: &[ViewObservations], intrinsics: IntrinsicsSolve, lens_model: LensModel) -> Result<SolveState, CalibrationSolveError> {
    let (extrinsics, view_indices, mut warnings) = solve_fixed_intrinsics_extrinsics(views, intrinsics)?;

    let used_views: Vec<ViewObservations> = view_indices.iter().filter_map(|idx| views.iter().find(|view| view.index == *idx).cloned()).collect();
    let mut distortion = if lens_model == LensModel::Pinhole {
        let (k1, k2, k3, p1, p2) = distortion::estimate_distortion(&used_views, &extrinsics, intrinsics);
        DistortionCoefficients { k1, k2, k3, p1, p2 }
    } else {
        DistortionCoefficients::default()
    };
    if !distortion::distortion_is_reasonable(distortion, lens_model) {
        warnings.push("distortion solve unstable; using zero distortion".into());
        distortion = DistortionCoefficients::default();
    }
    let reproj = projection::reprojection_error(&used_views, &extrinsics, intrinsics, distortion, lens_model);
    let points_used = used_views.iter().map(|view| view.points.len()).sum();
    Ok(SolveState { intrinsics, distortion, reprojection_error_px: reproj, warnings, views_used: extrinsics.len(), points_used, intrinsics_degenerate: false, extrinsics, view_indices, lens_model })
}
