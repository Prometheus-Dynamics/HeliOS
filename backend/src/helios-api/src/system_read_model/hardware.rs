use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex as StdMutex, OnceLock, Weak};

use crate::ipc::IpcHandles;
use crate::ws::device::PowerTelemetry;
use crate::ws::device::{CpuThrottleStatus, GpuMemorySample, GpuTelemetry};

pub(super) fn sample_gpu() -> Option<GpuTelemetry> {
    let usage_percent = read_gpu_busy_percent();
    let temperature_c = read_gpu_temperature();
    let frequency_mhz = read_gpu_frequency_mhz();
    let memory = read_gpu_memory();

    if usage_percent.is_none() && temperature_c.is_none() && frequency_mhz.is_none() && memory.is_none() {
        return None;
    }

    Some(GpuTelemetry { usage_percent, temperature_c, frequency_mhz, memory })
}

pub(super) fn read_cpu_temperature_c() -> Option<f32> {
    if let Ok(entries) = fs::read_dir("/sys/class/thermal") {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.file_name().and_then(|name| name.to_str()).map(|name| name.starts_with("thermal_zone")).unwrap_or(false) {
                continue;
            }
            let zone_type = fs::read_to_string(path.join("type")).ok().unwrap_or_default().to_ascii_lowercase();
            if !(zone_type.contains("cpu") || zone_type.contains("soc")) {
                continue;
            }
            let raw = fs::read_to_string(path.join("temp")).ok()?;
            let milli = raw.trim().parse::<i64>().ok()?;
            if milli <= 0 {
                continue;
            }
            return Some(milli as f32 / 1000.0);
        }
    }
    None
}

pub(super) fn read_cpu_throttle_status() -> Option<CpuThrottleStatus> {
    let raw = fs::read_to_string("/sys/devices/platform/soc/firmware/get_throttled").ok()?;
    let trimmed = raw.trim().trim_start_matches("0x");
    let value = u32::from_str_radix(trimmed, 16).ok()?;
    Some(CpuThrottleStatus {
        raw: value,
        undervoltage: (value & 0x1) != 0,
        frequency_capped: (value & (1 << 1)) != 0,
        throttled: (value & (1 << 2)) != 0,
        soft_temp_limit: (value & (1 << 3)) != 0,
        undervoltage_since_boot: (value & (1 << 16)) != 0,
        frequency_capped_since_boot: (value & (1 << 17)) != 0,
        throttled_since_boot: (value & (1 << 18)) != 0,
        soft_temp_limit_since_boot: (value & (1 << 19)) != 0,
    })
}

pub(super) async fn sample_power_from_peripherals(state: Option<&Weak<IpcHandles>>) -> Option<PowerTelemetry> {
    use crate::http::device::power::power_status_from_snapshot;
    use helios_peripherals::dto::SensorScope;

    let handles = state?.upgrade()?;
    let sensors = handles.ensure_sensors().await?;
    let response = tokio::time::timeout(tokio::time::Duration::from_millis(250), sensors.sensor_snapshot(SensorScope::Device)).await.ok()?;
    let response = response.ok()?;
    let snapshot = response.ok()?;
    let status = power_status_from_snapshot(&snapshot);

    Some(PowerTelemetry { watts: status.watts.map(|value| value as f32), volts: status.volts.map(|value| value as f32), amps: status.amps.map(|value| value as f32) })
}

fn read_gpu_busy_percent() -> Option<f32> {
    let drm_dir = Path::new("/sys/class/drm");
    let entries = fs::read_dir(drm_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.file_name().and_then(|name| name.to_str()).map(|name| name.starts_with("card")).unwrap_or(false) {
            continue;
        }
        let device_dir = path.join("device");
        if let Some(val) = read_percent_file(&device_dir.join("gpu_busy_percent")) {
            return Some(val);
        }
        if let Ok(engine_dirs) = fs::read_dir(device_dir.join("engine")) {
            for engine in engine_dirs.flatten() {
                if let Some(val) = read_percent_file(&engine.path().join("busy_percent")) {
                    return Some(val);
                }
            }
        }
    }
    read_gpu_busy_percent_from_stats()
}

#[derive(Debug, Clone, Copy)]
struct GpuStatsSnapshot {
    timestamp: u64,
    jobs: u64,
}

#[derive(Debug, Default, Clone, Copy)]
struct GpuBusyState {
    last: Option<GpuStatsSnapshot>,
}

fn read_gpu_busy_percent_from_stats() -> Option<f32> {
    static STATE: OnceLock<StdMutex<GpuBusyState>> = OnceLock::new();
    let snapshot = read_gpu_stats_snapshot()?;
    let state = STATE.get_or_init(|| StdMutex::new(GpuBusyState::default()));
    let mut guard = state.lock().ok()?;

    let percent = match guard.last {
        None => 0.0,
        Some(prev) => {
            let dt = snapshot.timestamp.saturating_sub(prev.timestamp);
            let dj = snapshot.jobs.saturating_sub(prev.jobs);
            if dt == 0 || dj == 0 { 0.0 } else { 100.0 }
        }
    };

    guard.last = Some(snapshot);
    Some(percent)
}

fn read_gpu_stats_snapshot() -> Option<GpuStatsSnapshot> {
    let drm_dir = Path::new("/sys/class/drm");
    let entries = fs::read_dir(drm_dir).ok()?;
    for entry in entries.flatten() {
        let card_dir = entry.path();
        let file_name = card_dir.file_name().and_then(|name| name.to_str()).unwrap_or("");
        if !file_name.starts_with("card") {
            continue;
        }
        if let Some(snapshot) = parse_gpu_stats_file(&card_dir.join("device/gpu_stats")) {
            return Some(snapshot);
        }
    }
    None
}

fn parse_gpu_stats_file(path: &Path) -> Option<GpuStatsSnapshot> {
    let raw = fs::read_to_string(path).ok()?;
    let mut timestamp: Option<u64> = None;
    let mut jobs_sum: u64 = 0;
    for (idx, line) in raw.lines().enumerate() {
        if idx == 0 {
            continue;
        }
        let mut parts = line.split_whitespace();
        let queue = parts.next()?;
        let ts = parts.next()?.parse::<u64>().ok()?;
        let jobs = parts.next()?.parse::<u64>().ok()?;
        let _runtime = parts.next()?.parse::<u64>().ok()?;
        if matches!(queue, "bin" | "render" | "tfu" | "csd") {
            jobs_sum = jobs_sum.saturating_add(jobs);
        }
        timestamp = Some(timestamp.map_or(ts, |prev| prev.max(ts)));
    }
    Some(GpuStatsSnapshot { timestamp: timestamp?, jobs: jobs_sum })
}

fn read_gpu_temperature() -> Option<f32> {
    let drm_dir = Path::new("/sys/class/drm");
    for entry in fs::read_dir(drm_dir).ok()?.flatten() {
        let card_dir = entry.path();
        if !card_dir.file_name().and_then(|name| name.to_str()).map(|name| name.starts_with("card")).unwrap_or(false) {
            continue;
        }
        if let Ok(hwmon_dirs) = fs::read_dir(card_dir.join("device/hwmon")) {
            for hwmon in hwmon_dirs.flatten() {
                if let Some(raw) = read_integer_file(&hwmon.path().join("temp1_input")) {
                    return Some(raw as f32 / 1000.0);
                }
            }
        }
    }
    None
}

fn read_gpu_frequency_mhz() -> Option<u64> {
    read_gpu_frequency_debugfs_mhz().or_else(read_gpu_frequency_sysfs_mhz)
}

fn read_gpu_frequency_sysfs_mhz() -> Option<u64> {
    let entries = fs::read_dir("/sys/class/devfreq").ok()?;
    for entry in entries.flatten() {
        let raw = fs::read_to_string(entry.path().join("cur_freq")).ok()?;
        let hz = raw.trim().parse::<u64>().ok()?;
        if hz > 0 {
            return Some(hz / 1_000_000);
        }
    }
    None
}

fn read_gpu_frequency_debugfs_mhz() -> Option<u64> {
    let entries = fs::read_dir("/sys/kernel/debug/dri").ok()?;
    for entry in entries.flatten() {
        let path = entry.path().join("measure_clock");
        if !path.exists() {
            continue;
        }
        if let Some(mhz) = parse_debugfs_measure_clock_mhz(&path) {
            return Some(mhz);
        }
    }
    None
}

fn parse_debugfs_measure_clock_mhz(path: &Path) -> Option<u64> {
    let raw = fs::read_to_string(path).ok()?;
    let lower = raw.to_ascii_lowercase();
    let start = lower.find('(')?;
    let mhz_pos = lower[start..].find("mhz")? + start;
    let inner = raw.get(start + 1..mhz_pos)?.trim();
    let number = inner.split_whitespace().next()?;
    let parsed = number.parse::<f64>().ok()?;
    Some(parsed.round() as u64)
}

fn read_gpu_memory() -> Option<GpuMemorySample> {
    let drm_dir = Path::new("/sys/class/drm");
    for entry in fs::read_dir(drm_dir).ok()?.flatten() {
        let device_dir = entry.path().join("device");
        if !device_dir.exists() {
            continue;
        }
        let used = read_integer_file(&device_dir.join("mem_info_vram_used"));
        let total = read_integer_file(&device_dir.join("mem_info_vram_total"));
        if used.is_none() && total.is_none() {
            continue;
        }
        let total_bytes = total.unwrap_or(0);
        let used_bytes = used.unwrap_or(0);
        let free_bytes = total_bytes.saturating_sub(used_bytes);
        return Some(GpuMemorySample { total_bytes, used_bytes, free_bytes });
    }
    read_gpu_memory_from_cma()
}

fn read_gpu_memory_from_cma() -> Option<GpuMemorySample> {
    let meminfo = fs::read_to_string("/proc/meminfo").ok()?;
    let mut cma_total_kb: Option<u64> = None;
    let mut cma_free_kb: Option<u64> = None;
    for line in meminfo.lines() {
        if let Some(rest) = line.strip_prefix("CmaTotal:") {
            cma_total_kb = rest.split_whitespace().next().and_then(|value| value.parse::<u64>().ok());
        } else if let Some(rest) = line.strip_prefix("CmaFree:") {
            cma_free_kb = rest.split_whitespace().next().and_then(|value| value.parse::<u64>().ok());
        }
        if cma_total_kb.is_some() && cma_free_kb.is_some() {
            break;
        }
    }
    let total_kb = cma_total_kb?;
    let free_kb = cma_free_kb?;
    let total_bytes = total_kb.saturating_mul(1024);
    let free_bytes = free_kb.saturating_mul(1024);
    let used_bytes = total_bytes.saturating_sub(free_bytes);
    Some(GpuMemorySample { total_bytes, used_bytes, free_bytes })
}

fn read_percent_file(path: &PathBuf) -> Option<f32> {
    read_integer_file(path).map(|value| value as f32)
}

fn read_integer_file(path: &PathBuf) -> Option<u64> {
    let raw = fs::read_to_string(path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<u64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_debugfs_measure_clock_rounds_mhz() {
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        fs::write(tmp.path(), "clock: v3d (249.6 MHz)\n").expect("write clock");

        assert_eq!(parse_debugfs_measure_clock_mhz(tmp.path()), Some(250));
    }

    #[test]
    fn parse_gpu_stats_file_sums_render_queues() {
        let tmp = tempfile::NamedTempFile::new().expect("tempfile");
        fs::write(tmp.path(), "queue timestamp jobs runtime\nbin 10 1 0\nrender 12 2 0\ntfu 15 3 0\nother 20 99 0\n").expect("write stats");

        let snapshot = parse_gpu_stats_file(tmp.path()).expect("snapshot");
        assert_eq!(snapshot.timestamp, 20);
        assert_eq!(snapshot.jobs, 6);
    }
}
