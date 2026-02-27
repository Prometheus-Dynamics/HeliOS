use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use chrono::Utc;
use lib_ai::backend::coral;
use serde_json::{Map as JsonMap, Value as JsonValue, json};
use tokio::sync::{Mutex, broadcast};
use tokio::time::{Duration, Instant, interval, timeout};

use crate::config_store::SensorConfigStore;
use crate::dto::{JsonData, SensorDescriptor};
use crate::ipc::{FirmwareUpdate, FirmwareUpdateStatus, SensorEvent};

use super::coral_diagnostics::{diagnostics_to_json_value, find_device_diagnostics};

fn flash_in_flight() -> &'static Mutex<HashSet<String>> {
    static IN_FLIGHT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    IN_FLIGHT.get_or_init(|| Mutex::new(HashSet::new()))
}

fn flash_completion_sent() -> &'static Mutex<HashMap<String, String>> {
    static SENT: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    SENT.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) async fn build_coral_descriptors(store: &Arc<Mutex<SensorConfigStore>>, event_sender: Option<broadcast::Sender<SensorEvent>>) -> Vec<SensorDescriptor> {
    let cfg = CoralInventoryConfig::from_env();
    if !cfg.enabled {
        return Vec::new();
    }

    let diagnostics_enabled = cfg.diagnostics_enabled;
    let snapshot = match timeout(cfg.discovery_timeout, tokio::task::spawn_blocking(move || CoralSnapshot::collect(diagnostics_enabled))).await {
        Ok(Ok(snapshot)) => snapshot,
        Ok(Err(err)) => {
            tracing::warn!(error = %err, "failed to collect Coral device snapshot");
            return Vec::new();
        }
        Err(_) => {
            tracing::warn!("timed out while collecting Coral device snapshot");
            return Vec::new();
        }
    };

    if snapshot.devices.is_empty() {
        return Vec::new();
    }

    let mut base_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut device_entries = Vec::with_capacity(snapshot.devices.len());
    for device in snapshot.devices {
        let base_identifier = coral_identifier_base(&device);
        *base_counts.entry(base_identifier.clone()).or_insert(0) += 1;
        device_entries.push((device, base_identifier));
    }

    let mut ordinal_tracker: BTreeMap<String, usize> = BTreeMap::new();
    let mut descriptors = Vec::new();
    let mut store_dirty = false;

    for (device, base_identifier) in device_entries.into_iter() {
        let count = base_counts.get(&base_identifier).copied().unwrap_or(1);
        let ordinal_entry = ordinal_tracker.entry(base_identifier.clone()).or_insert(0);
        let ordinal = *ordinal_entry;
        *ordinal_entry += 1;

        let identifier = if count == 1 { base_identifier.clone() } else { format!("{base_identifier}::slot{ordinal}") };
        let hardware_key = coral::device_hardware_key(&device);
        let hardware_id_base = build_hardware_id(&device);

        let (alias_identity, alias_display, alias_default, alias_custom, overrides, default_dirty) = {
            let mut guard = store.lock().await;
            let (alias_identity, alias_dirty) = guard.ensure_alias(&hardware_key, || coral::alias_identity_from_key(&hardware_key));
            let alias_default = coral::alias_display_from_identity(&alias_identity);
            let override_alias = guard.alias_override(&hardware_key);
            let alias_custom = override_alias.is_some();
            let alias_display = override_alias.unwrap_or_else(|| alias_default.clone());
            let overrides = guard.get_or_default(&identifier);
            let mut dirty = alias_dirty;
            if overrides.firmware.is_none() {
                overrides.firmware = Some("standard".into());
                dirty = true;
            }
            (alias_identity, alias_display, alias_default, alias_custom, overrides.clone(), dirty)
        };
        store_dirty |= default_dirty;

        let identifier_key = identifier.to_string();
        let final_overrides = overrides.clone();
        if cfg.auto_flash
            && let Some(desired_fw) = overrides.firmware.clone()
        {
            let desired_fw_label = desired_fw.clone();
            let prev_flashed = overrides.last_flashed_firmware.clone();
            let needs_flash = device.bootloader || overrides.last_flashed_firmware.as_deref().is_some_and(|last| last != desired_fw.as_str());
            if needs_flash {
                let should_spawn = {
                    let mut in_flight = flash_in_flight().lock().await;
                    if in_flight.contains(&identifier_key) {
                        false
                    } else {
                        in_flight.insert(identifier_key.clone());
                        true
                    }
                };

                if should_spawn {
                    let progress_tick_ms = env_u64("HELIOS_CORAL_FLASH_PROGRESS_TICK_MS", 2000).clamp(500, 10_000);
                    let progress_estimate_ms = env_u64("HELIOS_CORAL_FLASH_PROGRESS_ESTIMATE_MS", 90_000).clamp(10_000, 600_000);
                    if let Some(sender) = event_sender.as_ref() {
                        let progress_pct = estimate_flash_progress(0, progress_estimate_ms);
                        let update = FirmwareUpdate {
                            device_id: identifier_key.clone(),
                            firmware: desired_fw_label.clone(),
                            status: FirmwareUpdateStatus::Flashing,
                            active: prev_flashed.clone(),
                            error: None,
                            progress_pct: Some(progress_pct),
                            detail: Some("Flashing firmware".to_string()),
                            timestamp_ms: Utc::now().timestamp_millis().max(0) as u64,
                        };
                        let _ = sender.send(SensorEvent::FirmwareUpdate { update });
                    }
                    let store = Arc::clone(store);
                    let device_path = device.path.clone();
                    let desired_fw_for_task = desired_fw.clone();
                    let in_flight_key = identifier_key.clone();
                    let event_sender = event_sender.clone();
                    tokio::spawn(async move {
                        let start = Instant::now();
                        let mut progress_interval = interval(Duration::from_millis(progress_tick_ms));
                        progress_interval.tick().await;
                        let flash_task = tokio::task::spawn_blocking({
                            let desired_fw_for_task = desired_fw_for_task.clone();
                            move || coral::flash_device(device_path.as_deref(), &desired_fw_for_task)
                        });
                        tokio::pin!(flash_task);
                        let flash_result = loop {
                            tokio::select! {
                                result = &mut flash_task => break result,
                                _ = progress_interval.tick() => {
                                    if let Some(sender) = event_sender.as_ref() {
                                        let elapsed_ms = start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
                                        let progress_pct = estimate_flash_progress(elapsed_ms, progress_estimate_ms);
                                        let detail = format!("Flashing… {}s elapsed", start.elapsed().as_secs());
                                        let update = FirmwareUpdate {
                                            device_id: identifier_key.clone(),
                                            firmware: desired_fw_label.clone(),
                                            status: FirmwareUpdateStatus::Flashing,
                                            active: prev_flashed.clone(),
                                            error: None,
                                            progress_pct: Some(progress_pct),
                                            detail: Some(detail),
                                            timestamp_ms: Utc::now().timestamp_millis().max(0) as u64,
                                        };
                                        let _ = sender.send(SensorEvent::FirmwareUpdate { update });
                                    }
                                }
                            }
                        };
                        let (last_flashed_firmware, last_error) = match flash_result {
                            Ok(Ok(())) => (Some(desired_fw_label.clone()), None),
                            Ok(Err(err)) => (prev_flashed.clone(), Some(err.to_string())),
                            Err(err) => (prev_flashed.clone(), Some(format!("flash task failed: {err}"))),
                        };

                        let last_flashed_snapshot = last_flashed_firmware.clone();
                        let last_error_snapshot = last_error.clone();
                        {
                            let mut guard = store.lock().await;
                            let entry = guard.get_or_default(&identifier_key);
                            entry.last_flashed_firmware = last_flashed_firmware;
                            entry.last_error = last_error;
                            guard.save();
                        }

                        if let Some(sender) = event_sender.as_ref() {
                            let status = if last_error_snapshot.is_some() { FirmwareUpdateStatus::Failed } else { FirmwareUpdateStatus::Complete };
                            let update = FirmwareUpdate {
                                device_id: identifier_key.clone(),
                                firmware: desired_fw_label.clone(),
                                status,
                                active: last_flashed_snapshot.clone(),
                                error: last_error_snapshot.clone(),
                                progress_pct: Some(100),
                                detail: Some(if last_error_snapshot.is_some() { "Firmware update failed".to_string() } else { "Firmware applied".to_string() }),
                                timestamp_ms: Utc::now().timestamp_millis().max(0) as u64,
                            };
                            let _ = sender.send(SensorEvent::FirmwareUpdate { update });
                        }

                        let mut sent = flash_completion_sent().lock().await;
                        let key = if last_error_snapshot.is_some() { format!("{}::failed", desired_fw_label) } else { format!("{}::complete", desired_fw_label) };
                        sent.insert(identifier_key.clone(), key);

                        let mut in_flight = flash_in_flight().lock().await;
                        in_flight.remove(&in_flight_key);
                    });
                }
            } else if let Some(sender) = event_sender.as_ref() {
                let completion_key = if overrides.last_error.is_some() { format!("{}::failed", desired_fw_label) } else { format!("{}::complete", desired_fw_label) };
                let should_notify = {
                    let mut sent = flash_completion_sent().lock().await;
                    match sent.get(&identifier_key) {
                        Some(prev) if prev == &completion_key => false,
                        _ => {
                            sent.insert(identifier_key.clone(), completion_key.clone());
                            true
                        }
                    }
                };
                if should_notify {
                    let status = if overrides.last_error.is_some() { FirmwareUpdateStatus::Failed } else { FirmwareUpdateStatus::Complete };
                    let detail = if overrides.last_error.is_some() { "Firmware update failed".to_string() } else { "Firmware already active".to_string() };
                    let update = FirmwareUpdate {
                        device_id: identifier_key.clone(),
                        firmware: desired_fw_label.clone(),
                        status,
                        active: prev_flashed.clone(),
                        error: overrides.last_error.clone(),
                        progress_pct: Some(100),
                        detail: Some(detail),
                        timestamp_ms: Utc::now().timestamp_millis().max(0) as u64,
                    };
                    let _ = sender.send(SensorEvent::FirmwareUpdate { update });
                }
            }
        }

        let slot_suffix = if count == 1 { None } else { Some(ordinal) };
        let telemetry = find_device_diagnostics(&device, &snapshot.diagnostics).map(diagnostics_to_json_value);
        descriptors.push(descriptor_from_coral(
            &device,
            &identifier,
            slot_suffix,
            &final_overrides,
            &snapshot.firmware_options,
            &hardware_key,
            &hardware_id_base,
            &alias_identity,
            &alias_display,
            &alias_default,
            alias_custom,
            telemetry,
        ));
    }

    if store_dirty {
        let guard = store.lock().await;
        guard.save();
    }

    descriptors
}

struct CoralSnapshot {
    firmware_options: Vec<coral::CoralFirmwareImage>,
    devices: Vec<coral::CoralUsbDevice>,
    diagnostics: Vec<coral::CoralDeviceDiagnostics>,
}

impl CoralSnapshot {
    fn collect(with_diagnostics: bool) -> Self {
        let firmware_options = coral::available_firmware_images();
        let devices = match coral::enumerate_usb_devices() {
            Ok(devices) => devices,
            Err(err) => {
                tracing::warn!(error = %err, "failed to enumerate Coral Edge TPU devices");
                Vec::new()
            }
        };
        let diagnostics = if with_diagnostics { coral::collect_device_diagnostics() } else { Vec::new() };
        Self { firmware_options, devices, diagnostics }
    }
}

#[derive(Debug, Clone)]
struct CoralInventoryConfig {
    enabled: bool,
    diagnostics_enabled: bool,
    auto_flash: bool,
    discovery_timeout: Duration,
}

impl CoralInventoryConfig {
    fn from_env() -> Self {
        Self {
            enabled: env_bool("HELIOS_CORAL_INVENTORY_ENABLE", true),
            diagnostics_enabled: env_bool("HELIOS_CORAL_DIAGNOSTICS_ENABLE", true),
            auto_flash: env_bool("HELIOS_CORAL_AUTO_FLASH", true),
            discovery_timeout: Duration::from_millis(env_u64("HELIOS_CORAL_DISCOVERY_TIMEOUT_MS", 2500).max(250)),
        }
    }
}

const FLASH_PROGRESS_MIN_PCT: u8 = 10;
const FLASH_PROGRESS_MAX_PCT: u8 = 95;

fn estimate_flash_progress(elapsed_ms: u64, estimate_ms: u64) -> u8 {
    if estimate_ms == 0 {
        return FLASH_PROGRESS_MAX_PCT;
    }
    let clamped = (elapsed_ms as f64 / estimate_ms as f64).clamp(0.0, 1.0);
    let range = (FLASH_PROGRESS_MAX_PCT - FLASH_PROGRESS_MIN_PCT) as f64;
    let value = FLASH_PROGRESS_MIN_PCT as f64 + range * clamped;
    value.round().clamp(f64::from(FLASH_PROGRESS_MIN_PCT), f64::from(FLASH_PROGRESS_MAX_PCT)) as u8
}

fn env_bool(key: &str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(value) => matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "y" | "on"),
        Err(_) => default,
    }
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key).ok().and_then(|val| val.trim().parse::<u64>().ok()).unwrap_or(default)
}

fn coral_identifier_base(device: &coral::CoralUsbDevice) -> String {
    if let Some(serial) = device.serial_number.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        return format!("coral::usb{:03}-{:03}::serial::{serial}", device.bus_number, device.address);
    }
    if let Some(path) = &device.path {
        return path.clone();
    }
    if !device.port_path.is_empty() {
        let ports = device.port_path.iter().map(|value| value.to_string()).collect::<Vec<_>>().join(".");
        return format!("coral::usb{:03}-{:03}-{}", device.bus_number, device.address, ports);
    }
    format!("coral::usb{}-{}", device.bus_number, device.address)
}

fn build_hardware_id(device: &coral::CoralUsbDevice) -> String {
    let mut id = format!("coral::usb{:03}-{:03}", device.bus_number, device.address);
    if !device.port_path.is_empty() {
        let ports = device.port_path.iter().map(|value| value.to_string()).collect::<Vec<_>>().join(".");
        id.push_str("::ports::");
        id.push_str(&ports);
    }
    id
}

#[allow(clippy::too_many_arguments)]
fn descriptor_from_coral(
    device: &coral::CoralUsbDevice,
    identifier: &str,
    logical_slot: Option<usize>,
    overrides: &crate::config_store::SensorDeviceOverride,
    firmware_options: &[coral::CoralFirmwareImage],
    hardware_key: &str,
    hardware_id_base: &str,
    alias_identity: &str,
    alias_display: &str,
    alias_default: &str,
    alias_custom: bool,
    telemetry: Option<JsonValue>,
) -> SensorDescriptor {
    let mut info = JsonMap::new();
    info.insert("bus_number".into(), json!(device.bus_number));
    info.insert("address".into(), json!(device.address));
    if !device.port_path.is_empty() {
        let ports = device.port_path.iter().map(|value| value.to_string()).collect::<Vec<_>>().join(".");
        info.insert("port_path".into(), json!(ports));
    }
    if let Some(path) = &device.path {
        info.insert("path".into(), json!(path));
    }
    info.insert("speed".into(), json!(super::coral_diagnostics::coral_speed_label(device.speed).to_string()));

    let mut metadata = JsonMap::new();
    if let Some(product) = device.product.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        metadata.insert("product".into(), json!(product));
    }
    if let Some(manufacturer) = device.manufacturer.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        metadata.insert("manufacturer".into(), json!(manufacturer));
    }
    if let Some(serial) = device.serial_number.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        metadata.insert("serial".into(), json!(serial));
    }
    metadata.insert("hardware_key".into(), json!(hardware_key));
    let mut hardware_id = hardware_id_base.to_string();
    if let Some(slot) = logical_slot {
        hardware_id.push_str("::slot::");
        hardware_id.push_str(&slot.to_string());
    }
    metadata.insert("hardware_id".into(), json!(hardware_id));
    metadata.insert("alias".into(), json!(alias_display));
    metadata.insert("alias_identity".into(), json!(alias_identity));
    metadata.insert("alias_default".into(), json!(alias_default));
    metadata.insert("alias_custom".into(), json!(alias_custom));
    metadata.insert("mode".into(), json!(if device.bootloader { "bootloader" } else { "runtime" }));
    let status_label = super::coral_diagnostics::coral_status_label(device.speed, device.bootloader).to_string();
    metadata.insert("status".into(), json!(status_label.clone()));
    metadata.insert("firmware_status".into(), json!(status_label));

    if let Some(desired) = overrides.firmware.as_deref() {
        metadata.insert("firmware_desired".into(), json!(desired));
    }
    if let Some(active) = overrides.last_flashed_firmware.as_deref() {
        metadata.insert("firmware_active".into(), json!(active));
    }
    if let Some(error) = overrides.last_error.as_deref() {
        metadata.insert("firmware_error".into(), json!(error));
    }
    let mut options = Vec::new();
    let mut seen_variants = HashSet::<String>::new();
    if !firmware_options.is_empty() {
        options.reserve(firmware_options.len() + 2);
        for image in firmware_options {
            let variant = image.variant.as_str();
            let variant_key = variant.to_ascii_lowercase();
            if !seen_variants.insert(variant_key.clone()) {
                continue;
            }
            let mut option = JsonMap::new();
            option.insert("name".into(), json!(&image.name));
            option.insert("variant".into(), json!(variant));
            option.insert("path".into(), json!(image.path.display().to_string()));
            options.push(JsonValue::Object(option));
        }
    }
    // Always advertise the supported firmware variants so the UI can configure them even
    // when the image bundle isn't installed on this system.
    for (name, variant) in [("Standard", "standard"), ("Max", "max")] {
        if seen_variants.contains(variant) {
            continue;
        }
        let mut option = JsonMap::new();
        option.insert("name".into(), json!(name));
        option.insert("variant".into(), json!(variant));
        options.push(JsonValue::Object(option));
    }
    metadata.insert("firmware_options".into(), JsonValue::Array(options));

    if let Some(telemetry) = telemetry {
        metadata.insert("telemetry".into(), telemetry);
    }

    SensorDescriptor {
        backend: "coral".into(),
        identifier: identifier.to_string(),
        present: true,
        info: Some(JsonData::from_value(&JsonValue::Object(info))),
        metadata: Some(JsonData::from_value(&JsonValue::Object(metadata))),
        stream_id: None,
        value: None,
    }
}
