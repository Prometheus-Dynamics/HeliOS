use std::env;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::ipc::{UpdateStage, UpdaterEvent};
use serde::Deserialize;
use tokio::fs;
use tokio::process::Command;
use tokio::sync::{RwLock, broadcast::Sender};
use tokio::task::JoinHandle;
use tracing::{info, warn};
use uuid::Uuid;

use crate::artifact::{StagedMetadata, cache_usage_bytes, load_metadata, staged_to_url_artifacts};
use crate::bundle::{BundleApplyOutcome, apply_frontend_bundle, apply_service_bundle, is_frontend_bundle, is_service_bundle, trigger_updater_restart_later};
use crate::config::UpdaterConfig;
use crate::error::{Error, Result};
use crate::state::ServiceState;
use crate::util::{
    BlockPartitionInfo, ProgressSender, SlotScheme, SlotSelection, StreamFlashOutcome, blockdev_size_bytes, detect_compression_kind, decompress_if_needed, ensure_directory,
    flash_compressed_image_to_target, select_target_slot, sync_filesystem,
};

mod preflight;
mod progress;
mod sync;

use preflight::parse_apply_manifest_metadata;
use progress::{publish_snapshot, start_apply_progress};
use sync::{
    cleanup_source_media_after_apply, clear_completed_update_state, flash_image_to_target, relabel_target_filesystem, sync_boot_from_artifact, sync_boot_from_target, sync_persisted_state, update_boot_markers,
    validate_bootable_squashfs_root,
};
pub(crate) use preflight::preflight_staged_release;
pub(crate) use sync::purge_update_dirs;

#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(test)]
static TEST_FAKE_APPLY: AtomicUsize = AtomicUsize::new(0);

const APPLY_PROGRESS_START: u8 = 10;
const APPLY_PROGRESS_END: u8 = 85;
const PERSIST_NETWORKD_DIR: &str = "/var/lib/helios/networkd";
const PERSIST_NETWORKD_PREFIX: &str = "00-helios-persisted-";
const REQUIRED_BOOTABLE_ROOT_PATHS: &[&str] = &["/sbin/init", "/bin/sh", "/lib", "/lib64", "/usr/lib/systemd/systemd", "/etc/os-release"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SquashfsSlotResizePlan {
    Fits,
    GrowIntoGap { required_growth_bytes: u64, gap_after_bytes: u64, new_end_bytes_exclusive: u64 },
    NeedsDataResize { required_growth_bytes: u64, gap_after_bytes: u64, additional_from_data_bytes: u64, data_dir_available_bytes: u64 },
    ClearDataDir { required_growth_bytes: u64, gap_after_bytes: u64, additional_from_data_bytes: u64, data_dir_available_bytes: u64, clear_bytes: u64 },
}

#[derive(Debug, Clone, Copy)]
struct SquashfsPreflightContext<'a> {
    update_id: Uuid,
    artifact_kind: Option<&'a str>,
    slot_selection: &'a SlotSelection,
    target_info: &'a BlockPartitionInfo,
    image_size_bytes: u64,
    data_dir_available_bytes: u64,
    work_dir_available_bytes: u64,
    gap_after_bytes: u64,
}

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

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct ApplyManifestMetadata {
    #[serde(default = "default_true")]
    delete_image_after_apply: bool,
    #[serde(default, alias = "source_media_path")]
    source_artifact_path: Option<String>,
}

impl Default for ApplyManifestMetadata {
    fn default() -> Self {
        Self { delete_image_after_apply: true, source_artifact_path: None }
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
    let artifact_kind = metadata.manifest.artifacts.first().and_then(|artifact| artifact.kind.as_deref());
    info!(%update_id, staged = %metadata_path(config, update_id).display(), "applying staged release");

    // Some platforms can't switch root via bootloader; allow forcing single-slot mode detection.
    let single_slot_requested = env::var_os("UPDATER_SINGLE_SLOT").is_some();
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
    let apply_outcome = if is_frontend_bundle(artifact_kind) {
        if simulate {
            info!(%update_id, "frontend bundle apply running in simulation mode (UPDATER_FAKE_APPLY)");
            BundleApplyOutcome::frontend_bundle()
        } else {
            let staged_artifact = metadata.artifacts.first().ok_or_else(|| Error::InvalidState("no staged artifact found".into()))?;
            let staged_path = PathBuf::from(&staged_artifact.local_path);
            {
                let mut guard = state.write().await;
                guard.update_progress(UpdateStage::Applying, Some(65), None);
            }
            publish_snapshot(state, events).await;
            apply_frontend_bundle(config, update_id, &staged_path).await?
        }
    } else if is_service_bundle(artifact_kind) {
        if simulate {
            info!(%update_id, "service bundle apply running in simulation mode (UPDATER_FAKE_APPLY)");
            BundleApplyOutcome::service_bundle(false)
        } else {
            let staged_artifact = metadata.artifacts.first().ok_or_else(|| Error::InvalidState("no staged artifact found".into()))?;
            let staged_path = PathBuf::from(&staged_artifact.local_path);
            {
                let mut guard = state.write().await;
                guard.update_progress(UpdateStage::Applying, Some(65), None);
            }
            publish_snapshot(state, events).await;
            apply_service_bundle(config, update_id, &staged_path).await?
        }
    } else {
        apply_disk_image_release(config, state, events, update_id, &metadata, simulate, &work_dir, single_slot_requested, allow_single_slot_inplace).await?
    };

    if fs::metadata(&work_dir).await.is_ok() {
        fs::remove_dir_all(&work_dir).await?;
    }

    {
        let mut guard = state.write().await;
        guard.update_progress(UpdateStage::Complete, Some(100), None);
    }
    let _ = events.send(UpdaterEvent::ApplyComplete { update_id, reboot_required: apply_outcome.reboot_required });
    publish_snapshot(state, events).await;

    let usage = cache_usage_bytes(config.cache_dir()).await?;
    {
        let mut guard = state.write().await;
        guard.cache_usage_bytes = usage;
    }

    if !simulate {
        cleanup_source_media_after_apply(config, update_id, &manifest_metadata).await;
    }
    if !simulate && !apply_outcome.reboot_required {
        clear_completed_update_state(config, state, events, update_id).await?;
    }

    if !simulate
        && apply_outcome.restart_updater_after_apply
        && let Err(err) = trigger_updater_restart_later(config)
    {
        warn!(%update_id, %err, "failed to schedule updater restart after service bundle apply");
    }

    if !simulate && apply_outcome.reboot_required {
        let tryboot = apply_outcome.tryboot;
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

async fn apply_disk_image_release(
    config: &Arc<UpdaterConfig>,
    state: &Arc<RwLock<ServiceState>>,
    events: &Sender<UpdaterEvent>,
    update_id: Uuid,
    metadata: &StagedMetadata,
    simulate: bool,
    work_dir: &Path,
    mut single_slot: bool,
    allow_single_slot_inplace: bool,
) -> Result<BundleApplyOutcome> {
    if !simulate {
        ensure_directory(config.work_dir()).await?;
        ensure_directory(work_dir).await?;

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
            let expand_result = match decompress_if_needed(&staged_path, work_dir).await {
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
                    if let Err(err) = fs::remove_dir_all(work_dir).await {
                        warn!(error = %err, path = %work_dir.display(), "failed to clean work dir after decompression failure");
                    }
                    ensure_directory(work_dir).await?;
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
                flash_image_to_target(expanded_path, &slot_selection, config.data_dir(), Some(progress_sender.clone())).await?;
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
            sync_boot_from_target(&slot_selection.target_device, work_dir).await?;
            if slot_selection.scheme == SlotScheme::Ext4Labels
                && let Err(err) = sync_persisted_state(&slot_selection.target_device, work_dir).await
            {
                warn!(%err, %update_id, target_device = %slot_selection.target_device, "failed to sync persisted device config to target");
            }
        } else {
            let expanded_path = expanded_path.ok_or_else(|| Error::InvalidState("expanded OTA image missing".into()))?;
            sync_filesystem(Path::new("/")).await?;

            let boot_synced = sync_boot_from_artifact(&expanded_path).await?;
            if !boot_synced {
                sync_boot_from_target(&slot_selection.target_device, work_dir).await?;
            }
            if slot_selection.scheme == SlotScheme::Ext4Labels
                && let Err(err) = sync_persisted_state(&slot_selection.target_device, work_dir).await
            {
                warn!(%err, %update_id, target_device = %slot_selection.target_device, "failed to sync persisted device config to target");
            }

            if temp_file {
                let _ = fs::remove_file(expanded_path).await;
            }
        }

        if slot_selection.scheme == SlotScheme::SquashfsAb {
            validate_bootable_squashfs_root(&slot_selection.target_device, work_dir).await?;
        }

        {
            let mut guard = state.write().await;
            guard.update_progress(UpdateStage::Rebooting, Some(92), None);
        }
        publish_snapshot(state, events).await;

        if !single_slot {
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

    Ok(BundleApplyOutcome::disk_image(!single_slot))
}

fn ensure_artifacts_present(update_id: Uuid, metadata: &StagedMetadata) -> Result<()> {
    if metadata.artifacts.is_empty() { Err(crate::error::Error::InvalidState(format!("no artifacts staged for update {update_id}"))) } else { Ok(()) }
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

#[cfg(test)]
mod tests;
