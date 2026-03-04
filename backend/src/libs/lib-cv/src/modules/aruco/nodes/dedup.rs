use super::*;

#[node(
    id = "dedup_detections",
    inputs(
        port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()),
        port(name = "center_dist_px", default = 8.0f64, meta(ui_min = 0.0, ui_max = 50.0, ui_step = 0.5))
    ),
    outputs(port(name = "deduped", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()))
)]
fn cv_aruco_dedup_detections(detections: Option<Vec<ArucoDetection2D>>, center_dist_px: f64) -> Result<Vec<ArucoDetection2D>, NodeError> {
    fn quad_area(quad: &[Point; 4]) -> f64 {
        let mut area = 0.0f64;
        for i in 0..4 {
            let j = (i + 1) % 4;
            area += quad[i].x * quad[j].y - quad[j].x * quad[i].y;
        }
        area.abs() * 0.5
    }

    fn quad_center(quad: &[Point; 4]) -> (f64, f64) {
        let mut x = 0.0f64;
        let mut y = 0.0f64;
        for p in quad {
            x += p.x;
            y += p.y;
        }
        (x * 0.25, y * 0.25)
    }

    let detections = detections.unwrap_or_default();
    let dist = center_dist_px.max(0.0);
    if detections.len() <= 1 || dist <= 0.0 {
        return Ok(detections);
    }
    let dist2 = dist * dist;

    let mut by_id: std::collections::HashMap<u32, Vec<ArucoDetection2D>> = std::collections::HashMap::new();
    for det in &detections {
        by_id.entry(det.id).or_default().push(det.clone());
    }

    let mut out: Vec<ArucoDetection2D> = Vec::with_capacity(by_id.len());
    for (_id, mut group) in by_id {
        group.sort_by(|a, b| quad_area(&b.corners).total_cmp(&quad_area(&a.corners)));
        let mut kept: Vec<ArucoDetection2D> = Vec::new();
        for det in group {
            let (cx, cy) = quad_center(&det.corners);
            let mut dup = false;
            for k in &kept {
                let (kx, ky) = quad_center(&k.corners);
                let dx = cx - kx;
                let dy = cy - ky;
                if dx * dx + dy * dy <= dist2 {
                    dup = true;
                    break;
                }
            }
            if !dup {
                kept.push(det);
            }
        }
        out.extend(kept);
    }

    Ok(out)
}

#[derive(Clone, Debug, NodeConfig)]
struct ArucoDedupSpatialConfig {
    #[port(default = 8.0f64, meta(ui_min = 0.0, ui_max = 50.0, ui_step = 0.5))]
    center_dist_px: f64,
    // Require candidates to be similar size to merge (smaller/larger). A value <= 0 disables this check.
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 2.0, ui_step = 0.05))]
    min_area_ratio: f64,
    // Only merge near-identical detections. This prevents dropping detections when one path decodes but the other doesn't.
    #[port(default = 2.5f64, meta(ui_min = 0.0, ui_max = 10.0, ui_step = 0.1))]
    max_corner_dist_px: f64,
    // Optional additional spatial check: require (center distance)^2 <= (max_center_dist_ratio^2) * min(area).
    // A value <= 0 disables this check.
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 2.0, ui_step = 0.05))]
    max_center_dist_ratio: f64,
    // Optional additional spatial check: require quad intersection-over-union >= min_iou.
    // A value <= 0 disables this check.
    #[port(default = 0.0f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.05))]
    min_iou: f64,
    // Optional post-pass: drop smaller detections fully nested inside larger detections,
    // even when marker IDs differ (common ghost decode pattern).
    #[port(default = false)]
    suppress_nested: bool,
    #[port(default = 0.9f64, meta(ui_min = 0.1, ui_max = 1.0, ui_step = 0.01))]
    nested_area_ratio_max: f64,
}

#[node(
    id = "dedup_detections_spatial",
    inputs(
        port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()),
        config = ArucoDedupSpatialConfig
    ),
    outputs(port(name = "deduped", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()))
)]
fn cv_aruco_dedup_detections_spatial(detections: Option<Vec<ArucoDetection2D>>, cfg: ArucoDedupSpatialConfig) -> Result<Vec<ArucoDetection2D>, NodeError> {
    let detections = detections.unwrap_or_default();
    let merged = merge_detections_spatial_impl(detections, cfg.center_dist_px, cfg.min_area_ratio, cfg.max_corner_dist_px, cfg.max_center_dist_ratio, cfg.min_iou);
    if !cfg.suppress_nested {
        return Ok(merged);
    }
    Ok(suppress_nested_detections(merged, cfg.nested_area_ratio_max))
}

fn suppress_nested_detections(detections: Vec<ArucoDetection2D>, nested_area_ratio_max: f64) -> Vec<ArucoDetection2D> {
    fn ordered_quad(corners: &[Point; 4]) -> [Point; 4] {
        let mut ordered = *corners;
        let (cx, cy) = quad_center(corners);
        ordered.sort_by(|a, b| (a.y - cy).atan2(a.x - cx).total_cmp(&(b.y - cy).atan2(b.x - cx)));
        ordered
    }

    fn quad_area(quad: &[Point; 4]) -> f64 {
        let ordered = ordered_quad(quad);
        let mut area = 0.0f64;
        for i in 0..4 {
            let j = (i + 1) % 4;
            area += ordered[i].x * ordered[j].y - ordered[j].x * ordered[i].y;
        }
        area.abs() * 0.5
    }

    fn quad_center(quad: &[Point; 4]) -> (f64, f64) {
        let mut x = 0.0f64;
        let mut y = 0.0f64;
        for p in quad {
            x += p.x;
            y += p.y;
        }
        (x * 0.25, y * 0.25)
    }

    fn point_in_quad(px: f64, py: f64, corners: &[Point; 4]) -> bool {
        let ordered = ordered_quad(corners);

        let mut sign = 0i8;
        for i in 0..4 {
            let a = ordered[i];
            let b = ordered[(i + 1) & 3];
            let cross = (b.x - a.x) * (py - a.y) - (b.y - a.y) * (px - a.x);
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

    if detections.len() < 2 {
        return detections;
    }

    let ratio_max = nested_area_ratio_max.clamp(0.1, 1.0);
    let areas: Vec<f64> = detections.iter().map(|det| quad_area(&det.corners)).collect();
    let centers: Vec<(f64, f64)> = detections.iter().map(|det| quad_center(&det.corners)).collect();
    let mut keep = vec![true; detections.len()];

    for i in 0..detections.len() {
        if !keep[i] {
            continue;
        }
        let area_i = areas[i];
        let (cx, cy) = centers[i];
        for j in 0..detections.len() {
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

            let corners_inside = detections[i].corners.iter().filter(|p| point_in_quad(p.x, p.y, &detections[j].corners)).count();
            let center_inside = point_in_quad(cx, cy, &detections[j].corners);
            if corners_inside < 3 && !(corners_inside >= 2 && center_inside) {
                continue;
            }

            keep[i] = false;
            break;
        }
    }

    detections.into_iter().zip(keep).filter_map(|(det, keep)| if keep { Some(det) } else { None }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn det(id: u32, corners: [Point; 4]) -> ArucoDetection2D {
        ArucoDetection2D {
            id,
            rotation: 0,
            corners,
            score: None,
            best_distance: None,
            second_distance: None,
            border_mismatches: None,
            contrast_range: None,
            border_width: None,
            data_width: None,
            bits: None,
        }
    }

    #[test]
    fn nested_suppression_drops_inner_different_id() {
        let outer = det(1, [Point { x: 0.0, y: 0.0 }, Point { x: 100.0, y: 0.0 }, Point { x: 100.0, y: 100.0 }, Point { x: 0.0, y: 100.0 }]);
        let inner = det(42, [Point { x: 20.0, y: 20.0 }, Point { x: 70.0, y: 20.0 }, Point { x: 70.0, y: 70.0 }, Point { x: 20.0, y: 70.0 }]);
        let out = suppress_nested_detections(vec![outer.clone(), inner], 0.9);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, outer.id);
    }

    #[test]
    fn nested_suppression_handles_unordered_outer_corners() {
        // This ordering is intentionally non-cyclic; containment still needs to work.
        let outer = det(7, [Point { x: 0.0, y: 0.0 }, Point { x: 100.0, y: 100.0 }, Point { x: 100.0, y: 0.0 }, Point { x: 0.0, y: 100.0 }]);
        let inner = det(8, [Point { x: 25.0, y: 25.0 }, Point { x: 60.0, y: 25.0 }, Point { x: 60.0, y: 60.0 }, Point { x: 25.0, y: 60.0 }]);
        let out = suppress_nested_detections(vec![outer.clone(), inner], 0.95);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, outer.id);
    }

    #[test]
    fn nested_suppression_keeps_adjacent_tags() {
        let left = det(11, [Point { x: 0.0, y: 0.0 }, Point { x: 40.0, y: 0.0 }, Point { x: 40.0, y: 40.0 }, Point { x: 0.0, y: 40.0 }]);
        let right = det(12, [Point { x: 45.0, y: 0.0 }, Point { x: 85.0, y: 0.0 }, Point { x: 85.0, y: 40.0 }, Point { x: 45.0, y: 40.0 }]);
        let out = suppress_nested_detections(vec![left.clone(), right.clone()], 0.95);
        assert_eq!(out.len(), 2);
        assert!(out.iter().any(|d| d.id == left.id));
        assert!(out.iter().any(|d| d.id == right.id));
    }
}
