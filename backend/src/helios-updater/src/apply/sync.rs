use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::fs;
use tokio::process::Command;
use tokio::sync::{RwLock, broadcast::Sender};
use tracing::{info, warn};
use uuid::Uuid;

use crate::artifact::cache_usage_bytes;
use crate::config::UpdaterConfig;
use crate::error::{Error, Result};
use crate::ipc::{UpdateStage, UpdaterEvent};
use crate::state::ServiceState;
use crate::util::{
    BlockPartitionInfo, ProgressSender, SlotScheme, SlotSelection, available_bytes_for_path, blockdev_size_bytes, by_label_path, detect_ext4_partition_in_disk_image,
    detect_fat_partition_in_disk_image, detect_squashfs_partition_in_disk_image, ensure_directory, inspect_adjacent_partition, inspect_block_partition, resize_partition_end,
    resolve_boot_block_device, resolve_boot_dir_rw, rewrite_cmdline_root, sync_filesystem,
};

use super::{
    PERSISTED_FILE_SYNCS, PERSIST_NETWORKD_DIR, PERSIST_NETWORKD_PREFIX, REQUIRED_BOOTABLE_ROOT_PATHS, PersistedFileSync, SquashfsSlotResizePlan,
};
use super::progress::publish_snapshot;

pub(super) async fn cleanup_source_media_after_apply(config: &UpdaterConfig, update_id: Uuid, metadata: &super::ApplyManifestMetadata) {
    if !metadata.delete_image_after_apply {
        return;
    }
    let Some(media_path) = metadata.source_artifact_path.as_deref().map(str::trim).filter(|value| !value.is_empty()).map(PathBuf::from) else {
        return;
    };
    if !media_path.is_absolute() {
        return;
    }

    match fs::remove_file(&media_path).await {
        Ok(()) => info!(%update_id, path = %media_path.display(), "removed source OTA media file after apply"),
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => warn!(%update_id, path = %media_path.display(), error = %err, "failed to remove source OTA media file"),
    }

    let Some(filename) = sanitize_media_filename(media_path.to_string_lossy().as_ref()) else {
        return;
    };
    let is_media_path = media_path.parent().and_then(|dir| dir.file_name()).and_then(|name| name.to_str()).map(|name| name == "media").unwrap_or(false);
    if is_media_path {
        let media_meta_path = media_path
            .parent()
            .and_then(|media_dir| media_dir.parent().map(|api_data_dir| api_data_dir.join("media-meta").join(format!("{filename}.json"))))
            .unwrap_or_else(|| config.data_dir().join("api-data").join("media-meta").join(format!("{filename}.json")));
        match fs::remove_file(&media_meta_path).await {
            Ok(()) => {}
            Err(err) if err.kind() == ErrorKind::NotFound => {}
            Err(err) => warn!(%update_id, path = %media_meta_path.display(), error = %err, "failed to remove source OTA media metadata"),
        }
    }
}

pub(crate) async fn purge_update_dirs(config: &UpdaterConfig, update_id: Uuid) -> Result<()> {
    let cache_dir = config.cache_dir().join(update_id.to_string());
    if fs::metadata(&cache_dir).await.is_ok() {
        fs::remove_dir_all(&cache_dir).await?;
    }

    let work_dir = config.work_dir().join(update_id.to_string());
    if fs::metadata(&work_dir).await.is_ok() {
        fs::remove_dir_all(&work_dir).await?;
    }

    Ok(())
}

pub(super) async fn clear_completed_update_state(config: &UpdaterConfig, state: &Arc<RwLock<ServiceState>>, events: &Sender<UpdaterEvent>, update_id: Uuid) -> Result<()> {
    purge_update_dirs(config, update_id).await?;
    let cache_usage = cache_usage_bytes(config.cache_dir()).await?;

    {
        let mut guard = state.write().await;
        if matches!(guard.active_update.as_ref().map(|a| a.update_id), Some(id) if id == update_id) {
            guard.update_progress(UpdateStage::Idle, Some(0), None);
            guard.clear_active_update();
        }
        guard.cache_usage_bytes = cache_usage;
    }

    publish_snapshot(state, events).await;
    Ok(())
}

fn sanitize_media_filename(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Path::new(trimmed).file_name().map(|name| name.to_string_lossy().to_string())
}

pub(crate) async fn flash_image_to_target(expanded_path: &Path, slot_selection: &SlotSelection, data_dir: &Path, progress: Option<ProgressSender>) -> Result<()> {
    let target_device = slot_selection.target_device.as_str();
    let _ = Command::new("umount").arg(target_device).status().await;

    let mut offset = 0u64;
    let mut size: Option<u64> = None;
    let mut relabel = false;
    match slot_selection.scheme {
        SlotScheme::Ext4Labels => {
            if let Some((off, ext4_size)) = detect_ext4_partition_in_disk_image(expanded_path).await? {
                if let Some(target_bytes) = blockdev_size_bytes(target_device).await?
                    && ext4_size > target_bytes
                {
                    let size_mib = ext4_size / (1024 * 1024);
                    let target_mib = target_bytes / (1024 * 1024);
                    return Err(Error::InvalidState(format!(
                        "target partition {} is {} bytes ({} MiB) but image rootfs is {} bytes ({} MiB)",
                        target_device, target_bytes, target_mib, ext4_size, size_mib
                    )));
                }
                offset = off;
                size = Some(ext4_size);
            }
            relabel = true;
        }
        SlotScheme::SquashfsAb => {
            let Some((off, squashfs_size)) = detect_squashfs_partition_in_disk_image(expanded_path).await? else {
                return Err(Error::InvalidState(format!("no squashfs partition found inside OTA artifact {}; refusing to flash {}", expanded_path.display(), target_device)));
            };
            ensure_squashfs_target_capacity(target_device, squashfs_size, data_dir).await?;
            offset = off;
            size = Some(squashfs_size);
        }
    }

    let expanded_path = expanded_path.to_path_buf();
    let target_device = slot_selection.target_device.clone();
    let target_device_copy = target_device.clone();
    let handle = tokio::task::spawn_blocking(move || copy_image_to_target_blocking(&expanded_path, &target_device_copy, offset, size, progress));
    handle.await.map_err(|err| Error::Io(std::io::Error::other(err.to_string())))??;

    if relabel {
        relabel_target_filesystem(&slot_selection.target_slot, &target_device).await?;
    }

    Ok(())
}

async fn ensure_squashfs_target_capacity(target_device: &str, image_size_bytes: u64, data_dir: &Path) -> Result<()> {
    let Some(target_info) = inspect_block_partition(target_device).await? else {
        return Err(Error::InvalidState(format!("unable to inspect squashfs target slot {target_device}")));
    };
    let next_partition = inspect_adjacent_partition(target_device, 1).await?;
    let data_dir_available_bytes = available_bytes_for_path(data_dir).await?.unwrap_or(0);

    match plan_squashfs_slot_resize(&target_info, next_partition.as_ref(), image_size_bytes, data_dir_available_bytes) {
        SquashfsSlotResizePlan::Fits => Ok(()),
        SquashfsSlotResizePlan::GrowIntoGap { required_growth_bytes, gap_after_bytes, new_end_bytes_exclusive } => {
            info!(
                target_device = %target_info.device,
                disk = %target_info.disk,
                part = target_info.number,
                required_growth_bytes,
                gap_after_bytes,
                new_end_bytes_exclusive,
                "growing inactive squashfs slot before OTA flash"
            );
            resize_partition_end(&target_info.disk, target_info.number, new_end_bytes_exclusive).await
        }
        SquashfsSlotResizePlan::NeedsDataResize { required_growth_bytes, gap_after_bytes, additional_from_data_bytes, data_dir_available_bytes } => Err(Error::InvalidState(format!(
            "target squashfs slot {} is {} bytes but image rootfs needs {} bytes; OTA needs {} more bytes total, can reclaim {} bytes of post-slot slack, but still needs {} more bytes from DATA. {} currently has {} bytes free. Clear space there and retry once live DATA repartitioning is supported.",
            target_info.device,
            target_info.size_bytes(),
            image_size_bytes,
            required_growth_bytes,
            gap_after_bytes,
            additional_from_data_bytes,
            data_dir.display(),
            data_dir_available_bytes
        ))),
        SquashfsSlotResizePlan::ClearDataDir { required_growth_bytes, gap_after_bytes, additional_from_data_bytes, data_dir_available_bytes, clear_bytes } => Err(Error::InvalidState(format!(
            "target squashfs slot {} is {} bytes but image rootfs needs {} bytes; OTA needs {} more bytes total, can reclaim {} bytes of post-slot slack, but DATA only has {} bytes free and {} more bytes would have to come from DATA. Clear at least {} bytes from {} and retry.",
            target_info.device,
            target_info.size_bytes(),
            image_size_bytes,
            required_growth_bytes,
            gap_after_bytes,
            data_dir_available_bytes,
            additional_from_data_bytes,
            clear_bytes,
            data_dir.display()
        ))),
    }
}

pub(super) fn plan_squashfs_slot_resize(target_info: &BlockPartitionInfo, next_partition: Option<&BlockPartitionInfo>, image_size_bytes: u64, data_dir_available_bytes: u64) -> SquashfsSlotResizePlan {
    let required_capacity_bytes = align_up_bytes(image_size_bytes, target_info.sector_bytes);
    let target_bytes = target_info.size_bytes();
    if required_capacity_bytes <= target_bytes {
        return SquashfsSlotResizePlan::Fits;
    }

    let required_growth_bytes = required_capacity_bytes.saturating_sub(target_bytes);
    let gap_after_bytes = next_partition.map(|next| next.start_bytes().saturating_sub(target_info.end_bytes_exclusive())).unwrap_or(0);

    if required_growth_bytes <= gap_after_bytes {
        return SquashfsSlotResizePlan::GrowIntoGap { required_growth_bytes, gap_after_bytes, new_end_bytes_exclusive: target_info.end_bytes_exclusive().saturating_add(required_growth_bytes) };
    }

    let additional_from_data_bytes = required_growth_bytes.saturating_sub(gap_after_bytes);
    if data_dir_available_bytes >= additional_from_data_bytes {
        SquashfsSlotResizePlan::NeedsDataResize { required_growth_bytes, gap_after_bytes, additional_from_data_bytes, data_dir_available_bytes }
    } else {
        SquashfsSlotResizePlan::ClearDataDir {
            required_growth_bytes,
            gap_after_bytes,
            additional_from_data_bytes,
            data_dir_available_bytes,
            clear_bytes: additional_from_data_bytes.saturating_sub(data_dir_available_bytes),
        }
    }
}

const fn align_up_bytes(value: u64, align: u64) -> u64 {
    if align == 0 { value } else { value.div_ceil(align).saturating_mul(align) }
}

pub(super) async fn relabel_target_filesystem(target_label: &str, target_device: &str) -> Result<()> {
    let label_status = Command::new("tune2fs").args(["-L", target_label, target_device]).status().await.ok();
    let mut relabel_ok = label_status.is_some_and(|s| s.success());
    if !relabel_ok {
        let label_status = Command::new("e2label").args([target_device, target_label]).status().await.ok();
        relabel_ok = label_status.is_some_and(|s| s.success());
    }
    if !relabel_ok {
        let label_matches = by_label_path(target_label)
            .and_then(|p| std::fs::canonicalize(p).ok())
            .zip(std::fs::canonicalize(target_device).ok())
            .map(|(label_dev, target_dev)| label_dev == target_dev)
            .unwrap_or(false);
        if !label_matches {
            return Err(Error::InvalidState(format!("failed to relabel flashed filesystem {} -> {}", target_device, target_label)));
        }
        warn!(%target_label, target_device = %target_device, "relabel skipped; target already has desired label");
    }

    Ok(())
}

fn copy_image_to_target_blocking(expanded_path: &Path, target_device: &str, offset: u64, size: Option<u64>, progress: Option<ProgressSender>) -> Result<()> {
    use std::io::{Read, Seek, SeekFrom, Write};

    let mut input = std::fs::File::open(expanded_path).map_err(Error::Io)?;
    let metadata = input.metadata().map_err(Error::Io)?;
    let file_len = metadata.len();
    if offset > 0 {
        input.seek(SeekFrom::Start(offset)).map_err(Error::Io)?;
    }

    let total_bytes = size.or_else(|| file_len.checked_sub(offset));
    let mut remaining = size.unwrap_or_else(|| file_len.saturating_sub(offset));
    let mut written: u64 = 0;
    let mut buffer = vec![0u8; 4 * 1024 * 1024];

    let mut output = std::fs::OpenOptions::new().write(true).open(target_device).map_err(Error::Io)?;

    while remaining > 0 {
        let read_len = buffer.len().min(remaining as usize);
        let n = input.read(&mut buffer[..read_len]).map_err(Error::Io)?;
        if n == 0 {
            break;
        }
        output.write_all(&buffer[..n]).map_err(Error::Io)?;
        remaining = remaining.saturating_sub(n as u64);
        written = written.saturating_add(n as u64);
        if let Some(progress) = &progress {
            progress.report(written, total_bytes);
        }
    }

    output.flush().map_err(Error::Io)?;
    output.sync_all().map_err(Error::Io)?;
    Ok(())
}

pub(super) async fn update_boot_markers(config: &Arc<UpdaterConfig>, slot_selection: &SlotSelection) -> Result<()> {
    let Some((boot_dir, mounted)) = resolve_boot_dir_rw().await? else {
        return Err(crate::error::Error::InvalidState("cannot mount or find BOOT partition".into()));
    };
    let boot_path = boot_dir.as_path();

    let tryboot_root = slot_selection.target_device.to_string();
    let tryboot_content = format!("tryboot_once=1\ntryboot_root={}\n", tryboot_root);
    fs::write(boot_path.join("tryboot.txt"), tryboot_content).await.map_err(Error::Io)?;

    let cmdline_path = boot_path.join("cmdline.txt");
    let existing = fs::read_to_string(&cmdline_path).await.unwrap_or_default();
    let rewritten = rewrite_cmdline_root(&existing, &slot_selection.target_slot, &slot_selection.target_device).await;
    fs::write(&cmdline_path, rewritten).await.map_err(Error::Io)?;

    let ota_dir = boot_path.join("helios").join("ota");
    ensure_directory(&ota_dir).await?;
    let state_dir = config.data_dir().join("ota");
    ensure_directory(&state_dir).await?;
    let pending_value = slot_selection.target_slot.to_string();
    fs::write(ota_dir.join("pending"), pending_value).await.map_err(Error::Io)?;
    let _ = fs::write(ota_dir.join("active"), format!("{}\n", slot_selection.current_slot)).await;
    let _ = fs::write(ota_dir.join("reserve"), format!("{}\n", slot_selection.target_slot)).await;
    let _ = fs::write(ota_dir.join(format!("pending-{}", slot_selection.target_slot)), b"").await;
    let state_writes = [
        (state_dir.join("active"), format!("{}\n", slot_selection.current_slot)),
        (state_dir.join("reserve"), format!("{}\n", slot_selection.target_slot)),
        (state_dir.join("pending"), format!("{}\n", slot_selection.target_slot)),
    ];
    for (path, value) in state_writes {
        if let Err(err) = fs::write(&path, value).await {
            warn!(error = %err, path = %path.display(), "failed to write OTA state marker");
        }
    }

    sync_filesystem(boot_path).await?;

    if mounted {
        let _ = Command::new("umount").arg(boot_path).status().await;
    }

    Ok(())
}

pub(super) async fn sync_boot_from_artifact(artifact_path: &Path) -> Result<bool> {
    let Some((offset, size)) = detect_fat_partition_in_disk_image(artifact_path).await? else {
        return Ok(false);
    };
    let Some(boot_device) = resolve_boot_block_device() else {
        warn!(boot_image = %artifact_path.display(), "boot partition detected in artifact but no boot block device found; skipping boot sync");
        return Ok(false);
    };

    let _ = Command::new("umount").arg("/boot").status().await;
    let _ = Command::new("umount").arg("/mnt/boot").status().await;

    info!(boot_device = %boot_device, "writing BOOT partition from OTA artifact");
    let dd_args = vec![
        format!("if={}", artifact_path.display()),
        format!("of={}", boot_device),
        "bs=4M".to_string(),
        format!("skip={}", offset),
        format!("count={}", size),
        "iflag=skip_bytes,count_bytes".to_string(),
        "conv=fsync".to_string(),
    ];
    let status = Command::new("dd").args(&dd_args).status().await.map_err(Error::Io)?;
    if !status.success() {
        return Err(Error::InvalidState(format!("dd failed for BOOT from {}", artifact_path.display())));
    }
    Ok(true)
}

pub(super) async fn sync_boot_from_target(target_device: &str, work_dir: &Path) -> Result<()> {
    let mountpoint = work_dir.join("mnt-target");
    ensure_directory(&mountpoint).await?;

    let status = Command::new("mount").args(["-o", "ro", target_device, mountpoint.to_str().unwrap()]).status().await.map_err(Error::Io)?;
    if !status.success() {
        return Err(Error::InvalidState(format!("failed to mount flashed root {}", target_device)));
    }

    let root_boot = mountpoint.join("boot");
    if !root_boot.is_dir() {
        warn!(boot_path = %root_boot.display(), "boot directory missing in flashed root; skipping boot sync");
        let _ = Command::new("umount").arg(&mountpoint).status().await;
        return Ok(());
    }
    if !boot_dir_has_payload(&root_boot).await? {
        warn!(boot_path = %root_boot.display(), "boot directory empty in flashed root; skipping boot sync");
        let _ = Command::new("umount").arg(&mountpoint).status().await;
        return Ok(());
    }
    if !boot_dir_has_required_firmware(&root_boot).await? {
        warn!(boot_path = %root_boot.display(), "boot directory in flashed root missing required firmware files; skipping boot sync");
        let _ = Command::new("umount").arg(&mountpoint).status().await;
        return Ok(());
    }

    let result = async {
        let Some((boot_dir, boot_mounted)) = resolve_boot_dir_rw().await? else {
            return Err(Error::InvalidState("cannot mount BOOT to update firmware".into()));
        };

        clear_boot_dir_preserve_ota(&boot_dir).await?;
        copy_boot_tree(&root_boot, &boot_dir).await?;
        ensure_os_config_present(&boot_dir).await?;
        sync_filesystem(&boot_dir).await?;

        if boot_mounted {
            let _ = Command::new("umount").arg(&boot_dir).status().await;
        }
        Ok::<(), Error>(())
    }
    .await;

    let _ = Command::new("umount").arg(&mountpoint).status().await;
    result
}

pub(super) async fn sync_persisted_state(target_device: &str, work_dir: &Path) -> Result<()> {
    let mountpoint = work_dir.join("mnt-target-rw");
    ensure_directory(&mountpoint).await?;
    let status = Command::new("mount").args(["-o", "rw", target_device, mountpoint.to_str().unwrap()]).status().await.map_err(Error::Io)?;
    if !status.success() {
        return Err(Error::InvalidState(format!("failed to mount flashed root {} for config sync", target_device)));
    }

    let result = async {
        sync_persisted_networkd_into(&mountpoint, Path::new(PERSIST_NETWORKD_DIR)).await?;
        sync_persisted_files_into(&mountpoint).await
    }
    .await;
    sync_filesystem(&mountpoint).await.ok();
    let _ = Command::new("umount").arg(&mountpoint).status().await;
    result
}

pub(super) async fn sync_persisted_networkd_into(root: &Path, src_dir: &Path) -> Result<()> {
    let target_dir = root.join("etc/systemd/network");
    tokio::fs::create_dir_all(&target_dir).await.map_err(Error::Io)?;

    let mut entries = tokio::fs::read_dir(&target_dir).await.map_err(Error::Io)?;
    while let Some(entry) = entries.next_entry().await.map_err(Error::Io)? {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(PERSIST_NETWORKD_PREFIX) && name.ends_with(".network") {
            let _ = tokio::fs::remove_file(entry.path()).await;
        }
    }

    if !src_dir.is_dir() {
        return Ok(());
    }

    let mut src_entries = tokio::fs::read_dir(src_dir).await.map_err(Error::Io)?;
    while let Some(entry) = src_entries.next_entry().await.map_err(Error::Io)? {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with(PERSIST_NETWORKD_PREFIX) || !name.ends_with(".network") {
            continue;
        }
        let src_path = entry.path();
        let dst_path = target_dir.join(name.as_ref());
        tokio::fs::copy(&src_path, &dst_path).await.map_err(Error::Io)?;
    }

    Ok(())
}

async fn sync_persisted_files_into(root: &Path) -> Result<()> {
    sync_persisted_files_with_mappings_into(root, PERSISTED_FILE_SYNCS).await
}

pub(super) async fn sync_persisted_files_with_mappings_into(root: &Path, mappings: &[PersistedFileSync]) -> Result<()> {
    for mapping in mappings {
        let Some(src_path) = resolve_persisted_file_source(mapping).await? else {
            continue;
        };
        let target_path = root.join(mapping.target_path.trim_start_matches('/'));
        if let Some(parent) = target_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(Error::Io)?;
        }
        tokio::fs::copy(&src_path, &target_path).await.map_err(Error::Io)?;
    }
    Ok(())
}

async fn resolve_persisted_file_source(mapping: &PersistedFileSync) -> Result<Option<PathBuf>> {
    for candidate in mapping.source_candidates {
        match tokio::fs::metadata(candidate).await {
            Ok(meta) if meta.is_file() => return Ok(Some(PathBuf::from(candidate))),
            Ok(_) => continue,
            Err(err) if err.kind() == ErrorKind::NotFound => continue,
            Err(err) => return Err(Error::Io(err)),
        }
    }
    Ok(None)
}

pub(super) async fn validate_bootable_squashfs_root(target_device: &str, work_dir: &Path) -> Result<()> {
    let mountpoint = work_dir.join("mnt-target-validate");
    if fs::metadata(&mountpoint).await.is_ok() {
        let _ = Command::new("umount").arg(&mountpoint).status().await;
        let _ = fs::remove_dir_all(&mountpoint).await;
    }
    ensure_directory(&mountpoint).await?;

    let mount_status = Command::new("mount").args(["-t", "squashfs", "-o", "ro", target_device, mountpoint.to_str().unwrap()]).status().await.map_err(Error::Io)?;

    if !mount_status.success() {
        let _ = fs::remove_dir_all(&mountpoint).await;
        return Err(Error::InvalidState(format!("flashed squashfs slot {} is not mountable; refusing to switch boot slots", target_device)));
    }

    let missing = missing_bootable_root_paths(&mountpoint).await?;
    let _ = Command::new("umount").arg(&mountpoint).status().await;
    let _ = fs::remove_dir_all(&mountpoint).await;

    if !missing.is_empty() {
        return Err(Error::InvalidState(format!("flashed squashfs slot {} is not bootable; missing {}", target_device, missing.join(", "))));
    }

    Ok(())
}

pub(super) async fn missing_bootable_root_paths(root: &Path) -> Result<Vec<&'static str>> {
    let mut missing = Vec::new();
    for required in REQUIRED_BOOTABLE_ROOT_PATHS {
        let relative = required.trim_start_matches('/');
        if fs::symlink_metadata(root.join(relative)).await.is_err() {
            missing.push(*required);
        }
    }
    Ok(missing)
}

async fn boot_dir_has_payload(root_boot: &Path) -> Result<bool> {
    let mut entries = fs::read_dir(root_boot).await.map_err(Error::Io)?;
    while let Some(entry) = entries.next_entry().await.map_err(Error::Io)? {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "ota" || name == "lost+found" {
            continue;
        }
        return Ok(true);
    }
    Ok(false)
}

async fn clear_boot_dir_preserve_ota(boot_dir: &Path) -> Result<()> {
    let mut entries = fs::read_dir(boot_dir).await.map_err(Error::Io)?;
    while let Some(entry) = entries.next_entry().await.map_err(Error::Io)? {
        let name = entry.file_name();
        if name == "helios" || name == "os_config.json" {
            continue;
        }
        let path = entry.path();
        let meta = entry.metadata().await.map_err(Error::Io)?;
        if meta.is_dir() {
            fs::remove_dir_all(&path).await.map_err(Error::Io)?;
        } else {
            fs::remove_file(&path).await.map_err(Error::Io)?;
        }
    }
    Ok(())
}

async fn boot_dir_has_required_firmware(root_boot: &Path) -> Result<bool> {
    let required = ["start4.elf", "fixup4.dat", "config.txt", "cmdline.txt"];
    for name in required {
        if fs::metadata(root_boot.join(name)).await.is_err() {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn ensure_os_config_present(boot_dir: &Path) -> Result<()> {
    let os_config = boot_dir.join("os_config.json");
    if fs::metadata(&os_config).await.is_ok() {
        return Ok(());
    }
    const DEFAULT_OS_CONFIG_JSON: &str = r#"{
  "name": "HeliOS",
  "version": "unknown",
  "description": "HeliOS for Raspberry Pi 5 / CM5",
  "supports_secureboot": false,
  "supported_models": [
    "Raspberry Pi 5",
    "Raspberry Pi 500",
    "Raspberry Pi 5 Model B",
    "Raspberry Pi 5 Model B Rev 1.0",
    "Raspberry Pi 5 Model B Rev 1.1",
    "Raspberry Pi Compute Module 5",
    "Raspberry Pi Compute Module 5 Rev 1.0",
    "Raspberry Pi Compute Module 5 Rev 1.1",
    "Raspberry Pi 5 Compute Module",
    "Raspberry Pi 5 Compute Module Rev 1.0",
    "Raspberry Pi 5 Compute Module Rev 1.1",
    "Compute Module 5"
  ]
}
"#;
    fs::write(&os_config, DEFAULT_OS_CONFIG_JSON).await.map_err(Error::Io)?;
    warn!(path = %os_config.display(), "boot metadata missing after sync; wrote fallback os_config.json");
    Ok(())
}

async fn copy_boot_tree(src: &Path, dst: &Path) -> Result<()> {
    let mut stack = vec![(src.to_path_buf(), dst.to_path_buf())];
    while let Some((src_dir, dst_dir)) = stack.pop() {
        let mut entries = fs::read_dir(&src_dir).await.map_err(Error::Io)?;
        while let Some(entry) = entries.next_entry().await.map_err(Error::Io)? {
            let name = entry.file_name();
            if name == "ota" {
                continue;
            }
            let src_path = entry.path();
            let dst_path = dst_dir.join(&name);
            let meta = entry.metadata().await.map_err(Error::Io)?;
            let file_type = meta.file_type();
            if file_type.is_dir() {
                fs::create_dir_all(&dst_path).await.map_err(Error::Io)?;
                stack.push((src_path, dst_path));
            } else if file_type.is_symlink() {
                if let Ok(target) = fs::read_link(&src_path).await {
                    let resolved = if target.is_absolute() { target } else { src_path.parent().unwrap_or(&src_dir).join(target) };
                    if resolved.is_file() {
                        fs::copy(&resolved, &dst_path).await.map_err(Error::Io)?;
                    }
                }
            } else {
                fs::copy(&src_path, &dst_path).await.map_err(Error::Io)?;
            }
        }
    }
    Ok(())
}
