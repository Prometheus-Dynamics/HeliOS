//! The live-map template (`eidos:mapping.live_map`): the tracked AprilTag detector feeds the live
//! mapper, which gives the camera pose, the map snapshot and the timing as host outputs. The
//! per-tick map delta is not a host output (the host keeps only the latest value per port, so
//! the deltas of a tick the host did not read would be lost).

use daedalus::{
    data::{model::Value, to_value::ToValue},
    runtime::{graph_builder::GraphBuilder, handles::PortHandle, plugins::PluginRegistry},
};
use eidos_daedalus::types::{CameraCalibration, CameraExtrinsics};

use super::*;

/// The tracked detector node (`eidos:detectors.tracked_detect`).
const TRACKED_DETECT_NODE: &str = "eidos:detectors.tracked_detect";
/// The live mapper node (`eidos:mapping.live_map`).
const LIVE_MAP_NODE: &str = "eidos:mapping.live_map";

/// The live-map graph as a stored document: the host's `frame` feeds the detector and the
/// mapper; the detector's `detections`, `rejected` and `refined_corners` feed the mapper. The
/// camera calibration and the extrinsics are held host inputs (the camera's context), as in the
/// AprilTag templates. The field `layout` is left unconnected: a layout makes the mapper wait for
/// a known tag, and without one it starts on the first clear tag (the map's reference frame).
/// Host outputs: `pose`, `snapshot`, `timing` and the detector's `detections`.
pub(super) fn live_map_template(registry: &PluginRegistry) -> String {
    let mut document = registry.graph_document(live_map_builder(registry).build());
    document.metadata.insert("title".into(), "HeliOS live map".into());
    stored_form(&document)
}

/// The live map graph before it is built: the detector (`detector`) and the mapper (`mapper`)
/// with their host inputs and outputs, for templates that add to it (the dense map).
pub(super) fn live_map_builder(registry: &PluginRegistry) -> GraphBuilder {
    let detector = PortHandle::new("detector", "frame");
    let mapper = |port: &str| PortHandle::new("mapper", port);
    let builder = registry
        .graph_builder()
        .expect("graph builder")
        .input_typed::<FrameLease>("frame")
        .expect("frame input")
        .input_typed::<CameraCalibration>("camera")
        .expect("camera input")
        .held_input("camera")
        .input_typed::<CameraExtrinsics>("extrinsics")
        .expect("extrinsics input")
        .held_input("extrinsics")
        .try_node_id(TRACKED_DETECT_NODE, "detector")
        .expect("tracked detector node")
        .try_node_id(LIVE_MAP_NODE, "mapper")
        .expect("live map node")
        .try_connect("frame", &detector)
        .expect("frame into the detector")
        .try_connect("frame", &mapper("frame"))
        .expect("frame into the mapper")
        .try_connect(&PortHandle::new("detector", "detections"), &mapper("detections"))
        .expect("detections into the mapper")
        .try_connect(&PortHandle::new("detector", "rejected"), &mapper("rejected"))
        .expect("rejected into the mapper")
        .try_connect(&PortHandle::new("detector", "refined_corners"), &mapper("refined_corners"))
        .expect("refined corners into the mapper")
        .try_connect("camera", &mapper("calibration"))
        .expect("camera into the mapper")
        .try_connect("extrinsics", &mapper("extrinsics"))
        .expect("extrinsics into the mapper")
        .try_connect(&PortHandle::new("detector", "detections"), "detections")
        .expect("detections output")
        .try_connect(&mapper("pose"), "pose")
        .expect("pose output")
        .try_connect(&mapper("snapshot"), "snapshot")
        .expect("snapshot output")
        .try_connect(&mapper("timing"), "timing")
        .expect("timing output");
    detector_settings(builder)
}

/// The detector's settings: the AprilTag 36h11 family with HeliOS's tracking (full search every
/// 8th frame, no escalation of a missed track, window growth over 6 misses: the same as the
/// AprilTag templates). The mapper's own settings are the node defaults, with the FRC tag size.
fn detector_settings(builder: GraphBuilder) -> GraphBuilder {
    let set = |builder: GraphBuilder, port: &str, value: Value| builder.const_input(&PortHandle::new("detector", port), Some(value));
    let builder = set(builder, "dictionary", Dictionary::AprilTag36h11.to_value());
    let builder = set(builder, "full_search_every", Value::Int(8));
    let builder = set(builder, "loss_full_search_after", Value::Int(0));
    set(builder, "margin_growth_misses", Value::Int(6))
}

/// Camera of the synthetic scenes: 640 x 480, 500 px focal, the principal point at the centre.
pub(super) const SCENE_FOCAL_PX: f64 = 500.0;
/// Side of the synthetic marker, metres.
const SCENE_TAG_SIDE_M: f64 = 0.2;

/// A 3x3 matrix times a homogeneous point.
fn mul3(m: &[[f64; 3]; 3], p: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|row| m[row][0] * p[0] + m[row][1] * p[1] + m[row][2] * p[2])
}

/// The inverse of a 3x3 matrix (cofactors).
fn invert3(m: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let mut inverse = [[0.0; 3]; 3];
    for (row, out_row) in inverse.iter_mut().enumerate() {
        for (col, out) in out_row.iter_mut().enumerate() {
            let (r0, r1) = ((col + 1) % 3, (col + 2) % 3);
            let (c0, c1) = ((row + 1) % 3, (row + 2) % 3);
            *out = (m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0]) / det;
        }
    }
    inverse
}

/// The image-from-tag homography of the marker at `pose_index`: the tag is turned and tilted a
/// little more each frame, so the views the self-calibration fits differ in perspective.
fn tag_homography(pose_index: u64) -> [[f64; 3]; 3] {
    let t = pose_index as f64;
    let (rx, ry, rz) = (0.3 * (t * 0.05).sin(), 0.5 + 0.1 * (t * 0.07).sin(), 0.4 * (t * 0.03).sin());
    let (sx, sy, sz) = (rx.sin_cos(), ry.sin_cos(), rz.sin_cos());
    // Rotation R = Rz * Ry * Rx; only its first two columns are needed for a plane.
    let r = [
        [sz.1 * sy.1, sz.1 * sy.0 * sx.0 - sz.0 * sx.1, sz.1 * sy.0 * sx.1 + sz.0 * sx.0],
        [sz.0 * sy.1, sz.0 * sy.0 * sx.0 + sz.1 * sx.1, sz.0 * sy.0 * sx.1 - sz.1 * sx.0],
        [-sy.0, sy.1 * sx.0, sy.1 * sx.1],
    ];
    let depth = 0.9 + 0.15 * (t * 0.02).sin();
    let translation = [0.03 * (t * 0.04).sin(), 0.02 * (t * 0.06).cos(), depth];
    let focal = SCENE_FOCAL_PX;
    let k = [[focal, 0.0, 320.0], [0.0, focal, 240.0], [0.0, 0.0, 1.0]];
    let half = SCENE_TAG_SIDE_M / 2.0;
    // Columns: r1 * X + r2 * Y + t, with X and Y in units of the half side (the tag spans -1..1).
    let columns = [[r[0][0] * half, r[1][0] * half, r[2][0] * half], [r[0][1] * half, r[1][1] * half, r[2][1] * half], translation];
    let mut h = [[0.0; 3]; 3];
    for row in 0..3 {
        for col in 0..3 {
            h[row][col] = 0.0;
            for (k_index, k_row) in k[row].iter().enumerate() {
                h[row][col] += k_row * columns[col][k_index];
            }
        }
    }
    h
}

/// A GREY frame of 640 x 480 with the AprilTag 36h11 marker `id`, seen through `tag_homography`
/// of `pose_index`, over a flat grey background with a little hashed texture for the tracker to
/// follow.
pub(super) fn live_map_scene(id: u32, pose_index: u64, timestamp: u64) -> FrameLease {
    let (width, height) = (640_usize, 480_usize);
    let mut pixels = vec![0_u8; width * height];
    for y in 0..height {
        for x in 0..width {
            let hash = (x as u32).wrapping_mul(73_856_093) ^ (y as u32).wrapping_mul(19_349_663);
            pixels[y * width + x] = 180 + (hash % 17) as u8;
        }
    }
    let dictionary = dictionary(ArucoDictionaryKind::AprilTag36h11);
    let grid = BitGrid::from_marker(dictionary, id, 0).expect("marker bits");
    let cells = usize::from(dictionary.marker_size) + 2;
    let h = tag_homography(pose_index);
    let inverse = invert3(&h);
    // The marker's bounding box in the image, from its four projected corners.
    let corners = [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]].map(|[u, v]| {
        let p = mul3(&h, [u, v, 1.0]);
        [p[0] / p[2], p[1] / p[2]]
    });
    let min_x = corners.iter().map(|c| c[0]).fold(f64::INFINITY, f64::min).floor().max(0.0) as usize;
    let max_x = corners.iter().map(|c| c[0]).fold(f64::NEG_INFINITY, f64::max).ceil().min(width as f64 - 1.0) as usize;
    let min_y = corners.iter().map(|c| c[1]).fold(f64::INFINITY, f64::min).floor().max(0.0) as usize;
    let max_y = corners.iter().map(|c| c[1]).fold(f64::NEG_INFINITY, f64::max).ceil().min(height as f64 - 1.0) as usize;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let q = mul3(&inverse, [x as f64 + 0.5, y as f64 + 0.5, 1.0]);
            let (u, v) = ((q[0] / q[2] + 1.0) / 2.0, (q[1] / q[2] + 1.0) / 2.0);
            if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                continue;
            }
            let (cx, cy) = ((u * cells as f64) as usize, (v * cells as f64) as usize);
            let border = cx == 0 || cy == 0 || cx == cells - 1 || cy == cells - 1;
            pixels[y * width + x] = if !border && grid.bit(cx - 1, cy - 1) { 235 } else { 20 };
        }
    }
    grey_frame(width as u32, height as u32, &pixels, timestamp)
}

/// Parses the host output `kind` as JSON and checks the documented top-level keys.
pub(super) fn json_with_keys(artifacts: &[ExecutionArtifactRecord], kind: &str, keys: &[&str]) -> serde_json::Value {
    let value = artifact_json(artifacts, kind);
    for key in keys {
        assert!(value.get(*key).is_some(), "{kind} has `{key}`: {value}");
    }
    value
}

/// The live-map template run by the Eidos plugin library through the frame driver: a marker in
/// the scene is detected, the mapper gives a pose each tick with its documented shape, the
/// timing has per-stage milliseconds, and the snapshot (at most 2 Hz on the frame timestamps)
/// carries the points as packed little-endian byte arrays with their counts and bounds.
#[test]
fn live_map_graph_publishes_pose_snapshot_and_timing_through_the_frame_driver() {
    let plugins = TestPlugins::load();
    let document = live_map_template(plugins.registry());
    // An uncalibrated camera (fx 0) whose lens is known to be a pinhole, as the synthetic scene
    // is: the mapper self-calibrates with that model (Eidos's `auto` takes the hint).
    let uncalibrated = calibration_context(0.0, "pinhole");
    let mut workload_spec = workload("workload.live-map", document, vec![camera_binding(&uncalibrated)]);
    workload_spec.plugin_requirements = vec![PluginRequirement { plugin_name: "eidos".into(), version: None }];
    let spec = workload_spec.clone();
    let (mut driver, frames) = spawn_channel_driver(&plugins, workload_spec, "frame", GraphSettings::default());
    driver.update_camera_context(&spec);

    // The default calibration window is 150 frames; the mapper then starts on the first clear tag
    // (the tilted marker, which pins its pose). Timestamps are nanoseconds, 100 ms apart.
    let frame_count = 160_u64;
    for index in 0..frame_count {
        frames.send(live_map_scene(586, index, (index + 1) * 100_000_000)).expect("send frame");
        wait_for("the frame to be processed", || driver.stats().ticks_processed == index + 1);
    }
    let (session, artifacts) = driver.snapshot();
    assert_eq!(session.status, ExecutionSessionStatus::Running, "{:?}", session.message);

    let pose = json_with_keys(&artifacts, "host_output:pose", &["frame", "t_ns", "state", "reason", "valid", "pose", "calibration", "alignment"]);
    assert_eq!(pose["frame"].as_u64(), Some(frame_count - 1), "{pose}");
    assert_eq!(pose["state"], "tracking", "{pose}");
    assert_eq!(pose["valid"], true, "{pose}");
    assert_eq!(pose["calibration"]["estimated"], true, "{pose}");
    assert!(pose["alignment"]["aligned"].is_boolean(), "{pose}");
    let estimate = &pose["pose"];
    for key in ["reference_from_camera", "covariance", "sigma", "source", "rms_px", "points", "hypotheses", "rig"] {
        assert!(estimate.get(key).is_some(), "pose.pose has `{key}`: {pose}");
    }
    assert_eq!(estimate["covariance"].as_array().map(Vec::len), Some(36), "{pose}");
    assert_eq!(estimate["sigma"]["position_m"].as_array().map(Vec::len), Some(3), "{pose}");
    assert_eq!(estimate["sigma"]["rotation_deg"].as_array().map(Vec::len), Some(3), "{pose}");
    assert!(estimate["reference_from_camera"]["rotation"]["w"].is_number(), "{pose}");

    let timing = json_with_keys(&artifacts, "host_output:timing", &["frame", "t_ns", "frame_ms", "map_ms", "stages"]);
    assert!(timing["frame_ms"].as_f64().is_some_and(|ms| ms >= 0.0), "{timing}");
    assert!(timing["stages"].as_array().is_some_and(|stages| !stages.is_empty() && stages.iter().all(|stage| stage["name"].is_string() && stage["ms"].is_number())), "{timing}");

    let detections = artifact_json(&artifacts, "host_output:detections");
    assert!(detections["detections"].as_array().is_some_and(|list| list.len() == 1 && list[0]["id"] == 586), "the scene's marker is detected: {detections}");

    // The snapshot is due on the first mapping tick and then at most twice a second: it exists
    // with the tag and the trajectory, and its point buffers are packed as documented.
    let snapshot = json_with_keys(&artifacts, "host_output:snapshot", &["seq", "run_id", "tag_size_m", "aligned", "estimated", "trajectory", "keyframes", "points", "tags"]);
    assert_eq!(snapshot["estimated"], true, "{snapshot}");
    assert!(snapshot["trajectory"].as_array().is_some_and(|trajectory| !trajectory.is_empty()), "{snapshot}");
    assert!(snapshot["tags"].as_array().is_some_and(|tags| tags.iter().any(|tag| tag["id"] == 586)), "{snapshot}");
    let points = &snapshot["points"];
    let count = points["count"].as_u64().expect("point count") as usize;
    for key in ["min_xyz", "max_xyz"] {
        assert_eq!(points[key].as_array().map(Vec::len), Some(3), "{points}");
    }
    // Byte buffers come out of the host as base64 strings (`to_plain_json_with`, Base64): ids u32
    // LE, xyz three f32 LE, observations u16 LE, grey one byte.
    let bytes = |key: &str| points[key].as_str().map(base64_decoded_len);
    assert_eq!(bytes("ids"), Some(count * 4), "{points}");
    assert_eq!(bytes("xyz"), Some(count * 12), "{points}");
    assert_eq!(bytes("observations"), Some(count * 2), "{points}");
    assert_eq!(bytes("grey"), Some(count), "{points}");

    // The delta is not a host output.
    assert!(!artifacts.iter().any(|artifact| artifact.kind == "host_output:delta"));
}

/// The byte length a standard, padded base64 string decodes to.
pub(super) fn base64_decoded_len(text: &str) -> usize {
    let padding = text.bytes().rev().take_while(|b| *b == b'=').count();
    text.len() / 4 * 3 - padding
}
