use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use lib_storage_layout::{PartitionRole, SlotScheme as LayoutSlotScheme, StorageLayoutManifest};
use tokio::fs as tokio_fs;
use tokio::process::Command;

use crate::error::{Error, Result};

const LEGACY_SLOT_ACTIVE: &str = "ACTIVE";
const LEGACY_SLOT_RESERVE: &str = "RESERVE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotScheme {
    Ext4Labels,
    SquashfsAb,
}

impl From<LayoutSlotScheme> for SlotScheme {
    fn from(value: LayoutSlotScheme) -> Self {
        match value {
            LayoutSlotScheme::Ext4Labels => Self::Ext4Labels,
            LayoutSlotScheme::SquashfsAb => Self::SquashfsAb,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotSelection {
    pub current_slot: String,
    pub target_slot: String,
    pub target_device: String,
    pub single_slot: bool,
    pub scheme: SlotScheme,
}

async fn is_mountpoint(path: &str) -> bool {
    let data = match tokio_fs::read_to_string("/proc/self/mountinfo").await {
        Ok(data) => data,
        Err(_) => return false,
    };
    for line in data.lines() {
        let mut it = line.split_whitespace();
        let _id = it.next();
        let _parent = it.next();
        let _maj_min = it.next();
        let _root = it.next();
        let mnt = it.next();
        if let Some(mnt) = mnt
            && mnt == path
        {
            return true;
        }
    }
    false
}

pub fn by_label_path(label: &str) -> Option<String> {
    let p = Path::new("/dev/disk/by-label").join(label);
    fs::canonicalize(&p).ok().map(|p| p.to_string_lossy().into())
}

fn canonicalize_existing_path(path: &str) -> Option<String> {
    let trimmed = path.trim().trim_matches('"').trim_matches('\'');
    if trimmed.is_empty() {
        return None;
    }
    let as_path = Path::new(trimmed);
    if let Ok(canon) = fs::canonicalize(as_path) {
        return Some(canon.to_string_lossy().into_owned());
    }
    if as_path.exists() {
        return Some(trimmed.to_string());
    }
    None
}

fn boot_partition_from_config() -> Option<String> {
    let raw = fs::read_to_string("/etc/helios/bootloader.conf").ok()?;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() == "boot_partition" {
            return canonicalize_existing_path(value);
        }
    }
    None
}

fn load_layout_manifest() -> Option<StorageLayoutManifest> {
    StorageLayoutManifest::load_system().ok().or_else(detect_legacy_layout_manifest)
}

fn detect_legacy_layout_manifest() -> Option<StorageLayoutManifest> {
    if by_label_path(LEGACY_SLOT_ACTIVE).is_some() || by_label_path(LEGACY_SLOT_RESERVE).is_some() {
        return toml::from_str::<StorageLayoutManifest>(
            r#"
schema_version = 1
layout_id = "legacy_ext4_labels"
slot_scheme = "ext4_labels"
boot_label = "BOOT"

[spans]
boot_partition = 1
data_size_mib = 4096

[[partitions]]
role = "slot_a"
name = "ACTIVE"
number = 2
label = "ACTIVE"
mode = "resize"

[[partitions]]
role = "slot_b"
name = "RESERVE"
number = 3
label = "RESERVE"
mode = "mkpart"

[[partitions]]
role = "data"
name = "DATA"
number = 4
label = "DATA"
mode = "mkpart"
"#,
        )
        .ok();
    }

    if legacy_squashfs_slot_devices().is_some() {
        return toml::from_str::<StorageLayoutManifest>(
            r#"
schema_version = 1
layout_id = "legacy_squashfs_ab"
slot_scheme = "squashfs_ab"
boot_label = "BOOT"

[spans]
boot_partition = 1
start_after_partition = 1
data_size_mib = 0

[[partitions]]
role = "slot_a"
name = "ROOT_A"
number = 2
mode = "noop"

[[partitions]]
role = "slot_b"
name = "ROOT_B"
number = 3
mode = "mkpart"

[[partitions]]
role = "data"
name = "DATA"
number = 4
label = "DATA"
mode = "mkpart"
"#,
        )
        .ok();
    }

    None
}

fn base_disk_candidates(layout: &StorageLayoutManifest) -> Vec<String> {
    let mut candidates = Vec::<String>::new();

    if let Ok(data_partition) = layout.data()
        && let Some(label) = &data_partition.label
        && let Some(dev) = by_label_path(label)
        && let Some(base) = StorageLayoutManifest::disk_from_partition_device(&dev)
    {
        candidates.push(base);
    }

    if let Some(dev) = boot_partition_from_config()
        && let Some(base) = StorageLayoutManifest::disk_from_partition_device(&dev)
    {
        candidates.push(base);
    }

    if let Ok(dev) = read_mount_root_device()
        && let Some(base) = StorageLayoutManifest::disk_from_partition_device(&dev)
    {
        candidates.push(base);
    }

    if let Some(root) = read_cmdline_root()
        && root.starts_with("/dev/")
        && let Some(path) = canonicalize_existing_path(&root)
        && let Some(base) = StorageLayoutManifest::disk_from_partition_device(&path)
    {
        candidates.push(base);
    }

    let mut dedup = HashSet::<String>::new();
    candidates.retain(|candidate| dedup.insert(candidate.clone()));
    candidates
}

fn boot_device_candidates_with_layout(layout: &StorageLayoutManifest) -> Vec<String> {
    let mut candidates = Vec::<String>::new();
    if let Some(label) = layout.boot_label.as_deref()
        && let Some(dev) = by_label_path(label)
    {
        candidates.push(dev);
    }
    if let Some(dev) = boot_partition_from_config() {
        candidates.push(dev);
    }
    if let Some(boot_partition) = layout.boot_partition_number() {
        for base in base_disk_candidates(layout) {
            if let Some(canon) = canonicalize_existing_path(&StorageLayoutManifest::partition_device_for_disk(&base, boot_partition)) {
                candidates.push(canon);
            }
        }
    }
    for dev in ["/dev/mmcblk0p1", "/dev/mmcblk1p1", "/dev/sda1", "/dev/sdb1", "/dev/nvme0n1p1"] {
        if let Some(canon) = canonicalize_existing_path(dev) {
            candidates.push(canon);
        }
    }

    let mut dedup = HashSet::<String>::new();
    candidates.retain(|dev| dedup.insert(dev.clone()));
    candidates
}

fn resolve_boot_block_device_with_layout(layout: &StorageLayoutManifest) -> Option<String> {
    boot_device_candidates_with_layout(layout).into_iter().find(|dev| Path::new(dev).exists())
}

pub fn resolve_boot_block_device() -> Option<String> {
    if let Some(layout) = load_layout_manifest() {
        return resolve_boot_block_device_with_layout(&layout);
    }
    None
}

fn read_cmdline_root() -> Option<String> {
    let data = fs::read_to_string("/proc/cmdline").ok()?;
    for token in data.split_whitespace() {
        if let Some(root) = token.strip_prefix("root=") {
            return Some(root.to_string());
        }
    }
    None
}

fn read_root_maj_min() -> Option<String> {
    let data = fs::read_to_string("/proc/self/mountinfo").ok()?;
    for line in data.lines() {
        let mut it = line.split_whitespace();
        let _id = it.next();
        let _parent = it.next();
        let maj_min = it.next();
        let _root = it.next();
        let mnt = it.next();
        if let (Some(mm), Some("/")) = (maj_min, mnt) {
            return Some(mm.to_string());
        }
    }
    None
}

fn read_partuuid_for_device(dev: &str) -> Option<String> {
    let real = canonicalize_existing_path(dev)?;
    let name = Path::new(&real).file_name()?.to_string_lossy().into_owned();
    let sys = Path::new("/sys/class/block").join(name).join("uevent");
    let raw = fs::read_to_string(sys).ok()?;
    for line in raw.lines() {
        if let Some(value) = line.strip_prefix("PARTUUID=") {
            return Some(value.trim().to_string());
        }
    }
    None
}

fn legacy_squashfs_slot_devices() -> Option<(String, String)> {
    let base = by_label_path("DATA")
        .and_then(|dev| StorageLayoutManifest::disk_from_partition_device(&dev))
        .or_else(|| boot_partition_from_config().and_then(|dev| StorageLayoutManifest::disk_from_partition_device(&dev)))?;
    let slot_a = StorageLayoutManifest::partition_device_for_disk(&base, 2);
    let slot_b = StorageLayoutManifest::partition_device_for_disk(&base, 3);
    let data = StorageLayoutManifest::partition_device_for_disk(&base, 4);
    if Path::new(&slot_a).exists() && Path::new(&slot_b).exists() && (Path::new(&data).exists() || by_label_path("DATA").is_some()) { Some((slot_a, slot_b)) } else { None }
}

fn squashfs_slot_devices(layout: &StorageLayoutManifest) -> Option<(String, String)> {
    let base = layout
        .data()
        .ok()
        .and_then(|partition| partition.label.as_deref())
        .and_then(by_label_path)
        .and_then(|dev| StorageLayoutManifest::disk_from_partition_device(&dev))
        .or_else(|| resolve_boot_block_device_with_layout(layout).and_then(|dev| StorageLayoutManifest::disk_from_partition_device(&dev)))
        .or_else(|| read_mount_root_device().ok().and_then(|dev| StorageLayoutManifest::disk_from_partition_device(&dev)))?;

    let slot_a = layout.slot_device_for_disk(PartitionRole::SlotA, &base).ok()?;
    let slot_b = layout.slot_device_for_disk(PartitionRole::SlotB, &base).ok()?;
    let data_partition = layout.data().ok()?;
    let data = StorageLayoutManifest::partition_device_for_disk(&base, data_partition.number);
    if Path::new(&slot_a).exists() && Path::new(&slot_b).exists() && (Path::new(&data).exists() || data_partition.label.as_deref().and_then(by_label_path).is_some()) {
        Some((slot_a, slot_b))
    } else {
        None
    }
}

fn root_matches_device_or_partuuid(root: &str, dev: &str) -> bool {
    if let Some(partuuid) = root.strip_prefix("PARTUUID=") {
        return read_partuuid_for_device(dev).map(|current| current.eq_ignore_ascii_case(partuuid)).unwrap_or(false);
    }
    if let Some(path) = canonicalize_existing_path(root)
        && let Some(dev_path) = canonicalize_existing_path(dev)
    {
        return path == dev_path;
    }
    false
}

fn ext4_slot_pair(layout: &StorageLayoutManifest) -> Option<(String, String, String, String)> {
    let slot_a = layout.slot_a().ok()?;
    let slot_b = layout.slot_b().ok()?;
    let slot_a_name = slot_a.name.clone();
    let slot_b_name = slot_b.name.clone();
    let slot_a_dev = slot_a.label.as_deref().and_then(by_label_path)?;
    let slot_b_dev = slot_b.label.as_deref().and_then(by_label_path)?;
    Some((slot_a_name, slot_a_dev, slot_b_name, slot_b_dev))
}

pub fn select_target_slot(mut single_slot: bool) -> Result<SlotSelection> {
    let layout = load_layout_manifest().ok_or_else(|| Error::InvalidState("unable to load a system storage layout manifest".into()))?;
    let current_slot = current_slot_label_with_layout(&layout).ok_or_else(|| Error::InvalidState("unable to determine current slot".into()))?;

    match layout.slot_scheme {
        LayoutSlotScheme::Ext4Labels => {
            let Some((slot_a_name, slot_a_dev, slot_b_name, slot_b_dev)) = ext4_slot_pair(&layout) else {
                return Err(Error::InvalidState("expected both labeled OTA slots from the layout manifest".into()));
            };
            let (target_slot, target_device) = if single_slot {
                if current_slot == slot_a_name { (slot_a_name, slot_a_dev) } else { (slot_b_name, slot_b_dev) }
            } else if current_slot == slot_a_name {
                (slot_b_name, slot_b_dev)
            } else {
                (slot_a_name, slot_a_dev)
            };
            Ok(SlotSelection { current_slot, target_slot, target_device, single_slot, scheme: SlotScheme::Ext4Labels })
        }
        LayoutSlotScheme::SquashfsAb => {
            single_slot = false;
            let Some((slot_a_dev, slot_b_dev)) = squashfs_slot_devices(&layout) else {
                return Err(Error::InvalidState("unable to resolve squashfs slot devices from the layout manifest".into()));
            };
            let slot_a_name = layout.slot_name(PartitionRole::SlotA).map_err(|err| Error::InvalidState(err.to_string()))?.to_string();
            let slot_b_name = layout.slot_name(PartitionRole::SlotB).map_err(|err| Error::InvalidState(err.to_string()))?.to_string();
            let (target_slot, target_device) = if current_slot == slot_a_name { (slot_b_name, slot_b_dev) } else { (slot_a_name, slot_a_dev) };
            Ok(SlotSelection { current_slot, target_slot, target_device, single_slot, scheme: SlotScheme::SquashfsAb })
        }
    }
}

pub fn read_mount_root_device() -> Result<String> {
    let data = fs::read_to_string("/proc/mounts").map_err(Error::Io)?;
    for line in data.lines() {
        let mut it = line.split_whitespace();
        if let (Some(dev), Some(mnt)) = (it.next(), it.next())
            && mnt == "/"
        {
            return Ok(dev.to_string());
        }
    }
    Err(Error::Io(io::Error::other("root mount not found")))
}

pub fn current_slot_label() -> Option<String> {
    let layout = load_layout_manifest()?;
    current_slot_label_with_layout(&layout)
}

fn current_slot_label_with_layout(layout: &StorageLayoutManifest) -> Option<String> {
    match layout.slot_scheme {
        LayoutSlotScheme::Ext4Labels => current_ext4_slot_label(layout),
        LayoutSlotScheme::SquashfsAb => current_squashfs_slot_label(layout),
    }
}

fn current_ext4_slot_label(layout: &StorageLayoutManifest) -> Option<String> {
    let slot_a_name = layout.slot_name(PartitionRole::SlotA).ok()?.to_string();
    let slot_b_name = layout.slot_name(PartitionRole::SlotB).ok()?.to_string();

    fn label_for_dev(dev_path: &str, slot_a_name: &str, slot_b_name: &str) -> Option<String> {
        let root_canon = fs::canonicalize(dev_path).ok()?;
        if let Ok(rd) = fs::read_dir("/dev/disk/by-label") {
            for entry in rd.flatten() {
                let name_owned = entry.file_name().to_string_lossy().into_owned();
                if (name_owned == slot_a_name || name_owned == slot_b_name)
                    && let Ok(canon) = fs::canonicalize(entry.path())
                    && canon == root_canon
                {
                    return Some(name_owned);
                }
            }
        }
        None
    }

    fn dev_maj_min_from_label(label: &str) -> Option<String> {
        let p = Path::new("/dev/disk/by-label").join(label);
        let real = fs::canonicalize(p).ok()?;
        let name = real.file_name()?.to_string_lossy().into_owned();
        let sys_path = Path::new("/sys/class/block").join(name).join("dev");
        fs::read_to_string(sys_path).ok().map(|s| s.trim().to_string())
    }

    if let Some(root_mm) = read_root_maj_min() {
        for label in [&slot_a_name, &slot_b_name] {
            if let Some(dev_mm) = dev_maj_min_from_label(label)
                && dev_mm == root_mm
            {
                return Some(label.clone());
            }
        }
    }

    if let Ok(root) = read_mount_root_device()
        && let Some(label) = label_for_dev(&root, &slot_a_name, &slot_b_name)
    {
        return Some(label);
    }

    if let Some(root) = read_cmdline_root() {
        if let Some(label) = root.strip_prefix("LABEL=")
            && (label == slot_a_name || label == slot_b_name)
        {
            return Some(label.to_string());
        }
        if let Some(puuid) = root.strip_prefix("PARTUUID=")
            && let Ok(entry_path) = fs::canonicalize(Path::new("/dev/disk/by-partuuid").join(puuid))
            && let Some(label) = label_for_dev(entry_path.to_string_lossy().as_ref(), &slot_a_name, &slot_b_name)
        {
            return Some(label);
        }
        if root.starts_with("/dev/")
            && let Some(label) = label_for_dev(&root, &slot_a_name, &slot_b_name)
        {
            return Some(label);
        }
    }

    if by_label_path(&slot_a_name).is_some() && by_label_path(&slot_b_name).is_none() {
        return Some(slot_a_name);
    }
    if by_label_path(&slot_b_name).is_some() && by_label_path(&slot_a_name).is_none() {
        return Some(slot_b_name);
    }

    None
}

fn current_squashfs_slot_label(layout: &StorageLayoutManifest) -> Option<String> {
    let slot_a_name = layout.slot_name(PartitionRole::SlotA).ok()?.to_string();
    let slot_b_name = layout.slot_name(PartitionRole::SlotB).ok()?.to_string();

    if let Some(root) = read_cmdline_root() {
        if let Some((slot_a_dev, slot_b_dev)) = squashfs_slot_devices(layout) {
            if root_matches_device_or_partuuid(&root, &slot_a_dev) {
                return Some(slot_a_name.clone());
            }
            if root_matches_device_or_partuuid(&root, &slot_b_dev) {
                return Some(slot_b_name.clone());
            }
        }
        if root == "/dev/helios-rootfs" {
            return read_active_marker(layout).or_else(|| Some(slot_a_name.clone()));
        }
    }

    if let Ok(root) = read_mount_root_device()
        && let Some((slot_a_dev, slot_b_dev)) = squashfs_slot_devices(layout)
    {
        if root_matches_device_or_partuuid(&root, &slot_a_dev) {
            return Some(slot_a_name);
        }
        if root_matches_device_or_partuuid(&root, &slot_b_dev) {
            return Some(slot_b_name);
        }
    }

    read_active_marker(layout)
}

pub async fn rewrite_cmdline_root(content: &str, _target_slot: &str, target_dev: &str) -> String {
    let mut tokens: Vec<String> = content.replace('\n', " ").split_whitespace().map(|s| s.to_string()).collect();
    let new_root_val = target_dev.trim().to_string();
    tokens.retain(|t| !t.starts_with("root="));
    tokens.insert(0, format!("root={}", new_root_val));
    let mut out = tokens.join(" ");
    out.push('\n');
    out
}

pub async fn detect_boot_device() -> Result<Option<(PathBuf, bool)>> {
    if is_mountpoint("/mnt/boot").await {
        return Ok(Some((PathBuf::from("/mnt/boot"), false)));
    }
    let boot_mnt = Path::new("/mnt/boot");
    tokio_fs::create_dir_all(boot_mnt).await.map_err(Error::Io)?;
    let candidates = load_layout_manifest().map(|layout| boot_device_candidates_with_layout(&layout)).unwrap_or_default();
    for dev in candidates {
        let status = Command::new("mount").args(["-o", "rw"]).arg(&dev).arg(boot_mnt.to_str().unwrap()).status().await.map_err(Error::Io)?;
        if status.success() {
            return Ok(Some((boot_mnt.to_path_buf(), true)));
        }
    }
    Ok(None)
}

pub async fn resolve_boot_dir_rw() -> Result<Option<(PathBuf, bool)>> {
    if is_mountpoint("/boot").await {
        return Ok(Some((PathBuf::from("/boot"), false)));
    }
    detect_boot_device().await
}

fn read_active_marker(layout: &StorageLayoutManifest) -> Option<String> {
    let slot_a_name = layout.slot_name(PartitionRole::SlotA).ok()?;
    let slot_b_name = layout.slot_name(PartitionRole::SlotB).ok()?;
    for candidate in ["/boot/helios/ota/active", "/mnt/boot/helios/ota/active"] {
        if let Ok(contents) = fs::read_to_string(candidate) {
            let trimmed = contents.trim();
            if trimmed == slot_a_name || trimmed == slot_b_name {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}
