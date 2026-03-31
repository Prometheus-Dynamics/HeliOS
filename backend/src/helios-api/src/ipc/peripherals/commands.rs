use helios_peripherals::dto::{I2cInventory, LightingCommand, LightingRuntimeState, SensorData, SensorInventory, SensorKind, SensorScope, SensorSnapshot};
use helios_peripherals::ipc::{SensorCommand, SensorEvent};
use lib_ipc::client::ClientTransportError;
use lib_sensors::fan_config::FanConfig as PeripheralFanConfig;
use lib_sensors::fan_config::FanStatus as PeripheralFanStatus;

use crate::ipc::command_id_from_context;

use super::SensorsConnection;

impl SensorsConnection {
    pub async fn i2c_inventory(&self) -> Result<Result<I2cInventory, String>, ClientTransportError> {
        let command_id = command_id_from_context("i2c_inventory");
        self.run_command(SensorCommand::I2cInventory { command_id }, |event, command_id| match event {
            SensorEvent::I2cInventory { command_id: event_id, inventory } if event_id == command_id => Some(Ok(inventory)),
            SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => Some(Err(reason)),
            _ => None,
        })
        .await
    }

    pub async fn inventory(&self) -> Result<Result<SensorInventory, String>, ClientTransportError> {
        let command_id = command_id_from_context("inventory");
        self.run_command(SensorCommand::Inventory { command_id }, |event, command_id| match event {
            SensorEvent::Inventory { command_id: event_id, inventory } if event_id == command_id => Some(Ok(inventory)),
            SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => Some(Err(reason)),
            _ => None,
        })
        .await
    }

    pub async fn discover(&self, refresh: bool) -> Result<Result<SensorInventory, String>, ClientTransportError> {
        let command_id = command_id_from_context("discover");
        self.run_command(SensorCommand::Discover { command_id, refresh }, |event, command_id| match event {
            SensorEvent::Inventory { command_id: event_id, inventory } if event_id == command_id => Some(Ok(inventory)),
            SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => Some(Err(reason)),
            _ => None,
        })
        .await
    }

    pub async fn sensor_snapshot(&self, scope: SensorScope) -> Result<Result<SensorSnapshot, String>, ClientTransportError> {
        let command_id = command_id_from_context("snapshot");
        self.run_command(SensorCommand::Snapshot { command_id, scope: scope.clone() }, |event, command_id| match event {
            SensorEvent::Snapshot { command_id: Some(event_id), scope: event_scope, values } if event_id == command_id && event_scope == scope => Some(Ok(values)),
            SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => Some(Err(reason)),
            _ => None,
        })
        .await
    }

    pub async fn sensor_snapshot_typed(&self, scope: SensorScope) -> Result<Result<helios_peripherals::dto::SensorSnapshotTyped, String>, ClientTransportError> {
        let command_id = command_id_from_context("snapshot_typed");
        self.run_command(SensorCommand::SnapshotTyped { command_id, scope: scope.clone() }, |event, command_id| match event {
            SensorEvent::SnapshotTyped { command_id: Some(event_id), scope: event_scope, values } if event_id == command_id && event_scope == scope => Some(Ok(values)),
            SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => Some(Err(reason)),
            _ => None,
        })
        .await
    }

    pub async fn fan_status(&self) -> Result<Result<PeripheralFanStatus, String>, ClientTransportError> {
        let command_id = command_id_from_context("fan_status");
        self.run_command(SensorCommand::FanStatus { command_id }, |event, command_id| match event {
            SensorEvent::FanStatus { command_id: event_id, status } if event_id == command_id => Some(Ok(status)),
            SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => Some(Err(reason)),
            _ => None,
        })
        .await
    }

    pub async fn fan_config(&self) -> Result<Result<PeripheralFanConfig, String>, ClientTransportError> {
        let command_id = command_id_from_context("fan_config");
        self.run_command(SensorCommand::FanConfig { command_id }, |event, command_id| match event {
            SensorEvent::FanConfig { command_id: event_id, config } if event_id == command_id => Some(Ok(config)),
            SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => Some(Err(reason)),
            _ => None,
        })
        .await
    }

    pub async fn update_fan_config(&self, config: PeripheralFanConfig) -> Result<Result<(), String>, ClientTransportError> {
        let command_id = command_id_from_context("update_fan_config");
        self.run_command(SensorCommand::UpdateFanConfig { command_id, config }, |event, command_id| match event {
            SensorEvent::Ack { command_id: event_id, .. } if event_id == command_id => Some(Ok(())),
            SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => Some(Err(reason)),
            _ => None,
        })
        .await
    }

    pub async fn update_sensor(&self, scope: SensorScope, sensor: SensorKind, payload: SensorData) -> Result<Result<SensorSnapshot, String>, ClientTransportError> {
        let command_id = command_id_from_context("update_sensor");
        self.run_command(SensorCommand::Update { command_id, scope: scope.clone(), sensor, payload }, |event, command_id| match event {
            SensorEvent::Snapshot { command_id: Some(event_id), scope: event_scope, values } if event_id == command_id && event_scope == scope => Some(Ok(values)),
            SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => Some(Err(reason)),
            _ => None,
        })
        .await
    }

    pub async fn lighting_command(&self, command: LightingCommand) -> Result<Result<(), String>, ClientTransportError> {
        let command_id = command_id_from_context("lighting");
        self.run_command(SensorCommand::Lighting { command_id, command }, |event, command_id| match event {
            SensorEvent::Ack { command_id: event_id, .. } if event_id == command_id => Some(Ok(())),
            SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => Some(Err(reason)),
            _ => None,
        })
        .await
    }

    pub async fn lighting_state(&self) -> Result<Result<LightingRuntimeState, String>, ClientTransportError> {
        let command_id = command_id_from_context("lighting_state");
        self.run_command(SensorCommand::LightingState { command_id }, |event, command_id| match event {
            SensorEvent::LightingState { command_id: Some(event_id), state } if event_id == command_id => Some(Ok(state)),
            SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => Some(Err(reason)),
            _ => None,
        })
        .await
    }

    pub async fn configure_firmware(&self, device_id: String, firmware: String) -> Result<Result<(), String>, ClientTransportError> {
        let command_id = command_id_from_context("configure_firmware");
        self.run_command(SensorCommand::ConfigureFirmware { command_id, device_id, firmware }, |event, command_id| match event {
            SensorEvent::Ack { command_id: event_id, .. } if event_id == command_id => Some(Ok(())),
            SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => Some(Err(reason)),
            _ => None,
        })
        .await
    }

    pub async fn configure_alias(&self, hardware_key: String, alias: String) -> Result<Result<(), String>, ClientTransportError> {
        let command_id = command_id_from_context("configure_alias");
        self.run_command(SensorCommand::ConfigureAlias { command_id, hardware_key, alias }, |event, command_id| match event {
            SensorEvent::Ack { command_id: event_id, .. } if event_id == command_id => Some(Ok(())),
            SensorEvent::Nack { command_id: event_id, reason, .. } if event_id == command_id => Some(Err(reason)),
            _ => None,
        })
        .await
    }
}
