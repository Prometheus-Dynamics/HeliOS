use std::fs;
use std::path::{Path, PathBuf};

use lib_schema_migration::{SyncSchemaPlan, migrate_to_current};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const DEFAULT_SYSTEM_LAYOUT_PATH: &str = "/etc/helios/storage-layout.toml";
pub const SYSTEM_LAYOUT_ENV_VAR: &str = "HELIOS_STORAGE_LAYOUT_MANIFEST_PATH";
pub const CURRENT_STORAGE_LAYOUT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotScheme {
    Ext4Labels,
    SquashfsAb,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartitionRole {
    SlotA,
    SlotB,
    Data,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartitionMode {
    Resize,
    Mkpart,
    Noop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SizeSource {
    MirrorExisting,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LayoutDefaults {
    pub disk: Option<String>,
    pub align_mib: Option<u64>,
    pub gap_mib: Option<u64>,
    pub root_min_mib: Option<u64>,
    pub log_file: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LayoutSpans {
    pub boot_partition: Option<u32>,
    pub start_after_partition: Option<u32>,
    pub data_size_mib: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LiveRepartitionPolicy {
    #[serde(default)]
    pub allow_destructive_data_borrow: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayoutPartition {
    pub role: PartitionRole,
    pub name: String,
    pub number: u32,
    pub label: Option<String>,
    pub mode: PartitionMode,
    pub fs_type: Option<String>,
    pub marker: Option<String>,
    pub target_percent: Option<u64>,
    pub size_mib: Option<u64>,
    pub size_source: Option<SizeSource>,
    pub max_mib: Option<u64>,
    pub fill_to_end: Option<bool>,
    pub start_mib: Option<u64>,
    pub end_mib: Option<u64>,
    pub mkfs: Option<bool>,
    pub wipe_signatures: Option<bool>,
    pub run_fsck: Option<bool>,
    pub run_resizefs: Option<bool>,
    pub mount_point: Option<String>,
    pub mount_label: Option<String>,
    pub gap_after_mib: Option<u64>,
    pub secondary_marker: Option<String>,
    pub reformat_if_missing_secondary_marker: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StorageLayoutManifest {
    pub schema_version: u32,
    pub layout_id: String,
    pub slot_scheme: SlotScheme,
    pub boot_label: Option<String>,
    #[serde(default)]
    pub defaults: LayoutDefaults,
    #[serde(default)]
    pub spans: LayoutSpans,
    #[serde(default)]
    pub live_repartition: LiveRepartitionPolicy,
    #[serde(default)]
    pub partitions: Vec<LayoutPartition>,
}

#[derive(Debug, Error)]
pub enum LayoutError {
    #[error("failed to read layout manifest {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse layout manifest {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("failed to migrate layout manifest {path}: {message}")]
    Migration { path: PathBuf, message: String },
    #[error("layout manifest is missing required role {0:?}")]
    MissingRole(PartitionRole),
}

const STORAGE_LAYOUT_SCHEMA_PLAN: SyncSchemaPlan<toml::Value> =
    SyncSchemaPlan { document_name: "storage layout manifest", legacy_version: CURRENT_STORAGE_LAYOUT_SCHEMA_VERSION, current_version: CURRENT_STORAGE_LAYOUT_SCHEMA_VERSION, migrations: &[] };

impl StorageLayoutManifest {
    pub fn load_from_path(path: &Path) -> Result<Self, LayoutError> {
        let raw = fs::read_to_string(path).map_err(|source| LayoutError::Io { path: path.to_path_buf(), source })?;
        Self::load_from_str(path, &raw)
    }

    fn load_from_str(path: &Path, raw: &str) -> Result<Self, LayoutError> {
        let raw_value = toml::from_str::<toml::Value>(raw).map_err(|source| LayoutError::Parse { path: path.to_path_buf(), source })?;
        let migrated = migrate_to_current(raw_value, &STORAGE_LAYOUT_SCHEMA_PLAN).map_err(|message| LayoutError::Migration { path: path.to_path_buf(), message })?;
        let mut manifest: StorageLayoutManifest = migrated.try_into().map_err(|source| LayoutError::Parse { path: path.to_path_buf(), source })?;
        manifest.schema_version = CURRENT_STORAGE_LAYOUT_SCHEMA_VERSION;
        Ok(manifest)
    }

    pub fn system_layout_path() -> PathBuf {
        std::env::var_os(SYSTEM_LAYOUT_ENV_VAR).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(DEFAULT_SYSTEM_LAYOUT_PATH))
    }

    pub fn load_system() -> Result<Self, LayoutError> {
        Self::load_from_path(&Self::system_layout_path())
    }

    pub fn partition_by_role(&self, role: PartitionRole) -> Result<&LayoutPartition, LayoutError> {
        self.partitions.iter().find(|partition| partition.role == role).ok_or(LayoutError::MissingRole(role))
    }

    pub fn slot_a(&self) -> Result<&LayoutPartition, LayoutError> {
        self.partition_by_role(PartitionRole::SlotA)
    }

    pub fn slot_b(&self) -> Result<&LayoutPartition, LayoutError> {
        self.partition_by_role(PartitionRole::SlotB)
    }

    pub fn data(&self) -> Result<&LayoutPartition, LayoutError> {
        self.partition_by_role(PartitionRole::Data)
    }

    pub fn boot_partition_number(&self) -> Option<u32> {
        self.spans.boot_partition
    }

    pub fn slot_name(&self, role: PartitionRole) -> Result<&str, LayoutError> {
        Ok(self.partition_by_role(role)?.name.as_str())
    }

    pub fn partition_device_for_disk(disk: &str, number: u32) -> String {
        if disk.starts_with("/dev/mmcblk") || disk.starts_with("/dev/nvme") { format!("{disk}p{number}") } else { format!("{disk}{number}") }
    }

    pub fn disk_from_partition_device(dev: &str) -> Option<String> {
        match dev {
            path if path.starts_with("/dev/mmcblk") || path.starts_with("/dev/nvme") => {
                let (base, suffix) = path.rsplit_once('p')?;
                if suffix.chars().all(|ch| ch.is_ascii_digit()) { Some(base.to_string()) } else { None }
            }
            path if path.starts_with("/dev/sd") => Some(path.trim_end_matches(char::is_numeric).to_string()),
            _ => None,
        }
    }

    pub fn slot_device_for_disk(&self, role: PartitionRole, disk: &str) -> Result<String, LayoutError> {
        Ok(Self::partition_device_for_disk(disk, self.partition_by_role(role)?.number))
    }

    pub fn slot_partition_numbers(&self) -> Result<(u32, u32), LayoutError> {
        Ok((self.slot_a()?.number, self.slot_b()?.number))
    }

    pub fn allows_destructive_data_borrow(&self) -> bool {
        self.live_repartition.allow_destructive_data_borrow
    }
}

#[cfg(test)]
mod tests {
    use super::{CURRENT_STORAGE_LAYOUT_SCHEMA_VERSION, PartitionRole, SlotScheme, StorageLayoutManifest};
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../gaia/assets/generated/storage-layouts").join(name)
    }

    #[test]
    fn parses_ext4_labels_layout() {
        let layout = StorageLayoutManifest::load_from_path(&fixture("ext4-labels.toml")).expect("ext4 manifest");
        assert_eq!(layout.layout_id, "ext4_labels");
        assert_eq!(layout.slot_scheme, SlotScheme::Ext4Labels);
        assert_eq!(layout.boot_partition_number(), Some(1));
        assert_eq!(layout.slot_name(PartitionRole::SlotA).unwrap(), "ACTIVE");
        assert_eq!(layout.slot_name(PartitionRole::SlotB).unwrap(), "RESERVE");
        assert_eq!(layout.data().unwrap().label.as_deref(), Some("DATA"));
    }

    #[test]
    fn parses_squashfs_ab_layout() {
        let layout = StorageLayoutManifest::load_from_path(&fixture("squashfs-ab.toml")).expect("squashfs manifest");
        assert_eq!(layout.layout_id, "squashfs_ab");
        assert_eq!(layout.slot_scheme, SlotScheme::SquashfsAb);
        assert_eq!(layout.slot_partition_numbers().unwrap(), (2, 3));
        assert_eq!(layout.data().unwrap().number, 4);
        assert_eq!(layout.data().unwrap().label.as_deref(), Some("DATA"));
        assert!(layout.allows_destructive_data_borrow());
    }

    #[test]
    fn derives_partition_device_for_mmc_and_sd() {
        assert_eq!(StorageLayoutManifest::partition_device_for_disk("/dev/mmcblk0", 3), "/dev/mmcblk0p3");
        assert_eq!(StorageLayoutManifest::partition_device_for_disk("/dev/nvme0n1", 4), "/dev/nvme0n1p4");
        assert_eq!(StorageLayoutManifest::partition_device_for_disk("/dev/sda", 2), "/dev/sda2");
    }

    #[test]
    fn rejects_missing_layout_schema_version() {
        let path = fixture("ext4-labels.toml");
        let raw = std::fs::read_to_string(&path).expect("read layout fixture");
        let raw = raw.replace("schema_version = 1\n", "");
        let err = StorageLayoutManifest::load_from_str(&path, &raw).expect_err("missing schema version should fail");
        assert!(err.to_string().contains("missing required schema_version"));
    }

    #[test]
    fn rejects_future_layout_schema_version() {
        let path = fixture("ext4-labels.toml");
        let raw = std::fs::read_to_string(&path).expect("read layout fixture");
        let raw = raw.replace("schema_version = 1", "schema_version = 2");
        let err = StorageLayoutManifest::load_from_str(&path, &raw).expect_err("future layout should fail");
        assert!(err.to_string().contains("unsupported storage layout manifest schema_version"));
    }
}
