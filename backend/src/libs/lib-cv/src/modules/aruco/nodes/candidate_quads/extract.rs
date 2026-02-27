use super::super::*;

use super::geom::{fill_contour_points_f32, quad_from_cv, quad_perimeter_and_min_edge_sq, refine_quad_corners_from_contour};

#[node(
    id = "candidate_quads_extract",
    summary = "Approximate quad candidates from contours.",
    inputs(
        port(name = "contours", source = "Contours", ty = crate::daedalus_types::contours()),
        port(name = "downscale", default = 1i64, meta(ui_min = 1, ui_max = 8, ui_step = 1)),
        port(name = "epsilon", default = 0.03f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01)),
        port(name = "min_area", default = 0.0f64, meta(ui_min = 0.0, ui_max = 1000000.0, ui_step = 1000.0)),
        port(name = "max_area", default = 0.0f64, meta(ui_min = 0.0, ui_max = 1000000.0, ui_step = 1000.0)),
        port(name = "min_angle_deg", default = 0.0f64, meta(ui_min = 0.0, ui_max = 180.0, ui_step = 1.0)),
        port(name = "max_angle_deg", default = 180.0f64, meta(ui_min = 0.0, ui_max = 180.0, ui_step = 1.0)),
        port(name = "max_side_cv", default = 10.0f64, meta(ui_min = 0.1, ui_max = 10.0, ui_step = 0.1)),
        port(name = "max_side_ratio", default = 0.0f64, meta(ui_min = 0.0, ui_max = 10.0, ui_step = 0.1)),
        port(name = "max_diag_ratio", default = 0.0f64, meta(ui_min = 0.0, ui_max = 10.0, ui_step = 0.1)),
        port(name = "min_perimeter_rate", default = 0.03f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01)),
        port(name = "min_corner_distance_rate", default = 0.05f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01)),
        port(name = "mode", default = "robust"),
        port(name = "refine_corners", default = false),
        port(name = "refine_edge_dist", default = 1.5f64, meta(ui_min = 0.0, ui_max = 10.0, ui_step = 0.1)),
        port(name = "refine_min_points", default = 8i64, meta(ui_min = 0, ui_max = 50, ui_step = 1))
    ),
    outputs(port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()))
)]
#[allow(clippy::too_many_arguments)]
pub(super) fn cv_aruco_candidate_quads_extract(
    contours: &Vec<Vec<Point>>,
    downscale: i64,
    epsilon: f64,
    min_area: f64,
    max_area: f64,
    min_angle_deg: f64,
    max_angle_deg: f64,
    max_side_cv: f64,
    max_side_ratio: f64,
    max_diag_ratio: f64,
    min_perimeter_rate: f64,
    min_corner_distance_rate: f64,
    mode: crate::modules::aruco::ArucoCandidateQuadsMode,
    refine_corners: bool,
    refine_edge_dist: f64,
    refine_min_points: i64,
) -> Result<Vec<Quad>, NodeError> {
    let downscale = u32::try_from(downscale).unwrap_or(1).max(1);
    let scale = downscale as f32;
    let area_scale = (scale * scale).max(1.0);
    let min_area = (min_area.max(0.0) as f32) / area_scale;
    let max_area = if max_area > 0.0 { Some((max_area as f32) / area_scale) } else { None };

    let detect_cfg = ArucoTagDetectorConfig {
        epsilon: epsilon.clamp(0.01, 1000.0) as f32,
        min_area,
        max_area,
        min_angle_deg: min_angle_deg.clamp(0.0, 180.0) as f32,
        max_angle_deg: max_angle_deg.clamp(0.0, 180.0) as f32,
        max_side_cv: max_side_cv.clamp(0.05, 10.0) as f32,
        max_side_ratio: max_side_ratio.max(0.0) as f32,
        max_diag_ratio: max_diag_ratio.max(0.0) as f32,
        angle_cos_min: -1.0,
        angle_cos_max: 1.0,
    }
    .with_angle_cos_bounds();
    let fast_mode = matches!(mode, crate::modules::aruco::ArucoCandidateQuadsMode::Fast);
    let refine_dist = refine_edge_dist.max(0.25) as f32;
    let refine_min_points = refine_min_points.max(4) as usize;
    let min_perimeter_rate = min_perimeter_rate.max(0.0) as f32;
    let min_corner_distance_rate = min_corner_distance_rate.max(0.0) as f32;
    let min_perimeter = if min_area > f32::EPSILON { Some((4.0 * std::f32::consts::PI * min_area).sqrt()) } else { None };
    let max_step = std::f32::consts::SQRT_2;

    if contours.is_empty() {
        return Ok(Vec::new());
    }

    let min_perimeter_rate_px = if min_perimeter_rate > 0.0 {
        let mut max_x = 0.0f32;
        let mut max_y = 0.0f32;
        for contour in contours {
            for p in contour {
                max_x = max_x.max(p.x as f32);
                max_y = max_y.max(p.y as f32);
            }
        }
        let max_dim = (max_x.max(max_y) + 1.0).max(0.0);
        if max_dim > 0.0 { Some(min_perimeter_rate * max_dim) } else { None }
    } else {
        None
    };

    let use_parallel = contours.len() >= 128 && rayon::current_num_threads() > 1;
    if use_parallel {
        #[derive(Default)]
        struct LocalScratch {
            pts: Vec<CvPoint<f32>>,
        }

        let quads: Vec<Quad> = contours
            .par_iter()
            .map_init(
                || LocalScratch { pts: Vec::with_capacity(64) },
                |scratch, contour| {
                    if contour.len() < 4 {
                        return None;
                    }

                    let (perimeter, min_x, max_x, min_y, max_y) = fill_contour_points_f32(contour, &mut scratch.pts);

                    let width = max_x - min_x;
                    let height = max_y - min_y;
                    if width <= 0.0 || height <= 0.0 {
                        return None;
                    }
                    if min_area > f32::EPSILON && (width * height) < min_area {
                        return None;
                    }
                    if let Some(min_rate_px) = min_perimeter_rate_px
                        && perimeter < min_rate_px
                    {
                        return None;
                    }
                    if let Some(min_perimeter) = min_perimeter
                        && perimeter * max_step < min_perimeter
                    {
                        return None;
                    }

                    let quad = if fast_mode { candidate_quad_from_contour_fast(&scratch.pts, perimeter, min_perimeter, &detect_cfg) } else { candidate_quad_from_contour(&scratch.pts, &detect_cfg) };
                    let mut quad = quad?;
                    if refine_corners {
                        let _ = refine_quad_corners_from_contour(&scratch.pts, &mut quad, refine_dist, refine_min_points);
                    }
                    if min_corner_distance_rate > 0.0 {
                        let (perimeter, min_edge_sq) = quad_perimeter_and_min_edge_sq(&quad);
                        let min_edge_limit = perimeter * min_corner_distance_rate;
                        if min_edge_sq < min_edge_limit * min_edge_limit {
                            return None;
                        }
                    }
                    Some(quad_from_cv(&quad))
                },
            )
            .filter_map(|quad| quad)
            .collect();

        return Ok(quads);
    }

    let output = CANDIDATE_QUAD_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        scratch.pts.clear();
        let mut output = Vec::with_capacity(contours.len());

        for contour in contours {
            if contour.len() < 4 {
                continue;
            }
            let (perimeter, min_x, max_x, min_y, max_y) = fill_contour_points_f32(contour, &mut scratch.pts);

            let width = max_x - min_x;
            let height = max_y - min_y;
            if width <= 0.0 || height <= 0.0 {
                continue;
            }
            if min_area > f32::EPSILON && (width * height) < min_area {
                continue;
            }
            if let Some(min_rate_px) = min_perimeter_rate_px
                && perimeter < min_rate_px
            {
                continue;
            }
            if let Some(min_perimeter) = min_perimeter
                && perimeter * max_step < min_perimeter
            {
                continue;
            }

            let quad = if fast_mode { candidate_quad_from_contour_fast(&scratch.pts, perimeter, min_perimeter, &detect_cfg) } else { candidate_quad_from_contour(&scratch.pts, &detect_cfg) };
            let Some(mut quad) = quad else {
                continue;
            };
            if refine_corners {
                let _ = refine_quad_corners_from_contour(&scratch.pts, &mut quad, refine_dist, refine_min_points);
            }
            if min_corner_distance_rate > 0.0 {
                let (perimeter, min_edge_sq) = quad_perimeter_and_min_edge_sq(&quad);
                let min_edge_limit = perimeter * min_corner_distance_rate;
                if min_edge_sq < min_edge_limit * min_edge_limit {
                    continue;
                }
            }
            output.push(quad_from_cv(&quad));
        }
        output
    });

    Ok(output)
}
