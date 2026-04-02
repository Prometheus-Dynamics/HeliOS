use std::path::{Path, PathBuf};

use lib_storage_layout::{LayoutPartition, PartitionMode, PartitionRole, StorageLayoutManifest};
use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::process::Command;
use tracing::warn;
use uuid::Uuid;

use crate::artifact::ReleaseManifest;
use crate::config::UpdaterConfig;
use crate::error::{Error, Result};
use crate::util::{BlockPartitionInfo, SlotSelection, resolve_boot_dir_rw, sync_filesystem};

const MIB: u64 = 1024 * 1024;
const REPARTITION_REQUEST_VERSION: u32 = 1;
const OTA_DIR_COMPONENTS: &[&str] = &["helios", "ota"];
const REQUEST_ENV_NAME: &str = "repartition-request.env";
const RESULT_ENV_NAME: &str = "repartition-result.env";
const RESUME_JSON_NAME: &str = "repartition-resume.json";
const LAYOUT_TOML_NAME: &str = "repartition-layout.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OfflineDataBorrowPlan {
    pub target_role: PartitionRole,
    pub target_start_mib: u64,
    pub target_end_mib: u64,
    pub gap_after_bytes: u64,
    pub required_growth_bytes: u64,
    pub additional_from_data_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum OfflineDataBorrowBlocker {
    TargetNotAdjacentToData,
    NonReplayableArtifact(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum OfflineDataBorrowAssessment {
    Disabled,
    Supported(OfflineDataBorrowPlan),
    Blocked(OfflineDataBorrowBlocker),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct QueuedRepartitionResume {
    pub update_id: Uuid,
    pub manifest: ReleaseManifest,
}

pub(super) fn assess_offline_data_borrow(
    config: &UpdaterConfig,
    layout: &StorageLayoutManifest,
    manifest: &ReleaseManifest,
    target_info: &BlockPartitionInfo,
    next_partition: Option<&BlockPartitionInfo>,
    required_growth_bytes: u64,
    gap_after_bytes: u64,
    additional_from_data_bytes: u64,
) -> OfflineDataBorrowAssessment {
    if !layout.allows_destructive_data_borrow() {
        return OfflineDataBorrowAssessment::Disabled;
    }

    let target_role = match layout.partition_by_role(PartitionRole::SlotA).ok().filter(|partition| partition.number == target_info.number).map(|_| PartitionRole::SlotA).or_else(|| {
        layout.partition_by_role(PartitionRole::SlotB).ok().filter(|partition| partition.number == target_info.number).map(|_| PartitionRole::SlotB)
    }) {
        Some(role) => role,
        None => return OfflineDataBorrowAssessment::Blocked(OfflineDataBorrowBlocker::TargetNotAdjacentToData),
    };

    let data_partition_number = match layout.data() {
        Ok(data) => data.number,
        Err(_) => return OfflineDataBorrowAssessment::Blocked(OfflineDataBorrowBlocker::TargetNotAdjacentToData),
    };
    if next_partition.map(|partition| partition.number) != Some(data_partition_number) {
        return OfflineDataBorrowAssessment::Blocked(OfflineDataBorrowBlocker::TargetNotAdjacentToData);
    }

    if let Err(reason) = manifest_is_replayable_after_data_repartition(config, manifest) {
        return OfflineDataBorrowAssessment::Blocked(OfflineDataBorrowBlocker::NonReplayableArtifact(reason));
    }

    let align_mib = layout.defaults.align_mib.unwrap_or(4);
    let required_capacity_bytes = align_up_bytes(target_info.size_bytes().saturating_add(required_growth_bytes), target_info.sector_bytes);
    let target_start_mib = bytes_to_mib_floor(target_info.start_bytes());
    let target_end_mib = align_up_mib(bytes_to_mib_ceil(target_info.start_bytes().saturating_add(required_capacity_bytes)), align_mib);

    OfflineDataBorrowAssessment::Supported(OfflineDataBorrowPlan {
        target_role,
        target_start_mib,
        target_end_mib,
        gap_after_bytes,
        required_growth_bytes,
        additional_from_data_bytes,
    })
}

pub(super) fn offline_data_borrow_summary(target_device: &str, plan: &OfflineDataBorrowPlan) -> String {
    format!(
        "inactive squashfs slot {} needs {} more bytes, can reclaim {} bytes of post-slot gap, and policy allows taking the remaining {} bytes from DATA by rebooting into a tmpfs writable store, recreating DATA, redownloading the release, and then resuming apply",
        target_device, plan.required_growth_bytes, plan.gap_after_bytes, plan.additional_from_data_bytes
    )
}

pub(super) fn offline_data_borrow_blocked_summary(target_device: &str, blocker: &OfflineDataBorrowBlocker) -> String {
    match blocker {
        OfflineDataBorrowBlocker::TargetNotAdjacentToData => format!(
            "inactive squashfs slot {} is not the DATA-adjacent slot, so updater cannot safely borrow DATA space without moving the currently booted root slot",
            target_device
        ),
        OfflineDataBorrowBlocker::NonReplayableArtifact(reason) => format!(
            "inactive squashfs slot {} would need destructive DATA repartitioning, but updater cannot replay the staged artifact after DATA is recreated: {}",
            target_device, reason
        ),
    }
}

pub(super) async fn queue_offline_data_borrow_repartition(
    _config: &UpdaterConfig,
    update_id: Uuid,
    manifest: &ReleaseManifest,
    layout: &StorageLayoutManifest,
    slot_selection: &SlotSelection,
    plan: &OfflineDataBorrowPlan,
) -> Result<()> {
    let Some((boot_dir, mounted)) = resolve_boot_dir_rw().await? else {
        return Err(Error::InvalidState("cannot mount or find BOOT partition for queued repartition request".into()));
    };

    let result = queue_offline_data_borrow_repartition_into(&boot_dir, update_id, manifest, layout, slot_selection, plan).await;
    if let Err(err) = maybe_unmount_boot_dir(&boot_dir, mounted).await {
        warn!(error = %err, path = %boot_dir.display(), "failed to unmount BOOT after writing repartition request");
    }
    result
}

pub(crate) async fn load_queued_repartition_resume(config: &UpdaterConfig) -> Result<Option<QueuedRepartitionResume>> {
    let _ = config;
    let Some((boot_dir, mounted)) = resolve_boot_dir_rw().await? else {
        return Ok(None);
    };

    let result = load_queued_repartition_resume_from_dir(&boot_dir).await;
    if let Err(err) = maybe_unmount_boot_dir(&boot_dir, mounted).await {
        warn!(error = %err, path = %boot_dir.display(), "failed to unmount BOOT after reading repartition resume marker");
    }
    result
}

pub(crate) async fn clear_queued_repartition_resume(config: &UpdaterConfig) -> Result<()> {
    let _ = config;
    let Some((boot_dir, mounted)) = resolve_boot_dir_rw().await? else {
        return Ok(());
    };

    let result = clear_queued_repartition_resume_from_dir(&boot_dir).await;
    if let Err(err) = maybe_unmount_boot_dir(&boot_dir, mounted).await {
        warn!(error = %err, path = %boot_dir.display(), "failed to unmount BOOT after clearing repartition resume marker");
    }
    result
}

async fn queue_offline_data_borrow_repartition_into(
    boot_dir: &Path,
    update_id: Uuid,
    manifest: &ReleaseManifest,
    layout: &StorageLayoutManifest,
    slot_selection: &SlotSelection,
    plan: &OfflineDataBorrowPlan,
) -> Result<()> {
    let ota_dir = ota_dir_for_boot(boot_dir);
    fs::create_dir_all(&ota_dir).await.map_err(Error::Io)?;

    let repartition_layout = build_offline_data_borrow_layout(layout, plan)?;
    let layout_body = toml::to_string_pretty(&repartition_layout).map_err(|err| Error::InvalidState(format!("failed to serialize queued repartition layout: {err}")))?;
    let resume_body = serde_json::to_vec_pretty(&QueuedRepartitionResume { update_id, manifest: force_auto_apply(manifest) })?;
    let request_body = render_request_env(update_id, &slot_selection.target_slot);

    let layout_tmp = ota_dir.join(format!("{LAYOUT_TOML_NAME}.tmp"));
    let resume_tmp = ota_dir.join(format!("{RESUME_JSON_NAME}.tmp"));
    let request_tmp = ota_dir.join(format!("{REQUEST_ENV_NAME}.tmp"));
    let result_path = ota_dir.join(RESULT_ENV_NAME);

    remove_if_exists(&result_path).await?;
    fs::write(&layout_tmp, layout_body).await.map_err(Error::Io)?;
    fs::write(&resume_tmp, resume_body).await.map_err(Error::Io)?;
    fs::write(&request_tmp, request_body).await.map_err(Error::Io)?;

    fs::rename(&layout_tmp, ota_dir.join(LAYOUT_TOML_NAME)).await.map_err(Error::Io)?;
    fs::rename(&resume_tmp, ota_dir.join(RESUME_JSON_NAME)).await.map_err(Error::Io)?;
    fs::rename(&request_tmp, ota_dir.join(REQUEST_ENV_NAME)).await.map_err(Error::Io)?;
    sync_filesystem(boot_dir).await?;
    Ok(())
}

async fn load_queued_repartition_resume_from_dir(boot_dir: &Path) -> Result<Option<QueuedRepartitionResume>> {
    let ota_dir = ota_dir_for_boot(boot_dir);
    let result_raw = match fs::read_to_string(ota_dir.join(RESULT_ENV_NAME)).await {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(Error::Io(err)),
    };

    let values = parse_env_map(&result_raw);
    let Some(status) = values.get("HELIOS_REPARTITION_STATUS").map(String::as_str) else {
        return Ok(None);
    };
    if !matches!(status, "applied" | "recovered" | "unchanged") {
        return Ok(None);
    }

    let resume_raw = match fs::read(ota_dir.join(RESUME_JSON_NAME)).await {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(Error::Io(err)),
    };
    let resume: QueuedRepartitionResume = serde_json::from_slice(&resume_raw)?;
    if let Some(update_id) = values.get("HELIOS_REPARTITION_UPDATE_ID")
        && update_id != &resume.update_id.to_string()
    {
        return Err(Error::InvalidState(format!(
            "repartition result/update mismatch: result update_id={} resume update_id={}",
            update_id, resume.update_id
        )));
    }

    Ok(Some(resume))
}

async fn clear_queued_repartition_resume_from_dir(boot_dir: &Path) -> Result<()> {
    let ota_dir = ota_dir_for_boot(boot_dir);
    for name in [REQUEST_ENV_NAME, RESULT_ENV_NAME, RESUME_JSON_NAME, LAYOUT_TOML_NAME] {
        remove_if_exists(&ota_dir.join(name)).await?;
    }
    sync_filesystem(boot_dir).await?;
    Ok(())
}

fn build_offline_data_borrow_layout(layout: &StorageLayoutManifest, plan: &OfflineDataBorrowPlan) -> Result<StorageLayoutManifest> {
    let mut generated = layout.clone();
    for partition in &mut generated.partitions {
        match partition.role {
            PartitionRole::SlotA | PartitionRole::SlotB if partition.role == plan.target_role => {
                configure_target_partition(partition, plan);
            }
            PartitionRole::SlotA | PartitionRole::SlotB => {
                configure_live_slot_passthrough(partition);
            }
            PartitionRole::Data => {
                configure_recreated_data_partition(partition, plan.target_end_mib);
            }
        }
    }
    Ok(generated)
}

fn configure_target_partition(partition: &mut LayoutPartition, plan: &OfflineDataBorrowPlan) {
    partition.mode = PartitionMode::Mkpart;
    partition.start_mib = Some(plan.target_start_mib);
    partition.end_mib = Some(plan.target_end_mib);
    partition.size_mib = None;
    partition.size_source = None;
    partition.target_percent = None;
    partition.max_mib = None;
    partition.fill_to_end = Some(false);
    partition.gap_after_mib = Some(0);
}

fn configure_live_slot_passthrough(partition: &mut LayoutPartition) {
    partition.mode = PartitionMode::Noop;
    partition.start_mib = None;
    partition.end_mib = None;
    partition.size_mib = None;
    partition.size_source = None;
    partition.target_percent = None;
    partition.max_mib = None;
    partition.fill_to_end = None;
    partition.gap_after_mib = None;
}

fn configure_recreated_data_partition(partition: &mut LayoutPartition, data_start_mib: u64) {
    partition.mode = PartitionMode::Mkpart;
    partition.start_mib = Some(data_start_mib);
    partition.end_mib = None;
    partition.size_mib = None;
    partition.size_source = None;
    partition.target_percent = None;
    partition.max_mib = None;
    partition.fill_to_end = Some(true);
    partition.gap_after_mib = None;
}

fn manifest_is_replayable_after_data_repartition(config: &UpdaterConfig, manifest: &ReleaseManifest) -> std::result::Result<(), String> {
    if manifest.artifacts.is_empty() {
        return Err("manifest contains no artifacts".into());
    }

    for artifact in &manifest.artifacts {
        match artifact.url.scheme() {
            "http" | "https" => {}
            "file" => {
                let path = artifact
                    .url
                    .to_file_path()
                    .map_err(|_| format!("artifact {} is not a valid absolute file URL", artifact.url))?;
                if path.starts_with(config.data_dir()) || path.starts_with(config.cache_dir()) || path.starts_with(config.work_dir()) {
                    return Err(format!("artifact {} lives under {}, so it will disappear when DATA is recreated", artifact.url, config.data_dir().display()));
                }
            }
            scheme => {
                return Err(format!("artifact {} uses unsupported replay scheme {}", artifact.url, scheme));
            }
        }
    }

    Ok(())
}

fn force_auto_apply(manifest: &ReleaseManifest) -> ReleaseManifest {
    let mut updated = manifest.clone();
    let mut metadata = if updated.metadata_json.trim().is_empty() {
        serde_json::Value::Object(serde_json::Map::new())
    } else {
        serde_json::from_str::<serde_json::Value>(&updated.metadata_json).unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()))
    };

    if !metadata.is_object() {
        metadata = serde_json::Value::Object(serde_json::Map::new());
    }
    if let Some(object) = metadata.as_object_mut() {
        object.insert("auto_apply".into(), serde_json::Value::Bool(true));
    }
    updated.metadata_json = serde_json::to_string(&metadata).unwrap_or_else(|_| "{\"auto_apply\":true}".to_string());
    updated
}

fn render_request_env(update_id: Uuid, target_slot: &str) -> String {
    format!(
        "HELIOS_REPARTITION_REQUEST_VERSION={}\nHELIOS_REPARTITION_UPDATE_ID={}\nHELIOS_REPARTITION_TARGET_SLOT={}\n",
        REPARTITION_REQUEST_VERSION, update_id, target_slot
    )
}

fn parse_env_map(raw: &str) -> std::collections::BTreeMap<String, String> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim().to_string(), value.trim().trim_matches('\'').trim_matches('"').to_string()))
        .collect()
}

fn ota_dir_for_boot(boot_dir: &Path) -> PathBuf {
    OTA_DIR_COMPONENTS.iter().fold(boot_dir.to_path_buf(), |path, component| path.join(component))
}

async fn maybe_unmount_boot_dir(boot_dir: &Path, mounted: bool) -> Result<()> {
    if !mounted {
        return Ok(());
    }
    let status = Command::new("umount").arg(boot_dir).status().await.map_err(Error::Io)?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::InvalidState(format!("failed to unmount BOOT at {}", boot_dir.display())))
    }
}

async fn remove_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(Error::Io(err)),
    }
}

const fn align_up_bytes(value: u64, align: u64) -> u64 {
    if align == 0 { value } else { value.div_ceil(align).saturating_mul(align) }
}

const fn align_up_mib(value: u64, align: u64) -> u64 {
    if align == 0 { value } else { value.div_ceil(align).saturating_mul(align) }
}

const fn bytes_to_mib_floor(value: u64) -> u64 {
    value / MIB
}

const fn bytes_to_mib_ceil(value: u64) -> u64 {
    value.div_ceil(MIB)
}

#[cfg(test)]
mod tests {
    use super::{
        LAYOUT_TOML_NAME, OfflineDataBorrowAssessment, OfflineDataBorrowBlocker, RESULT_ENV_NAME, RESUME_JSON_NAME, StorageLayoutManifest, UpdaterConfig, assess_offline_data_borrow,
        build_offline_data_borrow_layout, clear_queued_repartition_resume_from_dir, load_queued_repartition_resume_from_dir, ota_dir_for_boot,
    };
    use crate::artifact::{ManifestArtifact, ReleaseManifest};
    use crate::util::BlockPartitionInfo;
    use lib_storage_layout::PartitionRole;
    use std::path::PathBuf;
    use url::Url;
    use uuid::Uuid;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../gaia/assets/generated/storage-layouts").join(name)
    }

    fn replayable_manifest(url: &str) -> ReleaseManifest {
        ReleaseManifest {
            update_id: None,
            version: Some("test".into()),
            artifacts: vec![ManifestArtifact {
                url: Url::parse(url).expect("url"),
                filename: Some("image.img".into()),
                size_bytes: Some(1),
                sha256: None,
                signature: None,
                kind: Some("disk-image".into()),
            }],
            metadata_json: "{}".into(),
        }
    }

    #[test]
    fn assess_offline_data_borrow_rejects_non_adjacent_target_slot() {
        let layout = StorageLayoutManifest::load_from_path(&fixture("squashfs-ab.toml")).expect("layout");
        let config = UpdaterConfig::new("/tmp/updater.sock", "/tmp/updater.log");
        let manifest = replayable_manifest("https://example.invalid/helios.img");
        let target = BlockPartitionInfo { device: "/dev/mmcblk0p2".into(), disk: "/dev/mmcblk0".into(), number: 2, sector_bytes: 512, start_sectors: 8192, size_sectors: 188_416 };
        let next = BlockPartitionInfo { device: "/dev/mmcblk0p3".into(), disk: "/dev/mmcblk0".into(), number: 3, sector_bytes: 512, start_sectors: 196_608, size_sectors: 188_416 };

        let assessment = assess_offline_data_borrow(&config, &layout, &manifest, &target, Some(&next), 6_291_456, 2_097_152, 4_194_304);
        assert_eq!(assessment, OfflineDataBorrowAssessment::Blocked(OfflineDataBorrowBlocker::TargetNotAdjacentToData));
    }

    #[test]
    fn assess_offline_data_borrow_rejects_artifacts_staged_on_data() {
        let layout = StorageLayoutManifest::load_from_path(&fixture("squashfs-ab.toml")).expect("layout");
        let config = UpdaterConfig::new("/tmp/updater.sock", "/tmp/updater.log");
        let manifest = replayable_manifest("file:///var/lib/helios/ota/cache/update/helios.img");
        let target = BlockPartitionInfo { device: "/dev/mmcblk0p3".into(), disk: "/dev/mmcblk0".into(), number: 3, sector_bytes: 512, start_sectors: 196_608, size_sectors: 188_416 };
        let next = BlockPartitionInfo { device: "/dev/mmcblk0p4".into(), disk: "/dev/mmcblk0".into(), number: 4, sector_bytes: 512, start_sectors: 389_120, size_sectors: 1_000_000 };

        let assessment = assess_offline_data_borrow(&config, &layout, &manifest, &target, Some(&next), 6_291_456, 2_097_152, 4_194_304);
        match assessment {
            OfflineDataBorrowAssessment::Blocked(OfflineDataBorrowBlocker::NonReplayableArtifact(reason)) => {
                assert!(reason.contains("/var/lib/helios"));
            }
            other => panic!("expected non-replayable artifact blocker, got {other:?}"),
        }
    }

    #[test]
    fn assess_offline_data_borrow_supports_adjacent_replayable_target_slot() {
        let layout = StorageLayoutManifest::load_from_path(&fixture("squashfs-ab.toml")).expect("layout");
        let config = UpdaterConfig::new("/tmp/updater.sock", "/tmp/updater.log");
        let manifest = replayable_manifest("https://example.invalid/helios.img");
        let target = BlockPartitionInfo { device: "/dev/mmcblk0p3".into(), disk: "/dev/mmcblk0".into(), number: 3, sector_bytes: 512, start_sectors: 196_608, size_sectors: 188_416 };
        let next = BlockPartitionInfo { device: "/dev/mmcblk0p4".into(), disk: "/dev/mmcblk0".into(), number: 4, sector_bytes: 512, start_sectors: 389_120, size_sectors: 1_000_000 };

        let assessment = assess_offline_data_borrow(&config, &layout, &manifest, &target, Some(&next), 6_291_456, 2_097_152, 4_194_304);
        match assessment {
            OfflineDataBorrowAssessment::Supported(plan) => {
                assert_eq!(plan.target_role, PartitionRole::SlotB);
                assert_eq!(plan.required_growth_bytes, 6_291_456);
                assert_eq!(plan.gap_after_bytes, 2_097_152);
                assert_eq!(plan.additional_from_data_bytes, 4_194_304);
                assert_eq!(plan.target_start_mib, 96);
                assert_eq!(plan.target_end_mib, 196);
            }
            other => panic!("expected supported plan, got {other:?}"),
        }
    }

    #[test]
    fn generated_layout_moves_data_immediately_after_target_slot() {
        let layout = StorageLayoutManifest::load_from_path(&fixture("squashfs-ab.toml")).expect("layout");
        let generated = build_offline_data_borrow_layout(
            &layout,
            &super::OfflineDataBorrowPlan {
                target_role: PartitionRole::SlotB,
                target_start_mib: 128,
                target_end_mib: 448,
                gap_after_bytes: 2_097_152,
                required_growth_bytes: 6_291_456,
                additional_from_data_bytes: 4_194_304,
            },
        )
        .expect("generated layout");

        let target = generated.slot_b().expect("slot b");
        assert_eq!(target.mode, lib_storage_layout::PartitionMode::Mkpart);
        assert_eq!(target.start_mib, Some(128));
        assert_eq!(target.end_mib, Some(448));
        assert_eq!(target.gap_after_mib, Some(0));

        let live = generated.slot_a().expect("slot a");
        assert_eq!(live.mode, lib_storage_layout::PartitionMode::Noop);

        let data = generated.data().expect("data");
        assert_eq!(data.start_mib, Some(448));
        assert_eq!(data.fill_to_end, Some(true));
    }

    #[tokio::test]
    async fn queued_resume_requires_success_result_marker() {
        let temp = tempfile::tempdir().expect("temp dir");
        let ota_dir = ota_dir_for_boot(temp.path());
        tokio::fs::create_dir_all(&ota_dir).await.expect("ota dir");

        let update_id = Uuid::new_v4();
        let resume = super::QueuedRepartitionResume { update_id, manifest: replayable_manifest("https://example.invalid/helios.img") };
        tokio::fs::write(ota_dir.join(RESULT_ENV_NAME), format!("HELIOS_REPARTITION_RESULT_VERSION=1\nHELIOS_REPARTITION_UPDATE_ID={update_id}\nHELIOS_REPARTITION_STATUS=applied\n"))
            .await
            .expect("result env");
        tokio::fs::write(ota_dir.join(RESUME_JSON_NAME), serde_json::to_vec(&resume).expect("resume json")).await.expect("resume json write");

        let loaded = load_queued_repartition_resume_from_dir(temp.path()).await.expect("load").expect("resume");
        assert_eq!(loaded.update_id, resume.update_id);
        assert_eq!(loaded.manifest.metadata_json, resume.manifest.metadata_json);
        assert_eq!(loaded.manifest.artifacts.len(), resume.manifest.artifacts.len());
        assert_eq!(loaded.manifest.artifacts[0].url, resume.manifest.artifacts[0].url);

        clear_queued_repartition_resume_from_dir(temp.path()).await.expect("clear");
        assert!(tokio::fs::metadata(ota_dir.join(RESULT_ENV_NAME)).await.is_err());
        assert!(tokio::fs::metadata(ota_dir.join(RESUME_JSON_NAME)).await.is_err());
        assert!(tokio::fs::metadata(ota_dir.join(LAYOUT_TOML_NAME)).await.is_err());
    }
}
