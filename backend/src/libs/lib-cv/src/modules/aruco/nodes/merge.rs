use super::*;

#[node(
    id = "merge_quads",
    inputs(
        port(name = "sources", source = "Quads", ty = crate::daedalus_types::quads()),
        port(name = "center_dist_px", default = 8.0f64, meta(ui_min = 0.0, ui_max = 50.0, ui_step = 0.5)),
        // Require candidates to be similar size to merge (smaller/larger). A value <= 0 disables this check.
        port(name = "min_area_ratio", default = 0.0f64, meta(ui_min = 0.0, ui_max = 2.0, ui_step = 0.05)),
        // Only merge near-identical quads. This prevents dropping detections when one polarity decodes but the other doesn't.
        port(name = "max_corner_dist_px", default = 2.5f64, meta(ui_min = 0.0, ui_max = 10.0, ui_step = 0.1))
    ),
    outputs(port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()))
)]
fn cv_aruco_merge_quads(sources: FanIn<Vec<Quad>>, center_dist_px: f64, min_area_ratio: f64, max_corner_dist_px: f64) -> Result<Vec<Quad>, NodeError> {
    #[inline(always)]
    fn quad_area(quad: &Quad) -> f64 {
        let mut area = 0.0f64;
        for i in 0..4 {
            let j = (i + 1) & 3;
            area += quad[i].x * quad[j].y - quad[j].x * quad[i].y;
        }
        area.abs() * 0.5
    }

    #[inline(always)]
    fn quad_center(quad: &Quad) -> (f64, f64) {
        let mut x = 0.0f64;
        let mut y = 0.0f64;
        for p in quad {
            x += p.x;
            y += p.y;
        }
        (x * 0.25, y * 0.25)
    }

    #[inline(always)]
    fn quad_min_avg_corner_dist2(a: &Quad, b: &Quad) -> f64 {
        let mut best = f64::INFINITY;
        for shift in 0..4usize {
            let mut sum = 0.0f64;
            for (i, point) in a.iter().enumerate().take(4) {
                let j = (i + shift) & 3;
                let dx = point.x - b[j].x;
                let dy = point.y - b[j].y;
                sum += dx * dx + dy * dy;
            }
            best = best.min(sum * 0.25);

            let mut sum_rev = 0.0f64;
            for (i, point) in a.iter().enumerate().take(4) {
                let j = (shift + 4 - i) & 3;
                let dx = point.x - b[j].x;
                let dy = point.y - b[j].y;
                sum_rev += dx * dx + dy * dy;
            }
            best = best.min(sum_rev * 0.25);
        }
        best
    }

    let dist = center_dist_px.max(0.0);
    let min_area_ratio = min_area_ratio.clamp(0.0, 1.0);
    if dist <= 0.0 {
        let mut out = Vec::new();
        for quads in sources.into_iter() {
            out.extend(quads);
        }
        return Ok(out);
    }

    let dist2 = dist * dist;
    let cell_size = dist.max(1.0);
    let max_corner_dist = max_corner_dist_px.max(0.0);
    let max_corner_dist2 = max_corner_dist * max_corner_dist;

    let sources = sources.into_vec();
    let total = sources.iter().map(|q| q.len()).sum();
    let mut out: Vec<Quad> = Vec::with_capacity(total);
    let mut centers: Vec<(f64, f64)> = Vec::with_capacity(total);
    let mut areas: Vec<f64> = Vec::with_capacity(total);
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();

    for quad in sources.into_iter().flatten() {
        let (cx, cy) = quad_center(&quad);
        let area = quad_area(&quad);
        let gx = (cx / cell_size).floor() as i32;
        let gy = (cy / cell_size).floor() as i32;

        let mut duplicate_index: Option<usize> = None;
        'cells: for yy in (gy - 1)..=(gy + 1) {
            for xx in (gx - 1)..=(gx + 1) {
                let Some(indices) = grid.get(&(xx, yy)) else { continue };
                for &idx in indices {
                    let (px, py) = centers[idx];
                    let dx = cx - px;
                    let dy = cy - py;
                    if dx * dx + dy * dy > dist2 {
                        continue;
                    }

                    if min_area_ratio > 0.0 {
                        let existing_area = areas[idx];
                        let larger = area.max(existing_area);
                        if larger > 0.0 {
                            let smaller = area.min(existing_area);
                            if (smaller / larger) < min_area_ratio {
                                continue;
                            }
                        }
                    }

                    if max_corner_dist2 > 0.0 && quad_min_avg_corner_dist2(&quad, &out[idx]) > max_corner_dist2 {
                        continue;
                    }

                    duplicate_index = Some(idx);
                    break 'cells;
                }
            }
        }

        if let Some(idx) = duplicate_index {
            if area > areas[idx] {
                out[idx] = quad;
                centers[idx] = (cx, cy);
                areas[idx] = area;
            }
            continue;
        }

        let idx = out.len();
        out.push(quad);
        centers.push((cx, cy));
        areas.push(area);
        grid.entry((gx, gy)).or_default().push(idx);
    }

    Ok(out)
}

#[node(
    id = "merge_quads_pair",
    summary = "Merge quads from exactly two sources.",
    description = "Like `merge_quads`, but accepts two explicit quad lists (a/b). This avoids relying on the current FanIn port metadata, which is not exposed to graph typechecking.",
    inputs(
        port(name = "a", source = "Quads", ty = crate::daedalus_types::quads()),
        port(name = "b", source = "Quads", ty = crate::daedalus_types::quads()),
        port(name = "center_dist_px", default = 8.0f64, meta(ui_min = 0.0, ui_max = 50.0, ui_step = 0.5)),
        port(name = "min_area_ratio", default = 0.0f64, meta(ui_min = 0.0, ui_max = 2.0, ui_step = 0.05)),
        port(name = "max_corner_dist_px", default = 2.5f64, meta(ui_min = 0.0, ui_max = 10.0, ui_step = 0.1))
    ),
    outputs(port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()))
)]
fn cv_aruco_merge_quads_pair(a: &Vec<Quad>, b: &Vec<Quad>, center_dist_px: f64, min_area_ratio: f64, max_corner_dist_px: f64) -> Result<Vec<Quad>, NodeError> {
    #[inline(always)]
    fn quad_area(quad: &Quad) -> f64 {
        let mut area = 0.0f64;
        for i in 0..4 {
            let j = (i + 1) & 3;
            area += quad[i].x * quad[j].y - quad[j].x * quad[i].y;
        }
        area.abs() * 0.5
    }

    #[inline(always)]
    fn quad_center(quad: &Quad) -> (f64, f64) {
        let mut x = 0.0f64;
        let mut y = 0.0f64;
        for p in quad {
            x += p.x;
            y += p.y;
        }
        (x * 0.25, y * 0.25)
    }

    #[inline(always)]
    fn quad_min_avg_corner_dist2(a: &Quad, b: &Quad) -> f64 {
        let mut best = f64::INFINITY;
        for shift in 0..4usize {
            let mut sum = 0.0f64;
            for (i, point) in a.iter().enumerate().take(4) {
                let j = (i + shift) & 3;
                let dx = point.x - b[j].x;
                let dy = point.y - b[j].y;
                sum += dx * dx + dy * dy;
            }
            best = best.min(sum * 0.25);

            let mut sum_rev = 0.0f64;
            for (i, point) in a.iter().enumerate().take(4) {
                let j = (shift + 4 - i) & 3;
                let dx = point.x - b[j].x;
                let dy = point.y - b[j].y;
                sum_rev += dx * dx + dy * dy;
            }
            best = best.min(sum_rev * 0.25);
        }
        best
    }

    let dist = center_dist_px.max(0.0);
    let min_area_ratio = min_area_ratio.clamp(0.0, 1.0);
    if dist <= 0.0 {
        let mut out = Vec::with_capacity(a.len() + b.len());
        out.extend(a.iter().copied());
        out.extend(b.iter().copied());
        return Ok(out);
    }

    let dist2 = dist * dist;
    let cell_size = dist.max(1.0);
    let max_corner_dist = max_corner_dist_px.max(0.0);
    let max_corner_dist2 = max_corner_dist * max_corner_dist;

    let total = a.len() + b.len();
    let mut out: Vec<Quad> = Vec::with_capacity(total);
    let mut centers: Vec<(f64, f64)> = Vec::with_capacity(total);
    let mut areas: Vec<f64> = Vec::with_capacity(total);
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();

    for quad in a.iter().chain(b.iter()) {
        let quad = *quad;
        let (cx, cy) = quad_center(&quad);
        let area = quad_area(&quad);
        let gx = (cx / cell_size).floor() as i32;
        let gy = (cy / cell_size).floor() as i32;

        let mut duplicate_index: Option<usize> = None;
        'cells: for yy in (gy - 1)..=(gy + 1) {
            for xx in (gx - 1)..=(gx + 1) {
                let Some(indices) = grid.get(&(xx, yy)) else { continue };
                for &idx in indices {
                    let (px, py) = centers[idx];
                    let dx = cx - px;
                    let dy = cy - py;
                    if dx * dx + dy * dy > dist2 {
                        continue;
                    }

                    if min_area_ratio > 0.0 {
                        let existing_area = areas[idx];
                        let larger = area.max(existing_area);
                        if larger > 0.0 {
                            let smaller = area.min(existing_area);
                            if (smaller / larger) < min_area_ratio {
                                continue;
                            }
                        }
                    }

                    if max_corner_dist2 > 0.0 && quad_min_avg_corner_dist2(&quad, &out[idx]) > max_corner_dist2 {
                        continue;
                    }

                    duplicate_index = Some(idx);
                    break 'cells;
                }
            }
        }

        if let Some(idx) = duplicate_index {
            if area > areas[idx] {
                out[idx] = quad;
                centers[idx] = (cx, cy);
                areas[idx] = area;
            }
            continue;
        }

        let idx = out.len();
        out.push(quad);
        centers.push((cx, cy));
        areas.push(area);
        grid.entry((gx, gy)).or_default().push(idx);
    }

    Ok(out)
}

#[node(
    id = "merge_detections",
    inputs(
        port(name = "sources", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d())
    ),
    outputs(port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()))
)]
fn cv_aruco_merge_detections(sources: FanIn<Vec<ArucoDetection2D>>) -> Result<Vec<ArucoDetection2D>, NodeError> {
    fn quad_area(quad: &[Point; 4]) -> f64 {
        let mut area = 0.0f64;
        for i in 0..4 {
            let j = (i + 1) % 4;
            area += quad[i].x * quad[j].y - quad[j].x * quad[i].y;
        }
        area.abs() * 0.5
    }

    let mut merged = std::collections::BTreeMap::<u32, ArucoDetection2D>::new();
    for det in sources.into_iter().flatten() {
        match merged.get(&det.id) {
            None => {
                merged.insert(det.id, det);
            }
            Some(existing) => {
                if quad_area(&det.corners) > quad_area(&existing.corners) {
                    merged.insert(det.id, det);
                }
            }
        }
    }

    Ok(merged.into_values().collect())
}

#[node(
    id = "merge_detections_pair",
    summary = "Merge detections from exactly two sources using spatial dedup.",
    description = "Like `merge_detections_spatial`, but accepts two explicit detection lists (a/b). This avoids relying on FanIn port metadata in graph JSON.",
    inputs(
        port(name = "a", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()),
        port(name = "b", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()),
        port(name = "center_dist_px", default = 8.0f64, meta(ui_min = 0.0, ui_max = 50.0, ui_step = 0.5)),
        // Require candidates to be similar size to merge (smaller/larger). A value <= 0 disables this check.
        port(name = "min_area_ratio", default = 0.0f64, meta(ui_min = 0.0, ui_max = 2.0, ui_step = 0.05)),
        // Only merge near-identical detections. This prevents dropping detections when one path decodes but the other doesn't.
        port(name = "max_corner_dist_px", default = 2.5f64, meta(ui_min = 0.0, ui_max = 10.0, ui_step = 0.1)),
        // Optional additional spatial check: require (center distance)^2 <= (max_center_dist_ratio^2) * min(area).
        // A value <= 0 disables this check.
        port(name = "max_center_dist_ratio", default = 0.0f64, meta(ui_min = 0.0, ui_max = 2.0, ui_step = 0.05)),
        // Optional additional spatial check: require quad intersection-over-union >= min_iou.
        // A value <= 0 disables this check.
        port(name = "min_iou", default = 0.0f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.05))
    ),
    outputs(port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()))
)]
fn cv_aruco_merge_detections_pair(
    a: &Vec<ArucoDetection2D>,
    b: &Vec<ArucoDetection2D>,
    center_dist_px: f64,
    min_area_ratio: f64,
    max_corner_dist_px: f64,
    max_center_dist_ratio: f64,
    min_iou: f64,
) -> Result<Vec<ArucoDetection2D>, NodeError> {
    Ok(merge_detections_spatial_impl(a.iter().cloned().chain(b.iter().cloned()), center_dist_px, min_area_ratio, max_corner_dist_px, max_center_dist_ratio, min_iou))
}

#[node(
    id = "merge_detections_spatial",
    inputs(
        port(name = "sources", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()),
        port(name = "center_dist_px", default = 8.0f64, meta(ui_min = 0.0, ui_max = 50.0, ui_step = 0.5)),
        // Require candidates to be similar size to merge (smaller/larger). A value <= 0 disables this check.
        port(name = "min_area_ratio", default = 0.0f64, meta(ui_min = 0.0, ui_max = 2.0, ui_step = 0.05)),
        // Only merge near-identical detections. This prevents dropping detections when one path decodes but the other doesn't.
        port(name = "max_corner_dist_px", default = 2.5f64, meta(ui_min = 0.0, ui_max = 10.0, ui_step = 0.1)),
        // Optional additional spatial check: require (center distance)^2 <= (max_center_dist_ratio^2) * min(area).
        // A value <= 0 disables this check.
        port(name = "max_center_dist_ratio", default = 0.0f64, meta(ui_min = 0.0, ui_max = 2.0, ui_step = 0.05)),
        // Optional additional spatial check: require quad intersection-over-union >= min_iou.
        // A value <= 0 disables this check.
        port(name = "min_iou", default = 0.0f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.05))
    ),
    outputs(port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()))
)]
fn cv_aruco_merge_detections_spatial(
    sources: FanIn<Vec<ArucoDetection2D>>,
    center_dist_px: f64,
    min_area_ratio: f64,
    max_corner_dist_px: f64,
    max_center_dist_ratio: f64,
    min_iou: f64,
) -> Result<Vec<ArucoDetection2D>, NodeError> {
    Ok(merge_detections_spatial_impl(sources.into_iter().flatten(), center_dist_px, min_area_ratio, max_corner_dist_px, max_center_dist_ratio, min_iou))
}
