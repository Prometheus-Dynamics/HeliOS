use anyhow::{anyhow, Context, Result};
use std::fs;

use crate::config::{Config, Mode, Partition};

const MIB: u64 = 1024 * 1024;

#[derive(Debug)]
pub struct PartInfo {
    pub start: u64,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct PartPlan {
    pub name: String,
    pub number: u32,
    pub mode: Mode,
    pub start_mib: u64,
    pub end_mib: u64,
    pub label: Option<String>,
    pub fs_type: Option<String>,
    pub marker: Option<String>,
    pub mkfs: bool,
    pub wipe_signatures: bool,
    pub run_fsck: bool,
    pub run_resizefs: bool,
    pub mount_point: Option<String>,
    pub mount_label: Option<String>,
    pub secondary_marker: Option<String>,
    pub reformat_if_missing_secondary_marker: bool,
}

pub fn detect_disk() -> Option<String> {
    detect_root_dev().map(|dev| disk_from_dev(&dev))
}

fn detect_root_dev() -> Option<String> {
    detect_from_cmdline().or_else(root_from_mountinfo).or_else(root_from_proc_mounts).or_else(|| resolve_by_path("/dev/disk/by-label/ACTIVE")).or_else(|| resolve_by_path("/dev/disk/by-label/RESERVE"))
}

fn detect_from_cmdline() -> Option<String> {
    let cmdline = fs::read_to_string("/proc/cmdline").ok()?;
    for token in cmdline.split_whitespace() {
        if let Some(rest) = token.strip_prefix("root=") {
            if let Some(dev) = resolve_root(rest) {
                return Some(dev);
            }
        }
    }
    None
}

pub fn resolve_root(root_arg: &str) -> Option<String> {
    if let Some(partuuid) = root_arg.strip_prefix("PARTUUID=") {
        if let Some(dev) = resolve_partuuid(partuuid) {
            return Some(dev);
        }
        return resolve_by_path(&format!("/dev/disk/by-partuuid/{}", partuuid));
    }
    if let Some(uuid) = root_arg.strip_prefix("UUID=") {
        if let Some(dev) = resolve_by_path(&format!("/dev/disk/by-uuid/{}", uuid)) {
            return Some(dev);
        }
    } else if let Some(label) = root_arg.strip_prefix("LABEL=") {
        if let Some(dev) = resolve_by_path(&format!("/dev/disk/by-label/{}", label)) {
            return Some(dev);
        }
    } else if root_arg.starts_with("/dev/") {
        if let Some(dev) = resolve_by_path(root_arg) {
            return Some(dev);
        }
        if root_arg != "/dev/root" && dev_path_exists(root_arg) {
            return Some(root_arg.to_string());
        }
    }
    None
}

fn dev_path_exists(path: &str) -> bool {
    if fs::metadata(path).is_ok() {
        return true;
    }

    let Some(name) = path.strip_prefix("/dev/") else {
        return false;
    };

    fs::metadata(format!("/sys/class/block/{}", name)).is_ok()
}

fn resolve_by_path(path: &str) -> Option<String> {
    fs::canonicalize(path).ok().map(|p| p.to_string_lossy().into_owned())
}

fn resolve_partuuid(partuuid: &str) -> Option<String> {
    let target = partuuid.trim().to_ascii_lowercase();
    let entries = fs::read_dir("/sys/class/block").ok()?;
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let uevent = path.join("uevent");
        if let Ok(raw) = fs::read_to_string(uevent) {
            for line in raw.lines() {
                if let Some(val) = line.strip_prefix("PARTUUID=") {
                    if val.trim().eq_ignore_ascii_case(&target) {
                        if let Some(name) = path.file_name() {
                            return Some(format!("/dev/{}", name.to_string_lossy()));
                        }
                    }
                }
            }
        }
    }
    None
}

fn root_from_mountinfo() -> Option<String> {
    let info = fs::read_to_string("/proc/self/mountinfo").ok()?;
    for line in info.lines() {
        let mut parts = line.split(" - ");
        let pre = parts.next().unwrap_or_default().split_whitespace().collect::<Vec<_>>();
        if pre.get(4).copied() != Some("/") {
            continue;
        }
        let post = parts.next().unwrap_or_default().split_whitespace().collect::<Vec<_>>();

        if let Some((maj, min)) = pre.get(2).and_then(|mm| parse_major_minor(mm)) {
            if let Some(dev) = device_from_major_minor(maj, min) {
                return Some(dev);
            }
        }

        if let Some(src) = post.get(1) {
            if let Some(dev) = resolve_root(src) {
                return Some(dev);
            }
        }
    }
    None
}

fn root_from_proc_mounts() -> Option<String> {
    let mounts = fs::read_to_string("/proc/mounts").ok()?;
    for line in mounts.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.get(1).copied() != Some("/") {
            continue;
        }
        if let Some(src) = cols.first() {
            if let Some(dev) = resolve_root(src) {
                return Some(dev);
            }
        }
    }
    None
}

fn device_from_major_minor(maj: u64, min: u64) -> Option<String> {
    let sys = format!("/sys/dev/block/{}:{}", maj, min);
    let path = fs::canonicalize(sys).ok()?;
    let name = path.file_name()?.to_string_lossy();
    Some(format!("/dev/{}", name))
}

fn parse_major_minor(s: &str) -> Option<(u64, u64)> {
    let mut parts = s.split(':');
    let maj = parts.next()?.parse().ok()?;
    let min = parts.next()?.parse().ok()?;
    Some((maj, min))
}

pub fn disk_from_dev(dev: &str) -> String {
    if dev.starts_with("/dev/mmcblk") || dev.starts_with("/dev/nvme") {
        if let Some((base, suffix)) = dev.rsplit_once('p') {
            if suffix.chars().all(|c| c.is_ascii_digit()) {
                return base.to_string();
            }
        }
        dev.to_string()
    } else if dev.starts_with("/dev/sd") {
        dev.trim_end_matches(char::is_numeric).to_string()
    } else {
        dev.to_string()
    }
}

pub fn disk_bn(disk: &str) -> String {
    disk.trim_start_matches("/dev/").to_string()
}

pub fn read_to_u64(path: &str) -> Result<Option<u64>> {
    match fs::read_to_string(path) {
        Ok(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed.parse::<u64>().with_context(|| format!("failed to parse {} as u64 from {}", trimmed, path))?))
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(anyhow!(e).context(format!("failed to read {}", path))),
    }
}

pub fn read_part_info(disk: &str, part: u32) -> Result<PartInfo> {
    let base = disk_bn(disk);
    let suffix = if base.starts_with("mmcblk") || base.starts_with("nvme") { format!("{}p{}", base, part) } else { format!("{}{}", base, part) };
    let start = read_to_u64(&format!("/sys/class/block/{}/start", suffix))?.ok_or_else(|| anyhow!("missing start for partition {}", part))?;
    let size = read_to_u64(&format!("/sys/class/block/{}/size", suffix))?.ok_or_else(|| anyhow!("missing size for partition {}", part))?;
    Ok(PartInfo { start, size })
}

pub fn part_dev_path(disk: &str, part: u32) -> String {
    let base = disk_bn(disk);
    if base.starts_with("mmcblk") || base.starts_with("nvme") {
        format!("{}p{}", disk, part)
    } else {
        format!("{}{}", disk, part)
    }
}

pub fn sectors_to_mib_floor(sectors: u64, sector_bytes: u64) -> u64 {
    sectors.saturating_mul(sector_bytes) / MIB
}

pub fn sectors_to_mib_ceil(sectors: u64, sector_bytes: u64) -> u64 {
    sectors.saturating_mul(sector_bytes).div_ceil(MIB)
}

fn align_up(val: u64, align: u64) -> u64 {
    if align == 0 {
        val
    } else {
        val.div_ceil(align) * align
    }
}

fn align_down(val: u64, align: u64) -> u64 {
    if align == 0 {
        val
    } else {
        (val / align) * align
    }
}

pub fn build_plan(cfg: &Config, ab_start: u64, ab_end: u64, data_start: u64, total_mib: u64) -> Result<Vec<PartPlan>> {
    let align = cfg.defaults.align_mib.unwrap_or(0);
    let gap = cfg.defaults.gap_mib.unwrap_or(0);
    let root_min = cfg.defaults.root_min_mib.unwrap_or(256);
    let mut cursor = ab_start;
    let ab_total = ab_end - ab_start;
    let mut plans = Vec::new();

    for p in &cfg.partitions {
        let PartPlan { mode, label, fs_type, marker, mkfs, wipe_signatures, run_fsck, run_resizefs, .. } = init_plan_defaults(p);

        let is_data = is_data_partition(p);

        let mut start = match p.start_mib {
            Some(s) => s,
            None if is_data && data_start > ab_start && !p.fill_to_end.unwrap_or(false) => data_start,
            None => cursor,
        };
        start = align_up(start, align);

        let mut span = if let Some(size) = p.size_mib {
            size
        } else if let Some(percent) = p.target_percent {
            let mut s = ab_total * percent / 100;
            if s < root_min {
                s = root_min;
            }
            if let Some(max) = p.max_mib {
                if max > 0 && s > max {
                    s = max;
                }
            }
            let half = ab_total / 2;
            if s > half {
                s = half;
            }
            s
        } else if p.fill_to_end.unwrap_or(false) {
            ab_end.saturating_sub(start)
        } else {
            return Err(anyhow!("partition {} missing size/percent", p.name));
        };

        if p.fill_to_end.unwrap_or(false) && mode == Mode::Mkpart {
            span = 0; // will be overridden by end below
        }

        let mut end = if p.fill_to_end.unwrap_or(false) && mode == Mode::Mkpart {
            if let Some(end_override) = p.end_mib {
                end_override
            } else {
                align_down(ab_end, align)
            }
        } else if let Some(end_override) = p.end_mib {
            end_override
        } else {
            start.saturating_add(span)
        };

        if align > 0 {
            end = align_down(end, align);
        }

        if mode != Mode::Mkpart && end > ab_end {
            end = ab_end;
        }
        if mode == Mode::Mkpart {
            // Stay inside the actual disk size; leave a small margin to avoid
            // rounding-up off-by-one issues from integer MiB calculations.
            let disk_cap = if align == 0 { total_mib.saturating_sub(1) } else { align_down(total_mib.saturating_sub(align.max(1)), align) };
            if end > disk_cap {
                end = disk_cap;
            }
        }

        if end <= start {
            return Err(anyhow!("invalid geometry for {}: end ({}) <= start ({})", p.name, end, start));
        }

        plans.push(PartPlan {
            name: p.name.clone(),
            number: p.number,
            mode,
            start_mib: start,
            end_mib: end,
            label,
            fs_type,
            marker,
            mkfs,
            wipe_signatures,
            run_fsck,
            run_resizefs,
            mount_point: p.mount_point.clone(),
            mount_label: p.mount_label.clone(),
            secondary_marker: p.secondary_marker.clone(),
            reformat_if_missing_secondary_marker: p.reformat_if_missing_secondary_marker.unwrap_or(false),
        });

        cursor = end + p.gap_after_mib.unwrap_or(gap);
    }

    Ok(plans)
}

fn init_plan_defaults(p: &Partition) -> PartPlan {
    PartPlan {
        name: p.name.clone(),
        number: p.number,
        mode: p.mode,
        start_mib: 0,
        end_mib: 0,
        label: p.label.clone(),
        fs_type: p.fs_type.clone(),
        marker: p.marker.clone(),
        mkfs: p.mkfs.unwrap_or(true),
        wipe_signatures: p.wipe_signatures.unwrap_or(false),
        run_fsck: p.run_fsck.unwrap_or(true),
        run_resizefs: p.run_resizefs.unwrap_or(true),
        mount_point: p.mount_point.clone(),
        mount_label: p.mount_label.clone(),
        secondary_marker: p.secondary_marker.clone(),
        reformat_if_missing_secondary_marker: p.reformat_if_missing_secondary_marker.unwrap_or(false),
    }
}

fn is_data_partition(p: &Partition) -> bool {
    p.name.eq_ignore_ascii_case("data") || p.label.as_deref().map(|l| l.eq_ignore_ascii_case("data")).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{build_plan, disk_from_dev, resolve_root, sectors_to_mib_ceil};
    use crate::config::{Config, Mode, Partition, Spans};

    #[test]
    fn disk_from_dev_handles_mmc_and_nvme() {
        assert_eq!(disk_from_dev("/dev/mmcblk0p2"), "/dev/mmcblk0");
        assert_eq!(disk_from_dev("/dev/mmcblk1"), "/dev/mmcblk1");
        assert_eq!(disk_from_dev("/dev/nvme0n1p3"), "/dev/nvme0n1");
        assert_eq!(disk_from_dev("/dev/nvme1n1"), "/dev/nvme1n1");
    }

    #[test]
    fn disk_from_dev_handles_sd() {
        assert_eq!(disk_from_dev("/dev/sda3"), "/dev/sda");
    }

    #[test]
    fn disk_from_dev_passthrough_other() {
        assert_eq!(disk_from_dev("/dev/loop0"), "/dev/loop0");
        assert_eq!(disk_from_dev("/dev/mapper/vg0-root"), "/dev/mapper/vg0-root");
    }

    #[test]
    fn resolve_root_rejects_nonexistent_placeholder_device() {
        assert_eq!(resolve_root("/dev/helios-rootfs"), None);
    }

    #[test]
    fn fill_to_end_data_starts_from_cursor_not_reserved_data_start() {
        let cfg = Config {
            spans: Spans { data_size_mib: Some(0), ..Default::default() },
            partitions: vec![Partition {
                name: "DATA".to_string(),
                number: 3,
                label: Some("DATA".to_string()),
                mode: Mode::Mkpart,
                fs_type: Some("ext4".to_string()),
                fill_to_end: Some(true),
                ..Default::default()
            }],
            ..Default::default()
        };

        let plan = build_plan(&cfg, 128, 512, 512, 512).expect("plan");
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].start_mib, 128);
        assert_eq!(plan[0].end_mib, 511);
    }

    #[test]
    fn partition_gap_override_only_expands_selected_gap() {
        let cfg = Config {
            defaults: crate::config::Defaults { gap_mib: Some(4), ..Default::default() },
            partitions: vec![
                Partition { name: "ROOT_A".to_string(), number: 2, mode: Mode::Noop, size_mib: Some(96), ..Default::default() },
                Partition { name: "ROOT_B".to_string(), number: 3, mode: Mode::Mkpart, size_mib: Some(96), gap_after_mib: Some(256), ..Default::default() },
                Partition {
                    name: "DATA".to_string(),
                    number: 4,
                    label: Some("DATA".to_string()),
                    mode: Mode::Mkpart,
                    fs_type: Some("ext4".to_string()),
                    fill_to_end: Some(true),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        let plan = build_plan(&cfg, 32, 1024, 1024, 1024).expect("plan");
        assert_eq!(plan.len(), 3);
        assert_eq!(plan[0].start_mib, 32);
        assert_eq!(plan[0].end_mib, 128);
        assert_eq!(plan[1].start_mib, 132);
        assert_eq!(plan[1].end_mib, 228);
        assert_eq!(plan[2].start_mib, 484);
        assert_eq!(plan[2].end_mib, 1023);
    }

    #[test]
    fn sectors_to_mib_ceil_uses_next_safe_boundary_for_partition_end() {
        assert_eq!(sectors_to_mib_ceil(221_633, 512), 109);
        assert_eq!(sectors_to_mib_ceil(221_184, 512), 108);
    }
}
