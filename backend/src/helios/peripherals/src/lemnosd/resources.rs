//! lemnosd's board devices as Orion resources: one `lemnos.device` resource per device, keyed by
//! its board device id, with the device class and channel units as labels and the latest
//! reading as resource state.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use lemnos_device::{DeviceClass, DeviceStatus};
use lemnos_ipc::DeviceDesc;

use crate::lemnosd::fan::{FAN_DUTY_CONTROL, FAN_OVERRIDE_ACTION, FAN_RELEASE_ACTION};
use crate::model::{NodeId, ObservedValue, ResourceDescriptor, ResourceKind, ResourceObservation, ResourceStatus};
use crate::resources::{DiscoveryContext, DiscoveryError, DiscoveryProbe, DiscoverySnapshot, ResourceBuilder};

pub const PROBE_NAME: &str = "lemnosd";
/// The generic control write, for devices other than fans.
pub const CONTROL_SET_ACTION: &str = "control.set";
pub const DEVICE_ID_LABEL: &str = "lemnos.device_id";
pub const CLASS_LABEL: &str = "lemnos.class";
pub const ENDPOINT_PROTOCOL: &str = "lemnos+unix";

/// The latest reading of one device.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceReading {
    /// When HeliOS received it (wall clock).
    pub observed_at_ms: u64,
    /// lemnosd's monotonic clock.
    pub timestamp_us: u64,
    /// `(channel, value in its unit)`; `None` for a channel that was not read.
    pub values: Vec<(String, Option<f64>)>,
}

/// A running fan override, as published.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OverrideView {
    pub duty: f64,
    pub until_ms: u64,
}

/// What HeliOS knows of lemnosd: written by the bridge thread, read when resources are built.
#[derive(Debug, Clone, Default)]
pub struct LemnosdState {
    pub socket: PathBuf,
    pub connected: bool,
    pub board: String,
    /// The board's devices, as of the last connection; statuses follow lemnosd's events.
    pub devices: Vec<DeviceDesc>,
    pub readings: BTreeMap<String, DeviceReading>,
    pub overrides: BTreeMap<String, OverrideView>,
}

/// The resources for lemnosd's devices. While lemnosd is disconnected the known devices stay
/// published as missing, without readings.
pub fn lemnosd_resources(node: &NodeId, state: &LemnosdState) -> Result<Vec<ResourceDescriptor>, DiscoveryError> {
    state.devices.iter().map(|device| device_resource(node, state, device)).collect()
}

fn device_resource(node: &NodeId, state: &LemnosdState, device: &DeviceDesc) -> Result<ResourceDescriptor, DiscoveryError> {
    let display_name = if device.label.is_empty() { device.id.clone() } else { device.label.clone() };
    let mut builder = ResourceBuilder::new(node.clone(), ResourceKind::LemnosDevice, sanitize_local_component(&device.id), display_name)
        .map_err(|error| DiscoveryError::ProbeFailed { probe: PROBE_NAME.into(), message: format!("device {}: {error}", device.id) })?
        .status(if state.connected { resource_status(device.status) } else { ResourceStatus::Missing })
        .label(DEVICE_ID_LABEL, device.id.clone())
        .label(CLASS_LABEL, device.class.name())
        .label("lemnos.status", if state.connected { device.status.name() } else { "disconnected" })
        .endpoint(ENDPOINT_PROTOCOL, state.socket.display().to_string());
    if !state.board.is_empty() {
        builder = builder.label("lemnos.board", state.board.clone());
    }
    if !device.model.is_empty() {
        builder = builder.label("lemnos.model", device.model.clone());
    }
    if device.pixels > 0 {
        builder = builder.label("lemnos.pixels", device.pixels.to_string());
    }
    for channel in &device.channels {
        builder = builder.label(format!("lemnos.channel.{}.quantity", channel.name), channel.quantity.name());
        let unit = channel.quantity.unit().symbol();
        if !unit.is_empty() {
            builder = builder.label(format!("lemnos.channel.{}.unit", channel.name), unit);
        }
    }
    for control in &device.controls {
        builder = builder
            .label(format!("lemnos.control.{}.quantity", control.name), control.quantity.name())
            .label(format!("lemnos.control.{}.min", control.name), scaled(control.min, control.exponent).to_string())
            .label(format!("lemnos.control.{}.max", control.name), scaled(control.max, control.exponent).to_string());
        let unit = control.quantity.unit().symbol();
        if !unit.is_empty() {
            builder = builder.label(format!("lemnos.control.{}.unit", control.name), unit);
        }
    }

    let fan = is_overridable_fan(device);
    if !device.channels.is_empty() {
        builder = builder.capability("sensor", Some("lemnosd"));
    }
    if fan {
        builder = builder.capability(FAN_OVERRIDE_ACTION, Some("lemnosd")).capability(FAN_RELEASE_ACTION, Some("lemnosd"));
    } else if !device.controls.is_empty() {
        builder = builder.capability(CONTROL_SET_ACTION, Some("lemnosd"));
    }

    if let Some(observation) = observation(state, device, fan) {
        builder = builder.observation(observation);
    }
    Ok(builder.build())
}

fn observation(state: &LemnosdState, device: &DeviceDesc, fan: bool) -> Option<ResourceObservation> {
    let reading = state.connected.then(|| state.readings.get(&device.id)).flatten();
    let mut values = BTreeMap::new();
    let mut observed_at_ms = 0;
    if let Some(reading) = reading {
        observed_at_ms = reading.observed_at_ms;
        values.insert("reading.timestamp_us".to_string(), ObservedValue::UInt(reading.timestamp_us));
        for (channel, value) in &reading.values {
            if let Some(value) = value {
                values.insert(format!("reading.{channel}"), ObservedValue::F64(*value));
            }
        }
    }
    if fan {
        let active = state.overrides.get(&device.id);
        values.insert("fan.override.active".to_string(), ObservedValue::Bool(active.is_some()));
        if let Some(active) = active {
            values.insert("fan.override.duty".to_string(), ObservedValue::F64(active.duty));
            values.insert("fan.override.until_ms".to_string(), ObservedValue::UInt(active.until_ms));
        }
    }
    (!values.is_empty()).then_some(ResourceObservation { observed_at_ms, values })
}

/// A fan HeliOS may override: the fan class with lemnosd's `duty` control.
pub fn is_overridable_fan(device: &DeviceDesc) -> bool {
    device.class == DeviceClass::Fan && device.controls.iter().any(|control| control.name == FAN_DUTY_CONTROL)
}

fn resource_status(status: DeviceStatus) -> ResourceStatus {
    match status {
        DeviceStatus::Available => ResourceStatus::Available,
        DeviceStatus::Degraded => ResourceStatus::Degraded,
        DeviceStatus::Faulted | DeviceStatus::Missing => ResourceStatus::Missing,
    }
}

fn scaled(raw: i32, exponent: i8) -> f64 {
    let scale = 10f64.powi(i32::from(exponent.unsigned_abs()));
    if exponent < 0 { f64::from(raw) / scale } else { f64::from(raw) * scale }
}

fn sanitize_local_component(value: &str) -> String {
    let mut cleaned = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' {
            cleaned.push(ch.to_ascii_lowercase());
        } else if matches!(ch, '/' | '.' | '_' | ':' | ' ') && !cleaned.ends_with('-') {
            cleaned.push('-');
        }
    }
    let cleaned = cleaned.trim_matches('-');
    if cleaned.is_empty() { "device".into() } else { cleaned.into() }
}

/// lemnosd's devices for the inventory: reads the bridge's state, never the socket.
pub struct LemnosdProbe {
    state: Arc<Mutex<LemnosdState>>,
}

impl LemnosdProbe {
    pub fn new(state: Arc<Mutex<LemnosdState>>) -> Self {
        Self { state }
    }
}

impl DiscoveryProbe for LemnosdProbe {
    fn name(&self) -> &'static str {
        PROBE_NAME
    }

    fn discover(&self, context: &DiscoveryContext) -> Result<DiscoverySnapshot, DiscoveryError> {
        let state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(DiscoverySnapshot::new(lemnosd_resources(&context.local_node_id, &state)?))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use lemnos_device::{Axis, Quantity};
    use lemnos_ipc::{ChannelDesc, ControlDesc};

    pub(crate) fn raze_devices() -> Vec<DeviceDesc> {
        let channel = |name: &str, quantity, axis, exponent| ChannelDesc { name: name.into(), quantity, axis, exponent };
        vec![
            DeviceDesc {
                id: "imu".into(),
                label: "IMU".into(),
                class: DeviceClass::Imu,
                model: "BMI088".into(),
                status: DeviceStatus::Available,
                channels: vec![channel("acceleration.x", Quantity::Acceleration, Axis::X, -3), channel("angular-rate.z", Quantity::AngularRate, Axis::Z, -4)],
                controls: Vec::new(),
                pixels: 0,
            },
            DeviceDesc {
                id: "fan".into(),
                label: String::new(),
                class: DeviceClass::Fan,
                model: "pwm-fan".into(),
                status: DeviceStatus::Available,
                channels: vec![channel("duty", Quantity::Ratio, Axis::None, -3), channel("rpm", Quantity::RotationalSpeed, Axis::None, 0)],
                controls: vec![ControlDesc { name: "duty".into(), quantity: Quantity::Ratio, exponent: -3, min: 0, max: 1000 }],
                pixels: 0,
            },
            DeviceDesc {
                id: "status-ring".into(),
                label: String::new(),
                class: DeviceClass::Light,
                model: "ws2812".into(),
                status: DeviceStatus::Available,
                channels: Vec::new(),
                controls: Vec::new(),
                pixels: 16,
            },
            DeviceDesc {
                id: "usb-a-power".into(),
                label: String::new(),
                class: DeviceClass::Gpio,
                model: "gpio-output".into(),
                status: DeviceStatus::Missing,
                channels: vec![channel("level", Quantity::Level, Axis::None, 0)],
                controls: vec![ControlDesc { name: "level".into(), quantity: Quantity::Level, exponent: 0, min: 0, max: 1 }],
                pixels: 0,
            },
        ]
    }

    fn connected_state() -> LemnosdState {
        LemnosdState { socket: PathBuf::from("/run/lemnos/lemnosd.sock"), connected: true, board: "raze".into(), devices: raze_devices(), ..LemnosdState::default() }
    }

    fn find<'a>(resources: &'a [ResourceDescriptor], device: &str) -> &'a ResourceDescriptor {
        resources.iter().find(|resource| resource.label(DEVICE_ID_LABEL) == Some(device)).expect(device)
    }

    #[test]
    fn one_resource_per_board_device_keyed_by_device_id() {
        let resources = lemnosd_resources(&NodeId::new("node1"), &connected_state()).expect("resources");
        assert_eq!(resources.len(), 4);
        let imu = find(&resources, "imu");
        assert_eq!(imu.id.as_str(), "lemnos_device_node1_imu");
        assert_eq!(imu.kind, ResourceKind::LemnosDevice);
        assert_eq!(imu.display_name.as_ref(), "IMU");
        assert_eq!(imu.label(CLASS_LABEL), Some("imu"));
        assert_eq!(imu.label("lemnos.model"), Some("BMI088"));
        assert_eq!(imu.label("lemnos.board"), Some("raze"));
        assert_eq!(imu.label("lemnos.channel.acceleration.x.unit"), Some("m/s²"));
        assert_eq!(imu.label("lemnos.channel.angular-rate.z.unit"), Some("rad/s"));
        assert_eq!(imu.label("lemnos.channel.acceleration.x.quantity"), Some("acceleration"));
        assert_eq!(imu.endpoint(ENDPOINT_PROTOCOL), Some("/run/lemnos/lemnosd.sock"));
        assert!(imu.has_capability("sensor"));
        assert!(!imu.has_capability(FAN_OVERRIDE_ACTION));
        assert!(imu.observation.is_none(), "no reading yet");

        let ring = find(&resources, "status-ring");
        assert_eq!(ring.id.as_str(), "lemnos_device_node1_status-ring");
        assert_eq!(ring.label("lemnos.pixels"), Some("16"));
        assert!(ring.capabilities.is_empty(), "HeliOS drives the ring through its status, not resource actions");

        let usb = find(&resources, "usb-a-power");
        assert_eq!(usb.status, ResourceStatus::Missing);
        assert!(usb.has_capability(CONTROL_SET_ACTION));
        assert_eq!(usb.label("lemnos.control.level.max"), Some("1"));
    }

    #[test]
    fn the_fan_offers_only_the_override_and_publishes_its_state() {
        let mut state = connected_state();
        state.readings.insert("fan".into(), DeviceReading { observed_at_ms: 42, timestamp_us: 7, values: vec![("duty".into(), Some(0.702)), ("rpm".into(), None)] });
        let fan = lemnosd_resources(&NodeId::new("node1"), &state).expect("resources").into_iter().find(|r| r.label(DEVICE_ID_LABEL) == Some("fan")).expect("fan");
        assert_eq!(fan.display_name.as_ref(), "fan");
        assert!(fan.has_capability(FAN_OVERRIDE_ACTION) && fan.has_capability(FAN_RELEASE_ACTION));
        assert!(!fan.has_capability(CONTROL_SET_ACTION), "the fan's only write is the override");
        assert_eq!(fan.label("lemnos.control.duty.max"), Some("1"));
        let observation = fan.observation.as_ref().expect("observation");
        assert_eq!(observation.observed_at_ms, 42);
        assert_eq!(observation.values.get("reading.duty"), Some(&ObservedValue::F64(0.702)));
        assert!(!observation.values.contains_key("reading.rpm"), "unread channels are left out");
        assert_eq!(observation.values.get("fan.override.active"), Some(&ObservedValue::Bool(false)));

        state.overrides.insert("fan".into(), OverrideView { duty: 1.0, until_ms: 99 });
        let fan = lemnosd_resources(&NodeId::new("node1"), &state).expect("resources").into_iter().find(|r| r.label(DEVICE_ID_LABEL) == Some("fan")).expect("fan");
        let values = &fan.observation.expect("observation").values;
        assert_eq!(values.get("fan.override.active"), Some(&ObservedValue::Bool(true)));
        assert_eq!(values.get("fan.override.until_ms"), Some(&ObservedValue::UInt(99)));
    }

    #[test]
    fn devices_go_missing_without_readings_while_lemnosd_is_disconnected() {
        let mut state = connected_state();
        state.readings.insert("imu".into(), DeviceReading { observed_at_ms: 1, timestamp_us: 1, values: vec![("acceleration.x".into(), Some(9.8))] });
        state.connected = false;
        let resources = lemnosd_resources(&NodeId::new("node1"), &state).expect("resources");
        assert_eq!(resources.len(), 4, "known devices stay published");
        assert!(resources.iter().all(|resource| resource.status == ResourceStatus::Missing));
        assert_eq!(find(&resources, "imu").label("lemnos.status"), Some("disconnected"));
        assert!(find(&resources, "imu").observation.is_none());
    }

    #[test]
    fn device_statuses_map_onto_resource_statuses() {
        assert_eq!(resource_status(DeviceStatus::Available), ResourceStatus::Available);
        assert_eq!(resource_status(DeviceStatus::Degraded), ResourceStatus::Degraded);
        assert_eq!(resource_status(DeviceStatus::Faulted), ResourceStatus::Missing);
        assert_eq!(resource_status(DeviceStatus::Missing), ResourceStatus::Missing);
        assert_eq!(sanitize_local_component("CPU Thermal"), "cpu-thermal");
        assert_eq!(sanitize_local_component(""), "device");
    }
}
