use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use crate::{
    config::{DEFAULT_OTA_ACTIVE_PATH, DEFAULT_OTA_RESERVE_PATH, DEFAULT_STORAGE_LAYOUT_ENV_PATH},
    model::{SlotLayout, UpdateSlot},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotProbeConfig {
    pub storage_layout_env_path: PathBuf,
    pub ota_active_path: PathBuf,
    pub ota_reserve_path: PathBuf,
}

impl Default for SlotProbeConfig {
    fn default() -> Self {
        Self { storage_layout_env_path: DEFAULT_STORAGE_LAYOUT_ENV_PATH.into(), ota_active_path: DEFAULT_OTA_ACTIVE_PATH.into(), ota_reserve_path: DEFAULT_OTA_RESERVE_PATH.into() }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SlotProbeError {
    #[error("storage layout env not found at {path}")]
    MissingLayoutEnv { path: PathBuf },
    #[error("storage layout env is missing required key {key}")]
    MissingLayoutKey { key: &'static str },
    #[error("invalid partition number for {key}: {value}")]
    InvalidPartitionNumber { key: &'static str, value: String },
    #[error("unable to determine active slot")]
    UnknownActiveSlot,
    #[error("unable to determine base disk for slots")]
    MissingBaseDisk,
    #[error("block device {path} size metadata is missing")]
    MissingBlockSize { path: PathBuf },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StorageLayoutEnv {
    slot_scheme: String,
    data_label: Option<String>,
    boot_label: Option<String>,
    boot_partition: u32,
    slot_a_name: String,
    slot_a_label: Option<String>,
    slot_a_partition: u32,
    slot_b_name: String,
    slot_b_label: Option<String>,
    slot_b_partition: u32,
}

pub fn probe_slot_layout() -> Result<SlotLayout, SlotProbeError> {
    probe_slot_layout_with(SlotProbeConfig::default())
}

fn probe_slot_layout_with(config: SlotProbeConfig) -> Result<SlotLayout, SlotProbeError> {
    let layout = read_storage_layout_env(&config.storage_layout_env_path)?;

    let active_name = read_trimmed(&config.ota_active_path)?;
    let reserve_name = read_trimmed(&config.ota_reserve_path)?;

    let active = match active_name.as_deref() {
        Some(name) if name == layout.slot_a_name => UpdateSlot::A,
        Some(name) if name == layout.slot_b_name => UpdateSlot::B,
        _ => infer_active_slot(&layout)?,
    };
    let inactive = UpdateSlot::inactive_for(active);

    let active_name = slot_name(&layout, active);
    let inactive_name = slot_name(&layout, inactive);

    if let Some(reserve_name) = reserve_name.as_deref() {
        if reserve_name != inactive_name && reserve_name != active_name {
            return Err(SlotProbeError::UnknownActiveSlot);
        }
    }

    let active_device = resolve_slot_device(&layout, active)?;
    let inactive_device = resolve_slot_device(&layout, inactive)?;
    let inactive_exists = inactive_device.exists();
    let inactive_size_bytes = if inactive_exists { Some(block_device_size_bytes(&inactive_device)?) } else { None };
    let required_size_bytes = if active_device.exists() { Some(block_device_size_bytes(&active_device)?) } else { None };

    Ok(SlotLayout {
        scheme: layout.slot_scheme.clone(),
        active,
        active_name: active_name.to_string(),
        active_label: slot_label(&layout, active).map(ToOwned::to_owned),
        active_device: Some(active_device),
        inactive: Some(inactive),
        inactive_name: Some(inactive_name.to_string()),
        inactive_label: slot_label(&layout, inactive).map(ToOwned::to_owned),
        inactive_device: Some(inactive_device),
        inactive_exists,
        inactive_size_bytes,
        required_size_bytes,
    })
}

fn read_storage_layout_env(path: &Path) -> Result<StorageLayoutEnv, SlotProbeError> {
    if !path.exists() {
        return Err(SlotProbeError::MissingLayoutEnv { path: path.to_path_buf() });
    }
    let env = parse_shell_env(&fs::read_to_string(path)?);
    Ok(StorageLayoutEnv {
        slot_scheme: env.get("HELIOS_LAYOUT_SLOT_SCHEME").cloned().unwrap_or_else(|| "squashfs_ab".to_string()),
        data_label: env.get("HELIOS_LAYOUT_DATA_LABEL").cloned(),
        boot_label: env.get("HELIOS_LAYOUT_BOOT_LABEL").cloned(),
        boot_partition: parse_partition_number(&env, "HELIOS_LAYOUT_BOOT_PARTITION").unwrap_or(1),
        slot_a_name: require_layout_key(&env, "HELIOS_LAYOUT_SLOT_A_NAME")?,
        slot_a_label: env.get("HELIOS_LAYOUT_SLOT_A_LABEL").cloned(),
        slot_a_partition: parse_partition_number(&env, "HELIOS_LAYOUT_SLOT_A_PARTITION")?,
        slot_b_name: require_layout_key(&env, "HELIOS_LAYOUT_SLOT_B_NAME")?,
        slot_b_label: env.get("HELIOS_LAYOUT_SLOT_B_LABEL").cloned(),
        slot_b_partition: parse_partition_number(&env, "HELIOS_LAYOUT_SLOT_B_PARTITION")?,
    })
}

fn require_layout_key(env: &BTreeMap<String, String>, key: &'static str) -> Result<String, SlotProbeError> {
    env.get(key).filter(|value| !value.trim().is_empty()).cloned().ok_or(SlotProbeError::MissingLayoutKey { key })
}

fn parse_partition_number(env: &BTreeMap<String, String>, key: &'static str) -> Result<u32, SlotProbeError> {
    let value = require_layout_key(env, key)?;
    value.parse::<u32>().map_err(|_| SlotProbeError::InvalidPartitionNumber { key, value: value.clone() })
}

fn parse_shell_env(src: &str) -> BTreeMap<String, String> {
    src.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            let (key, value) = trimmed.split_once('=')?;
            Some((key.trim().to_string(), unquote_shell(value.trim())))
        })
        .collect()
}

fn unquote_shell(value: &str) -> String {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'') || (bytes[0] == b'"' && bytes[value.len() - 1] == b'"') {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}

fn read_trimmed(path: &Path) -> Result<Option<String>, SlotProbeError> {
    match fs::read_to_string(path) {
        Ok(contents) => {
            let value = contents.trim().to_string();
            if value.is_empty() { Ok(None) } else { Ok(Some(value)) }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(SlotProbeError::Io(error)),
    }
}

fn infer_active_slot(layout: &StorageLayoutEnv) -> Result<UpdateSlot, SlotProbeError> {
    let cmdline_root = read_cmdline_root()?;
    if let Some(root) = cmdline_root.as_deref() {
        if slot_matches_value(root, layout, UpdateSlot::A)? {
            return Ok(UpdateSlot::A);
        }
        if slot_matches_value(root, layout, UpdateSlot::B)? {
            return Ok(UpdateSlot::B);
        }
    }
    Err(SlotProbeError::UnknownActiveSlot)
}

fn read_cmdline_root() -> Result<Option<String>, SlotProbeError> {
    let cmdline = fs::read_to_string("/proc/cmdline")?;
    Ok(cmdline.split_whitespace().find_map(|part| part.strip_prefix("root=").map(ToOwned::to_owned)))
}

fn slot_matches_value(root: &str, layout: &StorageLayoutEnv, slot: UpdateSlot) -> Result<bool, SlotProbeError> {
    let slot_name = slot_name(layout, slot);
    let slot_label = slot_label(layout, slot);
    let slot_device = resolve_slot_device(layout, slot)?;

    if root == slot_name {
        return Ok(true);
    }

    if let Some(label) = slot_label {
        if root == format!("LABEL={label}") {
            return Ok(true);
        }
    }

    let canonical_device = canonicalize_path(&slot_device);
    Ok(root == slot_device.display().to_string() || root == canonical_device.display().to_string())
}

fn resolve_slot_device(layout: &StorageLayoutEnv, slot: UpdateSlot) -> Result<PathBuf, SlotProbeError> {
    if let Some(label) = slot_label(layout, slot) {
        let label_path = PathBuf::from("/dev/disk/by-label").join(label);
        if label_path.exists() {
            return Ok(canonicalize_path(&label_path));
        }
    }

    let base_disk = resolve_base_disk(layout)?;
    Ok(partition_device(&base_disk, slot_partition(layout, slot)))
}

fn resolve_base_disk(layout: &StorageLayoutEnv) -> Result<PathBuf, SlotProbeError> {
    if let Some(data_label) = layout.data_label.as_deref() {
        let data_path = PathBuf::from("/dev/disk/by-label").join(data_label);
        if data_path.exists() {
            let data_device = canonicalize_path(&data_path);
            return Ok(disk_from_partition(&data_device));
        }
    }

    if let Some(boot_label) = layout.boot_label.as_deref() {
        let boot_path = PathBuf::from("/dev/disk/by-label").join(boot_label);
        if boot_path.exists() {
            let boot_device = canonicalize_path(&boot_path);
            return Ok(disk_from_partition(&boot_device));
        }
    }

    let cmdline_root = read_cmdline_root()?;
    if let Some(root) = cmdline_root.as_deref() {
        if let Some(label) = root.strip_prefix("LABEL=") {
            let root_path = PathBuf::from("/dev/disk/by-label").join(label);
            if root_path.exists() {
                let root_device = canonicalize_path(&root_path);
                return Ok(disk_from_partition(&root_device));
            }
        } else if root == "/dev/helios-rootfs" {
            if let Some(boot_label) = layout.boot_label.as_deref() {
                let boot_path = PathBuf::from("/dev/disk/by-label").join(boot_label);
                if boot_path.exists() {
                    let boot_device = canonicalize_path(&boot_path);
                    return Ok(disk_from_partition(&boot_device));
                }
            }
            if let Some(data_label) = layout.data_label.as_deref() {
                let data_path = PathBuf::from("/dev/disk/by-label").join(data_label);
                if data_path.exists() {
                    let data_device = canonicalize_path(&data_path);
                    return Ok(disk_from_partition(&data_device));
                }
            }
            return Err(SlotProbeError::MissingBaseDisk);
        } else if root.starts_with("/dev/") {
            return Ok(disk_from_partition(Path::new(root)));
        }
    }

    Err(SlotProbeError::MissingBaseDisk)
}

fn slot_name(layout: &StorageLayoutEnv, slot: UpdateSlot) -> &str {
    match slot {
        UpdateSlot::A => &layout.slot_a_name,
        UpdateSlot::B => &layout.slot_b_name,
    }
}

fn slot_label(layout: &StorageLayoutEnv, slot: UpdateSlot) -> Option<&str> {
    match slot {
        UpdateSlot::A => layout.slot_a_label.as_deref().filter(|value| !value.is_empty()),
        UpdateSlot::B => layout.slot_b_label.as_deref().filter(|value| !value.is_empty()),
    }
}

fn slot_partition(layout: &StorageLayoutEnv, slot: UpdateSlot) -> u32 {
    match slot {
        UpdateSlot::A => layout.slot_a_partition,
        UpdateSlot::B => layout.slot_b_partition,
    }
}

fn canonicalize_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn disk_from_partition(path: &Path) -> PathBuf {
    let value = path.display().to_string();
    if let Some(base) = strip_partition_suffix(&value) { PathBuf::from(base) } else { path.to_path_buf() }
}

fn strip_partition_suffix(value: &str) -> Option<&str> {
    if let Some(index) = value.rfind('p') {
        let suffix = &value[index + 1..];
        if value.starts_with("/dev/mmcblk") || value.starts_with("/dev/nvme") {
            if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) {
                return Some(&value[..index]);
            }
        }
    }

    let digits_len = value.chars().rev().take_while(|c| c.is_ascii_digit()).count();
    if digits_len == 0 {
        return None;
    }
    let index = value.len() - digits_len;
    if value[..index].ends_with("/dev/") {
        return None;
    }
    Some(&value[..index])
}

fn partition_device(base_disk: &Path, partition: u32) -> PathBuf {
    let mut value = base_disk.display().to_string();
    let needs_p = value.chars().last().map(|c| c.is_ascii_digit()).unwrap_or(false);
    if needs_p {
        value.push('p');
    }
    value.push_str(&partition.to_string());
    PathBuf::from(value)
}

fn block_device_size_bytes(path: &Path) -> Result<u64, SlotProbeError> {
    let block_name = path.file_name().ok_or_else(|| SlotProbeError::MissingBlockSize { path: path.to_path_buf() })?.to_string_lossy().to_string();
    let size_path = PathBuf::from("/sys/class/block").join(&block_name).join("size");
    let sector_size_path = PathBuf::from("/sys/class/block").join(&block_name).join("queue/logical_block_size");

    let sectors = fs::read_to_string(&size_path).ok().and_then(|value| value.trim().parse::<u64>().ok()).ok_or_else(|| SlotProbeError::MissingBlockSize { path: path.to_path_buf() })?;
    let logical_block_size = fs::read_to_string(&sector_size_path).ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(512);
    Ok(sectors.saturating_mul(logical_block_size))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parses_generated_shell_env_values() {
        let env = parse_shell_env(
            r#"
            # comment
            HELIOS_LAYOUT_SLOT_A_NAME='ROOT_A'
            HELIOS_LAYOUT_SLOT_A_PARTITION='2'
            HELIOS_LAYOUT_SLOT_A_LABEL=''
            "#,
        );
        assert_eq!(env.get("HELIOS_LAYOUT_SLOT_A_NAME"), Some(&"ROOT_A".to_string()));
        assert_eq!(env.get("HELIOS_LAYOUT_SLOT_A_PARTITION"), Some(&"2".to_string()));
        assert_eq!(env.get("HELIOS_LAYOUT_SLOT_A_LABEL"), Some(&"".to_string()));
    }

    #[test]
    fn partition_device_handles_numbered_disks() {
        assert_eq!(partition_device(Path::new("/dev/mmcblk0"), 3), PathBuf::from("/dev/mmcblk0p3"));
        assert_eq!(partition_device(Path::new("/dev/nvme0n1"), 2), PathBuf::from("/dev/nvme0n1p2"));
        assert_eq!(partition_device(Path::new("/dev/sda"), 4), PathBuf::from("/dev/sda4"));
    }

    #[test]
    fn probe_prefers_ota_state_when_present() {
        let tempdir = TempDir::new().expect("tempdir");
        let env_path = tempdir.path().join("storage-layout.env");
        let active_path = tempdir.path().join("active");
        let reserve_path = tempdir.path().join("reserve");

        fs::write(
            &env_path,
            [
                "HELIOS_LAYOUT_DATA_LABEL='DATA_TEST_MISSING'",
                "HELIOS_LAYOUT_SLOT_SCHEME='squashfs_ab'",
                "HELIOS_LAYOUT_BOOT_LABEL='BOOT_TEST_MISSING'",
                "HELIOS_LAYOUT_BOOT_PARTITION='1'",
                "HELIOS_LAYOUT_SLOT_A_NAME='ROOT_A'",
                "HELIOS_LAYOUT_SLOT_A_PARTITION='2'",
                "HELIOS_LAYOUT_SLOT_A_LABEL=''",
                "HELIOS_LAYOUT_SLOT_B_NAME='ROOT_B'",
                "HELIOS_LAYOUT_SLOT_B_PARTITION='3'",
                "HELIOS_LAYOUT_SLOT_B_LABEL=''",
            ]
            .join("\n"),
        )
        .expect("env");
        fs::write(&active_path, "ROOT_B\n").expect("active");
        fs::write(&reserve_path, "ROOT_A\n").expect("reserve");

        let error = probe_slot_layout_with(SlotProbeConfig { storage_layout_env_path: env_path, ota_active_path: active_path, ota_reserve_path: reserve_path })
            .expect_err("missing real devices should fail after ota decode");

        assert!(matches!(error, SlotProbeError::MissingBaseDisk | SlotProbeError::MissingBlockSize { .. }));
    }
}
