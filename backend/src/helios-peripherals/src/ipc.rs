use std::collections::BTreeMap;

use lib_ipc::frame::MessageKind;
use lib_ipc::protocol::ControlEvent;
use lib_ipc::server::ServerEvent;
use lib_ipc::types::{CommandId, Timestamp};
use serde::{Deserialize, Serialize};

#[cfg(feature = "tracing")]
macro_rules! error {
    ($($tt:tt)*) => {
        tracing::error!($($tt)*)
    };
}

#[cfg(not(feature = "tracing"))]
macro_rules! error {
    ($($tt:tt)*) => {};
}

use crate::dto::{AiModelDescriptor, AiModelId, AiModelInventory, AiModelUpload, I2cInventory, LightingCommand, LightingRuntimeState, SensorData, SensorInventory, SensorKind, SensorScope};
use lib_sensors::fan_config::{FanConfig, FanStatus};
use lib_sensors::model::SensorReading;

use bincode::{Decode, Encode};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Encode, Decode, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FirmwareUpdateStatus {
    Queued,
    Flashing,
    Complete,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub struct FirmwareUpdate {
    pub device_id: String,
    pub firmware: String,
    pub status: FirmwareUpdateStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress_pct: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub timestamp_ms: u64,
}

/// Commands accepted by the sensor service runtime over IPC.
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub enum SensorCommand {
    /// Perform a discovery pass and refresh the cached hardware inventory.
    #[serde(rename = "discover")]
    Discover { command_id: CommandId, refresh: bool },
    /// Retrieve the latest sensor inventory snapshot.
    #[serde(rename = "inventory")]
    Inventory { command_id: CommandId },
    /// Fetch a one-off snapshot for a given scope.
    #[serde(rename = "snapshot")]
    Snapshot { command_id: CommandId, scope: SensorScope },
    /// Fetch a typed snapshot for a given scope.
    #[serde(rename = "snapshot_typed")]
    SnapshotTyped { command_id: CommandId, scope: SensorScope },
    /// Apply a configuration or calibration payload to a sensor.
    #[serde(rename = "update")]
    Update {
        command_id: CommandId,
        scope: SensorScope,
        sensor: SensorKind,
        #[bincode(with_serde)]
        payload: SensorData,
    },
    /// Begin streaming sensor updates for a given scope.
    #[serde(rename = "subscribe")]
    Subscribe { command_id: CommandId, scope: SensorScope },
    /// Stop streaming sensor updates for a given scope.
    #[serde(rename = "unsubscribe")]
    Unsubscribe { command_id: CommandId, scope: SensorScope },
    /// Select and flash firmware for a peripheral device.
    #[serde(rename = "configure_firmware")]
    ConfigureFirmware { command_id: CommandId, device_id: String, firmware: String },
    /// Configure a human-friendly alias for a peripheral hardware key.
    ///
    /// Sending an empty alias clears the override and reverts to the generated default.
    #[serde(rename = "configure_alias")]
    ConfigureAlias { command_id: CommandId, hardware_key: String, alias: String },
    /// Retrieve the list of AI models available on the sensor service.
    #[serde(rename = "ai_list_models")]
    AiListModels { command_id: CommandId },
    /// Upload or replace an AI model artifact.
    #[serde(rename = "ai_upload_model")]
    AiUploadModel { command_id: CommandId, model: AiModelUpload },
    /// Delete a registered AI model and its artifact.
    #[serde(rename = "ai_delete_model")]
    AiDeleteModel { command_id: CommandId, model_id: AiModelId },
    /// Inspect I2C buses and attached devices.
    #[serde(rename = "i2c_inventory")]
    I2cInventory { command_id: CommandId },
    /// Apply a live lighting command (frame/animation) to the LED chain.
    #[serde(rename = "lighting")]
    Lighting { command_id: CommandId, command: LightingCommand },
    /// Retrieve the latest applied lighting runtime state.
    #[serde(rename = "lighting_state")]
    LightingState { command_id: CommandId },
    /// Retrieve the current fan status snapshot.
    #[serde(rename = "fan_status")]
    FanStatus { command_id: CommandId },
    /// Retrieve the stored fan configuration.
    #[serde(rename = "fan_config")]
    FanConfig { command_id: CommandId },
    /// Apply a new fan configuration.
    #[serde(rename = "update_fan_config")]
    UpdateFanConfig { command_id: CommandId, config: FanConfig },
}

/// Events emitted by the sensor service runtime.
#[derive(Debug, Clone, Serialize, Deserialize, Encode, Decode)]
pub enum SensorEvent {
    /// Command acknowledgement emitted upon successful processing.
    #[serde(rename = "ack")]
    Ack {
        command_id: CommandId,
        #[bincode(with_serde)]
        processed_at: Timestamp,
    },
    /// Command rejection emitted when processing fails.
    #[serde(rename = "nack")]
    Nack {
        command_id: CommandId,
        reason: String,
        retryable: bool,
    },
    /// Updated sensor inventory delivered in response to [`SensorCommand::Inventory`].
    #[serde(rename = "inventory")]
    Inventory {
        command_id: CommandId,
        inventory: SensorInventory,
    },
    /// Live or on-demand sensor snapshot payload.
    #[serde(rename = "snapshot")]
    Snapshot {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        command_id: Option<CommandId>,
        scope: SensorScope,
        #[bincode(with_serde)]
        values: BTreeMap<SensorKind, SensorData>,
    },
    /// Typed sensor snapshot payload.
    #[serde(rename = "snapshot_typed")]
    SnapshotTyped {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        command_id: Option<CommandId>,
        scope: SensorScope,
        values: BTreeMap<SensorKind, SensorReading>,
    },
    /// Confirmation that subscription has been established.
    #[serde(rename = "subscribed")]
    Subscribed {
        command_id: CommandId,
        scope: SensorScope,
    },
    /// Notification that a subscription has been terminated.
    #[serde(rename = "unsubscribed")]
    Unsubscribed {
        scope: SensorScope,
    },
    /// Snapshot of AI model inventory.
    #[serde(rename = "ai_models")]
    AiModelInventory {
        command_id: CommandId,
        inventory: AiModelInventory,
    },
    /// Notification emitted when a model upload succeeds.
    #[serde(rename = "ai_model_uploaded")]
    AiModelUploaded {
        command_id: CommandId,
        model: Box<AiModelDescriptor>,
    },
    /// Notification that a model has been deleted.
    #[serde(rename = "ai_model_deleted")]
    AiModelDeleted {
        command_id: CommandId,
        model_id: AiModelId,
    },
    /// Snapshot of available I2C buses and devices.
    #[serde(rename = "i2c_inventory")]
    I2cInventory {
        command_id: CommandId,
        inventory: I2cInventory,
    },
    /// Notification emitted when firmware update status changes.
    #[serde(rename = "firmware_update")]
    FirmwareUpdate {
        update: FirmwareUpdate,
    },
    Unknown {
        kind: u16,
        payload: Vec<u8>,
    },
    /// Latest fan status snapshot.
    #[serde(rename = "fan_status")]
    FanStatus {
        command_id: CommandId,
        status: FanStatus,
    },
    /// Current fan configuration.
    #[serde(rename = "fan_config")]
    FanConfig {
        command_id: CommandId,
        config: FanConfig,
    },
    /// Latest applied lighting command/runtime state.
    #[serde(rename = "lighting_state")]
    LightingState {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        command_id: Option<CommandId>,
        state: LightingRuntimeState,
    },
}

impl SensorCommand {
    pub fn command_id(&self) -> CommandId {
        match self {
            SensorCommand::Discover { command_id, .. }
            | SensorCommand::Inventory { command_id }
            | SensorCommand::Snapshot { command_id, .. }
            | SensorCommand::SnapshotTyped { command_id, .. }
            | SensorCommand::Update { command_id, .. }
            | SensorCommand::Subscribe { command_id, .. }
            | SensorCommand::Unsubscribe { command_id, .. }
            | SensorCommand::ConfigureFirmware { command_id, .. }
            | SensorCommand::ConfigureAlias { command_id, .. }
            | SensorCommand::AiListModels { command_id }
            | SensorCommand::AiUploadModel { command_id, .. }
            | SensorCommand::AiDeleteModel { command_id, .. }
            | SensorCommand::I2cInventory { command_id }
            | SensorCommand::Lighting { command_id, .. }
            | SensorCommand::LightingState { command_id }
            | SensorCommand::FanStatus { command_id }
            | SensorCommand::FanConfig { command_id }
            | SensorCommand::UpdateFanConfig { command_id, .. } => *command_id,
        }
    }
}

impl ServerEvent for SensorEvent {
    fn message_kind(&self) -> MessageKind {
        match self {
            SensorEvent::Ack { .. } | SensorEvent::Nack { .. } => MessageKind::Event,
            SensorEvent::Inventory { .. }
            | SensorEvent::Snapshot { .. }
            | SensorEvent::SnapshotTyped { .. }
            | SensorEvent::Subscribed { .. }
            | SensorEvent::Unsubscribed { .. }
            | SensorEvent::AiModelInventory { .. }
            | SensorEvent::AiModelUploaded { .. }
            | SensorEvent::AiModelDeleted { .. }
            | SensorEvent::I2cInventory { .. }
            | SensorEvent::FirmwareUpdate { .. }
            | SensorEvent::FanStatus { .. }
            | SensorEvent::FanConfig { .. }
            | SensorEvent::LightingState { .. }
            | SensorEvent::Unknown { .. } => MessageKind::Event,
        }
    }

    fn as_control(&self) -> Option<&ControlEvent> {
        None
    }
}

impl From<ControlEvent> for SensorEvent {
    fn from(value: ControlEvent) -> Self {
        match value {
            ControlEvent::Ack(ack) => SensorEvent::Ack { command_id: ack.command_id, processed_at: ack.processed_at },
            ControlEvent::Nack(nack) => SensorEvent::Nack { command_id: nack.command_id, reason: nack.reason, retryable: nack.retryable },
        }
    }
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SensorCommandKind {
    Discover = 0,
    Inventory = 1,
    Snapshot = 2,
    SnapshotTyped = 3,
    Update = 4,
    Subscribe = 5,
    Unsubscribe = 6,
    ConfigureFirmware = 7,
    AiListModels = 8,
    AiUploadModel = 9,
    AiDeleteModel = 10,
    I2cInventory = 11,
    Lighting = 12,
    FanStatus = 13,
    FanConfig = 14,
    UpdateFanConfig = 15,
    ConfigureAlias = 16,
    LightingState = 17,
}

impl SensorCommandKind {
    const fn to_u16(self) -> u16 {
        self as u16
    }

    fn from_u16(value: u16) -> Option<Self> {
        match value {
            0 => Some(Self::Discover),
            1 => Some(Self::Inventory),
            2 => Some(Self::Snapshot),
            3 => Some(Self::SnapshotTyped),
            4 => Some(Self::Update),
            5 => Some(Self::Subscribe),
            6 => Some(Self::Unsubscribe),
            7 => Some(Self::ConfigureFirmware),
            8 => Some(Self::AiListModels),
            9 => Some(Self::AiUploadModel),
            10 => Some(Self::AiDeleteModel),
            11 => Some(Self::I2cInventory),
            12 => Some(Self::Lighting),
            13 => Some(Self::FanStatus),
            14 => Some(Self::FanConfig),
            15 => Some(Self::UpdateFanConfig),
            16 => Some(Self::ConfigureAlias),
            17 => Some(Self::LightingState),
            _ => None,
        }
    }
}

#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SensorEventKind {
    Ack = 0,
    Nack = 1,
    Inventory = 2,
    Snapshot = 3,
    SnapshotTyped = 4,
    Subscribed = 5,
    Unsubscribed = 6,
    AiModelInventory = 7,
    AiModelUploaded = 8,
    AiModelDeleted = 9,
    I2cInventory = 10,
    FanStatus = 11,
    FanConfig = 12,
    FirmwareUpdate = 13,
    LightingState = 14,
}

impl SensorEventKind {
    const fn to_u16(self) -> u16 {
        self as u16
    }

    fn from_u16(value: u16) -> Option<Self> {
        match value {
            0 => Some(Self::Ack),
            1 => Some(Self::Nack),
            2 => Some(Self::Inventory),
            3 => Some(Self::Snapshot),
            4 => Some(Self::SnapshotTyped),
            5 => Some(Self::Subscribed),
            6 => Some(Self::Unsubscribed),
            7 => Some(Self::AiModelInventory),
            8 => Some(Self::AiModelUploaded),
            9 => Some(Self::AiModelDeleted),
            10 => Some(Self::I2cInventory),
            11 => Some(Self::FanStatus),
            12 => Some(Self::FanConfig),
            13 => Some(Self::FirmwareUpdate),
            14 => Some(Self::LightingState),
            _ => None,
        }
    }
}

#[allow(unreachable_code)]
const _: () = {
    lib_ipc::tagged_enum! {
        impl crate::ipc::SensorCommand => crate::ipc::SensorCommandKind {
            struct Discover { command_id: CommandId, refresh: bool },
            struct Inventory { command_id: CommandId },
            struct Snapshot { command_id: CommandId, scope: SensorScope },
            struct SnapshotTyped { command_id: CommandId, scope: SensorScope },
            struct Update { command_id: CommandId, scope: SensorScope, sensor: SensorKind, payload: SensorData },
            struct Subscribe { command_id: CommandId, scope: SensorScope },
            struct Unsubscribe { command_id: CommandId, scope: SensorScope },
            struct ConfigureFirmware { command_id: CommandId, device_id: String, firmware: String },
            struct ConfigureAlias { command_id: CommandId, hardware_key: String, alias: String },
            struct AiListModels { command_id: CommandId },
            struct AiUploadModel { command_id: CommandId, model: AiModelUpload },
            struct AiDeleteModel { command_id: CommandId, model_id: AiModelId },
            struct I2cInventory { command_id: CommandId },
            struct Lighting { command_id: CommandId, command: LightingCommand },
            struct LightingState { command_id: CommandId },
            struct FanStatus { command_id: CommandId },
            struct FanConfig { command_id: CommandId },
            struct UpdateFanConfig { command_id: CommandId, config: FanConfig },
        }
    }

    lib_ipc::tagged_enum! {
        impl crate::ipc::SensorEvent => crate::ipc::SensorEventKind, unknown = Unknown {
            struct Ack { command_id: CommandId, processed_at: Timestamp => with_serde },
            struct Nack { command_id: CommandId, reason: String, retryable: bool },
            struct Inventory { command_id: CommandId, inventory: SensorInventory },
            struct Snapshot { command_id: Option<CommandId>, scope: SensorScope, values: BTreeMap<SensorKind, SensorData> },
            struct SnapshotTyped { command_id: Option<CommandId>, scope: SensorScope, values: BTreeMap<SensorKind, SensorReading> },
            struct Subscribed { command_id: CommandId, scope: SensorScope },
            struct Unsubscribed { scope: SensorScope },
            struct AiModelInventory { command_id: CommandId, inventory: AiModelInventory },
            struct AiModelUploaded { command_id: CommandId, model: Box<AiModelDescriptor> },
            struct AiModelDeleted { command_id: CommandId, model_id: AiModelId },
            struct I2cInventory { command_id: CommandId, inventory: I2cInventory },
            struct FirmwareUpdate { update: FirmwareUpdate },
            struct FanStatus { command_id: CommandId, status: FanStatus },
            struct FanConfig { command_id: CommandId, config: FanConfig },
            struct LightingState { command_id: Option<CommandId>, state: LightingRuntimeState },
        }
    }
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::SensorDescriptor;

    #[test]
    fn sensor_command_roundtrip() {
        let command = SensorCommand::Discover { command_id: CommandId::new(), refresh: true };
        let serialized = serde_json::to_string(&command).expect("serialize");
        let deserialized: SensorCommand = serde_json::from_str(&serialized).expect("deserialize");
        match deserialized {
            SensorCommand::Discover { refresh, .. } => assert!(refresh),
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[test]
    fn sensor_event_roundtrip() {
        let event = SensorEvent::Inventory { command_id: CommandId::new(), inventory: SensorInventory::default() };
        let serialized = serde_json::to_string(&event).expect("serialize");
        let deserialized: SensorEvent = serde_json::from_str(&serialized).expect("deserialize");
        assert!(matches!(deserialized, SensorEvent::Inventory { .. }));
    }

    #[test]
    fn sensor_event_tagged_roundtrip() {
        let descriptor = SensorDescriptor { backend: "mock".into(), identifier: "device0".into(), present: true, info: None, metadata: None, stream_id: None, value: None };
        let inventory = SensorInventory { sensors: vec![descriptor] };
        let event = SensorEvent::Inventory { command_id: CommandId::new(), inventory };
        let envelope: lib_ipc::envelope::TaggedEnvelope = event.clone().into();
        let decoded: SensorEvent = SensorEvent::try_from(envelope).expect("decode envelope");
        assert!(matches!(decoded, SensorEvent::Inventory { .. }));
    }

    #[test]
    fn snapshot_typed_roundtrip() {
        use lib_sensors::model::{PowerSnapshot, SensorReading};
        let mut values = std::collections::BTreeMap::new();
        values.insert(SensorKind::Power, SensorReading::Power(PowerSnapshot::default()));
        let event = SensorEvent::SnapshotTyped { command_id: None, scope: SensorScope::Device, values };
        let serialized = serde_json::to_string(&event).expect("serialize");
        let deserialized: SensorEvent = serde_json::from_str(&serialized).expect("deserialize");
        assert!(matches!(deserialized, SensorEvent::SnapshotTyped { .. }));
    }
}
