use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tokio::fs as tokio_fs;
use tokio::process::Command;

use crate::error::{Error, Result};

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

pub fn slot_pair() -> Option<(&'static str, &'static str)> {
    if by_label_path("ACTIVE").is_some() && by_label_path("RESERVE").is_some() { Some(("ACTIVE", "RESERVE")) } else { None }
}

pub fn select_target_slot(mut single_slot: bool) -> Result<(String, String, bool)> {
    let current_slot = current_slot_label().ok_or_else(|| Error::InvalidState("unable to determine current slot".into()))?;
    let (slot_a, slot_b) = if let Some(pair) = slot_pair() {
        pair
    } else {
        single_slot = true;
        let fallback = if by_label_path("ACTIVE").is_some() {
            "ACTIVE"
        } else if by_label_path("RESERVE").is_some() {
            "RESERVE"
        } else {
            return Err(Error::InvalidState("no slot label found (expected ACTIVE or RESERVE)".into()));
        };
        (fallback, fallback)
    };
    let dev_a = by_label_path(slot_a).ok_or_else(|| Error::InvalidState(format!("{slot_a} not found")))?;
    let dev_b = by_label_path(slot_b).ok_or_else(|| Error::InvalidState(format!("{slot_b} not found")))?;
    let (target_label, target_device) = if single_slot {
        if current_slot == slot_a { (slot_a, dev_a) } else { (slot_b, dev_b) }
    } else if current_slot == slot_a {
        (slot_b, dev_b)
    } else {
        (slot_a, dev_a)
    };
    Ok((target_label.to_string(), target_device, single_slot))
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
                if (name_owned == "ACTIVE" || name_owned == "RESERVE")
                    && let Ok(canon) = fs::canonicalize(entry.path())
                    && canon == root_canon
                {
                    return Some(name_owned);
                }
            }
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

    fn dev_maj_min_from_label(label: &str) -> Option<String> {
        let p = Path::new("/dev/disk/by-label").join(label);
        let real = fs::canonicalize(p).ok()?;
        let name = real.file_name()?.to_string_lossy().into_owned();
        let sys_path = Path::new("/sys/class/block").join(name).join("dev");
        fs::read_to_string(sys_path).ok().map(|s| s.trim().to_string())
    }

    if let Some(root_mm) = read_root_maj_min() {
        for label in ["ACTIVE", "RESERVE"] {
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
            && (label == "ACTIVE" || label == "RESERVE")
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
    }

    if let Some(label) = read_active_marker() {
        return Some(label);
    }

    if by_label_path("ACTIVE").is_some() && by_label_path("RESERVE").is_none() {
        return Some("ACTIVE".to_string());
    }
    if by_label_path("RESERVE").is_some() && by_label_path("ACTIVE").is_none() {
        return Some("RESERVE".to_string());
    }

    None
}

pub async fn rewrite_cmdline_root(content: &str, _target_label: &str, target_dev: &str) -> String {
    let mut tokens: Vec<String> = content.replace('\n', " ").split_whitespace().map(|s| s.to_string()).collect();
    // The kernel early root resolver accepts /dev paths and PARTUUID/PARTLABEL,
    // but not filesystem LABEL= values without an initramfs.
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
            if trimmed == "ACTIVE" || trimmed == "RESERVE" {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}
