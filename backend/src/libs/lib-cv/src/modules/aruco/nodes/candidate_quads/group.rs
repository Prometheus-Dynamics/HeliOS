use super::super::*;

use super::geom::{quad_average_corner_distance_sq, quad_from_cv, quad_perimeter, quad_to_cv, quad_too_near_border};

#[node(
    id = "candidate_quads_group",
    summary = "Group quads and prune near-border/near-duplicate candidates.",
    inputs(
        port(name = "contours", source = "Contours", ty = crate::daedalus_types::contours()),
        port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()),
        port(name = "downscale", default = 1i64, meta(ui_min = 1, ui_max = 8, ui_step = 1)),
        port(name = "min_distance_to_border_px", default = 3.0f64, meta(ui_min = 0.0, ui_max = 64.0, ui_step = 1.0)),
        port(name = "min_marker_distance_rate", default = 0.125f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01))
    ),
    outputs(port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()))
)]
pub(super) fn cv_aruco_candidate_quads_group(
    contours: &Vec<Vec<Point>>,
    quads: &Vec<Quad>,
    downscale: i64,
    min_distance_to_border_px: f64,
    min_marker_distance_rate: f64,
) -> Result<Vec<Quad>, NodeError> {
    if quads.is_empty() {
        return Ok(Vec::new());
    }

    let downscale = u32::try_from(downscale).unwrap_or(1).max(1);
    let scale = downscale as f32;
    let mut max_x = 0.0f64;
    let mut max_y = 0.0f64;
    for contour in contours {
        for p in contour {
            if p.x > max_x {
                max_x = p.x;
            }
            if p.y > max_y {
                max_y = p.y;
            }
        }
    }
    let max_dim = (max_x.max(max_y) + 1.0).max(0.0) as f32;
    let min_distance_to_border_px = min_distance_to_border_px.max(0.0) as f32;
    let min_marker_distance_rate = min_marker_distance_rate.max(0.0) as f32;

    let output = CANDIDATE_QUAD_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        let CandidateQuadScratch { quads: quad_buf, quad_centers, ordered_centers, quad_perimeters, order, min_marker_dist_sq, group_id, grouped, is_selected, keep_sorted, filtered, .. } =
            &mut *scratch;

        quad_buf.clear();
        quad_centers.clear();
        ordered_centers.clear();
        quad_perimeters.clear();
        filtered.clear();

        quad_buf.reserve(quads.len());
        quad_centers.reserve(quads.len());
        quad_perimeters.reserve(quads.len());
        for quad in quads.iter() {
            let quad_cv = quad_to_cv(quad);
            let center_x = 0.25 * (quad_cv[0].x + quad_cv[1].x + quad_cv[2].x + quad_cv[3].x);
            let center_y = 0.25 * (quad_cv[0].y + quad_cv[1].y + quad_cv[2].y + quad_cv[3].y);
            quad_centers.push(CvPoint::new(center_x, center_y));
            quad_perimeters.push(quad_perimeter(&quad_cv));
            quad_buf.push(quad_cv);
        }
        if downscale != 1 {
            for quad in quad_buf.iter_mut() {
                for corner in quad.iter_mut() {
                    corner.x *= scale;
                    corner.y *= scale;
                }
            }
            for perimeter in quad_perimeters.iter_mut() {
                *perimeter *= scale;
            }
            for center in quad_centers.iter_mut() {
                center.x *= scale;
                center.y *= scale;
            }
        }
        if !quad_buf.is_empty() {
            order.clear();
            order.extend(0..quad_buf.len());
            order.sort_by(|a, b| quad_perimeters[*b].total_cmp(&quad_perimeters[*a]));
            let max_x = max_x as f32;
            let max_y = max_y as f32;

            min_marker_dist_sq.clear();
            min_marker_dist_sq.reserve(order.len());
            if min_marker_distance_rate > 0.0 {
                for &idx in order.iter() {
                    let dist = quad_perimeters[idx] * min_marker_distance_rate;
                    min_marker_dist_sq.push(dist * dist);
                }
            } else {
                min_marker_dist_sq.resize(order.len(), 0.0);
            }
            ordered_centers.clear();
            ordered_centers.reserve(order.len());
            for &idx in order.iter() {
                ordered_centers.push(quad_centers[idx]);
            }

            group_id.clear();
            group_id.resize(order.len(), -1);
            is_selected.clear();
            is_selected.resize(order.len(), true);
            keep_sorted.clear();
            keep_sorted.resize(order.len(), false);
            for group in grouped.iter_mut() {
                group.clear();
            }
            let mut grouped_len = 0usize;

            for i in 0..order.len() {
                for j in (i + 1)..order.len() {
                    let idx_i = order[i];
                    let idx_j = order[j];
                    let cj = ordered_centers[j];
                    let ci = ordered_centers[i];
                    let cdx = ci.x - cj.x;
                    let cdy = ci.y - cj.y;
                    // Lower bound: average corner distance is always >= distance between quad centers.
                    // This cheap reject avoids the expensive corner matching for far-apart candidates.
                    let center_dist_sq = cdx * cdx + cdy * cdy;
                    if center_dist_sq >= min_marker_dist_sq[j] {
                        continue;
                    }
                    let dist_sq = quad_average_corner_distance_sq(&quad_buf[idx_i], &quad_buf[idx_j]);
                    if dist_sq < min_marker_dist_sq[j] {
                        is_selected[i] = false;
                        is_selected[j] = false;
                        if group_id[i] < 0 && group_id[j] < 0 {
                            let g = grouped_len;
                            if g == grouped.len() {
                                grouped.push(Vec::new());
                            }
                            let group = &mut grouped[g];
                            group.clear();
                            group.push(i);
                            group.push(j);
                            group_id[i] = g as isize;
                            group_id[j] = g as isize;
                            grouped_len += 1;
                        } else if group_id[i] >= 0 && group_id[j] < 0 {
                            let g = group_id[i] as usize;
                            group_id[j] = g as isize;
                            grouped[g].push(j);
                        } else if group_id[j] >= 0 && group_id[i] < 0 {
                            let g = group_id[j] as usize;
                            group_id[i] = g as isize;
                            grouped[g].push(i);
                        }
                    }
                }
                if is_selected[i] {
                    is_selected[i] = false;
                    let g = grouped_len;
                    if g == grouped.len() {
                        grouped.push(Vec::new());
                    }
                    let group = &mut grouped[g];
                    group.clear();
                    group.push(i);
                    group_id[i] = g as isize;
                    grouped_len += 1;
                }
            }

            for group in grouped.iter_mut().take(grouped_len) {
                group.sort_unstable();
                let curr = group[0];
                let quad = &quad_buf[order[curr]];
                if min_distance_to_border_px > 0.0 && max_dim > 0.0 && quad_too_near_border(quad, max_x, max_y, min_distance_to_border_px) {
                    continue;
                }
                keep_sorted[curr] = true;
            }

            filtered.clear();
            filtered.reserve(quad_buf.len());
            for (sorted_idx, keep) in keep_sorted.iter().copied().enumerate() {
                if keep {
                    filtered.push(quad_buf[order[sorted_idx]]);
                }
            }
            std::mem::swap(quad_buf, filtered);
        }

        let mut output = Vec::with_capacity(quad_buf.len());
        for quad in quad_buf.iter() {
            output.push(quad_from_cv(quad));
        }
        output
    });

    Ok(output)
}
