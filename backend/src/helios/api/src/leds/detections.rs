//! The pipeline outputs the director reads: `detections` (the tags' corners, with the frame size)
//! and `multi_tag_pose` (the field pose's validity). Eidos's JSON: `detections[]` with `id` and
//! `corners` (four `{x, y}` in pixels), and `frame_width`/`frame_height`.

use serde_json::Value;

use super::bearing::bearing_deg;
use super::states::TagObs;
use crate::camera_context::CameraCalibration;

#[derive(Debug, Clone, PartialEq)]
pub struct Detected {
    pub id: u32,
    /// The mean of the four corners, pixels.
    pub centre: (f64, f64),
    /// The square root of the tag's quad area, pixels.
    pub side_px: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Detections {
    pub width: f64,
    pub height: f64,
    pub tags: Vec<Detected>,
}

pub fn parse_detections(value: &Value) -> Option<Detections> {
    let width = value.get("frame_width")?.as_f64().filter(|width| *width > 0.0)?;
    let height = value.get("frame_height")?.as_f64().filter(|height| *height > 0.0)?;
    let tags = value.get("detections")?.as_array()?.iter().filter_map(parse_tag).collect();
    Some(Detections { width, height, tags })
}

fn parse_tag(tag: &Value) -> Option<Detected> {
    let id = u32::try_from(tag.get("id")?.as_u64()?).ok()?;
    let corners = tag.get("corners")?.as_array()?;
    if corners.len() != 4 {
        return None;
    }
    let points = corners.iter().map(|corner| Some((corner.get("x")?.as_f64()?, corner.get("y")?.as_f64()?))).collect::<Option<Vec<_>>>()?;
    let centre = (points.iter().map(|p| p.0).sum::<f64>() / 4.0, points.iter().map(|p| p.1).sum::<f64>() / 4.0);
    // The shoelace formula over the quad's corners in order.
    let area = points.iter().zip(points.iter().cycle().skip(1)).map(|(a, b)| a.0 * b.1 - b.0 * a.1).sum::<f64>().abs() / 2.0;
    Some(Detected { id, centre, side_px: area.sqrt() })
}

/// The tags as the director sees them: bearing from the camera's forward axis, and size as a
/// fraction of the frame width.
pub fn tag_observations(detections: &Detections, calibration: Option<&CameraCalibration>, fov_deg: f64) -> Vec<TagObs> {
    detections
        .tags
        .iter()
        .map(|tag| TagObs { id: tag.id, bearing_deg: bearing_deg(tag.centre.0, tag.centre.1, detections.width, detections.height, calibration, fov_deg), size: tag.side_px / detections.width })
        .collect()
}

/// `multi_tag_pose`'s `valid`: a field pose solved from several tags.
pub fn multi_tag_valid(pose: &Value) -> bool {
    pose.get("valid").and_then(Value::as_bool).unwrap_or(false)
}

/// The calibration for a frame: one of the same size, else of the same aspect ratio (a
/// calibration at another resolution is scaled by the bearing maths).
pub fn pick_calibration(calibrations: &[CameraCalibration], width: f64, height: f64) -> Option<&CameraCalibration> {
    let exact = calibrations.iter().find(|c| f64::from(c.width) == width && f64::from(c.height) == height);
    exact.or_else(|| calibrations.iter().find(|c| (f64::from(c.width) / f64::from(c.height) - width / height).abs() < 0.01))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera_context::{Distortion, LensModel};
    use serde_json::json;

    fn square(id: u32, x: f64, y: f64, side: f64) -> Value {
        json!({ "id": id, "rotation": 0, "hamming_distance": 0, "corners": [
            { "x": x, "y": y }, { "x": x + side, "y": y }, { "x": x + side, "y": y + side }, { "x": x, "y": y + side }
        ] })
    }

    #[test]
    fn a_tags_centre_and_side_come_from_its_corners() {
        let value = json!({ "frame_width": 640, "frame_height": 400, "detections": [square(586, 300.0, 180.0, 40.0)] });
        let detections = parse_detections(&value).expect("detections");
        assert_eq!((detections.width, detections.height), (640.0, 400.0));
        let tag = &detections.tags[0];
        assert_eq!(tag.id, 586);
        assert_eq!(tag.centre, (320.0, 200.0));
        assert!((tag.side_px - 40.0).abs() < 1e-9);
    }

    #[test]
    fn a_malformed_tag_is_skipped_and_a_frame_without_a_size_is_refused() {
        let value = json!({ "frame_width": 640, "frame_height": 400, "detections": [
            { "id": 1, "corners": [{ "x": 1.0, "y": 1.0 }] },
            { "id": -4, "corners": [] },
            square(2, 0.0, 0.0, 10.0)
        ] });
        let detections = parse_detections(&value).expect("detections");
        assert_eq!(detections.tags.iter().map(|t| t.id).collect::<Vec<_>>(), vec![2]);
        assert!(parse_detections(&json!({ "detections": [] })).is_none());
        assert!(parse_detections(&json!({ "frame_width": 0, "frame_height": 4, "detections": [] })).is_none());
    }

    #[test]
    fn observations_use_the_size_fraction_and_the_bearing() {
        let value = json!({ "frame_width": 640, "frame_height": 400, "detections": [square(7, 480.0 - 20.0, 180.0, 40.0)] });
        let detections = parse_detections(&value).expect("detections");
        let tags = tag_observations(&detections, None, 90.0);
        assert_eq!(tags[0].id, 7);
        assert!((tags[0].bearing_deg - 22.5).abs() < 1e-9, "the centre is at x = 480 of 640: {}", tags[0].bearing_deg);
        assert!((tags[0].size - 40.0 / 640.0).abs() < 1e-9);
    }

    #[test]
    fn the_pose_is_valid_only_when_it_says_so() {
        assert!(multi_tag_valid(&json!({ "valid": true })));
        assert!(!multi_tag_valid(&json!({ "valid": false })));
        assert!(!multi_tag_valid(&json!({ "status": "uncalibrated" })));
    }

    #[test]
    fn a_calibration_of_the_same_size_or_aspect_is_picked() {
        let cal = |width, height| CameraCalibration {
            width,
            height,
            model: LensModel::Pinhole,
            fx: 1.0,
            fy: 1.0,
            cx: 0.0,
            cy: 0.0,
            distortion: Distortion::default(),
            rms_px: None,
            source: None,
            saved_at_ms: 0,
        };
        let calibrations = [cal(320, 200), cal(640, 400)];
        assert_eq!(pick_calibration(&calibrations, 640.0, 400.0).map(|c| c.width), Some(640));
        assert_eq!(pick_calibration(&calibrations, 1280.0, 800.0).map(|c| c.width), Some(320), "same aspect, scaled");
        assert!(pick_calibration(&calibrations, 640.0, 480.0).is_none());
    }
}
