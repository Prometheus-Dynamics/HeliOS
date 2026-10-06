//! Facts the kernel and systemd own: CPU load, temperature, disk, processes, units and the
//! journal. Orion's host metrics cover memory, load and uptime; CPU utilisation and temperature
//! are read here until Orion reports them.

use std::{
    collections::BTreeMap,
    path::Path,
    process::Stdio,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use tokio::process::Command;

use crate::error::{ApiError, ApiResult};

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or_default()
}

pub fn read_trimmed(path: impl AsRef<Path>) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|text| text.trim_matches(|c: char| c.is_whitespace() || c == '\0').to_string()).filter(|text| !text.is_empty())
}

/// `KEY=value` lines (os-release, env files).
pub fn parse_env(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|line| line.trim().split_once('='))
        .filter(|(key, _)| !key.starts_with('#'))
        .map(|(key, value)| (key.trim().to_string(), value.trim().trim_matches('"').trim_matches('\'').to_string()))
        .collect()
}

pub fn os_release() -> BTreeMap<String, String> {
    std::fs::read_to_string("/etc/os-release").map(|text| parse_env(&text)).unwrap_or_default()
}

/// One `/proc/stat` CPU line: busy and total jiffies.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CpuTimes {
    pub busy: u64,
    pub total: u64,
}

pub fn parse_proc_stat(text: &str) -> (CpuTimes, Vec<CpuTimes>) {
    let mut all = CpuTimes::default();
    let mut cores = Vec::new();
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let Some(name) = fields.next() else { continue };
        if !name.starts_with("cpu") {
            continue;
        }
        let values: Vec<u64> = fields.filter_map(|v| v.parse().ok()).collect();
        if values.len() < 4 {
            continue;
        }
        let idle = values[3] + values.get(4).copied().unwrap_or(0);
        let total: u64 = values.iter().take(8).sum();
        let times = CpuTimes { busy: total.saturating_sub(idle), total };
        if name == "cpu" {
            all = times;
        } else {
            cores.push(times);
        }
    }
    (all, cores)
}

fn fraction(prev: CpuTimes, next: CpuTimes) -> f64 {
    let total = next.total.saturating_sub(prev.total);
    if total == 0 {
        return 0.0;
    }
    next.busy.saturating_sub(prev.busy) as f64 / total as f64
}

/// CPU utilisation between successive calls (the first call measures since boot).
#[derive(Default)]
pub struct CpuSampler {
    last: Mutex<Option<(CpuTimes, Vec<CpuTimes>)>>,
}

impl CpuSampler {
    pub fn sample(&self) -> Option<(f64, Vec<f64>)> {
        let text = std::fs::read_to_string("/proc/stat").ok()?;
        let next = parse_proc_stat(&text);
        let mut last = self.last.lock().ok()?;
        let prev = last.clone().unwrap_or_default();
        let total = fraction(prev.0, next.0);
        let cores = next.1.iter().enumerate().map(|(i, core)| fraction(prev.1.get(i).copied().unwrap_or_default(), *core)).collect();
        *last = Some(next);
        Some((total, cores))
    }
}

/// Hottest thermal zone, °C.
pub fn temperature_c() -> Option<f64> {
    let zones = std::fs::read_dir("/sys/class/thermal").ok()?;
    zones
        .filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("thermal_zone"))
        .filter_map(|entry| read_trimmed(entry.path().join("temp")))
        .filter_map(|text| text.parse::<f64>().ok())
        .map(|milli| milli / 1000.0)
        .reduce(f64::max)
}

/// Raspberry Pi firmware throttle flags (`get_throttled`), when the firmware exposes them.
pub fn throttled() -> Option<u32> {
    let text = read_trimmed("/sys/devices/platform/soc/soc:firmware/get_throttled")?;
    u32::from_str_radix(text.trim_start_matches("0x"), 16).ok()
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DiskUsage {
    pub path: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub available_bytes: u64,
}

pub fn disk_usage(path: &str) -> Option<DiskUsage> {
    let c_path = std::ffi::CString::new(path).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: `c_path` is a valid NUL-terminated string and `stat` is a properly sized buffer
    // that statvfs fully initialises when it returns 0.
    #[allow(unsafe_code)]
    let stat = unsafe {
        if libc::statvfs(c_path.as_ptr(), stat.as_mut_ptr()) != 0 {
            return None;
        }
        stat.assume_init()
    };
    let block = stat.f_frsize as u64;
    let total = stat.f_blocks as u64 * block;
    let free = stat.f_bfree as u64 * block;
    Some(DiskUsage { path: path.to_string(), total_bytes: total, used_bytes: total.saturating_sub(free), available_bytes: stat.f_bavail as u64 * block })
}

// --- systemd ---------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UnitStatus {
    pub unit: String,
    pub active_state: String,
    pub sub_state: String,
    pub main_pid: Option<u32>,
    pub memory_bytes: Option<u64>,
    pub restarts: Option<u64>,
}

pub fn parse_systemctl_show(unit: &str, text: &str) -> UnitStatus {
    let props = parse_env(text);
    let number = |key: &str| props.get(key).and_then(|value| value.parse::<u64>().ok());
    UnitStatus {
        unit: unit.to_string(),
        active_state: props.get("ActiveState").cloned().unwrap_or_else(|| "unknown".into()),
        sub_state: props.get("SubState").cloned().unwrap_or_else(|| "unknown".into()),
        main_pid: number("MainPID").filter(|pid| *pid > 0).map(|pid| pid as u32),
        // systemd reports "[not set]" or u64::MAX when accounting is off.
        memory_bytes: number("MemoryCurrent").filter(|bytes| *bytes != u64::MAX),
        restarts: number("NRestarts"),
    }
}

pub async fn unit_status(unit: &str) -> ApiResult<UnitStatus> {
    let output = Command::new("systemctl")
        .args(["show", unit, "--property=ActiveState,SubState,MainPID,MemoryCurrent,NRestarts"])
        .stdin(Stdio::null())
        .output()
        .await
        .map_err(|error| ApiError::backend(format!("systemctl is not available: {error}")))?;
    if !output.status.success() {
        return Err(ApiError::backend(format!("systemctl show {unit} failed: {}", String::from_utf8_lossy(&output.stderr).trim())));
    }
    Ok(parse_systemctl_show(unit, &String::from_utf8_lossy(&output.stdout)))
}

pub async fn systemctl(args: &[&str]) -> ApiResult<()> {
    let output = Command::new("systemctl").args(args).stdin(Stdio::null()).output().await.map_err(|error| ApiError::backend(format!("systemctl is not available: {error}")))?;
    if output.status.success() { Ok(()) } else { Err(ApiError::backend(format!("systemctl {} failed: {}", args.join(" "), String::from_utf8_lossy(&output.stderr).trim()))) }
}

// --- journal ---------------------------------------------------------------

/// One journal entry, in the field names Atlas's log reader accepts.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LogLine {
    pub at_ms: u64,
    pub level: &'static str,
    pub unit: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
}

fn level_for(priority: Option<u8>) -> &'static str {
    match priority {
        Some(0..=3) => "error",
        Some(4) => "warn",
        Some(7) => "debug",
        _ => "info",
    }
}

/// A `journalctl -o json` line.
pub fn parse_journal_line(line: &str) -> Option<LogLine> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let text = |key: &str| value.get(key).and_then(|v| v.as_str()).map(str::to_string);
    let message = match value.get("MESSAGE") {
        Some(serde_json::Value::String(text)) => text.clone(),
        // Non-UTF-8 messages come as byte arrays.
        Some(serde_json::Value::Array(bytes)) => String::from_utf8_lossy(&bytes.iter().filter_map(|b| b.as_u64()).map(|b| b as u8).collect::<Vec<_>>()).into_owned(),
        _ => return None,
    };
    let at_ms = text("__REALTIME_TIMESTAMP").and_then(|us| us.parse::<u64>().ok()).map(|us| us / 1000).unwrap_or_default();
    let unit = text("_SYSTEMD_UNIT").or_else(|| text("SYSLOG_IDENTIFIER")).unwrap_or_else(|| "kernel".into());
    let priority = text("PRIORITY").and_then(|p| p.parse().ok());
    let pid = text("_PID").and_then(|p| p.parse().ok());
    Some(LogLine { at_ms, level: level_for(priority), unit: unit.trim_end_matches(".service").to_string(), message, pid })
}

#[derive(Debug, Clone, Default)]
pub struct LogQuery {
    pub unit: Option<String>,
    pub lines: usize,
    pub since_ms: Option<u64>,
    /// Highest syslog priority included (3 = errors, 4 = warnings, 6 = info).
    pub max_priority: Option<u8>,
}

pub fn journal_args(query: &LogQuery, follow: bool) -> Vec<String> {
    let mut args = vec!["--no-pager".to_string(), "-o".into(), "json".into(), "-n".into(), query.lines.to_string()];
    if follow {
        args.push("-f".into());
    }
    if let Some(unit) = &query.unit {
        args.push("-u".into());
        args.push(unit.clone());
    }
    if let Some(since) = query.since_ms {
        args.push(format!("--since=@{}", since / 1000));
    }
    if let Some(priority) = query.max_priority {
        args.push(format!("--priority={priority}"));
    }
    args
}

pub async fn journal(query: &LogQuery) -> ApiResult<Vec<LogLine>> {
    let output = Command::new("journalctl").args(journal_args(query, false)).stdin(Stdio::null()).output().await.map_err(|error| ApiError::backend(format!("journalctl is not available: {error}")))?;
    if !output.status.success() {
        return Err(ApiError::backend(format!("journalctl failed: {}", String::from_utf8_lossy(&output.stderr).trim())));
    }
    Ok(String::from_utf8_lossy(&output.stdout).lines().filter_map(parse_journal_line).collect())
}

// --- processes -------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    /// The systemd unit that owns it, when it belongs to one.
    pub unit: Option<String>,
    pub state: String,
    pub threads: u64,
    pub rss_bytes: u64,
    pub nice: i64,
    /// CPU time used since start, milliseconds.
    pub cpu_time_ms: u64,
    pub started_after_boot_ms: u64,
    pub cpus_allowed: Option<String>,
}

fn clock_ticks() -> u64 {
    // SAFETY: sysconf has no preconditions.
    #[allow(unsafe_code)]
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if ticks > 0 { ticks as u64 } else { 100 }
}

/// Parse `/proc/<pid>/stat` (the name is in parentheses and may contain spaces).
pub fn parse_pid_stat(text: &str) -> Option<(String, String, u64, i64, u64, u64)> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    let name = text[open + 1..close].to_string();
    let rest: Vec<&str> = text[close + 1..].split_whitespace().collect();
    // Fields after the name: state(0) ... utime(11) stime(12) ... nice(16) num_threads(17) ... starttime(19)
    let state = rest.first()?.to_string();
    let utime: u64 = rest.get(11)?.parse().ok()?;
    let stime: u64 = rest.get(12)?.parse().ok()?;
    let nice: i64 = rest.get(16)?.parse().ok()?;
    let threads: u64 = rest.get(17)?.parse().ok()?;
    let start: u64 = rest.get(19)?.parse().ok()?;
    Some((name, state, utime + stime, nice, threads, start))
}

fn unit_of(pid: u32) -> Option<String> {
    let cgroup = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    cgroup.lines().filter_map(|line| line.rsplit('/').next()).find(|segment| segment.ends_with(".service")).map(str::to_string)
}

pub fn processes() -> Vec<ProcessInfo> {
    let ticks = clock_ticks();
    let page = 4096u64;
    let Ok(entries) = std::fs::read_dir("/proc") else { return Vec::new() };
    let mut out = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else { continue };
        let Some(stat) = read_trimmed(format!("/proc/{pid}/stat")) else { continue };
        let Some((name, state, cpu_ticks, nice, threads, start)) = parse_pid_stat(&stat) else { continue };
        let rss_pages = read_trimmed(format!("/proc/{pid}/statm")).and_then(|statm| statm.split_whitespace().nth(1).and_then(|v| v.parse::<u64>().ok())).unwrap_or(0);
        let cpus_allowed =
            std::fs::read_to_string(format!("/proc/{pid}/status")).ok().and_then(|status| status.lines().find_map(|line| line.strip_prefix("Cpus_allowed_list:").map(|v| v.trim().to_string())));
        out.push(ProcessInfo {
            pid,
            name,
            unit: unit_of(pid),
            state,
            threads,
            rss_bytes: rss_pages * page,
            nice,
            cpu_time_ms: cpu_ticks * 1000 / ticks,
            started_after_boot_ms: start * 1000 / ticks,
            cpus_allowed,
        });
    }
    out.sort_by_key(|process| process.pid);
    out
}

pub fn signal_number(name: &str) -> Option<i32> {
    match name.trim_start_matches("SIG") {
        "TERM" => Some(libc::SIGTERM),
        "KILL" => Some(libc::SIGKILL),
        "STOP" => Some(libc::SIGSTOP),
        "CONT" => Some(libc::SIGCONT),
        "HUP" => Some(libc::SIGHUP),
        "INT" => Some(libc::SIGINT),
        _ => None,
    }
}

pub fn send_signal(pid: u32, signal: i32) -> ApiResult<()> {
    if pid <= 1 {
        return Err(ApiError::bad_request("refusing to signal pid 0 or 1"));
    }
    // SAFETY: kill(2) has no memory-safety preconditions.
    #[allow(unsafe_code)]
    let result = unsafe { libc::kill(pid as libc::pid_t, signal) };
    if result == 0 {
        Ok(())
    } else {
        let error = std::io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::ESRCH) => Err(ApiError::not_found(format!("no process {pid}"))),
            _ => Err(ApiError::internal(format!("kill({pid}) failed: {error}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proc_stat_fractions() {
        let a = parse_proc_stat("cpu  100 0 100 800 0 0 0 0 0 0\ncpu0 50 0 50 400 0 0 0 0\ncpu1 50 0 50 400 0 0 0 0\n");
        let b = parse_proc_stat("cpu  200 0 200 1400 0 0 0 0 0 0\ncpu0 150 0 50 600 0 0 0 0\ncpu1 50 0 150 800 0 0 0 0\n");
        assert_eq!(a.1.len(), 2);
        assert!((fraction(a.0, b.0) - 0.25).abs() < 1e-9);
        assert!((fraction(a.1[0], b.1[0]) - (100.0 / 300.0)).abs() < 1e-9);
    }

    #[test]
    fn journal_lines_map_to_log_lines() {
        let line = r#"{"__REALTIME_TIMESTAMP":"1700000000123456","_SYSTEMD_UNIT":"helios-engine.service","PRIORITY":"4","MESSAGE":"slow tick","_PID":"42"}"#;
        let parsed = parse_journal_line(line).expect("parsed");
        assert_eq!(parsed.at_ms, 1_700_000_000_123);
        assert_eq!(parsed.unit, "helios-engine");
        assert_eq!(parsed.level, "warn");
        assert_eq!(parsed.pid, Some(42));
        let bytes = r#"{"__REALTIME_TIMESTAMP":"1","SYSLOG_IDENTIFIER":"kernel","MESSAGE":[104,105]}"#;
        assert_eq!(parse_journal_line(bytes).expect("bytes").message, "hi");
    }

    #[test]
    fn journal_args_follow_the_query() {
        let args = journal_args(&LogQuery { unit: Some("helios-api.service".into()), lines: 50, since_ms: Some(10_000), max_priority: Some(4) }, true);
        assert!(args.contains(&"-f".to_string()));
        assert!(args.contains(&"--since=@10".to_string()));
        assert!(args.contains(&"--priority=4".to_string()));
    }

    #[test]
    fn pid_stat_with_spaces_in_name() {
        let stat = "1234 (helios engine) S 1 1234 1234 0 -1 4194560 100 0 0 0 30 20 0 0 20 -2 5 0 777 1000 200";
        let (name, state, cpu, nice, threads, start) = parse_pid_stat(stat).expect("stat");
        assert_eq!(name, "helios engine");
        assert_eq!(state, "S");
        assert_eq!(cpu, 50);
        assert_eq!(nice, -2);
        assert_eq!(threads, 5);
        assert_eq!(start, 777);
    }

    #[test]
    fn systemctl_show_parses_units() {
        let status = parse_systemctl_show("helios-api.service", "ActiveState=active\nSubState=running\nMainPID=812\nMemoryCurrent=[not set]\nNRestarts=2\n");
        assert_eq!(status.active_state, "active");
        assert_eq!(status.main_pid, Some(812));
        assert_eq!(status.memory_bytes, None);
        assert_eq!(status.restarts, Some(2));
    }
}
