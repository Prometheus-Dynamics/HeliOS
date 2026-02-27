use crate::modules::localization::CameraIntrinsics;
use std::time::Instant;

use super::bundle;
use super::fisheye;
use super::projection;
use super::types::*;
use super::zhang;

pub fn solve_camera_intrinsics(views: &[CalibrationView], config: CalibrationSolveConfig) -> Result<CalibrationSolveResult, CalibrationSolveError> {
    solve_camera_intrinsics_internal(views, config, None)
}

pub fn solve_camera_intrinsics_with_hint(views: &[CalibrationView], config: CalibrationSolveConfig, hint: Option<CameraIntrinsics>) -> Result<CalibrationSolveResult, CalibrationSolveError> {
    solve_camera_intrinsics_internal(views, config, hint)
}

pub fn solve_fixed_intrinsics_bundle_adjustment(views: &[CalibrationView], config: CalibrationSolveConfig, intrinsics: CameraIntrinsics) -> Result<CalibrationSolveResult, CalibrationSolveError> {
    if views.is_empty() {
        return Err(CalibrationSolveError::InsufficientViews { required: config.min_views.max(1), provided: 0 });
    }

    let mut warnings = Vec::new();
    let mut cleaned: Vec<ViewObservations> = Vec::new();
    let mut max_points = 0usize;
    for (index, view) in views.iter().enumerate() {
        if view.is_empty() {
            warnings.push(format!("view {index}: no points"));
            continue;
        }
        let mut points = Vec::with_capacity(view.points.len());
        for pair in &view.points {
            let obj = pair.object;
            let img = pair.image;
            if obj.x.is_finite() && obj.y.is_finite() && img.x.is_finite() && img.y.is_finite() {
                points.push(((obj.x, obj.y), (img.x, img.y)));
            }
        }
        max_points = max_points.max(points.len());
        if points.len() < config.min_points_per_view {
            warnings.push(format!("view {index}: insufficient points ({} < {})", points.len(), config.min_points_per_view));
            continue;
        }
        cleaned.push(ViewObservations { index, points });
    }

    if cleaned.is_empty() {
        return Err(CalibrationSolveError::InsufficientPoints { required: config.min_points_per_view, provided: max_points });
    }

    if cleaned.len() < config.min_views {
        return Err(CalibrationSolveError::InsufficientViews { required: config.min_views, provided: cleaned.len() });
    }

    let lens_model = config.lens_model;
    let intrinsics = IntrinsicsSolve { fx: intrinsics.fx, fy: intrinsics.fy, skew: 0.0, cx: intrinsics.cx, cy: intrinsics.cy };
    let (extrinsics, view_indices, mut extrinsic_warnings) = zhang::solve_fixed_intrinsics_extrinsics(&cleaned, intrinsics)?;
    warnings.append(&mut extrinsic_warnings);

    let used_views: Vec<ViewObservations> = view_indices.iter().filter_map(|idx| cleaned.iter().find(|view| view.index == *idx).cloned()).collect();
    let reproj = projection::reprojection_error(&used_views, &extrinsics, intrinsics, DistortionCoefficients::default(), lens_model);
    let points_used = used_views.iter().map(|view| view.points.len()).sum();

    let mut state = SolveState {
        intrinsics,
        distortion: DistortionCoefficients::default(),
        reprojection_error_px: reproj,
        warnings,
        views_used: extrinsics.len(),
        points_used,
        intrinsics_degenerate: false,
        extrinsics,
        view_indices,
        lens_model,
    };

    if config.refine_distortion {
        match bundle::refine_solution_bundle_adjustment_fixed_intrinsics(&cleaned, &state, config.refine_undistort_iters) {
            Ok(mut refined) => {
                state.warnings.append(&mut refined.warnings);
                state = refined;
            }
            Err(_) => {
                state.warnings.push("fixed-intrinsics bundle adjustment failed; using initial solution".into());
            }
        }
    }

    let intrinsics_out = CameraIntrinsics::new(state.intrinsics.fx, state.intrinsics.fy, state.intrinsics.cx, state.intrinsics.cy);
    let calibration = CameraCalibration::new(intrinsics_out, state.distortion, config.undistort_iters, lens_model);
    Ok(CalibrationSolveResult { calibration, reprojection_error_px: state.reprojection_error_px, views_used: state.views_used, points_used: state.points_used, warnings: state.warnings })
}

fn solve_camera_intrinsics_internal(views: &[CalibrationView], config: CalibrationSolveConfig, hint: Option<CameraIntrinsics>) -> Result<CalibrationSolveResult, CalibrationSolveError> {
    if views.is_empty() {
        return Err(CalibrationSolveError::InsufficientViews { required: config.min_views.max(1), provided: 0 });
    }

    let mut warnings = Vec::new();
    let mut cleaned: Vec<ViewObservations> = Vec::new();
    let mut max_points = 0usize;
    for (index, view) in views.iter().enumerate() {
        if view.is_empty() {
            warnings.push(format!("view {index}: no points"));
            continue;
        }
        let mut points = Vec::with_capacity(view.points.len());
        for pair in &view.points {
            let obj = pair.object;
            let img = pair.image;
            if obj.x.is_finite() && obj.y.is_finite() && img.x.is_finite() && img.y.is_finite() {
                points.push(((obj.x, obj.y), (img.x, img.y)));
            }
        }
        max_points = max_points.max(points.len());
        if points.len() < config.min_points_per_view {
            warnings.push(format!("view {index}: insufficient points ({} < {})", points.len(), config.min_points_per_view));
            continue;
        }
        cleaned.push(ViewObservations { index, points });
    }

    if cleaned.is_empty() {
        return Err(CalibrationSolveError::InsufficientPoints { required: config.min_points_per_view, provided: max_points });
    }

    if cleaned.len() < config.min_views {
        return Err(CalibrationSolveError::InsufficientViews { required: config.min_views, provided: cleaned.len() });
    }

    let lens_model = config.lens_model;
    let hint = hint.map(|intr| IntrinsicsSolve { fx: intr.fx, fy: intr.fy, skew: 0.0, cx: intr.cx, cy: intr.cy });
    if lens_model == LensModel::Fisheye {
        let mut state = fisheye::solve_fisheye_multi_start(&cleaned, config, hint)?;
        warnings.append(&mut state.warnings);
        let intrinsics = CameraIntrinsics::new(state.intrinsics.fx, state.intrinsics.fy, state.intrinsics.cx, state.intrinsics.cy);
        let calibration = CameraCalibration::new(intrinsics, state.distortion, config.undistort_iters, lens_model);
        let reprojection_error_px = if state.reprojection_error_px.is_finite() {
            state.reprojection_error_px
        } else {
            warnings.push("reprojection error was non-finite; set to 0".into());
            0.0
        };
        return Ok(CalibrationSolveResult { calibration, reprojection_error_px, views_used: state.views_used, points_used: state.points_used, warnings });
    }
    let mut skip_refinement = false;
    let total_points: usize = cleaned.iter().map(|view| view.points.len()).sum();
    let can_refine = cleaned.len() >= 3 && total_points >= 50;
    let zhang_start = Instant::now();
    let mut state = match zhang::solve_zhang(&cleaned, hint, lens_model) {
        Ok(state) => state,
        Err(err) => {
            if let Some(hint) = hint
                && let Ok(fallback) = zhang::solve_fixed_intrinsics(&cleaned, hint, lens_model)
            {
                return Ok(CalibrationSolveResult {
                    calibration: CameraCalibration::new(
                        CameraIntrinsics::new(fallback.intrinsics.fx, fallback.intrinsics.fy, fallback.intrinsics.cx, fallback.intrinsics.cy),
                        fallback.distortion,
                        config.undistort_iters,
                        lens_model,
                    ),
                    reprojection_error_px: fallback.reprojection_error_px,
                    views_used: fallback.views_used,
                    points_used: fallback.points_used,
                    warnings: fallback.warnings,
                });
            }
            return Err(err);
        }
    };
    tracing::info!(views = cleaned.len(), points = total_points, elapsed_ms = zhang_start.elapsed().as_millis(), "calibration: zhang solve complete");
    warnings.append(&mut state.warnings);

    let err_threshold = 500.0;
    if let Some(hint) = hint
        && (!state.reprojection_error_px.is_finite() || state.reprojection_error_px > err_threshold || state.intrinsics_degenerate)
        && let Ok(fallback) = zhang::solve_fixed_intrinsics(&cleaned, hint, lens_model)
    {
        warnings.push(format!("intrinsics solve: fixed-intrinsics fallback used (reproj {:.2} px)", fallback.reprojection_error_px));
        state = fallback;
        skip_refinement = true;
    }

    if config.refine_distortion && skip_refinement && can_refine {
        let refine_start = Instant::now();
        match bundle::refine_solution_bundle_adjustment_fixed_intrinsics(&cleaned, &state, config.refine_undistort_iters) {
            Ok(mut refined) => {
                warnings.append(&mut refined.warnings);
                state = refined;
            }
            Err(_) => {
                warnings.push("fixed-intrinsics bundle adjustment failed; using initial solution".into());
            }
        }
        tracing::info!(elapsed_ms = refine_start.elapsed().as_millis(), "calibration: fixed-intrinsics bundle adjustment complete");
    } else if config.refine_distortion && !state.intrinsics_degenerate && can_refine {
        let refine_start = Instant::now();
        match bundle::refine_solution_with_distortion(&cleaned, &state, config.refine_undistort_iters) {
            Ok(mut refined) => {
                warnings.append(&mut refined.warnings);
                state = refined;
            }
            Err(_) => {
                warnings.push("distortion refinement failed; using initial solution".into());
            }
        }
        tracing::info!(elapsed_ms = refine_start.elapsed().as_millis(), "calibration: distortion refinement complete");
    } else if config.refine_distortion && state.intrinsics_degenerate {
        warnings.push("distortion refinement skipped (degenerate intrinsics); using zero distortion".into());
    } else if config.refine_distortion && !can_refine {
        warnings.push("distortion refinement skipped (insufficient points)".into());
    }

    if config.refine_distortion && !state.intrinsics_degenerate && !skip_refinement && can_refine {
        let bundle_start = Instant::now();
        match bundle::refine_solution_bundle_adjustment(&cleaned, &state, config.refine_undistort_iters) {
            Ok(mut refined) => {
                warnings.append(&mut refined.warnings);
                state = refined;
            }
            Err(_) => {
                warnings.push("bundle adjustment failed; using previous solution".into());
            }
        }
        tracing::info!(elapsed_ms = bundle_start.elapsed().as_millis(), "calibration: bundle adjustment complete");
    }

    let intrinsics = CameraIntrinsics::new(state.intrinsics.fx, state.intrinsics.fy, state.intrinsics.cx, state.intrinsics.cy);
    let calibration = CameraCalibration::new(intrinsics, state.distortion, config.undistort_iters, lens_model);
    let reprojection_error_px = if state.reprojection_error_px.is_finite() {
        state.reprojection_error_px
    } else {
        warnings.push("reprojection error was non-finite; set to 0".into());
        0.0
    };
    Ok(CalibrationSolveResult { calibration, reprojection_error_px, views_used: state.views_used, points_used: state.points_used, warnings })
}
