use super::*;

#[node(
    id = "scale_quads",
    summary = "Scale quad coordinates by a constant factor.",
    description = "Multiplies (or divides) all quad corner coordinates by the provided factor. Useful when running detection on a downscaled image but decoding/overlaying in the original coordinate space.",
    inputs(
        port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()),
        port(name = "factor", default = 1i64, meta(ui_min = 1, ui_max = 16, ui_step = 1)),
        port(name = "invert", default = false)
    ),
    outputs(port(name = "quads", source = "Quads", ty = crate::daedalus_types::quads()))
)]
fn cv_aruco_scale_quads(quads: &Vec<Quad>, factor: i64, invert: bool) -> Result<Vec<Quad>, NodeError> {
    if quads.is_empty() {
        return Ok(Vec::new());
    }
    let factor = (factor.max(1)) as f64;
    let scale = if invert { 1.0 / factor } else { factor };

    let mut out = Vec::with_capacity(quads.len());
    for q in quads {
        out.push([
            Point { x: q[0].x * scale, y: q[0].y * scale },
            Point { x: q[1].x * scale, y: q[1].y * scale },
            Point { x: q[2].x * scale, y: q[2].y * scale },
            Point { x: q[3].x * scale, y: q[3].y * scale },
        ]);
    }
    Ok(out)
}

#[node(
    id = "scale_detections",
    summary = "Scale ArUco detection corner coordinates by a constant factor.",
    description = "Multiplies (or divides) all detection corner coordinates by the provided factor. This is typically used to map detections from a downscaled working resolution back into the original frame coordinate space for overlays and downstream consumers.",
    inputs(
        port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()),
        port(name = "factor", default = 1i64, meta(ui_min = 1, ui_max = 16, ui_step = 1)),
        port(name = "invert", default = false)
    ),
    outputs(port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()))
)]
fn cv_aruco_scale_detections(detections: &Vec<ArucoDetection2D>, factor: i64, invert: bool) -> Result<Vec<ArucoDetection2D>, NodeError> {
    if detections.is_empty() {
        return Ok(Vec::new());
    }
    let factor = (factor.max(1)) as f64;
    let scale = if invert { 1.0 / factor } else { factor };

    let mut out: Vec<ArucoDetection2D> = Vec::with_capacity(detections.len());
    for d in detections {
        let mut dd = d.clone();
        dd.corners = [
            Point { x: dd.corners[0].x * scale, y: dd.corners[0].y * scale },
            Point { x: dd.corners[1].x * scale, y: dd.corners[1].y * scale },
            Point { x: dd.corners[2].x * scale, y: dd.corners[2].y * scale },
            Point { x: dd.corners[3].x * scale, y: dd.corners[3].y * scale },
        ];
        out.push(dd);
    }
    Ok(out)
}

#[node(
    id = "offset_detections",
    summary = "Translate ArUco detection corner coordinates by pixel offsets.",
    description = "Adds offset_x/offset_y to all detection corners, typically to map detections computed on an ROI crop back to full-frame coordinates for overlay/output.",
    inputs(
        port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()),
        port(name = "offset_x", default = 0i64, meta(ui_min = -8192, ui_max = 8192, ui_step = 1)),
        port(name = "offset_y", default = 0i64, meta(ui_min = -8192, ui_max = 8192, ui_step = 1))
    ),
    outputs(port(name = "detections", source = "ArucoDetections2D", ty = crate::daedalus_types::aruco_detections_2d()))
)]
fn cv_aruco_offset_detections(detections: &Vec<ArucoDetection2D>, offset_x: i64, offset_y: i64) -> Result<Vec<ArucoDetection2D>, NodeError> {
    if detections.is_empty() {
        return Ok(Vec::new());
    }
    if offset_x == 0 && offset_y == 0 {
        return Ok(detections.clone());
    }

    let ox = offset_x as f64;
    let oy = offset_y as f64;
    let mut out: Vec<ArucoDetection2D> = Vec::with_capacity(detections.len());
    for d in detections {
        let mut dd = d.clone();
        dd.corners = [
            Point { x: dd.corners[0].x + ox, y: dd.corners[0].y + oy },
            Point { x: dd.corners[1].x + ox, y: dd.corners[1].y + oy },
            Point { x: dd.corners[2].x + ox, y: dd.corners[2].y + oy },
            Point { x: dd.corners[3].x + ox, y: dd.corners[3].y + oy },
        ];
        out.push(dd);
    }
    Ok(out)
}
