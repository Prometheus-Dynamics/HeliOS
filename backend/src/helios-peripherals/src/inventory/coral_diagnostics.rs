use std::{fs, path::Path};

use lib_ai::backend::coral::{CoralDevfreqStats, CoralDeviceDiagnostics, CoralHwmonMetric, CoralHwmonMetricKind, CoralHwmonStats, CoralUsbSpeed};
use serde_json::{Map as JsonMap, Value as JsonValue, json};

pub(crate) fn find_device_diagnostics<'a>(device: &lib_ai::backend::coral::CoralUsbDevice, diagnostics: &'a [CoralDeviceDiagnostics]) -> Option<&'a CoralDeviceDiagnostics> {
    let candidates = device_path_candidates(device);
    if let Some(matched) = diagnostics.iter().find(|diag| diagnostic_matches(diag, &candidates)) {
        return Some(matched);
    }
    let signature = usb_signature_from_device(device)?;
    diagnostics.iter().find(|diag| diag_usb_signature(diag).is_some_and(|other| usb_signatures_match(&signature, &other)))
}

fn diagnostic_matches(diag: &CoralDeviceDiagnostics, candidates: &[String]) -> bool {
    let paths = [diag.device_path.as_deref(), diag.usb_device_path.as_deref(), diag.debugfs_path.as_deref()];
    for diag_path in paths.into_iter().flatten() {
        let diag_variants = path_variants(diag_path);
        for variant in diag_variants {
            if candidates.iter().any(|candidate| candidate == &variant) {
                return true;
            }
        }
    }
    false
}

fn device_path_candidates(device: &lib_ai::backend::coral::CoralUsbDevice) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(path) = device.path.as_deref() {
        candidates.extend(path_variants(path));
    }
    if !device.port_path.is_empty() {
        let segments = device.port_path.iter().map(|value| value.to_string()).collect::<Vec<_>>().join(".");
        let chain = format!("{}-{}", device.bus_number, segments);
        let usb_path = format!("/sys/bus/usb/devices/{chain}");
        candidates.extend(path_variants(&usb_path));
        let padded_chain = format!("{:03}-{}", device.bus_number, segments);
        if padded_chain != chain {
            let padded_usb_path = format!("/sys/bus/usb/devices/{padded_chain}");
            candidates.extend(path_variants(&padded_usb_path));
        }
    }
    let usb_hint = format!("/sys/bus/usb/devices/{:03}-{:03}", device.bus_number, device.address);
    candidates.extend(path_variants(&usb_hint));
    candidates
}

fn path_variants(path: &str) -> Vec<String> {
    let mut variants = Vec::new();
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return variants;
    }
    push_path_variant(&mut variants, trimmed);
    if let Some(canonical) = canonical_path(trimmed)
        && canonical != trimmed
    {
        push_path_variant(&mut variants, &canonical);
    }
    variants
}

fn push_path_variant(variants: &mut Vec<String>, path: &str) {
    if path.is_empty() {
        return;
    }
    variants.push(path.to_string());
    if let Some(tail) = path.rsplit('/').next()
        && !tail.is_empty()
    {
        variants.push(tail.to_string());
        if let Some(stripped) = tail.split(':').next().filter(|segment| !segment.is_empty())
            && stripped != tail
        {
            variants.push(stripped.to_string());
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct UsbSignature {
    bus: u8,
    ports: Vec<u8>,
}

fn usb_signature_from_device(device: &lib_ai::backend::coral::CoralUsbDevice) -> Option<UsbSignature> {
    Some(UsbSignature { bus: device.bus_number, ports: device.port_path.clone() })
}

fn diag_usb_signature(diag: &CoralDeviceDiagnostics) -> Option<UsbSignature> {
    diag.usb_device_path.as_deref().or(diag.device_path.as_deref()).and_then(parse_usb_signature_from_path)
}

fn parse_usb_signature_from_path(path: &str) -> Option<UsbSignature> {
    let tail = path.trim().rsplit('/').next()?.split(':').next()?.trim();
    let (bus_str, ports_str) = tail.split_once('-')?;
    let bus = bus_str.parse::<u8>().ok()?;
    let mut ports = Vec::new();
    if !ports_str.is_empty() {
        for segment in ports_str.split('.') {
            if segment.is_empty() {
                continue;
            }
            ports.push(segment.parse::<u8>().ok()?);
        }
    }
    Some(UsbSignature { bus, ports })
}

fn usb_signatures_match(expected: &UsbSignature, candidate: &UsbSignature) -> bool {
    if expected.bus != candidate.bus {
        return false;
    }
    if expected.ports.is_empty() || candidate.ports.is_empty() {
        return true;
    }
    expected.ports == candidate.ports
}

fn canonical_path(path: &str) -> Option<String> {
    let path_ref = Path::new(path);
    fs::canonicalize(path_ref).ok().map(|value| value.to_string_lossy().into_owned())
}

pub(crate) fn diagnostics_to_json_value(diag: &CoralDeviceDiagnostics) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert("apex_name".into(), json!(&diag.apex_name));
    if let Some(class_path) = diag.class_path.as_ref() {
        map.insert("class_path".into(), json!(class_path));
    }
    if let Some(device_path) = diag.device_path.as_ref() {
        map.insert("device_path".into(), json!(device_path));
    }
    if let Some(usb_path) = diag.usb_device_path.as_ref() {
        map.insert("usb_device_path".into(), json!(usb_path));
    }
    if let Some(debug_path) = diag.debugfs_path.as_ref() {
        map.insert("debugfs_path".into(), json!(debug_path));
    }
    if diag.collected_at_ms > 0 {
        map.insert("collected_at_ms".into(), json_integer_from_u64(diag.collected_at_ms));
    }
    if let Some(devfreq) = diag.devfreq.as_ref() {
        map.insert("devfreq".into(), devfreq_to_json_value(devfreq));
    }
    if !diag.hwmon.is_empty() {
        map.insert("hwmon".into(), JsonValue::Array(diag.hwmon.iter().map(hwmon_to_json_value).collect()));
    }
    JsonValue::Object(map)
}

fn devfreq_to_json_value(devfreq: &CoralDevfreqStats) -> JsonValue {
    let mut map = JsonMap::new();
    if let Some(governor) = devfreq.governor.as_ref() {
        map.insert("governor".into(), json!(governor));
    }
    if let Some(value) = devfreq.current_freq_hz {
        map.insert("current_freq_hz".into(), json_integer_from_u64(value));
    }
    if let Some(value) = devfreq.min_freq_hz {
        map.insert("min_freq_hz".into(), json_integer_from_u64(value));
    }
    if let Some(value) = devfreq.max_freq_hz {
        map.insert("max_freq_hz".into(), json_integer_from_u64(value));
    }
    if let Some(value) = devfreq.target_freq_hz {
        map.insert("target_freq_hz".into(), json_integer_from_u64(value));
    }
    if let Some(load) = devfreq.load_percent {
        map.insert("load_percent".into(), json!(load));
    }
    if let Some(interval) = devfreq.polling_interval_ms {
        map.insert("polling_interval_ms".into(), json_integer_from_u64(interval));
    }
    if let Some(time) = devfreq.busy_time_us {
        map.insert("busy_time_us".into(), json_integer_from_u64(time));
    }
    if let Some(time) = devfreq.total_time_us {
        map.insert("total_time_us".into(), json_integer_from_u64(time));
    }
    if !devfreq.available_freqs_hz.is_empty() {
        let values = devfreq.available_freqs_hz.iter().map(|value| json_integer_from_u64(*value)).collect();
        map.insert("available_freqs_hz".into(), JsonValue::Array(values));
    }
    JsonValue::Object(map)
}

fn hwmon_to_json_value(stats: &CoralHwmonStats) -> JsonValue {
    let mut map = JsonMap::new();
    if let Some(name) = stats.name.as_ref() {
        map.insert("name".into(), json!(name));
    }
    if !stats.metrics.is_empty() {
        map.insert("metrics".into(), JsonValue::Array(stats.metrics.iter().map(hwmon_metric_to_json_value).collect()));
    }
    JsonValue::Object(map)
}

fn hwmon_metric_to_json_value(metric: &CoralHwmonMetric) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert("kind".into(), json!(hwmon_kind_label(&metric.kind)));
    if let Some(label) = metric.label.as_ref() {
        map.insert("label".into(), json!(label));
    }
    map.insert("index".into(), json!(metric.index));
    map.insert("value".into(), json!(metric.value));
    map.insert("units".into(), json!(&metric.units));
    JsonValue::Object(map)
}

fn hwmon_kind_label(kind: &CoralHwmonMetricKind) -> &'static str {
    match kind {
        CoralHwmonMetricKind::Temperature => "temperature",
        CoralHwmonMetricKind::Voltage => "voltage",
        CoralHwmonMetricKind::Current => "current",
        CoralHwmonMetricKind::Power => "power",
        CoralHwmonMetricKind::Energy => "energy",
        CoralHwmonMetricKind::Unknown => "unknown",
    }
}

fn json_integer_from_u64(value: u64) -> JsonValue {
    if value > i64::MAX as u64 { json!(value.to_string()) } else { json!(value as i64) }
}

pub(crate) fn coral_status_label(speed: CoralUsbSpeed, bootloader: bool) -> &'static str {
    if bootloader {
        return match speed {
            CoralUsbSpeed::SuperPlus | CoralUsbSpeed::Super => "Bootloader · USB 3.x",
            CoralUsbSpeed::High => "Bootloader · USB 2.0",
            CoralUsbSpeed::Full => "Bootloader · USB 1.1",
            CoralUsbSpeed::Low => "Bootloader · USB 1.0",
            CoralUsbSpeed::Unknown => "Bootloader",
        };
    }
    match speed {
        CoralUsbSpeed::SuperPlus | CoralUsbSpeed::Super => "Ready · USB 3.x",
        CoralUsbSpeed::High => "Ready · USB 2.0",
        CoralUsbSpeed::Full => "Ready · USB 1.1",
        CoralUsbSpeed::Low => "Ready · USB 1.0",
        CoralUsbSpeed::Unknown => "Ready",
    }
}

pub(crate) fn coral_speed_label(speed: CoralUsbSpeed) -> &'static str {
    match speed {
        CoralUsbSpeed::SuperPlus => "USB 3.2",
        CoralUsbSpeed::Super => "USB 3.0",
        CoralUsbSpeed::High => "USB 2.0",
        CoralUsbSpeed::Full => "USB 1.1",
        CoralUsbSpeed::Low => "USB 1.0",
        CoralUsbSpeed::Unknown => "Unknown",
    }
}
