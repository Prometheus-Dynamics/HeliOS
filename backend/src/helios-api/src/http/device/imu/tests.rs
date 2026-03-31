use lib_sensors::dto::ImuUpdateRequest;
use serde_json::json;

use super::{
    status::as_u64,
    update::{ImuUpdate, is_valid_axis_reference},
};

#[test]
fn imu_update_rejects_invalid_positive_fields() {
    match ImuUpdate::try_from_request(ImuUpdateRequest { dr_max_speed_mps: Some(0.0), ..ImuUpdateRequest::default() }) {
        Err(err) => assert!(err.to_string().contains("dr_max_speed_mps")),
        Ok(_) => panic!("request should be rejected"),
    }

    match ImuUpdate::try_from_request(ImuUpdateRequest { dr_velocity_damp_tau_seconds: Some(f64::NAN), ..ImuUpdateRequest::default() }) {
        Err(err) => assert!(err.to_string().contains("dr_velocity_damp_tau_seconds")),
        Ok(_) => panic!("request should be rejected"),
    }
}

#[test]
fn imu_update_drops_false_toggle_flags() {
    let update = ImuUpdate::try_from_request(ImuUpdateRequest { snap_gravity: Some(false), reset_pose: Some(false), ..ImuUpdateRequest::default() }).expect("request should normalize");
    assert!(update.is_empty());
    assert_eq!(update.into_json(), json!({}));
}

#[test]
fn axis_reference_validation_accepts_expected_variants() {
    for axis in ["+x", "x-", "-y", "z", "Y+"] {
        assert!(is_valid_axis_reference(axis), "{axis} should be accepted");
    }
    for axis in ["", "pitch", "+w", "xx"] {
        assert!(!is_valid_axis_reference(axis), "{axis} should be rejected");
    }
}

#[test]
fn as_u64_accepts_integer_and_rounds_non_negative_floats() {
    assert_eq!(as_u64(&json!(50)), Some(50));
    assert_eq!(as_u64(&json!(49.6)), Some(50));
    assert_eq!(as_u64(&json!(-1.0)), None);
    assert_eq!(as_u64(&json!("50")), None);
}
