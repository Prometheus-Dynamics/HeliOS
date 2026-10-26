//! The live-map template (`eidos:mapping.live_map`): the tracked AprilTag detector feeds the live
//! mapper, which gives the camera pose, the map snapshot and the timing as host outputs. The
//! per-tick map delta is not a host output (the host keeps only the latest value per port, so
//! the deltas of a tick the host did not read would be lost).

use daedalus::runtime::plugins::PluginRegistry;

use super::*;

/// The live-map graph as a stored document (`localization_map::map_document`): the tracked
/// detector feeds Eidos's live mapper. Host outputs: `detections`, `map_pose`, `map_snapshot`,
/// `map_timing`. The per-tick map delta is not wired (the host keeps only the latest value per
/// port, so the deltas of a tick the host did not read would be lost).
pub(super) fn live_map_template(registry: &PluginRegistry) -> String {
    super::localization_map::map_document(registry, None, None)
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

/// Smooth value noise on a lattice of `cell` (any unit): corners everywhere, no period.
fn value_noise(x: f64, y: f64, cell: f64, seed: u32) -> f64 {
    let lattice = |i: i64, j: i64| {
        let mut h = (i as u32).wrapping_mul(0x9E37_79B1) ^ (j as u32).wrapping_mul(0x85EB_CA77) ^ seed.wrapping_mul(0xC2B2_AE3D);
        h ^= h >> 15;
        h = h.wrapping_mul(0x2C1B_3C6D);
        h ^= h >> 12;
        f64::from(h & 0xFF)
    };
    let (gx, gy) = (x / cell, y / cell);
    let (i, j) = (gx.floor() as i64, gy.floor() as i64);
    let smooth = |t: f64| t * t * (3.0 - 2.0 * t);
    let (u, v) = (smooth(gx - gx.floor()), smooth(gy - gy.floor()));
    let top = lattice(i, j) * (1.0 - u) + lattice(i + 1, j) * u;
    let bottom = lattice(i, j + 1) * (1.0 - u) + lattice(i + 1, j + 1) * u;
    top * (1.0 - v) + bottom * v
}

/// The wall's texture at (`x`, `y`) metres on the wall: three octaves of value noise (13, 4 and
/// 1.7 cm cells, as Eidos's dense tests), 0..255.
fn wall_texture(x: f64, y: f64) -> f64 {
    let (x, y) = (x * 300.0, y * 300.0);
    0.5 * value_noise(x, y, 40.0, 1) + 0.3 * value_noise(x, y, 13.0, 2) + 0.2 * value_noise(x, y, 5.0, 3)
}

/// A GREY frame of 640 x 480 with the AprilTag 36h11 marker `id` on a textured wall, seen through
/// `tag_homography` of `pose_index`. The wall is the tag's plane and every pixel goes through the
/// same homography, so the scene is one rigid plane seen from the moving camera: the tracker has
/// texture to follow and the dense mapper has parallax that agrees with the tag's pose.
pub(super) fn live_map_scene(id: u32, pose_index: u64, timestamp: u64) -> FrameLease {
    let (width, height) = (640_usize, 480_usize);
    let dictionary = dictionary(ArucoDictionaryKind::AprilTag36h11);
    let grid = BitGrid::from_marker(dictionary, id, 0).expect("marker bits");
    let cells = usize::from(dictionary.marker_size) + 2;
    let inverse = invert3(&tag_homography(pose_index));
    let half = SCENE_TAG_SIDE_M / 2.0;
    let mut pixels = vec![0_u8; width * height];
    for y in 0..height {
        for x in 0..width {
            // The wall point under the pixel, in units of the tag's half side (the tag spans -1..1).
            let q = mul3(&inverse, [x as f64 + 0.5, y as f64 + 0.5, 1.0]);
            let (wu, wv) = (q[0] / q[2], q[1] / q[2]);
            let (u, v) = ((wu + 1.0) / 2.0, (wv + 1.0) / 2.0);
            pixels[y * width + x] = if (0.0..1.0).contains(&u) && (0.0..1.0).contains(&v) {
                let (cx, cy) = ((u * cells as f64) as usize, (v * cells as f64) as usize);
                let border = cx == 0 || cy == 0 || cx == cells - 1 || cy == cells - 1;
                if !border && grid.bit(cx - 1, cy - 1) { 235 } else { 20 }
            } else {
                // Bright enough that the tag's black border stands out against it.
                (120.0 + wall_texture(wu * half, wv * half) * 0.5) as u8
            };
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
    // The scene's own calibration (a 500 px pinhole): the mapper tracks from the first clear tag.
    // Its self-calibration needs views over most of the image, which one marker in this short clip
    // doesn't give; Eidos's tests and the sim's selfcal scenario cover it.
    let calibrated = calibration_context(SCENE_FOCAL_PX, "pinhole");
    let mut workload_spec = workload("workload.live-map", document, vec![camera_binding(&calibrated)]);
    workload_spec.plugin_requirements = vec![PluginRequirement { plugin_name: "eidos".into(), version: None }];
    let spec = workload_spec.clone();
    let (mut driver, frames) = spawn_channel_driver(&plugins, workload_spec, "frame", GraphSettings::default());
    driver.update_camera_context(&spec);

    // The mapper starts on the first clear tag (the tilted marker, which pins its pose).
    // Timestamps are nanoseconds, 100 ms apart.
    let frame_count = 160_u64;
    for index in 0..frame_count {
        frames.send(live_map_scene(586, index, (index + 1) * 100_000_000)).expect("send frame");
        wait_for("the frame to be processed", || driver.stats().ticks_processed == index + 1);
    }
    let (session, artifacts) = driver.snapshot();
    assert_eq!(session.status, ExecutionSessionStatus::Running, "{:?}", session.message);

    let pose = json_with_keys(&artifacts, "host_output:map_pose", &["frame", "t_ns", "state", "reason", "valid", "pose", "calibration", "alignment"]);
    assert_eq!(pose["frame"].as_u64(), Some(frame_count - 1), "{pose}");
    assert_eq!(pose["state"], "tracking", "{pose}");
    assert_eq!(pose["valid"], true, "{pose}");
    assert_eq!(pose["calibration"]["estimated"], false, "the given calibration, not an estimate: {pose}");
    assert!(pose["alignment"]["aligned"].is_boolean(), "{pose}");
    let estimate = &pose["pose"];
    for key in ["reference_from_camera", "covariance", "sigma", "source", "rms_px", "points", "hypotheses", "rig"] {
        assert!(estimate.get(key).is_some(), "pose.pose has `{key}`: {pose}");
    }
    assert_eq!(estimate["covariance"].as_array().map(Vec::len), Some(36), "{pose}");
    assert_eq!(estimate["sigma"]["position_m"].as_array().map(Vec::len), Some(3), "{pose}");
    assert_eq!(estimate["sigma"]["rotation_deg"].as_array().map(Vec::len), Some(3), "{pose}");
    assert!(estimate["reference_from_camera"]["rotation"]["w"].is_number(), "{pose}");

    let timing = json_with_keys(&artifacts, "host_output:map_timing", &["frame", "t_ns", "frame_ms", "map_ms", "stages"]);
    assert!(timing["frame_ms"].as_f64().is_some_and(|ms| ms >= 0.0), "{timing}");
    assert!(timing["stages"].as_array().is_some_and(|stages| !stages.is_empty() && stages.iter().all(|stage| stage["name"].is_string() && stage["ms"].is_number())), "{timing}");

    let detections = artifact_json(&artifacts, "host_output:detections");
    assert!(detections["detections"].as_array().is_some_and(|list| list.len() == 1 && list[0]["id"] == 586), "the scene's marker is detected: {detections}");

    // The snapshot is due on the first mapping tick and then at most twice a second: it exists
    // with the tag and the trajectory, and its point buffers are packed as documented.
    let snapshot = json_with_keys(&artifacts, "host_output:map_snapshot", &["seq", "run_id", "tag_size_m", "aligned", "estimated", "trajectory", "keyframes", "points", "tags"]);
    assert_eq!(snapshot["estimated"], false, "the given calibration: {snapshot}");
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
