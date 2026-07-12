use super::super::*;

use super::geom::quad_area_and_perimeter_f64;

#[node(
    id = "candidate_quads_filter_area",
    summary = "Filter quad candidates by area and perimeter bounds.",
    inputs(
        port(name = "contours", source = "Contours", ty = crate::daedalus_types::contours()),
        port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()),
        port(name = "downscale", default = 1i64, meta(ui_min = 1, ui_max = 8, ui_step = 1)),
        port(name = "min_area", default = 0.0f64, meta(ui_min = 0.0, ui_max = 1000000.0, ui_step = 1000.0)),
        port(name = "max_area", default = 0.0f64, meta(ui_min = 0.0, ui_max = 1000000.0, ui_step = 1000.0)),
        port(name = "min_perimeter_rate", default = 0.03f64, meta(ui_min = 0.0, ui_max = 1.0, ui_step = 0.01)),
        port(name = "max_perimeter_rate", default = 4.0f64, meta(ui_min = 0.0, ui_max = 10.0, ui_step = 0.1))
    ),
    outputs(port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()))
)]
pub(super) fn cv_aruco_candidate_quads_filter_area(
    contours: &Vec<Vec<Point>>,
    quads: &Vec<Quad>,
    downscale: i64,
    min_area: f64,
    max_area: f64,
    min_perimeter_rate: f64,
    max_perimeter_rate: f64,
) -> Result<Vec<Quad>, NodeError> {
    if quads.is_empty() {
        return Ok(Vec::new());
    }
    let downscale = u32::try_from(downscale).unwrap_or(1).max(1);
    let scale = downscale as f32;
    let area_scale = (scale * scale).max(1.0);
    let area_scale = area_scale as f64;
    let min_area = min_area.max(0.0) / area_scale;
    let max_area = if max_area > 0.0 { Some(max_area / area_scale) } else { None };
    let min_perimeter_rate = min_perimeter_rate.max(0.0);
    let max_perimeter_rate = max_perimeter_rate.max(0.0);

    let mut max_x = 0.0f64;
    let mut max_y = 0.0f64;
    for contour in contours {
        for p in contour {
            max_x = max_x.max(p.x);
            max_y = max_y.max(p.y);
        }
    }
    let max_dim = (max_x.max(max_y) + 1.0).max(0.0);
    let min_perimeter_rate_px = if max_dim > 0.0 && min_perimeter_rate > 0.0 { Some(min_perimeter_rate * max_dim) } else { None };
    let max_perimeter_rate_px = if max_dim > 0.0 && max_perimeter_rate > 0.0 { Some(max_perimeter_rate * max_dim) } else { None };
    let min_perimeter = if min_area > f64::EPSILON { Some((4.0 * std::f64::consts::PI * min_area).sqrt()) } else { None };
    let max_step = std::f64::consts::SQRT_2;

    let output = CANDIDATE_QUAD_SCRATCH.with(|scratch| {
        let mut scratch = scratch.borrow_mut();
        let filtered_quads = &mut scratch.filtered_quads;
        filtered_quads.clear();

        for quad in quads {
            let (area, perimeter) = quad_area_and_perimeter_f64(quad);
            if area < min_area {
                continue;
            }
            if let Some(max_area) = max_area
                && area > max_area
            {
                continue;
            }
            if let Some(min_rate_px) = min_perimeter_rate_px
                && perimeter < min_rate_px
            {
                continue;
            }
            if let Some(max_rate_px) = max_perimeter_rate_px
                && perimeter > max_rate_px
            {
                continue;
            }
            if let Some(min_perimeter) = min_perimeter
                && perimeter * max_step < min_perimeter
            {
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
