use std::path::PathBuf;

use lib_storage_layout::{PartitionRole, SlotScheme as LayoutSlotScheme, StorageLayoutManifest};
use tokio::fs;
use tokio::process::Command;
use tracing::{info, warn};

use crate::config::UpdaterConfig;
use crate::error::Result;
use crate::util::{by_label_path, current_slot_label, resolve_boot_dir_rw};

pub async fn run_post_boot_cleanup(config: &UpdaterConfig) -> Result<bool> {
    let Some(current_slot) = current_slot_label() else {
        return Ok(false);
    };

    let Some((boot_dir, mounted)) = resolve_boot_dir_rw().await? else {
        return Ok(false);
    };

    let ota_dir = boot_dir.join("helios").join("ota");
    let active_marker = read_trimmed(&ota_dir.join("active")).await;
    let cleaned_marker = ota_dir.join(format!("cleaned-{}", current_slot));
    let wipe_marker = ota_dir.join("wipe-once");
    let already_cleaned = fs::metadata(&cleaned_marker).await.is_ok();

    if active_marker.as_deref() != Some(current_slot.as_str()) || already_cleaned {
        if mounted {
            let _ = Command::new("umount").arg(&boot_dir).status().await;
        }
        return Ok(false);
    }

    cleanup_staged_image(config).await;
    handle_wipe_marker(&wipe_marker, &current_slot).await;

    if let Err(err) = fs::create_dir_all(&ota_dir).await {
        warn!(error = %err, path = %ota_dir.display(), "failed to ensure OTA directory during cleanup");
    }

    if let Err(err) = fs::write(&cleaned_marker, b"").await {
        warn!(error = %err, marker = %cleaned_marker.display(), "failed to write cleanup marker");
    }

    if mounted {
        let _ = Command::new("umount").arg(&boot_dir).status().await;
    }

    info!(slot = %current_slot, "post-update cleanup completed");
    Ok(true)
}

async fn cleanup_staged_image(config: &UpdaterConfig) {
    let data_ota = config.data_dir().join("ota");
    let last_applied_path = data_ota.join("last_applied");
    let entry = read_trimmed(&last_applied_path).await;
    if let Some(name) = entry {
        if !name.is_empty() {
            let candidate = data_ota.join(&name);
            if let Err(err) = fs::remove_file(&candidate).await {
                warn!(error = %err, path = %candidate.display(), "failed to remove staged image during cleanup");
            }
        }
        let _ = fs::remove_file(&last_applied_path).await;
    }
}

async fn handle_wipe_marker(marker: &PathBuf, current_slot: &str) {
    if fs::metadata(marker).await.is_err() {
        return;
    }

    if let Ok(layout) = StorageLayoutManifest::load_system() {
        if layout.slot_scheme == LayoutSlotScheme::Ext4Labels {
            let slot_a = layout.slot_name(PartitionRole::SlotA).ok();
            let slot_b = layout.slot_name(PartitionRole::SlotB).ok();
            let old_label = match (slot_a, slot_b) {
                (Some(slot_a), Some(slot_b)) if current_slot == slot_a => Some(slot_b),
                (Some(slot_a), Some(slot_b)) if current_slot == slot_b => Some(slot_a),
                _ => None,
            };
            if let Some(old_label) = old_label
                && let Some(old_dev) = by_label_path(old_label)
            {
                let _ = Command::new("umount").arg(&old_dev).status().await;
                let _ = Command::new("mkfs.ext4").args(["-F", "-L", old_label, &old_dev]).status().await;
            }
        } else {
            info!(slot = %current_slot, "wipe-once marker ignored for squashfs OTA layout");
        }
    } else if current_slot == "ACTIVE" || current_slot == "RESERVE" {
        let old_label = if current_slot == "ACTIVE" { "RESERVE" } else { "ACTIVE" };
        if let Some(old_dev) = by_label_path(old_label) {
            let _ = Command::new("umount").arg(&old_dev).status().await;
            let _ = Command::new("mkfs.ext4").args(["-F", "-L", old_label, &old_dev]).status().await;
        }
    }

    if let Err(err) = fs::remove_file(marker).await {
        warn!(error = %err, path = %marker.display(), "failed to remove wipe-once marker");
    }
}

async fn read_trimmed(path: &PathBuf) -> Option<String> {
    fs::read_to_string(path).await.ok().map(|s| s.trim().to_string())
}
