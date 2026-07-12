use super::super::*;

use super::geom::quad_perimeter_and_min_edge_sq_f64;

#[node(
    id = "candidate_quads_filter_corner_spacing",
    summary = "Filter quad candidates by minimum corner spacing.",
    inputs(
        port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()),
        port(name = "min_corner_distance_rate", default = 0.05f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01))
    ),
    outputs(port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()))
)]
pub(super) fn cv_aruco_candidate_quads_filter_corner_spacing(quads: &Vec<Quad>, min_corner_distance_rate: f64) -> Result<Vec<Quad>, NodeError> {
    if quads.is_empty() {
        return Ok(Vec::new());
    }
    let min_corner_distance_rate = min_corner_distance_rate.max(0.0);
    if min_corner_distance_rate <= 0.0 {
        return Ok(quads.clone());
    }

    let output = CANDIDATE_QUAD_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        let filtered_quads = &mut scratch.filtered_quads;
        filtered_quads.clear();

        for quad in quads {
            let (perimeter, min_edge_sq) = quad_perimeter_and_min_edge_sq_f64(quad);
            let min_edge_limit = perimeter * min_corner_distance_rate;
            if min_edge_sq < min_edge_limit * min_edge_limit {
                continue;
            }
            filtered_quads.push(*quad);
        }

        let mut output = Vec::with_capacity(filtered_quads.len());
        std::mem::swap(&mut output, filtered_quads);
        output
    });

    Ok(output)
}
