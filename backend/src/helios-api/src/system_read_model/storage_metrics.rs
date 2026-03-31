use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use nix::sys::statvfs::statvfs;
use sysinfo::Disks;

use crate::http::device::metrics::DiskMetrics;
use crate::ws::device::DiskPartitionTelemetry;

pub(super) fn collect_disk_partitions(disks: &Disks) -> Vec<DiskPartitionTelemetry> {
    let mounts = read_mountinfo_entries();
    let overlay_backing_key = overlay_backing_mount_key(&mounts);
    let mut grouped = BTreeMap::<String, DiskPartitionTelemetry>::new();
    let mut selection_keys = BTreeMap::<String, MountSelectionKey>::new();

    for disk in disks.iter() {
        let mount_point = disk.mount_point();
        let mount_entry = mounts.iter().find(|entry| entry.mount_point == mount_point);
        let (effective_entry, effective_mount_point) = effective_disk_mount_context(&mounts, mount_entry, mount_point, overlay_backing_key.as_deref());
        let key = effective_entry.map(disk_group_key).unwrap_or_else(|| effective_mount_point.to_string_lossy().to_string());

        let (total, used, free) =
            filesystem_usage_for_path(&effective_mount_point).unwrap_or_else(|| (disk.total_space(), disk.total_space().saturating_sub(disk.available_space()), disk.available_space()));
        let selection_key = mount_selection_key(effective_entry, &effective_mount_point);
        let telemetry = DiskPartitionTelemetry { mount: effective_mount_point.to_string_lossy().to_string(), total_bytes: total, used_bytes: used, free_bytes: free };

        if selection_keys.get(&key).is_none_or(|current| selection_key < *current) {
            selection_keys.insert(key.clone(), selection_key);
            grouped.insert(key, telemetry);
        }
    }

    grouped.into_values().collect()
}

pub(super) fn select_disk_for_path<'a>(disks: &'a Disks, path: &Path) -> Option<&'a sysinfo::Disk> {
    let mut best: Option<&sysinfo::Disk> = None;
    let mut best_len = 0usize;
    for disk in disks.iter() {
        let mount = disk.mount_point();
        if path.starts_with(mount) {
            let len = mount.as_os_str().len();
            if len >= best_len {
                best = Some(disk);
                best_len = len;
            }
        }
    }
    best
}

pub(super) fn collect_disk_metrics(disks: &Disks) -> Vec<DiskMetrics> {
    collect_disk_partitions(disks).into_iter().map(|partition| DiskMetrics { mount: partition.mount, total_bytes: partition.total_bytes, available_bytes: partition.free_bytes }).collect()
}

pub(super) fn filesystem_usage_for_path(path: &Path) -> Option<(u64, u64, u64)> {
    let stats = statvfs(path).ok()?;
    let fragment = stats.fragment_size();
    let total = stats.blocks().saturating_mul(fragment);
    // `blocks_free` includes reserved ext4 blocks, which should not count as user-visible
    // "used" space on the dashboard. Keep `free` as unprivileged-available bytes while
    // deriving used bytes from actual occupied blocks.
    let free = stats.blocks_available().saturating_mul(fragment);
    let used = total.saturating_sub(stats.blocks_free().saturating_mul(fragment));
    Some((total, used, free))
}

#[derive(Debug, Clone)]
struct MountinfoEntry {
    root: PathBuf,
    mount_point: PathBuf,
    major_minor: String,
    fs_type: String,
    source: String,
    super_options: Vec<String>,
}

fn read_mountinfo_entries() -> Vec<MountinfoEntry> {
    let Ok(raw) = fs::read_to_string("/proc/self/mountinfo") else {
        return Vec::new();
    };

    raw.lines()
        .filter_map(|line| {
            let (pre, post) = line.split_once(" - ")?;
            let pre_fields: Vec<&str> = pre.split_whitespace().collect();
            let post_fields: Vec<&str> = post.split_whitespace().collect();
            let root = decode_mountinfo_field(pre_fields.get(3)?);
            let mount_point = decode_mountinfo_field(pre_fields.get(4)?);
            let major_minor = (*pre_fields.get(2)?).to_string();
            let fs_type = (*post_fields.first()?).to_string();
            let source = post_fields.get(1).copied().unwrap_or_default().to_string();
            let super_options = post_fields.get(2).copied().unwrap_or_default().split(',').map(str::to_string).collect();
            Some(MountinfoEntry { root, mount_point, major_minor, fs_type, source, super_options })
        })
        .collect()
}

fn decode_mountinfo_field(raw: &str) -> PathBuf {
    let mut out = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx] == b'\\' && idx + 3 < bytes.len() {
            let octal = &raw[idx + 1..idx + 4];
            if let Ok(value) = u8::from_str_radix(octal, 8) {
                out.push(value as char);
                idx += 4;
                continue;
            }
        }
        out.push(bytes[idx] as char);
        idx += 1;
    }
    PathBuf::from(out)
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct MountSelectionKey {
    filesystem_kind_rank: usize,
    root_rank: usize,
    depth: usize,
    mount_len: usize,
    mount: String,
}

fn overlay_backing_mount_key(mounts: &[MountinfoEntry]) -> Option<String> {
    let overlay_root = mounts.iter().find(|entry| entry.mount_point == Path::new("/") && entry.fs_type == "overlay")?;

    for prefix in ["upperdir=", "workdir="] {
        let Some(candidate_path) = overlay_mount_option_path(overlay_root, prefix) else {
            continue;
        };
        let Some(backing_mount) = mountinfo_entry_for_path(mounts, &candidate_path) else {
            continue;
        };
        if backing_mount.mount_point == Path::new("/") {
            continue;
        }
        return Some(disk_group_key(backing_mount));
    }

    infer_visible_overlay_backing_mount_key(mounts)
}

fn infer_visible_overlay_backing_mount_key(mounts: &[MountinfoEntry]) -> Option<String> {
    let mut counts = BTreeMap::<String, (u64, MountSelectionKey)>::new();

    for entry in mounts.iter().filter(|entry| is_probable_block_filesystem_mount(entry)) {
        let key = disk_group_key(entry);
        let selection = mount_selection_key(Some(entry), &entry.mount_point);
        let record = counts.entry(key).or_insert((0, selection.clone()));
        record.0 = record.0.saturating_add(1);
        if selection < record.1 {
            record.1 = selection;
        }
    }

    counts
        .into_iter()
        .max_by(|(left_key, (left_count, left_selection)), (right_key, (right_count, right_selection))| {
            left_count.cmp(right_count).then_with(|| right_selection.cmp(left_selection)).then_with(|| right_key.cmp(left_key))
        })
        .map(|(key, _)| key)
}

fn is_probable_block_filesystem_mount(entry: &MountinfoEntry) -> bool {
    if entry.mount_point == Path::new("/") || entry.fs_type == "overlay" {
        return false;
    }

    entry.source.starts_with("/dev/")
}

fn effective_disk_mount_context<'a>(
    mounts: &'a [MountinfoEntry],
    mount_entry: Option<&'a MountinfoEntry>,
    mount_point: &Path,
    overlay_backing_key: Option<&str>,
) -> (Option<&'a MountinfoEntry>, PathBuf) {
    if mount_point == Path::new("/")
        && let Some(entry) = mount_entry
        && entry.fs_type == "overlay"
        && let Some(backing_key) = overlay_backing_key
        && let Some(backing_entry) = mounts.iter().find(|candidate| disk_group_key(candidate) == backing_key)
    {
        return (Some(backing_entry), backing_entry.mount_point.clone());
    }

    (mount_entry, mount_point.to_path_buf())
}

fn disk_group_key(entry: &MountinfoEntry) -> String {
    format!("{}:{}:{}", entry.major_minor, entry.source, entry.fs_type)
}

fn mount_selection_key(entry: Option<&MountinfoEntry>, mount_point: &Path) -> MountSelectionKey {
    let filesystem_kind_rank = match entry.map(|entry| entry.fs_type.as_str()) {
        Some("overlay") => 2,
        Some(_) => 0,
        None => 1,
    };
    let root_rank = match entry {
        Some(entry) if entry.root == Path::new("/") => 0,
        Some(_) => 1,
        None => 2,
    };
    MountSelectionKey { filesystem_kind_rank, root_rank, depth: mount_point.components().count(), mount_len: mount_point.as_os_str().len(), mount: mount_point.to_string_lossy().to_string() }
}

fn mountinfo_entry_for_path<'a>(mounts: &'a [MountinfoEntry], path: &Path) -> Option<&'a MountinfoEntry> {
    mounts.iter().filter(|entry| path.starts_with(&entry.mount_point)).max_by_key(|entry| entry.mount_point.as_os_str().len())
}

fn overlay_mount_option_path(entry: &MountinfoEntry, prefix: &str) -> Option<PathBuf> {
    entry.super_options.iter().find_map(|option| option.strip_prefix(prefix).map(decode_mountinfo_field))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mountinfo_entry(root: &str, mount_point: &str, major_minor: &str, fs_type: &str, source: &str, super_options: &[&str]) -> MountinfoEntry {
        MountinfoEntry {
            root: PathBuf::from(root),
            mount_point: PathBuf::from(mount_point),
            major_minor: major_minor.to_string(),
            fs_type: fs_type.to_string(),
            source: source.to_string(),
            super_options: super_options.iter().map(|value| (*value).to_string()).collect(),
        }
    }

    #[test]
    fn overlay_backing_mount_key_prefers_upperdir_backing_filesystem() {
        let mounts = vec![
            mountinfo_entry("/", "/", "0:53", "overlay", "overlay", &["rw", "lowerdir=/sysroot", "upperdir=/.overlay-data/root-a/upper", "workdir=/.overlay-data/root-a/work"]),
            mountinfo_entry("/", "/.overlay-data", "179:4", "ext4", "/dev/mmcblk0p4", &["rw"]),
            mountinfo_entry("/var/lib/helios", "/var/lib/helios", "179:4", "ext4", "/dev/mmcblk0p4", &["rw"]),
        ];

        assert_eq!(overlay_backing_mount_key(&mounts).as_deref(), Some("179:4:/dev/mmcblk0p4:ext4"));
    }

    #[test]
    fn overlay_backing_mount_key_falls_back_to_visible_block_mount_family_when_upperdir_is_hidden() {
        let mounts = vec![
            mountinfo_entry("/", "/", "0:53", "overlay", "overlay", &["rw", "lowerdir=/mnt/lower", "upperdir=/mnt/data/root-overlay/root-a/upper", "workdir=/mnt/data/root-overlay/root-a/work"]),
            mountinfo_entry("/", "/.overlay-data", "179:4", "ext4", "/dev/mmcblk0p4", &["rw"]),
            mountinfo_entry("/", "/var/lib/helios", "179:4", "ext4", "/dev/mmcblk0p4", &["rw"]),
            mountinfo_entry("/", "/run", "0:26", "tmpfs", "tmpfs", &["rw"]),
            mountinfo_entry("/", "/dev/shm", "0:24", "tmpfs", "tmpfs", &["rw"]),
        ];

        assert_eq!(overlay_backing_mount_key(&mounts).as_deref(), Some("179:4:/dev/mmcblk0p4:ext4"));
    }

    #[test]
    fn mount_selection_key_prefers_primary_mount_over_bind_mount() {
        let primary = mountinfo_entry("/", "/.overlay-data", "179:4", "ext4", "/dev/mmcblk0p4", &["rw"]);
        let bind = mountinfo_entry("/var/lib/helios", "/var/lib/helios", "179:4", "ext4", "/dev/mmcblk0p4", &["rw"]);

        assert!(mount_selection_key(Some(&primary), &primary.mount_point) < mount_selection_key(Some(&bind), &bind.mount_point));
    }

    #[test]
    fn effective_disk_mount_context_maps_overlay_root_to_backing_mount() {
        let mounts = vec![
            mountinfo_entry("/", "/", "0:53", "overlay", "overlay", &["rw", "lowerdir=/sysroot", "upperdir=/.overlay-data/root-a/upper", "workdir=/.overlay-data/root-a/work"]),
            mountinfo_entry("/", "/.overlay-data", "179:4", "ext4", "/dev/mmcblk0p4", &["rw"]),
            mountinfo_entry("/var/lib/helios", "/var/lib/helios", "179:4", "ext4", "/dev/mmcblk0p4", &["rw"]),
        ];
        let root_entry = mounts.iter().find(|entry| entry.mount_point == Path::new("/"));
        let backing_key = overlay_backing_mount_key(&mounts);

        let (effective_entry, effective_mount) = effective_disk_mount_context(&mounts, root_entry, Path::new("/"), backing_key.as_deref());

        assert_eq!(effective_mount, PathBuf::from("/.overlay-data"));
        assert_eq!(effective_entry.map(disk_group_key).as_deref(), Some("179:4:/dev/mmcblk0p4:ext4"));
    }
}
