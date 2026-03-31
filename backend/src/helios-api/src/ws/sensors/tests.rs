use super::hub::SnapshotPayload;
use super::session::{Kind, build_payload, handle_client_message};
use std::collections::BTreeSet;
use std::time::Duration;

#[test]
fn subscribe_requires_valid_kinds() {
    let mut requested = BTreeSet::new();
    let mut min_interval = Duration::from_millis(100);

    let error = handle_client_message(
        r#"{"op":"subscribe","kinds":["bogus"],"interval_ms":250}"#,
        &mut requested,
        &mut min_interval,
    )
    .unwrap_err();

    assert!(error.contains("unsupported kinds"));
    assert!(requested.is_empty());
    assert_eq!(min_interval, Duration::from_millis(250));
}

#[test]
fn subscribe_populates_requested_kinds_and_interval() {
    let mut requested = BTreeSet::new();
    let mut min_interval = Duration::from_millis(100);

    handle_client_message(
        r#"{"op":"subscribe","kinds":["imu","power","firmware"],"interval_ms":500}"#,
        &mut requested,
        &mut min_interval,
    )
    .unwrap();

    assert_eq!(requested.len(), 3);
    assert!(requested.contains(&Kind::Imu));
    assert!(requested.contains(&Kind::Power));
    assert!(requested.contains(&Kind::Firmware));
    assert_eq!(min_interval, Duration::from_millis(500));
}

#[test]
fn unsubscribe_without_kinds_clears_everything() {
    let mut requested = BTreeSet::from([Kind::Imu, Kind::Power]);
    let mut min_interval = Duration::from_millis(100);

    handle_client_message(r#"{"op":"unsubscribe"}"#, &mut requested, &mut min_interval).unwrap();

    assert!(requested.is_empty());
}

#[test]
fn build_payload_returns_none_when_requested_data_is_absent() {
    let requested = BTreeSet::from([Kind::Imu, Kind::Power, Kind::Lighting]);
    assert!(build_payload(&requested, &SnapshotPayload::default()).is_none());
}
