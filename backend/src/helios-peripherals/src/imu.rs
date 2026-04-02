use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;

use lib_runtime_policy::HELIOS_IMU_RUNTIME_POLICY;
use lib_sensors::imu::{ImuDevice, ImuFusionState, ImuProbeConfig, ImuSources};
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tokio::time::{Duration, MissedTickBehavior, interval, sleep};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::dto::SensorScope;
use crate::error::Result;
use crate::service::SensorsService;
pub use lib_sensors::imu::{ImuFusionMethod, ImuRange, ImuSample, ImuSettings};

const IMU_DETECT_RETRY_INTERVAL: Duration = Duration::from_secs(1);
const IMU_MAX_CONSECUTIVE_SAMPLE_ERRORS: usize = 10;

fn idle_imu_update_interval() -> Duration {
    Duration::from_millis(HELIOS_IMU_RUNTIME_POLICY.resolve().idle_interval_ms)
}

fn effective_imu_update_interval(active_interval: Duration, idle_interval: Duration, has_live_subscribers: bool) -> Duration {
    if has_live_subscribers { active_interval } else { active_interval.max(idle_interval) }
}

#[derive(Debug, Clone, Default)]
pub struct ImuState {
    pub sample: Option<ImuSample>,
    pub last_error: Option<String>,
    pub sources: ImuSources,
}

pub struct ImuRuntime {
    state: Arc<RwLock<ImuState>>,
    settings: Arc<RwLock<ImuSettings>>,
    reset_pose_requested: Arc<AtomicBool>,
    shutdown: CancellationToken,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl ImuRuntime {
    pub async fn spawn(service: &Arc<SensorsService>, shutdown: CancellationToken) -> Result<Option<Self>> {
        let config = service.config();
        let probe = ImuProbeConfig { accel_range_g: config.icm_accel_range_g(), gyro_range_dps: config.icm_gyro_range_dps() };
        let config_paths = config.config_paths();

        let settings = Arc::new(RwLock::new(ImuSettings {
            range: config.imu_range(),
            update_interval: config.imu_update_interval(),
            fusion: config.imu_fusion(),
            yaw_offset_deg: config.imu_yaw_offset_deg(),
            mount_correction: config.imu_mount_correction(),
            dr_velocity_damp_tau_seconds: ImuSettings::default_dr_velocity_damp_tau_seconds(),
            dr_still_velocity_zero_tau_seconds: ImuSettings::default_dr_still_velocity_zero_tau_seconds(),
            dr_max_accel_world_mps2: ImuSettings::default_dr_max_accel_world_mps2(),
            dr_max_speed_mps: ImuSettings::default_dr_max_speed_mps(),
            dr_max_position_m: ImuSettings::default_dr_max_position_m(),
            dr_lock_position: ImuSettings::default_dr_lock_position(),
        }));
        let state = Arc::new(RwLock::new(ImuState { sample: None, last_error: Some("IMU devices not detected".into()), sources: ImuSources::default() }));
        let reset_pose_requested = Arc::new(AtomicBool::new(false));
        let service_handle = Arc::downgrade(service);
        let settings_for_task = Arc::clone(&settings);
        let state_for_task = Arc::clone(&state);
        let reset_pose_for_task = Arc::clone(&reset_pose_requested);
        let shutdown_for_task = shutdown.clone();

        let handle = tokio::spawn(async move {
            run_imu_supervisor(service_handle, config_paths, probe, settings_for_task, state_for_task, reset_pose_for_task, shutdown_for_task).await;
        });

        Ok(Some(Self { state, settings, reset_pose_requested, shutdown, handle: Mutex::new(Some(handle)) }))
    }

    pub async fn stop(&self) -> Result<()> {
        self.shutdown.cancel();
        if let Some(handle) = self.handle.lock().await.take() {
            handle.await?;
        }
        Ok(())
    }

    pub async fn state(&self) -> ImuState {
        self.state.read().await.clone()
    }

    pub async fn update_settings(&self, settings: ImuSettings) {
        let mut guard = self.settings.write().await;
        *guard = settings;
    }

    pub async fn settings(&self) -> ImuSettings {
        self.settings.read().await.clone()
    }

    pub fn request_pose_reset(&self) {
        self.reset_pose_requested.store(true, Ordering::Release);
    }
}

async fn run_imu_supervisor(
    service: std::sync::Weak<SensorsService>,
    config_paths: Vec<std::path::PathBuf>,
    probe: ImuProbeConfig,
    settings: Arc<RwLock<ImuSettings>>,
    state: Arc<RwLock<ImuState>>,
    reset_pose_requested: Arc<AtomicBool>,
    shutdown: CancellationToken,
) {
    let idle_interval = idle_imu_update_interval();
    loop {
        if shutdown.is_cancelled() {
            break;
        }

        let devices = lib_sensors::sensor_config::load_sensor_devices(&config_paths);
        let Some(device) = ImuDevice::detect(&devices, &probe) else {
            {
                let mut guard = state.write().await;
                guard.sample = None;
                guard.sources = ImuSources::default();
                guard.last_error = Some("IMU devices not detected".into());
            }
            if let Some(service) = service.upgrade() {
                service.apply_imu_error("IMU devices not detected".into()).await;
            }
            sleep(IMU_DETECT_RETRY_INTERVAL).await;
            continue;
        };

        let sources = device.sources();
        {
            let mut guard = state.write().await;
            guard.sources = sources;
            guard.last_error = None;
        }

        let mut fusion_state = ImuFusionState::default();
        let mut previous_fusion = None;
        let mut previous_interval = None;
        let mut ticker = {
            let guard = settings.read().await;
            let mut handle = interval(guard.update_interval);
            handle.set_missed_tick_behavior(MissedTickBehavior::Delay);
            handle
        };
        let mut last_tick = Instant::now();
        let mut consecutive_errors = 0usize;
        let mut device = device;

        loop {
            tokio::select! {
                _ = shutdown.cancelled() => return,
                _ = ticker.tick() => {
                    if reset_pose_requested.swap(false, Ordering::AcqRel) {
                        fusion_state.reset_pose();
                    }

                    let now = Instant::now();
                    let dt_seconds = now.saturating_duration_since(last_tick).as_secs_f32().max(1e-3);
                    last_tick = now;

                    let settings_snapshot = settings.read().await.clone();
                    let has_live_subscribers = if let Some(service) = service.upgrade() {
                        service.has_scope_subscribers(&SensorScope::Device).await
                    } else {
                        false
                    };
                    let effective_interval = effective_imu_update_interval(settings_snapshot.update_interval, idle_interval, has_live_subscribers);
                    if previous_interval != Some(effective_interval) {
                        ticker = tokio::time::interval(effective_interval);
                        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
                        previous_interval = Some(effective_interval);
                    }
                    if previous_fusion != Some(settings_snapshot.fusion) {
                        previous_fusion = Some(settings_snapshot.fusion);
                        debug!(method = %settings_snapshot.fusion, "IMU fusion method changed");
                    }

                    match device.sample(dt_seconds, &settings_snapshot, &mut fusion_state).await {
                        Ok(sample) => {
                            consecutive_errors = 0;

                            {
                                let mut guard = state.write().await;
                                guard.last_error = None;
                                guard.sample = Some(sample.clone());
                            }
                            if let Some(service) = service.upgrade() {
                                service.apply_imu_sample(sample).await;
                            }
                        }
                        Err(err) => {
                            consecutive_errors = consecutive_errors.saturating_add(1);
                            let message = err.to_string();
                            {
                                let mut guard = state.write().await;
                                guard.last_error = Some(message.clone());
                            }
                            warn!(error = %err, count = consecutive_errors, "IMU sample failed");
                            if let Some(service) = service.upgrade() {
                                service.apply_imu_error(message).await;
                            }

                            if consecutive_errors >= IMU_MAX_CONSECUTIVE_SAMPLE_ERRORS {
                                warn!("IMU entered error state; retrying initialization");
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::effective_imu_update_interval;

    #[test]
    fn effective_imu_update_interval_keeps_active_rate_for_live_subscribers() {
        assert_eq!(effective_imu_update_interval(Duration::from_millis(20), Duration::from_millis(100), true), Duration::from_millis(20));
    }

    #[test]
    fn effective_imu_update_interval_uses_idle_floor_without_live_subscribers() {
        assert_eq!(effective_imu_update_interval(Duration::from_millis(20), Duration::from_millis(100), false), Duration::from_millis(100));
        assert_eq!(effective_imu_update_interval(Duration::from_millis(250), Duration::from_millis(100), false), Duration::from_millis(250));
    }
}
