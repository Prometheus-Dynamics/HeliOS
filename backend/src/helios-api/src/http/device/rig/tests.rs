use super::{
    UpdateRobotDimensionsRequest,
    peer_forward::encode_path_segment,
    state::{apply_robot_dimensions_patch, camera_uid_from_keys, decode_robot_dimensions},
    types::RobotDimensions,
};

#[test]
fn camera_uid_from_keys_prefers_path_like_keys() {
    let keys = vec!["serial:abcd".to_string(), "/base/soc/i2c0mux/i2c@1/ov9782@60".to_string()];
    assert_eq!(camera_uid_from_keys(&keys, Some("fallback")).as_deref(), Some("/base/soc/i2c0mux/i2c@1/ov9782@60"));
}

#[test]
fn camera_uid_from_keys_sorts_simple_keys_before_fallback() {
    let keys = vec!["zeta".to_string(), "alpha".to_string()];
    assert_eq!(camera_uid_from_keys(&keys, Some("fallback")).as_deref(), Some("alpha"));
}

#[test]
fn apply_robot_dimensions_patch_ignores_invalid_values() {
    let robot = RobotDimensions::default();
    let patched = apply_robot_dimensions_patch(
        robot.clone(),
        UpdateRobotDimensionsRequest { width_m: Some(-1.0), length_m: Some(f64::NAN), bumper_height_m: Some(0.0), bumper_thickness_m: Some(0.04), ground_clearance_m: Some(-0.5) },
    );

    assert_eq!(patched.width_m, robot.width_m);
    assert_eq!(patched.length_m, robot.length_m);
    assert_eq!(patched.bumper_height_m, robot.bumper_height_m);
    assert_eq!(patched.bumper_thickness_m, 0.04);
    assert_eq!(patched.ground_clearance_m, robot.ground_clearance_m);
}

#[test]
fn encode_path_segment_uses_percent_encoding_for_spaces() {
    assert_eq!(encode_path_segment("front camera/pose value"), "front%20camera%2Fpose%20value");
}

#[test]
fn decode_robot_dimensions_rejects_missing_schema_version() {
    let raw = serde_json::json!({
        "robot": RobotDimensions::default()
    });

    let err = decode_robot_dimensions(&serde_json::to_vec(&raw).expect("encode")).expect_err("missing schema version should fail");
    assert!(err.contains("missing required schema_version"));
}

#[test]
fn decode_robot_dimensions_rejects_future_schema_version() {
    let raw = serde_json::json!({
        "schema_version": 2,
        "robot": RobotDimensions::default()
    });

    let err = decode_robot_dimensions(&serde_json::to_vec(&raw).expect("encode")).expect_err("future schema version should fail");
    assert!(err.contains("unsupported robot dimensions document schema_version"));
}
