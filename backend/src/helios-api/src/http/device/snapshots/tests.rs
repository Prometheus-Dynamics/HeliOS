use super::storage::latest_snapshot_id;

#[test]
fn latest_snapshot_id_strips_tar_suffix_and_path_segments() {
    assert_eq!(latest_snapshot_id("/tmp/demo-1.tar.gz"), "demo-1");
    assert_eq!(latest_snapshot_id("nested/demo-2"), "demo-2");
    assert_eq!(latest_snapshot_id(""), "");
}
