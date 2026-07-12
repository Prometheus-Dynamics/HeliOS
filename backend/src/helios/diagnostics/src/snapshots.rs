use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::Utc;

use crate::archive::{ensure_dir, tar_gz_dir, try_glob_copy, write_bytes, write_str};
use crate::checks;
use crate::cmd::{CmdResult, CmdSpec, run_cmd};
use crate::config::DiagnosticsConfig;
use crate::model::SnapshotSummary;

pub fn collect_failure_snapshot(config: &DiagnosticsConfig, unit: &str, trigger: &str) -> Result<PathBuf> {
    let base_dir = resolve_base_dir(config)?;
    ensure_dir(&base_dir).context("failed to create diagnostics base dir")?;
    let mut config = config.clone();
    config.base_dir = base_dir.clone();
    prune_old_snapshots(&config)?;

    let report = checks::collect_health_report(&config)?;
    let id = format!("{}-{}-{}", Utc::now().format("%Y%m%d-%H%M%S"), trigger, sanitize_name(unit));
    let out_dir = config.base_dir.join(&id);
    ensure_dir(&out_dir)?;
    let commands = collect_command_outputs(&config, &out_dir, Some(unit))?;
    collect_file_copies(&out_dir)?;

    let summary = SnapshotSummary { bundle_dir: out_dir.display().to_string(), archive: None, trigger: trigger.to_string(), unit: Some(unit.to_string()), generated_at: Utc::now(), commands };
    let summary_path = out_dir.join("summary.json");
    let payload = serde_json::to_vec_pretty(&serde_json::json!({
        "trigger": trigger,
        "unit": unit,
        "report": report,
        "summary": summary,
    }))?;
    write_bytes(&summary_path, &payload).with_context(|| format!("failed to write diagnostics snapshot to {}", summary_path.display()))?;

    if config.snapshot_tar {
        let archive = config.base_dir.join(format!("{id}.tar.gz"));
        tar_gz_dir(&out_dir, &archive)?;
        let _ = fs::remove_dir_all(&out_dir);
        return Ok(archive);
    }

    Ok(summary_path)
}

fn resolve_base_dir(config: &DiagnosticsConfig) -> Result<PathBuf> {
    if ensure_dir(&config.base_dir).is_ok() {
        return Ok(config.base_dir.clone());
    }
    let fallback = std::env::temp_dir().join("helios-diagnostics");
    ensure_dir(&fallback)?;
    Ok(fallback)
}

fn collect_command_outputs(config: &DiagnosticsConfig, out_dir: &Path, unit: Option<&str>) -> Result<Vec<CmdResult>> {
    let commands_dir = out_dir.join("commands");
    ensure_dir(&commands_dir)?;
    let mut results = Vec::new();

    let mut specs = vec![
        ("uname", "uname", vec!["-a"]),
        ("os-release", "cat", vec!["/etc/os-release"]),
        ("cmdline", "cat", vec!["/proc/cmdline"]),
        ("uptime", "uptime", vec![]),
        ("date", "date", vec![]),
        ("df", "df", vec!["-h"]),
        ("mount", "mount", vec![]),
        ("free", "free", vec!["-h"]),
        ("ps", "ps", vec!["-ef"]),
        ("systemd-failed", "systemctl", vec!["list-units", "--failed", "--no-pager"]),
        ("journalctl-boot", "journalctl", vec!["-b", "-n", "2000", "--no-pager", "-o", "short-precise"]),
        ("ip-addr", "ip", vec!["addr"]),
        ("ip-route", "ip", vec!["route"]),
        ("dmesg", "dmesg", vec!["-T"]),
    ];

    if let Some(unit) = unit {
        specs.push(("unit-status", "systemctl", vec!["status", unit, "--no-pager", "--full"]));
        specs.push(("unit-journal", "journalctl", vec!["-u", unit, "-b", "-n", "800", "--no-pager", "-o", "short-precise"]));
    }

    for (name, program, args) in specs {
        let arg_refs: Vec<&str> = args.to_vec();
        let spec = CmdSpec { name, program, args: &arg_refs, cwd: None, max_bytes: config.max_cmd_bytes };
        let (result, stdout, stderr) = run_cmd(&spec)?;
        write_bytes(&commands_dir.join(format!("{name}.stdout")), &stdout)?;
        write_bytes(&commands_dir.join(format!("{name}.stderr")), &stderr)?;
        results.push(result);
    }

    Ok(results)
}

fn collect_file_copies(out_dir: &Path) -> Result<()> {
    let files_out = out_dir.join("files");
    let _ = try_glob_copy(
        &[
            "/etc/hostname",
            "/etc/os-release",
            "/var/lib/helios/hostname",
            "/var/lib/helios/team",
            "/var/lib/helios/nt4.json",
            "/var/lib/helios/peers.json",
            "/var/lib/helios/usb-power.env",
            "/var/lib/helios/leds.toml",
            "/var/lib/helios/led-animations.json",
            "/var/lib/helios/sensors.toml",
            "/var/lib/helios/fan.toml",
            "/var/lib/helios/networkd/*.network",
        ],
        &files_out,
    )?;
    write_str(&out_dir.join("README.txt"), "Helios diagnostics snapshot bundle\n")?;
    Ok(())
}

fn prune_old_snapshots(config: &DiagnosticsConfig) -> Result<()> {
    let mut entries = Vec::new();
    if !config.base_dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(&config.base_dir)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        let modified = metadata.modified().ok();
        entries.push((entry.path(), modified));
    }
    entries.sort_by_key(|(_, modified)| *modified);
    let keep = config.snapshot_keep;
    if entries.len() <= keep {
        return Ok(());
    }
    let remove_count = entries.len().saturating_sub(keep);
    for (path, _) in entries.into_iter().take(remove_count) {
        if path.is_dir() {
            let _ = fs::remove_dir_all(&path);
        } else {
            let _ = fs::remove_file(&path);
        }
    }
    Ok(())
}

fn sanitize_name(input: &str) -> String {
    input.chars().map(|ch| if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_' { ch } else { '_' }).collect()
}
