use std::sync::Arc;
use std::time::Duration;

use lib_math::linalg::Quaternion;
use lib_sensors::fan_config;

use crate::dto::{LightingCommand, LightingRuntimeState};
use crate::error::Result;
use crate::imu::{ImuFusionMethod, ImuRange, ImuSettings, ImuState};
use crate::ipc::SensorEvent;

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

    pub async fn update_imu_settings(
        &self,
        fusion: Option<ImuFusionMethod>,
        range: Option<ImuRange>,
        interval: Option<Duration>,
        yaw_offset_deg: Option<f32>,
        mount_correction: Option<Quaternion>,
        dr_velocity_damp_tau_seconds: Option<f32>,
        dr_still_velocity_zero_tau_seconds: Option<f32>,
        dr_max_accel_world_mps2: Option<f32>,
        dr_max_speed_mps: Option<f32>,
        dr_max_position_m: Option<f32>,
        dr_lock_position: Option<bool>,
    ) -> Result<ImuSettings> {
        self.runtimes
            .update_imu_settings(
                fusion,
                range,
                interval,
                yaw_offset_deg,
                mount_correction,
                dr_velocity_damp_tau_seconds,
                dr_still_velocity_zero_tau_seconds,
                dr_max_accel_world_mps2,
                dr_max_speed_mps,
                dr_max_position_m,
                dr_lock_position,
            )
            .await
    }
}
