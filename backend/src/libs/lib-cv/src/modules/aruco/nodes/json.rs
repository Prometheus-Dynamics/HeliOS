use super::*;

#[node(
    id = "poses_json",
    inputs(port(name = "poses", source = "DetectionPoses", ty = crate::daedalus_types::detection_pose_output())),
    outputs(port(name = "json", source = "Json", ty = crate::daedalus_types::json_value()))
)]
fn cv_aruco_poses_json(poses: &DetectionPoseOutput) -> Result<String, NodeError> {
    // Host outputs capture JSON from `Value::String`/`Value::Bytes`; keep this as a plain string.
    serde_json::to_string(poses).map_err(|err| NodeError::InvalidInput(format!("poses_json serialization failed: {err}")))
}

#[node(
    id = "detections_json",
    inputs(
        port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()),
        port(name = "min_id", default = -1i64, meta(ui_min = -1, ui_max = 10000, ui_step = 1)),
        port(name = "max_id", default = -1i64, meta(ui_min = -1, ui_max = 10000, ui_step = 1))
    ),
    outputs(port(name = "json", source = "Json", ty = crate::daedalus_types::json_value()))
)]
fn cv_aruco_detections_json(detections: &Vec<ArucoDetection2D>, min_id: i64, max_id: i64) -> Result<String, NodeError> {
    #[derive(serde::Serialize)]
    struct DetectionsStats {
        #[serde(rename = "outputDetections")]
        output_detections: usize,
    }

    #[derive(serde::Serialize)]
    struct DetectionsJson<'a> {
        detections: &'a [ArucoDetection2D],
        stats: DetectionsStats,
    }

    let filtered;
    let detections = if min_id < 0 || max_id < 0 || max_id < min_id {
        detections.as_slice()
    } else {
        filtered = filter_detections_id_range(detections, min_id, max_id);
        filtered.as_slice()
    };
    let value = DetectionsJson { detections, stats: DetectionsStats { output_detections: detections.len() } };
    match serde_json::to_string(&value) {
        Ok(serialized) => Ok(serialized),
        Err(err) => Ok(serde_json::json!({
            "detections": [],
            "stats": { "outputDetections": 0 },
            "error": format!("detections_json serialization failed: {err}")
        })
        .to_string()),
    }
}

#[node(
    id = "detections_filter_area",
    summary = "Filter detections by quad area.",
    description = "Keeps detections where area is in [min_area, max_area]. Use max_area <= 0 to disable the upper bound.",
    inputs(
        port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()),
        port(name = "min_area", default = 0.0f64, meta(ui_min = 0.0, ui_max = 1000000.0, ui_step = 10.0)),
        port(name = "max_area", default = 0.0f64, meta(ui_min = 0.0, ui_max = 1000000.0, ui_step = 10.0)),
        port(name = "suppress_nested", default = false),
        port(name = "nested_area_ratio_max", default = 0.85f64, meta(ui_min = 0.1, ui_max = 1.0, ui_step = 0.01))
    ),
    outputs(port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()))
)]
fn cv_aruco_detections_filter_area(detections: &Vec<ArucoDetection2D>, min_area: f64, max_area: f64, suppress_nested: bool, nested_area_ratio_max: f64) -> Result<Vec<ArucoDetection2D>, NodeError> {
    fn det_center(det: &ArucoDetection2D) -> (f64, f64) {
        let mut sx = 0.0;
        let mut sy = 0.0;
        for p in &det.corners {
            sx += p.x;
            sy += p.y;
        }
        (sx / 4.0, sy / 4.0)
    }

    fn point_in_quad(px: f64, py: f64, corners: &[Point; 4]) -> bool {
        let mut sign = 0i8;
        for i in 0..4 {
            let a = corners[i];
            let b = corners[(i + 1) & 3];
            let ax = a.x;
            let ay = a.y;
            let bx = b.x;
            let by = b.y;
            let cross = (bx - ax) * (py - ay) - (by - ay) * (px - ax);
            if cross.abs() <= 1e-9 {
                continue;
            }
            let cur = if cross > 0.0 { 1 } else { -1 };
            if sign == 0 {
                sign = cur;
            } else if sign != cur {
                return false;
            }
        }
        true
    }

    let min_area = min_area.max(0.0);
    let max_area = if max_area > 0.0 { Some(max_area.max(min_area)) } else { None };
    let mut out: Vec<ArucoDetection2D> = detections
        .iter()
        .filter(|det| {
            let area = detection_area(det);
            if area < min_area {
                return false;
            }
            if let Some(max_area) = max_area
                && area > max_area
            {
                return false;
            }
            true
        })
        .cloned()
        .collect();

    if !suppress_nested || out.len() < 2 {
        return Ok(out);
    }

    // Suppress tiny nested boxes (often ghost decodes inside a valid marker).
    let ratio_max = nested_area_ratio_max.clamp(0.1, 1.0);
    let areas: Vec<f64> = out.iter().map(detection_area).collect();
    let centers: Vec<(f64, f64)> = out.iter().map(det_center).collect();
    let mut keep = vec![true; out.len()];

    for i in 0..out.len() {
        if !keep[i] {
            continue;
        }
        let area_i = areas[i];
        let (cx, cy) = centers[i];
        for j in 0..out.len() {
            if i == j || !keep[j] {
                continue;
            }
            let area_j = areas[j];
            if area_j <= area_i {
                continue;
            }
            if area_i > area_j * ratio_max {
                continue;
            }
            if point_in_quad(cx, cy, &out[j].corners) {
                keep[i] = false;
                break;
            }
        }
    }

    out = out.into_iter().zip(keep).filter_map(|(det, keep)| if keep { Some(det) } else { None }).collect();
    Ok(out)
}

#[node(
    id = "detections_filter",
    summary = "Select a single detection using a spatial/area filter.",
    description = "Filters a list of ArUco detections down to a single detection based on position (left/right/top/bottom/corners), middle-most, or area.",
    inputs(
        port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()),
        port(name = "mode", default = "left_most")
    ),
    outputs(port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()))
)]
fn cv_aruco_detections_filter(detections: &Vec<ArucoDetection2D>, mode: ArucoDetectionsFilterMode) -> Result<Vec<ArucoDetection2D>, NodeError> {
    #[derive(Clone, Copy)]
    struct Candidate<'a> {
        det: &'a ArucoDetection2D,
        cx: f64,
        cy: f64,
        area: f64,
    }

    fn cmp_f64(a: f64, b: f64) -> std::cmp::Ordering {
        if !a.is_finite() && !b.is_finite() {
            return std::cmp::Ordering::Equal;
        }
        if !a.is_finite() {
            return std::cmp::Ordering::Greater;
        }
        if !b.is_finite() {
            return std::cmp::Ordering::Less;
        }
        a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
    }

    fn is_better_left(a: Candidate<'_>, b: Candidate<'_>) -> bool {
        let ord = cmp_f64(a.cx, b.cx);
        if ord != std::cmp::Ordering::Equal {
            return ord == std::cmp::Ordering::Less;
        }
        let ord = cmp_f64(a.cy, b.cy);
        if ord != std::cmp::Ordering::Equal {
            return ord == std::cmp::Ordering::Less;
        }
        a.det.id < b.det.id
    }

    fn is_better_right(a: Candidate<'_>, b: Candidate<'_>) -> bool {
        let ord = cmp_f64(a.cx, b.cx);
        if ord != std::cmp::Ordering::Equal {
            return ord == std::cmp::Ordering::Greater;
        }
        let ord = cmp_f64(a.cy, b.cy);
        if ord != std::cmp::Ordering::Equal {
            return ord == std::cmp::Ordering::Less;
        }
        a.det.id < b.det.id
    }

    fn is_better_top(a: Candidate<'_>, b: Candidate<'_>) -> bool {
        let ord = cmp_f64(a.cy, b.cy);
        if ord != std::cmp::Ordering::Equal {
            return ord == std::cmp::Ordering::Less;
        }
        let ord = cmp_f64(a.cx, b.cx);
        if ord != std::cmp::Ordering::Equal {
            return ord == std::cmp::Ordering::Less;
        }
        a.det.id < b.det.id
    }

    fn is_better_bottom(a: Candidate<'_>, b: Candidate<'_>) -> bool {
        let ord = cmp_f64(a.cy, b.cy);
        if ord != std::cmp::Ordering::Equal {
            return ord == std::cmp::Ordering::Greater;
        }
        let ord = cmp_f64(a.cx, b.cx);
        if ord != std::cmp::Ordering::Equal {
            return ord == std::cmp::Ordering::Less;
        }
        a.det.id < b.det.id
    }

    fn is_better_top_right(a: Candidate<'_>, b: Candidate<'_>) -> bool {
        let ord = cmp_f64(a.cy, b.cy);
        if ord != std::cmp::Ordering::Equal {
            return ord == std::cmp::Ordering::Less;
        }
        let ord = cmp_f64(a.cx, b.cx);
        if ord != std::cmp::Ordering::Equal {
            return ord == std::cmp::Ordering::Greater;
        }
        a.det.id < b.det.id
    }

    fn is_better_bottom_left(a: Candidate<'_>, b: Candidate<'_>) -> bool {
        let ord = cmp_f64(a.cy, b.cy);
        if ord != std::cmp::Ordering::Equal {
            return ord == std::cmp::Ordering::Greater;
        }
        let ord = cmp_f64(a.cx, b.cx);
        if ord != std::cmp::Ordering::Equal {
            return ord == std::cmp::Ordering::Less;
        }
        a.det.id < b.det.id
    }

    fn is_better_bottom_right(a: Candidate<'_>, b: Candidate<'_>) -> bool {
        let ord = cmp_f64(a.cy, b.cy);
        if ord != std::cmp::Ordering::Equal {
            return ord == std::cmp::Ordering::Greater;
        }
        let ord = cmp_f64(a.cx, b.cx);
        if ord != std::cmp::Ordering::Equal {
            return ord == std::cmp::Ordering::Greater;
        }
        a.det.id < b.det.id
    }

    fn is_better_area(a: Candidate<'_>, b: Candidate<'_>, prefer_largest: bool) -> bool {
        let ord = cmp_f64(a.area, b.area);
        if ord != std::cmp::Ordering::Equal {
            return if prefer_largest { ord == std::cmp::Ordering::Greater } else { ord == std::cmp::Ordering::Less };
        }
        a.det.id < b.det.id
    }

    let mut candidates: Vec<Candidate<'_>> = Vec::with_capacity(detections.len());
    for det in detections {
        let (cx, cy) = detection_center(det);
        if !cx.is_finite() || !cy.is_finite() {
            continue;
        }
        let area = detection_area(det);
        if !area.is_finite() {
            continue;
        }
        candidates.push(Candidate { det, cx, cy, area });
    }
    if candidates.is_empty() {
        return Ok(Vec::new());
    }

    let mut best: Candidate<'_> = candidates[0];
    match mode {
        ArucoDetectionsFilterMode::LeftMost => {
            for candidate in candidates.iter().copied().skip(1) {
                if is_better_left(candidate, best) {
                    best = candidate;
                }
            }
        }
        ArucoDetectionsFilterMode::RightMost => {
            for candidate in candidates.iter().copied().skip(1) {
                if is_better_right(candidate, best) {
                    best = candidate;
                }
            }
        }
        ArucoDetectionsFilterMode::TopMost | ArucoDetectionsFilterMode::TopLeft => {
            for candidate in candidates.iter().copied().skip(1) {
                if is_better_top(candidate, best) {
                    best = candidate;
                }
            }
        }
        ArucoDetectionsFilterMode::BottomMost => {
            for candidate in candidates.iter().copied().skip(1) {
                if is_better_bottom(candidate, best) {
                    best = candidate;
                }
            }
        }
        ArucoDetectionsFilterMode::TopRight => {
            for candidate in candidates.iter().copied().skip(1) {
                if is_better_top_right(candidate, best) {
                    best = candidate;
                }
            }
        }
        ArucoDetectionsFilterMode::BottomLeft => {
            for candidate in candidates.iter().copied().skip(1) {
                if is_better_bottom_left(candidate, best) {
                    best = candidate;
                }
            }
        }
        ArucoDetectionsFilterMode::BottomRight => {
            for candidate in candidates.iter().copied().skip(1) {
                if is_better_bottom_right(candidate, best) {
                    best = candidate;
                }
            }
        }
        ArucoDetectionsFilterMode::LargestArea => {
            for candidate in candidates.iter().copied().skip(1) {
                if is_better_area(candidate, best, true) {
                    best = candidate;
                }
            }
        }
        ArucoDetectionsFilterMode::SmallestArea => {
            for candidate in candidates.iter().copied().skip(1) {
                if is_better_area(candidate, best, false) {
                    best = candidate;
                }
            }
        }
        ArucoDetectionsFilterMode::MiddleMost => {
            let mut sx = 0.0f64;
            let mut sy = 0.0f64;
            for candidate in candidates.iter() {
                sx += candidate.cx;
                sy += candidate.cy;
            }
            let count = candidates.len() as f64;
            let mx = if count > 0.0 { sx / count } else { 0.0 };
            let my = if count > 0.0 { sy / count } else { 0.0 };
            let mut best_dist = (best.cx - mx).powi(2) + (best.cy - my).powi(2);
            for candidate in candidates.iter().copied().skip(1) {
                let dist = (candidate.cx - mx).powi(2) + (candidate.cy - my).powi(2);
                if dist < best_dist || (dist == best_dist && candidate.det.id < best.det.id) {
                    best = candidate;
                    best_dist = dist;
                }
            }
        }
    }

    Ok(vec![best.det.clone()])
}

#[node(
    id = "detections_order",
    summary = "Order detections by spatial/area/crosshair mode.",
    description = "Returns all detections sorted by the selected mode. `none` preserves input order.",
    inputs(
        port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()),
        port(name = "mode", default = "none"),
        port(name = "crosshair_x", default = 0i64, meta(ui_min = 0, ui_max = 8192, ui_step = 1)),
        port(name = "crosshair_y", default = 0i64, meta(ui_min = 0, ui_max = 8192, ui_step = 1))
    ),
    outputs(port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()))
)]
fn cv_aruco_detections_order(detections: &Vec<ArucoDetection2D>, mode: String, crosshair_x: i64, crosshair_y: i64) -> Result<Vec<ArucoDetection2D>, NodeError> {
    #[derive(Clone)]
    struct Candidate {
        det: ArucoDetection2D,
        cx: f64,
        cy: f64,
        area: f64,
    }

    fn cmp_f64(a: f64, b: f64) -> std::cmp::Ordering {
        if !a.is_finite() && !b.is_finite() {
            return std::cmp::Ordering::Equal;
        }
        if !a.is_finite() {
            return std::cmp::Ordering::Greater;
        }
        if !b.is_finite() {
            return std::cmp::Ordering::Less;
        }
        a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
    }

    fn parse_order_mode(raw: &str) -> ArucoDetectionsOrderMode {
        match raw.trim().to_ascii_lowercase().as_str() {
            "largest_to_smallest" => ArucoDetectionsOrderMode::LargestToSmallest,
            "smallest_to_largest" => ArucoDetectionsOrderMode::SmallestToLargest,
            "top_most" => ArucoDetectionsOrderMode::TopMost,
            "bottom_most" => ArucoDetectionsOrderMode::BottomMost,
            "left_most" => ArucoDetectionsOrderMode::LeftMost,
            "right_most" => ArucoDetectionsOrderMode::RightMost,
            "top_left" => ArucoDetectionsOrderMode::TopLeft,
            "top_right" => ArucoDetectionsOrderMode::TopRight,
            "bottom_left" => ArucoDetectionsOrderMode::BottomLeft,
            "bottom_right" => ArucoDetectionsOrderMode::BottomRight,
            "center_most" => ArucoDetectionsOrderMode::CenterMost,
            "crosshair" => ArucoDetectionsOrderMode::Crosshair,
            _ => ArucoDetectionsOrderMode::None,
        }
    }

    let mode = parse_order_mode(&mode);

    if detections.len() <= 1 || mode == ArucoDetectionsOrderMode::None {
        return Ok(detections.clone());
    }

    let mut candidates: Vec<Candidate> = Vec::with_capacity(detections.len());
    let mut invalid: Vec<ArucoDetection2D> = Vec::new();
    for det in detections.iter().cloned() {
        let (cx, cy) = detection_center(&det);
        let area = detection_area(&det);
        if !(cx.is_finite() && cy.is_finite() && area.is_finite()) {
            invalid.push(det);
            continue;
        }
        candidates.push(Candidate { det, cx, cy, area });
    }

    if candidates.len() <= 1 {
        let mut out = candidates.into_iter().map(|c| c.det).collect::<Vec<_>>();
        out.extend(invalid);
        return Ok(out);
    }

    let (center_x, center_y) = if mode == ArucoDetectionsOrderMode::CenterMost {
        let mut sx = 0.0;
        let mut sy = 0.0;
        for c in &candidates {
            sx += c.cx;
            sy += c.cy;
        }
        let n = candidates.len() as f64;
        (sx / n, sy / n)
    } else {
        (crosshair_x as f64, crosshair_y as f64)
    };

    candidates.sort_by(|a, b| {
        use std::cmp::Ordering;
        let ord = match mode {
            ArucoDetectionsOrderMode::LargestToSmallest => cmp_f64(b.area, a.area),
            ArucoDetectionsOrderMode::SmallestToLargest => cmp_f64(a.area, b.area),
            ArucoDetectionsOrderMode::TopMost => cmp_f64(a.cy, b.cy).then_with(|| cmp_f64(a.cx, b.cx)),
            ArucoDetectionsOrderMode::BottomMost => cmp_f64(b.cy, a.cy).then_with(|| cmp_f64(a.cx, b.cx)),
            ArucoDetectionsOrderMode::LeftMost => cmp_f64(a.cx, b.cx).then_with(|| cmp_f64(a.cy, b.cy)),
            ArucoDetectionsOrderMode::RightMost => cmp_f64(b.cx, a.cx).then_with(|| cmp_f64(a.cy, b.cy)),
            ArucoDetectionsOrderMode::TopLeft => cmp_f64(a.cy, b.cy).then_with(|| cmp_f64(a.cx, b.cx)),
            ArucoDetectionsOrderMode::TopRight => cmp_f64(a.cy, b.cy).then_with(|| cmp_f64(b.cx, a.cx)),
            ArucoDetectionsOrderMode::BottomLeft => cmp_f64(b.cy, a.cy).then_with(|| cmp_f64(a.cx, b.cx)),
            ArucoDetectionsOrderMode::BottomRight => cmp_f64(b.cy, a.cy).then_with(|| cmp_f64(b.cx, a.cx)),
            ArucoDetectionsOrderMode::CenterMost | ArucoDetectionsOrderMode::Crosshair => {
                let adx = a.cx - center_x;
                let ady = a.cy - center_y;
                let bdx = b.cx - center_x;
                let bdy = b.cy - center_y;
                let ad2 = adx * adx + ady * ady;
                let bd2 = bdx * bdx + bdy * bdy;
                cmp_f64(ad2, bd2).then_with(|| cmp_f64(b.area, a.area))
            }
            ArucoDetectionsOrderMode::None => Ordering::Equal,
        };
        if ord == Ordering::Equal { a.det.id.cmp(&b.det.id) } else { ord }
    });

    let mut out = candidates.into_iter().map(|c| c.det).collect::<Vec<_>>();
    out.extend(invalid);
    Ok(out)
}

#[derive(Clone, Debug, NodeConfig)]
struct ArucoCrosshairTargetConfig {
    #[port(default = 0i64, meta(ui_min = 0, ui_max = 8192, ui_step = 1))]
    crosshair_x: i64,
    #[port(default = 0i64, meta(ui_min = 0, ui_max = 8192, ui_step = 1))]
    crosshair_y: i64,
    #[port(default = 0i64, meta(ui_min = 0, ui_max = 8192, ui_step = 1))]
    frame_width: i64,
    #[port(default = 0i64, meta(ui_min = 0, ui_max = 8192, ui_step = 1))]
    frame_height: i64,
    #[port(default = false)]
    require_crosshair_inside: bool,
    #[port(default = true)]
    fallback_to_nearest: bool,
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 4096.0, ui_step = 1.0))]
    max_distance_px: f64,
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 180.0, ui_step = 0.1))]
    hfov_deg: f64,
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 180.0, ui_step = 0.1))]
    vfov_deg: f64,
}

type ArucoCrosshairTargetOutput = (Vec<ArucoDetection2D>, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64);

#[node(
    id = "detections_crosshair_target",
    summary = "Select the best detection for a crosshair point.",
    description = "Returns the nearest detection to (crosshair_x, crosshair_y), with optional in-box gating and Limelight-style target metrics.",
    inputs(
        port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()),
        config = ArucoCrosshairTargetConfig
    ),
    outputs(
        port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()),
        "tv",
        "tid",
        "tx",
        "ty",
        "ta",
        "distance_px",
        "cx",
        "cy",
        "thor",
        "tvert",
        "tshort",
        "tlong"
    )
)]
fn cv_aruco_detections_crosshair_target(detections: &Vec<ArucoDetection2D>, cfg: ArucoCrosshairTargetConfig) -> Result<ArucoCrosshairTargetOutput, NodeError> {
    #[derive(Clone, Copy)]
    struct Candidate<'a> {
        det: &'a ArucoDetection2D,
        cx: f64,
        cy: f64,
        area: f64,
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
        dist2: f64,
        contains_crosshair: bool,
    }

    let chx = cfg.crosshair_x as f64;
    let chy = cfg.crosshair_y as f64;
    let mut candidates: Vec<Candidate<'_>> = Vec::with_capacity(detections.len());
    for det in detections {
        let (cx, cy) = detection_center(det);
        if !cx.is_finite() || !cy.is_finite() {
            continue;
        }
        let area = detection_area(det);
        if !area.is_finite() || area <= 0.0 {
            continue;
        }
        let (min_x, min_y, max_x, max_y) = detection_bbox(det);
        if !(min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite()) {
            continue;
        }
        let contains_crosshair = chx >= min_x && chx <= max_x && chy >= min_y && chy <= max_y;
        let dx = cx - chx;
        let dy = cy - chy;
        candidates.push(Candidate { det, cx, cy, area, min_x, min_y, max_x, max_y, dist2: dx * dx + dy * dy, contains_crosshair });
    }

    if candidates.is_empty() {
        return Ok((Vec::new(), 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0));
    }

    let mut active: Vec<Candidate<'_>> = if cfg.require_crosshair_inside { candidates.iter().copied().filter(|candidate| candidate.contains_crosshair).collect() } else { candidates.clone() };
    if active.is_empty() {
        if cfg.require_crosshair_inside && cfg.fallback_to_nearest {
            active = candidates;
        } else {
            return Ok((Vec::new(), 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0));
        }
    }

    let max_distance_px = if cfg.max_distance_px.is_finite() { cfg.max_distance_px.max(0.0) } else { 0.0 };
    if max_distance_px > 0.0 {
        let max_d2 = max_distance_px * max_distance_px;
        active.retain(|candidate| candidate.dist2 <= max_d2);
        if active.is_empty() {
            return Ok((Vec::new(), 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0));
        }
    }

    let mut best = active[0];
    for candidate in active.into_iter().skip(1) {
        if candidate.dist2 < best.dist2
            || (candidate.dist2 == best.dist2 && candidate.area > best.area)
            || (candidate.dist2 == best.dist2 && candidate.area == best.area && candidate.det.id < best.det.id)
        {
            best = candidate;
        }
    }

    let fw = cfg.frame_width.max(0) as f64;
    let fh = cfg.frame_height.max(0) as f64;
    let tx_base = best.cx - chx;
    let ty_base = chy - best.cy;
    let tx_norm = if fw > 1.0 { tx_base / (fw * 0.5) } else { tx_base };
    let ty_norm = if fh > 1.0 { ty_base / (fh * 0.5) } else { ty_base };
    let tx = if fw > 1.0 && cfg.hfov_deg > 0.0 { tx_norm * (cfg.hfov_deg * 0.5) } else { tx_norm };
    let ty = if fh > 1.0 && cfg.vfov_deg > 0.0 { ty_norm * (cfg.vfov_deg * 0.5) } else { ty_norm };
    let ta = if fw > 1.0 && fh > 1.0 { (best.area / (fw * fh)) * 100.0 } else { best.area };
    let thor = (best.max_x - best.min_x).abs().max(1.0);
    let tvert = (best.max_y - best.min_y).abs().max(1.0);
    let (tshort, tlong) = detection_side_min_max(best.det);
    let distance_px = best.dist2.sqrt();

    Ok((vec![best.det.clone()], 1.0, best.det.id as f64, tx, ty, ta, distance_px, best.cx, best.cy, thor, tvert, tshort, tlong))
}

#[node(
    id = "merge_decode_stats_json",
    inputs(
        port(name = "inverted", source = "Json", ty = crate::daedalus_types::json_value()),
        port(name = "normal", source = "Json", ty = crate::daedalus_types::json_value())
    ),
    outputs(port(name = "json", source = "Json", ty = crate::daedalus_types::json_value()))
)]
fn cv_aruco_merge_decode_stats_json(inverted: String, normal: String) -> Result<String, NodeError> {
    fn parse(raw: &str) -> serde_json::Value {
        match serde_json::from_str::<serde_json::Value>(raw) {
            Ok(v) => v,
            Err(_) => serde_json::json!({ "raw": raw }),
        }
    }

    fn as_u64(v: Option<&serde_json::Value>) -> u64 {
        match v {
            Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(0),
            _ => 0,
        }
    }

    let inv = parse(&inverted);
    let norm = parse(&normal);

    let inv_stats = inv.get("stats").and_then(|v| v.as_object()).cloned().unwrap_or_default();
    let norm_stats = norm.get("stats").and_then(|v| v.as_object()).cloned().unwrap_or_default();

    let mut combined = serde_json::Map::new();
    let keys = [
        "inputQuads",
        "outputDetections",
        "decodedSampled",
        "decodedWarp",
        "verifyWarpAttempted",
        "verifyWarpPassed",
        "verifyWarpRejectMismatch",
        "verifyWarpRejectFailed",
        "sampledFailProjection",
        "sampledFailInvalidInput",
        "sampledFailLowContrast",
        "sampledFailBorderMismatch",
        "sampledFailHammingMarginTooLow",
        "sampledFailHammingTooHigh",
        "warpAttempted",
        "warpFailed",
        "warpFailProjection",
        "warpFailLowContrast",
        "warpFailGrid",
        "warpFailFamilyDecode",
    ];
    for key in keys {
        let a = as_u64(inv_stats.get(key));
        let b = as_u64(norm_stats.get(key));
        combined.insert(key.to_string(), serde_json::Value::Number((a + b).into()));
    }

    let out = serde_json::json!({
        "stats": {
            "inverted": inv_stats,
            "normal": norm_stats,
            "combined": combined,
        }
    });
    match serde_json::to_string(&out) {
        Ok(serialized) => Ok(serialized),
        Err(err) => Ok(serde_json::json!({
            "stats": { "combined": {} },
            "error": format!("merge_decode_stats_json serialization failed: {err}")
        })
        .to_string()),
    }
}

fn detection_center(det: &ArucoDetection2D) -> (f64, f64) {
    let mut sx = 0.0f64;
    let mut sy = 0.0f64;
    for p in det.corners {
        sx += p.x;
        sy += p.y;
    }
    (sx * 0.25, sy * 0.25)
}

fn detection_area(det: &ArucoDetection2D) -> f64 {
    let pts = det.corners;
    let mut sum = 0.0f64;
    for i in 0..4 {
        let j = (i + 1) & 3;
        sum += pts[i].x * pts[j].y - pts[j].x * pts[i].y;
    }
    0.5 * sum.abs()
}

fn detection_bbox(det: &ArucoDetection2D) -> (f64, f64, f64, f64) {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for p in det.corners {
        min_x = min_x.min(p.x);
        min_y = min_y.min(p.y);
        max_x = max_x.max(p.x);
        max_y = max_y.max(p.y);
    }
    (min_x, min_y, max_x, max_y)
}

fn detection_side_min_max(det: &ArucoDetection2D) -> (f64, f64) {
    let corners = det.corners;
    let mut min_side = f64::INFINITY;
    let mut max_side = 0.0f64;
    for i in 0..4usize {
        let a = corners[i];
        let b = corners[(i + 1) & 3];
        let dx = a.x - b.x;
        let dy = a.y - b.y;
        let len = (dx * dx + dy * dy).sqrt();
        min_side = min_side.min(len);
        max_side = max_side.max(len);
    }
    if !min_side.is_finite() { (0.0, 0.0) } else { (min_side, max_side) }
}
