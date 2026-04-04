use super::*;

fn quad_perimeter_f64(quad: &Quad) -> f64 {
    let mut perim = 0.0f64;
    for i in 0..4usize {
        let a = quad[i];
        let b = quad[(i + 1) % 4];
        let dx = a.x - b.x;
        let dy = a.y - b.y;
        perim += (dx * dx + dy * dy).sqrt();
    }
    perim
}

fn quad_average_corner_distance_f64(a: &Quad, b: &Quad) -> f64 {
    // Find the best rotation alignment between the 4 corners and compute the mean distance.
    let mut min_dist_sq = f64::MAX;
    for fc in 0..4usize {
        let mut dist_sq = 0.0f64;
        for c in 0..4usize {
            let ac = a[(c + fc) % 4];
            let bc = b[c];
            let dx = ac.x - bc.x;
            let dy = ac.y - bc.y;
            dist_sq += dx * dx + dy * dy;
        }
        dist_sq *= 0.25;
        min_dist_sq = min_dist_sq.min(dist_sq);
    }
    min_dist_sq.sqrt()
}

fn merge_quads_inplace(out: &mut Vec<Quad>, candidates: &[Quad], min_distance_rate: f64) {
    for quad in candidates.iter() {
        let perim = quad_perimeter_f64(quad);
        let mut replaced = false;
        for existing in out.iter_mut() {
            let dist = quad_average_corner_distance_f64(existing, quad);
            let min_perim = quad_perimeter_f64(existing).min(perim);
            if dist < min_perim * min_distance_rate {
                if perim > quad_perimeter_f64(existing) {
                    *existing = *quad;
                }
                replaced = true;
                break;
            }
        }
        if !replaced {
            out.push(*quad);
        }
    }
}

#[node(
    id = "adaptive_merge_quads",
    inputs(
        port(name = "base", source = "Quads", ty = crate::daedalus_types::quads()),
        port(name = "next", source = "Quads", ty = crate::daedalus_types::quads()),
        port(name = "min_distance_rate", default = 0.125f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.005))
    ),
    outputs(port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads())),
    sync_groups(vec![daedalus::SyncGroup {
        name: "__no_implicit_sync".into(),
        policy: daedalus::SyncPolicy::AllReady,
        backpressure: None,
        capacity: None,
        ports: vec![],
    }])
)]
pub(super) fn cv_aruco_adaptive_merge_quads(base: Option<Vec<Quad>>, next: Option<Vec<Quad>>, min_distance_rate: f64) -> Result<Vec<Quad>, NodeError> {
    let mut base = base.unwrap_or_default();
    let next = next.unwrap_or_default();
    if base.is_empty() {
        return Ok(next);
    }
    if next.is_empty() {
        return Ok(base);
    }
    merge_quads_inplace(&mut base, &next, min_distance_rate.max(0.0));
    Ok(base)
}

#[node(
    id = "quads_concat",
    inputs(
        port(name = "a", source = "Quads", ty = crate::daedalus_types::quads()),
        port(name = "b", source = "Quads", ty = crate::daedalus_types::quads())
    ),
    outputs(port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads())),
    sync_groups(vec![daedalus::SyncGroup {
        name: "__no_implicit_sync".into(),
        policy: daedalus::SyncPolicy::AllReady,
        backpressure: None,
        capacity: None,
        ports: vec![],
    }])
)]
pub(super) fn cv_aruco_quads_concat(a: Option<Vec<Quad>>, b: Option<Vec<Quad>>) -> Result<Vec<Quad>, NodeError> {
    let mut out = a.unwrap_or_default();
    let b = b.unwrap_or_default();
    if out.is_empty() {
        return Ok(b);
    }
    if b.is_empty() {
        return Ok(out);
    }
    out.extend(b);
    Ok(out)
}

#[node(
    id = "adaptive_quads_select_best",
    summary = "Choose primary or alternate quad set.",
    inputs(
        port(name = "primary", source = "Quads", ty = crate::daedalus_types::quads()),
        port(name = "alternate", source = "Quads", ty = crate::daedalus_types::quads()),
        port(name = "enable_alternate", default = false)
    ),
    outputs(port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads())),
    sync_groups(vec![daedalus::SyncGroup {
        name: "__no_implicit_sync".into(),
        policy: daedalus::SyncPolicy::AllReady,
        backpressure: None,
        capacity: None,
        ports: vec![],
    }])
)]
pub(super) fn cv_aruco_adaptive_quads_select_best(primary: Option<Vec<Quad>>, alternate: Option<Vec<Quad>>, enable_alternate: bool) -> Result<Vec<Quad>, NodeError> {
    let primary = primary.unwrap_or_default();
    let alternate = alternate.unwrap_or_default();
    if enable_alternate && alternate.len() > primary.len() {
        return Ok(alternate);
    }
    Ok(primary)
}

#[node(
    id = "adaptive_quads_gate",
    summary = "Gate quad output by enable flag.",
    inputs(
        port(name = "enabled", default = true),
        port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads())
    ),
    outputs(port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads())),
    sync_groups(vec![daedalus::SyncGroup {
        name: "__no_implicit_sync".into(),
        policy: daedalus::SyncPolicy::AllReady,
        backpressure: None,
        capacity: None,
        ports: vec![],
    }])
)]
pub(super) fn cv_aruco_adaptive_quads_gate(enabled: bool, quads: Option<Vec<Quad>>) -> Result<Vec<Quad>, NodeError> {
    let quads = quads.unwrap_or_default();
    if enabled {
        return Ok(quads);
    }
    Ok(Vec::new())
}

#[node(
    id = "adaptive_quads_pass",
    summary = "Single adaptive-quads sweep pass (node-group wrapper).",
    inputs(
        port(name = "gray"),
        port(name = "enabled", default = true),
        port(name = "pass_idx", default = 0i64, meta(ui_min = 0, ui_max = 64, ui_step = 1)),
        port(name = "adaptive_window", default = 15i64, meta(ui_min = 3, ui_max = 101, ui_step = 2)),
        port(name = "use_window_sweep", default = false),
        port(name = "adaptive_window_min", default = 3i64, meta(ui_min = 3, ui_max = 101, ui_step = 2)),
        port(name = "adaptive_window_max", default = 23i64, meta(ui_min = 3, ui_max = 101, ui_step = 2)),
        port(name = "adaptive_window_step", default = 10i64, meta(ui_min = 1, ui_max = 50, ui_step = 1)),
        port(name = "adaptive_offset", default = 7.0f64, meta(ui_min = 0.0, ui_max = 64.0, ui_step = 1.0)),
        port(name = "threshold_offset", default = 0.0f64, meta(ui_min = -32.0, ui_max = 32.0, ui_step = 1.0)),
        port(name = "invert", default = true),
        port(name = "invert_flip", default = false),
        port(name = "open_k", default = 0i64, meta(ui_min = 0, ui_max = 31, ui_step = 1)),
        port(name = "min_perimeter_rate", default = 0.03f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01)),
        port(name = "max_perimeter_rate", default = 4.0f64, meta(ui_min = 0.0, ui_max = 20.0, ui_step = 0.1)),
        port(name = "epsilon", default = 5.0f64, meta(ui_min = 0.01, ui_max = 1000.0, ui_step = 0.1)),
        port(name = "min_area", default = 120.0f64, meta(ui_min = 0.0, ui_max = 1000000.0, ui_step = 1000.0)),
        port(name = "max_area", default = 0.0f64, meta(ui_min = 0.0, ui_max = 1000000.0, ui_step = 1000.0)),
        port(name = "min_angle_deg", default = 5.0f64, meta(ui_min = 0.0, ui_max = 180.0, ui_step = 1.0)),
        port(name = "max_angle_deg", default = 175.0f64, meta(ui_min = 0.0, ui_max = 180.0, ui_step = 1.0)),
        port(name = "max_side_cv", default = 4.0f64, meta(ui_min = 0.05, ui_max = 20.0, ui_step = 0.1)),
        port(name = "min_corner_distance_rate", default = 0.05f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01)),
        port(name = "min_distance_to_border", default = 3i64, meta(ui_min = 0, ui_max = 128, ui_step = 1))
    ),
    outputs(port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()))
)]
#[allow(clippy::too_many_arguments)]
pub(super) fn cv_aruco_adaptive_quads_pass(
    g: &mut GraphCtx,
    gray: GrayImage,
    enabled: bool,
    pass_idx: i64,
    adaptive_window: i64,
    use_window_sweep: bool,
    adaptive_window_min: i64,
    adaptive_window_max: i64,
    adaptive_window_step: i64,
    adaptive_offset: f64,
    threshold_offset: f64,
    invert: bool,
    invert_flip: bool,
    open_k: i64,
    min_perimeter_rate: f64,
    max_perimeter_rate: f64,
    epsilon: f64,
    min_area: f64,
    max_area: f64,
    min_angle_deg: f64,
    max_angle_deg: f64,
    max_side_cv: f64,
    min_corner_distance_rate: f64,
    min_distance_to_border: i64,
) -> Vec<Quad> {
    let thresh = g.node_as("cv:aruco:adaptive_threshold_mask", "threshold");
    g.connect(&enabled, &thresh.input("enabled"));
    g.connect(&gray, &thresh.input("gray"));
    let _ = (&pass_idx, &use_window_sweep, &adaptive_window_min, &adaptive_window_max, &adaptive_window_step);
    g.connect(&adaptive_window, &thresh.input("adaptive_window"));
    g.connect(&adaptive_offset, &thresh.input("adaptive_offset"));
    g.connect(&threshold_offset, &thresh.input("threshold_offset"));
    g.connect(&invert, &thresh.input("invert"));
    g.connect(&invert_flip, &thresh.input("invert_flip"));

    let opened = g.node_as("cv:aruco:mask_open_stage", "open");
    g.connect(&thresh.output("mask"), &opened.input("in_mask"));
    g.connect(&open_k, &opened.input("k"));

    let quads = g.node_as("cv:aruco:adaptive_quads_from_mask", "quads");
    g.connect(&enabled, &quads.input("enabled"));
    g.connect(&opened.output("out_mask"), &quads.input("mask"));
    g.connect(&min_perimeter_rate, &quads.input("min_perimeter_rate"));
    g.connect(&max_perimeter_rate, &quads.input("max_perimeter_rate"));
    g.connect(&epsilon, &quads.input("epsilon"));
    g.connect(&min_area, &quads.input("min_area"));
    g.connect(&max_area, &quads.input("max_area"));
    g.connect(&min_angle_deg, &quads.input("min_angle_deg"));
    g.connect(&max_angle_deg, &quads.input("max_angle_deg"));
    g.connect(&max_side_cv, &quads.input("max_side_cv"));
    g.connect(&min_corner_distance_rate, &quads.input("min_corner_distance_rate"));
    g.connect(&min_distance_to_border, &quads.input("min_distance_to_border"));
    quads.output("quads")
}
