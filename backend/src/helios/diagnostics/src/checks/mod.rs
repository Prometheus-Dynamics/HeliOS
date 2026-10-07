use std::collections::BTreeSet;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use chrono::Utc;

use crate::config::DiagnosticsConfig;
use crate::model::{
    CameraReport, DynamicLinkReport, ExecutableFileReport, HealthReport, HealthStatus, IdentityReport, JournalReport, LoadAverageReport, MemoryReport, MountReport, OrionReport, PluginReport,
    ProcessRuntimeReport, RuntimeReport, ServiceReport, UpdateReport,
};

/// Service binaries on the read-only root.
const EXPECTED_BINARIES: &[&str] = &["/usr/bin/orion-node", "/usr/bin/orionctl", "/usr/bin/helios-engine", "/usr/bin/helios-peripherals", "/usr/bin/helios-api"];

const EXPECTED_SERVICES: &[&str] = &["orion-node.service", "helios-engine.service", "helios-peripherals.service", "helios-api.service", "sshd.service", "helios-set-governor.service"];

const EXPECTED_EXECUTABLE_FILES: &[&str] = &["/usr/lib/helios/data-setup", "/opt/set-governor.sh", "/opt/helios-update-issue.sh", "/etc/pd-device/update-health", "/etc/pd-device/update.d/pre-reboot"];
const PD_UPDATE_TOOL: &str = "/usr/lib/pd-device/update";
const PD_UPDATE_HEALTH: &str = "/etc/pd-device/update-health";
const SSH_HOST_KEYS: &[&str] = &["ssh_host_ecdsa_key", "ssh_host_ed25519_key", "ssh_host_rsa_key"];

pub fn collect_health_report(config: &DiagnosticsConfig) -> Result<HealthReport> {
    let root = collect_mount_report(Path::new("/"))?;
    let data = collect_mount_report(&config.data_mount)?;
    let writable_store = collect_mount_report(&config.writable_store_mount)?;
    let journal = collect_journal_report(config, &data)?;
    let identity = collect_identity_report(config);
    let camera = collect_camera_report(config)?;
    let runtime = collect_runtime_report()?;
    let executable_files = collect_executable_files();
    let dynamic_links = EXPECTED_BINARIES.iter().map(|binary| inspect_dynamic_links(Path::new(binary))).collect::<Vec<_>>();
    let plugins = collect_plugins(&config.plugin_dir)?;
    let orion = collect_orion(&config.orion_run_dir);
    let update = collect_update_report(&config.update_status_path);
    let services = collect_services()?;
    let data_is_tmpfs = data.fs_type.as_deref() == Some("tmpfs");
    let store_on_data = on_mount(&writable_store, &data);

    let mut status = HealthStatus::Ok;
    if !data.mounted
        || data_is_tmpfs
        || !store_on_data
        || executable_files.iter().any(|file| !file.exists || !file.executable)
        || dynamic_links.iter().any(|link| !link.ok)
        || !orion.control_socket
        || !orion.control_stream_socket
        || !journal.mounted
        || !journal.on_writable_store
        || services.iter().any(|service| service.active_state == "failed")
        || services.iter().any(|service| service.unit == "orion-node.service" && !service_is_healthy(service))
    {
        status = HealthStatus::Failed;
    } else if !journal.has_files
        || !identity.machine_id_persisted
        || !identity.ssh_host_keys_persisted
        || (camera.startup_preset_declares_camera && camera.discovered_camera_resources == 0)
        || !update.issues.is_empty()
        || runtime.processes.iter().any(|process| process.process_count > 1)
        || services.iter().any(|service| !service_is_healthy(service))
    {
        status = HealthStatus::Degraded;
    }

    Ok(HealthReport { generated_at: Utc::now(), status, root, data, writable_store, journal, identity, camera, runtime, executable_files, dynamic_links, plugins, orion, update, services })
}

/// `mount` is bind-mounted from `store` (findmnt prints the source as `/dev/mmcblk0p7[/helios]`).
fn on_mount(mount: &MountReport, store: &MountReport) -> bool {
    match (&mount.source, &store.source) {
        (Some(source), Some(store_source)) => mount.mounted && (source == store_source || source.starts_with(&format!("{store_source}["))),
        _ => false,
    }
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

fn collect_journal_report(config: &DiagnosticsConfig, data: &MountReport) -> Result<JournalReport> {
    let mount = collect_mount_report(&config.journal_mount)?;
    let on_writable_store = on_mount(&mount, data) && data.fs_type.as_deref() != Some("tmpfs");
    let has_files = mount.mounted && journal_has_files(&config.journal_mount)?;
    Ok(JournalReport { path: mount.path, mounted: mount.mounted, source: mount.source, on_writable_store, has_files })
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
    let persistent = read_trimmed(&config.persistent_machine_id_path);
    let machine_id_persisted = persistent.is_some() && read_trimmed(&config.machine_id_path) == persistent;
    let ssh_host_keys_persisted = SSH_HOST_KEYS.iter().all(|name| config.ssh_host_key_dir.join(name).is_file() && config.ssh_host_key_dir.join(format!("{name}.pub")).is_file());
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

fn collect_update_report(status_path: &Path) -> UpdateReport {
    let status = fs::read_to_string(status_path).ok().and_then(|text| serde_json::from_str::<serde_json::Value>(text.lines().next().unwrap_or_default()).ok());
    let field = |key: &str| status.as_ref().and_then(|s| s.get(key)).and_then(|v| v.as_str()).map(str::to_string).filter(|v| !v.is_empty());
    let tool_installed = is_executable(Path::new(PD_UPDATE_TOOL));
    let confirm_service_loaded = systemd_unit_load_state("pd-device-update-confirm.service").is_some_and(|state| state == "loaded");
    let health_check_installed = is_executable(Path::new(PD_UPDATE_HEALTH));
    let slot_active = field("slot_active");
    let ab_layout = matches!(slot_active.as_deref(), Some("A" | "B"));
    let state = field("state");
    let mut issues = Vec::new();
    if !tool_installed {
        issues.push("update_tool_missing".to_string());
    }
    if !confirm_service_loaded {
        issues.push("update_confirm_service_missing".to_string());
    }
    if !health_check_installed {
        issues.push("update_health_check_missing".to_string());
    }
    if status.is_none() {
        issues.push("update_status_missing".to_string());
    } else if !ab_layout {
        issues.push("not_on_ab_layout".to_string());
    }
    if matches!(state.as_deref(), Some("error" | "rolled-back")) {
        issues.push(format!("last_update_{}", state.as_deref().unwrap_or_default()));
    }
    UpdateReport {
        tool_installed,
        confirm_service_loaded,
        health_check_installed,
        state,
        slot_active,
        slot_staged: field("slot_staged"),
        version_active: field("version_active"),
        error: field("error"),
        ab_layout,
        issues,
    }
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
        let recent_errors = collect_recent_service_errors(unit);
        reports.push(ServiceReport { unit: (*unit).to_string(), active_state, sub_state, main_pid, exec_start, recent_errors });
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
        ServiceReport { unit: unit.to_string(), active_state: active_state.to_string(), sub_state: sub_state.to_string(), main_pid, exec_start: Vec::new(), recent_errors: Vec::new() }
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
