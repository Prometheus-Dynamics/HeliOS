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
    // This node is used in generated sweep chains where the "base" input may legitimately be absent
    // (e.g. the first merge in a reduced/pruned sweep). Allow partial inputs and avoid implicit
    // sync-group behavior.
    sync_groups(vec![daedalus::SyncGroup {
        name: "__no_implicit_sync".into(),
        policy: daedalus::SyncPolicy::AllReady,
        backpressure: None,
        capacity: None,
        ports: vec![],
    }])
)]
fn cv_aruco_adaptive_merge_quads(base: Option<Vec<Quad>>, next: Option<Vec<Quad>>, min_distance_rate: f64) -> Result<Vec<Quad>, NodeError> {
    // NOTE: Keep this node cheap; it runs once per sweep-pass and exists mainly for profiling.
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
    // This node intentionally supports partial inputs (it's a fan-in combiner, not a strict aligner).
    // Provide an empty sync group to disable the runtime's implicit AllReady sync group.
    sync_groups(vec![daedalus::SyncGroup {
        name: "__no_implicit_sync".into(),
        policy: daedalus::SyncPolicy::AllReady,
        backpressure: None,
        capacity: None,
        ports: vec![],
    }])
)]
fn cv_aruco_quads_concat(a: Option<Vec<Quad>>, b: Option<Vec<Quad>>) -> Result<Vec<Quad>, NodeError> {
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
fn cv_aruco_adaptive_quads_select_best(primary: Option<Vec<Quad>>, alternate: Option<Vec<Quad>>, enable_alternate: bool) -> Result<Vec<Quad>, NodeError> {
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
    // Allow this gate to run even when upstream candidate extraction emits no payload.
    // In that case we still want to propagate an explicit empty Vec so downstream decode
    // stages keep running and host outputs do not starve.
    sync_groups(vec![daedalus::SyncGroup {
        name: "__no_implicit_sync".into(),
        policy: daedalus::SyncPolicy::AllReady,
        backpressure: None,
        capacity: None,
        ports: vec![],
    }])
)]
fn cv_aruco_adaptive_quads_gate(enabled: bool, quads: Option<Vec<Quad>>) -> Result<Vec<Quad>, NodeError> {
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
fn cv_aruco_adaptive_quads_pass(
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
    // Split the old mega-node into stages so profiling can attribute time to:
    // - window selection (cheap)
    // - adaptive thresholding
    // - morphology (optional)
    // - quad extraction/DP filtering (heavy)
    let thresh = g.node_as("cv:aruco:adaptive_threshold_mask", "threshold");
    g.connect(&enabled, &thresh.input("enabled"));
    g.connect(&gray, &thresh.input("gray"));
    // Keep this path direct; group-expanded scalar pass-through from helper nodes has proven
    // brittle in this graph and can drop required inputs under demand-driven execution.
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

#[node(
    id = "adaptive_window_select",
    inputs(
        port(name = "enabled", default = true),
        port(name = "pass_idx", default = 0i64, meta(ui_min = 0, ui_max = 64, ui_step = 1)),
        port(name = "adaptive_window", default = 15i64, meta(ui_min = 3, ui_max = 101, ui_step = 2)),
        port(name = "use_window_sweep", default = false),
        port(name = "adaptive_window_min", default = 3i64, meta(ui_min = 3, ui_max = 101, ui_step = 2)),
        port(name = "adaptive_window_max", default = 23i64, meta(ui_min = 3, ui_max = 101, ui_step = 2)),
        port(name = "adaptive_window_step", default = 10i64, meta(ui_min = 1, ui_max = 50, ui_step = 1))
    ),
    outputs(port(name = "window", default = 0i64))
)]
fn cv_aruco_adaptive_window_select(
    enabled: bool,
    pass_idx: i64,
    adaptive_window: i64,
    use_window_sweep: bool,
    adaptive_window_min: i64,
    adaptive_window_max: i64,
    adaptive_window_step: i64,
) -> Result<i64, NodeError> {
    if !enabled {
        return Ok(0);
    }
    let pass_idx_u32 = u32::try_from(pass_idx.max(0)).unwrap_or(0);
    let window = if use_window_sweep {
        let mut window_min = u32::try_from(adaptive_window_min).unwrap_or(3).max(3);
        if window_min.is_multiple_of(2) {
            window_min = window_min.saturating_add(1);
        }
        let mut window_max = u32::try_from(adaptive_window_max).unwrap_or(window_min).max(window_min);
        if window_max.is_multiple_of(2) {
            window_max = window_max.saturating_add(1);
        }
        let step = u32::try_from(adaptive_window_step).unwrap_or(1).max(1);
        let mut w = window_min;
        for _ in 0..pass_idx_u32 {
            let next = w.saturating_add(step);
            w = if next.is_multiple_of(2) { next.saturating_add(1) } else { next };
        }
        if w > window_max { 0 } else { w }
    } else {
        if pass_idx_u32 != 0 {
            return Ok(0);
        }
        let mut w = u32::try_from(adaptive_window).unwrap_or(15).max(3);
        if w.is_multiple_of(2) {
            w = w.saturating_add(1);
        }
        w
    };
    Ok(window as i64)
}

#[node(
    id = "adaptive_threshold_mask",
    inputs(
        port(name = "enabled", default = true),
        port(name = "gray"),
        port(name = "adaptive_window", default = 15i64, meta(ui_min = 0, ui_max = 101, ui_step = 1)),
        port(name = "adaptive_offset", default = 7.0f64, meta(ui_min = 0.0, ui_max = 64.0, ui_step = 1.0)),
        port(name = "threshold_offset", default = 0.0f64, meta(ui_min = -32.0, ui_max = 32.0, ui_step = 1.0)),
        port(name = "invert", default = true),
        port(name = "invert_flip", default = false)
    ),
    outputs(port(name = "mask"))
)]
fn cv_aruco_adaptive_threshold_mask(
    enabled: bool,
    gray: &GrayImage,
    adaptive_window: i64,
    adaptive_offset: f64,
    threshold_offset: f64,
    invert: bool,
    invert_flip: bool,
) -> Result<GrayImage, NodeError> {
    if !enabled {
        return Ok(GrayImage::new(0, 0));
    }
    let window = u32::try_from(adaptive_window).unwrap_or(0);
    if window < 3 || gray.width() == 0 || gray.height() == 0 {
        return Ok(GrayImage::new(0, 0));
    }
    let offset = adaptive_offset.clamp(0.0, 64.0) as f32 + threshold_offset.clamp(-32.0, 32.0) as f32;
    let invert = invert ^ invert_flip;
    Ok(crate::modules::image::binary::adaptive_mean_threshold_fast_with_invert(gray, window, offset, invert))
}

#[node(
    id = "adaptive_quads_from_mask",
    summary = "Extract quads from a binary mask.",
    inputs(
        port(name = "enabled", default = true),
        port(name = "mask"),
        port(name = "min_perimeter_rate", default = 0.03f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01)),
        port(name = "max_perimeter_rate", default = 4.0f64, meta(ui_min = 0.0, ui_max = 20.0, ui_step = 0.1)),
        port(name = "epsilon", default = 5.0f64, meta(ui_min = 0.01, ui_max = 1000.0, ui_step = 0.1)),
        port(name = "min_area", default = 120.0f64, meta(ui_min = 0.0, ui_max = 1000000.0, ui_step = 1000.0)),
        port(name = "max_area", default = 0.0f64, meta(ui_min = 0.0, ui_max = 1000000.0, ui_step = 1000.0)),
        port(name = "min_angle_deg", default = 5.0f64, meta(ui_min = 0.0, ui_max = 180.0, ui_step = 1.0)),
        port(name = "max_angle_deg", default = 175.0f64, meta(ui_min = 0.0, ui_max = 180.0, ui_step = 1.0)),
        port(name = "max_side_cv", default = 4.0f64, meta(ui_min = 0.05, ui_max = 20.0, ui_step = 0.1)),
        port(name = "min_corner_distance_rate", default = 0.05f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01)),
        port(name = "min_distance_to_border", default = 3i64, meta(ui_min = 0, ui_max = 128, ui_step = 1)),
        port(name = "min_side_px", default = 0.0f64, meta(ui_min = 0.0, ui_max = 128.0, ui_step = 0.5)),
        port(name = "fallback_max_contours", default = 120i64, meta(ui_min = 0, ui_max = 1000, ui_step = 1)),
        port(name = "max_quads", default = 0i64, meta(ui_min = 0, ui_max = 512, ui_step = 1)),
        port(name = "expand_inner_scale", default = 1.0f64, meta(ui_min = 1.0, ui_max = 4.0, ui_step = 0.05)),
        port(name = "expand_inner_max_side_px", default = 0.0f64, meta(ui_min = 0.0, ui_max = 512.0, ui_step = 1.0))
    ),
    outputs(port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()))
)]
#[allow(clippy::too_many_arguments)]
fn cv_aruco_adaptive_quads_from_mask(
    enabled: bool,
    mask: &GrayImage,
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
    min_side_px: f64,
    fallback_max_contours: i64,
    max_quads: i64,
    expand_inner_scale: f64,
    expand_inner_max_side_px: f64,
) -> Result<Vec<Quad>, NodeError> {
    if !enabled {
        return Ok(Vec::new());
    }

    if mask.width() == 0 || mask.height() == 0 {
        return Ok(Vec::new());
    }

    let cfg = crate::modules::aruco::adaptive::AdaptiveDetectorConfig {
        adaptive_window: 15,
        adaptive_offset: 7.0,
        threshold_offset: 0.0,
        invert: false,
        open_k: 0,
        min_perimeter_rate: min_perimeter_rate.max(0.0) as f32,
        max_perimeter_rate: max_perimeter_rate.max(min_perimeter_rate + f64::EPSILON) as f32,
        epsilon: epsilon.max(0.01) as f32,
        min_area: min_area.max(0.0) as f32,
        max_area: if max_area > 0.0 { Some(max_area as f32) } else { None },
        min_angle: min_angle_deg.max(0.0) as f32,
        max_angle: max_angle_deg.min(180.0) as f32,
        max_side_cv: max_side_cv.max(0.05) as f32,
        min_corner_distance_rate: min_corner_distance_rate.max(0.0) as f32,
        min_distance_to_border: u32::try_from(min_distance_to_border.max(0)).unwrap_or(0),
        min_side_px: min_side_px.max(0.0) as f32,
        fallback_max_contours: usize::try_from(fallback_max_contours.clamp(0, 1000)).unwrap_or(120),
        max_quads: usize::try_from(max_quads.clamp(0, 512)).unwrap_or(0),
    };

    let expand_inner_scale = expand_inner_scale.max(1.0);
    let expand_inner_max_side_px = expand_inner_max_side_px.max(0.0);
    let max_x = (mask.width().saturating_sub(1)) as f64;
    let max_y = (mask.height().saturating_sub(1)) as f64;

    let mut out = Vec::new();
    for quad in crate::modules::aruco::adaptive::adaptive_quads_from_mask(mask, &cfg) {
        let base_quad = [
            Point { x: quad[0].x as f64, y: quad[0].y as f64 },
            Point { x: quad[1].x as f64, y: quad[1].y as f64 },
            Point { x: quad[2].x as f64, y: quad[2].y as f64 },
            Point { x: quad[3].x as f64, y: quad[3].y as f64 },
        ];
        out.push(base_quad);

        if expand_inner_scale > 1.0 && expand_inner_max_side_px > 0.0 {
            let mut max_side = 0.0f64;
            for i in 0..4usize {
                let a = base_quad[i];
                let b = base_quad[(i + 1) % 4];
                let dx = a.x - b.x;
                let dy = a.y - b.y;
                max_side = max_side.max((dx * dx + dy * dy).sqrt());
            }
            if max_side <= expand_inner_max_side_px {
                let cx = (base_quad[0].x + base_quad[1].x + base_quad[2].x + base_quad[3].x) * 0.25;
                let cy = (base_quad[0].y + base_quad[1].y + base_quad[2].y + base_quad[3].y) * 0.25;
                let mut expanded = base_quad;
                for p in &mut expanded {
                    p.x = (cx + (p.x - cx) * expand_inner_scale).clamp(0.0, max_x);
                    p.y = (cy + (p.y - cy) * expand_inner_scale).clamp(0.0, max_y);
                }
                out.push(expanded);
            }
        }
    }
    Ok(out)
}
