use std::env;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::ipc::{UpdateStage, UpdaterEvent};
use serde::Deserialize;
use tokio::fs;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::sync::{RwLock, broadcast::Sender};
use tokio::task::JoinHandle;
use tracing::{info, warn};
use uuid::Uuid;

use crate::artifact::{StagedMetadata, cache_usage_bytes, load_metadata, staged_to_url_artifacts};
use crate::config::UpdaterConfig;
use crate::error::{Error, Result};
use crate::state::ServiceState;
use crate::util::{
    ProgressSender, ProgressUpdate, SlotScheme, SlotSelection, StreamFlashOutcome, blockdev_size_bytes, by_label_path, decompress_if_needed, detect_compression_kind,
    detect_ext4_partition_in_disk_image, detect_fat_partition_in_disk_image, detect_squashfs_partition_in_disk_image, ensure_directory, flash_compressed_image_to_target, resolve_boot_block_device,
    resolve_boot_dir_rw, rewrite_cmdline_root, select_target_slot, sync_filesystem,
};

#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(test)]
static TEST_FAKE_APPLY: AtomicUsize = AtomicUsize::new(0);

const APPLY_PROGRESS_START: u8 = 10;
const APPLY_PROGRESS_END: u8 = 85;
const PERSIST_NETWORKD_DIR: &str = "/var/lib/helios/networkd";
const PERSIST_NETWORKD_PREFIX: &str = "00-helios-persisted-";

#[derive(Debug, Clone, Copy)]
struct PersistedFileSync {
    source_candidates: &'static [&'static str],
    target_path: &'static str,
}

const PERSISTED_FILE_SYNCS: &[PersistedFileSync] = &[
    PersistedFileSync { source_candidates: &["/var/lib/helios/hostname", "/etc/hostname"], target_path: "/etc/hostname" },
    PersistedFileSync { source_candidates: &["/var/lib/helios/team", "/etc/helios/team"], target_path: "/etc/helios/team" },
    PersistedFileSync { source_candidates: &["/var/lib/helios/nt4.json", "/etc/helios/nt4.json"], target_path: "/etc/helios/nt4.json" },
    PersistedFileSync { source_candidates: &["/var/lib/helios/peers.json", "/etc/helios/peers.json"], target_path: "/etc/helios/peers.json" },
    PersistedFileSync { source_candidates: &["/var/lib/helios/usb-power.env", "/etc/helios/usb-power.env"], target_path: "/etc/helios/usb-power.env" },
    PersistedFileSync { source_candidates: &["/var/lib/helios/leds.toml", "/etc/helios/leds.toml"], target_path: "/etc/helios/leds.toml" },
    PersistedFileSync { source_candidates: &["/var/lib/helios/led-animations.json", "/etc/helios/led-animations.json"], target_path: "/etc/helios/led-animations.json" },
    PersistedFileSync { source_candidates: &["/var/lib/helios/sensors.toml", "/etc/helios/sensors.toml"], target_path: "/etc/helios/sensors.toml" },
    PersistedFileSync { source_candidates: &["/var/lib/helios/fan.toml", "/etc/helios/fan.toml"], target_path: "/etc/helios/fan.toml" },
];

#[derive(Debug, Clone, Deserialize)]
struct ApplyManifestMetadata {
    #[serde(default = "default_true")]
    delete_image_after_apply: bool,
    #[serde(default)]
    source_media_path: Option<String>,
}

impl Default for ApplyManifestMetadata {
    fn default() -> Self {
        Self { delete_image_after_apply: true, source_media_path: None }
    }
}

const fn default_true() -> bool {
    true
}

#[cfg(test)]
fn fake_apply_requested() -> bool {
    std::env::var_os("UPDATER_FAKE_APPLY").is_some() || TEST_FAKE_APPLY.load(Ordering::Relaxed) > 0
}

#[cfg(not(test))]
fn fake_apply_requested() -> bool {
    std::env::var_os("UPDATER_FAKE_APPLY").is_some()
}

fn parse_env_flag(raw: &str) -> bool {
    matches!(raw.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

fn env_flag_enabled(name: &str) -> bool {
    env::var(name).map(|v| parse_env_flag(&v)).unwrap_or(false)
}

#[cfg(test)]
pub(crate) fn set_fake_apply_for_tests(enabled: bool) {
    if enabled {
        TEST_FAKE_APPLY.fetch_add(1, Ordering::Relaxed);
    } else {
        let _ = TEST_FAKE_APPLY.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| current.checked_sub(1));
    }
}

pub fn spawn_apply_job(config: Arc<UpdaterConfig>, state: Arc<RwLock<ServiceState>>, events: Sender<UpdaterEvent>, update_id: Uuid) -> JoinHandle<()> {
    tokio::spawn(run_apply_job(config, state, events, update_id))
}

async fn run_apply_job(config: Arc<UpdaterConfig>, state: Arc<RwLock<ServiceState>>, events: Sender<UpdaterEvent>, update_id: Uuid) {
    if let Err(err) = apply_job_inner(&config, &state, &events, update_id).await {
        warn!(%err, %update_id, "apply job failed");
        let work_dir = config.work_dir().join(update_id.to_string());
        if fs::metadata(&work_dir).await.is_ok()
            && let Err(err) = fs::remove_dir_all(&work_dir).await
        {
            warn!(error = %err, path = %work_dir.display(), "failed to clean work dir after apply failure");
        }
        {
            let mut guard = state.write().await;
            guard.update_progress(UpdateStage::RolledBack, Some(0), Some(err.to_string()));
        }
        publish_snapshot(&state, &events).await;
        let reason = err.to_string();
        let _ = events.send(UpdaterEvent::RollbackTriggered { update_id, reason });
    }
}

async fn apply_job_inner(config: &Arc<UpdaterConfig>, state: &Arc<RwLock<ServiceState>>, events: &Sender<UpdaterEvent>, update_id: Uuid) -> Result<()> {
    let metadata = load_metadata(config, update_id).await?;
    let manifest_metadata = parse_apply_manifest_metadata(&metadata);
    info!(%update_id, staged = %metadata_path(config, update_id).display(), "applying staged release");

    // Some platforms can't switch root via bootloader; allow forcing single-slot mode detection.
    let mut single_slot = env::var_os("UPDATER_SINGLE_SLOT").is_some();
    let allow_single_slot_inplace = env_flag_enabled("UPDATER_ALLOW_SINGLE_SLOT_INPLACE");

    {
        let mut guard = state.write().await;
        guard.set_started_at_if_missing(metadata.staged_at);
        guard.set_artifacts(staged_to_url_artifacts(&metadata.artifacts));
        guard.update_progress(UpdateStage::Applying, Some(APPLY_PROGRESS_START), None);
    }
    publish_snapshot(state, events).await;

    ensure_artifacts_present(update_id, &metadata)?;

    let simulate = fake_apply_requested();
    let work_dir = config.work_dir().join(update_id.to_string());

    if !simulate {
        ensure_directory(config.work_dir()).await?;
        ensure_directory(&work_dir).await?;

        let slot_selection = select_target_slot(single_slot)?;
        single_slot = slot_selection.single_slot;

        if single_slot && !allow_single_slot_inplace {
            return Err(Error::InvalidState("single-slot OTA is disabled: inactive RESERVE slot not found; in-place flashing the live root risks filesystem corruption".into()));
        }
        if single_slot {
            warn!(
                %update_id,
                "single-slot OTA override enabled (UPDATER_ALLOW_SINGLE_SLOT_INPLACE); applying update in-place to live root"
            );
        }

        info!(
            %update_id,
            current_slot = %slot_selection.current_slot,
            target_slot = %slot_selection.target_slot,
            target_device = %slot_selection.target_device,
            single_slot,
            scheme = ?slot_selection.scheme,
            "preparing staged image"
        );

        let staged_artifact = metadata.artifacts.first().ok_or_else(|| Error::InvalidState("no staged artifact found".into()))?;
        let staged_path = PathBuf::from(&staged_artifact.local_path);
        let compression = detect_compression_kind(&staged_path).map_err(Error::Io)?;
        let stream_flash_requested = env::var_os("UPDATER_STREAM_FLASH").is_some() && slot_selection.scheme == SlotScheme::Ext4Labels;
        let mut temp_file = false;
        let mut expanded_path: Option<PathBuf> = None;
        let mut streamed = false;

        let mut stream_outcome: Option<StreamFlashOutcome> = None;
        let (progress_tx, progress_task) = start_apply_progress(state.clone(), events.clone());
        let progress_sender = ProgressSender::new(progress_tx.clone());

        let flash_result = if stream_flash_requested && compression.is_some() {
            info!(%update_id, target_slot = %slot_selection.target_slot, target_device = %slot_selection.target_device, "streaming staged image");
            let _ = Command::new("umount").arg(&slot_selection.target_device).status().await;
            let target_bytes = blockdev_size_bytes(&slot_selection.target_device).await?;
            let outcome = flash_compressed_image_to_target(&staged_path, &slot_selection.target_device, target_bytes, Some(progress_sender.clone())).await;
            if let Ok(outcome) = outcome.as_ref() {
                stream_outcome = Some(*outcome);
            }
            streamed = true;
            outcome.map(|_| ())
        } else {
            let expand_result = match decompress_if_needed(&staged_path, &work_dir).await {
                Ok((path, is_temp)) => {
                    expanded_path = Some(path);
                    temp_file = is_temp;
                    Ok(())
                }
                Err(Error::Io(err)) if err.kind() == ErrorKind::StorageFull && compression.is_some() => {
                    if slot_selection.scheme != SlotScheme::Ext4Labels {
                        return Err(Error::InvalidState(format!(
                            "OTA work dir is full and streaming fallback is disabled for {:?} updates; need temporary space to decompress {}",
                            slot_selection.scheme,
                            staged_path.display()
                        )));
                    }
                    warn!(%update_id, target_slot = %slot_selection.target_slot, target_device = %slot_selection.target_device, "work dir full; streaming staged image");
                    if let Err(err) = fs::remove_dir_all(&work_dir).await {
                        warn!(error = %err, path = %work_dir.display(), "failed to clean work dir after decompression failure");
                    }
                    ensure_directory(&work_dir).await?;
                    let _ = Command::new("umount").arg(&slot_selection.target_device).status().await;
                    let target_bytes = blockdev_size_bytes(&slot_selection.target_device).await?;
                    let outcome = flash_compressed_image_to_target(&staged_path, &slot_selection.target_device, target_bytes, Some(progress_sender.clone())).await;
                    if let Ok(outcome) = outcome.as_ref() {
                        stream_outcome = Some(*outcome);
                    }
                    streamed = true;
                    outcome.map(|_| ())
                }
                Err(err) => Err(err),
            };
            if let Ok(()) = expand_result
                && !streamed
            {
                let expanded_path = expanded_path.as_ref().ok_or_else(|| Error::InvalidState("expanded OTA image missing".into()))?;
                info!(%update_id, target_slot = %slot_selection.target_slot, target_device = %slot_selection.target_device, "writing staged image");
                flash_image_to_target(expanded_path, &slot_selection, Some(progress_sender.clone())).await?;
            }
            expand_result
        };

        drop(progress_sender);
        drop(progress_tx);
        let _ = progress_task.await;
        flash_result?;

        if streamed {
            if slot_selection.scheme == SlotScheme::Ext4Labels {
                relabel_target_filesystem(&slot_selection.target_slot, &slot_selection.target_device).await?;
            }
            sync_filesystem(Path::new("/")).await?;
            if let Some(outcome) = stream_outcome
                && !outcome.used_partition
            {
                if slot_selection.scheme == SlotScheme::SquashfsAb {
                    return Err(Error::InvalidState("streamed squashfs OTA image did not expose a root partition; refusing to overwrite the inactive slot with the whole disk image".into()));
                }
                warn!(%update_id, target_slot = %slot_selection.target_slot, target_device = %slot_selection.target_device, "partition table not detected; streamed full image");
            }
            sync_boot_from_target(&slot_selection.target_device, &work_dir).await?;
            if slot_selection.scheme == SlotScheme::Ext4Labels
                && let Err(err) = sync_persisted_state(&slot_selection.target_device, &work_dir).await
            {
                warn!(%err, %update_id, target_device = %slot_selection.target_device, "failed to sync persisted device config to target");
            }
        } else {
            let expanded_path = expanded_path.ok_or_else(|| Error::InvalidState("expanded OTA image missing".into()))?;
            sync_filesystem(Path::new("/")).await?;

            let boot_synced = sync_boot_from_artifact(&expanded_path).await?;
            if !boot_synced {
                sync_boot_from_target(&slot_selection.target_device, &work_dir).await?;
            }
            if slot_selection.scheme == SlotScheme::Ext4Labels
                && let Err(err) = sync_persisted_state(&slot_selection.target_device, &work_dir).await
            {
                warn!(%err, %update_id, target_device = %slot_selection.target_device, "failed to sync persisted device config to target");
            }

            if temp_file {
                let _ = fs::remove_file(expanded_path).await;
            }
        }

        {
            let mut guard = state.write().await;
            guard.update_progress(UpdateStage::Rebooting, Some(92), None);
        }
        publish_snapshot(state, events).await;

        if single_slot {
            // Staying on the same slot: skip tryboot/cmdline juggling and rely on in-place update.
        } else {
            update_boot_markers(config, &slot_selection).await?;
        }
    } else {
        info!(%update_id, "apply running in simulation mode (UPDATER_FAKE_APPLY)");
        {
            let mut guard = state.write().await;
            guard.update_progress(UpdateStage::Rebooting, Some(92), None);
        }
        publish_snapshot(state, events).await;
    }

    if fs::metadata(&work_dir).await.is_ok() {
        fs::remove_dir_all(&work_dir).await?;
    }

    {
        let mut guard = state.write().await;
        guard.update_progress(UpdateStage::Complete, Some(100), None);
    }
    let _ = events.send(UpdaterEvent::ApplyComplete { update_id, reboot_required: true });
    publish_snapshot(state, events).await;

    let usage = cache_usage_bytes(config.cache_dir()).await?;
    {
        let mut guard = state.write().await;
        guard.cache_usage_bytes = usage;
    }

    if !simulate {
        cleanup_source_media_after_apply(config, update_id, &manifest_metadata).await;
    }

    if !simulate {
        let tryboot = !single_slot;
        match reboot_with_args(if tryboot { &["tryboot"] } else { &[] }).await {
            Ok(output) if reboot_output_is_expected_success(&output) => {
                if output.status.success() {
                    info!(%update_id, tryboot, "reboot requested after update apply");
                } else {
                    info!(%update_id, tryboot, status = %output.status, "reboot handoff interrupted by shutdown signal (expected during reboot)");
                }
            }
            Ok(output) => {
                let message = reboot_failure_message(&output);
                warn!(%update_id, tryboot, %message, "reboot request failed");
            }
            Err(err) => {
                warn!(%update_id, tryboot, %err, "reboot request failed");
            }
        }
    }

    Ok(())
}

fn ensure_artifacts_present(update_id: Uuid, metadata: &StagedMetadata) -> Result<()> {
    if metadata.artifacts.is_empty() { Err(crate::error::Error::InvalidState(format!("no artifacts staged for update {update_id}"))) } else { Ok(()) }
}

fn parse_apply_manifest_metadata(metadata: &StagedMetadata) -> ApplyManifestMetadata {
    let raw = metadata.manifest.metadata_json.trim();
    if raw.is_empty() || raw == "{}" {
        return ApplyManifestMetadata::default();
    }
    serde_json::from_str::<ApplyManifestMetadata>(raw).unwrap_or_default()
}

async fn cleanup_source_media_after_apply(config: &UpdaterConfig, update_id: Uuid, metadata: &ApplyManifestMetadata) {
    if !metadata.delete_image_after_apply {
        return;
    }
    let Some(media_path) = metadata.source_media_path.as_deref().map(str::trim).filter(|value| !value.is_empty()).map(PathBuf::from) else {
        return;
    };
    if !media_path.is_absolute() {
        return;
    }
    let Some(filename) = media_path.file_name().and_then(|name| name.to_str()).and_then(sanitize_media_filename) else {
        return;
    };

    match fs::remove_file(&media_path).await {
        Ok(()) => info!(%update_id, path = %media_path.display(), "removed source OTA media file after apply"),
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => warn!(%update_id, path = %media_path.display(), error = %err, "failed to remove source OTA media file"),
    }

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

fn sanitize_media_filename(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Path::new(trimmed).file_name().map(|name| name.to_string_lossy().to_string())
}

pub(crate) async fn flash_image_to_target(expanded_path: &Path, slot_selection: &SlotSelection, progress: Option<ProgressSender>) -> Result<()> {
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
            if let Some(target_bytes) = blockdev_size_bytes(target_device).await?
                && squashfs_size > target_bytes
            {
                let size_mib = squashfs_size / (1024 * 1024);
                let target_mib = target_bytes / (1024 * 1024);
                return Err(Error::InvalidState(format!(
                    "target squashfs slot {} is {} bytes ({} MiB) but image rootfs is {} bytes ({} MiB)",
                    target_device, target_bytes, target_mib, squashfs_size, size_mib
                )));
            }
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

async fn relabel_target_filesystem(target_label: &str, target_device: &str) -> Result<()> {
    // Ensure the flashed filesystem keeps the target slot label.
    // This is required so /dev/disk/by-label stays unambiguous and OTA always writes to RESERVE.
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

async fn update_boot_markers(config: &Arc<UpdaterConfig>, slot_selection: &SlotSelection) -> Result<()> {
    let Some((boot_dir, mounted)) = resolve_boot_dir_rw().await? else {
        return Err(crate::error::Error::InvalidState("cannot mount or find BOOT partition".into()));
    };
    let boot_path = boot_dir.as_path();

    // Prefer an explicit device path to avoid PARTUUID churn and LABEL= parsing
    // issues in early kernel root lookup.
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

async fn publish_snapshot(state: &Arc<RwLock<ServiceState>>, events: &Sender<UpdaterEvent>) {
    let snapshot = {
        let guard = state.read().await;
        let (active, cache_usage) = guard.snapshot();
        UpdaterEvent::StateSnapshot { active_update: active, cache_usage_bytes: cache_usage }
    };
    let _ = events.send(snapshot);
}

fn start_apply_progress(state: Arc<RwLock<ServiceState>>, events: Sender<UpdaterEvent>) -> (mpsc::Sender<ProgressUpdate>, tokio::task::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel(16);
    let handle = tokio::spawn(async move {
        drive_apply_progress(state, events, rx).await;
    });
    (tx, handle)
}

async fn drive_apply_progress(state: Arc<RwLock<ServiceState>>, events: Sender<UpdaterEvent>, mut rx: mpsc::Receiver<ProgressUpdate>) {
    let mut last_percent: Option<u8> = None;
    let mut last_sent = tokio::time::Instant::now().checked_sub(Duration::from_secs(1)).unwrap_or_else(tokio::time::Instant::now);
    while let Some(update) = rx.recv().await {
        let Some(percent) = apply_progress_percent(update.bytes_written, update.total_bytes) else {
            continue;
        };
        if let Some(prev) = last_percent
            && percent <= prev
        {
            continue;
        }
        let now = tokio::time::Instant::now();
        if now.duration_since(last_sent) < Duration::from_millis(300) {
            continue;
        }
        {
            let mut guard = state.write().await;
            guard.update_progress(UpdateStage::Applying, Some(percent), None);
        }
        publish_snapshot(&state, &events).await;
        last_percent = Some(percent);
        last_sent = now;
    }
}

fn apply_progress_percent(bytes_written: u64, total_bytes: Option<u64>) -> Option<u8> {
    let total = total_bytes?;
    if total == 0 {
        return None;
    }
    let ratio = (bytes_written as f64 / total as f64).clamp(0.0, 1.0);
    let range = (APPLY_PROGRESS_END.saturating_sub(APPLY_PROGRESS_START)) as f64;
    let raw = (APPLY_PROGRESS_START as f64) + range * ratio;
    let percent = raw.round().clamp(APPLY_PROGRESS_START as f64, APPLY_PROGRESS_END as f64) as u8;
    Some(percent)
}

fn metadata_path(config: &UpdaterConfig, update_id: Uuid) -> std::path::PathBuf {
    config.cache_dir().join(update_id.to_string()).join("metadata.json")
}

fn reboot_output_is_expected_success(output: &std::process::Output) -> bool {
    if output.status.success() {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if output.status.signal() == Some(15) {
            return true;
        }
    }
    false
}

fn reboot_failure_message(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("reboot exited with {}", output.status)
    }
}

async fn reboot_with_args(args: &[&str]) -> std::io::Result<std::process::Output> {
    if Path::new("/run/systemd/system").exists() {
        let mut cmd = Command::new("systemctl");
        cmd.arg("reboot").arg("--no-block");
        if !args.is_empty() {
            cmd.arg(format!("--reboot-argument={}", args.join(" ")));
        }
        match cmd.output().await {
            Ok(output) => return Ok(output),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }
    for cmd in ["/sbin/reboot", "/usr/sbin/reboot", "reboot"] {
        match Command::new(cmd).args(args).output().await {
            Ok(output) => return Ok(output),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        }
    }
    Command::new("systemctl").args(["reboot", "--no-block"]).output().await
}

async fn sync_boot_from_artifact(artifact_path: &Path) -> Result<bool> {
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

async fn sync_boot_from_target(target_device: &str, work_dir: &Path) -> Result<()> {
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

async fn sync_persisted_state(target_device: &str, work_dir: &Path) -> Result<()> {
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

async fn sync_persisted_networkd_into(root: &Path, src_dir: &Path) -> Result<()> {
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

async fn sync_persisted_files_with_mappings_into(root: &Path, mappings: &[PersistedFileSync]) -> Result<()> {
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
    // Minimal compatibility metadata so Pi 5 bootloader doesn't reject the image.
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

#[cfg(test)]
mod tests {
    use super::{PersistedFileSync, parse_env_flag, reboot_failure_message, reboot_output_is_expected_success, sync_persisted_files_with_mappings_into, sync_persisted_networkd_into};
    use std::path::Path;

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
}
