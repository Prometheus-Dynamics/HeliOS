use std::sync::Arc;

use lib_sensors::fan_config;

use crate::dto::{LightingCommand, LightingRuntimeState};
use crate::error::Result;
use crate::imu::{ImuSettings, ImuState};
use crate::ipc::SensorEvent;
use crate::orchestrator::ImuSettingsUpdate;

use super::SensorsService;

impl SensorsService {
    pub async fn lighting_command(&self, command: LightingCommand) -> Result<LightingRuntimeState> {
        self.runtimes.lighting_command(command.clone()).await?;
        let state = self.update_lighting_state(command).await;
        self.publish_event(SensorEvent::LightingState { command_id: None, state: state.clone() });
        Ok(state)
    }

    pub async fn fan_status(&self) -> Result<fan_config::FanStatus> {
        self.runtimes.ensure_fan_running().await?;
        self.runtimes.fan_status().await
    }

    pub async fn fan_config(&self) -> Result<fan_config::FanConfig> {
        self.runtimes.ensure_fan_running().await?;
        self.runtimes.fan_config().await
    }

    pub async fn update_fan_config(&self, config: fan_config::FanConfig) -> Result<()> {
        self.runtimes.update_fan_config(config).await
    }

    pub async fn start_imu(self: &Arc<Self>) -> Result<()> {
        self.runtimes.start_imu(self, self.shutdown.child_token()).await
    }

    pub async fn stop_imu(&self) -> Result<()> {
        self.runtimes.stop_imu().await
    }

    pub async fn start_power(self: &Arc<Self>) -> Result<()> {
        self.runtimes.start_power(self, self.shutdown.child_token()).await
    }

    pub async fn stop_power(&self) -> Result<()> {
        self.runtimes.stop_power().await
    }

    pub async fn restart_runtimes(self: &Arc<Self>) -> Result<()> {
        self.runtimes.restart_all(self, self.shutdown.child_token()).await
    }

    pub async fn imu_state(&self) -> Option<ImuState> {
        self.runtimes.imu_state().await
    }

    pub async fn current_imu_settings(&self) -> Option<ImuSettings> {
        self.runtimes.current_imu_settings().await
    }

    pub async fn reset_imu_pose(&self) -> Result<()> {
        self.runtimes.reset_imu_pose().await
    }

    pub async fn update_imu_settings(&self, update: ImuSettingsUpdate) -> Result<ImuSettings> {
        self.runtimes.update_imu_settings(update).await
    }
}
