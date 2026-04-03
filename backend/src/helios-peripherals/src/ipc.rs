use std::collections::BTreeMap;

use lib_ipc::types::{CommandId, RequestIdentity, Timestamp};
use serde::{Deserialize, Serialize};

use crate::dto::{AiModelDescriptor, AiModelId, AiModelInventory, AiModelUpload, I2cInventory, LightingCommand, LightingRuntimeState, SensorData, SensorInventory, SensorKind, SensorScope};
use lib_sensors::fan_config::{FanConfig, FanStatus};
use lib_sensors::model::SensorReading;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FirmwareUpdateStatus {
    Queued,
    Flashing,
    Complete,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    Update { command_id: CommandId, scope: SensorScope, sensor: SensorKind, payload: SensorData },
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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SensorEvent {
    /// Command acknowledgement emitted upon successful processing.
    #[serde(rename = "ack")]
    Ack { command_id: CommandId, processed_at: Timestamp },
    /// Command rejection emitted when processing fails.
    #[serde(rename = "nack")]
    Nack { command_id: CommandId, reason: String, retryable: bool },
    /// Updated sensor inventory delivered in response to [`SensorCommand::Inventory`].
    #[serde(rename = "inventory")]
    Inventory { command_id: CommandId, inventory: SensorInventory },
    /// Live or on-demand sensor snapshot payload.
    #[serde(rename = "snapshot")]
    Snapshot {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        command_id: Option<CommandId>,
        scope: SensorScope,
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
    Subscribed { command_id: CommandId, scope: SensorScope },
    /// Notification that a subscription has been terminated.
    #[serde(rename = "unsubscribed")]
    Unsubscribed { scope: SensorScope },
    /// Snapshot of AI model inventory.
    #[serde(rename = "ai_models")]
    AiModelInventory { command_id: CommandId, inventory: AiModelInventory },
    /// Notification emitted when a model upload succeeds.
    #[serde(rename = "ai_model_uploaded")]
    AiModelUploaded { command_id: CommandId, model: Box<AiModelDescriptor> },
    /// Notification that a model has been deleted.
    #[serde(rename = "ai_model_deleted")]
    AiModelDeleted { command_id: CommandId, model_id: AiModelId },
    /// Snapshot of available I2C buses and devices.
    #[serde(rename = "i2c_inventory")]
    I2cInventory { command_id: CommandId, inventory: I2cInventory },
    /// Notification emitted when firmware update status changes.
    #[serde(rename = "firmware_update")]
    FirmwareUpdate { update: FirmwareUpdate },
    /// Latest fan status snapshot.
    #[serde(rename = "fan_status")]
    FanStatus { command_id: CommandId, status: FanStatus },
    /// Current fan configuration.
    #[serde(rename = "fan_config")]
    FanConfig { command_id: CommandId, config: FanConfig },
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

impl RequestIdentity for SensorCommand {
    fn request_id(&self) -> CommandId {
        self.command_id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lib_ipc::wire::{FrameFlags, ServiceKind, StreamKind};

    fn assert_ipc_round_trip<T>(stream: StreamKind, value: &T) -> T
    where
        T: Serialize + for<'de> Deserialize<'de>,
    {
        let frame = lib_ipc::frame::Frame::encode_payload(ServiceKind::Peripherals, stream, CommandId::new(), FrameFlags::empty(), value).expect("encode ipc payload");
        frame.decode_payload().expect("decode ipc payload")
    }

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
        let deserialized: SensorEvent = assert_ipc_round_trip(StreamKind::Event, &event);
        assert!(matches!(deserialized, SensorEvent::Inventory { .. }));
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
