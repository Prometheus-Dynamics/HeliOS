use std::sync::{Arc, Mutex};

use lemnos::core::{
    Availability, DeviceDescriptor, DeviceHealth, DeviceId, DeviceKind, DeviceLifecycleState, DeviceResponse, DeviceStateSnapshot, GpioLevel, GpioLineConfiguration, GpioRequest, I2cRequest,
    I2cResponse, InteractionResponse, InterfaceKind, PwmConfiguration, PwmPolarity, PwmRequest, SpiRequest, SpiResponse, StandardRequest, StandardResponse, Value,
};
use lemnos::linux::{LinuxBackend, LinuxHotplugWatcher};
use lemnos::prelude::Lemnos;

use crate::model::{
    GpioControl, GpioDirection, I2cControl, PwmControl, ResourceControlData, ResourceControlRequest, ResourceControlResult, ResourceDescriptor, ResourceKind, ResourceStatus, SpiControl,
};
use crate::resources::{CaptureProbe, DiscoveryContext, DiscoveryError, DiscoveryProbe, DiscoverySnapshot, PeripheralInventoryService, ResourceBuilder};
use crate::workloads::{PeripheralManager, ResourceControlError, ResourceController};

use super::lemnos_fan::HeliosLinuxHwmonFanDriver;

#[derive(Clone)]
pub struct LemnosPeripheralStack {
    inner: Arc<Mutex<Lemnos>>,
    backend: LinuxBackend,
}

impl LemnosPeripheralStack {
    pub fn new(_owner: crate::model::NodeId) -> Result<Self, String> {
        let backend = LinuxBackend::default();
        let mut lemnos = Lemnos::builder().with_linux_backend_ref(&backend).build();
        lemnos.register_driver(HeliosLinuxHwmonFanDriver).map_err(|error| error.to_string())?;
        lemnos.register_builtin_drivers().map_err(|error| error.to_string())?;
        lemnos.start();
        Ok(Self { inner: Arc::new(Mutex::new(lemnos)), backend })
    }

    pub fn register_into_inventory(&self, service: &mut PeripheralInventoryService) {
        service.register_probe(Arc::new(LemnosDiscoveryProbe { inner: Arc::clone(&self.inner), backend: self.backend.clone() })).register_probe(Arc::new(CaptureProbe));
    }

    pub fn build_manager(&self) -> PeripheralManager {
        let mut manager = PeripheralManager::new();
        manager.register_controller(Box::new(LemnosResourceController { inner: Arc::clone(&self.inner) }));
        manager
    }

    pub fn hotplug_watcher(&self) -> Result<LinuxHotplugWatcher, String> {
        self.backend.hotplug_watcher().map_err(|error| error.to_string())
    }

    pub fn poll_hotplug_refresh(&self, watcher: &mut LinuxHotplugWatcher, _observed_at_ms: u64) -> Result<bool, String> {
        let mut lemnos = self.inner.lock().expect("lemnos runtime poisoned");
        let report = lemnos.poll_watcher_and_refresh_incremental_with_linux(&lemnos::discovery::DiscoveryContext::new(), &self.backend, watcher).map_err(|error| error.to_string())?;
        Ok(report.is_some())
    }
}

struct LemnosDiscoveryProbe {
    inner: Arc<Mutex<Lemnos>>,
    backend: LinuxBackend,
}

impl DiscoveryProbe for LemnosDiscoveryProbe {
    fn name(&self) -> &'static str {
        "lemnos-linux"
    }

    fn discover(&self, context: &DiscoveryContext) -> Result<DiscoverySnapshot, DiscoveryError> {
        let mut lemnos = self.inner.lock().expect("lemnos runtime poisoned");
        lemnos.refresh_incremental_with_linux(&lemnos::discovery::DiscoveryContext::new(), &self.backend).map_err(|error| probe_error(self.name(), error))?;

        let actionable_ids = lemnos
            .inventory()
            .devices
            .iter()
            .filter(|device| matches!(device.kind, DeviceKind::GpioLine | DeviceKind::PwmChannel | DeviceKind::I2cDevice | DeviceKind::SpiDevice | DeviceKind::Unspecified(InterfaceKind::Pwm)))
            .map(|device| device.id.clone())
            .collect::<Vec<_>>();
        for device_id in actionable_ids {
            if !lemnos.is_bound(&device_id) {
                let _ = lemnos.bind(&device_id);
            }
        }

        let resources = lemnos
            .inventory()
            .devices
            .iter()
            .filter_map(|device| {
                lemnos.state(&device.id).and_then(|state| resource_from_lemnos(context, device, Some(state)).transpose()).or_else(|| resource_from_lemnos(context, device, None).transpose())
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(DiscoverySnapshot::new(resources))
    }
}

struct LemnosResourceController {
    inner: Arc<Mutex<Lemnos>>,
}

impl ResourceController for LemnosResourceController {
    fn kind(&self) -> &'static str {
        "lemnos"
    }

    fn supported_kinds(&self) -> &'static [ResourceKind] {
        const KINDS: &[ResourceKind] = &[ResourceKind::GpioLine, ResourceKind::PwmChannel, ResourceKind::I2cDevice, ResourceKind::SpiDevice];
        KINDS
    }

    fn apply(&self, _resources: &[ResourceDescriptor], resource: &ResourceDescriptor, request: &ResourceControlRequest) -> Result<Option<ResourceControlResult>, ResourceControlError> {
        let device_id = lemnos_device_id(resource)?;
        let mut lemnos = self.inner.lock().expect("lemnos runtime poisoned");
        if !lemnos.is_bound(&device_id) {
            lemnos.bind(&device_id).map_err(|error| op_failed(resource, error))?;
        }

        let response = match request {
            ResourceControlRequest::Gpio(request) if resource.kind == ResourceKind::GpioLine => lemnos
                .request_standard(
                    device_id.clone(),
                    StandardRequest::Gpio(match &request.control {
                        GpioControl::Read => GpioRequest::Read,
                        GpioControl::Write { high } => GpioRequest::Write { level: if *high { GpioLevel::High } else { GpioLevel::Low } },
                        GpioControl::ConfigureDirection { direction, initial_high } => GpioRequest::Configure(GpioLineConfiguration {
                            direction: match direction {
                                GpioDirection::Input => lemnos::core::GpioDirection::Input,
                                GpioDirection::Output => lemnos::core::GpioDirection::Output,
                            },
                            active_low: false,
                            bias: None,
                            drive: None,
                            edge: None,
                            debounce_us: None,
                            initial_level: initial_high.map(|high| if high { GpioLevel::High } else { GpioLevel::Low }),
                        }),
                    }),
                )
                .map_err(|error| op_failed(resource, error))?,
            ResourceControlRequest::Pwm(request) if resource.kind == ResourceKind::PwmChannel => lemnos
                .request_standard(
                    device_id.clone(),
                    StandardRequest::Pwm(match &request.control {
                        PwmControl::Enable { enabled } => PwmRequest::Enable { enabled: *enabled },
                        PwmControl::SetPeriodNs { period_ns } => PwmRequest::SetPeriod { period_ns: *period_ns },
                        PwmControl::SetDutyCycleNs { duty_cycle_ns } => PwmRequest::SetDutyCycle { duty_cycle_ns: *duty_cycle_ns },
                        PwmControl::Configure { period_ns, duty_cycle_ns, enabled } => {
                            PwmRequest::Configure(PwmConfiguration { period_ns: *period_ns, duty_cycle_ns: *duty_cycle_ns, enabled: *enabled, polarity: PwmPolarity::Normal })
                        }
                    }),
                )
                .map_err(|error| op_failed(resource, error))?,
            ResourceControlRequest::I2c(request) if resource.kind == ResourceKind::I2cDevice => lemnos
                .request_standard(
                    device_id.clone(),
                    StandardRequest::I2c(match &request.control {
                        I2cControl::Read { len } => I2cRequest::Read { length: *len as u32 },
                        I2cControl::Write { bytes } => I2cRequest::Write { bytes: bytes.clone() },
                        I2cControl::WriteRead { write, read_len } => I2cRequest::WriteRead { write: write.clone(), read_length: *read_len as u32 },
                    }),
                )
                .map_err(|error| op_failed(resource, error))?,
            ResourceControlRequest::Spi(request) if resource.kind == ResourceKind::SpiDevice => lemnos
                .request_standard(
                    device_id.clone(),
                    StandardRequest::Spi(match &request.control {
                        SpiControl::Transfer { bytes } => SpiRequest::Transfer { write: bytes.clone() },
                        SpiControl::Write { bytes } => SpiRequest::Write { bytes: bytes.clone() },
                    }),
                )
                .map_err(|error| op_failed(resource, error))?,
            _ => return Ok(None),
        };

        Ok(Some(response_to_result(resource, response)))
    }
}

fn probe_error(probe: &str, error: impl ToString) -> DiscoveryError {
    DiscoveryError::ProbeFailed { probe: probe.to_string(), message: error.to_string() }
}

fn resource_from_lemnos(context: &DiscoveryContext, device: &DeviceDescriptor, state: Option<&DeviceStateSnapshot>) -> Result<Option<ResourceDescriptor>, DiscoveryError> {
    let kind = match device.kind {
        DeviceKind::GpioChip => ResourceKind::GpioChip,
        DeviceKind::GpioLine => ResourceKind::GpioLine,
        DeviceKind::PwmChip => ResourceKind::PwmChip,
        DeviceKind::PwmChannel => ResourceKind::PwmChannel,
        DeviceKind::Unspecified(InterfaceKind::Pwm) => ResourceKind::PwmChannel,
        DeviceKind::I2cBus => ResourceKind::I2cBus,
        DeviceKind::I2cDevice => ResourceKind::I2cDevice,
        DeviceKind::SpiBus => ResourceKind::SpiBus,
        DeviceKind::SpiDevice => ResourceKind::SpiDevice,
        DeviceKind::UsbBus => ResourceKind::UsbBus,
        DeviceKind::UsbDevice => ResourceKind::UsbDevice,
        DeviceKind::UsbInterface => ResourceKind::UsbInterface,
        _ => return Ok(None),
    };

    let local = device.local_id.as_ref().map(|value| value.as_str()).or_else(|| device.display_name.as_deref()).unwrap_or_else(|| device.id.as_str());

    let display_name = device.display_name.clone().unwrap_or_else(|| device.id.as_str().to_string());
    let mut builder = ResourceBuilder::new(context.local_node_id.clone(), kind, sanitize_local_component(local), display_name)
        .map_err(|error| probe_error("lemnos-linux", error))?
        .label("lemnos.device_id", device.id.as_str());

    if let Some(local_id) = &device.local_id {
        builder = builder.label("lemnos.local_id", local_id.as_str());
    }

    for (key, value) in &device.labels {
        builder = builder.label(key.clone(), value.clone());
    }

    for (key, value) in &device.properties {
        builder = add_value_label(builder, key.clone(), value);
    }

    if let Some(address) = &device.address {
        builder = builder.endpoint("lemnos", address.to_string());
    } else {
        builder = builder.endpoint("lemnos", device.id.as_str().to_string());
    }

    if matches!(device.kind, DeviceKind::Unspecified(InterfaceKind::Pwm)) && device.properties.get("linux.subsystem").and_then(Value::as_str) == Some("hwmon") {
        builder = builder.capability("fan", Some("hwmon"));
    }

    if let Some(state) = state {
        builder = builder.status(resource_status(state));
        for (key, value) in &state.telemetry {
            builder = add_value_label(builder, format!("telemetry.{key}"), value);
        }
        for (key, value) in &state.realized_config {
            builder = add_value_label(builder, format!("config.{key}"), value);
        }
        builder = builder
            .label("lemnos.lifecycle", format!("{:?}", state.lifecycle).to_lowercase())
            .label("lemnos.availability", format!("{:?}", state.availability).to_lowercase())
            .label("lemnos.health", format!("{:?}", state.health).to_lowercase());
    }

    Ok(Some(builder.build()))
}

fn resource_status(state: &DeviceStateSnapshot) -> ResourceStatus {
    match (state.availability, state.health, state.lifecycle) {
        (Availability::Missing, _, _) | (_, DeviceHealth::Offline | DeviceHealth::Failed, DeviceLifecycleState::Removed | DeviceLifecycleState::Faulted) => ResourceStatus::Missing,
        (_, DeviceHealth::Degraded | DeviceHealth::Unknown, _) | (Availability::Unknown, _, _) => ResourceStatus::Degraded,
        _ => ResourceStatus::Available,
    }
}

fn add_value_label(builder: ResourceBuilder, key: String, value: &Value) -> ResourceBuilder {
    match value {
        Value::Null => builder,
        Value::Bool(value) => builder.label(key, value.to_string()),
        Value::I64(value) => builder.label(key, value.to_string()),
        Value::U64(value) => builder.label(key, value.to_string()),
        Value::F64(value) => builder.label(key, value.to_string()),
        Value::String(value) => builder.label(key, value.clone()),
        Value::Bytes(bytes) => builder.label(key, format!("0x{}", encode_hex(bytes))),
        Value::List(_) | Value::Map(_) => builder,
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
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

fn lemnos_device_id(resource: &ResourceDescriptor) -> Result<DeviceId, ResourceControlError> {
    let raw = resource.label("lemnos.device_id").ok_or_else(|| ResourceControlError::OperationFailed { resource_id: resource.id.clone(), message: "missing lemnos.device_id label".into() })?;
    DeviceId::new(raw).map_err(|error| ResourceControlError::OperationFailed { resource_id: resource.id.clone(), message: error.to_string() })
}

fn op_failed(resource: &ResourceDescriptor, error: impl ToString) -> ResourceControlError {
    ResourceControlError::OperationFailed { resource_id: resource.id.clone(), message: error.to_string() }
}

fn response_to_result(resource: &ResourceDescriptor, response: DeviceResponse) -> ResourceControlResult {
    match response.interaction {
        InteractionResponse::Standard(StandardResponse::Gpio(response)) => match response {
            lemnos::core::GpioResponse::Level(level) => ResourceControlResult::read(resource.id.clone(), "gpio.read", ResourceControlData::Bool(matches!(level, GpioLevel::High))),
            lemnos::core::GpioResponse::Configuration(_) | lemnos::core::GpioResponse::Applied => ResourceControlResult::applied(resource.id.clone(), "gpio.write"),
        },
        InteractionResponse::Standard(StandardResponse::Pwm(response)) => match response {
            lemnos::core::PwmResponse::Configuration(_) | lemnos::core::PwmResponse::Applied => ResourceControlResult::applied(resource.id.clone(), "pwm.configure"),
        },
        InteractionResponse::Standard(StandardResponse::I2c(response)) => match response {
            I2cResponse::Bytes(bytes) => ResourceControlResult::read(resource.id.clone(), "i2c.read", ResourceControlData::Bytes(bytes)),
            I2cResponse::Transaction(_) | I2cResponse::Applied => ResourceControlResult::applied(resource.id.clone(), "i2c.write"),
        },
        InteractionResponse::Standard(StandardResponse::Spi(response)) => match response {
            SpiResponse::Bytes(bytes) => ResourceControlResult::read(resource.id.clone(), "spi.transfer", ResourceControlData::Bytes(bytes)),
            SpiResponse::Configuration(_) | SpiResponse::Applied => ResourceControlResult::applied(resource.id.clone(), "spi.write"),
        },
        _ => ResourceControlResult::applied(resource.id.clone(), "apply"),
    }
}
