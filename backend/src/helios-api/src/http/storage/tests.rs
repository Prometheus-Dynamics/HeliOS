use std::path::PathBuf;

use super::health::{StorageProbeSnapshot, decode_mount_field, evaluate_storage_health, parse_code_list, parse_self_check_report};
use super::paths::sanitize_name;

#[test]
fn sanitize_name_trims_and_strips_paths() {
    assert_eq!(sanitize_name(" /tmp/plugin.so ").as_deref(), Some("plugin.so"));
    assert_eq!(sanitize_name("nested/dir/file.txt").as_deref(), Some("file.txt"));
}

#[test]
fn sanitize_name_rejects_empty_input() {
    assert!(sanitize_name("   ").is_none());
}

#[test]
fn overlay_tmpfs_fallback_is_reported() {
    let snapshot = StorageProbeSnapshot {
        root_fs_type: Some("overlay".to_string()),
        data_fs_type: Some("tmpfs".to_string()),
        overlay_data_fs_type: Some("tmpfs".to_string()),
        data_partition_present: false,
        data_mountpoint_present: true,
        overlay_data_mountpoint_present: true,
        ota_pending_present: false,
        self_check_repairs: Vec::new(),
        self_check_failures: Vec::new(),
    };

    let health = evaluate_storage_health(&snapshot);
    assert!(health.issues.iter().any(|issue| issue.code == "overlay_tmpfs_fallback"));
    assert!(health.issues.iter().any(|issue| issue.code == "data_partition_ephemeral"));
}

#[test]
fn data_partition_unmounted_is_reported() {
    let snapshot = StorageProbeSnapshot {
        root_fs_type: Some("overlay".to_string()),
        data_fs_type: None,
        overlay_data_fs_type: Some("ext4".to_string()),
        data_partition_present: true,
        data_mountpoint_present: false,
        overlay_data_mountpoint_present: true,
        ota_pending_present: false,
        self_check_repairs: Vec::new(),
        self_check_failures: Vec::new(),
    };

    let health = evaluate_storage_health(&snapshot);
    assert!(health.issues.iter().any(|issue| issue.code == "data_partition_unmounted"));
}

#[test]
fn benign_state_directory_repair_is_not_reported_as_health_issue() {
    let snapshot = StorageProbeSnapshot {
        root_fs_type: Some("overlay".to_string()),
        data_fs_type: Some("ext4".to_string()),
        overlay_data_fs_type: Some("ext4".to_string()),
        data_partition_present: true,
        data_mountpoint_present: true,
        overlay_data_mountpoint_present: true,
        ota_pending_present: false,
        self_check_repairs: vec!["helios_state_dirs_created".to_string()],
        self_check_failures: Vec::new(),
    };

    let health = evaluate_storage_health(&snapshot);
    assert!(!health.issues.iter().any(|issue| issue.code == "self_repair_applied"));
}

#[test]
fn non_benign_repairs_remain_reported() {
    let snapshot = StorageProbeSnapshot {
        root_fs_type: Some("overlay".to_string()),
        data_fs_type: Some("ext4".to_string()),
        overlay_data_fs_type: Some("ext4".to_string()),
        data_partition_present: true,
        data_mountpoint_present: true,
        overlay_data_mountpoint_present: true,
        ota_pending_present: false,
        self_check_repairs: vec!["helios_state_dirs_created".to_string(), "data_partition_mounted".to_string()],
        self_check_failures: Vec::new(),
    };

    let health = evaluate_storage_health(&snapshot);
    let issue = health.issues.iter().find(|issue| issue.code == "self_repair_applied").expect("expected non-benign repair issue");
    assert!(issue.description.contains("mounted the DATA partition"));
    assert!(!issue.description.contains("created missing HeliOS state directories"));
}

#[test]
fn direct_squashfs_root_is_reported() {
    let snapshot = StorageProbeSnapshot {
        root_fs_type: Some("squashfs".to_string()),
        data_fs_type: None,
        overlay_data_fs_type: None,
        data_partition_present: false,
        data_mountpoint_present: false,
        overlay_data_mountpoint_present: false,
        ota_pending_present: false,
        self_check_repairs: Vec::new(),
        self_check_failures: Vec::new(),
    };

    let health = evaluate_storage_health(&snapshot);
    assert!(health.issues.iter().any(|issue| issue.code == "root_overlay_missing"));
    assert!(health.issues.iter().any(|issue| issue.code == "data_partition_missing"));
}

#[test]
fn mount_field_decodes_octal_escapes() {
    assert_eq!(decode_mount_field("/path/with\\040space"), PathBuf::from("/path/with space"));
}

#[test]
fn ota_pending_is_reported() {
    let snapshot = StorageProbeSnapshot {
        root_fs_type: Some("overlay".to_string()),
        data_fs_type: Some("ext4".to_string()),
        overlay_data_fs_type: Some("ext4".to_string()),
        data_partition_present: true,
        data_mountpoint_present: true,
        overlay_data_mountpoint_present: true,
        ota_pending_present: true,
        self_check_repairs: Vec::new(),
        self_check_failures: Vec::new(),
    };

    let health = evaluate_storage_health(&snapshot);
    assert!(health.issues.iter().any(|issue| issue.code == "ota_pending_confirmation"));
}

#[test]
fn self_repair_outcomes_are_reported() {
    let snapshot = StorageProbeSnapshot {
        root_fs_type: Some("overlay".to_string()),
        data_fs_type: Some("ext4".to_string()),
        overlay_data_fs_type: Some("ext4".to_string()),
        data_partition_present: true,
        data_mountpoint_present: true,
        overlay_data_mountpoint_present: true,
        ota_pending_present: false,
        self_check_repairs: vec!["data_partition_mounted".to_string()],
        self_check_failures: vec!["overlay_data_bind_failed".to_string()],
    };

    let health = evaluate_storage_health(&snapshot);
    assert!(health.issues.iter().any(|issue| issue.code == "self_repair_applied"));
    assert!(health.issues.iter().any(|issue| issue.code == "self_repair_failed"));
}

#[test]
fn self_check_report_parses_env_lists() {
    let path = std::env::temp_dir().join(format!("helios-os-self-check-{}.env", std::process::id()));
    std::fs::write(&path, "REPAIRS=data_partition_mounted,overlay_data_bound\nFAILURES=overlay_data_bind_failed\n").expect("write report");
    let report = parse_self_check_report(&path).expect("parse report");
    let _ = std::fs::remove_file(&path);

    assert_eq!(report.repairs, vec!["data_partition_mounted".to_string(), "overlay_data_bound".to_string()]);
    assert_eq!(report.failures, vec!["overlay_data_bind_failed".to_string()]);
    assert_eq!(parse_code_list("a,b,c"), vec!["a".to_string(), "b".to_string(), "c".to_string()]);
}
