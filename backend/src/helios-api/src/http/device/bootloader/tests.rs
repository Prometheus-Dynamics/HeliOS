use std::path::PathBuf;

use super::support::{bootloader_status_with_config, canonicalize_existing_path, disk_from_partition_device, partition_device};
use super::types::BootloaderConfig;

#[test]
fn disk_from_partition_device_handles_mmc_nvme_and_sd_layouts() {
    assert_eq!(disk_from_partition_device("/dev/mmcblk0p4"), Some("/dev/mmcblk0".to_string()));
    assert_eq!(disk_from_partition_device("/dev/nvme0n1p7"), Some("/dev/nvme0n1".to_string()));
    assert_eq!(disk_from_partition_device("/dev/sda3"), Some("/dev/sda".to_string()));
    assert_eq!(disk_from_partition_device("/dev/mapper/root"), None);
}

#[test]
fn partition_device_preserves_partition_naming_conventions() {
    assert_eq!(partition_device("/dev/mmcblk0", 1), PathBuf::from("/dev/mmcblk0p1"));
    assert_eq!(partition_device("/dev/nvme0n1", 2), PathBuf::from("/dev/nvme0n1p2"));
    assert_eq!(partition_device("/dev/sda", 3), PathBuf::from("/dev/sda3"));
}

#[test]
fn bootloader_status_marks_update_required_when_required_version_differs() {
    let temp = tempfile::tempdir().expect("temp dir");
    let update_file = temp.path().join("pieeprom.upd");
    std::fs::write(&update_file, "firmware").expect("write update");

    let config = BootloaderConfig { required_version: Some("2025-12-08".to_string()), update_file: update_file.clone(), update_sig: None, boot_partition: temp.path().join("boot") };

    let (status, message) = bootloader_status_with_config(&config);
    assert!(status.update_available);
    if status.supported {
        let current = status.current_version.clone();
        let required = status.required_version.clone();
        assert_eq!(status.needs_update, current.as_deref() != required.as_deref());
    }
    assert_eq!(status.update_file, Some(update_file.display().to_string()));
    assert_eq!(status.status_message, None);
    if status.supported && status.needs_update {
        assert_eq!(message.as_deref(), Some("Bootloader update required to enable RP1 peripherals."));
    }
}

#[test]
fn canonicalize_existing_path_returns_none_for_missing_path() {
    let missing = format!("/tmp/helios-missing-{}", std::process::id());
    assert_eq!(canonicalize_existing_path(&missing), None);
}
