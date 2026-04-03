use std::fmt;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::backends::accelerometer::Accelerometer;
use crate::backends::gyro::Gyro;
use crate::backends::magnetometer::Magnetometer;
use crate::sensor_config::SensorDeviceCfg;
use chrono::{DateTime, Utc};
use lib_math::linalg::Quaternion;
use linux_embedded_hal::I2cdev;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::drivers::{
    bmi088::{AccelRange as BmiAccelRange, Bmi088, BmiConfig, BmiSettings, GyroRange as BmiGyroRange},
    bmm150::{Bmm150, Bmm150Config, DataRate as BmmDataRate, OperationMode as BmmOperationMode, Preset as BmmPreset},
    icm20948::{Icm20948, Icm20948Config},
    icm42688p::{Icm42688p, IcmConfig as Icm42688pConfig},
};
use crate::{Error, Result};


mod constants;
mod fusion;
mod math;
pub use constants::IMU_INTERVAL_PRESETS_MS;
use constants::*;
use fusion::*;
use math::*;
#[cfg(test)]
mod tests;


/// Fusion strategy used to combine accelerometer, gyroscope, and magnetometer samples.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub enum ImuFusionMethod {
    /// Madgwick 6-axis fusion (ignores magnetometer even if present).
    #[default]
    MadgwickNoMag,
    /// Madgwick 9-axis fusion.
    Madgwick,
}

impl ImuFusionMethod {
    /// Available fusion modes supported by the library.
    pub const fn options() -> &'static [ImuFusionMethod] {
        &[ImuFusionMethod::MadgwickNoMag, ImuFusionMethod::Madgwick]
    }
}

impl FromStr for ImuFusionMethod {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "madgwick" | "ahrs" => Ok(Self::Madgwick),
            "madgwick_no_mag" | "madgwick_nomag" | "madgwick_6axis" | "madgwick6" | "6axis" | "no_mag" | "nomag" => Ok(Self::MadgwickNoMag),
            other => Err(Error::InvalidConfig(format!("unknown IMU fusion method '{other}'"))),
        }
    }
}

impl fmt::Display for ImuFusionMethod {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ImuFusionMethod::Madgwick => fmt.write_str("madgwick"),
            ImuFusionMethod::MadgwickNoMag => fmt.write_str("madgwick_no_mag"),
        }
    }
}

/// Supported output ranges for fused IMU readings.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum ImuRange {
    /// Values span 0°..360°.
    ZeroTo360,
    /// Values span -180°..180°.
    #[default]
    Negative180To180,
}

impl ImuRange {
    /// Available range options supported by the library.
    pub const fn options() -> &'static [ImuRange] {
        &[ImuRange::ZeroTo360, ImuRange::Negative180To180]
    }
}

impl fmt::Display for ImuRange {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.write_str(match self {
            ImuRange::ZeroTo360 => "zero_to_360",
            ImuRange::Negative180To180 => "negative_180_to_180",
        })
    }
}

impl FromStr for ImuRange {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "zero_to_360" | "0_360" | "0-360" | "360" => Ok(Self::ZeroTo360),
            "negative_180_to_180" | "-180_180" | "-180..180" | "+-180" | "180" => Ok(Self::Negative180To180),
            _ => Err(Error::InvalidConfig(format!("unknown IMU range: {s}"))),
        }
    }
}

/// Runtime IMU configuration shared between the runtime and fusion logic.
#[derive(Debug, Clone)]
pub struct ImuSettings {
    pub range: ImuRange,
    pub update_interval: Duration,
    pub fusion: ImuFusionMethod,
    pub yaw_offset_deg: f32,
    /// Rotation applied to all IMU vectors + orientation to align the IMU axes with the chassis/device frame.
    pub mount_correction: Quaternion,
    pub dr_velocity_damp_tau_seconds: f32,
    pub dr_still_velocity_zero_tau_seconds: f32,
    pub dr_max_accel_world_mps2: f32,
    pub dr_max_speed_mps: f32,
    pub dr_max_position_m: f32,
    pub dr_lock_position: bool,
}

impl ImuSettings {
    pub fn beta(&self) -> f32 {
        match self.fusion {
            ImuFusionMethod::Madgwick => MADGWICK_BETA,
            ImuFusionMethod::MadgwickNoMag => MADGWICK_BETA,
        }
    }

    pub fn default_dr_velocity_damp_tau_seconds() -> f32 {
        DR_VELOCITY_DAMP_TAU_SECONDS
    }

    pub fn default_dr_still_velocity_zero_tau_seconds() -> f32 {
        DR_STILL_VELOCITY_ZERO_TAU_SECONDS
    }

    pub fn default_dr_max_accel_world_mps2() -> f32 {
        DR_MAX_ACCEL_WORLD_MPS2
    }

    pub fn default_dr_max_speed_mps() -> f32 {
        DR_MAX_SPEED_MPS
    }

    pub fn default_dr_max_position_m() -> f32 {
        DR_MAX_POSITION_M
    }

    pub fn default_dr_lock_position() -> bool {
        DR_LOCK_POSITION_DEFAULT
    }
}

#[derive(Debug, Clone, Default)]
pub struct ImuSources {
    pub accel_gyro: Option<String>,
    pub magnetometer: Option<String>,
}

/// Latest IMU measurement alongside fusion metadata.
#[derive(Debug, Clone)]
pub struct ImuSample {
    pub accel: [f32; 3],
    pub gyro: [f32; 3],
    pub mag: Option<[f32; 3]>,
    pub quaternion: [f32; 4],
    pub orientation: [f32; 3],
    /// Linear acceleration in device coordinates (g), with gravity removed and lightly low-pass filtered.
    pub linear_accel: [f32; 3],
    /// Corrected linear acceleration in world frame (m/s²) used for DR integration.
    pub corrected_world_accel_mps2: [f32; 3],
    /// True when the device is confidently moving (filters out small fan vibration).
    pub is_moving: bool,
    /// Fast motion detector using near-raw gravity-compensated acceleration (g).
    pub is_moving_fast: bool,
    /// True when stillness heuristics classify the device as still.
    pub is_still: bool,
    /// Stillness confidence score (0..1).
    pub stillness_confidence: f32,
    /// True when rotational dynamics are likely contaminating linear acceleration estimates.
    pub rotation_contaminated: bool,
    /// Motion score in g (norm of filtered linear acceleration).
    pub motion_g: f32,
    /// Fast motion score in g (norm of near-raw gravity-compensated acceleration).
    pub motion_fast_g: f32,
    /// Adaptive motion threshold used by the fast detector in g.
    pub motion_fast_threshold_g: f32,
    /// Estimated stationary noise floor used by the fast detector in g.
    pub motion_noise_floor_g: f32,
    /// Estimated velocity in world coordinates (m/s).
    pub velocity_world: [f32; 3],
    /// Frame-to-frame velocity delta in world coordinates (m/s).
    pub velocity_delta_world: [f32; 3],
    /// Norm of world-frame velocity in m/s.
    pub linear_speed_mps: f32,
    /// World-frame velocity norm normalized to configured max speed (0..1).
    pub linear_speed_normalized: f32,
    /// Bias-corrected angular velocity in device frame (deg/s).
    pub angular_velocity_dps: [f32; 3],
    /// Norm of bias-corrected angular velocity (deg/s).
    pub angular_speed_dps: f32,
    /// Angular speed normalized to contamination full-scale threshold (0..1).
    pub angular_speed_normalized: f32,
    /// Estimated gyro bias in device frame (deg/s).
    pub gyro_bias_dps: [f32; 3],
    /// Estimated position in world coordinates (m).
    pub position_world: [f32; 3],
    /// Confidence score for dead-reckoning velocity/position quality (0..1).
    pub dr_confidence: f32,
    pub dr_velocity_damp_tau_seconds: f32,
    pub dr_still_velocity_zero_tau_seconds: f32,
    pub dr_max_accel_world_mps2: f32,
    pub dr_max_speed_mps: f32,
    pub dr_max_position_m: f32,
    pub dr_lock_position: bool,
    pub fusion: ImuFusionMethod,
    pub range: ImuRange,
    pub update_interval: Duration,
    pub updated_at: DateTime<Utc>,
    pub dt_seconds: f32,
    pub sources: ImuSources,
}

#[derive(Debug, Clone)]
pub struct ImuFusionState {
    quaternion: Quaternion,
    gyro_bias_deg_per_sec: [f32; 3],
    gyro_bias_time_seconds: f32,
    post_motion_seconds: f32,
    accel_lp: Option<[f32; 3]>,
    gyro_lp: Option<[f32; 3]>,
    mag_lp: Option<[f32; 3]>,
    mag_norm_lp: Option<f32>,
    accel_bias_g: [f32; 3],
    linear_accel_lp: [f32; 3],
    motion_fast_active: bool,
    motion_noise_floor_g: f32,
    rotation_recovery_seconds: f32,
    direction_change_hold_seconds: f32,
    linear_world_mps2_lp: [f32; 3],
    world_accel_bias_mps2: [f32; 3],
    velocity_world_mps: [f32; 3],
    position_world_m: [f32; 3],
    stillness_confidence_lp: f32,
    stillness_confident_time_seconds: f32,
    still_time_seconds: f32,
    relevel_time_seconds: f32,
    initialized: bool,
}

impl Default for ImuFusionState {
    fn default() -> Self {
        Self {
            quaternion: Quaternion::IDENTITY,
            gyro_bias_deg_per_sec: [0.0; 3],
            gyro_bias_time_seconds: 0.0,
            post_motion_seconds: 10.0,
            accel_lp: None,
            gyro_lp: None,
            mag_lp: None,
            mag_norm_lp: None,
            accel_bias_g: [0.0; 3],
            linear_accel_lp: [0.0; 3],
            motion_fast_active: false,
            motion_noise_floor_g: 0.012,
            rotation_recovery_seconds: 0.0,
            direction_change_hold_seconds: 0.0,
            linear_world_mps2_lp: [0.0; 3],
            world_accel_bias_mps2: [0.0; 3],
            velocity_world_mps: [0.0; 3],
            position_world_m: [0.0; 3],
            stillness_confidence_lp: 0.0,
            stillness_confident_time_seconds: 0.0,
            still_time_seconds: 0.0,
            relevel_time_seconds: 0.0,
            initialized: false,
        }
    }
}

impl ImuFusionState {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn reset_pose(&mut self) {
        self.velocity_world_mps = [0.0; 3];
        self.position_world_m = [0.0; 3];
        self.direction_change_hold_seconds = 0.0;
    }
}

#[derive(Debug, Clone, Default)]
pub struct ImuProbeConfig {
    pub accel_range_g: u16,
    pub gyro_range_dps: u16,
}

pub struct ImuDevice {
    accel_gyro: AccelGyro,
    mag: Option<MagDevice>,
}

enum AccelGyro {
    Bmi088 { dev: Arc<Mutex<Bmi088<I2cdev>>>, label: String },
    Icm20948 { dev: Arc<Mutex<Icm20948<I2cdev>>>, label: String },
    Icm42688p { dev: Arc<Mutex<Icm42688p<I2cdev>>>, label: String },
}

enum MagDevice {
    Bmm150 { dev: Arc<Mutex<Bmm150<I2cdev>>>, label: String, min_interval: Duration, cached_sample: Option<CachedMagSample> },
}

#[derive(Debug, Clone, Copy)]
struct CachedMagSample {
    sampled_at: Instant,
    value: Option<[f32; 3]>,
}

impl ImuDevice {
    pub fn sources(&self) -> ImuSources {
        ImuSources {
            accel_gyro: Some(match &self.accel_gyro {
                AccelGyro::Bmi088 { label, .. } => label.clone(),
                AccelGyro::Icm20948 { label, .. } => label.clone(),
                AccelGyro::Icm42688p { label, .. } => label.clone(),
            }),
            magnetometer: self.mag.as_ref().map(|mag| match mag {
                MagDevice::Bmm150 { label, .. } => label.clone(),
            }),
        }
    }

    pub fn detect(devices: &[SensorDeviceCfg], config: &ImuProbeConfig) -> Option<Self> {
        if let Some(bmi) = Self::build_bmi(devices, config) {
            let mag = Self::build_bmm(devices);
            return Some(Self { accel_gyro: bmi, mag });
        }

        if let Some(icm) = Self::build_icm20948(devices, config) {
            let mag = Self::build_bmm(devices);
            return Some(Self { accel_gyro: icm, mag });
        }

        if let Some(icm) = Self::build_icm42688p(devices, config) {
            let mag = Self::build_bmm(devices);
            return Some(Self { accel_gyro: icm, mag });
        }

        warn!("No supported IMU devices found in sensor configuration");
        None
    }

    async fn read_axes(&mut self) -> Result<ImuAxes> {
        let (accel, gyro) = match &self.accel_gyro {
            AccelGyro::Bmi088 { dev, .. } => {
                let mut guard = dev.lock().await;
                (guard.read_accel()?, guard.read_gyro()?)
            }
            AccelGyro::Icm20948 { dev, .. } => {
                let mut guard = dev.lock().await;
                (guard.read_accel()?, guard.read_gyro()?)
            }
            AccelGyro::Icm42688p { dev, .. } => {
                let mut guard = dev.lock().await;
                (guard.read_accel()?, guard.read_gyro()?)
            }
        };

        let mag = match &mut self.mag {
            Some(MagDevice::Bmm150 { dev, min_interval, cached_sample, .. }) => {
                let now = Instant::now();
                let reuse_cached = cached_sample.map(|sample| now.duration_since(sample.sampled_at) < *min_interval).unwrap_or(false);
                if reuse_cached {
                    cached_sample.and_then(|sample| sample.value)
                } else {
                    let value = {
                        let mut guard = dev.lock().await;
                        guard.read_mag().ok()
                    };
                    *cached_sample = Some(CachedMagSample { sampled_at: now, value });
                    value
                }
            }
            None => None,
        };

        Ok(ImuAxes { accel, gyro, mag })
    }

    pub async fn sample(&mut self, dt_seconds: f32, settings: &ImuSettings, fusion_state: &mut ImuFusionState) -> Result<ImuSample> {
        let axes = self.read_axes().await?;
        // Remap the physical IMU frame into the backend IMU frame convention:
        // backend: +X forward, +Y right, +Z up (right-handed).
        let (mut accel, mut gyro, mut mag) = match &self.accel_gyro {
            AccelGyro::Bmi088 { .. } => (axes.accel, axes.gyro, axes.mag),
            _ => (remap_sensor_to_backend(axes.accel), remap_sensor_to_backend(axes.gyro), axes.mag.map(remap_sensor_to_backend)),
        };

        if matches!(&self.accel_gyro, AccelGyro::Bmi088 { .. }) {
            let correction = imu_frame_correction();
            accel = rotate_vec3(correction, accel);
            gyro = rotate_vec3(correction, gyro);
            mag = mag.map(|m| rotate_vec3(correction, m));
        }

        let fused = fuse_orientation_stateful(accel, gyro, mag, dt_seconds, settings, fusion_state);

        let mount = quat_normalize(settings.mount_correction).unwrap_or(settings.mount_correction);
        let mut quaternion = fused.quaternion;
        let mut orientation = fused.orientation;
        let mut linear_accel = fused.linear_accel;

        if mount != Quaternion::IDENTITY {
            accel = rotate_vec3(mount, accel);
            gyro = rotate_vec3(mount, gyro);
            mag = mag.map(|m| rotate_vec3(mount, m));
            linear_accel = rotate_vec3(mount, linear_accel);

            let mut q = Quaternion::new(quaternion[0], quaternion[1], quaternion[2], quaternion[3]);
            let mount_inv = Quaternion::new(mount.w, -mount.x, -mount.y, -mount.z);
            q = quat_normalize(q * mount_inv).unwrap_or(q);
            quaternion = [q.w, q.x, q.y, q.z];

            let (roll, pitch, yaw) = quat_to_euler_deg(q);
            orientation = [normalize_angle(roll, settings.range), normalize_angle(pitch, settings.range), normalize_angle(yaw, settings.range)];
        }

        Ok(ImuSample {
            accel,
            gyro,
            mag,
            quaternion,
            orientation,
            linear_accel,
            corrected_world_accel_mps2: fused.corrected_world_accel_mps2,
            is_moving: fused.is_moving,
            is_moving_fast: fused.is_moving_fast,
            is_still: fused.is_still,
            stillness_confidence: fused.stillness_confidence,
            rotation_contaminated: fused.rotation_contaminated,
            motion_g: fused.motion_g,
            motion_fast_g: fused.motion_fast_g,
            motion_fast_threshold_g: fused.motion_fast_threshold_g,
            motion_noise_floor_g: fused.motion_noise_floor_g,
            velocity_world: fused.velocity_world,
            velocity_delta_world: fused.velocity_delta_world,
            linear_speed_mps: fused.linear_speed_mps,
            linear_speed_normalized: fused.linear_speed_normalized,
            angular_velocity_dps: fused.angular_velocity_dps,
            angular_speed_dps: fused.angular_speed_dps,
            angular_speed_normalized: fused.angular_speed_normalized,
            gyro_bias_dps: fused.gyro_bias_dps,
            position_world: fused.position_world,
            dr_confidence: fused.dr_confidence,
            dr_velocity_damp_tau_seconds: settings.dr_velocity_damp_tau_seconds,
            dr_still_velocity_zero_tau_seconds: settings.dr_still_velocity_zero_tau_seconds,
            dr_max_accel_world_mps2: settings.dr_max_accel_world_mps2,
            dr_max_speed_mps: settings.dr_max_speed_mps,
            dr_max_position_m: settings.dr_max_position_m,
            dr_lock_position: settings.dr_lock_position,
            fusion: settings.fusion,
            range: settings.range,
            update_interval: settings.update_interval,
            updated_at: Utc::now(),
            dt_seconds,
            sources: self.sources(),
        })
    }

    fn build_bmi(devices: &[SensorDeviceCfg], config: &ImuProbeConfig) -> Option<AccelGyro> {
        let mut accel_entry: Option<&SensorDeviceCfg> = None;
        let mut gyro_entry: Option<&SensorDeviceCfg> = None;
        for device in devices.iter().filter(|d| d.driver.eq_ignore_ascii_case("bmi088")) {
            match device.address {
                0x18 | 0x19 => accel_entry = Some(device),
                0x68 | 0x69 => gyro_entry = Some(device),
                _ => {
                    if accel_entry.is_none() {
                        accel_entry = Some(device);
                    } else if gyro_entry.is_none() {
                        gyro_entry = Some(device);
                    }
                }
            }
        }

        let (accel, gyro) = match (accel_entry, gyro_entry) {
            (Some(accel), Some(gyro)) if accel.bus == gyro.bus => (accel, gyro),
            (Some(accel), Some(gyro)) => {
                warn!(accel_bus = accel.bus, gyro_bus = gyro.bus, "BMI088 addresses found on different buses; skipping combined IMU");
                return None;
            }
            _ => return None,
        };

        let settings = BmiSettings {
            gyro_range: bmi_gyro_from_dps(config.gyro_range_dps),
            gyro_bandwidth: crate::drivers::bmi088::GyroBandwidth::Odr200Hz23,
            accel_range: bmi_accel_from_g(config.accel_range_g),
            accel_odr: crate::drivers::bmi088::AccelOdr::Hz200,
            accel_bandwidth: crate::drivers::bmi088::AccelBandwidth::Normal,
        };
        let path = format!("/dev/i2c-{}", accel.bus);
        for attempt in 1..=I2C_INIT_RETRIES {
            let dev = match I2cdev::new(path.clone()) {
                Ok(dev) => dev,
                Err(err) => {
                    warn!(%err, bus = accel.bus, %path, attempt, "failed to open I2C bus for BMI088");
                    return None;
                }
            };
            match Bmi088::new_with_config(dev, BmiConfig { accel_address: accel.address, gyro_address: gyro.address, settings }) {
                Ok(driver) => {
                    info!(bus = accel.bus, accel = format_args!("{:#04x}", accel.address), gyro = format_args!("{:#04x}", gyro.address), "BMI088 IMU initialized");
                    let label = format!("BMI088 on i2c-{} (0x{:02X}/0x{:02X})", accel.bus, accel.address, gyro.address);
                    return Some(AccelGyro::Bmi088 { dev: Arc::new(Mutex::new(driver)), label });
                }
                Err(err) => {
                    warn!(bus = accel.bus, error = %err, attempt, "failed to initialize BMI088 IMU");
                    std::thread::sleep(std::time::Duration::from_millis(I2C_INIT_RETRY_SLEEP_MS));
                }
            }
        }
        None
    }

    fn build_icm20948(devices: &[SensorDeviceCfg], config: &ImuProbeConfig) -> Option<AccelGyro> {
        let entry = devices.iter().find(|d| d.driver.eq_ignore_ascii_case("icm-20948") || d.driver.eq_ignore_ascii_case("icm20948"))?;
        let path = format!("/dev/i2c-{}", entry.bus);
        let dev = match I2cdev::new(path.clone()) {
            Ok(dev) => dev,
            Err(err) => {
                warn!(%err, bus = entry.bus, %path, "failed to open I2C bus for ICM-20948");
                return None;
            }
        };
        let mut cfg = Icm20948Config::new(config.accel_range_g, config.gyro_range_dps);
        cfg.address = entry.address;
        match Icm20948::new_with_config(dev, cfg) {
            Ok(driver) => {
                info!(bus = entry.bus, address = format_args!("{:#04x}", entry.address), "ICM-20948 IMU initialized");
                let label = format!("ICM-20948 on i2c-{} (0x{:02X})", entry.bus, entry.address);
                Some(AccelGyro::Icm20948 { dev: Arc::new(Mutex::new(driver)), label })
            }
            Err(err) => {
                warn!(bus = entry.bus, error = %err, "failed to initialize ICM-20948");
                None
            }
        }
    }

    fn build_icm42688p(devices: &[SensorDeviceCfg], config: &ImuProbeConfig) -> Option<AccelGyro> {
        let entry = devices.iter().find(|d| d.driver.eq_ignore_ascii_case("icm-42688p") || d.driver.eq_ignore_ascii_case("icm42688p"))?;
        let path = format!("/dev/i2c-{}", entry.bus);
        let dev = match I2cdev::new(path.clone()) {
            Ok(dev) => dev,
            Err(err) => {
                warn!(%err, bus = entry.bus, %path, "failed to open I2C bus for ICM-42688P");
                return None;
            }
        };
        let cfg = Icm42688pConfig::new(config.accel_range_g, config.gyro_range_dps);
        match Icm42688p::new_with_config(dev, cfg) {
            Ok(driver) => {
                info!(bus = entry.bus, address = format_args!("{:#04x}", entry.address), "ICM-42688P IMU initialized");
                let label = format!("ICM-42688P on i2c-{} (0x{:02X})", entry.bus, entry.address);
                Some(AccelGyro::Icm42688p { dev: Arc::new(Mutex::new(driver)), label })
            }
            Err(err) => {
                warn!(bus = entry.bus, error = %err, "failed to initialize ICM-42688P");
                None
            }
        }
    }

    fn build_bmm(devices: &[SensorDeviceCfg]) -> Option<MagDevice> {
        let entry = devices.iter().find(|d| d.driver.eq_ignore_ascii_case("bmm150"))?;
        let cfg = Bmm150Config { address: entry.address, data_rate: BmmDataRate::Hz30, preset: BmmPreset::Regular, mode: BmmOperationMode::Normal };
        let min_interval = Duration::from_secs_f32((1.0 / cfg.data_rate.hertz()).max(1.0 / 1000.0));
        let path = format!("/dev/i2c-{}", entry.bus);
        for attempt in 1..=I2C_INIT_RETRIES {
            let dev = match I2cdev::new(path.clone()) {
                Ok(dev) => dev,
                Err(err) => {
                    warn!(%err, bus = entry.bus, %path, attempt, "failed to open I2C bus for BMM150");
                    return None;
                }
            };
            match Bmm150::new_with_config(dev, cfg) {
                Ok(driver) => {
                    info!(bus = entry.bus, address = format_args!("{:#04x}", entry.address), "BMM150 magnetometer initialized");
                    let label = format!("BMM150 on i2c-{} (0x{:02X})", entry.bus, entry.address);
                    return Some(MagDevice::Bmm150 { dev: Arc::new(Mutex::new(driver)), label, min_interval, cached_sample: None });
                }
                Err(err) => {
                    warn!(bus = entry.bus, error = %err, attempt, "failed to initialize BMM150 magnetometer");
                    std::thread::sleep(std::time::Duration::from_millis(I2C_INIT_RETRY_SLEEP_MS));
                }
            }
        }
        None
    }
}
