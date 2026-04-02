use super::storage::{decode_snapshot_meta, latest_snapshot_id};

#[test]
fn latest_snapshot_id_strips_tar_suffix_and_path_segments() {
    assert_eq!(latest_snapshot_id("/tmp/demo-1.tar.gz"), "demo-1");
    assert_eq!(latest_snapshot_id("nested/demo-2"), "demo-2");
    assert_eq!(latest_snapshot_id(""), "");
}

#[test]
fn decode_snapshot_meta_rejects_missing_schema_version() {
    let raw = serde_json::json!({
        "id": "20260402-010203-manual",
        "created_utc": "2026-04-02T01:02:03Z",
        "host": "helios",
        "tag": "manual"
    });

    let err = decode_snapshot_meta(&serde_json::to_vec(&raw).expect("encode")).expect_err("missing schema version should fail");
    assert!(err.contains("missing required schema_version"));
}
