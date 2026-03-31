use super::{
    ApplyManifestMetadata, PersistedFileSync, SquashfsPreflightContext, SquashfsSlotResizePlan, parse_env_flag, reboot_failure_message, reboot_output_is_expected_success,
};
use super::preflight::{parse_apply_manifest_metadata, preflight_report_for_squashfs_plan};
use super::sync::{
    clear_completed_update_state, missing_bootable_root_paths, plan_squashfs_slot_resize, purge_update_dirs, sync_persisted_files_with_mappings_into, sync_persisted_networkd_into,
};
use crate::{
    artifact::ReleaseManifest,
    config::UpdaterConfig,
    ipc::{PreflightVerdict, UpdateStage, UpdaterEvent},
    state::ServiceState,
    util::{BlockPartitionInfo, SlotScheme, SlotSelection},
};
use std::{path::Path, sync::Arc};
use tokio::sync::{RwLock, broadcast};
use uuid::Uuid;

#[test]
fn reboot_success_status_is_accepted() {
    let output = std::process::Command::new("sh").arg("-c").arg("exit 0").output().expect("run shell");
    assert!(reboot_output_is_expected_success(&output));
}

#[test]
fn reboot_failure_message_prefers_stderr() {
    let output = std::process::Command::new("sh").arg("-c").arg("echo boom >&2; exit 2").output().expect("run shell");
    assert_eq!(reboot_failure_message(&output), "boom");
}

#[cfg(unix)]
#[test]
fn reboot_sigterm_is_treated_as_expected() {
    use std::os::unix::process::ExitStatusExt;
    let output = std::process::Output { status: std::process::ExitStatus::from_raw(15), stdout: Vec::new(), stderr: Vec::new() };
    assert!(reboot_output_is_expected_success(&output));
}

#[test]
fn parse_env_flag_accepts_truthy_values() {
    for raw in ["1", "true", "TRUE", "yes", "on", " On "] {
        assert!(parse_env_flag(raw), "expected truthy: {raw}");
    }
}

#[test]
fn parse_env_flag_rejects_non_truthy_values() {
    for raw in ["", "0", "false", "no", "off", "2", "enabled"] {
        assert!(!parse_env_flag(raw), "expected falsey: {raw}");
    }
}

#[test]
fn parse_apply_manifest_metadata_accepts_new_source_artifact_path() {
    let manifest = ReleaseManifest {
        update_id: None,
        version: None,
        artifacts: Vec::new(),
        metadata_json: r#"{"delete_image_after_apply":true,"source_artifact_path":"/var/lib/helios/updater/api-uploads/bundle.tar"}"#.into(),
    };
    let metadata = crate::artifact::StagedMetadata { manifest, artifacts: Vec::new(), staged_at: chrono::Utc::now() };

    let parsed = parse_apply_manifest_metadata(&metadata);

    assert!(parsed.delete_image_after_apply);
    assert_eq!(parsed.source_artifact_path.as_deref(), Some("/var/lib/helios/updater/api-uploads/bundle.tar"));
}

#[test]
fn parse_apply_manifest_metadata_accepts_legacy_source_media_path_alias() {
    let manifest = ReleaseManifest {
        update_id: None,
        version: None,
        artifacts: Vec::new(),
        metadata_json: r#"{"delete_image_after_apply":true,"source_media_path":"/var/lib/helios/api-data/media/update.tar"}"#.into(),
    };
    let metadata = crate::artifact::StagedMetadata { manifest, artifacts: Vec::new(), staged_at: chrono::Utc::now() };

    let parsed = parse_apply_manifest_metadata(&metadata);

    assert_eq!(parsed, ApplyManifestMetadata { delete_image_after_apply: true, source_artifact_path: Some("/var/lib/helios/api-data/media/update.tar".into()) });
}

#[tokio::test]
async fn purge_update_dirs_removes_cache_and_work_content() {
    let temp = tempfile::tempdir().expect("temp dir");
    let update_id = Uuid::new_v4();
    let cache_root = temp.path().join("cache");
    let work_root = temp.path().join("work");
    let cache_dir = cache_root.join(update_id.to_string());
    let work_dir = work_root.join(update_id.to_string());
    tokio::fs::create_dir_all(&cache_dir).await.expect("create cache dir");
    tokio::fs::create_dir_all(&work_dir).await.expect("create work dir");
    tokio::fs::write(cache_dir.join("bundle.tar"), b"cache").await.expect("write cache file");
    tokio::fs::write(work_dir.join("expanded.img"), b"work").await.expect("write work file");

    let config = UpdaterConfig::new(temp.path().join("updater.sock"), temp.path().join("updater.log")).with_cache_dir(&cache_root).with_work_dir(&work_root);

    purge_update_dirs(&config, update_id).await.expect("purge update dirs");

    assert!(tokio::fs::metadata(&cache_dir).await.is_err(), "cache dir should be removed");
    assert!(tokio::fs::metadata(&work_dir).await.is_err(), "work dir should be removed");
}

#[tokio::test]
async fn clear_completed_update_state_purges_dirs_and_resets_snapshot() {
    let temp = tempfile::tempdir().expect("temp dir");
    let update_id = Uuid::new_v4();
    let cache_root = temp.path().join("cache");
    let work_root = temp.path().join("work");
    let cache_dir = cache_root.join(update_id.to_string());
    let work_dir = work_root.join(update_id.to_string());
    tokio::fs::create_dir_all(&cache_dir).await.expect("create cache dir");
    tokio::fs::create_dir_all(&work_dir).await.expect("create work dir");
    tokio::fs::write(cache_dir.join("bundle.tar"), b"cache").await.expect("write cache file");
    tokio::fs::write(work_dir.join("expanded.img"), b"work").await.expect("write work file");

    let config = UpdaterConfig::new(temp.path().join("updater.sock"), temp.path().join("updater.log")).with_cache_dir(&cache_root).with_work_dir(&work_root);

    let state = Arc::new(RwLock::new(ServiceState::default()));
    {
        let mut guard = state.write().await;
        guard.ensure_active_update(update_id);
        guard.update_progress(UpdateStage::Complete, Some(100), None);
        guard.cache_usage_bytes = 999;
    }

    let (events, mut rx) = broadcast::channel(8);

    clear_completed_update_state(&config, &state, &events, update_id).await.expect("clear completed update state");

    let guard = state.read().await;
    assert!(guard.active_update.is_none(), "completed update should be cleared");
    assert_eq!(guard.cache_usage_bytes, 0, "cache usage should be refreshed after purge");
    drop(guard);

    assert!(tokio::fs::metadata(&cache_dir).await.is_err(), "cache dir should be removed");
    assert!(tokio::fs::metadata(&work_dir).await.is_err(), "work dir should be removed");

    match rx.recv().await.expect("snapshot event") {
        UpdaterEvent::StateSnapshot { active_update, cache_usage_bytes } => {
            assert!(active_update.is_none(), "snapshot should report idle updater");
            assert_eq!(cache_usage_bytes, 0);
        }
        other => panic!("expected state snapshot, got {other:?}"),
    }
}

#[tokio::test]
async fn sync_persisted_files_prefers_data_candidate() {
    let temp = tempfile::tempdir().expect("temp dir");
    let root = temp.path().join("root");
    let primary = temp.path().join("primary/team");
    let legacy = temp.path().join("legacy/team");
    tokio::fs::create_dir_all(primary.parent().expect("primary parent")).await.expect("create primary parent");
    tokio::fs::create_dir_all(legacy.parent().expect("legacy parent")).await.expect("create legacy parent");
    tokio::fs::write(&primary, "2468\n").await.expect("write primary");
    tokio::fs::write(&legacy, "1111\n").await.expect("write legacy");

    let primary_str: &'static str = Box::leak(primary.display().to_string().into_boxed_str());
    let legacy_str: &'static str = Box::leak(legacy.display().to_string().into_boxed_str());
    let candidates: &'static [&'static str] = Box::leak(vec![primary_str, legacy_str].into_boxed_slice());
    let mappings = [PersistedFileSync { source_candidates: candidates, target_path: "/etc/helios/team" }];

    sync_persisted_files_with_mappings_into(&root, &mappings).await.expect("sync files");

    let written = tokio::fs::read_to_string(root.join("etc/helios/team")).await.expect("read target");
    assert_eq!(written, "2468\n");
}

#[tokio::test]
async fn sync_persisted_networkd_removes_stale_target_files_when_source_missing() {
    let temp = tempfile::tempdir().expect("temp dir");
    let root = temp.path().join("root");
    let target_dir = root.join("etc/systemd/network");
    tokio::fs::create_dir_all(&target_dir).await.expect("create target dir");
    tokio::fs::write(target_dir.join("00-helios-persisted-eth0.network"), "stale").await.expect("write stale file");
    tokio::fs::write(target_dir.join("10-default.network"), "keep").await.expect("write packaged file");

    sync_persisted_networkd_into(&root, Path::new("/definitely/missing")).await.expect("sync networkd");

    assert!(!target_dir.join("00-helios-persisted-eth0.network").exists());
    assert!(target_dir.join("10-default.network").exists());
}

#[tokio::test]
async fn bootable_root_validation_reports_missing_paths() {
    let temp = tempfile::tempdir().expect("temp dir");
    let root = temp.path().join("root");
    tokio::fs::create_dir_all(root.join("usr/lib/systemd")).await.expect("create systemd dir");
    tokio::fs::create_dir_all(root.join("etc")).await.expect("create etc dir");
    tokio::fs::write(root.join("usr/lib/systemd/systemd"), b"systemd").await.expect("write systemd");
    tokio::fs::write(root.join("etc/os-release"), b"NAME=Helios\n").await.expect("write os-release");

    let missing = missing_bootable_root_paths(&root).await.expect("missing paths");
    assert!(missing.contains(&"/sbin/init"));
    assert!(missing.contains(&"/bin/sh"));
    assert!(missing.contains(&"/lib"));
    assert!(missing.contains(&"/lib64"));
    assert!(!missing.contains(&"/usr/lib/systemd/systemd"));
    assert!(!missing.contains(&"/etc/os-release"));
}

#[cfg(unix)]
#[tokio::test]
async fn bootable_root_validation_accepts_expected_layout() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("temp dir");
    let root = temp.path().join("root");
    tokio::fs::create_dir_all(root.join("usr/lib/systemd")).await.expect("create systemd dir");
    tokio::fs::create_dir_all(root.join("sbin")).await.expect("create sbin dir");
    tokio::fs::create_dir_all(root.join("bin")).await.expect("create bin dir");
    tokio::fs::create_dir_all(root.join("lib")).await.expect("create lib dir");
    tokio::fs::create_dir_all(root.join("etc")).await.expect("create etc dir");
    tokio::fs::write(root.join("usr/lib/systemd/systemd"), b"systemd").await.expect("write systemd");
    tokio::fs::write(root.join("bin/sh"), b"#!/bin/sh\n").await.expect("write shell");
    tokio::fs::write(root.join("etc/os-release"), b"NAME=Helios\n").await.expect("write os-release");
    symlink("../usr/lib/systemd/systemd", root.join("sbin/init")).expect("symlink init");
    symlink("lib", root.join("lib64")).expect("symlink lib64");

    let missing = missing_bootable_root_paths(&root).await.expect("missing paths");
    assert!(missing.is_empty(), "unexpected missing paths: {missing:?}");
}

#[test]
fn squashfs_resize_plan_grows_into_post_slot_gap() {
    let target = BlockPartitionInfo { device: "/dev/mmcblk0p3".to_string(), disk: "/dev/mmcblk0".to_string(), number: 3, sector_bytes: 512, start_sectors: 0, size_sectors: 188_416 };
    let next = BlockPartitionInfo {
        device: "/dev/mmcblk0p4".to_string(),
        disk: "/dev/mmcblk0".to_string(),
        number: 4,
        sector_bytes: 512,
        start_sectors: target.size_sectors + 8_192,
        size_sectors: 1_000_000,
    };

    let plan = plan_squashfs_slot_resize(&target, Some(&next), target.size_bytes() + 671_744, 0);
    assert_eq!(plan, SquashfsSlotResizePlan::GrowIntoGap { required_growth_bytes: 671_744, gap_after_bytes: 4_194_304, new_end_bytes_exclusive: target.end_bytes_exclusive() + 671_744 });
}

#[test]
fn squashfs_resize_plan_requests_clear_space_when_data_free_is_short() {
    let target = BlockPartitionInfo { device: "/dev/mmcblk0p3".to_string(), disk: "/dev/mmcblk0".to_string(), number: 3, sector_bytes: 512, start_sectors: 0, size_sectors: 188_416 };
    let next = BlockPartitionInfo {
        device: "/dev/mmcblk0p4".to_string(),
        disk: "/dev/mmcblk0".to_string(),
        number: 4,
        sector_bytes: 512,
        start_sectors: target.size_sectors + 4_096,
        size_sectors: 1_000_000,
    };

    let plan = plan_squashfs_slot_resize(&target, Some(&next), target.size_bytes() + 6_291_456, 1_048_576);
    assert_eq!(
        plan,
        SquashfsSlotResizePlan::ClearDataDir {
            required_growth_bytes: 6_291_456,
            gap_after_bytes: 2_097_152,
            additional_from_data_bytes: 4_194_304,
            data_dir_available_bytes: 1_048_576,
            clear_bytes: 3_145_728,
        }
    );
}

#[test]
fn squashfs_preflight_marks_gap_growth_as_ready() {
    let target = BlockPartitionInfo { device: "/dev/mmcblk0p3".to_string(), disk: "/dev/mmcblk0".to_string(), number: 3, sector_bytes: 512, start_sectors: 0, size_sectors: 188_416 };
    let slot_selection = SlotSelection { current_slot: "ROOT_A".into(), target_slot: "ROOT_B".into(), target_device: target.device.clone(), single_slot: false, scheme: SlotScheme::SquashfsAb };
    let report = preflight_report_for_squashfs_plan(
        SquashfsPreflightContext {
            update_id: Uuid::nil(),
            artifact_kind: Some("disk-image"),
            slot_selection: &slot_selection,
            target_info: &target,
            image_size_bytes: target.size_bytes() + 671_744,
            data_dir_available_bytes: 0,
            work_dir_available_bytes: 8_388_608,
            gap_after_bytes: 4_194_304,
        },
        SquashfsSlotResizePlan::GrowIntoGap { required_growth_bytes: 671_744, gap_after_bytes: 4_194_304, new_end_bytes_exclusive: target.end_bytes_exclusive() + 671_744 },
    );

    assert!(report.ready);
    assert_eq!(report.verdict, PreflightVerdict::GrowIntoGap);
    assert_eq!(report.target_size_bytes, Some(target.size_bytes()));
    assert_eq!(report.gap_after_bytes, Some(4_194_304));
}

#[test]
fn squashfs_preflight_marks_clear_data_dir_as_blocking() {
    let target = BlockPartitionInfo { device: "/dev/mmcblk0p3".to_string(), disk: "/dev/mmcblk0".to_string(), number: 3, sector_bytes: 512, start_sectors: 0, size_sectors: 188_416 };
    let slot_selection = SlotSelection { current_slot: "ROOT_A".into(), target_slot: "ROOT_B".into(), target_device: target.device.clone(), single_slot: false, scheme: SlotScheme::SquashfsAb };
    let report = preflight_report_for_squashfs_plan(
        SquashfsPreflightContext {
            update_id: Uuid::nil(),
            artifact_kind: Some("disk-image"),
            slot_selection: &slot_selection,
            target_info: &target,
            image_size_bytes: target.size_bytes() + 6_291_456,
            data_dir_available_bytes: 1_048_576,
            work_dir_available_bytes: 8_388_608,
            gap_after_bytes: 2_097_152,
        },
        SquashfsSlotResizePlan::ClearDataDir {
            required_growth_bytes: 6_291_456,
            gap_after_bytes: 2_097_152,
            additional_from_data_bytes: 4_194_304,
            data_dir_available_bytes: 1_048_576,
            clear_bytes: 3_145_728,
        },
    );

    assert!(!report.ready);
    assert_eq!(report.verdict, PreflightVerdict::ClearDataDir);
    assert_eq!(report.additional_from_data_bytes, Some(4_194_304));
    assert_eq!(report.clear_bytes, Some(3_145_728));
}
