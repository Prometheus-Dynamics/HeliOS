use lib_sensors::dto::{ImuAxesPayload, ImuOrientationPayload, ImuQuaternionPayload, ImuStatusPayload};

use super::sampling::build_device_imu_sample;

fn sample_status() -> ImuStatusPayload {
    ImuStatusPayload {
        orientation: Some(ImuOrientationPayload { roll: 1.0, pitch: 2.0, yaw: 3.0, quaternion: Some(ImuQuaternionPayload { x: 0.1, y: 0.2, z: 0.3, w: 0.4 }) }),
        position_world: Some(ImuAxesPayload { x: 1.5, y: -2.0, z: 0.25 }),
        velocity_world: Some(ImuAxesPayload { x: 4.0, y: 5.0, z: 6.0 }),
        updated_at: Some("2026-03-31T12:34:56Z".to_string()),
        ..ImuStatusPayload::default()
    }
}

#[test]
fn device_imu_sample_omits_translation_when_dead_reckoning_is_locked() {
    let mut status = sample_status();
    status.dr_lock_position = Some(true);

    let sample = build_device_imu_sample(&status).expect("orientation should produce a sample");
    let payload = sample.value.as_object().expect("sample payload should be an object");

    assert!(!payload.contains_key("translation"));
    assert_eq!(payload.get("sampleTimestampMs"), Some(&serde_json::json!(1774960496000u64)));
    assert_eq!(payload.get("velocity_world"), Some(&serde_json::json!({"x": 4.0, "y": 5.0, "z": 6.0})));
}

#[test]
fn device_imu_sample_keeps_unlocked_nonzero_translation() {
    let mut status = sample_status();
    status.dr_lock_position = Some(false);

    let sample = build_device_imu_sample(&status).expect("orientation should produce a sample");
    let payload = sample.value.as_object().expect("sample payload should be an object");

    assert_eq!(payload.get("translation"), Some(&serde_json::json!({"x": 1.5, "y": -2.0, "z": 0.25})));
}

#[test]
fn device_imu_sample_omits_zero_translation_even_when_unlocked() {
    let mut status = sample_status();
    status.dr_lock_position = Some(false);
    status.position_world = Some(ImuAxesPayload { x: 0.0, y: 0.0, z: 0.0 });

    let sample = build_device_imu_sample(&status).expect("orientation should produce a sample");
    let payload = sample.value.as_object().expect("sample payload should be an object");

    assert!(!payload.contains_key("translation"));
}
