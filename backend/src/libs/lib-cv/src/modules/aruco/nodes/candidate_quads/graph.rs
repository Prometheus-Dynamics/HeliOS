use super::super::*;

#[allow(clippy::too_many_arguments)]
#[node(
    id = "candidate_quads",
    inputs(
        port(name = "contours", source = "Contours", ty = crate::daedalus_types::contours()),
        port(name = "downscale", default = 1i64, meta(ui_min = 1, ui_max = 8, ui_step = 1)),
        port(name = "epsilon", default = 0.03f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01)),
        port(name = "min_area", default = 0.0f64, meta(ui_min = 0.0, ui_max = 1000000.0, ui_step = 1000.0)),
        port(name = "max_area", default = 0.0f64, meta(ui_min = 0.0, ui_max = 1000000.0, ui_step = 1000.0)),
        port(name = "min_angle_deg", default = 0.0f64, meta(ui_min = 0.0, ui_max = 180.0, ui_step = 1.0)),
        port(name = "max_angle_deg", default = 180.0f64, meta(ui_min = 0.0, ui_max = 180.0, ui_step = 1.0)),
        port(name = "max_side_cv", default = 10.0f64, meta(ui_min = 0.1, ui_max = 10.0, ui_step = 0.1)),
        port(name = "min_perimeter_rate", default = 0.03f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01)),
        port(name = "max_perimeter_rate", default = 4.0f64, meta(ui_min = 0.0, ui_max = 10.0, ui_step = 0.1)),
        port(name = "max_side_ratio", default = 0.0f64, meta(ui_min = 0.0, ui_max = 10.0, ui_step = 0.1)),
        port(name = "max_diag_ratio", default = 0.0f64, meta(ui_min = 0.0, ui_max = 10.0, ui_step = 0.1)),
        port(name = "min_corner_distance_rate", default = 0.05f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01)),
        port(name = "min_distance_to_border_px", default = 3.0f64, meta(ui_min = 0.0, ui_max = 64.0, ui_step = 1.0)),
        port(name = "min_marker_distance_rate", default = 0.125f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01)),
        port(name = "mode", default = "robust"),
        port(name = "refine_corners", default = false),
        port(name = "refine_edge_dist", default = 1.5f64, meta(ui_min = 0.0, ui_max = 10.0, ui_step = 0.1)),
        port(name = "refine_min_points", default = 8i64, meta(ui_min = 0, ui_max = 50, ui_step = 1))
    ),
    outputs(port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()))
)]
pub(super) fn cv_aruco_candidate_quads(
    g: &mut GraphCtx,
    contours: Vec<Vec<Point>>,
    downscale: i64,
    epsilon: f64,
    min_area: f64,
    max_area: f64,
    min_angle_deg: f64,
    max_angle_deg: f64,
    max_side_cv: f64,
    min_perimeter_rate: f64,
    max_perimeter_rate: f64,
    max_side_ratio: f64,
    max_diag_ratio: f64,
    min_corner_distance_rate: f64,
    min_distance_to_border_px: f64,
    min_marker_distance_rate: f64,
    mode: crate::modules::aruco::ArucoCandidateQuadsMode,
    refine_corners: bool,
    refine_edge_dist: f64,
    refine_min_points: i64,
) -> Vec<Quad> {
    let extract = g.node_as("cv:aruco:candidate_quads_extract", "extract");
    let group = g.node_as("cv:aruco:candidate_quads_group", "group");

    g.connect(&contours, &extract.input("contours"));
    g.connect(&downscale, &extract.input("downscale"));
    g.connect(&epsilon, &extract.input("epsilon"));
    g.connect(&min_area, &extract.input("min_area"));
    g.connect(&max_area, &extract.input("max_area"));
    g.connect(&min_angle_deg, &extract.input("min_angle_deg"));
    g.connect(&max_angle_deg, &extract.input("max_angle_deg"));
    g.connect(&max_side_cv, &extract.input("max_side_cv"));
    g.connect(&max_side_ratio, &extract.input("max_side_ratio"));
    g.connect(&max_diag_ratio, &extract.input("max_diag_ratio"));
    g.connect(&min_perimeter_rate, &extract.input("min_perimeter_rate"));
    g.connect(&min_corner_distance_rate, &extract.input("min_corner_distance_rate"));
    g.connect(&mode, &extract.input("mode"));
    g.connect(&refine_corners, &extract.input("refine_corners"));
    g.connect(&refine_edge_dist, &extract.input("refine_edge_dist"));
    g.connect(&refine_min_points, &extract.input("refine_min_points"));

    g.connect(&contours, &group.input("contours"));
    g.connect(&downscale, &group.input("downscale"));
    g.connect(&min_distance_to_border_px, &group.input("min_distance_to_border_px"));
    g.connect(&min_marker_distance_rate, &group.input("min_marker_distance_rate"));
    g.connect(&extract.output("quads"), &group.input("quads"));

    // `max_perimeter_rate` is preserved in the node signature for graph compatibility, but the
    // filtering now runs in a single pass inside `candidate_quads_extract`.
    let _ = &max_perimeter_rate;

    group.output("quads")
}
