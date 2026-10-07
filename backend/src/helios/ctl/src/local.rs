use std::{path::Path, process::Command};

use anyhow::Result;
use helios_diagnostics::{
    collect_health_report,
    config::DiagnosticsConfig,
    model::{HealthReport, HealthStatus, ServiceReport},
};
use heliosctl::pd_update::{self, parse_env_file, read_trimmed};

const EXPECTED_SERVICES: &[&str] = &["orion-node.service", "helios-engine.service", "helios-peripherals.service", "helios-api.service"];

const VERSION_FILE: &str = "/etc/helios/version";
const BUILD_ID_FILE: &str = "/etc/helios/build-id";
const OS_RELEASE_FILE: &str = "/etc/os-release";
const MACHINE_ID_PATH: &str = "/etc/machine-id";
pub fn print_status(config: &DiagnosticsConfig) -> Result<()> {
    let report = collect_health_report(config)?;
    let version = version_summary();
    let update = pd_update::read_status().unwrap_or_default();
    println!("status={}", health_status_str(report.status));
    println!("time={}", report.generated_at.to_rfc3339());
    println!("version={}", version.display);
    println!("build_id={}", version.build_id.unwrap_or_else(|| "-".into()));
    println!("root_device={}", version.root_device.unwrap_or_else(|| "-".into()));
    println!("root_slot={}", update.slot_active.as_deref().unwrap_or("-"));
    println!("data={}", mount_summary(&report.data));
    println!("writable_store={}", mount_summary(&report.writable_store));
    println!("journal={}", journal_summary(&report));
    println!("services={}", summarize_services(&report.services));
    println!("update_state={}", if update.state.is_empty() { "-" } else { update.state.as_str() });
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

pub fn print_storage(config: &DiagnosticsConfig) -> Result<()> {
    let report = collect_health_report(config)?;
    println!("root_mount={}", mount_for("/").unwrap_or_else(|| "-".into()));
    println!("data={}", mount_summary(&report.data));
    println!("writable_store={}", mount_summary(&report.writable_store));
    println!("journal={}", journal_summary(&report));
    for path in ["/data/helios", "/data/helios/orion", "/data/helios/diagnostics", "/data/helios/updates", "/data/journal", "/data/log", "/data/ssh", "/data/root"] {
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
    println!("machine_id={}", read_trimmed(Path::new(MACHINE_ID_PATH)).unwrap_or_else(|| "-".into()));
    println!("persistent_machine_id={}", exists_str(&config.persistent_machine_id_path));
    println!("ssh_host_keys_persisted={}", bool_str(report.identity.ssh_host_keys_persisted));
    println!("ssh_host_key_dir={}", config.ssh_host_key_dir.display());
    println!("hostname={}", read_trimmed(Path::new("/proc/sys/kernel/hostname")).unwrap_or_else(|| "-".into()));
    Ok(())
}

struct VersionSummary {
    display: String,
    build_id: Option<String>,
    root_device: Option<String>,
    root_cmdline: Option<String>,
}

fn version_summary() -> VersionSummary {
    let os_release = parse_env_file(Path::new(OS_RELEASE_FILE));
    let display = os_release.get("PRETTY_NAME").cloned().or_else(|| read_trimmed(Path::new(VERSION_FILE))).unwrap_or_else(|| "unknown".into());
    let build_id = read_trimmed(Path::new(BUILD_ID_FILE));
    let root_cmdline = read_trimmed(Path::new("/proc/cmdline"));
    let root_device = root_cmdline.as_deref().and_then(|cmdline| cmdline.split_whitespace().find(|part| part.starts_with("root="))).map(|value| value.trim_start_matches("root=").to_string());
    VersionSummary { display, build_id, root_device, root_cmdline }
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

fn exists_str(path: &Path) -> &'static str {
    if path.exists() { "yes" } else { "no" }
}

fn bool_str(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}
