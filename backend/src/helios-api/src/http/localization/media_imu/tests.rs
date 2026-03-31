use uuid::Uuid;

use lib_sensors::dto::{ImuAxesPayload, ImuOrientationPayload, ImuStatusPayload};

use super::binding::{MEDIA_IMU_EXTERNAL_PREFIX, media_name_candidates, parse_media_imu_stream_id};
use super::sampling::imu_status_to_pose_sample;

#[test]
fn media_name_candidates_strip_replay_suffixes() {
    let candidates = media_name_candidates(std::path::Path::new("/tmp/capture.replay.30.mp4"));
    assert_eq!(candidates, vec!["capture.replay.30.mp4".to_string(), "capture".to_string()]);

    let candidates = media_name_candidates(std::path::Path::new("/tmp/capture.replay.mp4"));
    assert_eq!(candidates, vec!["capture.replay.mp4".to_string(), "capture".to_string()]);
}

#[test]
fn media_imu_source_id_round_trips_stream_uuid() {
    let stream_id = Uuid::new_v4();
    let source_id = format!("{MEDIA_IMU_EXTERNAL_PREFIX}{stream_id}");
    assert_eq!(parse_media_imu_stream_id(&source_id), Some(stream_id));
    assert_eq!(parse_media_imu_stream_id("media-imu-invalid"), None);
}

#[test]
fn pose_sample_keeps_translation_and_time_index() {
    let status = ImuStatusPayload {
        orientation: Some(ImuOrientationPayload { roll: 0.5, pitch: 1.0, yaw: 1.5, quaternion: None }),
        position_world: Some(ImuAxesPayload { x: 1.0, y: 2.0, z: 3.0 }),
        linear_speed_mps: Some(4.0),
        is_still: Some(false),
        ..ImuStatusPayload::default()
    };

    let sample = imu_status_to_pose_sample(&status, 1234).expect("orientation should produce a pose sample");
    let payload = sample.value.as_object().expect("payload should be an object");

    assert_eq!(payload.get("t_ms"), Some(&serde_json::json!(1234)));
    assert_eq!(payload.get("translation"), Some(&serde_json::json!({"x": 1.0, "y": 2.0, "z": 3.0})));
    assert_eq!(payload.get("linear_speed_mps"), Some(&serde_json::json!(4.0)));
    assert_eq!(payload.get("is_still"), Some(&serde_json::json!(false)));
}
