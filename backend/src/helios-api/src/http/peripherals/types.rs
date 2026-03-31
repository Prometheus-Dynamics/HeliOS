use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Serialize, ToSchema)]
pub struct PeripheralErrors {
    #[serde(default)]
    pub(crate) cameras: Vec<String>,
    #[serde(default)]
    pub(crate) i2c: Vec<String>,
    #[serde(default)]
    pub(crate) usb: Vec<String>,
    #[serde(default)]
    pub(crate) fan: Vec<String>,
    #[serde(default)]
    pub(crate) lighting: Vec<String>,
}

#[derive(Clone, Serialize, ToSchema)]
pub struct PeripheralInventory {
    pub(crate) cameras: Vec<helios_engine::capture::DiscoveredDevice>,
    #[serde(default)]
    pub(crate) sensors: Vec<SensorPeripheral>,
    #[serde(default)]
    pub(crate) errors: PeripheralErrors,
    #[serde(default)]
    pub(crate) i2c: Option<helios_peripherals::dto::I2cInventory>,
    #[serde(default)]
    pub(crate) usb: Vec<UsbPeripheral>,
    #[serde(default)]
    pub(crate) lighting: Option<LightingStatus>,
    #[serde(default)]
    pub(crate) fan: Option<FanStatus>,
}

#[derive(Clone, Serialize, ToSchema)]
pub struct UsbPeripheral {
    pub(crate) id: String,
    #[serde(default)]
    pub(crate) kind: Option<String>,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) present: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) warnings: Vec<UsbPeripheralWarning>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UsbPeripheralWarning {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Serialize, ToSchema)]
pub struct LightingStatus {
    #[serde(default)]
    pub(crate) present: bool,
    #[serde(default)]
    pub(crate) last_error: Option<String>,
}

#[derive(Clone, Serialize, ToSchema)]
pub struct FanStatus {
    #[serde(default)]
    pub(crate) present: bool,
    #[serde(default)]
    pub(crate) rpm: Option<u32>,
    #[serde(default)]
    pub(crate) mode: Option<String>,
    #[serde(default)]
    pub(crate) target_percent: Option<u8>,
    #[serde(default)]
    pub(crate) last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SensorPeripheralFirmwareOption {
    pub name: String,
    pub variant: String,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SensorPeripheralWarning {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SensorPeripheralFirmwareStatus {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub active: Option<String>,
    #[serde(default)]
    pub desired: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub options: Vec<SensorPeripheralFirmwareOption>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SensorPeripheral {
    pub name: String,
    pub driver_namespace: String,
    pub driver_camera_id: String,
    #[serde(default)]
    pub present: bool,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub interval: Option<String>,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub hardware_id: Option<String>,
    #[serde(default)]
    pub hardware_key: Option<String>,
    #[serde(default)]
    pub alias_identity: Option<String>,
    #[serde(default)]
    pub alias: Option<String>,
    #[serde(default)]
    pub firmware: Option<SensorPeripheralFirmwareStatus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<SensorPeripheralWarning>,
    #[serde(default)]
    #[schema(value_type = Option<Object>)]
    pub telemetry: Option<serde_json::Value>,
}

#[derive(Serialize, ToSchema)]
pub struct ErrorBody {
    pub code: String,
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl ErrorBody {
    pub(crate) fn new(code: impl Into<String>, error: impl Into<String>) -> Self {
        Self { code: code.into(), error: error.into(), details: None }
    }
}

#[derive(Clone, Serialize, ToSchema)]
pub struct CameraDiscoveryResponse {
    pub(crate) cameras: Vec<helios_engine::capture::DiscoveredDevice>,
    #[serde(default)]
    pub(crate) errors: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ConfigureSensorFirmwareRequest {
    pub device_id: String,
    pub firmware: String,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ConfigureSensorAliasRequest {
    pub hardware_key: String,
    pub alias: String,
}
