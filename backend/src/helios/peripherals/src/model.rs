pub use orion::core::{NodeId, ResourceId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ResourceKind {
    GpioChip,
    GpioLine,
    PwmChip,
    PwmChannel,
    I2cBus,
    I2cDevice,
    SpiBus,
    SpiDevice,
    UsbBus,
    UsbDevice,
    UsbInterface,
    CaptureDevice,
    Virtual,
    Channel,
}

impl ResourceKind {
    pub const fn id_kind(self) -> &'static str {
        match self {
            Self::GpioChip => "gpio_chip",
            Self::GpioLine => "gpio_line",
            Self::PwmChip => "pwm_chip",
            Self::PwmChannel => "pwm_channel",
            Self::I2cBus => "i2c_bus",
            Self::I2cDevice => "i2c_device",
            Self::SpiBus => "spi_bus",
            Self::SpiDevice => "spi_device",
            Self::UsbBus => "usb_bus",
            Self::UsbDevice => "usb_device",
            Self::UsbInterface => "usb_interface",
            Self::CaptureDevice => "capture_device",
            Self::Virtual => "virtual",
            Self::Channel => "channel",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResourceStatus {
    Available,
    Degraded,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceCapability {
    pub name: Box<str>,
    pub detail: Option<Box<str>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceEndpoint {
    pub protocol: Box<str>,
    pub address: Box<str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceLink {
    pub target: ResourceId,
    pub relation: Box<str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceDescriptor {
    pub id: ResourceId,
    pub owner: NodeId,
    pub kind: ResourceKind,
    pub display_name: Box<str>,
    pub status: ResourceStatus,
    pub capabilities: Vec<ResourceCapability>,
    pub labels: BTreeMap<String, Box<str>>,
    pub endpoints: Vec<ResourceEndpoint>,
    pub links: Vec<ResourceLink>,
}

impl ResourceDescriptor {
    pub fn from_parts(owner: NodeId, kind: ResourceKind, local: impl AsRef<str>, display_name: impl Into<String>) -> Result<Self, orion::core::OrionError> {
        Ok(Self {
            id: ResourceId::try_new(format!("{}_{}_{}", kind.id_kind(), owner.as_str(), local.as_ref()))?,
            owner,
            kind,
            display_name: display_name.into().into_boxed_str(),
            status: ResourceStatus::Available,
            capabilities: Vec::new(),
            labels: BTreeMap::new(),
            endpoints: Vec::new(),
            links: Vec::new(),
        })
    }

    pub fn add_capability(&mut self, name: impl Into<String>, detail: Option<String>) {
        self.capabilities.push(ResourceCapability { name: name.into().into_boxed_str(), detail: detail.map(String::into_boxed_str) });
    }

    pub fn set_label(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.labels.insert(key.into(), value.into().into_boxed_str());
    }

    pub fn label(&self, key: &str) -> Option<&str> {
        self.labels.get(key).map(Box::as_ref)
    }

    pub fn add_endpoint(&mut self, protocol: impl Into<String>, address: impl Into<String>) {
        self.endpoints.push(ResourceEndpoint { protocol: protocol.into().into_boxed_str(), address: address.into().into_boxed_str() });
    }

    pub fn endpoint(&self, protocol: &str) -> Option<&str> {
        self.endpoints.iter().find(|endpoint| endpoint.protocol.as_ref() == protocol).map(|endpoint| endpoint.address.as_ref())
    }

    pub fn add_link(&mut self, target: ResourceId, relation: impl Into<String>) {
        self.links.push(ResourceLink { target, relation: relation.into().into_boxed_str() });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceControlData {
    Bool(bool),
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlStatus {
    Applied,
    Read,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceControlResult {
    pub resource_id: ResourceId,
    pub control: Box<str>,
    pub status: ControlStatus,
    pub data: Option<ResourceControlData>,
}

impl ResourceControlResult {
    pub fn applied(resource_id: ResourceId, control: impl Into<String>) -> Self {
        Self { resource_id, control: control.into().into_boxed_str(), status: ControlStatus::Applied, data: None }
    }

    pub fn read(resource_id: ResourceId, control: impl Into<String>, data: ResourceControlData) -> Self {
        Self { resource_id, control: control.into().into_boxed_str(), status: ControlStatus::Read, data: Some(data) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpioDirection {
    Input,
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpioControl {
    Read,
    Write { high: bool },
    ConfigureDirection { direction: GpioDirection, initial_high: Option<bool> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpioControlRequest {
    pub resource_id: ResourceId,
    pub owner: Option<String>,
    pub lease_generation: Option<u64>,
    pub control: GpioControl,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PwmControl {
    Enable { enabled: bool },
    SetPeriodNs { period_ns: u64 },
    SetDutyCycleNs { duty_cycle_ns: u64 },
    Configure { period_ns: u64, duty_cycle_ns: u64, enabled: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PwmControlRequest {
    pub resource_id: ResourceId,
    pub owner: Option<String>,
    pub lease_generation: Option<u64>,
    pub control: PwmControl,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum I2cControl {
    Read { len: usize },
    Write { bytes: Vec<u8> },
    WriteRead { write: Vec<u8>, read_len: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct I2cControlRequest {
    pub resource_id: ResourceId,
    pub owner: Option<String>,
    pub lease_generation: Option<u64>,
    pub control: I2cControl,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpiControl {
    Transfer { bytes: Vec<u8> },
    Write { bytes: Vec<u8> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpiControlRequest {
    pub resource_id: ResourceId,
    pub owner: Option<String>,
    pub lease_generation: Option<u64>,
    pub control: SpiControl,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceControlRequest {
    Gpio(GpioControlRequest),
    Pwm(PwmControlRequest),
    I2c(I2cControlRequest),
    Spi(SpiControlRequest),
}

impl ResourceControlRequest {
    pub fn resource_id(&self) -> &ResourceId {
        match self {
            Self::Gpio(request) => &request.resource_id,
            Self::Pwm(request) => &request.resource_id,
            Self::I2c(request) => &request.resource_id,
            Self::Spi(request) => &request.resource_id,
        }
    }

    pub fn control_kind(&self) -> &'static str {
        match self {
            Self::Gpio(_) => "gpio",
            Self::Pwm(_) => "pwm",
            Self::I2c(_) => "i2c",
            Self::Spi(_) => "spi",
        }
    }

    pub fn action_kind(&self) -> String {
        match self {
            Self::Gpio(request) => match request.control {
                GpioControl::Read => "gpio.read".into(),
                GpioControl::Write { .. } => "gpio.write".into(),
                GpioControl::ConfigureDirection { .. } => "gpio.configure_direction".into(),
            },
            Self::Pwm(request) => match request.control {
                PwmControl::Enable { .. } => "pwm.enable".into(),
                PwmControl::SetPeriodNs { .. } => "pwm.set_period_ns".into(),
                PwmControl::SetDutyCycleNs { .. } => "pwm.set_duty_cycle_ns".into(),
                PwmControl::Configure { .. } => "pwm.configure".into(),
            },
            Self::I2c(request) => match request.control {
                I2cControl::Read { .. } => "i2c.read".into(),
                I2cControl::Write { .. } => "i2c.write".into(),
                I2cControl::WriteRead { .. } => "i2c.write_read".into(),
            },
            Self::Spi(request) => match request.control {
                SpiControl::Transfer { .. } => "spi.transfer".into(),
                SpiControl::Write { .. } => "spi.write".into(),
            },
        }
    }
}
