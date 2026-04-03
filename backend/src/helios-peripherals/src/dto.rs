use std::collections::BTreeMap;

use lib_ipc::json::JsonValue as WireJsonValue;
use lib_ipc::types::Timestamp;
use lib_sensors::model::SensorReading;
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use uuid::Uuid;

/// Binary-wire-friendly JSON payload used over IPC.
///
/// The archived representation is a real JSON tree, while human-readable serialization still uses
/// ordinary JSON values.
#[derive(Debug, Clone, PartialEq, Default, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct JsonData(pub WireJsonValue);

impl JsonData {
    #[must_use]
    pub fn from_value(value: &JsonValue) -> Self {
        Self(WireJsonValue::from(value))
    }

    #[must_use]
    pub fn to_value(&self) -> JsonValue {
        self.0.to_serde()
    }
}

impl Serialize for JsonData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serde::Serialize::serialize(&self.0, serializer)
    }
}

impl<'de> Deserialize<'de> for JsonData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Ok(Self(WireJsonValue::deserialize(deserializer)?))
    }
}

/// Serialized sensor measurement payload.
pub type SensorData = JsonData;

/// Canonical identifier for supported sensor types.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Archive, RkyvSerialize, RkyvDeserialize)]
#[rkyv(derive(PartialEq, Eq, PartialOrd, Ord))]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub enum SensorKind {
    #[serde(rename = "temperature")]
    Temperature,
    #[serde(rename = "accelerometer")]
    Accelerometer,
    #[serde(rename = "gyroscope")]
    Gyroscope,
    #[serde(rename = "magnetometer")]
    Magnetometer,
    #[serde(rename = "imu")]
    Imu,
    #[serde(rename = "power")]
    Power,
    #[serde(rename = "range")]
    Range,
    #[serde(rename = "custom")]
    Custom { name: String },
}

/// Logical ownership scope for sensor readings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Archive, RkyvSerialize, RkyvDeserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub enum SensorScope {
    #[serde(rename = "device")]
    Device,
    #[serde(rename = "stream")]
    Stream { stream_id: Uuid },
    #[serde(rename = "pipeline")]
    Pipeline { stream_id: Uuid, pipeline_id: Uuid },
    #[serde(rename = "custom")]
    Custom { name: String },
}

/// Descriptor used to surface hardware inventory via IPC.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct SensorDescriptor {
    pub backend: String,
    pub identifier: String,
    pub present: bool,
    #[serde(default)]
    #[cfg_attr(feature = "schema", schema(value_type = Option<Object>))]
    pub info: Option<JsonData>,
    #[serde(default)]
    #[cfg_attr(feature = "schema", schema(value_type = Option<Object>))]
    pub metadata: Option<JsonData>,
    #[serde(default)]
    pub stream_id: Option<Uuid>,
    #[serde(default)]
    #[cfg_attr(feature = "schema", schema(value_type = Option<Object>))]
    pub value: Option<SensorData>,
}

impl SensorDescriptor {
    /// Helper that assigns the latest measurement to the descriptor.
    #[must_use]
    pub fn with_value(mut self, value: Option<SensorData>) -> Self {
        self.value = value;
        self
    }
}

/// Aggregated collection of sensors discovered by the service.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, Archive, RkyvSerialize, RkyvDeserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct SensorInventory {
    pub sensors: Vec<SensorDescriptor>,
}

/// Convenience alias for a sensor snapshot map keyed by sensor kind.
pub type SensorSnapshot = BTreeMap<SensorKind, SensorData>;
/// Typed snapshot map keyed by sensor kind.
pub type SensorSnapshotTyped = BTreeMap<SensorKind, SensorReading>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Archive, RkyvSerialize, RkyvDeserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub enum AiModelFormat {
    TensorFlowLite,
    Onnx,
    Raw,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Archive, RkyvSerialize, RkyvDeserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct AiModelId(pub Uuid);

impl AiModelId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for AiModelId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub enum AiTensorElementType {
    U8,
    I8,
    I16,
    I32,
    F16,
    F32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Archive, RkyvSerialize, RkyvDeserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct AiTensorQuantization {
    #[serde(default)]
    pub zero_point: Vec<i64>,
    #[serde(default)]
    pub scale: Vec<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Archive, RkyvSerialize, RkyvDeserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct AiModelTensorMetadata {
    #[serde(default)]
    pub name: Option<String>,
    pub element_type: AiTensorElementType,
    #[serde(default)]
    pub shape: Vec<usize>,
    #[serde(default)]
    pub quantization: Option<AiTensorQuantization>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Archive, RkyvSerialize, RkyvDeserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct AiModelMetadata {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub preferred_batch_size: Option<usize>,
    #[serde(default)]
    pub inputs: Vec<AiModelTensorMetadata>,
    #[serde(default)]
    pub outputs: Vec<AiModelTensorMetadata>,
    #[serde(default)]
    pub labels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct AiModelDescriptor {
    pub id: AiModelId,
    pub format: AiModelFormat,
    pub metadata: AiModelMetadata,
    #[serde(default)]
    pub backend: Option<String>,
    #[serde(default)]
    pub health: AiModelHealth,
    #[serde(default)]
    #[rkyv(with = lib_ipc::archive::with::SerdeBytes)]
    pub created_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct AiModelInventory {
    pub models: Vec<AiModelDescriptor>,
    #[serde(default)]
    pub max_upload_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct AiModelHealth {
    pub status: AiModelHealthStatus,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    #[rkyv(with = lib_ipc::archive::with::SerdeBytes)]
    pub last_checked_at: Option<Timestamp>,
    #[serde(default)]
    #[rkyv(with = lib_ipc::archive::with::SerdeBytes)]
    pub last_inference_at: Option<Timestamp>,
}

impl AiModelHealth {
    pub fn ready(now: Timestamp) -> Self {
        Self { status: AiModelHealthStatus::Ready, last_error: None, last_checked_at: Some(now), last_inference_at: None }
    }

    pub fn degraded(now: Timestamp, error: String) -> Self {
        Self { status: AiModelHealthStatus::Degraded, last_error: Some(error), last_checked_at: Some(now), last_inference_at: None }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, Archive, RkyvSerialize, RkyvDeserialize)]
pub enum AiModelHealthStatus {
    #[default]
    Unknown,
    Ready,
    Degraded,
}

#[derive(Debug, Clone, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct AiModelUpload {
    #[serde(default)]
    pub id: Option<AiModelId>,
    pub format: AiModelFormat,
    #[serde(default)]
    pub metadata: AiModelMetadata,
    pub bytes: Vec<u8>,
    #[serde(default)]
    pub label_bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default, Archive, RkyvSerialize, RkyvDeserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct I2cInventory {
    pub buses: Vec<I2cBusInfo>,
    pub devices: Vec<I2cDeviceInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct I2cBusInfo {
    pub bus: u32,
    pub adapter: String,
    pub label: String,
    pub path: String,
    #[serde(default)]
    pub error_count: Option<u64>,
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Archive, RkyvSerialize, RkyvDeserialize)]
#[cfg_attr(feature = "schema", derive(utoipa::ToSchema))]
pub struct I2cDeviceInfo {
    pub bus: u32,
    pub address_hex: String,
    #[serde(default)]
    pub driver: Option<String>,
    #[serde(default)]
    pub modalias: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    pub path: String,
}
pub use lib_lighting::{LightingAnimation, LightingColor, LightingCommand, LightingRuntimeState};
