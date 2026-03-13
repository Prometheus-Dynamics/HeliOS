use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tokio::fs as tokio_fs;
use tokio::process::Command;

use crate::error::{Error, Result};

const SLOT_ACTIVE: &str = "ACTIVE";
const SLOT_RESERVE: &str = "RESERVE";
const SLOT_ROOT_A: &str = "ROOT_A";
const SLOT_ROOT_B: &str = "ROOT_B";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotScheme {
    Ext4Labels,
    SquashfsAb,
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

fn boot_device_candidates() -> Vec<String> {
    let mut candidates = Vec::<String>::new();
    if let Some(dev) = by_label_path("BOOT") {
        candidates.push(dev);
    }
    if let Some(dev) = boot_partition_from_config() {
        candidates.push(dev);
    }
    for dev in ["/dev/mmcblk0p1", "/dev/mmcblk1p1", "/dev/sda1"] {
        if let Some(canon) = canonicalize_existing_path(dev) {
            candidates.push(canon);
        }
    }

    let mut dedup = HashSet::<String>::new();
    candidates.retain(|dev| dedup.insert(dev.clone()));
    candidates
}

pub fn resolve_boot_block_device() -> Option<String> {
    boot_device_candidates().into_iter().find(|dev| Path::new(dev).exists())
}

fn disk_from_partition_device(dev: &str) -> Option<String> {
    let canonical = canonicalize_existing_path(dev)?;
    match canonical.as_str() {
        path if path.starts_with("/dev/mmcblk") || path.starts_with("/dev/nvme") => {
            let (base, suffix) = path.rsplit_once('p')?;
            if suffix.chars().all(|ch| ch.is_ascii_digit()) { Some(base.to_string()) } else { None }
        }
        path if path.starts_with("/dev/sd") => Some(path.trim_end_matches(char::is_numeric).to_string()),
        _ => None,
    }
}

fn partition_device(disk: &str, part: u32) -> String {
    if disk.starts_with("/dev/mmcblk") || disk.starts_with("/dev/nvme") { format!("{disk}p{part}") } else { format!("{disk}{part}") }
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

fn squashfs_slot_devices() -> Option<(String, String)> {
    let base = by_label_path("DATA").and_then(|dev| disk_from_partition_device(&dev)).or_else(|| resolve_boot_block_device().and_then(|dev| disk_from_partition_device(&dev)))?;
    let slot_a = partition_device(&base, 2);
    let slot_b = partition_device(&base, 3);
    let data = partition_device(&base, 4);
    if Path::new(&slot_a).exists() && Path::new(&slot_b).exists() && (Path::new(&data).exists() || by_label_path("DATA").is_some()) { Some((slot_a, slot_b)) } else { None }
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

pub fn slot_pair() -> Option<(&'static str, &'static str)> {
    if by_label_path(SLOT_ACTIVE).is_some() && by_label_path(SLOT_RESERVE).is_some() { Some((SLOT_ACTIVE, SLOT_RESERVE)) } else { None }
}

pub fn select_target_slot(mut single_slot: bool) -> Result<SlotSelection> {
    let current_slot = current_slot_label().ok_or_else(|| Error::InvalidState("unable to determine current slot".into()))?;
    if let Some((slot_a, slot_b)) = slot_pair() {
        let (slot_a_dev, slot_b_dev) =
            (by_label_path(slot_a).ok_or_else(|| Error::InvalidState(format!("{slot_a} not found")))?, by_label_path(slot_b).ok_or_else(|| Error::InvalidState(format!("{slot_b} not found")))?);
        let (target_slot, target_device) = if single_slot {
            if current_slot == slot_a { (slot_a, slot_a_dev) } else { (slot_b, slot_b_dev) }
        } else if current_slot == slot_a {
            (slot_b, slot_b_dev)
        } else {
            (slot_a, slot_a_dev)
        };
        return Ok(SlotSelection { current_slot, target_slot: target_slot.to_string(), target_device, single_slot, scheme: SlotScheme::Ext4Labels });
    }

    if let Some((slot_a_dev, slot_b_dev)) = squashfs_slot_devices() {
        single_slot = false;
        let (target_slot, target_device) = match current_slot.as_str() {
            SLOT_ROOT_A => (SLOT_ROOT_B, slot_b_dev),
            SLOT_ROOT_B => (SLOT_ROOT_A, slot_a_dev),
            other => return Err(Error::InvalidState(format!("unsupported squashfs slot {other}"))),
        };
        return Ok(SlotSelection { current_slot, target_slot: target_slot.to_string(), target_device, single_slot, scheme: SlotScheme::SquashfsAb });
    }

    single_slot = true;
    let fallback = if by_label_path(SLOT_ACTIVE).is_some() {
        SLOT_ACTIVE
    } else if by_label_path(SLOT_RESERVE).is_some() {
        SLOT_RESERVE
    } else {
        return Err(Error::InvalidState("no OTA slot found (expected ACTIVE/RESERVE labels or squashfs ROOT_A/ROOT_B layout)".into()));
    };
    let target_device = by_label_path(fallback).ok_or_else(|| Error::InvalidState(format!("{fallback} not found")))?;
    Ok(SlotSelection { current_slot, target_slot: fallback.to_string(), target_device, single_slot, scheme: SlotScheme::Ext4Labels })
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
    fn label_for_dev(dev_path: &str) -> Option<String> {
        let root_canon = fs::canonicalize(dev_path).ok()?;
        if let Ok(rd) = fs::read_dir("/dev/disk/by-label") {
            for entry in rd.flatten() {
                let name_owned = entry.file_name().to_string_lossy().into_owned();
                if (name_owned == SLOT_ACTIVE || name_owned == SLOT_RESERVE)
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
        for label in [SLOT_ACTIVE, SLOT_RESERVE] {
            if let Some(dev_mm) = dev_maj_min_from_label(label)
                && dev_mm == root_mm
            {
                return Some(label.to_string());
            }
        }
    }

    if let Ok(root) = read_mount_root_device()
        && let Some(label) = label_for_dev(&root)
    {
        return Some(label);
    }

    if let Some(root) = read_cmdline_root() {
        if let Some(label) = root.strip_prefix("LABEL=")
            && (label == SLOT_ACTIVE || label == SLOT_RESERVE)
        {
            return Some(label.to_string());
        }
        if let Some(puuid) = root.strip_prefix("PARTUUID=")
            && let Ok(entry_path) = fs::canonicalize(Path::new("/dev/disk/by-partuuid").join(puuid))
            && let Some(label) = label_for_dev(entry_path.to_string_lossy().as_ref())
        {
            return Some(label);
        }
        if root.starts_with("/dev/")
            && let Some(label) = label_for_dev(&root)
        {
            return Some(label);
        }
        if let Some((slot_a_dev, slot_b_dev)) = squashfs_slot_devices() {
            if root_matches_device_or_partuuid(&root, &slot_a_dev) {
                return Some(SLOT_ROOT_A.to_string());
            }
            if root_matches_device_or_partuuid(&root, &slot_b_dev) {
                return Some(SLOT_ROOT_B.to_string());
            }
        }
        if root == "/dev/helios-rootfs" && squashfs_slot_devices().is_some() {
            return read_active_marker().or_else(|| Some(SLOT_ROOT_A.to_string()));
        }
    }

    if let Some(label) = read_active_marker() {
        return Some(label);
    }

    if by_label_path(SLOT_ACTIVE).is_some() && by_label_path(SLOT_RESERVE).is_none() {
        return Some(SLOT_ACTIVE.to_string());
    }
    if by_label_path(SLOT_RESERVE).is_some() && by_label_path(SLOT_ACTIVE).is_none() {
        return Some(SLOT_RESERVE.to_string());
    }

    None
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
    for dev in boot_device_candidates() {
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

fn read_active_marker() -> Option<String> {
    for candidate in ["/boot/helios/ota/active", "/mnt/boot/helios/ota/active"] {
        if let Ok(contents) = fs::read_to_string(candidate) {
            let trimmed = contents.trim();
            if matches!(trimmed, SLOT_ACTIVE | SLOT_RESERVE | SLOT_ROOT_A | SLOT_ROOT_B) {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}
