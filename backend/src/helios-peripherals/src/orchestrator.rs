use std::sync::Arc;
use std::time::Duration;

use lib_math::linalg::Quaternion;
use tokio::sync::{Mutex, Notify};
use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::error::{Error, Result};
use crate::fan::FanController;
use crate::imu::{ImuFusionMethod, ImuRange, ImuRuntime, ImuSettings, ImuState};
use crate::lighting::LightingController;
use crate::power::PowerRuntime;
use crate::service::SensorsService;
use lib_sensors::fan_config;

#[derive(Debug)]
struct RuntimeSlot<T> {
    runtime: Option<Arc<T>>,
    start_in_progress: bool,
}

impl<T> Default for RuntimeSlot<T> {
    fn default() -> Self {
        Self { runtime: None, start_in_progress: false }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ImuSettingsUpdate {
    pub fusion: Option<ImuFusionMethod>,
    pub range: Option<ImuRange>,
    pub interval: Option<Duration>,
    pub yaw_offset_deg: Option<f32>,
    pub mount_correction: Option<Quaternion>,
    pub dr_velocity_damp_tau_seconds: Option<f32>,
    pub dr_still_velocity_zero_tau_seconds: Option<f32>,
    pub dr_max_accel_world_mps2: Option<f32>,
    pub dr_max_speed_mps: Option<f32>,
    pub dr_max_position_m: Option<f32>,
    pub dr_lock_position: Option<bool>,
}

pub struct RuntimeOrchestrator {
    imu: Mutex<RuntimeSlot<ImuRuntime>>,
    power: Mutex<RuntimeSlot<PowerRuntime>>,
    imu_notify: Notify,
    power_notify: Notify,
    fan: Arc<FanController>,
    lighting: Arc<LightingController>,
}

impl RuntimeOrchestrator {
    pub fn new(fan: Arc<FanController>, lighting: Arc<LightingController>) -> Self {
        Self { imu: Mutex::new(RuntimeSlot::default()), power: Mutex::new(RuntimeSlot::default()), imu_notify: Notify::new(), power_notify: Notify::new(), fan, lighting }
    }

    pub async fn start_imu(&self, service: &Arc<SensorsService>, shutdown: CancellationToken) -> Result<()> {
        loop {
            let mut slot = self.imu.lock().await;
            if slot.runtime.is_some() {
                return Ok(());
            }
            if slot.start_in_progress {
                drop(slot);
                self.imu_notify.notified().await;
                continue;
            }
            slot.start_in_progress = true;
            break;
        }

        let result = ImuRuntime::spawn(service, shutdown.child_token()).await;
        let mut error_message: Option<String> = None;
        {
            let mut slot = self.imu.lock().await;
            match result {
                Ok(Some(runtime)) => {
                    slot.runtime = Some(Arc::new(runtime));
                }
                Ok(None) => {
                    slot.runtime = None;
                    error_message = Some("IMU devices not detected".into());
                }
                Err(err) => {
                    slot.runtime = None;
                    warn!(%err, "IMU runtime start failed");
                    error_message = Some(format!("IMU runtime failed to start: {err}"));
                }
            }
            slot.start_in_progress = false;
        }
        self.imu_notify.notify_waiters();
        if let Some(message) = error_message {
            service.apply_imu_error(message).await;
        }
        Ok(())
    }

    pub async fn stop_imu(&self) -> Result<()> {
        loop {
            let runtime = {
                let mut slot = self.imu.lock().await;
                if slot.start_in_progress {
                    drop(slot);
                    self.imu_notify.notified().await;
                    continue;
                }
                slot.runtime.take()
            };
            if let Some(runtime) = runtime {
                runtime.stop().await?;
            }
            break;
        }
        Ok(())
    }

    pub async fn start_power(&self, service: &Arc<SensorsService>, shutdown: CancellationToken) -> Result<()> {
        loop {
            let mut slot = self.power.lock().await;
            if slot.runtime.is_some() {
                return Ok(());
            }
            if slot.start_in_progress {
                drop(slot);
                self.power_notify.notified().await;
                continue;
            }
            slot.start_in_progress = true;
            break;
        }

        let result = PowerRuntime::spawn(service, shutdown.child_token()).await;
        let mut start_error: Option<Error> = None;
        let runtime = match result {
            Ok(runtime) => runtime.map(Arc::new),
            Err(err) => {
                start_error = Some(err);
                None
            }
        };
        let mut slot = self.power.lock().await;
        slot.runtime = runtime;
        slot.start_in_progress = false;
        drop(slot);
        self.power_notify.notify_waiters();
        if let Some(err) = start_error {
            return Err(err);
        }
        Ok(())
    }

    pub async fn stop_power(&self) -> Result<()> {
        loop {
            let runtime = {
                let mut slot = self.power.lock().await;
                if slot.start_in_progress {
                    drop(slot);
                    self.power_notify.notified().await;
                    continue;
                }
                slot.runtime.take()
            };
            if let Some(runtime) = runtime {
                runtime.stop().await?;
            }
            break;
        }
        Ok(())
    }

    pub async fn restart_all(&self, service: &Arc<SensorsService>, shutdown: CancellationToken) -> Result<()> {
        let (power_stop, imu_stop) = tokio::join!(self.stop_power(), self.stop_imu());
        power_stop.ok();
        imu_stop.ok();
        let (imu_res, power_res) = tokio::join!(self.start_imu(service, shutdown.child_token()), self.start_power(service, shutdown.child_token()));
        imu_res.and(power_res)
    }

    pub async fn ensure_fan_running(&self) -> Result<()> {
        self.fan.ensure_running().await
    }

    pub async fn fan_status(&self) -> Result<fan_config::FanStatus> {
        self.fan.status().await
    }

    pub async fn fan_config(&self) -> Result<fan_config::FanConfig> {
        self.fan.config().await
    }

    pub async fn update_fan_config(&self, config: fan_config::FanConfig) -> Result<()> {
        self.fan.apply_config(config).await
    }

    pub async fn lighting_command(&self, command: crate::dto::LightingCommand) -> Result<()> {
        self.lighting.apply(command).await
    }

    pub async fn imu_state(&self) -> Option<ImuState> {
        let runtime = { self.imu.lock().await.runtime.clone() }?;
        Some(runtime.state().await)
    }

    pub async fn current_imu_settings(&self) -> Option<ImuSettings> {
        let runtime = { self.imu.lock().await.runtime.clone() }?;
        Some(runtime.settings().await)
    }

    pub async fn reset_imu_pose(&self) -> Result<()> {
        let runtime = {
            let slot = self.imu.lock().await;
            if slot.start_in_progress {
                return Err(Error::InvalidState("IMU runtime is starting".into()));
            }
            slot.runtime.clone()
        };
        let Some(runtime) = runtime else {
            return Err(Error::InvalidState("IMU runtime is not running".into()));
        };
        runtime.request_pose_reset();
        Ok(())
    }

    pub async fn update_imu_settings(&self, update: ImuSettingsUpdate) -> Result<ImuSettings> {
        let runtime = {
            let slot = self.imu.lock().await;
            if slot.start_in_progress {
                return Err(Error::InvalidState("IMU runtime is starting".into()));
            }
            slot.runtime.clone()
        };
        let Some(runtime) = runtime else {
            return Err(Error::InvalidState("IMU runtime is not running".into()));
        };
        if update.interval.is_some_and(|value| value.is_zero()) {
            return Err(Error::InvalidConfig("IMU update interval must be greater than zero".into()));
        }
        if update.yaw_offset_deg.is_some_and(|value| !value.is_finite()) {
            return Err(Error::InvalidConfig("IMU yaw offset must be a finite number".into()));
        }
        if update.mount_correction.is_some_and(|q| !q.w.is_finite() || !q.x.is_finite() || !q.y.is_finite() || !q.z.is_finite() || q.normalized().is_err()) {
            return Err(Error::InvalidConfig("IMU mount correction must be a valid quaternion".into()));
        }
        if update.dr_velocity_damp_tau_seconds.is_some_and(|value| !value.is_finite() || value <= 0.0) {
            return Err(Error::InvalidConfig("IMU dr_velocity_damp_tau_seconds must be greater than zero".into()));
        }
        if update.dr_still_velocity_zero_tau_seconds.is_some_and(|value| !value.is_finite() || value <= 0.0) {
            return Err(Error::InvalidConfig("IMU dr_still_velocity_zero_tau_seconds must be greater than zero".into()));
        }
        if update.dr_max_accel_world_mps2.is_some_and(|value| !value.is_finite() || value <= 0.0) {
            return Err(Error::InvalidConfig("IMU dr_max_accel_world_mps2 must be greater than zero".into()));
        }
        if update.dr_max_speed_mps.is_some_and(|value| !value.is_finite() || value <= 0.0) {
            return Err(Error::InvalidConfig("IMU dr_max_speed_mps must be greater than zero".into()));
        }
        if update.dr_max_position_m.is_some_and(|value| !value.is_finite() || value <= 0.0) {
            return Err(Error::InvalidConfig("IMU dr_max_position_m must be greater than zero".into()));
        }
        let mut settings = runtime.settings().await;
        if let Some(fusion) = update.fusion {
            settings.fusion = fusion;
        }
        if let Some(range) = update.range {
            settings.range = range;
        }
        if let Some(interval) = update.interval {
            settings.update_interval = interval;
        }
        if let Some(yaw_offset_deg) = update.yaw_offset_deg {
            settings.yaw_offset_deg = yaw_offset_deg;
        }
        if let Some(mount_correction) = update.mount_correction {
            settings.mount_correction = mount_correction;
        }
        if let Some(value) = update.dr_velocity_damp_tau_seconds {
            settings.dr_velocity_damp_tau_seconds = value.clamp(0.1, 30.0);
        }
        if let Some(value) = update.dr_still_velocity_zero_tau_seconds {
            settings.dr_still_velocity_zero_tau_seconds = value.clamp(0.02, 2.0);
        }
        if let Some(value) = update.dr_max_accel_world_mps2 {
            settings.dr_max_accel_world_mps2 = value.clamp(0.5, 30.0);
        }
        if let Some(value) = update.dr_max_speed_mps {
            settings.dr_max_speed_mps = value.clamp(0.1, 20.0);
        }
        if let Some(value) = update.dr_max_position_m {
            settings.dr_max_position_m = value.clamp(0.1, 100.0);
        }
        if let Some(value) = update.dr_lock_position {
            settings.dr_lock_position = value;
        }
        runtime.update_settings(settings.clone()).await;
        Ok(settings)
    }
}
