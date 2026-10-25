//! The robot pose topics: a pipeline's `multi_tag_pose` turned into what FRC robot code reads.
//!
//! For a pipeline whose outputs include `multi_tag_pose`, every tick publishes (under
//! `<prefix>/<pipeline>/`):
//!
//! - `robot_pose` (`double[]`: `[x, y, z, roll, pitch, yaw]`, metres and radians, WPILib's
//!   field frame, from `reference_from_rig`). Published only when there is a robot pose.
//! - `robot_pose_valid` (`boolean`): whether there is one this tick (`valid && has_rig`, with
//!   the pose's fields present). Robot code checks this before using `robot_pose`, which keeps
//!   its last value.
//! - `robot_pose_struct` (`raw`, type `struct:Pose3d`): the same pose as WPILib's `Pose3d`
//!   struct: translation `x, y, z` then rotation quaternion `w, x, y, z`, little-endian `f64`s
//!   (56 bytes). Published with `robot_pose`, after its schemas ([`POSE3D_SCHEMAS`]:
//!   `/.schema/struct:Pose3d` and the structs it nests, type `structschema`), so dashboards
//!   (AdvantageScope, Glass) decode it too.
//! - `tag_count` (`int`): `inlier_tags`, 0 when missing.
//! - `robot_pose_std_devs` (`double[]`: `[σx, σy, σθ]`, metres and radians, for WPILib's
//!   `addVisionMeasurement`), with `robot_pose`, from Eidos's `multi_tag_pose.covariance` (see
//!   [`robot_pose_std_devs`]).
//!
//! The field frame: `reference_from_rig` is in the reference frame of the field layout's known
//! tag poses (`helios-field`), which is WPILib's blue-origin field frame: origin at the blue
//! alliance wall's corner, `+X` toward the red wall, `+Z` up. The repo's FRC 2026 `.fmap` gives
//! the tags with blue-wall tags (29 to 32) at `x = 0`, facing `+X`, red-wall tags at `x = 16.5`
//! facing `-X`, and every tag above the floor (`z` 0.55 to 1.12 m); those confirm the `X` and
//! `Z` axes. The `Y` sign is the layout's own and is not checkable from the repo alone.
//!
//! Roll, pitch and yaw follow WPILib's `Rotation3d` (intrinsic `Z`-`Y`-`X`, the quaternion
//! `w, x, y, z`).
//!
//! A pipeline whose outputs include `pose` of type `eidos:map_pose` (the live mapper, the
//! `live-map` and `dense-map` templates; see [`read_map_pose`]) gets the same topics from the
//! map's `pose.rig`, valid only while the map is aligned to the field layout (the frame is the
//! field's only then). When the pipeline also has `multi_tag_pose`, its topics keep their names
//! and the map's get a `map_` prefix (`map_robot_pose`, `map_robot_pose_valid`, ...). Also:
//!
//! - `map_state` (`string`): the tracker's state, `bootstrapping`, `tracking`, `coasting`, `lost`
//!   or `relocalized`.
//! - `weak_robot_pose`, `weak_robot_pose_valid`, `weak_robot_pose_struct`,
//!   `weak_robot_pose_std_devs` and `weak_source` (`string`): the map's weak candidate (a lone
//!   small or ambiguous tag the map does not map yet). Opt-in: separate from `robot_pose`, and
//!   never in it. The candidate's robot pose needs a rig: it is read from `weak.rig` (the shape
//!   of `pose.rig`) when there is one. Eidos's weak candidate at 676cbf20 has no rig, so
//!   `weak_robot_pose_valid` is false until it does. `weak_source` is empty without a candidate.
//!
//! Roll, pitch and yaw follow WPILib's `Rotation3d` (intrinsic `Z`-`Y`-`X`, the quaternion
//! `w, x, y, z`).

use orion_nt4::Value;
use serde_json::Value as Json;

/// The output port whose JSON these topics come from.
pub const MULTI_TAG_POSE_PORT: &str = "multi_tag_pose";
/// The output port of the live mapper's pose (`eidos:map_pose`): `map_pose` in the graphs built
/// by Eidos's templates, `pose` in older stored graphs. Only its shape tells `pose` apart from the
/// AprilTag graphs' `pose` outputs: see [`is_map_pose`].
pub const MAP_POSE_PORT: &str = "map_pose";
/// The live mapper's pose port in graphs stored before Eidos's templates named it.
pub const LEGACY_MAP_POSE_PORT: &str = "pose";

/// `port` carries the live mapper's pose (either name).
pub fn is_map_pose_port(port: &str) -> bool {
    port == MAP_POSE_PORT || port == LEGACY_MAP_POSE_PORT
}
/// The NT4 type of `robot_pose_struct` (WPILib's `Pose3d`).
pub const POSE3D_STRUCT_TYPE: &str = "struct:Pose3d";
/// The NT4 type of a struct schema topic.
pub const STRUCT_SCHEMA_TYPE: &str = "structschema";
/// WPILib's schemas for `Pose3d` and the structs it nests, by schema topic (as WPILib's
/// `Pose3dStruct`, `Translation3dStruct`, `Rotation3dStruct` and `QuaternionStruct` give them).
pub const POSE3D_SCHEMAS: &[(&str, &str)] = &[
    ("/.schema/struct:Pose3d", "Translation3d translation;Rotation3d rotation"),
    ("/.schema/struct:Translation3d", "double x;double y;double z"),
    ("/.schema/struct:Rotation3d", "Quaternion q"),
    ("/.schema/struct:Quaternion", "double w;double x;double y;double z"),
];

/// The robot pose in the field: translation in metres, rotation as a unit quaternion and as
/// WPILib's roll, pitch and yaw in radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RobotPose {
    pub translation: [f64; 3],
    /// `w, x, y, z`.
    pub quaternion: [f64; 4],
    /// Roll (about `X`), pitch (about `Y`), yaw (about `Z`).
    pub rpy: [f64; 3],
}

/// The values one tick publishes for a pipeline's robot pose topics.
#[derive(Debug, Clone, PartialEq)]
pub struct RobotPoseTopics {
    /// `inlier_tags`, 0 when missing.
    pub tag_count: i64,
    /// `Some` when there is a robot pose this tick (`valid && has_rig`, fields present).
    pub pose: Option<RobotPose>,
    /// `Some` when [`robot_pose_std_devs`] has a value.
    pub std_devs: Option<[f64; 3]>,
}

impl RobotPoseTopics {
    /// `robot_pose_valid`.
    pub fn valid(&self) -> bool {
        self.pose.is_some()
    }

    /// `robot_pose` as the `double[]` value.
    pub fn robot_pose_value(pose: &RobotPose) -> Value {
        Value::DoubleArray(pose.translation.iter().chain(pose.rpy.iter()).copied().collect())
    }
}

/// A map pose's topics (`eidos:map_pose`, see [`read_map_pose`]).
#[derive(Debug, Clone, PartialEq)]
pub struct MapPoseTopics {
    /// `state`: `bootstrapping`, `tracking`, `coasting`, `lost` or `relocalized`.
    pub state: String,
    /// The main track's robot pose; `tag_count` is `alignment.matched_tags`.
    pub robot: RobotPoseTopics,
    /// The weak candidate's robot pose (the opt-in topics).
    pub weak: WeakPoseTopics,
}

/// The weak candidate's topics: a lone small or ambiguous tag the map does not map yet.
#[derive(Debug, Clone, PartialEq)]
pub struct WeakPoseTopics {
    /// `Some` when the map is aligned and the candidate has a rig (see the module docs).
    pub pose: Option<RobotPose>,
    /// `Some` with `pose`, when its covariance is usable.
    pub std_devs: Option<[f64; 3]>,
    /// `weak.source`: why the tag is weak. Empty without a candidate.
    pub source: String,
}

impl WeakPoseTopics {
    /// `weak_robot_pose_valid`.
    pub fn valid(&self) -> bool {
        self.pose.is_some()
    }
}

/// Whether `json` is an `eidos:map_pose`. The port name is not enough (the AprilTag graphs have
/// `pose` outputs too), so this checks the shape: a `state` string and an `alignment` object.
pub fn is_map_pose(json: &Json) -> bool {
    json.get("state").is_some_and(Json::is_string) && json.get("alignment").is_some_and(Json::is_object)
}

/// Read an `eidos:map_pose` JSON (the output as `GET /v1/outputs` shows it); `None` for any other
/// JSON (see [`is_map_pose`]).
///
/// The robot pose is `pose.rig`'s `reference_from_rig`, and it is published only while the map is
/// aligned to the field layout and there is a rig. Eidos leaves `rig` out when the camera has no
/// mount: it reads as the identity with an empty covariance, which is not a pose. Without the
/// rig there is no robot pose (`valid` false), as for `multi_tag_pose`.
pub fn read_map_pose(json: &Json) -> Option<MapPoseTopics> {
    if !is_map_pose(json) {
        return None;
    }
    let aligned = json["alignment"]["aligned"].as_bool().unwrap_or(false);
    let valid = json.get("valid").and_then(Json::as_bool).unwrap_or(false);
    let (pose, std_devs) = if valid { robot_pose_of_estimate(&json["pose"], aligned) } else { (None, None) };
    let weak = json.get("weak").filter(|weak| weak.is_object());
    let (weak_pose, weak_std_devs) = weak.map_or((None, None), |weak| robot_pose_of_estimate(weak, aligned));
    Some(MapPoseTopics {
        state: json["state"].as_str().unwrap_or_default().to_string(),
        robot: RobotPoseTopics { tag_count: json["alignment"]["matched_tags"].as_i64().unwrap_or(0), pose, std_devs },
        weak: WeakPoseTopics { pose: weak_pose, std_devs: weak_std_devs, source: weak.and_then(|weak| weak.get("source")).and_then(Json::as_str).unwrap_or_default().to_string() },
    })
}

/// The robot pose of one estimate (a map pose's `pose` or `weak`) and its std devs: from its
/// `rig`, when the map is aligned and the rig has its 36-value covariance.
fn robot_pose_of_estimate(estimate: &Json, aligned: bool) -> (Option<RobotPose>, Option<[f64; 3]>) {
    let Some(rig) = estimate.get("rig").filter(|rig| aligned && has_covariance(rig)) else {
        return (None, None);
    };
    let pose = rig.get("reference_from_rig").and_then(robot_pose_from_transform);
    let std_devs = pose.as_ref().and_then(|pose| robot_pose_std_devs(rig, pose.translation));
    (pose, std_devs)
}

/// Whether `object` has a 6 x 6 `covariance` (36 values).
fn has_covariance(object: &Json) -> bool {
    object.get("covariance").and_then(Json::as_array).is_some_and(|values| values.len() == 36)
}

/// Read a `multi_tag_pose` JSON (the output as `GET /v1/outputs` shows it). Missing or
/// malformed fields give no robot pose (`valid` false), never a guessed one.
pub fn read(multi_tag_pose: &Json) -> RobotPoseTopics {
    let has_rig = multi_tag_pose.get("has_rig").and_then(Json::as_bool).unwrap_or(false);
    let valid = multi_tag_pose.get("valid").and_then(Json::as_bool).unwrap_or(false);
    let pose = if valid && has_rig { multi_tag_pose.get("reference_from_rig").and_then(robot_pose_from_transform) } else { None };
    let std_devs = pose.as_ref().and_then(|pose| robot_pose_std_devs(multi_tag_pose, pose.translation));
    RobotPoseTopics { tag_count: multi_tag_pose.get("inlier_tags").and_then(Json::as_i64).unwrap_or(0), pose, std_devs }
}

/// The robot pose's standard deviations `[σx, σy, σθ]` (metres, metres, radians: what WPILib's
/// pose estimators take), from the covariance in `estimate` (an object with a `covariance`: the
/// `multi_tag_pose`, or a map pose's `rig`, whose covariance is the camera's), for a robot at
/// `translation` (field frame). `None` without a usable covariance.
///
/// Eidos's convention: 6 x 6 row-major `[rx, ry, rz, tx, ty, tz]`, the left perturbation
/// `reference_from_camera' = exp([w, v]^) reference_from_camera`, `w` and `v` in the reference
/// (field) frame. `reference_from_rig` is `reference_from_camera` times the fixed mount, so the
/// same perturbation holds for it. A left perturbation moves a point `p` by `v + w × p`, so the
/// robot's position error is `v - [t]× w` (rotation error turns about the field origin):
/// `Σt = V - Cᵀ[t]×ᵀ - [t]× C + [t]× W [t]×ᵀ` with `W`, `C`, `V` the rotation, rotation-translation
/// and translation blocks. `σθ` is the error about the field's `z` (`w_z`).
pub fn robot_pose_std_devs(estimate: &Json, translation: [f64; 3]) -> Option<[f64; 3]> {
    let values = estimate.get("covariance")?.as_array()?;
    if values.len() != 36 {
        return None;
    }
    let mut sigma = [[0.0_f64; 6]; 6];
    for (index, value) in values.iter().enumerate() {
        let value = value.as_f64().filter(|v| v.is_finite())?;
        sigma[index / 6][index % 6] = value;
    }
    let [x, y, z] = translation;
    // [t]×, so that [t]× a = t × a.
    let cross = [[0.0, -z, y], [z, 0.0, -x], [-y, x, 0.0]];
    // J = [-[t]×, I] maps [w, v] to the position error; Σt = J Σ Jᵀ.
    let mut jacobian = [[0.0_f64; 6]; 3];
    for row in 0..3 {
        for col in 0..3 {
            jacobian[row][col] = -cross[row][col];
        }
        jacobian[row][3 + row] = 1.0;
    }
    let variance = |row: usize| -> f64 {
        let mut total = 0.0;
        for a in 0..6 {
            for b in 0..6 {
                total += jacobian[row][a] * sigma[a][b] * jacobian[row][b];
            }
        }
        total
    };
    let (vx, vy, vtheta) = (variance(0), variance(1), sigma[2][2]);
    if !(vx >= 0.0 && vy >= 0.0 && vtheta >= 0.0) {
        return None;
    }
    Some([vx.sqrt().max(MIN_POSITION_STD_DEV_M), vy.sqrt().max(MIN_POSITION_STD_DEV_M), vtheta.sqrt().max(MIN_HEADING_STD_DEV_RAD)])
}

/// The smallest std devs robot code is given. Each frame's covariance comes from that one solve's
/// residuals, so it is right on average but now and then far too small (Eidos's Monte Carlo); a
/// pose estimator would then snap to that frame. The floors are the field's own placement error.
pub const MIN_POSITION_STD_DEV_M: f64 = 0.01;
pub const MIN_HEADING_STD_DEV_RAD: f64 = 0.5 * std::f64::consts::PI / 180.0;

/// A `reference_from_rig` transform (`{translation, rotation}`) as a robot pose; `None` unless
/// every field is there and finite and the quaternion is not zero.
fn robot_pose_from_transform(transform: &Json) -> Option<RobotPose> {
    let translation = transform.get("translation")?;
    let rotation = transform.get("rotation")?;
    let translation = [number(translation, "x")?, number(translation, "y")?, number(translation, "z")?];
    let raw = [number(rotation, "w")?, number(rotation, "x")?, number(rotation, "y")?, number(rotation, "z")?];
    let norm = raw.iter().map(|c| c * c).sum::<f64>().sqrt();
    if !(norm.is_finite() && norm > 1e-9) || !translation.iter().all(|c| c.is_finite()) {
        return None;
    }
    let quaternion = raw.map(|c| c / norm);
    Some(RobotPose { translation, quaternion, rpy: rpy_from_quaternion(quaternion) })
}

fn number(object: &Json, key: &str) -> Option<f64> {
    object.get(key)?.as_f64().filter(|value| value.is_finite())
}

/// Roll, pitch and yaw of a unit quaternion `[w, x, y, z]`, as WPILib's `Rotation3d` reads them:
/// `R = Rz(yaw) Ry(pitch) Rx(roll)`. Pitch is clamped to `±π/2` at the gimbal lock.
pub fn rpy_from_quaternion([w, x, y, z]: [f64; 4]) -> [f64; 3] {
    let roll = (2.0 * (w * x + y * z)).atan2(1.0 - 2.0 * (x * x + y * y));
    let sin_pitch = 2.0 * (w * y - z * x);
    let pitch = if sin_pitch.abs() >= 1.0 { std::f64::consts::FRAC_PI_2.copysign(sin_pitch) } else { sin_pitch.asin() };
    let yaw = (2.0 * (w * z + x * y)).atan2(1.0 - 2.0 * (y * y + z * z));
    [roll, pitch, yaw]
}

/// WPILib's `Pose3d` struct bytes: translation `x, y, z`, then quaternion `w, x, y, z`, as
/// little-endian `f64`s.
pub fn pose3d_struct_bytes(pose: &RobotPose) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(56);
    for component in pose.translation.iter().chain(pose.quaternion.iter()) {
        bytes.extend_from_slice(&component.to_le_bytes());
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::f64::consts::{FRAC_PI_2, PI};

    /// A diagonal 6 x 6 covariance (`[rx, ry, rz, tx, ty, tz]` variances), row-major.
    fn diagonal(variances: [f64; 6]) -> Json {
        let mut values = vec![0.0; 36];
        for (i, v) in variances.iter().enumerate() {
            values[i * 7] = *v;
        }
        json!(values)
    }

    #[test]
    fn std_devs_never_go_below_the_floor() {
        let pose = json!({ "covariance": diagonal([0.0; 6]) });
        assert_eq!(robot_pose_std_devs(&pose, [1.0, 1.0, 0.0]), Some([MIN_POSITION_STD_DEV_M, MIN_POSITION_STD_DEV_M, MIN_HEADING_STD_DEV_RAD]));
    }

    #[test]
    fn std_devs_at_the_origin_are_the_translation_and_yaw_sigmas() {
        let pose = json!({ "covariance": diagonal([1e-4, 4e-4, 9e-4, 0.01, 0.04, 0.09]) });
        let [sx, sy, st] = robot_pose_std_devs(&pose, [0.0, 0.0, 0.0]).expect("std devs");
        assert!((sx - 0.1).abs() < 1e-12 && (sy - 0.2).abs() < 1e-12 && (st - 0.03).abs() < 1e-12, "{sx} {sy} {st}");
    }

    #[test]
    fn yaw_error_moves_a_robot_away_from_the_field_origin() {
        // Only yaw uncertainty (0.01 rad): at (10, 0, 0) it is 10 cm across the field (y), none along x.
        let pose = json!({ "covariance": diagonal([0.0, 0.0, 1e-4, 0.0, 0.0, 0.0]) });
        let [sx, sy, st] = robot_pose_std_devs(&pose, [10.0, 0.0, 0.0]).expect("std devs");
        assert!((sx - MIN_POSITION_STD_DEV_M).abs() < 1e-12 && (sy - 0.1).abs() < 1e-12 && (st - 0.01).abs() < 1e-12, "{sx} {sy} {st}");
    }

    #[test]
    fn no_or_broken_covariance_means_no_std_devs() {
        assert_eq!(robot_pose_std_devs(&json!({}), [1.0, 2.0, 0.0]), None);
        assert_eq!(robot_pose_std_devs(&json!({ "covariance": [1.0, 2.0] }), [1.0, 2.0, 0.0]), None);
        assert_eq!(robot_pose_std_devs(&json!({ "covariance": diagonal([0.0, 0.0, -1.0, 0.0, 0.0, 0.0]) }), [0.0; 3]), None);
    }

    /// `[w, x, y, z]` of a rotation of `angle` about a unit axis.
    fn axis_angle(axis: [f64; 3], angle: f64) -> [f64; 4] {
        let (s, c) = (angle / 2.0).sin_cos();
        [c, axis[0] * s, axis[1] * s, axis[2] * s]
    }

    /// `a * b` for quaternions `[w, x, y, z]`.
    fn multiply(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
        let [aw, ax, ay, az] = a;
        let [bw, bx, by, bz] = b;
        [aw * bw - ax * bx - ay * by - az * bz, aw * bx + ax * bw + ay * bz - az * by, aw * by - ax * bz + ay * bw + az * bx, aw * bz + ax * by - ay * bx + az * bw]
    }

    fn assert_rpy(actual: [f64; 3], expected: [f64; 3]) {
        for (a, e) in actual.iter().zip(expected) {
            assert!((a - e).abs() < 1e-12, "{actual:?} != {expected:?}");
        }
    }

    #[test]
    fn rpy_of_known_quaternions() {
        assert_rpy(rpy_from_quaternion([1.0, 0.0, 0.0, 0.0]), [0.0, 0.0, 0.0]);
        let (roll, pitch, yaw) = (30f64.to_radians(), 20f64.to_radians(), 90f64.to_radians());
        assert_rpy(rpy_from_quaternion(axis_angle([1.0, 0.0, 0.0], roll)), [roll, 0.0, 0.0]);
        assert_rpy(rpy_from_quaternion(axis_angle([0.0, 1.0, 0.0], pitch)), [0.0, pitch, 0.0]);
        assert_rpy(rpy_from_quaternion(axis_angle([0.0, 0.0, 1.0], yaw)), [0.0, 0.0, yaw]);
        // R = Rz(yaw) Ry(pitch) Rx(roll), composed from the axis rotations.
        let composed = multiply(axis_angle([0.0, 0.0, 1.0], yaw), multiply(axis_angle([0.0, 1.0, 0.0], pitch), axis_angle([1.0, 0.0, 0.0], roll)));
        assert_rpy(rpy_from_quaternion(composed), [roll, pitch, yaw]);
        // A half turn about X, the usual "camera upside down" case, is roll pi and nothing else.
        assert_rpy(rpy_from_quaternion([0.0, 1.0, 0.0, 0.0]), [PI, 0.0, 0.0]);
    }

    #[test]
    fn rpy_at_gimbal_lock_is_finite() {
        let [roll, pitch, yaw] = rpy_from_quaternion(axis_angle([0.0, 1.0, 0.0], FRAC_PI_2));
        assert!(roll.is_finite() && yaw.is_finite());
        assert!((pitch - FRAC_PI_2).abs() < 1e-6, "pitch {pitch}");
        let [_, pitch, _] = rpy_from_quaternion(axis_angle([0.0, 1.0, 0.0], -FRAC_PI_2));
        assert!((pitch + FRAC_PI_2).abs() < 1e-6, "pitch {pitch}");
    }

    #[test]
    fn pose3d_struct_is_wpilib_layout() {
        let pose = RobotPose { translation: [1.5, -2.0, 0.25], quaternion: [0.5, 0.5, -0.5, 0.5], rpy: [0.0; 3] };
        let bytes = pose3d_struct_bytes(&pose);
        assert_eq!(bytes.len(), 56);
        let (chunks, rest) = bytes.as_chunks::<8>();
        assert!(rest.is_empty());
        let doubles: Vec<f64> = chunks.iter().map(|chunk| f64::from_le_bytes(*chunk)).collect();
        assert_eq!(doubles, vec![1.5, -2.0, 0.25, 0.5, 0.5, -0.5, 0.5]);
    }

    fn pose_json(valid: bool, has_rig: bool) -> Json {
        json!({
            "status": "calibrated",
            "valid": valid,
            "inlier_tags": 3,
            "unknown_tags": [],
            "unsolved_tags": [],
            "rms_px": 0.3,
            "ambiguity": 0.1,
            "has_rig": has_rig,
            "reference_from_camera": {"rotation": {"w": 1.0, "x": 0.0, "y": 0.0, "z": 0.0}, "translation": {"x": 9.0, "y": 9.0, "z": 9.0}},
            "reference_from_rig": {
                "rotation": {"w": 0.0, "x": 0.0, "y": 0.0, "z": 1.0},
                "translation": {"x": 3.0, "y": 4.0, "z": 0.1}
            },
            "tags": []
        })
    }

    #[test]
    fn topics_for_a_valid_pose_with_a_rig() {
        let topics = read(&pose_json(true, true));
        assert!(topics.valid());
        assert_eq!(topics.tag_count, 3);
        let pose = topics.pose.expect("a robot pose");
        assert_eq!(pose.translation, [3.0, 4.0, 0.1]);
        // A half turn about Z: yaw pi.
        assert_rpy(pose.rpy, [0.0, 0.0, PI]);
        assert_eq!(RobotPoseTopics::robot_pose_value(&pose), Value::DoubleArray(vec![3.0, 4.0, 0.1, 0.0, 0.0, PI]));
        assert_eq!(topics.std_devs, None);
    }

    #[test]
    fn invalid_or_rigless_poses_publish_no_robot_pose() {
        for (valid, has_rig) in [(false, true), (true, false), (false, false)] {
            let topics = read(&pose_json(valid, has_rig));
            assert!(!topics.valid(), "valid={valid} has_rig={has_rig}");
            assert_eq!(topics.pose, None);
            assert_eq!(topics.tag_count, 3, "tag_count is published every tick");
        }
    }

    #[test]
    fn missing_or_bad_fields_give_no_robot_pose() {
        let mut no_rotation = pose_json(true, true);
        no_rotation["reference_from_rig"].as_object_mut().unwrap().remove("rotation");
        assert!(!read(&no_rotation).valid());

        let mut zero_quaternion = pose_json(true, true);
        zero_quaternion["reference_from_rig"]["rotation"] = json!({"w": 0.0, "x": 0.0, "y": 0.0, "z": 0.0});
        assert!(!read(&zero_quaternion).valid());

        let mut missing_z = pose_json(true, true);
        missing_z["reference_from_rig"]["translation"].as_object_mut().unwrap().remove("z");
        assert!(!read(&missing_z).valid());

        let empty = read(&json!({}));
        assert!(!empty.valid());
        assert_eq!(empty.tag_count, 0);

        assert!(!read(&Json::Null).valid());
    }

    #[test]
    fn quaternion_is_normalized() {
        let mut scaled = pose_json(true, true);
        scaled["reference_from_rig"]["rotation"] = json!({"w": 0.0, "x": 0.0, "y": 0.0, "z": 2.0});
        let pose = read(&scaled).pose.expect("a robot pose");
        assert_eq!(pose.quaternion, [0.0, 0.0, 0.0, 1.0]);
    }

    /// A recorded `eidos:map_pose` from a simulator run of the live map: `tracking`, aligned to
    /// the layout by one tag, with no mount (`rig` is the identity with an empty covariance).
    const RECORDED_MAP_POSE: &str = r#"{"alignment":{"aligned":true,"matched_tags":1,"residual_m":0.0},"calibration":{"estimated":false,"frames":0,"rms_px":0.0,"views":0},"frame":50,"pose":{"covariance":[0.00025738762911545904,-0.0009181988682054043,-1.1943684986205787e-05,0.0004563093623984005,0.0001917703074587258,-0.003350650540182484,-0.0009181988682055767,0.006966704277136646,7.063100758548332e-05,-0.003466920213365822,-0.0008342132007037621,0.027513324005900137,-1.1943684986199051e-05,7.063100758522983e-05,0.02839884579091783,0.0017398525475707323,-0.13437105115217013,0.00032625548367885273,0.00045630936239848726,-0.0034669202133658676,0.0017398525475706178,0.0031838755813809795,-0.010380103389826527,-0.013682926205436883,0.00019177030745869152,-0.0008342132007024116,-0.1343710511521693,-0.010380103389827179,0.6464427135269668,-0.0034178276312190146,-0.0033506505401835096,0.02751332400590161,0.0003262554836801152,-0.013682926205437396,-0.003417827631225824,0.11397049956519029],"hypotheses":{"chosen":0,"count":0,"weights":[],"why":""},"points":0,"reference_from_camera":{"rotation":{"w":0.5395005352963929,"x":-0.5400136831425433,"y":0.4569982668047262,"z":-0.45659279294707106},"translation":{"x":3.422192356110152,"y":-0.24991488920721347,"z":0.4991956071259854}},"rig":{"covariance":[],"reference_from_rig":{"rotation":{"w":1.0,"x":0.0,"y":0.0,"z":0.0},"translation":{"x":0.0,"y":0.0,"z":0.0}}},"rms_px":0.14691589982824188,"sigma":{"position_m":[0.06418397392687641,0.24343230836475382,0.08584188059431067],"rotation_deg":[0.9192136548565283,4.782294537106446,9.655460180407216]},"source":"bootstrap"},"reason":"bootstrap","state":"tracking","t_ns":3929783590420729,"valid":true}"#;

    fn recorded_map_pose() -> Json {
        serde_json::from_str(RECORDED_MAP_POSE).expect("the recorded map pose")
    }

    /// The recorded map pose with a mount: `rig` at `translation`, yaw pi, with the camera's
    /// covariance (as Eidos gives it).
    fn with_mount(mut map_pose: Json, translation: [f64; 3]) -> Json {
        let covariance = map_pose["pose"]["covariance"].clone();
        map_pose["pose"]["rig"] = json!({
            "covariance": covariance,
            "reference_from_rig": {
                "rotation": {"w": 0.0, "x": 0.0, "y": 0.0, "z": 1.0},
                "translation": {"x": translation[0], "y": translation[1], "z": translation[2]}
            }
        });
        map_pose
    }

    #[test]
    fn map_pose_is_recognised_by_its_shape() {
        assert!(is_map_pose(&recorded_map_pose()));
        assert!(!is_map_pose(&pose_json(true, true)), "an AprilTag pose output is not a map pose");
        assert!(!is_map_pose(&json!({"state": "tracking"})));
        assert!(!is_map_pose(&json!({"alignment": {"aligned": true}})));
        assert!(read_map_pose(&pose_json(true, true)).is_none());
    }

    #[test]
    fn recorded_map_pose_without_a_mount_has_no_robot_pose() {
        let topics = read_map_pose(&recorded_map_pose()).expect("a map pose");
        assert_eq!(topics.state, "tracking");
        assert_eq!(topics.robot.tag_count, 1, "tag_count is the matched layout tags");
        assert!(!topics.robot.valid(), "the identity placeholder is not a pose");
        assert_eq!(topics.robot.std_devs, None);
        assert!(!topics.weak.valid());
        assert_eq!(topics.weak.source, "");
    }

    #[test]
    fn aligned_map_pose_with_a_mount_is_the_robot_pose() {
        let topics = read_map_pose(&with_mount(recorded_map_pose(), [3.0, 4.0, 0.1])).expect("a map pose");
        assert!(topics.robot.valid());
        let pose = topics.robot.pose.expect("a robot pose");
        assert_eq!(pose.translation, [3.0, 4.0, 0.1]);
        assert_rpy(pose.rpy, [0.0, 0.0, PI]);
        assert_eq!(topics.robot.tag_count, 1);
        let [sx, sy, st] = topics.robot.std_devs.expect("std devs from the rig's covariance");
        assert!(sx > 0.0 && sy > 0.0 && st > 0.0 && [sx, sy, st].iter().all(|s| s.is_finite()));
        assert!(!topics.weak.valid());
    }

    #[test]
    fn unaligned_map_pose_has_no_robot_pose() {
        let mut map_pose = with_mount(recorded_map_pose(), [3.0, 4.0, 0.1]);
        map_pose["alignment"]["aligned"] = json!(false);
        map_pose["weak"] = json!({
            "reference_from_camera": {"rotation": {"w": 1.0, "x": 0.0, "y": 0.0, "z": 0.0}, "translation": {"x": 1.0, "y": 0.0, "z": 0.0}},
            "source": "lone small tag, not mapped",
            "rig": {"covariance": map_pose["pose"]["covariance"].clone(), "reference_from_rig": {"rotation": {"w": 1.0, "x": 0.0, "y": 0.0, "z": 0.0}, "translation": {"x": 7.0, "y": 1.0, "z": 0.2}}}
        });
        let topics = read_map_pose(&map_pose).expect("a map pose");
        assert!(!topics.robot.valid());
        assert_eq!(topics.robot.std_devs, None);
        assert!(!topics.weak.valid(), "the weak topics need the alignment too");
    }

    #[test]
    fn absent_weak_candidate_means_weak_is_not_valid() {
        let absent = read_map_pose(&with_mount(recorded_map_pose(), [3.0, 4.0, 0.1])).expect("a map pose");
        assert!(!absent.weak.valid());
        assert_eq!(absent.weak.source, "");
        // Eidos writes a missing weak candidate as null.
        let mut null_weak = with_mount(recorded_map_pose(), [3.0, 4.0, 0.1]);
        null_weak["weak"] = Json::Null;
        let topics = read_map_pose(&null_weak).expect("a map pose");
        assert!(!topics.weak.valid());
        assert_eq!(topics.weak.source, "");
        assert!(topics.robot.valid(), "the main track is unaffected");
    }

    #[test]
    fn weak_candidate_without_a_rig_reports_its_source_only() {
        let mut map_pose = recorded_map_pose();
        map_pose["weak"] = json!({
            "reference_from_camera": {"rotation": {"w": 1.0, "x": 0.0, "y": 0.0, "z": 0.0}, "translation": {"x": 1.0, "y": 0.0, "z": 0.0}},
            "covariance": diagonal([1e-4, 1e-4, 1e-4, 0.01, 0.01, 0.01]),
            "source": "lone small tag, not mapped"
        });
        let topics = read_map_pose(&map_pose).expect("a map pose");
        assert!(!topics.weak.valid());
        assert_eq!(topics.weak.source, "lone small tag, not mapped");
        assert!(!topics.robot.valid());
    }

    #[test]
    fn weak_candidate_with_a_rig_is_its_own_robot_pose() {
        let mut map_pose = with_mount(recorded_map_pose(), [3.0, 4.0, 0.1]);
        map_pose["weak"] = json!({
            "reference_from_camera": {"rotation": {"w": 1.0, "x": 0.0, "y": 0.0, "z": 0.0}, "translation": {"x": 1.0, "y": 0.0, "z": 0.0}},
            "covariance": diagonal([1e-4, 1e-4, 1e-4, 0.01, 0.01, 0.01]),
            "source": "lone small tag, not mapped",
            "rig": {
                "covariance": diagonal([1e-4, 1e-4, 1e-4, 0.01, 0.01, 0.01]),
                "reference_from_rig": {"rotation": {"w": 1.0, "x": 0.0, "y": 0.0, "z": 0.0}, "translation": {"x": 7.0, "y": 1.0, "z": 0.2}}
            }
        });
        let topics = read_map_pose(&map_pose).expect("a map pose");
        assert!(topics.weak.valid());
        let weak = topics.weak.pose.expect("a weak robot pose");
        assert_eq!(weak.translation, [7.0, 1.0, 0.2]);
        assert_eq!(topics.weak.source, "lone small tag, not mapped");
        assert!(topics.weak.std_devs.is_some());
        // The main track's robot pose is its own rig's, never the weak candidate's.
        assert_eq!(topics.robot.pose.expect("the main robot pose").translation, [3.0, 4.0, 0.1]);
    }
}
