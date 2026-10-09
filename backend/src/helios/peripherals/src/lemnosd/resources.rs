//! lemnosd's devices as HeliOS sees them, for two things HeliOS does itself over lemnosd: the
//! fan override (`helios.fan`, one resource per overridable fan: the override, its state and the
//! fan's availability) and raw GPIO, PWM, I2C and SPI access (`lemnos.raw`, one per node, with
//! HeliOS's claims). The devices' readings and controls are not mirrored here: lemnosd's own Orion
//! bridge publishes them as `lemnos.device` resources (Lemnos `docs/orion.md`).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use lemnos_ipc::{DeviceClass, DeviceDesc, DeviceStatus};

use crate::lemnosd::fan::{FAN_DUTY_CONTROL, FAN_OVERRIDE_ACTION, FAN_RELEASE_ACTION};
use crate::lemnosd::raw::RAW_ACTIONS;
use crate::model::{NodeId, ObservedValue, ResourceDescriptor, ResourceKind, ResourceObservation, ResourceStatus};
use crate::resources::{DiscoveryContext, DiscoveryError, DiscoveryProbe, DiscoverySnapshot, ResourceBuilder};

pub const PROBE_NAME: &str = "lemnosd";
pub const DEVICE_ID_LABEL: &str = "lemnos.device_id";
pub const ENDPOINT_PROTOCOL: &str = "lemnos+unix";
/// The raw-access resource's local id (`lemnos_raw_<node>_io`).
pub const RAW_LOCAL_ID: &str = "io";

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
    /// HeliOS's live raw claims.
    pub claims: usize,
    /// The claims' published values (`claim.<id>.<field>`).
    pub raw: BTreeMap<String, ObservedValue>,
}

/// The resources for lemnosd's fans (the override) and raw access, once lemnosd has been reached.
/// While lemnosd is disconnected they stay published as missing.
pub fn lemnosd_resources(node: &NodeId, state: &LemnosdState) -> Result<Vec<ResourceDescriptor>, DiscoveryError> {
    let mut resources = state.devices.iter().filter(|device| is_overridable_fan(device)).map(|device| fan_resource(node, state, device)).collect::<Result<Vec<_>, _>>()?;
    if !state.board.is_empty() {
        resources.push(raw_resource(node, state)?);
    }
    Ok(resources)
}

/// Raw GPIO, PWM, I2C and SPI access through lemnosd. lemnosd refuses whatever a board device
/// owns; the state lists HeliOS's claims (`claim.<id>.kind`, `.target`, `.expires_at_ms`,
/// `.direction`, `.value`, `.edges`, `.edge.*`, PWM `.period_ns`, `.duty_ns`, `.enabled`).
fn raw_resource(node: &NodeId, state: &LemnosdState) -> Result<ResourceDescriptor, DiscoveryError> {
    let mut builder = ResourceBuilder::new(node.clone(), ResourceKind::LemnosRaw, RAW_LOCAL_ID, "Raw GPIO, PWM, I2C and SPI (lemnosd)")
        .map_err(|error| DiscoveryError::ProbeFailed { probe: PROBE_NAME.into(), message: format!("raw access: {error}") })?
        .status(if state.connected { ResourceStatus::Available } else { ResourceStatus::Missing })
        .label("lemnos.board", state.board.clone())
        .label("lemnos.status", if state.connected { "available" } else { "disconnected" })
        .endpoint(ENDPOINT_PROTOCOL, state.socket.display().to_string());
    for action in RAW_ACTIONS {
        builder = builder.capability(action, Some("lemnosd"));
    }
    let mut values = state.raw.clone();
    values.insert("claims".into(), ObservedValue::UInt(state.claims as u64));
    Ok(builder.observation(ResourceObservation { observed_at_ms: 0, values }).build())
}

/// A HeliOS fan resource: its availability is the device's, its one write is the override, and
/// its state lists the running override (`fan.override.active|duty|until_ms`).
fn fan_resource(node: &NodeId, state: &LemnosdState, device: &DeviceDesc) -> Result<ResourceDescriptor, DiscoveryError> {
    let display_name = if device.label.is_empty() { device.id.clone() } else { device.label.clone() };
    let mut builder = ResourceBuilder::new(node.clone(), ResourceKind::Fan, sanitize_local_component(&device.id), display_name)
        .map_err(|error| DiscoveryError::ProbeFailed { probe: PROBE_NAME.into(), message: format!("fan {}: {error}", device.id) })?
        .status(if state.connected { resource_status(device.status) } else { ResourceStatus::Missing })
        .label(DEVICE_ID_LABEL, device.id.clone())
        .label("lemnos.status", if state.connected { device.status.name() } else { "disconnected" })
        .endpoint(ENDPOINT_PROTOCOL, state.socket.display().to_string())
        .capability(FAN_OVERRIDE_ACTION, Some("lemnosd"))
        .capability(FAN_RELEASE_ACTION, Some("lemnosd"));
    if !state.board.is_empty() {
        builder = builder.label("lemnos.board", state.board.clone());
    }
    if !device.model.is_empty() {
        builder = builder.label("lemnos.model", device.model.clone());
    }
    let mut values = BTreeMap::new();
    let active = state.overrides.get(&device.id);
    values.insert("fan.override.active".to_string(), ObservedValue::Bool(active.is_some()));
    if let Some(active) = active {
        values.insert("fan.override.duty".to_string(), ObservedValue::F64(active.duty));
        values.insert("fan.override.until_ms".to_string(), ObservedValue::UInt(active.until_ms));
    }
    Ok(builder.observation(ResourceObservation { observed_at_ms: 0, values }).build())
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
    use lemnos_ipc::{Axis, Quantity};
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
                reason: String::new(),
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
                reason: String::new(),
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
                reason: String::new(),
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
                reason: String::new(),
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
    fn only_the_overridable_fan_and_raw_access_are_published() {
        let resources = lemnosd_resources(&NodeId::new("node1"), &connected_state()).expect("resources");
        assert_eq!(resources.len(), 2, "the fan and raw access; the devices' readings are the Lemnos bridge's");
        assert!(resources.iter().all(|resource| matches!(resource.kind, ResourceKind::Fan | ResourceKind::LemnosRaw)), "no per-device mirror");
        let fan = find(&resources, "fan");
        assert_eq!(fan.id.as_str(), "helios_fan_node1_fan");
        assert_eq!(fan.kind, ResourceKind::Fan);
        assert_eq!(fan.display_name.as_ref(), "fan");
        assert!(fan.has_capability(FAN_OVERRIDE_ACTION) && fan.has_capability(FAN_RELEASE_ACTION));
        assert!(!fan.has_capability("control.set"), "the fan's only write is the override");
        assert_eq!(fan.status, ResourceStatus::Available);
        assert_eq!(fan.observation.as_ref().expect("observation").values.get("fan.override.active"), Some(&ObservedValue::Bool(false)));
    }

    #[test]
    fn the_fan_publishes_its_override_state() {
        let mut state = connected_state();
        state.overrides.insert("fan".into(), OverrideView { duty: 1.0, until_ms: 99 });
        let resources = lemnosd_resources(&NodeId::new("node1"), &state).expect("resources");
        let values = &find(&resources, "fan").observation.as_ref().expect("observation").values;
        assert_eq!(values.get("fan.override.active"), Some(&ObservedValue::Bool(true)));
        assert_eq!(values.get("fan.override.until_ms"), Some(&ObservedValue::UInt(99)));
    }

    #[test]
    fn the_fan_goes_missing_while_lemnosd_is_disconnected() {
        let mut state = connected_state();
        state.connected = false;
        let resources = lemnosd_resources(&NodeId::new("node1"), &state).expect("resources");
        assert!(resources.iter().filter(|resource| resource.kind == ResourceKind::Fan).all(|resource| resource.status == ResourceStatus::Missing));
        assert_eq!(find(&resources, "fan").label("lemnos.status"), Some("disconnected"));
    }

    #[test]
    fn raw_access_is_one_resource_listing_the_claims() {
        assert!(lemnosd_resources(&NodeId::new("node1"), &LemnosdState::default()).expect("resources").is_empty(), "nothing before lemnosd is reached");
        let mut state = connected_state();
        state.claims = 1;
        state.raw.insert("claim.gpio-1.target".into(), ObservedValue::String("aux".into()));
        let resources = lemnosd_resources(&NodeId::new("node1"), &state).expect("resources");
        let raw = resources.iter().find(|resource| resource.kind == ResourceKind::LemnosRaw).expect("raw resource");
        assert_eq!(raw.id.as_str(), "lemnos_raw_node1_io");
        assert_eq!(raw.status, ResourceStatus::Available);
        assert!(RAW_ACTIONS.iter().all(|action| raw.has_capability(action)));
        let values = &raw.observation.as_ref().expect("claims").values;
        assert_eq!(values.get("claims"), Some(&ObservedValue::UInt(1)));
        assert_eq!(values.get("claim.gpio-1.target"), Some(&ObservedValue::String("aux".into())));
        state.connected = false;
        let resources = lemnosd_resources(&NodeId::new("node1"), &state).expect("resources");
        assert_eq!(resources.iter().find(|resource| resource.kind == ResourceKind::LemnosRaw).map(|raw| raw.status), Some(ResourceStatus::Missing));
    }

    #[test]
    fn device_statuses_map_onto_resource_statuses() {
        assert_eq!(resource_status(DeviceStatus::Available), ResourceStatus::Available);
        assert_eq!(resource_status(DeviceStatus::Degraded), ResourceStatus::Degraded);
        assert_eq!(resource_status(DeviceStatus::Faulted), ResourceStatus::Missing);
        assert_eq!(resource_status(DeviceStatus::Missing), ResourceStatus::Missing);
    }
}
