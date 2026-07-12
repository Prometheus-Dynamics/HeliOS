use std::path::Path;

use anyhow::{anyhow, Context, Result};

use crate::config::{Config, Defaults, Partition, Spans};
use crate::geometry::{read_part_info, read_to_u64, sectors_to_mib_ceil};
use crate::storage_layout::{LayoutPartition, PartitionMode, SizeSource, StorageLayoutManifest};

pub fn load_layout_config(path: &Path) -> Result<Config> {
    let layout = StorageLayoutManifest::load_from_path(path).map_err(|err| anyhow!(err))?;
    let disk = layout.defaults.disk.clone().or_else(crate::geometry::detect_disk).unwrap_or_else(|| "/dev/mmcblk0".to_string());
    let sector_bytes = read_to_u64(&format!("/sys/class/block/{}/queue/logical_block_size", crate::geometry::disk_bn(&disk)))?.unwrap_or(512);
    let mirrored_slot_size_mib = compute_mirrored_slot_size_mib(&layout, &disk, sector_bytes).ok();

    let partitions = layout.partitions.iter().map(|partition| convert_partition(partition, mirrored_slot_size_mib)).collect::<Result<Vec<_>>>()?;

    Ok(Config {
        defaults: Defaults {
            disk: layout.defaults.disk.clone(),
            align_mib: layout.defaults.align_mib,
            gap_mib: layout.defaults.gap_mib,
            root_min_mib: layout.defaults.root_min_mib,
            log_file: layout.defaults.log_file.clone(),
        },
        spans: Spans { boot_partition: layout.spans.boot_partition, start_after_partition: layout.spans.start_after_partition, data_size_mib: layout.spans.data_size_mib },
        partitions,
    })
}

fn convert_partition(partition: &LayoutPartition, mirrored_slot_size_mib: Option<u64>) -> Result<Partition> {
    let size_mib = match partition.size_source {
        Some(SizeSource::MirrorExisting) => {
            Some(mirrored_slot_size_mib.ok_or_else(|| anyhow!("layout partition {} requests mirror_existing size but the current disk geometry could not be read", partition.name))?)
        }
        None => partition.size_mib,
    };

    Ok(Partition {
        name: partition.name.clone(),
        number: partition.number,
        label: partition.label.clone(),
        mode: partition.mode.into(),
        fs_type: partition.fs_type.clone(),
        marker: partition.marker.clone(),
        target_percent: partition.target_percent,
        size_mib,
        max_mib: partition.max_mib,
        fill_to_end: partition.fill_to_end,
        start_mib: partition.start_mib,
        end_mib: partition.end_mib,
        mkfs: partition.mkfs,
        wipe_signatures: partition.wipe_signatures,
        run_fsck: partition.run_fsck,
        run_resizefs: partition.run_resizefs,
        mount_point: partition.mount_point.clone(),
        mount_label: partition.mount_label.clone(),
        gap_after_mib: partition.gap_after_mib,
        secondary_marker: partition.secondary_marker.clone(),
        reformat_if_missing_secondary_marker: partition.reformat_if_missing_secondary_marker,
    })
}

fn compute_mirrored_slot_size_mib(layout: &StorageLayoutManifest, disk: &str, sector_bytes: u64) -> Result<u64> {
    let source = mirrored_slot_source_partition(layout)?;
    let boot_partition = layout.boot_partition_number().ok_or_else(|| anyhow!("layout {} is missing spans.boot_partition", layout.layout_id))?;
    let boot_info = read_part_info(disk, boot_partition).with_context(|| format!("failed to inspect boot partition {} for layout {}", boot_partition, layout.layout_id))?;
    let source_info = read_part_info(disk, source.number).with_context(|| format!("failed to inspect source slot {} for layout {}", source.number, layout.layout_id))?;
    Ok(resolve_mirror_existing_size_mib(sectors_to_mib_ceil(boot_info.size, sector_bytes), sectors_to_mib_ceil(source_info.size, sector_bytes), layout.defaults.align_mib.unwrap_or(4)))
}

fn mirrored_slot_source_partition(layout: &StorageLayoutManifest) -> Result<&LayoutPartition> {
    layout
        .partitions
        .iter()
        .find(|partition| matches!(partition.role, crate::storage_layout::PartitionRole::SlotA | crate::storage_layout::PartitionRole::SlotB) && partition.mode == PartitionMode::Noop)
        .or_else(|| layout.partitions.iter().find(|partition| partition.role == crate::storage_layout::PartitionRole::SlotA))
        .ok_or_else(|| anyhow!("layout is missing an A/B source slot for mirror_existing sizing"))
}

fn resolve_mirror_existing_size_mib(anchor_size_mib: u64, source_size_mib: u64, align_mib: u64) -> u64 {
    let aligned_anchor = align_up(anchor_size_mib, align_mib);
    let alignment_gap = aligned_anchor.saturating_sub(anchor_size_mib);
    align_up(source_size_mib.saturating_sub(alignment_gap), align_mib)
}

const fn align_up(value: u64, align: u64) -> u64 {
    if align == 0 {
        value
    } else {
        value.div_ceil(align).saturating_mul(align)
    }
}

impl From<PartitionMode> for crate::config::Mode {
    fn from(value: PartitionMode) -> Self {
        match value {
            PartitionMode::Resize => Self::Resize,
            PartitionMode::Mkpart => Self::Mkpart,
            PartitionMode::Noop => Self::Noop,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{mirrored_slot_source_partition, resolve_mirror_existing_size_mib};
    use std::path::PathBuf;

    use crate::storage_layout::StorageLayoutManifest;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../gaia/assets/generated/storage-layouts").join(name)
    }

    #[test]
    fn resolves_squashfs_layout_fixture() {
        let layout = StorageLayoutManifest::load_from_path(&fixture("squashfs-ab.toml")).expect("squashfs layout");
        assert_eq!(layout.layout_id, "squashfs_ab");
        assert_eq!(layout.slot_a().unwrap().number, 2);
        assert_eq!(layout.slot_b().unwrap().number, 3);
    }

    #[test]
    fn mirror_existing_size_keeps_alignment_gap_stable() {
        assert_eq!(resolve_mirror_existing_size_mib(124, 112, 4), 112);
        assert_eq!(resolve_mirror_existing_size_mib(123, 112, 4), 112);
        assert_eq!(resolve_mirror_existing_size_mib(125, 116, 4), 116);
    }

    #[test]
    fn mirrored_slot_source_uses_noop_ab_partition() {
        let layout = StorageLayoutManifest::load_from_path(&fixture("squashfs-ab.toml")).expect("squashfs layout");
        let mut swapped = layout.clone();
        for partition in &mut swapped.partitions {
            match partition.role {
                crate::storage_layout::PartitionRole::SlotA => {
                    partition.mode = crate::storage_layout::PartitionMode::Mkpart;
                    partition.fs_type = Some("ext4".into());
                }
                crate::storage_layout::PartitionRole::SlotB => {
                    partition.mode = crate::storage_layout::PartitionMode::Noop;
                }
                crate::storage_layout::PartitionRole::Data => {}
            }
        }
        let source = mirrored_slot_source_partition(&swapped).expect("source slot");
        assert_eq!(source.name, "ROOT_B");
        assert_eq!(source.number, 3);
    }
}
