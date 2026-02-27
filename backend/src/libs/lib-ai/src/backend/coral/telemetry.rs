use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct CoralDeviceDiagnostics {
    pub apex_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usb_device_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debugfs_path: Option<String>,
    pub collected_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub devfreq: Option<CoralDevfreqStats>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hwmon: Vec<CoralHwmonStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct CoralDevfreqStats {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub governor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_freq_hz: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_freq_hz: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_freq_hz: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_freq_hz: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_percent: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub polling_interval_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub busy_time_us: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_time_us: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub available_freqs_hz: Vec<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct CoralHwmonStats {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub metrics: Vec<CoralHwmonMetric>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CoralHwmonMetricKind {
    Temperature,
    Voltage,
    Current,
    Power,
    Energy,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct CoralHwmonMetric {
    pub kind: CoralHwmonMetricKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub index: u32,
    pub value: f64,
    pub units: String,
}

const SYSFS_MISC_ROOT: &str = "/sys/class/misc";
const SYSFS_DEVFREQ_ROOT: &str = "/sys/class/devfreq";
const SYSFS_USB_ROOT: &str = "/sys/bus/usb/devices";
const DEBUGFS_EDGETPU_ROOT: &str = "/sys/kernel/debug/edgetpu";

pub fn collect_device_diagnostics() -> Vec<CoralDeviceDiagnostics> {
    let usb_index = build_usb_index();
    let debugfs_index = build_debugfs_index();
    list_apex_nodes().into_iter().map(|node| build_snapshot(node, &usb_index, &debugfs_index)).collect()
}

#[derive(Debug)]
struct ApexNode {
    name: String,
    class_path: PathBuf,
    device_path: Option<PathBuf>,
}

fn list_apex_nodes() -> Vec<ApexNode> {
    let mut nodes = Vec::new();
    if let Ok(entries) = fs::read_dir(SYSFS_MISC_ROOT) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("apex_") {
                continue;
            }
            let path = entry.path();
            let device_path = path.join("device");
            let resolved = fs::read_link(&device_path).ok().and_then(|link| resolve_path(&device_path.parent().unwrap_or(&path).join(link)));
            nodes.push(ApexNode { name, class_path: path, device_path: resolved });
        }
    }
    nodes
}

fn build_snapshot(node: ApexNode, usb_index: &[(PathBuf, String)], debugfs_index: &[(PathBuf, String)]) -> CoralDeviceDiagnostics {
    let usb_path = node.device_path.as_ref().and_then(|device| match_usb_entry(device, usb_index));
    let devfreq = read_devfreq_stats(&node.name);
    let hwmon = node.device_path.as_deref().map(read_hwmon_stats).unwrap_or_default();
    let collected_at_ms = now_millis();
    let debugfs_path = match_debugfs_entry(&node.name, node.device_path.as_deref(), debugfs_index);

    CoralDeviceDiagnostics {
        apex_name: node.name,
        class_path: Some(node.class_path.to_string_lossy().into_owned()),
        device_path: node.device_path.as_ref().map(|path| path.to_string_lossy().into_owned()),
        usb_device_path: usb_path,
        debugfs_path,
        collected_at_ms,
        devfreq,
        hwmon,
    }
}

fn resolve_path(path: &Path) -> Option<PathBuf> {
    fs::canonicalize(path).ok()
}

fn now_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_millis() as u64).unwrap_or_default()
}

fn read_devfreq_stats(name: &str) -> Option<CoralDevfreqStats> {
    let base = Path::new(SYSFS_DEVFREQ_ROOT).join(name);
    if !base.exists() {
        return None;
    }
    let stats = CoralDevfreqStats {
        governor: read_text(base.join("governor")),
        current_freq_hz: read_u64(base.join("cur_freq")),
        min_freq_hz: read_u64(base.join("min_freq")),
        max_freq_hz: read_u64(base.join("max_freq")),
        target_freq_hz: read_u64(base.join("target_freq")),
        load_percent: read_f32(base.join("load")),
        polling_interval_ms: read_u64(base.join("polling_interval")),
        busy_time_us: read_u64(base.join("busy_time")),
        total_time_us: read_u64(base.join("total_time")),
        available_freqs_hz: read_u64_list(base.join("available_frequencies")),
    };

    if stats.governor.is_none()
        && stats.current_freq_hz.is_none()
        && stats.min_freq_hz.is_none()
        && stats.max_freq_hz.is_none()
        && stats.target_freq_hz.is_none()
        && stats.load_percent.is_none()
        && stats.polling_interval_ms.is_none()
        && stats.busy_time_us.is_none()
        && stats.total_time_us.is_none()
        && stats.available_freqs_hz.is_empty()
    {
        None
    } else {
        Some(stats)
    }
}

fn read_hwmon_stats(device_path: &Path) -> Vec<CoralHwmonStats> {
    let mut stats = Vec::new();
    let hwmon_root = device_path.join("hwmon");
    let entries = match fs::read_dir(&hwmon_root) {
        Ok(entries) => entries,
        Err(_) => return stats,
    };

    for entry in entries.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let metrics = collect_hwmon_metrics(&dir);
        if metrics.is_empty() {
            continue;
        }
        let name = read_text(dir.join("name"));
        stats.push(CoralHwmonStats { name, metrics });
    }

    stats
}

fn collect_hwmon_metrics(dir: &Path) -> Vec<CoralHwmonMetric> {
    let mut metrics = Vec::new();
    metrics.extend(read_hwmon_group(dir, "temp", CoralHwmonMetricKind::Temperature, 1000.0, "C"));
    metrics.extend(read_hwmon_group(dir, "in", CoralHwmonMetricKind::Voltage, 1000.0, "V"));
    metrics.extend(read_hwmon_group(dir, "curr", CoralHwmonMetricKind::Current, 1000.0, "A"));
    metrics.extend(read_hwmon_group(dir, "power", CoralHwmonMetricKind::Power, 1_000_000.0, "W"));
    metrics.extend(read_hwmon_group(dir, "energy", CoralHwmonMetricKind::Energy, 1_000_000.0, "J"));
    metrics
}

fn read_hwmon_group(dir: &Path, prefix: &str, kind: CoralHwmonMetricKind, divisor: f64, units: &str) -> Vec<CoralHwmonMetric> {
    let mut readings = Vec::new();
    for index in hwmon_metric_indices(dir, prefix) {
        let input_path = dir.join(format!("{prefix}{index}_input"));
        let raw = match read_f64(&input_path) {
            Some(value) => value,
            None => continue,
        };
        let label = read_text(dir.join(format!("{prefix}{index}_label")));
        readings.push(CoralHwmonMetric { kind: kind.clone(), label, index, value: raw / divisor, units: units.to_string() });
    }
    readings
}

fn hwmon_metric_indices(dir: &Path, prefix: &str) -> BTreeSet<u32> {
    let mut indices = BTreeSet::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(index) = parse_hwmon_index(&name, prefix) {
                indices.insert(index);
            }
        }
    }
    indices
}

fn parse_hwmon_index(name: &str, prefix: &str) -> Option<u32> {
    if !name.starts_with(prefix) || !name.ends_with("_input") {
        return None;
    }
    let trimmed = &name[prefix.len()..name.len() - "_input".len()];
    trimmed.parse().ok()
}

fn read_text(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).ok().map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
}

fn read_u64(path: impl AsRef<Path>) -> Option<u64> {
    read_text(path).and_then(|value| value.parse().ok())
}

fn read_f64(path: &Path) -> Option<f64> {
    read_text(path).and_then(|value| value.parse().ok())
}

fn read_f32(path: impl AsRef<Path>) -> Option<f32> {
    read_text(path).and_then(|value| value.parse().ok())
}

fn read_u64_list(path: impl AsRef<Path>) -> Vec<u64> {
    read_text(path).map(|value| value.split_whitespace().filter_map(|entry| entry.parse().ok()).collect()).unwrap_or_default()
}

fn build_usb_index() -> Vec<(PathBuf, String)> {
    let mut entries = Vec::new();
    if let Ok(dir) = fs::read_dir(SYSFS_USB_ROOT) {
        for entry in dir.flatten() {
            let path = entry.path();
            match resolve_path(&path) {
                Some(real) => entries.push((real, path.to_string_lossy().into_owned())),
                None => continue,
            }
        }
    }
    entries
}

fn match_usb_entry(device_path: &Path, usb_index: &[(PathBuf, String)]) -> Option<String> {
    for (real, display) in usb_index {
        if real == device_path || device_path.starts_with(real) || real.starts_with(device_path) {
            return Some(display.clone());
        }
    }
    None
}

fn build_debugfs_index() -> Vec<(PathBuf, String)> {
    let mut entries = Vec::new();
    if let Ok(dir) = fs::read_dir(DEBUGFS_EDGETPU_ROOT) {
        for entry in dir.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            if let Some(real) = resolve_path(&path) {
                entries.push((real, path.to_string_lossy().into_owned()));
            }
        }
    }
    entries
}

fn match_debugfs_entry(apex_name: &str, device_path: Option<&Path>, debug_index: &[(PathBuf, String)]) -> Option<String> {
    if let Some(path) = debug_index.iter().find(|(_, display)| display.ends_with(apex_name)).map(|(_, display)| display.clone()) {
        return Some(path);
    }
    if let Some(device) = device_path {
        for (real, display) in debug_index {
            if real == device {
                return Some(display.clone());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parses_hwmon_indices() {
        assert_eq!(parse_hwmon_index("temp1_input", "temp"), Some(1));
        assert_eq!(parse_hwmon_index("temp1_label", "temp"), None);
        assert_eq!(parse_hwmon_index("in3_input", "in"), Some(3));
        assert_eq!(parse_hwmon_index("power0_input", "power"), Some(0));
    }

    #[test]
    fn reads_hwmon_group_metrics() {
        let dir = tempdir().expect("tempdir");
        let hwmon_dir = dir.path();
        fs::write(hwmon_dir.join("temp1_input"), "42000").unwrap();
        fs::write(hwmon_dir.join("temp1_label"), "edge tpu temp").unwrap();
        fs::write(hwmon_dir.join("temp2_input"), "0").unwrap();
        let metrics = read_hwmon_group(hwmon_dir, "temp", CoralHwmonMetricKind::Temperature, 1000.0, "C");
        assert_eq!(metrics.len(), 2);
        assert!((metrics[0].value - 42.0).abs() < f64::EPSILON);
        assert_eq!(metrics[0].label.as_deref(), Some("edge tpu temp"));
    }

    #[test]
    fn matches_usb_entry_by_prefix() {
        let temp = tempdir().unwrap();
        let usb_path = temp.path().join("1-1");
        fs::create_dir(&usb_path).unwrap();
        let real = resolve_path(&usb_path).unwrap();
        let index = vec![(real.clone(), usb_path.to_string_lossy().into_owned())];
        assert_eq!(match_usb_entry(&real, &index), Some(usb_path.to_string_lossy().into_owned()));
    }
}
