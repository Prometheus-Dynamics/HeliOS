use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use tracing::warn;

use crate::http::AppState;
use crate::ipc::peripherals::SensorsConnection;

use super::types::{FanStatus, UsbPeripheral, UsbPeripheralWarning};

pub(crate) fn list_usb_sysfs() -> Vec<UsbPeripheral> {
    let mut out = Vec::new();
    let root = Path::new("/sys/bus/usb/devices");
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return out,
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.contains(':') {
            continue;
        }
        let path = entry.path();
        let vendor = read_hex_u16(path.join("idVendor").as_path());
        let product = read_hex_u16(path.join("idProduct").as_path());
        let (Some(vendor), Some(product)) = (vendor, product) else {
            continue;
        };

        if vendor == 0x1D6B {
            continue;
        }
        if let Some(class) = read_hex_u8(path.join("bDeviceClass").as_path()) {
            if class == 0x09 {
                continue;
            }
        }

        let manufacturer = read_string(path.join("manufacturer").as_path());
        let product_name = read_string(path.join("product").as_path());
        let description = match (manufacturer.as_deref(), product_name.as_deref()) {
            (Some(m), Some(p)) => Some(format!("{m} {p}")),
            (Some(m), None) => Some(m.to_string()),
            (None, Some(p)) => Some(p.to_string()),
            _ => None,
        };

        let desc_lower = description.as_deref().unwrap_or_default().to_lowercase();
        let is_coral =
            matches!((vendor, product), (0x1A6E, 0x089A) | (0x18D1, 0x9302) | (0x18D1, 0x9301)) || desc_lower.contains("coral") || desc_lower.contains("edge tpu") || desc_lower.contains("edgetpu");
        let kind = if is_coral { Some("coral".into()) } else { Some("usb".into()) };
        let description = if is_coral && description.is_none() { Some("Coral Edge TPU".into()) } else { description };
        let warnings = usb_warnings_from_path(&path);
        out.push(UsbPeripheral { id: format!("{vendor:04x}:{product:04x}@{name}"), kind, description, present: true, warnings });
    }

    out
}

fn read_string(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn read_hex_u16(path: &Path) -> Option<u16> {
    let raw = fs::read_to_string(path).ok()?;
    let trimmed = raw.trim().trim_start_matches("0x");
    u16::from_str_radix(trimmed, 16).ok()
}

fn read_hex_u8(path: &Path) -> Option<u8> {
    let raw = fs::read_to_string(path).ok()?;
    let trimmed = raw.trim().trim_start_matches("0x");
    u8::from_str_radix(trimmed, 16).ok()
}

pub(super) fn usb_warnings_from_path(device_path: &Path) -> Vec<UsbPeripheralWarning> {
    let mut warnings = Vec::new();

    if let Some(speed) = read_usb_speed(device_path) {
        if speed < 5_000.0 {
            warnings.push(UsbPeripheralWarning { code: "usb_speed_low".into(), message: "USB 2.0/1.x link detected; use a quality USB 3.0 cable and connect directly to a USB 3 port.".into() });
        }
    }

    if usb_has_external_hub_parent(device_path) {
        warnings.push(UsbPeripheralWarning {
            code: "usb_hub_power".into(),
            message: "Device is connected through a USB hub; hubs can under-power peripherals and cause flaky USB events. Prefer a direct USB port.".into(),
        });
    }

    if let Some(undervoltage) = read_system_undervoltage() {
        if undervoltage {
            warnings.push(UsbPeripheralWarning {
                code: "system_undervoltage".into(),
                message: "System undervoltage detected; USB devices can reset or drop to lower speeds. Check the power supply and cabling.".into(),
            });
        }
    }

    warnings
}

fn read_usb_speed(device_path: &Path) -> Option<f32> {
    let raw = fs::read_to_string(device_path.join("speed")).ok()?;
    raw.trim().parse::<f32>().ok()
}

fn usb_has_external_hub_parent(device_path: &Path) -> bool {
    let root = Path::new("/sys/bus/usb/devices");
    let mut path = device_path;
    loop {
        if let (Some(class), Some(vendor)) = (read_hex_u8(path.join("bDeviceClass").as_path()), read_hex_u16(path.join("idVendor").as_path())) {
            if class == 0x09 && vendor != 0x1D6B {
                return true;
            }
        }
        let Some(parent) = path.parent() else {
            break;
        };
        if parent == path || parent == root {
            break;
        }
        path = parent;
    }
    false
}

fn read_system_undervoltage() -> Option<bool> {
    let raw = fs::read_to_string("/sys/devices/platform/soc/firmware/get_throttled").ok()?;
    let trimmed = raw.trim().trim_start_matches("0x");
    let value = u32::from_str_radix(trimmed, 16).ok()?;
    Some((value & 0x1) != 0 || (value & (1 << 16)) != 0)
}

pub(crate) fn map_fan_status(status: lib_sensors::fan_config::FanStatus) -> FanStatus {
    FanStatus {
        present: true,
        rpm: status.rpm,
        mode: Some(status.mode),
        target_percent: Some(status.target_percent),
        temperature_c: status.temperature_c,
        path_in_use: status.path_in_use,
        last_error: status.last_error,
        updated_at_ms: status.updated_at_ms,
    }
}

fn read_timeout_env(var: &str, default_ms: u64, min_ms: u64, max_ms: u64) -> Duration {
    let ms = std::env::var(var).ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(default_ms);
    Duration::from_millis(ms.clamp(min_ms, max_ms))
}

pub(super) fn sensor_ipc_timeout() -> Duration {
    static VALUE: OnceLock<Duration> = OnceLock::new();
    *VALUE.get_or_init(|| read_timeout_env("HELIOS_PERIPHERALS_TIMEOUT_MS", 1_500, 250, 15_000))
}

fn sensor_refresh_timeout() -> Duration {
    static VALUE: OnceLock<Duration> = OnceLock::new();
    *VALUE.get_or_init(|| read_timeout_env("HELIOS_PERIPHERALS_REFRESH_TIMEOUT_MS", 2_500, 500, 20_000))
}

pub(super) async fn cached_discover_cameras_snapshot(state: &AppState) -> Result<(helios_engine::capture::DiscoveryResult, u64), String> {
    state.services.hardware.cached_discover_cameras_snapshot(state).await
}

pub(super) async fn allow_inventory_refresh(state: &AppState) -> bool {
    state.services.hardware.allow_inventory_refresh().await
}

pub(super) async fn refresh_inventory(state: &AppState, sensors: Arc<SensorsConnection>) -> Option<helios_peripherals::dto::SensorInventory> {
    match tokio::time::timeout(sensor_refresh_timeout(), sensors.discover(true)).await {
        Ok(Ok(Ok(inv))) => return Some(inv),
        Ok(Ok(Err(reason))) => {
            warn!(%reason, "sensor inventory refresh rejected");
            return None;
        }
        Ok(Err(err)) => {
            warn!(%err, "sensor inventory refresh failed");
        }
        Err(_) => {
            warn!("sensor inventory refresh timed out");
        }
    }

    state.invalidate_sensors().await;
    let sensors = state.ensure_sensors().await?;
    match tokio::time::timeout(sensor_refresh_timeout(), sensors.discover(true)).await {
        Ok(Ok(Ok(inv))) => Some(inv),
        Ok(Ok(Err(reason))) => {
            warn!(%reason, "sensor inventory refresh rejected after reconnect");
            None
        }
        Ok(Err(err)) => {
            warn!(%err, "sensor inventory refresh failed after reconnect");
            None
        }
        Err(_) => {
            warn!("sensor inventory refresh timed out after reconnect");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use lib_sensors::fan_config::{FanMode, FanStatus as PeripheralFanStatus};

    use super::map_fan_status;

    #[test]
    fn map_fan_status_marks_status_present() {
        let status =
            PeripheralFanStatus { mode: FanMode::Manual, target_percent: 42, temperature_c: Some(51.5), rpm: Some(2_100), path_in_use: None, last_error: Some("none".into()), updated_at_ms: Some(7) };

        let mapped = map_fan_status(status);
        assert!(mapped.present);
        assert_eq!(mapped.mode, Some(FanMode::Manual));
        assert_eq!(mapped.target_percent, Some(42));
        assert_eq!(mapped.temperature_c, Some(51.5));
        assert_eq!(mapped.rpm, Some(2_100));
        assert_eq!(mapped.path_in_use, None);
        assert_eq!(mapped.last_error.as_deref(), Some("none"));
        assert_eq!(mapped.updated_at_ms, Some(7));
    }
}
