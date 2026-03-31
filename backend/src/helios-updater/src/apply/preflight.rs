use std::env;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use tokio::fs;
use tracing::warn;
use uuid::Uuid;

use super::{ApplyManifestMetadata, SquashfsPreflightContext, SquashfsSlotResizePlan, env_flag_enabled};
use crate::artifact::{StagedMetadata, load_metadata};
use crate::bundle::{is_frontend_bundle, is_service_bundle};
use crate::config::UpdaterConfig;
use crate::error::{Error, Result};
use crate::ipc::{PreflightReport, PreflightVerdict};
use crate::util::{
    SlotScheme, available_bytes_for_path, blockdev_size_bytes, decompress_if_needed, detect_ext4_partition_in_disk_image, detect_squashfs_partition_in_disk_image, ensure_directory,
    inspect_adjacent_partition, inspect_block_partition, select_target_slot,
};

pub(super) fn parse_apply_manifest_metadata(metadata: &StagedMetadata) -> ApplyManifestMetadata {
    let raw = metadata.manifest.metadata_json.trim();
    if raw.is_empty() || raw == "{}" {
        return ApplyManifestMetadata::default();
    }
    serde_json::from_str::<ApplyManifestMetadata>(raw).unwrap_or_default()
}

pub(crate) async fn preflight_staged_release(config: &UpdaterConfig, update_id: Uuid) -> Result<PreflightReport> {
    let metadata = load_metadata(config, update_id).await?;
    let artifact_kind = metadata.manifest.artifacts.first().and_then(|artifact| artifact.kind.as_deref());

    if is_frontend_bundle(artifact_kind) {
        return Ok(ready_preflight_report(update_id, artifact_kind, None, false, "frontend bundle is ready to apply".into()));
    }
    if is_service_bundle(artifact_kind) {
        return Ok(ready_preflight_report(update_id, artifact_kind, None, false, "service bundle is ready to apply".into()));
    }

    let Some(staged_artifact) = metadata.artifacts.first() else {
        return Ok(simple_preflight_report(update_id, artifact_kind, None, false, PreflightVerdict::InvalidArtifact, "staged update has no artifacts".into()));
    };

    let staged_path = PathBuf::from(&staged_artifact.local_path);
    if fs::metadata(&staged_path).await.is_err() {
        return Ok(simple_preflight_report(update_id, artifact_kind, None, false, PreflightVerdict::InvalidArtifact, format!("staged artifact {} is missing", staged_path.display())));
    }

    let single_slot_requested = env::var_os("UPDATER_SINGLE_SLOT").is_some();
    let allow_single_slot_inplace = env_flag_enabled("UPDATER_ALLOW_SINGLE_SLOT_INPLACE");
    let slot_selection = select_target_slot(single_slot_requested)?;
    let work_dir_available_bytes = available_bytes_for_path(config.work_dir()).await?.unwrap_or(0);

    if slot_selection.single_slot && !allow_single_slot_inplace {
        return Ok(PreflightReport {
            update_id,
            ready: false,
            verdict: PreflightVerdict::SingleSlotDisabled,
            summary: "single-slot OTA is disabled: inactive reserve slot not found; in-place flashing the live root is blocked".into(),
            artifact_kind: artifact_kind.map(str::to_string),
            target_device: Some(slot_selection.target_device.clone()),
            image_size_bytes: None,
            target_size_bytes: None,
            gap_after_bytes: None,
            additional_from_data_bytes: None,
            data_dir_available_bytes: None,
            work_dir_available_bytes: Some(work_dir_available_bytes),
            clear_bytes: None,
            single_slot: true,
        });
    }

    let preflight_dir = config.work_dir().join(format!("preflight-{update_id}"));
    ensure_directory(&preflight_dir).await?;
    let report = match decompress_if_needed(&staged_path, &preflight_dir).await {
        Ok((expanded_path, temp_file)) => {
            let report = preflight_disk_image_release(config, update_id, artifact_kind, &slot_selection, &expanded_path, work_dir_available_bytes).await?;

            if temp_file
                && let Err(err) = fs::remove_file(&expanded_path).await
                && err.kind() != ErrorKind::NotFound
            {
                warn!(error = %err, path = %expanded_path.display(), "failed to remove preflight expanded image");
            }

            report
        }
        Err(Error::Io(err)) if err.kind() == ErrorKind::StorageFull => PreflightReport {
            update_id,
            ready: false,
            verdict: PreflightVerdict::WorkDirFull,
            summary: format!("OTA work dir {} does not have enough free space to unpack {}; free bytes: {}", config.work_dir().display(), staged_path.display(), work_dir_available_bytes),
            artifact_kind: artifact_kind.map(str::to_string),
            target_device: Some(slot_selection.target_device.clone()),
            image_size_bytes: None,
            target_size_bytes: None,
            gap_after_bytes: None,
            additional_from_data_bytes: None,
            data_dir_available_bytes: None,
            work_dir_available_bytes: Some(work_dir_available_bytes),
            clear_bytes: None,
            single_slot: slot_selection.single_slot,
        },
        Err(err) => {
            if fs::metadata(&preflight_dir).await.is_ok()
                && let Err(cleanup_err) = fs::remove_dir_all(&preflight_dir).await
            {
                warn!(error = %cleanup_err, path = %preflight_dir.display(), "failed to remove OTA preflight dir after error");
            }
            return Err(err);
        }
    };

    if fs::metadata(&preflight_dir).await.is_ok()
        && let Err(err) = fs::remove_dir_all(&preflight_dir).await
    {
        warn!(error = %err, path = %preflight_dir.display(), "failed to remove OTA preflight dir");
    }

    Ok(report)
}

async fn preflight_disk_image_release(
    config: &UpdaterConfig,
    update_id: Uuid,
    artifact_kind: Option<&str>,
    slot_selection: &crate::util::SlotSelection,
    expanded_path: &Path,
    work_dir_available_bytes: u64,
) -> Result<PreflightReport> {
    match slot_selection.scheme {
        SlotScheme::Ext4Labels => {
            let Some((_, image_size_bytes)) = detect_ext4_partition_in_disk_image(expanded_path).await? else {
                return Ok(simple_preflight_report(
                    update_id,
                    artifact_kind,
                    Some(slot_selection.target_device.clone()),
                    slot_selection.single_slot,
                    PreflightVerdict::InvalidArtifact,
                    format!("OTA artifact {} does not contain an ext4 rootfs partition", expanded_path.display()),
                ));
            };
            let Some(target_size_bytes) = blockdev_size_bytes(&slot_selection.target_device).await? else {
                return Err(Error::InvalidState(format!("unable to inspect target partition {}", slot_selection.target_device)));
            };
            if image_size_bytes > target_size_bytes {
                return Ok(PreflightReport {
                    update_id,
                    ready: false,
                    verdict: PreflightVerdict::TargetTooSmall,
                    summary: format!("target partition {} is {} bytes but image rootfs needs {} bytes", slot_selection.target_device, target_size_bytes, image_size_bytes),
                    artifact_kind: artifact_kind.map(str::to_string),
                    target_device: Some(slot_selection.target_device.clone()),
                    image_size_bytes: Some(image_size_bytes),
                    target_size_bytes: Some(target_size_bytes),
                    gap_after_bytes: None,
                    additional_from_data_bytes: None,
                    data_dir_available_bytes: None,
                    work_dir_available_bytes: Some(work_dir_available_bytes),
                    clear_bytes: None,
                    single_slot: slot_selection.single_slot,
                });
            }
            Ok(PreflightReport {
                update_id,
                ready: true,
                verdict: PreflightVerdict::Ready,
                summary: format!("target partition {} has enough capacity for the staged ext4 image", slot_selection.target_device),
                artifact_kind: artifact_kind.map(str::to_string),
                target_device: Some(slot_selection.target_device.clone()),
                image_size_bytes: Some(image_size_bytes),
                target_size_bytes: Some(target_size_bytes),
                gap_after_bytes: None,
                additional_from_data_bytes: None,
                data_dir_available_bytes: None,
                work_dir_available_bytes: Some(work_dir_available_bytes),
                clear_bytes: None,
                single_slot: slot_selection.single_slot,
            })
        }
        SlotScheme::SquashfsAb => {
            let Some((_, image_size_bytes)) = detect_squashfs_partition_in_disk_image(expanded_path).await? else {
                return Ok(simple_preflight_report(
                    update_id,
                    artifact_kind,
                    Some(slot_selection.target_device.clone()),
                    slot_selection.single_slot,
                    PreflightVerdict::InvalidArtifact,
                    format!("OTA artifact {} does not contain a squashfs rootfs partition", expanded_path.display()),
                ));
            };
            let Some(target_info) = inspect_block_partition(&slot_selection.target_device).await? else {
                return Err(Error::InvalidState(format!("unable to inspect squashfs target slot {}", slot_selection.target_device)));
            };
            let next_partition = inspect_adjacent_partition(&slot_selection.target_device, 1).await?;
            let data_dir_available_bytes = available_bytes_for_path(config.data_dir()).await?.unwrap_or(0);
            let gap_after_bytes = next_partition.as_ref().map(|next| next.start_bytes().saturating_sub(target_info.end_bytes_exclusive())).unwrap_or(0);
            let ctx =
                SquashfsPreflightContext { update_id, artifact_kind, slot_selection, target_info: &target_info, image_size_bytes, data_dir_available_bytes, work_dir_available_bytes, gap_after_bytes };
            Ok(preflight_report_for_squashfs_plan(ctx, super::sync::plan_squashfs_slot_resize(&target_info, next_partition.as_ref(), image_size_bytes, data_dir_available_bytes)))
        }
    }
}

fn ready_preflight_report(update_id: Uuid, artifact_kind: Option<&str>, target_device: Option<String>, single_slot: bool, summary: String) -> PreflightReport {
    PreflightReport {
        update_id,
        ready: true,
        verdict: PreflightVerdict::Ready,
        summary,
        artifact_kind: artifact_kind.map(str::to_string),
        target_device,
        image_size_bytes: None,
        target_size_bytes: None,
        gap_after_bytes: None,
        additional_from_data_bytes: None,
        data_dir_available_bytes: None,
        work_dir_available_bytes: None,
        clear_bytes: None,
        single_slot,
    }
}

fn simple_preflight_report(update_id: Uuid, artifact_kind: Option<&str>, target_device: Option<String>, single_slot: bool, verdict: PreflightVerdict, summary: String) -> PreflightReport {
    PreflightReport {
        update_id,
        ready: matches!(verdict, PreflightVerdict::Ready | PreflightVerdict::GrowIntoGap),
        verdict,
        summary,
        artifact_kind: artifact_kind.map(str::to_string),
        target_device,
        image_size_bytes: None,
        target_size_bytes: None,
        gap_after_bytes: None,
        additional_from_data_bytes: None,
        data_dir_available_bytes: None,
        work_dir_available_bytes: None,
        clear_bytes: None,
        single_slot,
    }
}

pub(super) fn preflight_report_for_squashfs_plan(ctx: SquashfsPreflightContext<'_>, plan: SquashfsSlotResizePlan) -> PreflightReport {
    let base = PreflightReport {
        update_id: ctx.update_id,
        ready: false,
        verdict: PreflightVerdict::InvalidArtifact,
        summary: String::new(),
        artifact_kind: ctx.artifact_kind.map(str::to_string),
        target_device: Some(ctx.slot_selection.target_device.clone()),
        image_size_bytes: Some(ctx.image_size_bytes),
        target_size_bytes: Some(ctx.target_info.size_bytes()),
        gap_after_bytes: Some(ctx.gap_after_bytes),
        additional_from_data_bytes: None,
        data_dir_available_bytes: Some(ctx.data_dir_available_bytes),
        work_dir_available_bytes: Some(ctx.work_dir_available_bytes),
        clear_bytes: None,
        single_slot: ctx.slot_selection.single_slot,
    };

    match plan {
        SquashfsSlotResizePlan::Fits => PreflightReport {
            ready: true,
            verdict: PreflightVerdict::Ready,
            summary: format!("inactive squashfs slot {} already has enough capacity for {} bytes", ctx.target_info.device, ctx.image_size_bytes),
            ..base
        },
        SquashfsSlotResizePlan::GrowIntoGap { required_growth_bytes, gap_after_bytes, .. } => PreflightReport {
            ready: true,
            verdict: PreflightVerdict::GrowIntoGap,
            summary: format!(
                "inactive squashfs slot {} needs {} more bytes and updater can grow it into the {} byte post-slot gap before apply",
                ctx.target_info.device, required_growth_bytes, gap_after_bytes
            ),
            gap_after_bytes: Some(gap_after_bytes),
            ..base
        },
        SquashfsSlotResizePlan::NeedsDataResize { required_growth_bytes, gap_after_bytes, additional_from_data_bytes, data_dir_available_bytes } => PreflightReport {
            ready: false,
            verdict: PreflightVerdict::NeedsDataResize,
            summary: format!(
                "inactive squashfs slot {} needs {} more bytes, can reclaim {} bytes of post-slot gap, but would still need {} bytes taken from DATA; {} currently has {} free bytes",
                ctx.target_info.device, required_growth_bytes, gap_after_bytes, additional_from_data_bytes, "/var/lib/helios", data_dir_available_bytes
            ),
            gap_after_bytes: Some(gap_after_bytes),
            additional_from_data_bytes: Some(additional_from_data_bytes),
            data_dir_available_bytes: Some(data_dir_available_bytes),
            ..base
        },
        SquashfsSlotResizePlan::ClearDataDir { required_growth_bytes, gap_after_bytes, additional_from_data_bytes, data_dir_available_bytes, clear_bytes } => PreflightReport {
            ready: false,
            verdict: PreflightVerdict::ClearDataDir,
            summary: format!(
                "inactive squashfs slot {} needs {} more bytes, can reclaim {} bytes of post-slot gap, but DATA only has {} free bytes; clear at least {} bytes so {} more bytes can be taken from DATA",
                ctx.target_info.device, required_growth_bytes, gap_after_bytes, data_dir_available_bytes, clear_bytes, additional_from_data_bytes
            ),
            gap_after_bytes: Some(gap_after_bytes),
            additional_from_data_bytes: Some(additional_from_data_bytes),
            data_dir_available_bytes: Some(data_dir_available_bytes),
            clear_bytes: Some(clear_bytes),
            ..base
        },
    }
}
