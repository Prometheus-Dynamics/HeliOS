use once_cell::sync::Lazy;
use std::io::{Read, Seek, SeekFrom};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::OnceLock;
use tracing::{info, warn};

#[cfg(test)]
static TEST_DATA_ROOT: OnceLock<PathBuf> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageHealthIssue {
    pub code: &'static str,
    pub description: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageHealthSnapshot {
    pub root_fs_type: Option<String>,
    pub data_fs_type: Option<String>,
    pub overlay_data_fs_type: Option<String>,
    pub issues: Vec<StorageHealthIssue>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct StorageProbeSnapshot {
    root_fs_type: Option<String>,
    data_fs_type: Option<String>,
    overlay_data_fs_type: Option<String>,
    data_partition_present: bool,
    data_mountpoint_present: bool,
    overlay_data_mountpoint_present: bool,
    ota_pending_present: bool,
    self_check_repairs: Vec<String>,
    self_check_failures: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SelfCheckReport {
    repairs: Vec<String>,
    failures: Vec<String>,
}

/// Resolve a writable data directory for HTTP handlers. Defaults to a temp dir
/// but can be overridden with `HELIOS_API_DATA_DIR`.
static DATA_ROOT: Lazy<PathBuf> = Lazy::new(|| {
    if let Ok(dir) = std::env::var("HELIOS_API_DATA_DIR") {
        let path = PathBuf::from(dir);
        info!(path = %path.display(), "using HELIOS_API_DATA_DIR for persistent storage");
        warn_if_unmounted_data_root(&path);
        return path;
    }
    let root = default_data_root();
    warn_if_unmounted_data_root(&root);
    root
});

fn default_data_root() -> PathBuf {
    // Prefer persistent storage on device images; fall back to tmp.
    // This avoids "no space" failures when `/tmp` is a small tmpfs.
    let candidates = [PathBuf::from("/data/helios/api"), PathBuf::from("/var/lib/helios/api"), std::env::temp_dir().join("helios-api")];

    for candidate in candidates {
        if ensure_dir(&candidate) {
            if candidate.starts_with("/data") || candidate.starts_with("/var/lib") {
                info!(path = %candidate.display(), "using persistent data directory");
            } else {
                warn!(path = %candidate.display(), "using temporary data directory; persisted streams/pipelines may be lost on restart");
            }
            return candidate;
        }
    }

    let fallback = std::env::temp_dir().join("helios-api");
    warn!(path = %fallback.display(), "using temporary data directory; persisted streams/pipelines may be lost on restart");
    fallback
}

fn warn_if_unmounted_data_root(path: &Path) {
    let mount_root = Path::new("/var/lib/helios");
    if !path.starts_with(mount_root) {
        return;
    }
    if !mount_root.exists() {
        warn!(
            data_root = %path.display(),
            mount_root = %mount_root.display(),
            "data root is under /var/lib/helios but the mount point is missing; data may land on rootfs"
        );
        return;
    }
    if !is_mountpoint(mount_root) {
        warn!(
            data_root = %path.display(),
            mount_root = %mount_root.display(),
            "data root is under /var/lib/helios but the DATA partition is not mounted; data may be hidden after mount"
        );
    }
}

pub fn probe_storage_health() -> StorageHealthSnapshot {
    evaluate_storage_health(&capture_storage_probe())
}

fn capture_storage_probe() -> StorageProbeSnapshot {
    let mounts = read_mounts().unwrap_or_default();
    let self_check = read_self_check_report().unwrap_or_default();
    StorageProbeSnapshot {
        root_fs_type: mount_fs_type(&mounts, Path::new("/")),
        data_fs_type: mount_fs_type(&mounts, Path::new("/var/lib/helios")),
        overlay_data_fs_type: mount_fs_type(&mounts, Path::new("/.overlay-data")),
        data_partition_present: Path::new("/dev/disk/by-label/DATA").exists(),
        data_mountpoint_present: is_mountpoint(Path::new("/var/lib/helios")),
        overlay_data_mountpoint_present: is_mountpoint(Path::new("/.overlay-data")),
        ota_pending_present: ota_pending_marker_present(),
        self_check_repairs: self_check.repairs,
        self_check_failures: self_check.failures,
    }
}

fn evaluate_storage_health(snapshot: &StorageProbeSnapshot) -> StorageHealthSnapshot {
    let mut issues = Vec::new();
    let root_fs = snapshot.root_fs_type.as_deref();
    let data_fs = snapshot.data_fs_type.as_deref();
    let overlay_fs = snapshot.overlay_data_fs_type.as_deref();

    if root_fs == Some("overlay") {
        if !snapshot.overlay_data_mountpoint_present {
            issues.push(StorageHealthIssue { code: "overlay_storage_missing", description: "Overlay root is active but the writable backing store at /.overlay-data is not mounted.".to_string() });
        } else if overlay_fs == Some("tmpfs") {
            issues.push(StorageHealthIssue { code: "overlay_tmpfs_fallback", description: "Writable overlay is backed by tmpfs; OS changes and diagnostics will be lost on reboot.".to_string() });
        }
    }

    if snapshot.data_mountpoint_present {
        if data_fs == Some("tmpfs") {
            issues.push(StorageHealthIssue {
                code: "data_partition_ephemeral",
                description: "Persistent state path /var/lib/helios is on tmpfs; recordings, settings, and logs are ephemeral.".to_string(),
            });
        }
    } else if snapshot.data_partition_present {
        issues.push(StorageHealthIssue { code: "data_partition_unmounted", description: "DATA partition exists but is not mounted at /var/lib/helios.".to_string() });
    } else if matches!(root_fs, Some("overlay") | Some("squashfs")) {
        issues.push(StorageHealthIssue { code: "data_partition_missing", description: "DATA partition is missing; persistent state is unavailable on this boot.".to_string() });
    }

    if root_fs == Some("squashfs") {
        issues.push(StorageHealthIssue { code: "root_overlay_missing", description: "Root filesystem is mounted directly as squashfs without a writable overlay.".to_string() });
    }

    issues.extend(evaluate_ota_health(root_fs));

    if !snapshot.self_check_failures.is_empty() {
        issues.push(StorageHealthIssue {
            code: "self_repair_failed",
            description: format!("Automatic OS self-check could not complete all safe repairs: {}.", describe_self_check_codes(&snapshot.self_check_failures)),
        });
    }

    if snapshot.ota_pending_present {
        issues.push(StorageHealthIssue {
            code: "ota_pending_confirmation",
            description: "An OTA update is still pending confirmation on this boot; rollback may occur if confirmation does not complete.".to_string(),
        });
    }

    let visible_repairs: Vec<String> = snapshot.self_check_repairs.iter().filter(|code| !is_benign_self_repair(code)).cloned().collect();
    if !visible_repairs.is_empty() {
        issues.push(StorageHealthIssue { code: "self_repair_applied", description: format!("Automatic OS self-check repaired this boot: {}.", describe_self_check_codes(&visible_repairs)) });
    }

    StorageHealthSnapshot { root_fs_type: snapshot.root_fs_type.clone(), data_fs_type: snapshot.data_fs_type.clone(), overlay_data_fs_type: snapshot.overlay_data_fs_type.clone(), issues }
}

fn is_benign_self_repair(code: &str) -> bool {
    matches!(code, "helios_state_dirs_created")
}

fn ota_pending_marker_present() -> bool {
    ["/boot/helios/ota/pending", "/mnt/boot/helios/ota/pending", "/var/lib/helios/ota/pending"].iter().any(|candidate| Path::new(candidate).exists())
}

fn read_self_check_report() -> Option<SelfCheckReport> {
    ["/run/helios/os-self-check.env", "/var/lib/helios/state/os-self-check.env"].iter().find_map(|candidate| parse_self_check_report(Path::new(candidate)))
}

fn parse_self_check_report(path: &Path) -> Option<SelfCheckReport> {
    let raw = std::fs::read_to_string(path).ok()?;
    let mut report = SelfCheckReport::default();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "REPAIRS" => report.repairs = parse_code_list(value),
            "FAILURES" => report.failures = parse_code_list(value),
            _ => {}
        }
    }
    Some(report)
}

fn parse_code_list(raw: &str) -> Vec<String> {
    raw.split(',').map(str::trim).filter(|entry| !entry.is_empty()).map(ToOwned::to_owned).collect()
}

fn describe_self_check_codes(codes: &[String]) -> String {
    let parts = codes
        .iter()
        .map(|code| match code.as_str() {
            "data_partition_mounted" => "mounted the DATA partition",
            "overlay_data_bound" => "restored the /.overlay-data bind mount",
            "helios_state_dirs_created" => "created missing HeliOS state directories",
            "data_partition_mount_failed" => "mounting the DATA partition failed",
            "overlay_data_bind_failed" => "restoring the /.overlay-data bind mount failed",
            "helios_state_dirs_failed" => "creating required HeliOS state directories failed",
            other => other,
        })
        .collect::<Vec<_>>();
    parts.join(", ")
}

#[derive(Debug, Clone)]
struct MountEntry {
    mount_point: PathBuf,
    fs_type: String,
}

fn read_mounts() -> std::io::Result<Vec<MountEntry>> {
    let raw = std::fs::read_to_string("/proc/self/mountinfo")?;
    Ok(raw
        .lines()
        .filter_map(|line| {
            let (pre, post) = line.split_once(" - ")?;
            let pre_fields: Vec<&str> = pre.split_whitespace().collect();
            let post_fields: Vec<&str> = post.split_whitespace().collect();
            let mount_point = pre_fields.get(4)?;
            let fs_type = post_fields.first()?;
            Some(MountEntry { mount_point: decode_mount_field(mount_point), fs_type: (*fs_type).to_string() })
        })
        .collect())
}

fn mount_fs_type(mounts: &[MountEntry], path: &Path) -> Option<String> {
    mounts.iter().find(|entry| entry.mount_point == path).map(|entry| entry.fs_type.clone())
}

fn decode_mount_field(raw: &str) -> PathBuf {
    let mut out = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx] == b'\\' && idx + 3 < bytes.len() {
            let octal = &raw[idx + 1..idx + 4];
            if octal.as_bytes().iter().all(|b| (b'0'..=b'7').contains(b))
                && let Ok(value) = u8::from_str_radix(octal, 8)
            {
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

#[cfg(unix)]
fn is_mountpoint(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    let Some(parent) = path.parent() else {
        return false;
    };
    let Ok(parent_meta) = std::fs::metadata(parent) else {
        return false;
    };
    meta.dev() != parent_meta.dev()
}

#[cfg(not(unix))]
fn is_mountpoint(_path: &Path) -> bool {
    false
}

fn ensure_dir(path: &PathBuf) -> bool {
    std::fs::create_dir_all(path).and_then(|_| std::fs::metadata(path)).map(|meta| meta.is_dir()).unwrap_or(false)
}

fn evaluate_ota_health(root_fs: Option<&str>) -> Vec<StorageHealthIssue> {
    if by_label("ACTIVE").is_some() && by_label("RESERVE").is_some() {
        return Vec::new();
    }
    if !matches!(root_fs, Some("overlay") | Some("squashfs")) {
        return Vec::new();
    }

    let Some(layout) = detect_squashfs_ota_layout() else {
        return vec![StorageHealthIssue {
            code: "ota_layout_unsupported",
            description: "This squashfs image does not expose a dedicated inactive root slot and DATA partition layout, so normal OTA updates are unavailable.".to_string(),
        }];
    };

    if !layout.data_matches_expected {
        return vec![StorageHealthIssue {
            code: "ota_layout_unsupported",
            description: format!("DATA is mounted from {} but squashfs OTA expects {} so the inactive root slot cannot be identified safely.", layout.data_device, layout.data_partition),
        }];
    }

    if !layout.slot_b_exists {
        return vec![StorageHealthIssue { code: "ota_inactive_slot_missing", description: "Inactive squashfs root slot ROOT_B is missing; OTA cannot stage a second root image.".to_string() }];
    }

    let mut issues = Vec::new();
    if slot_has_ext4_signature(&layout.slot_b_partition) {
        issues.push(StorageHealthIssue {
            code: "ota_inactive_slot_unprepared",
            description: "Inactive squashfs root slot still contains an ext4 filesystem signature from an older layout and was not prepared as a raw reserve slot.".to_string(),
        });
    }

    let active = boot_ota_marker("active");
    let reserve = boot_ota_marker("reserve");
    let active_ok = matches!(active.as_deref(), Some("ROOT_A") | Some("ROOT_B"));
    let reserve_ok = matches!(reserve.as_deref(), Some("ROOT_A") | Some("ROOT_B"));
    if !active_ok || !reserve_ok {
        issues.push(StorageHealthIssue {
            code: "ota_boot_metadata_missing",
            description: "BOOT is missing squashfs OTA slot metadata; the updater cannot prove which slot is active versus inactive.".to_string(),
        });
    }

    if let Some((active_slot, reserve_slot)) = active.as_deref().zip(reserve.as_deref())
        && let (Some(active_bytes), Some(reserve_bytes)) = (slot_size_for_marker(&layout, active_slot), slot_size_for_marker(&layout, reserve_slot))
        && reserve_bytes < active_bytes
    {
        issues.push(StorageHealthIssue {
            code: "ota_inactive_slot_too_small",
            description: format!(
                "Inactive squashfs slot {} is smaller than the active slot {} ({} MiB vs {} MiB) and may not fit the next OTA.",
                reserve_slot,
                active_slot,
                reserve_bytes / (1024 * 1024),
                active_bytes / (1024 * 1024)
            ),
        });
    }

    issues
}

#[derive(Debug, Clone)]
struct SquashfsOtaLayout {
    slot_a_partition: String,
    slot_b_partition: String,
    data_partition: String,
    data_device: String,
    data_matches_expected: bool,
    slot_b_exists: bool,
}

fn detect_squashfs_ota_layout() -> Option<SquashfsOtaLayout> {
    let data_device = by_label("DATA")?;
    let disk = disk_from_partition_device(&data_device)?;
    let slot_a_partition = partition_device(&disk, 2);
    let slot_b_partition = partition_device(&disk, 3);
    let data_partition = partition_device(&disk, 4);
    let data_matches_expected = canonicalize_existing_path(&data_device) == canonicalize_existing_path(&data_partition);
    Some(SquashfsOtaLayout {
        slot_a_partition,
        slot_b_partition: slot_b_partition.clone(),
        data_partition,
        data_device,
        data_matches_expected,
        slot_b_exists: canonicalize_existing_path(&slot_b_partition).is_some() || Path::new(&slot_b_partition).exists(),
    })
}

fn by_label(label: &str) -> Option<String> {
    canonicalize_existing_path(&format!("/dev/disk/by-label/{label}"))
}

fn canonicalize_existing_path(path: &str) -> Option<String> {
    let raw = Path::new(path);
    if let Ok(canon) = std::fs::canonicalize(raw) {
        return Some(canon.to_string_lossy().into_owned());
    }
    if raw.exists() {
        return Some(path.to_string());
    }
    None
}

fn disk_from_partition_device(dev: &str) -> Option<String> {
    match dev {
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

fn read_block_size_bytes(dev: &str) -> Option<u64> {
    let canonical = canonicalize_existing_path(dev)?;
    let name = Path::new(&canonical).file_name()?.to_string_lossy().into_owned();
    let sys_root = Path::new("/sys/class/block").join(name);
    let sectors = std::fs::read_to_string(sys_root.join("size")).ok()?.trim().parse::<u64>().ok()?;
    let sector_bytes = std::fs::read_to_string(sys_root.join("queue/logical_block_size")).ok()?.trim().parse::<u64>().ok().unwrap_or(512);
    Some(sectors.saturating_mul(sector_bytes))
}

fn slot_size_for_marker(layout: &SquashfsOtaLayout, slot: &str) -> Option<u64> {
    match slot {
        "ROOT_A" => read_block_size_bytes(&layout.slot_a_partition),
        "ROOT_B" => read_block_size_bytes(&layout.slot_b_partition),
        _ => None,
    }
}

fn boot_ota_marker(name: &str) -> Option<String> {
    [Path::new("/boot/helios/ota").join(name), Path::new("/mnt/boot/helios/ota").join(name), Path::new("/var/lib/helios/ota").join(name)]
        .into_iter()
        .find_map(|path| std::fs::read_to_string(path).ok().map(|raw| raw.trim().to_string()).filter(|raw| !raw.is_empty()))
}

fn slot_has_ext4_signature(dev: &str) -> bool {
    let Some(path) = canonicalize_existing_path(dev) else {
        return false;
    };
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    if file.seek(SeekFrom::Start(1080)).is_err() {
        return false;
    }
    let mut buf = [0u8; 2];
    if file.read_exact(&mut buf).is_err() {
        return false;
    }
    buf == [0x53, 0xEF]
}

fn data_root() -> PathBuf {
    #[cfg(test)]
    if let Some(path) = TEST_DATA_ROOT.get() {
        return path.clone();
    }
    DATA_ROOT.clone()
}

/// Expose the resolved data root for callers that need to locate the data partition.
pub fn data_root_path() -> PathBuf {
    data_root()
}

#[cfg(test)]
pub fn set_data_root_for_tests(path: PathBuf) {
    let _ = TEST_DATA_ROOT.set(path);
}

/// Ensure a named subdirectory exists and return its path.
pub fn ensure_subdir(name: &str) -> std::io::Result<PathBuf> {
    let dir = data_root().join(name);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Async version of [`ensure_subdir`], avoiding blocking calls inside async handlers.
pub async fn ensure_subdir_async(name: &str) -> std::io::Result<PathBuf> {
    let dir = data_root().join(name);
    tokio::fs::create_dir_all(&dir).await?;
    Ok(dir)
}

/// Sanitize a user-provided file name by stripping paths and trimming whitespace.
pub fn sanitize_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    std::path::Path::new(trimmed).file_name().map(|name| name.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::{StorageProbeSnapshot, decode_mount_field, evaluate_storage_health, parse_code_list, parse_self_check_report};
    use std::path::PathBuf;

    #[test]
    fn overlay_tmpfs_fallback_is_reported() {
        let snapshot = StorageProbeSnapshot {
            root_fs_type: Some("overlay".to_string()),
            data_fs_type: Some("tmpfs".to_string()),
            overlay_data_fs_type: Some("tmpfs".to_string()),
            data_partition_present: false,
            data_mountpoint_present: true,
            overlay_data_mountpoint_present: true,
            ota_pending_present: false,
            self_check_repairs: Vec::new(),
            self_check_failures: Vec::new(),
        };

        let health = evaluate_storage_health(&snapshot);
        assert!(health.issues.iter().any(|issue| issue.code == "overlay_tmpfs_fallback"));
        assert!(health.issues.iter().any(|issue| issue.code == "data_partition_ephemeral"));
    }

    #[test]
    fn data_partition_unmounted_is_reported() {
        let snapshot = StorageProbeSnapshot {
            root_fs_type: Some("overlay".to_string()),
            data_fs_type: None,
            overlay_data_fs_type: Some("ext4".to_string()),
            data_partition_present: true,
            data_mountpoint_present: false,
            overlay_data_mountpoint_present: true,
            ota_pending_present: false,
            self_check_repairs: Vec::new(),
            self_check_failures: Vec::new(),
        };

        let health = evaluate_storage_health(&snapshot);
        assert!(health.issues.iter().any(|issue| issue.code == "data_partition_unmounted"));
    }

    #[test]
    fn benign_state_directory_repair_is_not_reported_as_health_issue() {
        let snapshot = StorageProbeSnapshot {
            root_fs_type: Some("overlay".to_string()),
            data_fs_type: Some("ext4".to_string()),
            overlay_data_fs_type: Some("ext4".to_string()),
            data_partition_present: true,
            data_mountpoint_present: true,
            overlay_data_mountpoint_present: true,
            ota_pending_present: false,
            self_check_repairs: vec!["helios_state_dirs_created".to_string()],
            self_check_failures: Vec::new(),
        };

        let health = evaluate_storage_health(&snapshot);
        assert!(!health.issues.iter().any(|issue| issue.code == "self_repair_applied"));
    }

    #[test]
    fn non_benign_repairs_remain_reported() {
        let snapshot = StorageProbeSnapshot {
            root_fs_type: Some("overlay".to_string()),
            data_fs_type: Some("ext4".to_string()),
            overlay_data_fs_type: Some("ext4".to_string()),
            data_partition_present: true,
            data_mountpoint_present: true,
            overlay_data_mountpoint_present: true,
            ota_pending_present: false,
            self_check_repairs: vec!["helios_state_dirs_created".to_string(), "data_partition_mounted".to_string()],
            self_check_failures: Vec::new(),
        };

        let health = evaluate_storage_health(&snapshot);
        let issue = health.issues.iter().find(|issue| issue.code == "self_repair_applied").expect("expected non-benign repair issue");
        assert!(issue.description.contains("mounted the DATA partition"));
        assert!(!issue.description.contains("created missing HeliOS state directories"));
    }

    #[test]
    fn direct_squashfs_root_is_reported() {
        let snapshot = StorageProbeSnapshot {
            root_fs_type: Some("squashfs".to_string()),
            data_fs_type: None,
            overlay_data_fs_type: None,
            data_partition_present: false,
            data_mountpoint_present: false,
            overlay_data_mountpoint_present: false,
            ota_pending_present: false,
            self_check_repairs: Vec::new(),
            self_check_failures: Vec::new(),
        };

        let health = evaluate_storage_health(&snapshot);
        assert!(health.issues.iter().any(|issue| issue.code == "root_overlay_missing"));
        assert!(health.issues.iter().any(|issue| issue.code == "data_partition_missing"));
    }

    #[test]
    fn mount_field_decodes_octal_escapes() {
        assert_eq!(decode_mount_field("/path/with\\040space"), PathBuf::from("/path/with space"));
    }

    #[test]
    fn ota_pending_is_reported() {
        let snapshot = StorageProbeSnapshot {
            root_fs_type: Some("overlay".to_string()),
            data_fs_type: Some("ext4".to_string()),
            overlay_data_fs_type: Some("ext4".to_string()),
            data_partition_present: true,
            data_mountpoint_present: true,
            overlay_data_mountpoint_present: true,
            ota_pending_present: true,
            self_check_repairs: Vec::new(),
            self_check_failures: Vec::new(),
        };

        let health = evaluate_storage_health(&snapshot);
        assert!(health.issues.iter().any(|issue| issue.code == "ota_pending_confirmation"));
    }

    #[test]
    fn self_repair_outcomes_are_reported() {
        let snapshot = StorageProbeSnapshot {
            root_fs_type: Some("overlay".to_string()),
            data_fs_type: Some("ext4".to_string()),
            overlay_data_fs_type: Some("ext4".to_string()),
            data_partition_present: true,
            data_mountpoint_present: true,
            overlay_data_mountpoint_present: true,
            ota_pending_present: false,
            self_check_repairs: vec!["data_partition_mounted".to_string()],
            self_check_failures: vec!["overlay_data_bind_failed".to_string()],
        };

        let health = evaluate_storage_health(&snapshot);
        assert!(health.issues.iter().any(|issue| issue.code == "self_repair_applied"));
        assert!(health.issues.iter().any(|issue| issue.code == "self_repair_failed"));
    }

    #[test]
    fn self_check_report_parses_env_lists() {
        let path = std::env::temp_dir().join(format!("helios-os-self-check-{}.env", std::process::id()));
        std::fs::write(&path, "REPAIRS=data_partition_mounted,overlay_data_bound\nFAILURES=overlay_data_bind_failed\n").expect("write report");
        let report = parse_self_check_report(&path).expect("parse report");
        let _ = std::fs::remove_file(&path);

        assert_eq!(report.repairs, vec!["data_partition_mounted".to_string(), "overlay_data_bound".to_string()]);
        assert_eq!(report.failures, vec!["overlay_data_bind_failed".to_string()]);
        assert_eq!(parse_code_list("a,b,c"), vec!["a".to_string(), "b".to_string(), "c".to_string()]);
    }
}
