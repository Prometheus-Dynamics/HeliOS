use super::super::*;

use super::geom::{quad_from_cv, quad_to_cv};

#[node(
    id = "candidate_quads_filter_geometry",
    summary = "Filter quad candidates by geometry constraints.",
    inputs(
        port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()),
        port(name = "min_angle_deg", default = 0.0f64, meta(ui_min = 0.0, ui_max = 180.0, ui_step = 1.0)),
        port(name = "max_angle_deg", default = 180.0f64, meta(ui_min = 0.0, ui_max = 180.0, ui_step = 1.0)),
        port(name = "max_side_cv", default = 10.0f64, meta(ui_min = 0.1, ui_max = 10.0, ui_step = 0.1)),
        port(name = "max_side_ratio", default = 0.0f64, meta(ui_min = 0.0, ui_max = 10.0, ui_step = 0.1)),
        port(name = "max_diag_ratio", default = 0.0f64, meta(ui_min = 0.0, ui_max = 10.0, ui_step = 0.1))
    ),
    outputs(port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()))
)]
pub(super) fn cv_aruco_candidate_quads_filter_geometry(
    quads: &Vec<Quad>,
    min_angle_deg: f64,
    max_angle_deg: f64,
    max_side_cv: f64,
    max_side_ratio: f64,
    max_diag_ratio: f64,
) -> Result<Vec<Quad>, NodeError> {
    if quads.is_empty() {
        return Ok(Vec::new());
    }
    let detect_cfg = ArucoTagDetectorConfig {
        epsilon: 0.0,
        min_area: 0.0,
        max_area: None,
        min_angle_deg: min_angle_deg.clamp(0.0, 180.0) as f32,
        max_angle_deg: max_angle_deg.clamp(0.0, 180.0) as f32,
        max_side_cv: max_side_cv.clamp(0.05, 10.0) as f32,
        max_side_ratio: max_side_ratio.max(0.0) as f32,
        max_diag_ratio: max_diag_ratio.max(0.0) as f32,
        angle_cos_min: -1.0,
        angle_cos_max: 1.0,
    }
    .with_angle_cos_bounds();

    let output = CANDIDATE_QUAD_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        scratch.filtered.clear();

        for quad in quads {
            let quad_cv = quad_to_cv(quad);
            if quad_satisfies_config(&quad_cv, &detect_cfg) {
                scratch.filtered.push(quad_cv);
            }
        }

        let mut output = Vec::with_capacity(scratch.filtered.len());
        for quad in scratch.filtered.iter() {
            output.push(quad_from_cv(quad));
        }
        output
    });

    Ok(output)
}
