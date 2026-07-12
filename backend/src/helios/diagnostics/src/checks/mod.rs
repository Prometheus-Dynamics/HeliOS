use std::collections::BTreeSet;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use chrono::Utc;

use crate::config::DiagnosticsConfig;
use crate::model::{
    CameraReport, DynamicLinkReport, ExecutableFileReport, HealthReport, HealthStatus, IdentityReport, JournalReport, LoadAverageReport, ManagedBinEntry, ManagedBinsReport, MemoryReport, MountReport,
    OrionReport, OtaReport, OverlayReport, PluginReport, ProcessRuntimeReport, RuntimeReport, ServiceReport,
};

const EXPECTED_MANAGED_BINS: &[&str] = &["orion-node", "orionctl", "helios-engine", "helios-peripherals", "helios-api", "helios-updater"];

const EXPECTED_SERVICES: &[&str] =
    &["orion-node.service", "helios-engine.service", "helios-peripherals.service", "helios-api.service", "helios-updater.service", "sshd.service", "helios-set-governor.service"];

const EXPECTED_EXECUTABLE_FILES: &[&str] = &["/usr/local/bin/helios-os-self-check.sh", "/opt/set-governor.sh", "/opt/helios-update-issue.sh"];
const LOCAL_OTA_DIR: &str = "/var/lib/helios/ota";
const UPDATER_SOCKET: &str = "/run/helios/updater.sock";
const BOOT_MOUNT: &str = "/boot";
const STORAGE_LAYOUT_ENV: &str = "/etc/helios/storage-layout.env";

pub fn collect_health_report(config: &DiagnosticsConfig) -> Result<HealthReport> {
    let writable_store = collect_mount_report(&config.writable_store_mount)?;
    let journal = collect_journal_report(config)?;
    let identity = collect_identity_report(config);
    let camera = collect_camera_report(config)?;
    let overlay = collect_overlay_report(&config.overlay_root)?;
    let runtime = collect_runtime_report()?;
    let managed_bins = collect_managed_bins(&config.managed_bin_dir)?;
    let executable_files = collect_executable_files();
    let dynamic_links = collect_dynamic_links(&config.managed_bin_dir);
    let plugins = collect_plugins(&config.plugin_dir)?;
    let orion = collect_orion(&config.orion_run_dir);
    let ota = collect_ota_report()?;
    let services = collect_services()?;
    let writable_store_is_tmpfs = writable_store.fs_type.as_deref() == Some("tmpfs");
    let journal_is_tmpfs = journal.source.as_deref().is_some_and(|source| source.starts_with("tmpfs"));

    let mut status = HealthStatus::Ok;
    if !writable_store.mounted
        || writable_store_is_tmpfs
        || !managed_bins.exists
        || managed_bins.entries.iter().any(|entry| !entry.exists || !entry.executable)
        || executable_files.iter().any(|file| !file.exists || !file.executable)
        || dynamic_links.iter().any(|link| !link.ok)
        || !orion.control_socket
        || !orion.control_stream_socket
        || !journal.mounted
        || !journal.on_writable_store
        || journal_is_tmpfs
        || services.iter().any(|service| service.active_state == "failed")
        || services.iter().any(|service| service.unit == "orion-node.service" && !service_is_healthy(service))
    {
        status = HealthStatus::Failed;
    } else if !journal.has_files
        || !identity.machine_id_persisted
        || !identity.ssh_host_keys_persisted
        || (camera.startup_preset_declares_camera && camera.discovered_camera_resources == 0)
        || !overlay.slot_dirs_present
        || overlay.image_owned_override_count > 0
        || !ota.os_image_ready
        || runtime.processes.iter().any(|process| process.process_count > 1)
        || services.iter().any(|service| service.unit.starts_with("helios-") && !service.uses_managed_bin && service.exec_start.iter().any(|exec| exec.contains("/usr/bin/helios-")))
    {
        status = HealthStatus::Degraded;
    } else if services.iter().any(|service| !service_is_healthy(service)) {
        status = HealthStatus::Degraded;
    }

    Ok(HealthReport { generated_at: Utc::now(), status, writable_store, journal, identity, camera, overlay, runtime, managed_bins, executable_files, dynamic_links, plugins, orion, ota, services })
}

fn collect_runtime_report() -> Result<RuntimeReport> {
    Ok(RuntimeReport { loadavg: collect_loadavg()?, memory: collect_memory_report()?, processes: collect_process_runtime_reports()? })
}

fn collect_loadavg() -> Result<Option<LoadAverageReport>> {
    let content = match fs::read_to_string("/proc/loadavg") {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };

    let mut parts = content.split_whitespace();
    let one = match parts.next().and_then(|value| value.parse::<f32>().ok()) {
        Some(value) => value,
        None => return Ok(None),
    };
    let five = match parts.next().and_then(|value| value.parse::<f32>().ok()) {
        Some(value) => value,
        None => return Ok(None),
    };
    let fifteen = match parts.next().and_then(|value| value.parse::<f32>().ok()) {
        Some(value) => value,
        None => return Ok(None),
    };
    let tasks = match parts.next() {
        Some(value) => value,
        None => return Ok(None),
    };
    let mut task_parts = tasks.split('/');
    let running_tasks = match task_parts.next().and_then(|value| value.parse::<u32>().ok()) {
        Some(value) => value,
        None => return Ok(None),
    };
    let total_tasks = match task_parts.next().and_then(|value| value.parse::<u32>().ok()) {
        Some(value) => value,
        None => return Ok(None),
    };

    Ok(Some(LoadAverageReport { one, five, fifteen, running_tasks, total_tasks }))
}

fn collect_memory_report() -> Result<Option<MemoryReport>> {
    let content = match fs::read_to_string("/proc/meminfo") {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };

    let value_for = |key: &str| -> Option<u64> {
        content.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            if name != key {
                return None;
            }
            value.split_whitespace().next()?.parse::<u64>().ok()
        })
    };

    let total_kib = match value_for("MemTotal") {
        Some(value) => value,
        None => return Ok(None),
    };

    Ok(Some(MemoryReport {
        total_kib,
        available_kib: value_for("MemAvailable"),
        buffers_kib: value_for("Buffers"),
        cached_kib: value_for("Cached"),
        slab_kib: value_for("Slab"),
        reclaimable_slab_kib: value_for("SReclaimable"),
        shmem_kib: value_for("Shmem"),
        swap_total_kib: value_for("SwapTotal"),
        swap_free_kib: value_for("SwapFree"),
    }))
}

fn collect_process_runtime_reports() -> Result<Vec<ProcessRuntimeReport>> {
    let mut reports = Vec::new();
    for unit in EXPECTED_SERVICES {
        let output = Command::new("systemctl").args(["show", unit, "--property=MainPID", "--value"]).output().with_context(|| format!("failed to inspect main pid for {unit}"))?;
        let pid = String::from_utf8_lossy(&output.stdout).trim().parse::<u32>().ok().filter(|pid| *pid > 0);
        let Some(pid) = pid else {
            continue;
        };
        let executable_name = unit.trim_end_matches(".service");
        let matching_pids = process_ids_for_executable(executable_name)?;
        let extra_pids = matching_pids.iter().copied().filter(|other| *other != pid).collect::<Vec<_>>();

        reports.push(ProcessRuntimeReport {
            unit: (*unit).to_string(),
            process_count: matching_pids.len(),
            extra_pids,
            pid,
            rss_kib: smaps_rollup_value(pid, "Rss"),
            pss_kib: smaps_rollup_value(pid, "Pss"),
            private_clean_kib: smaps_rollup_value(pid, "Private_Clean"),
            private_dirty_kib: smaps_rollup_value(pid, "Private_Dirty"),
        });
    }
    Ok(reports)
}

fn process_ids_for_executable(executable_name: &str) -> Result<Vec<u32>> {
    let mut pids = BTreeSet::new();
    for entry in fs::read_dir("/proc").context("failed to enumerate /proc")? {
        let entry = entry?;
        let file_name = entry.file_name();
        let Some(pid) = file_name.to_str().and_then(|value| value.parse::<u32>().ok()) else {
            continue;
        };
        let exe = match fs::read_link(format!("/proc/{pid}/exe")) {
            Ok(exe) => exe,
            Err(_) => continue,
        };
        if exe.file_name().and_then(|name| name.to_str()) == Some(executable_name) {
            pids.insert(pid);
        }
    }
    Ok(pids.into_iter().collect())
}

fn smaps_rollup_value(pid: u32, key: &str) -> Option<u64> {
    let path = format!("/proc/{pid}/smaps_rollup");
    let content = fs::read_to_string(path).ok()?;
    content.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        if name.trim() != key {
            return None;
        }
        value.split_whitespace().next()?.parse::<u64>().ok()
    })
}

fn collect_mount_report(path: &Path) -> Result<MountReport> {
    let output = Command::new("findmnt").args(["-n", "-o", "SOURCE,FSTYPE", path.to_string_lossy().as_ref()]).output().context("failed to run findmnt")?;

    if !output.status.success() {
        return Ok(MountReport { path: path.display().to_string(), mounted: false, fs_type: None, source: None });
    }

    let line = String::from_utf8_lossy(&output.stdout);
    let mut parts = line.split_whitespace();
    let source = parts.next().map(ToOwned::to_owned);
    let fs_type = parts.next().map(ToOwned::to_owned);

    Ok(MountReport { path: path.display().to_string(), mounted: true, fs_type, source })
}

fn collect_journal_report(config: &DiagnosticsConfig) -> Result<JournalReport> {
    let mount = collect_mount_report(&config.journal_mount)?;
    let store_mount = collect_mount_report(&config.writable_store_mount)?;
    let on_writable_store = match (&mount.source, &store_mount.source) {
        (Some(journal_source), Some(store_source)) => journal_source == store_source || journal_source.starts_with(&format!("{store_source}[")),
        _ => false,
    };
    let has_files = mount.mounted && journal_has_files(&config.journal_mount)?;
    Ok(JournalReport { path: mount.path, mounted: mount.mounted, source: mount.source, persistent_dir: config.persistent_journal_dir.display().to_string(), on_writable_store, has_files })
}

fn journal_has_files(path: &Path) -> Result<bool> {
    if !path.is_dir() {
        return Ok(false);
    }

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_file() {
            return Ok(true);
        }
        if file_type.is_dir() && journal_has_files(&entry.path())? {
            return Ok(true);
        }
    }

    Ok(false)
}

fn collect_identity_report(config: &DiagnosticsConfig) -> IdentityReport {
    let machine_id_persisted = symlink_points_to(&config.machine_id_path, &config.persistent_machine_id_path);
    let ssh_host_keys_persisted = ["ssh_host_rsa_key", "ssh_host_rsa_key.pub", "ssh_host_ecdsa_key", "ssh_host_ecdsa_key.pub", "ssh_host_ed25519_key", "ssh_host_ed25519_key.pub"]
        .iter()
        .all(|name| symlink_points_to(&config.ssh_host_key_dir.join(name), &config.persistent_ssh_host_key_dir.join(name)));
    IdentityReport { machine_id_persisted, ssh_host_keys_persisted }
}

fn collect_camera_report(config: &DiagnosticsConfig) -> Result<CameraReport> {
    let startup_preset_declares_camera = fs::read_to_string(&config.startup_preset_path)
        .map(|content| content.contains("[[streams]]") && (content.contains("cameraId") || content.contains("backend = \"Libcamera\"")))
        .unwrap_or(false);

    let discovered_camera_resources = count_camera_resources(&config.orion_run_dir)?;
    Ok(CameraReport { startup_preset_declares_camera, discovered_camera_resources })
}

fn count_camera_resources(orion_run_dir: &Path) -> Result<usize> {
    let socket = orion_run_dir.join("control.sock");
    if !socket.exists() {
        return Ok(0);
    }

    let output = Command::new("orionctl").args(["get", "resources", "--socket", socket.to_string_lossy().as_ref()]).output().context("failed to inspect Orion resources")?;
    if !output.status.success() {
        return Ok(0);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.lines().filter(|line| line.contains(" type=camera.device") || line.contains("type: camera.device")).count())
}

fn collect_overlay_report(path: &Path) -> Result<OverlayReport> {
    let slot_dirs_present = path.join("root-a/upper").is_dir() && path.join("root-a/work").is_dir() && path.join("root-b/upper").is_dir() && path.join("root-b/work").is_dir();
    let image_owned_overrides = collect_overlay_image_overrides(path)?;
    Ok(OverlayReport { path: path.display().to_string(), slot_dirs_present, image_owned_override_count: image_owned_overrides.len(), image_owned_overrides })
}

fn collect_overlay_image_overrides(path: &Path) -> Result<Vec<String>> {
    let mut overrides = Vec::new();
    if !path.exists() {
        return Ok(overrides);
    }

    for slot in ["root-a", "root-b"] {
        let upper = path.join(slot).join("upper");
        if !upper.is_dir() {
            continue;
        }

        for rel in [
            "usr/bin/helios-api",
            "usr/bin/helios-engine",
            "usr/bin/helios-peripherals",
            "usr/bin/helios-updater",
            "usr/bin/heliosctl",
            "usr/bin/helios-provision",
            "usr/bin/orion-node",
            "usr/bin/orionctl",
            "usr/local/bin/helios-provision-squashfs.sh",
        ] {
            let candidate = upper.join(rel);
            if candidate.exists() {
                overrides.push(candidate.display().to_string());
            }
        }
    }

    overrides.sort();
    Ok(overrides)
}

fn symlink_points_to(path: &Path, expected_target: &Path) -> bool {
    fs::read_link(path).ok().map(|target| target == expected_target).unwrap_or(false)
}

fn collect_managed_bins(path: &Path) -> Result<ManagedBinsReport> {
    let exists = path.exists();
    let mut entries = Vec::new();

    for name in EXPECTED_MANAGED_BINS {
        let full = path.join(name);
        let target = fs::read_link(&full).ok().map(|target| target.display().to_string());
        entries.push(ManagedBinEntry { name: (*name).to_string(), exists: full.exists(), executable: is_executable(&full), target });
    }

    Ok(ManagedBinsReport { path: path.display().to_string(), exists, entries })
}

fn collect_executable_files() -> Vec<ExecutableFileReport> {
    EXPECTED_EXECUTABLE_FILES
        .iter()
        .map(|path| {
            let path = Path::new(path);
            let metadata = fs::metadata(path).ok();
            ExecutableFileReport {
                path: path.display().to_string(),
                exists: metadata.is_some(),
                executable: metadata.as_ref().is_some_and(|metadata| metadata.is_file() && executable_mode(metadata)),
                mode: metadata.as_ref().map(mode_string),
            }
        })
        .collect()
}

fn collect_dynamic_links(managed_bin_dir: &Path) -> Vec<DynamicLinkReport> {
    EXPECTED_MANAGED_BINS
        .iter()
        .map(|name| {
            let managed = managed_bin_dir.join(name);
            let binary = if managed.exists() { managed } else { PathBuf::from(format!("/usr/bin/{name}")) };
            inspect_dynamic_links(&binary)
        })
        .collect()
}

fn inspect_dynamic_links(binary: &Path) -> DynamicLinkReport {
    if !binary.exists() {
        return DynamicLinkReport { binary: binary.display().to_string(), ok: false, missing_libraries: Vec::new(), error: Some("binary does not exist".to_string()) };
    }
    let output = match Command::new("ldd").arg(binary).output() {
        Ok(output) => output,
        Err(error) => {
            return DynamicLinkReport { binary: binary.display().to_string(), ok: false, missing_libraries: Vec::new(), error: Some(format!("failed to run ldd: {error}")) };
        }
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");
    let missing_libraries = combined
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if !line.ends_with("=> not found") {
                return None;
            }
            line.split_whitespace().next().map(ToOwned::to_owned)
        })
        .collect::<Vec<_>>();
    let error = if output.status.success() || !missing_libraries.is_empty() { None } else { Some(combined.trim().to_string()) };
    DynamicLinkReport { binary: binary.display().to_string(), ok: missing_libraries.is_empty() && error.is_none(), missing_libraries, error }
}

fn collect_plugins(path: &Path) -> Result<PluginReport> {
    let mut entries = Vec::new();
    if path.exists() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                entries.push(entry.file_name().to_string_lossy().into_owned());
            }
        }
        entries.sort();
    }

    Ok(PluginReport { path: path.display().to_string(), entries })
}

fn collect_orion(path: &Path) -> OrionReport {
    OrionReport { run_dir: path.display().to_string(), control_socket: path.join("control.sock").exists(), control_stream_socket: path.join("control-stream.sock").exists() }
}

fn collect_ota_report() -> Result<OtaReport> {
    let local_ota_dir = Path::new(LOCAL_OTA_DIR);
    let updater_socket = Path::new(UPDATER_SOCKET);
    let boot_mount = collect_mount_report(Path::new(BOOT_MOUNT))?;
    let active_slot = read_trimmed(local_ota_dir.join("active"));
    let reserve_slot = read_trimmed(local_ota_dir.join("reserve"));
    let boot_device = boot_device_from_layout().map(|path| path.display().to_string());
    let boot_device_exists = boot_device.as_deref().is_some_and(|path| Path::new(path).exists());
    let confirm_service_installed = systemd_unit_load_state("helios-ota-confirm.service").is_some_and(|state| state == "loaded");

    let mut issues = Vec::new();
    if !local_ota_dir.is_dir() {
        issues.push("local_ota_dir_missing".to_string());
    }
    if active_slot.is_none() {
        issues.push("active_slot_marker_missing".to_string());
    }
    if reserve_slot.is_none() {
        issues.push("reserve_slot_marker_missing".to_string());
    }
    if !updater_socket.exists() {
        issues.push("updater_socket_missing".to_string());
    }
    if !boot_device_exists {
        issues.push("boot_device_missing".to_string());
    }
    if !confirm_service_installed {
        issues.push("ota_confirm_service_missing".to_string());
    }

    let os_image_ready = issues.is_empty();
    Ok(OtaReport {
        local_ota_dir: LOCAL_OTA_DIR.to_string(),
        local_ota_dir_exists: local_ota_dir.is_dir(),
        active_slot,
        reserve_slot,
        updater_socket: UPDATER_SOCKET.to_string(),
        updater_socket_exists: updater_socket.exists(),
        boot_mount,
        boot_device,
        boot_device_exists,
        confirm_service_installed,
        os_image_ready,
        issues,
    })
}

fn collect_services() -> Result<Vec<ServiceReport>> {
    let mut reports = Vec::new();
    for unit in EXPECTED_SERVICES {
        let output = Command::new("systemctl")
            .args(["show", unit, "--property=ActiveState", "--property=SubState", "--property=MainPID"])
            .output()
            .with_context(|| format!("failed to inspect service {unit}"))?;
        let values = String::from_utf8_lossy(&output.stdout);
        let active_state = systemctl_property(&values, "ActiveState").unwrap_or("unknown").to_string();
        let sub_state = systemctl_property(&values, "SubState").unwrap_or("unknown").to_string();
        let main_pid = systemctl_property(&values, "MainPID").unwrap_or("0").parse::<u32>().ok().filter(|pid| *pid > 0);
        let exec_start = collect_service_exec_start(unit);
        let uses_managed_bin = exec_start.iter().any(|line| line.contains("/var/lib/helios/bin/"));
        let recent_errors = collect_recent_service_errors(unit);
        reports.push(ServiceReport { unit: (*unit).to_string(), active_state, sub_state, main_pid, exec_start, uses_managed_bin, recent_errors });
    }
    Ok(reports)
}

fn service_is_healthy(service: &ServiceReport) -> bool {
    service.active_state == "active" || successful_inactive_oneshot(service)
}

fn successful_inactive_oneshot(service: &ServiceReport) -> bool {
    service.unit == "helios-set-governor.service" && service.active_state == "inactive" && service.sub_state == "dead" && service.main_pid.is_none()
}

fn systemctl_property<'a>(values: &'a str, name: &str) -> Option<&'a str> {
    let prefix = format!("{name}=");
    values.lines().find_map(|line| line.strip_prefix(&prefix).map(str::trim))
}

fn systemd_unit_load_state(unit: &str) -> Option<String> {
    let output = Command::new("systemctl").args(["show", unit, "--property=LoadState"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let values = String::from_utf8_lossy(&output.stdout);
    systemctl_property(&values, "LoadState").map(ToOwned::to_owned)
}

fn read_trimmed(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).ok().map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
}

fn boot_device_from_layout() -> Option<PathBuf> {
    let layout = parse_shell_env_file(Path::new(STORAGE_LAYOUT_ENV));
    let boot_label = layout.get("HELIOS_LAYOUT_BOOT_LABEL").filter(|value| !value.is_empty()).cloned().unwrap_or_else(|| "BOOT".to_string());
    let boot_partition = layout.get("HELIOS_LAYOUT_BOOT_PARTITION").and_then(|value| value.parse::<u32>().ok()).unwrap_or(1);
    let labeled = PathBuf::from(format!("/dev/disk/by-label/{boot_label}"));
    if labeled.exists() {
        return fs::canonicalize(&labeled).ok().or(Some(labeled));
    }

    let data_source = mount_source(Path::new(LOCAL_OTA_DIR).parent().unwrap_or(Path::new("/var/lib/helios")))?;
    let base = partition_base(&data_source);
    Some(partition_device(&base, boot_partition))
}

fn parse_shell_env_file(path: &Path) -> std::collections::BTreeMap<String, String> {
    let Ok(contents) = fs::read_to_string(path) else {
        return std::collections::BTreeMap::new();
    };
    contents
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            let (key, value) = trimmed.split_once('=')?;
            Some((key.trim().to_string(), value.trim().trim_matches('\'').trim_matches('"').to_string()))
        })
        .collect()
}

fn mount_source(path: &Path) -> Option<PathBuf> {
    let target = path.display().to_string();
    fs::read_to_string("/proc/mounts").ok()?.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let source = parts.next()?;
        let mountpoint = parts.next()?;
        if mountpoint == target { Some(PathBuf::from(source)) } else { None }
    })
}

fn partition_base(device: &Path) -> PathBuf {
    let dev = device.display().to_string();
    if let Some(base) = strip_partition_suffix(&dev) { PathBuf::from(base) } else { device.to_path_buf() }
}

fn strip_partition_suffix(device: &str) -> Option<String> {
    if let Some(prefix) = device.strip_prefix("/dev/") {
        if let Some(index) = prefix.rfind('p') {
            let (base, suffix) = prefix.split_at(index);
            if suffix[1..].chars().all(|ch| ch.is_ascii_digit()) && base.chars().last().is_some_and(|ch| ch.is_ascii_digit()) {
                return Some(format!("/dev/{base}"));
            }
        }
        let trimmed = prefix.trim_end_matches(|ch: char| ch.is_ascii_digit());
        if trimmed.len() != prefix.len() {
            return Some(format!("/dev/{trimmed}"));
        }
    }
    None
}

fn partition_device(base: &Path, partition: u32) -> PathBuf {
    let base = base.display().to_string();
    let suffix = if base.chars().last().is_some_and(|ch| ch.is_ascii_digit()) { format!("p{partition}") } else { partition.to_string() };
    PathBuf::from(format!("{base}{suffix}"))
}

fn collect_service_exec_start(unit: &str) -> Vec<String> {
    let output = match Command::new("systemctl").args(["cat", unit, "--no-pager"]).output() {
        Ok(output) if output.status.success() => output,
        _ => return Vec::new(),
    };
    String::from_utf8_lossy(&output.stdout).lines().filter_map(|line| line.strip_prefix("ExecStart=").map(ToOwned::to_owned)).collect()
}

fn collect_recent_service_errors(unit: &str) -> Vec<String> {
    let output = match Command::new("journalctl").args(["-u", unit, "-b", "--no-pager", "-n", "40", "-o", "cat"]).output() {
        Ok(output) if output.status.success() => output,
        _ => return Vec::new(),
    };
    let mut lines = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && (line.contains("Error:")
                    || line.contains("error while loading shared libraries")
                    || line.contains("Failed ")
                    || line.contains("Failed:")
                    || line.contains("Failed at step")
                    || line.contains("not found")
                    || line.contains("Permission denied")
                    || line.contains("Start request repeated too quickly"))
        })
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if lines.len() > 8 {
        lines = lines.split_off(lines.len() - 8);
    }
    lines
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    fs::metadata(path).ok().is_some_and(|metadata| metadata.is_file() && executable_mode(&metadata))
}

#[cfg(unix)]
fn executable_mode(metadata: &fs::Metadata) -> bool {
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(unix)]
fn mode_string(metadata: &fs::Metadata) -> String {
    format!("{:04o}", metadata.permissions().mode() & 0o7777)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

#[cfg(not(unix))]
fn executable_mode(metadata: &fs::Metadata) -> bool {
    metadata.is_file()
}

#[cfg(not(unix))]
fn mode_string(_metadata: &fs::Metadata) -> String {
    "unknown".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service(unit: &str, active_state: &str, sub_state: &str, main_pid: Option<u32>) -> ServiceReport {
        ServiceReport {
            unit: unit.to_string(),
            active_state: active_state.to_string(),
            sub_state: sub_state.to_string(),
            main_pid,
            exec_start: Vec::new(),
            uses_managed_bin: false,
            recent_errors: Vec::new(),
        }
    }

    #[test]
    fn parses_systemctl_named_properties_independent_of_order() {
        let values = "MainPID=578\nActiveState=active\nSubState=running\n";

        assert_eq!(systemctl_property(values, "ActiveState"), Some("active"));
        assert_eq!(systemctl_property(values, "SubState"), Some("running"));
        assert_eq!(systemctl_property(values, "MainPID"), Some("578"));
    }

    #[test]
    fn active_services_are_healthy() {
        assert!(service_is_healthy(&service("helios-api.service", "active", "running", Some(748))));
    }

    #[test]
    fn successful_governor_oneshot_is_healthy_after_exit() {
        assert!(service_is_healthy(&service("helios-set-governor.service", "inactive", "dead", None)));
    }

    #[test]
    fn inactive_long_running_services_are_not_healthy() {
        assert!(!service_is_healthy(&service("helios-api.service", "inactive", "dead", None)));
    }
}
