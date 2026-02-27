use geo::{Coord, LineString, Polygon, algorithm::area::Area, algorithm::bounding_rect::BoundingRect};
use imageproc::point::Point;

/// Calculate the signed area of a polygon using the shoelace formula.
pub fn polygon_area(points: &[Point<f32>]) -> f32 {
    if points.len() < 3 {
        return 0.0;
    }
    let coords: Vec<Coord<f32>> = points.iter().map(|p| Coord { x: p.x, y: p.y }).collect();
    let poly = Polygon::new(LineString::from(coords), vec![]);
    poly.unsigned_area()
}

/// Calculate polygon area directly from lib-cv points (f64).
pub fn polygon_area_points(points: &[crate::Point]) -> f64 {
    if points.len() < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..points.len() {
        let j = (i + 1) % points.len();
        sum += points[i].x * points[j].y - points[j].x * points[i].y;
    }
    sum.abs() * 0.5
}

/// Compute an axis aligned bounding box for the provided points.
/// Returns `(min_x, min_y, width, height)`.
pub fn bounding_box(points: &[Point<u32>]) -> (u32, u32, u32, u32) {
    if points.is_empty() {
        return (0, 0, 0, 0);
    }
    let coords: Vec<Coord<u32>> = points.iter().map(|p| Coord { x: p.x, y: p.y }).collect();
    let ls: LineString<u32> = coords.into();
    if let Some(rect) = ls.bounding_rect() {
        let min = rect.min();
        let max = rect.max();
        (min.x, min.y, max.x - min.x + 1, max.y - min.y + 1)
    } else {
        (0, 0, 0, 0)
    }
}

/// Very small helper to classify polygons by number of vertices.
/// This is a naive approach but works for regular shapes.
pub fn classify_polygon_len(len: usize) -> &'static str {
    match len {
        3 => "triangle",
        4 => "quadrilateral",
        5 => "pentagon",
        6 => "hexagon",
        _ => {
            if len > 6 {
                "circle"
            } else {
                "unknown"
            }
        }
    }
}

/// Compute the perimeter length of a polygonal contour.
pub fn polygon_perimeter(points: &[Point<f32>]) -> f32 {
    if points.len() < 2 {
        return 0.0;
    }
    let mut sum = 0.0f32;
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        let dx = a.x - b.x;
        let dy = a.y - b.y;
        sum += (dx * dx + dy * dy).sqrt();
    }
    sum
}

/// Compute the perimeter for lib-cv points (f64) without temporary allocations.
pub fn polygon_perimeter_points(points: &[crate::Point]) -> f64 {
    if points.len() < 2 {
        return 0.0;
    }
    let mut sum = 0.0f64;
    for i in 0..points.len() {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        let dx = a.x - b.x;
        let dy = a.y - b.y;
        sum += (dx * dx + dy * dy).sqrt();
    }
    sum
}

/// Compute axis aligned bounding box for lib-cv points (rounded to pixels).
pub fn bounding_box_points(points: &[crate::Point]) -> (u32, u32, u32, u32) {
    if points.is_empty() {
        return (0, 0, 0, 0);
    }
    let mut min_x = u32::MAX;
    let mut min_y = u32::MAX;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    for p in points {
        let x = if p.x.is_finite() { p.x.round().clamp(0.0, u32::MAX as f64) as u32 } else { 0 };
        let y = if p.y.is_finite() { p.y.round().clamp(0.0, u32::MAX as f64) as u32 } else { 0 };
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    if min_x > max_x || min_y > max_y {
        return (0, 0, 0, 0);
    }
    (min_x, min_y, max_x - min_x + 1, max_y - min_y + 1)
}

#[cfg(feature = "engine")]
pub mod nodes {
    #![allow(clippy::ptr_arg)]

    use crate::Point;
    use crate::modules::analysis::{bounding_box_points, classify_polygon_len, polygon_area_points, polygon_perimeter_points};
    use daedalus::declare_plugin;
    use daedalus::macros::node;
    use daedalus::runtime::NodeError;
    use geo::{LineString, Polygon, algorithm::minimum_rotated_rect::MinimumRotatedRect};
    use serde::Serialize;

    #[derive(Serialize)]
    struct ContourBBoxJson {
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    }

    #[derive(Serialize)]
    struct ContourCentroidJson {
        x: f64,
        y: f64,
    }

    #[derive(Serialize)]
    struct ContourSummaryJson {
        index: usize,
        vertices: usize,
        shape: String,
        area: f64,
        perimeter: f64,
        bbox: ContourBBoxJson,
        centroid: ContourCentroidJson,
    }

    #[derive(Serialize)]
    struct ContoursJson {
        count: usize,
        truncated: bool,
        items: Vec<ContourSummaryJson>,
    }

    fn contour_centroid(contour: &[Point]) -> ContourCentroidJson {
        let mut sum_x = 0.0f64;
        let mut sum_y = 0.0f64;
        let mut count = 0.0f64;
        for p in contour {
            if p.x.is_finite() && p.y.is_finite() {
                sum_x += p.x;
                sum_y += p.y;
                count += 1.0;
            }
        }
        if count == 0.0 {
            return ContourCentroidJson { x: 0.0, y: 0.0 };
        }
        ContourCentroidJson { x: sum_x / count, y: sum_y / count }
    }

    #[node(id = "contour_area", inputs("contour"), outputs("area"))]
    fn cv_polygon_area(contour: &Vec<Point>) -> Result<f64, NodeError> {
        if contour.len() < 3 {
            return Ok(0.0);
        }
        Ok(polygon_area_points(contour))
    }

    #[node(id = "polygon_perimeter", inputs("contour"), outputs("perimeter"))]
    fn cv_polygon_perimeter(contour: &Vec<Point>) -> Result<f64, NodeError> {
        if contour.len() < 2 {
            return Ok(0.0);
        }
        Ok(polygon_perimeter_points(contour))
    }

    #[node(id = "bounding_box", inputs("contour"), outputs("x", "y", "width", "height"))]
    fn cv_bounding_box(contour: &Vec<Point>) -> Result<(u32, u32, u32, u32), NodeError> {
        if contour.is_empty() {
            return Err(NodeError::InvalidInput("bounding_box requires points".into()));
        }
        Ok(bounding_box_points(contour))
    }

    #[node(id = "classify_shape", inputs("contour"), outputs("shape"))]
    fn cv_classify_shape(contour: &Vec<Point>) -> Result<String, NodeError> {
        if contour.len() < 3 {
            return Err(NodeError::InvalidInput("classify_shape requires a polygon".into()));
        }
        Ok(classify_polygon_len(contour.len()).to_string())
    }

    #[node(id = "minarearect", inputs("contour"), outputs("rect"))]
    fn cv_min_area_rect(contour: &Vec<Point>) -> Result<Vec<Point>, NodeError> {
        if contour.len() < 3 {
            return Err(NodeError::InvalidInput("min_area_rect requires at least 3 points".into()));
        }
        let coords: Vec<(f32, f32)> = contour.iter().map(|p| (p.x as f32, p.y as f32)).collect();
        let poly = Polygon::new(LineString::from(coords), vec![]);
        if let Some(rect) = poly.minimum_rotated_rect() {
            let rect_points: Vec<Point> = rect.exterior().points().map(|c| Point { x: c.x() as f64, y: c.y() as f64 }).collect();
            Ok(rect_points)
        } else {
            Err(NodeError::InvalidInput("min_area_rect failed to compute".into()))
        }
    }

    #[node(
        id = "filter_contours",
        inputs("contours", port(name = "min", meta(ui_min = 0, ui_max = 5000, ui_step = 1)), port(name = "max", meta(ui_min = 0, ui_max = 5000, ui_step = 1))),
        outputs("contours")
    )]
    fn cv_filter_contours(contours: &Vec<Vec<Point>>, min: i64, max: i64) -> Result<Vec<Vec<Point>>, NodeError> {
        if contours.is_empty() {
            return Ok(Vec::new());
        }
        let min = min.clamp(0, u32::MAX as i64) as u32;
        let max = max.clamp(0, u32::MAX as i64) as u32;
        // Treat `0` as "no bound" to make graph tuning easier.
        if max != 0 && max < min {
            return Err(NodeError::InvalidInput("invalid min/max".into()));
        }
        if min == 0 && max == 0 {
            return Ok(contours.to_vec());
        }
        Ok(contours
            .iter()
            .filter(|c| {
                let n = c.len() as u32;
                n >= min && (max == 0 || n <= max)
            })
            .cloned()
            .collect())
    }

    #[node(
        id = "filter_contours_bbox",
        summary = "Filter contours by bounding box size.",
        description = "Keeps contours whose axis-aligned bounding box meets the provided minimum size/area constraints. Any parameter set to 0 disables that constraint.",
        inputs(
            port(name = "contours", source = "Contours", ty = crate::daedalus_types::contours()),
            port(name = "min_width", default = 0i64, meta(ui_min = 0, ui_max = 2000, ui_step = 1)),
            port(name = "min_height", default = 0i64, meta(ui_min = 0, ui_max = 2000, ui_step = 1)),
            port(name = "min_area", default = 0i64, meta(ui_min = 0, ui_max = 2000000, ui_step = 1000)),
            port(name = "max_area", default = 0i64, meta(ui_min = 0, ui_max = 2000000, ui_step = 1000))
        ),
        outputs(port(name = "contours", source = "Contours", ty = crate::daedalus_types::contours()))
    )]
    fn cv_filter_contours_bbox(contours: &Vec<Vec<Point>>, min_width: i64, min_height: i64, min_area: i64, max_area: i64) -> Result<Vec<Vec<Point>>, NodeError> {
        if contours.is_empty() {
            return Ok(Vec::new());
        }
        let min_width = min_width.clamp(0, u32::MAX as i64) as u32;
        let min_height = min_height.clamp(0, u32::MAX as i64) as u32;
        let min_area = min_area.clamp(0, i64::MAX) as u64;
        let max_area = max_area.clamp(0, i64::MAX) as u64;
        if max_area != 0 && max_area < min_area {
            return Err(NodeError::InvalidInput("invalid min_area/max_area".into()));
        }

        Ok(contours
            .iter()
            .filter(|contour| {
                if contour.is_empty() {
                    return false;
                }
                let mut min_x = f64::INFINITY;
                let mut min_y = f64::INFINITY;
                let mut max_x = f64::NEG_INFINITY;
                let mut max_y = f64::NEG_INFINITY;
                for p in contour.iter() {
                    if !p.x.is_finite() || !p.y.is_finite() {
                        continue;
                    }
                    min_x = min_x.min(p.x);
                    min_y = min_y.min(p.y);
                    max_x = max_x.max(p.x);
                    max_y = max_y.max(p.y);
                }
                if !min_x.is_finite() || !min_y.is_finite() || !max_x.is_finite() || !max_y.is_finite() {
                    return false;
                }
                let w = (max_x - min_x).abs().ceil().max(1.0) as u32;
                let h = (max_y - min_y).abs().ceil().max(1.0) as u32;
                if min_width != 0 && w < min_width {
                    return false;
                }
                if min_height != 0 && h < min_height {
                    return false;
                }
                let area = (w as u64).saturating_mul(h as u64);
                if min_area != 0 && area < min_area {
                    return false;
                }
                if max_area != 0 && area > max_area {
                    return false;
                }
                true
            })
            .cloned()
            .collect())
    }

    #[node(
        id = "contours_json",
        summary = "Summarize contours as JSON.",
        description = "Returns a JSON string with per-contour stats (area, perimeter, bbox, centroid, vertex count, shape).",
        inputs(
            port(name = "contours", source = "Contours", ty = crate::daedalus_types::contours()),
            port(name = "max_items", default = 50i64, meta(ui_min = 0, ui_max = 500, ui_step = 1))
        ),
        outputs(port(name = "json", source = "Json", ty = crate::daedalus_types::json_value()))
    )]
    fn cv_contours_json(contours: &Vec<Vec<Point>>, max_items: i64) -> Result<String, NodeError> {
        let limit = if max_items <= 0 { contours.len() } else { max_items as usize };
        let mut items = Vec::new();
        for (index, contour) in contours.iter().take(limit).enumerate() {
            let (x, y, width, height) = bounding_box_points(contour);
            let summary = ContourSummaryJson {
                index,
                vertices: contour.len(),
                shape: classify_polygon_len(contour.len()).to_string(),
                area: polygon_area_points(contour),
                perimeter: polygon_perimeter_points(contour),
                bbox: ContourBBoxJson { x, y, width, height },
                centroid: contour_centroid(contour),
            };
            items.push(summary);
        }
        let payload = ContoursJson { count: contours.len(), truncated: contours.len() > items.len(), items };
        match serde_json::to_string(&payload) {
            Ok(json) => Ok(json),
            Err(err) => Ok(serde_json::json!({
                "error": format!("contours_json serialization failed: {err}")
            })
            .to_string()),
        }
    }

    declare_plugin!(
        CvAnalysisPlugin,
        "analysis",
        [cv_polygon_area, cv_polygon_perimeter, cv_bounding_box, cv_classify_shape, cv_min_area_rect, cv_filter_contours, cv_filter_contours_bbox, cv_contours_json]
    );
}
