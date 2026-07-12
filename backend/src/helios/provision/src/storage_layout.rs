use std::fs;
use std::path::{Path, PathBuf};

use lib_schema_migration::{normalize_to_current, SyncSchemaPlan};
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
    #[cfg(test)]
    MissingRole(PartitionRole),
}

const STORAGE_LAYOUT_SCHEMA_PLAN: SyncSchemaPlan<toml::Value> = SyncSchemaPlan::strict("storage layout manifest", CURRENT_STORAGE_LAYOUT_SCHEMA_VERSION);

impl StorageLayoutManifest {
    pub fn load_from_path(path: &Path) -> Result<Self, LayoutError> {
        let raw = fs::read_to_string(path).map_err(|source| LayoutError::Io { path: path.to_path_buf(), source })?;
        Self::load_from_str(path, &raw)
    }

    fn load_from_str(path: &Path, raw: &str) -> Result<Self, LayoutError> {
        let raw_value = toml::from_str::<toml::Value>(raw).map_err(|source| LayoutError::Parse { path: path.to_path_buf(), source })?;
        let migrated = normalize_to_current(raw_value, &STORAGE_LAYOUT_SCHEMA_PLAN).map_err(|message| LayoutError::Migration { path: path.to_path_buf(), message })?;
        let mut manifest: StorageLayoutManifest = migrated.try_into().map_err(|source| LayoutError::Parse { path: path.to_path_buf(), source })?;
        manifest.schema_version = CURRENT_STORAGE_LAYOUT_SCHEMA_VERSION;
        Ok(manifest)
    }

    pub fn system_layout_path() -> PathBuf {
        std::env::var_os(SYSTEM_LAYOUT_ENV_VAR).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(DEFAULT_SYSTEM_LAYOUT_PATH))
    }

    #[cfg(test)]
    pub fn partition_by_role(&self, role: PartitionRole) -> Result<&LayoutPartition, LayoutError> {
        self.partitions.iter().find(|partition| partition.role == role).ok_or(LayoutError::MissingRole(role))
    }

    #[cfg(test)]
    pub fn slot_a(&self) -> Result<&LayoutPartition, LayoutError> {
        self.partition_by_role(PartitionRole::SlotA)
    }

    #[cfg(test)]
    pub fn slot_b(&self) -> Result<&LayoutPartition, LayoutError> {
        self.partition_by_role(PartitionRole::SlotB)
    }

    #[cfg(test)]
    pub fn data(&self) -> Result<&LayoutPartition, LayoutError> {
        self.partition_by_role(PartitionRole::Data)
    }

    pub fn boot_partition_number(&self) -> Option<u32> {
        self.spans.boot_partition
    }

    #[cfg(test)]
    pub fn slot_partition_numbers(&self) -> Result<(u32, u32), LayoutError> {
        Ok((self.slot_a()?.number, self.slot_b()?.number))
    }

    #[cfg(test)]
    pub fn allows_destructive_data_borrow(&self) -> bool {
        self.live_repartition.allow_destructive_data_borrow
    }
}

#[cfg(test)]
mod tests {
    use super::{SlotScheme, StorageLayoutManifest};
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../gaia/assets/generated/storage-layouts").join(name)
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
}
