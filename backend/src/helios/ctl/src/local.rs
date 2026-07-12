use std::{fs, path::Path, process::Command};

use anyhow::{Context, Result};
use helios_diagnostics::{
    collect_health_report,
    config::DiagnosticsConfig,
    model::{HealthReport, HealthStatus, ServiceReport},
};

const EXPECTED_SERVICES: &[&str] = &["orion-node.service", "helios-engine.service", "helios-peripherals.service", "helios-api.service", "helios-updater.service"];

const OTA_DIR: &str = "/var/lib/helios/ota";
const UPDATER_DIR: &str = "/var/lib/helios/updater";
const VERSION_FILE: &str = "/etc/helios/version";
const BUILD_ID_FILE: &str = "/etc/helios/build-id";
const OS_RELEASE_FILE: &str = "/etc/os-release";
const MACHINE_ID_PATH: &str = "/etc/machine-id";
const PERSISTENT_MACHINE_ID_PATH: &str = "/var/lib/helios/identity/machine-id";
const SSH_DIR: &str = "/etc/ssh";
const PERSISTENT_SSH_DIR: &str = "/var/lib/helios/identity/ssh";
pub fn print_status(config: &DiagnosticsConfig) -> Result<()> {
    let report = collect_health_report(config)?;
    let version = version_summary();
    let update = update_summary()?;
    println!("status={}", health_status_str(report.status));
    println!("time={}", report.generated_at.to_rfc3339());
    println!("version={}", version.display);
    println!("build_id={}", version.build_id.unwrap_or_else(|| "-".into()));
    println!("root_device={}", version.root_device.unwrap_or_else(|| "-".into()));
    println!("root_slot={}", update.active.clone().unwrap_or_else(|| "-".into()));
    println!("reserve_slot={}", update.reserve.clone().unwrap_or_else(|| "-".into()));
    println!("writable_store={}", mount_summary(&report.writable_store));
    println!("journal={}", journal_summary(&report));
    println!("services={}", summarize_services(&report.services));
    println!("updater_phase={}", update.confirm_status.clone().or(update.pending.clone()).unwrap_or_else(|| "idle".into()));
    Ok(())
}

pub fn print_version() -> Result<()> {
    let version = version_summary();
    println!("heliosctl={}", env!("CARGO_PKG_VERSION"));
    println!("helios={}", version.display);
    println!("build_id={}", version.build_id.unwrap_or_else(|| "-".into()));
    println!("kernel={}", read_command_first_line("uname", &["-sr"]).unwrap_or_else(|| "-".into()));
    println!("root_device={}", version.root_device.unwrap_or_else(|| "-".into()));
    println!("root_cmdline={}", version.root_cmdline.unwrap_or_else(|| "-".into()));
    Ok(())
}

pub fn print_update() -> Result<()> {
    let update = update_summary()?;
    println!("ota_dir={OTA_DIR}");
    println!("active={}", update.active.unwrap_or_else(|| "-".into()));
    println!("reserve={}", update.reserve.unwrap_or_else(|| "-".into()));
    println!("pending={}", update.pending.unwrap_or_else(|| "-".into()));
    println!("confirm_request_id={}", update.confirm_request_id.unwrap_or_else(|| "-".into()));
    println!("confirm_status={}", update.confirm_status.unwrap_or_else(|| "-".into()));
    println!("confirm_selector={}", update.confirm_selector.unwrap_or_else(|| "-".into()));
    println!("repartition_request_id={}", update.repartition_request_id.unwrap_or_else(|| "-".into()));
    println!("repartition_status={}", update.repartition_status.unwrap_or_else(|| "-".into()));
    println!("prepared_updates={}", join_or_dash(update.prepared_updates));
    println!("staged_artifacts={}", join_or_dash(update.staged_artifacts));
    println!("updater_service={}", service_state("helios-updater.service").unwrap_or_else(|| "unknown".into()));
    Ok(())
}

pub fn print_storage(config: &DiagnosticsConfig) -> Result<()> {
    let report = collect_health_report(config)?;
    println!("root_mount={}", mount_for("/").unwrap_or_else(|| "-".into()));
    println!("writable_store={}", mount_summary(&report.writable_store));
    println!("journal={}", journal_summary(&report));
    println!("overlay_slots={}", if report.overlay.slot_dirs_present { "root-a,root-b" } else { "missing" });
    println!("overlay_overrides={}", if report.overlay.image_owned_override_count == 0 { "-".into() } else { report.overlay.image_owned_overrides.join(",") });
    for path in [
        "/var/lib/helios/bin",
        "/var/lib/helios/diagnostics",
        "/var/lib/helios/identity",
        "/var/lib/helios/journal",
        "/var/lib/helios/orion",
        "/var/lib/helios/ota",
        "/var/lib/helios/root-overlay",
        "/var/lib/helios/state",
        "/var/lib/helios/updater",
        "/var/log/helios",
    ] {
        println!("size[{path}]={}", du_size(path).unwrap_or_else(|| "-".into()));
    }
    Ok(())
}

pub fn print_services() -> Result<()> {
    for unit in EXPECTED_SERVICES {
        let state = service_state(unit).unwrap_or_else(|| "unknown".into());
        let pid = systemctl_show(unit, "MainPID").unwrap_or_else(|| "-".into());
        println!("{unit} state={state} pid={pid}");
    }
    Ok(())
}

pub fn print_identity(config: &DiagnosticsConfig) -> Result<()> {
    let report = collect_health_report(config)?;
    println!("machine_id_persisted={}", bool_str(report.identity.machine_id_persisted));
    println!("machine_id_path={MACHINE_ID_PATH}");
    println!("machine_id_target={}", symlink_target(Path::new(MACHINE_ID_PATH)).unwrap_or_else(|| "-".into()));
    println!("persistent_machine_id={}", exists_str(Path::new(PERSISTENT_MACHINE_ID_PATH)));
    println!("ssh_host_keys_persisted={}", bool_str(report.identity.ssh_host_keys_persisted));
    for key in ["ssh_host_rsa_key", "ssh_host_rsa_key.pub", "ssh_host_ecdsa_key", "ssh_host_ecdsa_key.pub", "ssh_host_ed25519_key", "ssh_host_ed25519_key.pub"] {
        let live = Path::new(SSH_DIR).join(key);
        let persistent = Path::new(PERSISTENT_SSH_DIR).join(key);
        println!("{key} live={} target={} persistent={}", exists_str(&live), symlink_target(&live).unwrap_or_else(|| "-".into()), exists_str(&persistent));
    }
    Ok(())
}

struct VersionSummary {
    display: String,
    build_id: Option<String>,
    root_device: Option<String>,
    root_cmdline: Option<String>,
}

#[derive(Default)]
struct UpdateSummary {
    active: Option<String>,
    reserve: Option<String>,
    pending: Option<String>,
    confirm_request_id: Option<String>,
    confirm_status: Option<String>,
    confirm_selector: Option<String>,
    repartition_request_id: Option<String>,
    repartition_status: Option<String>,
    prepared_updates: Vec<String>,
    staged_artifacts: Vec<String>,
}

fn version_summary() -> VersionSummary {
    let os_release = parse_env_file(Path::new(OS_RELEASE_FILE));
    let display = os_release.get("PRETTY_NAME").cloned().or_else(|| read_trimmed(Path::new(VERSION_FILE))).unwrap_or_else(|| "unknown".into());
    let build_id = read_trimmed(Path::new(BUILD_ID_FILE));
    let root_cmdline = read_trimmed(Path::new("/proc/cmdline"));
    let root_device = root_cmdline.as_deref().and_then(|cmdline| cmdline.split_whitespace().find(|part| part.starts_with("root="))).map(|value| value.trim_start_matches("root=").to_string());
    VersionSummary { display, build_id, root_device, root_cmdline }
}

fn update_summary() -> Result<UpdateSummary> {
    let mut summary = UpdateSummary {
        active: read_trimmed(Path::new(OTA_DIR).join("active").as_path()),
        reserve: read_trimmed(Path::new(OTA_DIR).join("reserve").as_path()),
        pending: read_trimmed(Path::new(OTA_DIR).join("pending").as_path()),
        ..UpdateSummary::default()
    };

    let confirm_request = parse_env_file(&Path::new(OTA_DIR).join("confirm-request.env"));
    summary.confirm_request_id = confirm_request.get("HELIOS_UPDATE_ID").cloned();

    let confirm_result = parse_env_file(&Path::new(OTA_DIR).join("confirm-result.env"));
    summary.confirm_status = confirm_result.get("HELIOS_UPDATE_CONFIRM_STATUS").cloned();
    summary.confirm_selector = confirm_result.get("HELIOS_UPDATE_CONFIRMED_SELECTOR").cloned();

    let repartition_result = parse_env_file(&Path::new(OTA_DIR).join("repartition-result.env"));
    summary.repartition_request_id = repartition_result.get("HELIOS_REPARTITION_UPDATE_ID").cloned();
    summary.repartition_status = repartition_result.get("HELIOS_REPARTITION_STATUS").cloned();
    if summary.repartition_request_id.is_none() {
        let repartition_request = parse_env_file(&Path::new(OTA_DIR).join("repartition-request.env"));
        summary.repartition_request_id = repartition_request.get("HELIOS_REPARTITION_UPDATE_ID").cloned();
    }

    let prepared_dir = Path::new(UPDATER_DIR).join("prepared");
    summary.prepared_updates = list_json_basenames(&prepared_dir)?;
    let staging_dir = Path::new(UPDATER_DIR).join("staging");
    summary.staged_artifacts = list_dir_names(&staging_dir)?;
    Ok(summary)
}

fn health_status_str(status: HealthStatus) -> &'static str {
    match status {
        HealthStatus::Ok => "ok",
        HealthStatus::Degraded => "degraded",
        HealthStatus::Failed => "failed",
    }
}

fn mount_summary(report: &helios_diagnostics::model::MountReport) -> String {
    if !report.mounted {
        return format!("{} unmounted", report.path);
    }
    format!("{} {} {}", report.path, report.fs_type.as_deref().unwrap_or("-"), report.source.as_deref().unwrap_or("-"))
}

fn journal_summary(report: &HealthReport) -> String {
    format!(
        "{} {} {} on_writable_store={}",
        report.journal.path,
        if report.journal.mounted { "mounted" } else { "unmounted" },
        report.journal.source.as_deref().unwrap_or("-"),
        bool_str(report.journal.on_writable_store)
    )
}

fn summarize_services(services: &[ServiceReport]) -> String {
    services.iter().map(|service| format!("{}={}/{}", service.unit, service.active_state, service.sub_state)).collect::<Vec<_>>().join(",")
}

fn mount_for(path: &str) -> Option<String> {
    let output = Command::new("findmnt").args(["-n", "-o", "SOURCE,FSTYPE,OPTIONS", path]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn du_size(path: &str) -> Option<String> {
    let output = Command::new("du").args(["-sh", path]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).split_whitespace().next().unwrap_or("-").to_string())
}

fn service_state(unit: &str) -> Option<String> {
    let active = systemctl_show(unit, "ActiveState")?;
    let sub = systemctl_show(unit, "SubState")?;
    Some(format!("{active}/{sub}"))
}

fn systemctl_show(unit: &str, property: &str) -> Option<String> {
    let output = Command::new("systemctl").args(["show", unit, "--property", property, "--value"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn read_command_first_line(command: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(command).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout).lines().next().map(|line| line.trim().to_string())
}

fn parse_env_file(path: &Path) -> std::collections::BTreeMap<String, String> {
    let mut values = std::collections::BTreeMap::new();
    let Ok(contents) = fs::read_to_string(path) else {
        return values;
    };
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        values.insert(key.trim().to_string(), value.trim().trim_matches('\'').trim_matches('"').to_string());
    }
    values
}

fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|content| content.trim().to_string()).filter(|value| !value.is_empty())
}

fn symlink_target(path: &Path) -> Option<String> {
    fs::read_link(path).ok().map(|target| target.display().to_string())
}

fn exists_str(path: &Path) -> &'static str {
    if path.exists() { "yes" } else { "no" }
}

fn bool_str(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn join_or_dash(values: Vec<String>) -> String {
    if values.is_empty() { "-".into() } else { values.join(",") }
}

fn list_json_basenames(path: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    if !path.is_dir() {
        return Ok(names);
    }
    for entry in fs::read_dir(path).with_context(|| format!("failed to read {}", path.display()))? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if let Some(stripped) = name.strip_suffix(".json") {
            names.push(stripped.to_string());
        } else {
            names.push(name.into_owned());
        }
    }
    names.sort();
    Ok(names)
}

fn list_dir_names(path: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    if !path.is_dir() {
        return Ok(names);
    }
    for entry in fs::read_dir(path).with_context(|| format!("failed to read {}", path.display()))? {
        let entry = entry?;
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    Ok(names)
}
