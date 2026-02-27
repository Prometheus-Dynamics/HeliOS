use std::fmt;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

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

const MADGWICK_BETA: f32 = 0.06;
const I2C_INIT_RETRIES: usize = 6;
const I2C_INIT_RETRY_SLEEP_MS: u64 = 30;
const FUSION_ACCEL_LP_TAU_SECONDS: f32 = 0.35;
const FUSION_ACCEL_LP_TAU_FAST_SECONDS: f32 = 0.18;
const FUSION_MAG_LP_TAU_SECONDS: f32 = 0.60;
const FUSION_LINEAR_ACCEL_LP_TAU_SECONDS: f32 = 0.12;
const STILL_GYRO_LP_MAX_DPS: f32 = 2.5;
// Allow bias estimation to converge even if there's a few dps of constant gyro offset.
const STILL_GYRO_LP_MAX_DPS_FOR_BIAS: f32 = 8.0;
// Require a tighter gyro threshold before we "re-level" against gravity.
const STILL_GYRO_LP_MAX_DPS_FOR_RELEVEL: f32 = 1.2;
const STILL_ACCEL_MAG_ERROR_G: f32 = 0.05;
const STILL_LINEAR_ACCEL_LAX_G: f32 = 0.06;
const RELEVEL_ACCEL_MAG_ERROR_G: f32 = 0.08;
const RELEVEL_LINEAR_ACCEL_LAX_G: f32 = 0.12;
const RELEVEL_GYRO_LP_MAX_DPS: f32 = 2.0;
const MOTION_FAST_NOISE_TAU_SECONDS: f32 = 1.6;
const MOTION_FAST_FLOOR_MIN_G: f32 = 0.004;
const MOTION_FAST_FLOOR_MAX_G: f32 = 0.08;
const MOTION_FAST_ENTER_SCALE: f32 = 1.35;
const MOTION_FAST_ENTER_BIAS_G: f32 = 0.004;
const MOTION_FAST_EXIT_SCALE: f32 = 1.10;
const MOTION_FAST_EXIT_BIAS_G: f32 = 0.002;
const MOTION_FAST_THRESHOLD_MIN_G: f32 = 0.007;
const MOTION_FAST_THRESHOLD_MAX_G: f32 = 0.08;
const MOTION_ROTATION_THRESHOLD_BOOST_START_DPS: f32 = 2.5;
const MOTION_ROTATION_THRESHOLD_BOOST_FULL_DPS: f32 = 20.0;
const MOTION_ROTATION_THRESHOLD_BOOST_SCALE: f32 = 1.40;
const MOTION_ROTATION_THRESHOLD_BOOST_BIAS_G: f32 = 0.006;
const MOTION_ROTATION_RELEASE_BOOST_SCALE: f32 = 1.25;
const MOTION_ROTATION_RELEASE_BOOST_BIAS_G: f32 = 0.003;
const GYRO_BIAS_TAU_SECONDS: f32 = 3.0;
const GYRO_BIAS_TAU_FAST_SECONDS: f32 = 0.6;
const GYRO_BIAS_FAST_WINDOW_SECONDS: f32 = 0.5;
const ACCEL_BIAS_TAU_SECONDS: f32 = 1.60;
const ACCEL_BIAS_MAX_G: f32 = 0.08;
// Note: do not gate bias estimation on the *current* bias-corrected gyro magnitude.
// Bias estimation must be able to start from a zero bias estimate and converge even if the
// device has a few dps of constant gyro offset (see `STILL_GYRO_LP_MAX_DPS_FOR_BIAS`).
// After detecting motion, hold off bias updates briefly to avoid learning the deceleration tail.
const POST_MOTION_BIAS_HOLD_SECONDS: f32 = 0.20;
const GYRO_STILL_LP_TAU_SECONDS: f32 = 0.45;
const FAN_REJECT_ACCEL_LP_TAU_SECONDS: f32 = 0.75;
const STILL_LINEAR_ACCEL_LP_TAU_SECONDS: f32 = 0.28;
const RELEVEL_MIN_STILL_SECONDS: f32 = 0.18;
const RELEVEL_TAU_SECONDS: f32 = 0.18;
const STANDARD_GRAVITY_MPS2: f32 = 9.806_65;
const DR_VELOCITY_DAMP_TAU_SECONDS: f32 = 0.55;
const DR_VELOCITY_DAMP_TAU_MAX_EFFECTIVE: f32 = 1.20;
const DR_STILL_VELOCITY_ZERO_TAU_SECONDS: f32 = 0.10;
const DR_MAX_ACCEL_WORLD_MPS2: f32 = 6.0;
const DR_MAX_SPEED_MPS: f32 = 4.0;
const DR_MAX_POSITION_M: f32 = 2.0;
const DR_LOCK_POSITION_DEFAULT: bool = true;
const DR_WORLD_ACCEL_LP_TAU_SECONDS: f32 = 0.025;
const DR_WORLD_ACCEL_BIAS_TAU_SECONDS: f32 = 1.20;
const DR_WORLD_ACCEL_BIAS_XY_DECAY_TAU_SECONDS: f32 = 0.45;
const DR_WORLD_ACCEL_BIAS_XY_MAX_MPS2: f32 = 0.020;
const DR_WORLD_ACCEL_BIAS_Z_MAX_MPS2: f32 = 0.140;
const DR_WORLD_ACCEL_BIAS_XY_APPLY_WHILE_MOVING_SCALE: f32 = 0.30;
const DR_WORLD_ACCEL_DEADBAND_MPS2: f32 = 0.028;
const DR_ZUPT_RELEASE_ACCEL_MPS2: f32 = 0.075;
const DR_COAST_GYRO_MAX_DPS: f32 = 4.5;
const DR_STOP_SNAP_HOLD_SECONDS: f32 = 0.08;
const DR_STOP_SNAP_SPEED_MPS: f32 = 0.20;
const DR_ROTATION_LEAK_START_DPS: f32 = 70.0;
const DR_ROTATION_LEAK_FULL_DPS: f32 = 220.0;
const DR_ROTATION_LEAK_MIN_SCALE: f32 = 0.35;
const DR_ROTATION_CONTAMINATION_START_DPS: f32 = 20.0;
const DR_ROTATION_CONTAMINATION_FULL_DPS: f32 = 60.0;
const DR_ROTATION_TRANSLATION_DEADBAND_MPS2: f32 = 0.045;
const DR_ROTATION_TRANSLATION_SPEED_CAP_MPS: f32 = 0.85;
const DR_ROTATION_RECOVERY_HOLD_SECONDS: f32 = 0.20;
const DR_ROTATION_ACCEL_MIN_SCALE: f32 = 0.25;
const DR_DIRECTION_CHANGE_MIN_SPEED_MPS: f32 = 0.08;
const DR_DIRECTION_CHANGE_OPPOSING_ACCEL_MPS2: f32 = 0.045;
const DR_DIRECTION_CHANGE_HOLD_SECONDS: f32 = 0.10;
const DR_DIRECTION_CHANGE_DEADBAND_SCALE: f32 = 0.40;
const DR_MOVING_EVIDENCE_ACCEL_MPS2: f32 = 0.040;
const DR_DIRECTION_CHANGE_VELOCITY_BRAKE_SCALE: f32 = 0.75;
const DR_GRAVITY_AXIS_LEAK_DOMINANCE_RATIO: f32 = 1.6;
const DR_GRAVITY_AXIS_LEAK_ACCEL_MAX_MPS2: f32 = 0.35;
const DR_GRAVITY_AXIS_LEAK_SUPPRESS_SCALE: f32 = 0.20;
const DR_GRAVITY_AXIS_BIAS_RECOVERY_TAU_SECONDS: f32 = 0.30;
const DR_GRAVITY_AXIS_LEAK_MOTION_FAST_MAX_G: f32 = 0.070;
const DR_GRAVITY_AXIS_Z_VELOCITY_DAMP_TAU_SECONDS: f32 = 0.05;
const DR_CONFIDENCE_ACCEL_GOOD_MPS2: f32 = 0.04;
const DR_CONFIDENCE_ACCEL_BAD_MPS2: f32 = 0.30;
const DR_CONFIDENCE_GYRO_GOOD_DPS: f32 = 0.8;
const DR_CONFIDENCE_GYRO_BAD_DPS: f32 = 6.0;
const STILLNESS_CONFIDENCE_LP_TAU_SECONDS: f32 = 0.10;
const STILLNESS_CONFIDENCE_ZUPT_MIN: f32 = 0.88;
const STILLNESS_CONFIDENCE_BIAS_MIN: f32 = 0.72;
const STILLNESS_CONFIDENCE_IS_STILL_MIN: f32 = 0.78;
const STILLNESS_CONFIDENCE_RELEVEL_MIN: f32 = 0.62;
const STILLNESS_CONFIDENCE_ZUPT_HOLD_SECONDS: f32 = 0.12;

// Frame alignment correction for the BMI088/BMM150 IMU stack.
//
// Backend expects the IMU frame to be +X forward, +Y right, +Z up (right-handed).
// The physical module orientation differs from that convention; this quaternion is a
// constant rotation that brings sensor axes into the backend convention so gravity
// alignment and yaw/pitch/roll match real-world expectations.
//
// Quaternion is (w, x, y, z) and must be a proper rotation (det=+1).
const IMU_FRAME_CORRECTION: Quaternion = Quaternion { w: 0.004_166_289, x: -0.971_540_06, y: 0.236_728_45, z: -0.007_224_368 };

/// Default polling intervals suggested for IMU updates.
pub const IMU_INTERVAL_PRESETS_MS: &[u64] = &[20, 40, 60, 80, 100, 150, 200, 250, 500, 1000];

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
    Bmm150 { dev: Arc<Mutex<Bmm150<I2cdev>>>, label: String },
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

        let mag = match &self.mag {
            Some(MagDevice::Bmm150 { dev, .. }) => {
                let mut guard = dev.lock().await;
                guard.read_mag().ok()
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
                    return Some(MagDevice::Bmm150 { dev: Arc::new(Mutex::new(driver)), label });
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

struct ImuAxes {
    accel: [f32; 3],
    gyro: [f32; 3],
    mag: Option<[f32; 3]>,
}

struct FusedStateOutput {
    quaternion: [f32; 4],
    orientation: [f32; 3],
    linear_accel: [f32; 3],
    corrected_world_accel_mps2: [f32; 3],
    is_moving: bool,
    is_moving_fast: bool,
    is_still: bool,
    stillness_confidence: f32,
    rotation_contaminated: bool,
    motion_g: f32,
    motion_fast_g: f32,
    motion_fast_threshold_g: f32,
    motion_noise_floor_g: f32,
    velocity_world: [f32; 3],
    velocity_delta_world: [f32; 3],
    linear_speed_mps: f32,
    linear_speed_normalized: f32,
    angular_velocity_dps: [f32; 3],
    angular_speed_dps: f32,
    angular_speed_normalized: f32,
    gyro_bias_dps: [f32; 3],
    position_world: [f32; 3],
    dr_confidence: f32,
}

fn remap_sensor_to_backend(v: [f32; 3]) -> [f32; 3] {
    // [x, y, z] -> [z, -y, x]
    [v[2], -v[1], v[0]]
}

fn imu_frame_correction() -> Quaternion {
    static CACHED: OnceLock<Quaternion> = OnceLock::new();
    *CACHED.get_or_init(|| {
        let Ok(raw) = std::env::var("IMU_FRAME_CORRECTION_WXYZ") else {
            return IMU_FRAME_CORRECTION;
        };

        match parse_quat_wxyz(&raw) {
            Some(q) => q,
            None => {
                warn!(value = %raw, "Invalid IMU_FRAME_CORRECTION_WXYZ; using built-in correction");
                IMU_FRAME_CORRECTION
            }
        }
    })
}

fn parse_quat_wxyz(raw: &str) -> Option<Quaternion> {
    let mut values: Vec<f32> = raw.split(|c: char| c == ',' || c.is_whitespace()).map(str::trim).filter(|part| !part.is_empty()).filter_map(|part| part.parse::<f32>().ok()).collect();
    if values.len() != 4 {
        return None;
    }
    let q = Quaternion::new(values.remove(0), values.remove(0), values.remove(0), values.remove(0));
    quat_normalize(q)
}

fn bmi_gyro_from_dps(dps: u16) -> BmiGyroRange {
    match dps {
        0..=125 => BmiGyroRange::Dps125,
        126..=250 => BmiGyroRange::Dps250,
        251..=500 => BmiGyroRange::Dps500,
        501..=1000 => BmiGyroRange::Dps1000,
        _ => BmiGyroRange::Dps2000,
    }
}

fn bmi_accel_from_g(g: u16) -> BmiAccelRange {
    match g {
        0..=3 => BmiAccelRange::G3,
        4..=6 => BmiAccelRange::G6,
        7..=12 => BmiAccelRange::G12,
        _ => BmiAccelRange::G24,
    }
}

fn fuse_orientation_stateful(
    accel_g_raw: [f32; 3],
    gyro_deg_per_sec_raw: [f32; 3],
    mag_raw: Option<[f32; 3]>,
    dt_seconds: f32,
    settings: &ImuSettings,
    state: &mut ImuFusionState,
) -> FusedStateOutput {
    let dt_seconds = dt_seconds.clamp(1e-3, 0.25);

    // Keep a low-pass estimate of gravity to reject fan vibration (high-frequency accel noise).
    // When already still, prefer a slower time constant (stronger smoothing).
    // Right after motion stops, settle faster so pitch/roll converge quickly.
    let accel_lp_tau = if state.still_time_seconds >= 0.4 {
        FAN_REJECT_ACCEL_LP_TAU_SECONDS
    } else if state.post_motion_seconds < 0.8 {
        FUSION_ACCEL_LP_TAU_FAST_SECONDS
    } else {
        FUSION_ACCEL_LP_TAU_SECONDS
    };
    let accel_lp = match state.accel_lp {
        Some(prev) => lowpass_vec3(prev, accel_g_raw, dt_seconds, accel_lp_tau),
        None => accel_g_raw,
    };
    state.accel_lp = Some(accel_lp);

    // Low-pass gyro for stillness detection and bias estimation (fan vibration can spike raw gyro).
    let gyro_lp = match state.gyro_lp {
        Some(prev) => lowpass_vec3(prev, gyro_deg_per_sec_raw, dt_seconds, GYRO_STILL_LP_TAU_SECONDS),
        None => gyro_deg_per_sec_raw,
    };
    state.gyro_lp = Some(gyro_lp);

    // Smooth magnetometer for yaw stabilization when available.
    let mag_lp = match (state.mag_lp, mag_raw) {
        (_, None) => None,
        (Some(prev), Some(current)) => Some(lowpass_vec3(prev, current, dt_seconds, FUSION_MAG_LP_TAU_SECONDS)),
        (None, Some(current)) => Some(current),
    };
    state.mag_lp = mag_lp;

    // Stillness detection uses filtered accel magnitude + filtered gyro magnitude.
    let accel_norm = vec3_norm(accel_lp);
    let accel_mag_error = (accel_norm - 1.0).abs();
    let gyro_norm = vec3_norm(gyro_deg_per_sec_raw);
    let gyro_lp_norm = vec3_norm(gyro_lp);
    let gyro_stillness_norm = gyro_norm.min(gyro_lp_norm);

    let mag_for_init = match settings.fusion {
        ImuFusionMethod::MadgwickNoMag => None,
        ImuFusionMethod::Madgwick => select_mag_for_fusion(accel_lp, mag_lp, dt_seconds, state, gyro_stillness_norm, accel_mag_error, 1.0),
    };

    let gyro_corrected = [gyro_deg_per_sec_raw[0] - state.gyro_bias_deg_per_sec[0], gyro_deg_per_sec_raw[1] - state.gyro_bias_deg_per_sec[1], gyro_deg_per_sec_raw[2] - state.gyro_bias_deg_per_sec[2]];
    let gyro_lp_corrected = [gyro_lp[0] - state.gyro_bias_deg_per_sec[0], gyro_lp[1] - state.gyro_bias_deg_per_sec[1], gyro_lp[2] - state.gyro_bias_deg_per_sec[2]];
    let gyro_lp_corrected_norm = vec3_norm(gyro_lp_corrected);

    // Initialize only once we have a reasonable gravity direction (works in any pose / any angle).
    // This avoids locking in a bad orientation when the first sample is taken mid-motion.
    if !state.initialized && accel_norm.is_finite() && accel_norm >= 0.3 {
        let (roll, pitch) = tilt_from_accel(accel_lp);
        let yaw = mag_for_init.and_then(|mag| yaw_from_accel_mag(accel_lp, mag)).unwrap_or(0.0);
        state.quaternion = euler_deg_to_quat(roll, pitch, yaw);
        state.initialized = true;
    }

    // Reduce accelerometer correction during linear acceleration (prevents tilt drift after motion).
    let gravity_prev = rotate_world_to_body(state.quaternion, [0.0, 0.0, 1.0]);
    let linear_est_prev = [accel_lp[0] - gravity_prev[0], accel_lp[1] - gravity_prev[1], accel_lp[2] - gravity_prev[2]];
    let linear_est_norm = vec3_norm(linear_est_prev);
    let mut accel_trust = (1.0 - (linear_est_norm / 0.25)).clamp(0.0, 1.0);
    // After a movement stop, linear-accel estimates can stay elevated briefly due to filtering lag.
    // If gyro indicates we've settled and accel magnitude is reasonable, restore accel trust quickly
    // so roll/pitch snap back without taking ~1s to converge.
    if gyro_stillness_norm <= 2.2 && accel_mag_error <= 0.08 {
        accel_trust = accel_trust.max(if state.post_motion_seconds < 0.8 { 0.98 } else { 0.90 });
    }
    // Boost correction gain briefly right after a stop (still gated by accel_trust).
    let beta_boost = if state.post_motion_seconds < 0.8 && gyro_stillness_norm <= 2.2 && accel_mag_error <= 0.08 { 1.35 } else { 1.0 };
    let beta = settings.beta() * beta_boost * (0.12 + 0.88 * accel_trust);

    // Madgwick 9-axis fusion (uses mag when available).
    let mag_for_fusion = match settings.fusion {
        ImuFusionMethod::MadgwickNoMag => None,
        ImuFusionMethod::Madgwick => select_mag_for_fusion(accel_lp, mag_lp, dt_seconds, state, gyro_stillness_norm, accel_mag_error, accel_trust),
    };

    let mut fused_quat = madgwick_update(state.quaternion, gyro_corrected, accel_lp, mag_for_fusion, dt_seconds, beta);
    state.quaternion = fused_quat;

    // Linear acceleration in device frame: subtract expected gravity.
    // When confidently still, use the filtered accelerometer direction as gravity (it rejects fan noise better than the fused quaternion).
    let gravity_body =
        if state.still_time_seconds >= 0.4 { vec3_normalize(accel_lp).unwrap_or_else(|| rotate_world_to_body(fused_quat, [0.0, 0.0, 1.0])) } else { rotate_world_to_body(fused_quat, [0.0, 0.0, 1.0]) };
    let linear_accel_raw_pre_bias = [accel_g_raw[0] - gravity_body[0], accel_g_raw[1] - gravity_body[1], accel_g_raw[2] - gravity_body[2]];
    let linear_accel_raw = [linear_accel_raw_pre_bias[0] - state.accel_bias_g[0], linear_accel_raw_pre_bias[1] - state.accel_bias_g[1], linear_accel_raw_pre_bias[2] - state.accel_bias_g[2]];
    let linear_tau = if state.still_time_seconds >= 0.4 { STILL_LINEAR_ACCEL_LP_TAU_SECONDS } else { FUSION_LINEAR_ACCEL_LP_TAU_SECONDS };
    state.linear_accel_lp = lowpass_vec3(state.linear_accel_lp, linear_accel_raw, dt_seconds, linear_tau);

    let motion_g = vec3_norm(state.linear_accel_lp);
    let motion_fast_g = vec3_norm(linear_accel_raw);

    // Learn the near-still accel noise floor online and derive adaptive fast-motion thresholds.
    let noise_update_gate = !state.motion_fast_active && gyro_stillness_norm <= STILL_GYRO_LP_MAX_DPS_FOR_BIAS && accel_mag_error <= RELEVEL_ACCEL_MAG_ERROR_G;
    if noise_update_gate {
        let floor_target = motion_fast_g.clamp(MOTION_FAST_FLOOR_MIN_G, MOTION_FAST_FLOOR_MAX_G);
        state.motion_noise_floor_g = lowpass_scalar(state.motion_noise_floor_g.clamp(MOTION_FAST_FLOOR_MIN_G, MOTION_FAST_FLOOR_MAX_G), floor_target, dt_seconds, MOTION_FAST_NOISE_TAU_SECONDS);
    } else {
        state.motion_noise_floor_g = state.motion_noise_floor_g.clamp(MOTION_FAST_FLOOR_MIN_G, MOTION_FAST_FLOOR_MAX_G);
    }
    let motion_fast_threshold_g = (state.motion_noise_floor_g * MOTION_FAST_ENTER_SCALE + MOTION_FAST_ENTER_BIAS_G).clamp(MOTION_FAST_THRESHOLD_MIN_G, MOTION_FAST_THRESHOLD_MAX_G);
    let motion_fast_release_threshold_g = (state.motion_noise_floor_g * MOTION_FAST_EXIT_SCALE + MOTION_FAST_EXIT_BIAS_G).clamp(MOTION_FAST_THRESHOLD_MIN_G * 0.6, motion_fast_threshold_g * 0.9);
    let rotation_motion_boost = 1.0 - inverse_ramp_score(gyro_stillness_norm, MOTION_ROTATION_THRESHOLD_BOOST_START_DPS, MOTION_ROTATION_THRESHOLD_BOOST_FULL_DPS);
    let motion_fast_threshold_effective_g = (motion_fast_threshold_g * lerp(1.0, MOTION_ROTATION_THRESHOLD_BOOST_SCALE, rotation_motion_boost)
        + MOTION_ROTATION_THRESHOLD_BOOST_BIAS_G * rotation_motion_boost)
        .clamp(MOTION_FAST_THRESHOLD_MIN_G, MOTION_FAST_THRESHOLD_MAX_G * 2.5);
    let motion_fast_release_threshold_effective_g = (motion_fast_release_threshold_g * lerp(1.0, MOTION_ROTATION_RELEASE_BOOST_SCALE, rotation_motion_boost)
        + MOTION_ROTATION_RELEASE_BOOST_BIAS_G * rotation_motion_boost)
        .clamp(MOTION_FAST_THRESHOLD_MIN_G * 0.5, motion_fast_threshold_effective_g * 0.92);

    // Translation detector: rely on gravity-compensated accel, not gyro alone.
    // Rotating in place should not report linear movement.
    let fast_activate = motion_fast_g >= motion_fast_threshold_effective_g;
    let fast_release = motion_fast_g <= motion_fast_release_threshold_effective_g;
    if fast_activate {
        state.motion_fast_active = true;
    } else if fast_release {
        state.motion_fast_active = false;
    }

    // Bias estimation should only happen when the device is truly still.
    // In particular, do *not* update bias immediately after a quick motion stop; that tends to
    // "learn" the deceleration tail and causes post-stop drift in the direction of motion.
    let moving_candidate = state.motion_fast_active;
    if moving_candidate {
        state.post_motion_seconds = 0.0;
    } else {
        state.post_motion_seconds = (state.post_motion_seconds + dt_seconds).min(10.0);
    }
    if gyro_norm >= DR_ROTATION_CONTAMINATION_START_DPS {
        state.rotation_recovery_seconds = DR_ROTATION_RECOVERY_HOLD_SECONDS;
    } else {
        state.rotation_recovery_seconds = (state.rotation_recovery_seconds - dt_seconds).max(0.0);
    }

    let stillness_confidence_raw = stillness_confidence_from_metrics(gyro_lp_corrected_norm, accel_mag_error, motion_g);
    state.stillness_confidence_lp = lowpass_scalar(state.stillness_confidence_lp, stillness_confidence_raw, dt_seconds, STILLNESS_CONFIDENCE_LP_TAU_SECONDS);
    if state.stillness_confidence_lp >= STILLNESS_CONFIDENCE_ZUPT_MIN {
        state.stillness_confident_time_seconds = (state.stillness_confident_time_seconds + dt_seconds).min(10.0);
    } else {
        // Decay confidence hold faster than rise so ZUPT disengages quickly after motion resumes.
        state.stillness_confident_time_seconds = (state.stillness_confident_time_seconds - dt_seconds * 1.8).max(0.0);
    }
    let confident_still = state.stillness_confident_time_seconds >= STILLNESS_CONFIDENCE_ZUPT_HOLD_SECONDS;
    let bias_hold_satisfied = state.post_motion_seconds >= POST_MOTION_BIAS_HOLD_SECONDS || confident_still;
    let bias_update_allowed = bias_hold_satisfied && accel_mag_error <= STILL_ACCEL_MAG_ERROR_G && motion_g <= STILL_LINEAR_ACCEL_LAX_G;

    // Use the smaller of raw vs low-pass gyro for bias gating. This avoids a long "cooldown"
    // after fast motion where `gyro_lp` stays elevated for ~tau seconds, preventing bias updates.
    let still_for_bias = bias_update_allowed && gyro_stillness_norm <= STILL_GYRO_LP_MAX_DPS_FOR_BIAS && state.stillness_confidence_lp >= STILLNESS_CONFIDENCE_BIAS_MIN;
    let still_for_relevel = gyro_lp_corrected_norm <= RELEVEL_GYRO_LP_MAX_DPS
        && accel_mag_error <= RELEVEL_ACCEL_MAG_ERROR_G
        && motion_g <= RELEVEL_LINEAR_ACCEL_LAX_G
        && state.stillness_confidence_lp >= STILLNESS_CONFIDENCE_RELEVEL_MIN;
    if still_for_bias {
        // Estimate bias from the filtered gyro while still; it is less noisy than raw.
        state.gyro_bias_time_seconds = (state.gyro_bias_time_seconds + dt_seconds).min(60.0);
        let bias_tau = if state.gyro_bias_time_seconds < GYRO_BIAS_FAST_WINDOW_SECONDS { GYRO_BIAS_TAU_FAST_SECONDS } else { GYRO_BIAS_TAU_SECONDS };
        let blend = 1.0 - (-dt_seconds / bias_tau).exp();
        // If raw gyro is lower than the low-pass signal (common right after motion stops),
        // prefer raw so we don't "learn" the low-pass tail as bias.
        let bias_sample = if gyro_norm <= gyro_lp_norm { gyro_deg_per_sec_raw } else { gyro_lp };
        state.gyro_bias_deg_per_sec =
            [lerp(state.gyro_bias_deg_per_sec[0], bias_sample[0], blend), lerp(state.gyro_bias_deg_per_sec[1], bias_sample[1], blend), lerp(state.gyro_bias_deg_per_sec[2], bias_sample[2], blend)];
        state.accel_bias_g = lowpass_vec3(state.accel_bias_g, linear_accel_raw_pre_bias, dt_seconds, ACCEL_BIAS_TAU_SECONDS);
        state.accel_bias_g[0] = state.accel_bias_g[0].clamp(-ACCEL_BIAS_MAX_G, ACCEL_BIAS_MAX_G);
        state.accel_bias_g[1] = state.accel_bias_g[1].clamp(-ACCEL_BIAS_MAX_G, ACCEL_BIAS_MAX_G);
        state.accel_bias_g[2] = state.accel_bias_g[2].clamp(-ACCEL_BIAS_MAX_G, ACCEL_BIAS_MAX_G);
        state.still_time_seconds = (state.still_time_seconds + dt_seconds).min(10.0);
    } else {
        state.still_time_seconds = 0.0;
    }

    if still_for_relevel {
        state.relevel_time_seconds = (state.relevel_time_seconds + dt_seconds).min(10.0);
    } else {
        state.relevel_time_seconds = 0.0;
    }

    // "Still" output should reflect bias-corrected gyro (to avoid reporting motion due to a steady bias).
    let is_still = (gyro_lp_corrected_norm <= STILL_GYRO_LP_MAX_DPS && accel_mag_error <= STILL_ACCEL_MAG_ERROR_G && motion_g <= STILL_LINEAR_ACCEL_LAX_G)
        || (state.stillness_confidence_lp >= STILLNESS_CONFIDENCE_IS_STILL_MIN && confident_still);

    // Stillness-driven gravity calibration: when we've been confidently still for a bit,
    // re-level roll/pitch against gravity while preserving yaw. This works for any pose
    // and does not assume a particular axis alignment or starting orientation.
    if state.relevel_time_seconds >= RELEVEL_MIN_STILL_SECONDS && accel_norm.is_finite() && accel_norm >= 0.3 && gyro_lp_corrected_norm <= STILL_GYRO_LP_MAX_DPS_FOR_RELEVEL {
        let (roll_acc, pitch_acc) = tilt_from_accel(accel_lp);
        let (_, _, yaw_current) = quat_to_euler_deg(fused_quat);
        let target = euler_deg_to_quat(roll_acc, pitch_acc, yaw_current);

        let alpha = 1.0 - (-dt_seconds / RELEVEL_TAU_SECONDS).exp();
        fused_quat = quat_slerp(fused_quat, target, alpha);
        state.quaternion = fused_quat;
    }

    let is_moving_fast = moving_candidate;

    // Best-effort dead reckoning in world frame for short windows.
    //
    // For velocity, prioritize responsiveness over heavy body-frame smoothing to reduce lag.
    // Integrate from near-raw linear acceleration in world frame, then remove a learned world-frame
    // residual bias captured during stillness to compensate gravity/cross-axis leakage.
    let mut linear_world_mps2 = rotate_vec3(fused_quat, linear_accel_raw);
    linear_world_mps2 = [linear_world_mps2[0] * STANDARD_GRAVITY_MPS2, linear_world_mps2[1] * STANDARD_GRAVITY_MPS2, linear_world_mps2[2] * STANDARD_GRAVITY_MPS2];
    linear_world_mps2 = lowpass_vec3(state.linear_world_mps2_lp, linear_world_mps2, dt_seconds, DR_WORLD_ACCEL_LP_TAU_SECONDS);
    state.linear_world_mps2_lp = linear_world_mps2;
    linear_world_mps2 = vec3_clamp_norm(linear_world_mps2, settings.dr_max_accel_world_mps2);

    let bias_reference_still = confident_still && is_still;
    let recovery_bias_update = state.rotation_recovery_seconds > 0.0 && !is_moving_fast && gyro_stillness_norm <= DR_COAST_GYRO_MAX_DPS * 1.8 && accel_mag_error <= RELEVEL_ACCEL_MAG_ERROR_G;
    if bias_reference_still {
        state.world_accel_bias_mps2 = lowpass_vec3(state.world_accel_bias_mps2, linear_world_mps2, dt_seconds, DR_WORLD_ACCEL_BIAS_TAU_SECONDS);
    } else if recovery_bias_update {
        // During fast-rotation recovery, adapt the gravity-axis bias faster to cancel residual gravity leakage.
        state.world_accel_bias_mps2[2] = lowpass_scalar(state.world_accel_bias_mps2[2], linear_world_mps2[2], dt_seconds, DR_GRAVITY_AXIS_BIAS_RECOVERY_TAU_SECONDS);
    } else {
        // Lateral bias estimates can create one-direction sensitivity; decay them toward zero
        // while moving so dead-reckoning remains directionally symmetric.
        state.world_accel_bias_mps2[0] = lowpass_scalar(state.world_accel_bias_mps2[0], 0.0, dt_seconds, DR_WORLD_ACCEL_BIAS_XY_DECAY_TAU_SECONDS);
        state.world_accel_bias_mps2[1] = lowpass_scalar(state.world_accel_bias_mps2[1], 0.0, dt_seconds, DR_WORLD_ACCEL_BIAS_XY_DECAY_TAU_SECONDS);
    }
    state.world_accel_bias_mps2[0] = state.world_accel_bias_mps2[0].clamp(-DR_WORLD_ACCEL_BIAS_XY_MAX_MPS2, DR_WORLD_ACCEL_BIAS_XY_MAX_MPS2);
    state.world_accel_bias_mps2[1] = state.world_accel_bias_mps2[1].clamp(-DR_WORLD_ACCEL_BIAS_XY_MAX_MPS2, DR_WORLD_ACCEL_BIAS_XY_MAX_MPS2);
    state.world_accel_bias_mps2[2] = state.world_accel_bias_mps2[2].clamp(-DR_WORLD_ACCEL_BIAS_Z_MAX_MPS2, DR_WORLD_ACCEL_BIAS_Z_MAX_MPS2);
    let bias_apply_xy = if is_moving_fast { DR_WORLD_ACCEL_BIAS_XY_APPLY_WHILE_MOVING_SCALE } else { 1.0 };
    let corrected_world_unscaled = [
        linear_world_mps2[0] - state.world_accel_bias_mps2[0] * bias_apply_xy,
        linear_world_mps2[1] - state.world_accel_bias_mps2[1] * bias_apply_xy,
        linear_world_mps2[2] - state.world_accel_bias_mps2[2],
    ];
    let rotation_leak_scale = rotation_leak_scale_from_gyro(gyro_norm);
    let rotation_contamination_weight = rotation_contamination_weight_from_gyro(gyro_norm);
    let mut corrected_world_mps2 = vec3_scale(corrected_world_unscaled, rotation_leak_scale);
    let rotation_accel_scale = lerp(DR_ROTATION_ACCEL_MIN_SCALE, 1.0, rotation_contamination_weight);
    corrected_world_mps2 = vec3_scale(corrected_world_mps2, rotation_accel_scale);
    let rotation_contaminated = rotation_contamination_weight < 0.55;
    let velocity_before = state.velocity_world_mps;
    let speed_before = vec3_norm(velocity_before);
    let velocity_before_unit = if speed_before > 1e-4 { Some(vec3_scale(velocity_before, 1.0 / speed_before)) } else { None };
    let horizontal_norm = (corrected_world_mps2[0] * corrected_world_mps2[0] + corrected_world_mps2[1] * corrected_world_mps2[1]).sqrt();
    let vertical_abs = corrected_world_mps2[2].abs();
    let gravity_axis_leak_suspected = accel_mag_error <= RELEVEL_ACCEL_MAG_ERROR_G
        && motion_fast_g <= DR_GRAVITY_AXIS_LEAK_MOTION_FAST_MAX_G
        && vertical_abs <= DR_GRAVITY_AXIS_LEAK_ACCEL_MAX_MPS2
        && vertical_abs >= horizontal_norm * DR_GRAVITY_AXIS_LEAK_DOMINANCE_RATIO
        && (state.rotation_recovery_seconds > 0.0 || gyro_stillness_norm <= DR_COAST_GYRO_MAX_DPS * 1.5 || rotation_contaminated);
    if gravity_axis_leak_suspected {
        corrected_world_mps2[2] *= DR_GRAVITY_AXIS_LEAK_SUPPRESS_SCALE;
    }
    let opposing_accel_mps2 = velocity_before_unit.map(|unit| -vec3_dot(corrected_world_mps2, unit)).unwrap_or(0.0);
    let direction_change_detected = speed_before >= DR_DIRECTION_CHANGE_MIN_SPEED_MPS && opposing_accel_mps2 >= DR_DIRECTION_CHANGE_OPPOSING_ACCEL_MPS2;
    if direction_change_detected {
        state.direction_change_hold_seconds = DR_DIRECTION_CHANGE_HOLD_SECONDS;
    } else {
        state.direction_change_hold_seconds = (state.direction_change_hold_seconds - dt_seconds).max(0.0);
    }
    let direction_change_active = direction_change_detected || state.direction_change_hold_seconds > 0.0;
    let deadband_mps2 = if rotation_contaminated { DR_ROTATION_TRANSLATION_DEADBAND_MPS2 } else { DR_WORLD_ACCEL_DEADBAND_MPS2 };
    let deadband_mps2 = if direction_change_active { deadband_mps2 * DR_DIRECTION_CHANGE_DEADBAND_SCALE } else { deadband_mps2 };
    corrected_world_mps2 = vec3_soft_deadband_norm(corrected_world_mps2, deadband_mps2);
    let corrected_world_norm = vec3_norm(corrected_world_mps2);
    let mut dr_confidence = dr_confidence_from_metrics(state.stillness_confidence_lp, corrected_world_norm, gyro_stillness_norm);
    if is_moving_fast {
        dr_confidence *= 0.90;
    }
    if rotation_contaminated {
        dr_confidence *= 0.72;
    }
    if direction_change_active {
        dr_confidence = dr_confidence.max(0.30);
    }
    let still_for_zupt = confident_still && is_still && corrected_world_norm <= DR_ZUPT_RELEASE_ACCEL_MPS2 && !direction_change_active;
    let moving_evidence = is_moving_fast || corrected_world_norm >= DR_MOVING_EVIDENCE_ACCEL_MPS2 || speed_before >= DR_DIRECTION_CHANGE_MIN_SPEED_MPS || direction_change_active;
    state.velocity_world_mps = vec3_add(state.velocity_world_mps, vec3_scale(corrected_world_mps2, dt_seconds));
    if direction_change_detected && let Some(unit) = velocity_before_unit {
        // On reversal, quickly bleed the stale component along the previous direction
        // so velocity flips with the physical motion instead of lagging by seconds.
        let stale_along_prev = vec3_dot(state.velocity_world_mps, unit);
        if stale_along_prev > 0.0 {
            state.velocity_world_mps = vec3_sub(state.velocity_world_mps, vec3_scale(unit, stale_along_prev * DR_DIRECTION_CHANGE_VELOCITY_BRAKE_SCALE));
        }
    }
    if still_for_zupt {
        let alpha_zero = 1.0 - (-dt_seconds / settings.dr_still_velocity_zero_tau_seconds.max(1e-3)).exp();
        state.velocity_world_mps = [lerp(state.velocity_world_mps[0], 0.0, alpha_zero), lerp(state.velocity_world_mps[1], 0.0, alpha_zero), lerp(state.velocity_world_mps[2], 0.0, alpha_zero)];
        if vec3_norm(state.velocity_world_mps) < 0.01 {
            state.velocity_world_mps = [0.0; 3];
        }
    } else {
        let vel_damp_tau_base = settings.dr_velocity_damp_tau_seconds.clamp(0.08, DR_VELOCITY_DAMP_TAU_MAX_EFFECTIVE);
        let settle_tau = (vel_damp_tau_base * 0.55).clamp(0.06, 0.60);
        let moving_tau = if rotation_contaminated { (vel_damp_tau_base * 1.10).clamp(0.12, 1.60) } else { (vel_damp_tau_base * 1.35).clamp(0.16, 1.80) };
        let direction_change_tau = (vel_damp_tau_base * 1.60).clamp(0.18, 2.40);
        let vel_damp_tau = if direction_change_active {
            direction_change_tau
        } else if moving_evidence {
            moving_tau
        } else {
            settle_tau
        };
        let vel_damp = (-dt_seconds / vel_damp_tau.max(1e-3)).exp();
        state.velocity_world_mps = vec3_scale(state.velocity_world_mps, vel_damp);
        if gravity_axis_leak_suspected && !direction_change_active {
            let z_alpha = 1.0 - (-dt_seconds / DR_GRAVITY_AXIS_Z_VELOCITY_DAMP_TAU_SECONDS.max(1e-3)).exp();
            state.velocity_world_mps[2] = lerp(state.velocity_world_mps[2], 0.0, z_alpha);
        }
        if !moving_evidence && state.post_motion_seconds >= DR_STOP_SNAP_HOLD_SECONDS && vec3_norm(state.velocity_world_mps) <= DR_STOP_SNAP_SPEED_MPS * 0.35 {
            state.velocity_world_mps = [0.0; 3];
        }
    }

    state.velocity_world_mps = vec3_clamp_norm(state.velocity_world_mps, settings.dr_max_speed_mps);
    if rotation_contaminated {
        state.velocity_world_mps = vec3_clamp_norm(state.velocity_world_mps, DR_ROTATION_TRANSLATION_SPEED_CAP_MPS.min(settings.dr_max_speed_mps));
    }
    if dr_confidence <= 0.12 && !moving_evidence && !direction_change_active {
        state.velocity_world_mps = [0.0; 3];
    }
    let velocity_delta_world = vec3_sub(state.velocity_world_mps, velocity_before);
    let linear_speed_mps = vec3_norm(state.velocity_world_mps);
    let is_moving = !is_still && (is_moving_fast || linear_speed_mps >= 0.04 || direction_change_active);
    let linear_speed_normalized = if settings.dr_max_speed_mps.is_finite() && settings.dr_max_speed_mps > 0.0 { (linear_speed_mps / settings.dr_max_speed_mps).clamp(0.0, 1.0) } else { 0.0 };
    let angular_speed_dps = vec3_norm(gyro_corrected);
    let angular_speed_normalized = (angular_speed_dps / DR_ROTATION_CONTAMINATION_FULL_DPS.max(1e-3)).clamp(0.0, 1.0);

    if settings.dr_lock_position {
        state.position_world_m = [0.0; 3];
    } else {
        // Position integration follows velocity directly in unlocked mode.
        // Avoid extra confidence scaling to keep pose response aligned with velocity timing.
        let velocity_for_position = state.velocity_world_mps;
        if moving_evidence || vec3_norm(velocity_for_position) >= 0.008 {
            state.position_world_m = vec3_add(state.position_world_m, vec3_scale(velocity_for_position, dt_seconds));
        }
        state.position_world_m = vec3_clamp_norm(state.position_world_m, settings.dr_max_position_m);
    }

    let output_quat = if settings.yaw_offset_deg.is_finite() && settings.yaw_offset_deg.abs() > 1e-6 {
        let yaw_offset = euler_deg_to_quat(0.0, 0.0, settings.yaw_offset_deg);
        quat_normalize(yaw_offset * fused_quat).unwrap_or(fused_quat)
    } else {
        fused_quat
    };

    // Euler angles near ±90° pitch are numerically ill-conditioned (yaw/roll coupling).
    // Prefer roll/pitch derived from gravity when accel is trustworthy so the UI doesn't show
    // a 1s \"settling\" wobble after a yaw twist.
    let (_, _, yaw) = quat_to_euler_deg(output_quat);
    let (roll, pitch) = if accel_trust >= 0.85 && gyro_stillness_norm <= 8.0 && accel_mag_error <= 0.10 {
        tilt_from_accel(accel_lp)
    } else {
        let (r, p, _) = quat_to_euler_deg(output_quat);
        (r, p)
    };
    let orientation = [normalize_angle(roll, settings.range), normalize_angle(pitch, settings.range), normalize_angle(yaw, settings.range)];

    FusedStateOutput {
        quaternion: [output_quat.w, output_quat.x, output_quat.y, output_quat.z],
        orientation,
        linear_accel: state.linear_accel_lp,
        corrected_world_accel_mps2: corrected_world_mps2,
        is_moving,
        is_moving_fast,
        is_still,
        stillness_confidence: state.stillness_confidence_lp,
        rotation_contaminated,
        motion_g,
        motion_fast_g,
        motion_fast_threshold_g: motion_fast_threshold_effective_g,
        motion_noise_floor_g: state.motion_noise_floor_g,
        velocity_world: state.velocity_world_mps,
        velocity_delta_world,
        linear_speed_mps,
        linear_speed_normalized,
        angular_velocity_dps: gyro_corrected,
        angular_speed_dps,
        angular_speed_normalized,
        gyro_bias_dps: state.gyro_bias_deg_per_sec,
        position_world: state.position_world_m,
        dr_confidence,
    }
}

fn quat_dot(a: Quaternion, b: Quaternion) -> f32 {
    a.w * b.w + a.x * b.x + a.y * b.y + a.z * b.z
}

fn quat_slerp(from: Quaternion, to: Quaternion, t: f32) -> Quaternion {
    let t = t.clamp(0.0, 1.0);
    let from = quat_normalize(from).unwrap_or(from);
    let mut to = quat_normalize(to).unwrap_or(to);

    // Take the shortest arc.
    let mut dot = quat_dot(from, to);
    if dot < 0.0 {
        dot = -dot;
        to = Quaternion::new(-to.w, -to.x, -to.y, -to.z);
    }

    // If very close, lerp is fine and avoids division by zero.
    if dot > 0.9995 {
        return quat_normalize(Quaternion::new(lerp(from.w, to.w, t), lerp(from.x, to.x, t), lerp(from.y, to.y, t), lerp(from.z, to.z, t))).unwrap_or(from);
    }

    let theta0 = dot.acos();
    let theta = theta0 * t;
    let sin_theta0 = theta0.sin();
    if !sin_theta0.is_finite() || sin_theta0.abs() <= f32::EPSILON {
        return from;
    }
    let s0 = (theta0 - theta).sin() / sin_theta0;
    let s1 = theta.sin() / sin_theta0;
    quat_normalize(Quaternion::new(from.w * s0 + to.w * s1, from.x * s0 + to.x * s1, from.y * s0 + to.y * s1, from.z * s0 + to.z * s1)).unwrap_or(from)
}

fn vec3_normalize(v: [f32; 3]) -> Option<[f32; 3]> {
    let n = vec3_norm(v);
    if !n.is_finite() || n <= f32::EPSILON {
        return None;
    }
    Some([v[0] / n, v[1] / n, v[2] / n])
}

fn lowpass_scalar(previous: f32, current: f32, dt_seconds: f32, tau_seconds: f32) -> f32 {
    if !dt_seconds.is_finite() || dt_seconds <= 0.0 || !tau_seconds.is_finite() || tau_seconds <= 0.0 {
        return current;
    }
    let alpha = (-dt_seconds / tau_seconds).exp();
    previous * alpha + current * (1.0 - alpha)
}

fn stillness_confidence_from_metrics(gyro_dps: f32, accel_mag_error_g: f32, motion_g: f32) -> f32 {
    let gyro_score = inverse_ramp_score(gyro_dps, 0.9, 5.0);
    let accel_mag_score = inverse_ramp_score(accel_mag_error_g, 0.02, 0.12);
    let motion_score = inverse_ramp_score(motion_g, 0.02, 0.13);
    (gyro_score * accel_mag_score * motion_score).clamp(0.0, 1.0)
}

fn dr_confidence_from_metrics(stillness_confidence: f32, corrected_accel_world_mps2: f32, gyro_dps: f32) -> f32 {
    let accel_score = inverse_ramp_score(corrected_accel_world_mps2, DR_CONFIDENCE_ACCEL_GOOD_MPS2, DR_CONFIDENCE_ACCEL_BAD_MPS2);
    let gyro_score = inverse_ramp_score(gyro_dps, DR_CONFIDENCE_GYRO_GOOD_DPS, DR_CONFIDENCE_GYRO_BAD_DPS);
    (stillness_confidence.clamp(0.0, 1.0) * 0.55 + accel_score * 0.25 + gyro_score * 0.20).clamp(0.0, 1.0)
}

fn rotation_leak_scale_from_gyro(gyro_dps: f32) -> f32 {
    inverse_ramp_score(gyro_dps, DR_ROTATION_LEAK_START_DPS, DR_ROTATION_LEAK_FULL_DPS).clamp(DR_ROTATION_LEAK_MIN_SCALE, 1.0)
}

fn rotation_contamination_weight_from_gyro(gyro_dps: f32) -> f32 {
    inverse_ramp_score(gyro_dps, DR_ROTATION_CONTAMINATION_START_DPS, DR_ROTATION_CONTAMINATION_FULL_DPS).clamp(0.0, 1.0)
}

fn inverse_ramp_score(value: f32, good_max: f32, bad_min: f32) -> f32 {
    if !value.is_finite() {
        return 0.0;
    }
    if value <= good_max {
        return 1.0;
    }
    if value >= bad_min || bad_min <= good_max {
        return 0.0;
    }
    1.0 - ((value - good_max) / (bad_min - good_max))
}

fn mag_norm_rel_tol() -> f32 {
    static CACHED: OnceLock<f32> = OnceLock::new();
    *CACHED.get_or_init(|| std::env::var("HELIOS_IMU_MAG_NORM_REL_TOL").ok().and_then(|raw| raw.parse::<f32>().ok()).map(|v| v.clamp(0.0, 5.0)).unwrap_or(0.45))
}

fn mag_norm_lp_tau_seconds() -> f32 {
    static CACHED: OnceLock<f32> = OnceLock::new();
    *CACHED.get_or_init(|| std::env::var("HELIOS_IMU_MAG_NORM_LP_TAU_SECONDS").ok().and_then(|raw| raw.parse::<f32>().ok()).map(|v| v.clamp(0.05, 60.0)).unwrap_or(6.0))
}

fn mag_min_horizontal_component() -> f32 {
    static CACHED: OnceLock<f32> = OnceLock::new();
    *CACHED.get_or_init(|| std::env::var("HELIOS_IMU_MAG_MIN_HORIZONTAL").ok().and_then(|raw| raw.parse::<f32>().ok()).map(|v| v.clamp(0.0, 1.0)).unwrap_or(0.15))
}

fn select_mag_for_fusion(
    accel_lp: [f32; 3],
    mag_lp: Option<[f32; 3]>,
    dt_seconds: f32,
    state: &mut ImuFusionState,
    gyro_stillness_norm: f32,
    accel_mag_error: f32,
    accel_trust: f32,
) -> Option<[f32; 3]> {
    let mag = mag_lp?;
    let mag_norm = vec3_norm(mag);
    if !mag_norm.is_finite() || mag_norm <= f32::EPSILON {
        return None;
    }

    // Reject magnetometer readings that cannot provide a stable yaw (field nearly vertical).
    let gravity = vec3_normalize(accel_lp).unwrap_or([0.0, 0.0, 1.0]);
    let mag_unit = [mag[0] / mag_norm, mag[1] / mag_norm, mag[2] / mag_norm];
    let dot = mag_unit[0] * gravity[0] + mag_unit[1] * gravity[1] + mag_unit[2] * gravity[2];
    let horiz = [mag_unit[0] - gravity[0] * dot, mag_unit[1] - gravity[1] * dot, mag_unit[2] - gravity[2] * dot];
    if vec3_norm(horiz) < mag_min_horizontal_component() {
        return None;
    }

    // Learn a baseline field magnitude while confidently still, then reject large excursions
    // (common for hard/soft-iron interference, nearby motors, or moving past metal).
    let update_baseline = accel_trust >= 0.7 && gyro_stillness_norm <= STILL_GYRO_LP_MAX_DPS_FOR_BIAS && accel_mag_error <= STILL_ACCEL_MAG_ERROR_G;
    if update_baseline {
        let tau = mag_norm_lp_tau_seconds();
        state.mag_norm_lp = Some(match state.mag_norm_lp {
            Some(prev) => lowpass_scalar(prev, mag_norm, dt_seconds, tau),
            None => mag_norm,
        });
    }

    if let Some(baseline) = state.mag_norm_lp {
        let denom = baseline.abs().max(1e-6);
        let rel = ((mag_norm - baseline).abs()) / denom;
        if rel > mag_norm_rel_tol() && accel_trust >= 0.3 {
            return None;
        }
    }

    Some(mag)
}

fn madgwick_update(previous: Quaternion, gyro_deg_per_sec: [f32; 3], accel_g: [f32; 3], mag: Option<[f32; 3]>, dt_seconds: f32, beta: f32) -> Quaternion {
    let (gx, gy, gz) = (gyro_deg_per_sec[0].to_radians(), gyro_deg_per_sec[1].to_radians(), gyro_deg_per_sec[2].to_radians());
    let (mut q1, mut q2, mut q3, mut q4) = (previous.w, previous.x, previous.y, previous.z);

    let accel_norm = vec3_norm(accel_g);
    if !accel_norm.is_finite() || accel_norm <= f32::EPSILON {
        return integrate_gyro(previous, gyro_deg_per_sec, dt_seconds);
    }
    let (ax, ay, az) = (accel_g[0] / accel_norm, accel_g[1] / accel_norm, accel_g[2] / accel_norm);

    let (mut s1, mut s2, mut s3, mut s4) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);

    let mut used_mag = false;
    if let Some(mag) = mag {
        let mag_norm = vec3_norm(mag);
        if mag_norm.is_finite() && mag_norm > f32::EPSILON {
            used_mag = true;
            let (mx, my, mz) = (mag[0] / mag_norm, mag[1] / mag_norm, mag[2] / mag_norm);

            let _2q1 = 2.0 * q1;
            let _2q2 = 2.0 * q2;
            let _2q3 = 2.0 * q3;
            let _2q4 = 2.0 * q4;
            let _2q1q3 = 2.0 * q1 * q3;
            let _2q3q4 = 2.0 * q3 * q4;
            let q1q1 = q1 * q1;
            let q1q2 = q1 * q2;
            let q1q3 = q1 * q3;
            let q1q4 = q1 * q4;
            let q2q2 = q2 * q2;
            let q2q3 = q2 * q3;
            let q2q4 = q2 * q4;
            let q3q3 = q3 * q3;
            let q3q4 = q3 * q4;
            let q4q4 = q4 * q4;

            let _2q1mx = 2.0 * q1 * mx;
            let _2q1my = 2.0 * q1 * my;
            let _2q1mz = 2.0 * q1 * mz;
            let _2q2mx = 2.0 * q2 * mx;

            let hx = mx * q1q1 - _2q1my * q4 + _2q1mz * q3 + mx * q2q2 + _2q2 * my * q3 + _2q2 * mz * q4 - mx * q3q3 - mx * q4q4;
            let hy = _2q1mx * q4 + my * q1q1 - _2q1mz * q2 + _2q2mx * q3 - my * q2q2 + my * q3q3 + _2q3 * mz * q4 - my * q4q4;
            let _2bx = (hx * hx + hy * hy).sqrt();
            let _2bz = -_2q1mx * q3 + _2q1my * q2 + mz * q1q1 + _2q2mx * q4 - mz * q2q2 + _2q3 * my * q4 - mz * q3q3 + mz * q4q4;
            let _4bx = 2.0 * _2bx;
            let _4bz = 2.0 * _2bz;

            s1 = -_2q3 * (2.0 * q2q4 - _2q1q3 - ax) + _2q2 * (2.0 * q1q2 + _2q3q4 - ay) - _2bz * q3 * (_2bx * (0.5 - q3q3 - q4q4) + _2bz * (q2q4 - q1q3) - mx)
                + (-_2bx * q4 + _2bz * q2) * (_2bx * (q2q3 - q1q4) + _2bz * (q1q2 + q3q4) - my)
                + _2bx * q3 * (_2bx * (q1q3 + q2q4) + _2bz * (0.5 - q2q2 - q3q3) - mz);
            s2 = _2q4 * (2.0 * q2q4 - _2q1q3 - ax) + _2q1 * (2.0 * q1q2 + _2q3q4 - ay) - 4.0 * q2 * (1.0 - 2.0 * q2q2 - 2.0 * q3q3 - az)
                + _2bz * q4 * (_2bx * (0.5 - q3q3 - q4q4) + _2bz * (q2q4 - q1q3) - mx)
                + (_2bx * q3 + _2bz * q1) * (_2bx * (q2q3 - q1q4) + _2bz * (q1q2 + q3q4) - my)
                + (_2bx * q4 - _4bz * q2) * (_2bx * (q1q3 + q2q4) + _2bz * (0.5 - q2q2 - q3q3) - mz);
            s3 = -_2q1 * (2.0 * q2q4 - _2q1q3 - ax) + _2q4 * (2.0 * q1q2 + _2q3q4 - ay) - 4.0 * q3 * (1.0 - 2.0 * q2q2 - 2.0 * q3q3 - az)
                + (-_4bx * q3 - _2bz * q1) * (_2bx * (0.5 - q3q3 - q4q4) + _2bz * (q2q4 - q1q3) - mx)
                + (_2bx * q2 + _2bz * q4) * (_2bx * (q2q3 - q1q4) + _2bz * (q1q2 + q3q4) - my)
                + (_2bx * q1 - _4bz * q3) * (_2bx * (q1q3 + q2q4) + _2bz * (0.5 - q2q2 - q3q3) - mz);
            s4 = _2q2 * (2.0 * q2q4 - _2q1q3 - ax)
                + _2q3 * (2.0 * q1q2 + _2q3q4 - ay)
                + (-_4bx * q4 + _2bz * q2) * (_2bx * (0.5 - q3q3 - q4q4) + _2bz * (q2q4 - q1q3) - mx)
                + (-_2bx * q1 + _2bz * q3) * (_2bx * (q2q3 - q1q4) + _2bz * (q1q2 + q3q4) - my)
                + _2bx * q2 * (_2bx * (q1q3 + q2q4) + _2bz * (0.5 - q2q2 - q3q3) - mz);
        }
    }

    if !used_mag {
        let _2q1 = 2.0 * q1;
        let _2q2 = 2.0 * q2;
        let _2q3 = 2.0 * q3;
        let _2q4 = 2.0 * q4;
        let _4q1 = 4.0 * q1;
        let _4q2 = 4.0 * q2;
        let _4q3 = 4.0 * q3;
        let _8q2 = 8.0 * q2;
        let _8q3 = 8.0 * q3;
        let q1q1 = q1 * q1;
        let q2q2 = q2 * q2;
        let q3q3 = q3 * q3;
        let q4q4 = q4 * q4;

        s1 = _4q1 * q3q3 + _2q3 * ax + _4q1 * q2q2 - _2q2 * ay;
        s2 = _4q2 * q4q4 - _2q4 * ax + 4.0 * q1q1 * q2 - _2q1 * ay - _4q2 + _8q2 * q2q2 + _8q2 * q3q3 + _4q2 * az;
        s3 = 4.0 * q1q1 * q3 + _2q1 * ax + _4q3 * q4q4 - _2q4 * ay - _4q3 + _8q3 * q2q2 + _8q3 * q3q3 + _4q3 * az;
        s4 = 4.0 * q2q2 * q4 - _2q2 * ax + 4.0 * q3q3 * q4 - _2q3 * ay;
    }

    let s_norm = (s1 * s1 + s2 * s2 + s3 * s3 + s4 * s4).sqrt();
    if s_norm.is_finite() && s_norm > f32::EPSILON {
        s1 /= s_norm;
        s2 /= s_norm;
        s3 /= s_norm;
        s4 /= s_norm;
    }

    let q_dot1 = 0.5 * (-q2 * gx - q3 * gy - q4 * gz) - beta * s1;
    let q_dot2 = 0.5 * (q1 * gx + q3 * gz - q4 * gy) - beta * s2;
    let q_dot3 = 0.5 * (q1 * gy - q2 * gz + q4 * gx) - beta * s3;
    let q_dot4 = 0.5 * (q1 * gz + q2 * gy - q3 * gx) - beta * s4;

    q1 += q_dot1 * dt_seconds;
    q2 += q_dot2 * dt_seconds;
    q3 += q_dot3 * dt_seconds;
    q4 += q_dot4 * dt_seconds;

    quat_normalize(Quaternion::new(q1, q2, q3, q4)).unwrap_or(previous)
}

fn vec3_norm(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

fn vec3_add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn vec3_sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn vec3_scale(v: [f32; 3], scalar: f32) -> [f32; 3] {
    [v[0] * scalar, v[1] * scalar, v[2] * scalar]
}

fn vec3_dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn vec3_clamp_norm(v: [f32; 3], max_norm: f32) -> [f32; 3] {
    let n = vec3_norm(v);
    if !n.is_finite() || n <= f32::EPSILON || !max_norm.is_finite() || max_norm <= 0.0 || n <= max_norm {
        return v;
    }
    vec3_scale(v, max_norm / n)
}

fn vec3_soft_deadband_norm(v: [f32; 3], deadband: f32) -> [f32; 3] {
    if !deadband.is_finite() || deadband <= 0.0 {
        return v;
    }
    let n = vec3_norm(v);
    if !n.is_finite() || n <= deadband {
        return [0.0; 3];
    }
    vec3_scale(v, (n - deadband) / n)
}

fn lowpass_vec3(previous: [f32; 3], current: [f32; 3], dt_seconds: f32, tau_seconds: f32) -> [f32; 3] {
    if !dt_seconds.is_finite() || dt_seconds <= 0.0 || !tau_seconds.is_finite() || tau_seconds <= 0.0 {
        return current;
    }
    let alpha = (-dt_seconds / tau_seconds).exp();
    [previous[0] * alpha + current[0] * (1.0 - alpha), previous[1] * alpha + current[1] * (1.0 - alpha), previous[2] * alpha + current[2] * (1.0 - alpha)]
}

fn rotate_world_to_body(q: Quaternion, v_world: [f32; 3]) -> [f32; 3] {
    let q = quat_normalize(q).unwrap_or(q);
    let q_conj = Quaternion::new(q.w, -q.x, -q.y, -q.z);
    let vq = Quaternion::new(0.0, v_world[0], v_world[1], v_world[2]);
    let rotated = q_conj * vq * q;
    [rotated.x, rotated.y, rotated.z]
}

fn rotate_vec3(q: Quaternion, v: [f32; 3]) -> [f32; 3] {
    let q = quat_normalize(q).unwrap_or(q);
    let q_conj = Quaternion::new(q.w, -q.x, -q.y, -q.z);
    let vq = Quaternion::new(0.0, v[0], v[1], v[2]);
    let rotated = q * vq * q_conj;
    [rotated.x, rotated.y, rotated.z]
}

fn tilt_from_accel(accel: [f32; 3]) -> (f32, f32) {
    let ax = accel[0] as f64;
    let ay = accel[1] as f64;
    let az = accel[2] as f64;
    let roll = ay.atan2(az).to_degrees() as f32;
    let pitch = (-ax).atan2((ay * ay + az * az).sqrt()).to_degrees() as f32;
    (roll, pitch)
}

fn yaw_from_accel_mag(accel: [f32; 3], mag: [f32; 3]) -> Option<f32> {
    if !mag[0].is_finite() || !mag[1].is_finite() || !mag[2].is_finite() {
        return None;
    }

    let (roll_deg, pitch_deg) = tilt_from_accel(accel);
    let (roll, pitch) = (roll_deg.to_radians() as f64, pitch_deg.to_radians() as f64);
    let (sr, cr) = roll.sin_cos();
    let (sp, cp) = pitch.sin_cos();

    let mx = mag[0] as f64;
    let my = mag[1] as f64;
    let mz = mag[2] as f64;

    // Tilt-compensated heading (yaw about +Z) for +X forward / +Y right / +Z up.
    // For a level sensor (roll=pitch=0), this reduces to yaw = atan2(-my, mx).
    let mx2 = mx * cp + mz * sp;
    let my2 = mx * sr * sp + my * cr - mz * sr * cp;
    if !mx2.is_finite() || !my2.is_finite() {
        return None;
    }
    Some((-my2).atan2(mx2).to_degrees() as f32)
}

fn normalize_angle(mut angle: f32, range: ImuRange) -> f32 {
    match range {
        ImuRange::ZeroTo360 => {
            while angle < 0.0 {
                angle += 360.0;
            }
            while angle >= 360.0 {
                angle -= 360.0;
            }
        }
        ImuRange::Negative180To180 => {
            while angle <= -180.0 {
                angle += 360.0;
            }
            while angle > 180.0 {
                angle -= 360.0;
            }
        }
    }
    angle
}

fn integrate_gyro(previous: Quaternion, gyro_deg_per_sec: [f32; 3], dt_seconds: f32) -> Quaternion {
    let (wx, wy, wz) = (gyro_deg_per_sec[0].to_radians(), gyro_deg_per_sec[1].to_radians(), gyro_deg_per_sec[2].to_radians());
    let omega = Quaternion::new(0.0, wx, wy, wz);
    let q_dot = quat_scale(previous * omega, 0.5);
    quat_normalize(previous + quat_scale(q_dot, dt_seconds)).unwrap_or(previous)
}

fn euler_deg_to_quat(roll_deg: f32, pitch_deg: f32, yaw_deg: f32) -> Quaternion {
    let (roll, pitch, yaw) = (roll_deg.to_radians(), pitch_deg.to_radians(), yaw_deg.to_radians());
    let (sr, cr) = (roll * 0.5).sin_cos();
    let (sp, cp) = (pitch * 0.5).sin_cos();
    let (sy, cy) = (yaw * 0.5).sin_cos();

    // Z (yaw) * Y (pitch) * X (roll)
    Quaternion::new(cr * cp * cy + sr * sp * sy, sr * cp * cy - cr * sp * sy, cr * sp * cy + sr * cp * sy, cr * cp * sy - sr * sp * cy)
}

fn quat_to_euler_deg(q: Quaternion) -> (f32, f32, f32) {
    let q = quat_normalize(q).unwrap_or(q);
    let (w, x, y, z) = (q.w as f64, q.x as f64, q.y as f64, q.z as f64);

    let sinr_cosp = 2.0 * (w * x + y * z);
    let cosr_cosp = 1.0 - 2.0 * (x * x + y * y);
    let roll = sinr_cosp.atan2(cosr_cosp);

    let sinp = 2.0 * (w * y - z * x);
    let pitch = if sinp.abs() >= 1.0 { sinp.signum() * (std::f64::consts::FRAC_PI_2) } else { sinp.asin() };

    let siny_cosp = 2.0 * (w * z + x * y);
    let cosy_cosp = 1.0 - 2.0 * (y * y + z * z);
    let yaw = siny_cosp.atan2(cosy_cosp);

    (roll.to_degrees() as f32, pitch.to_degrees() as f32, yaw.to_degrees() as f32)
}

fn quat_scale(q: Quaternion, s: f32) -> Quaternion {
    Quaternion::new(q.w * s, q.x * s, q.y * s, q.z * s)
}

fn quat_normalize(q: Quaternion) -> Option<Quaternion> {
    let norm_sq = q.w * q.w + q.x * q.x + q.y * q.y + q.z * q.z;
    if !norm_sq.is_finite() || norm_sq <= f32::EPSILON {
        return None;
    }
    let inv = 1.0 / norm_sq.sqrt();
    Some(Quaternion::new(q.w * inv, q.x * inv, q.y * inv, q.z * inv))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    fn angle_delta_deg(a: f32, b: f32) -> f32 {
        let mut delta = a - b;
        while delta > 180.0 {
            delta -= 360.0;
        }
        while delta < -180.0 {
            delta += 360.0;
        }
        delta.abs()
    }

    #[test]
    fn initializes_from_gravity_when_z_negative() {
        let mut state = ImuFusionState::default();
        let settings = ImuSettings {
            range: ImuRange::Negative180To180,
            update_interval: Duration::from_millis(20),
            fusion: ImuFusionMethod::Madgwick,
            yaw_offset_deg: 0.0,
            mount_correction: Quaternion::IDENTITY,
            dr_velocity_damp_tau_seconds: ImuSettings::default_dr_velocity_damp_tau_seconds(),
            dr_still_velocity_zero_tau_seconds: ImuSettings::default_dr_still_velocity_zero_tau_seconds(),
            dr_max_accel_world_mps2: ImuSettings::default_dr_max_accel_world_mps2(),
            dr_max_speed_mps: ImuSettings::default_dr_max_speed_mps(),
            dr_max_position_m: ImuSettings::default_dr_max_position_m(),
            dr_lock_position: ImuSettings::default_dr_lock_position(),
        };
        let out = fuse_orientation_stateful([0.0, 0.0, -1.0], [0.0, 0.0, 0.0], None, 0.02, &settings, &mut state);

        assert!(approx(out.linear_accel[0], 0.0, 1e-3));
        assert!(approx(out.linear_accel[1], 0.0, 1e-3));
        assert!(approx(out.linear_accel[2], 0.0, 1e-3));

        let roll = out.orientation[0];
        assert!(approx(roll.abs(), 180.0, 2.0));
        assert!(approx(out.orientation[1], 0.0, 2.0));
    }

    #[test]
    fn still_gravity_subtraction_prefers_filtered_accel_over_bad_quat() {
        let mut state = ImuFusionState {
            initialized: true,
            still_time_seconds: 0.6,
            quaternion: euler_deg_to_quat(90.0, 0.0, 0.0),
            accel_lp: Some([0.0, 0.0, 1.0]),
            gyro_lp: Some([0.0, 0.0, 0.0]),
            ..Default::default()
        };

        let settings = ImuSettings {
            range: ImuRange::Negative180To180,
            update_interval: Duration::from_millis(20),
            fusion: ImuFusionMethod::Madgwick,
            yaw_offset_deg: 0.0,
            mount_correction: Quaternion::IDENTITY,
            dr_velocity_damp_tau_seconds: ImuSettings::default_dr_velocity_damp_tau_seconds(),
            dr_still_velocity_zero_tau_seconds: ImuSettings::default_dr_still_velocity_zero_tau_seconds(),
            dr_max_accel_world_mps2: ImuSettings::default_dr_max_accel_world_mps2(),
            dr_max_speed_mps: ImuSettings::default_dr_max_speed_mps(),
            dr_max_position_m: ImuSettings::default_dr_max_position_m(),
            dr_lock_position: ImuSettings::default_dr_lock_position(),
        };
        let out = fuse_orientation_stateful([0.0, 0.0, 1.0], [0.0, 0.0, 0.0], None, 0.02, &settings, &mut state);

        assert!(out.motion_g < 0.02);
        assert!(!out.is_moving);
    }

    #[test]
    fn fast_motion_does_not_cause_long_post_stop_yaw_drift() {
        let mut state = ImuFusionState::default();
        let settings = ImuSettings {
            range: ImuRange::Negative180To180,
            update_interval: Duration::from_millis(20),
            fusion: ImuFusionMethod::Madgwick,
            yaw_offset_deg: 0.0,
            mount_correction: Quaternion::IDENTITY,
            dr_velocity_damp_tau_seconds: ImuSettings::default_dr_velocity_damp_tau_seconds(),
            dr_still_velocity_zero_tau_seconds: ImuSettings::default_dr_still_velocity_zero_tau_seconds(),
            dr_max_accel_world_mps2: ImuSettings::default_dr_max_accel_world_mps2(),
            dr_max_speed_mps: ImuSettings::default_dr_max_speed_mps(),
            dr_max_position_m: ImuSettings::default_dr_max_position_m(),
            dr_lock_position: ImuSettings::default_dr_lock_position(),
        };

        let dt = 0.02;
        for _ in 0..10 {
            fuse_orientation_stateful([0.0, 0.0, 1.0], [0.0, 0.0, 0.0], None, dt, &settings, &mut state);
        }

        let yaw_rate_dps = 180.0;
        let gyro_bias_dps = 5.0;
        for _ in 0..10 {
            fuse_orientation_stateful([0.0, 0.0, 1.0], [0.0, 0.0, yaw_rate_dps + gyro_bias_dps], None, dt, &settings, &mut state);
        }

        let out_at_stop = fuse_orientation_stateful([0.0, 0.0, 1.0], [0.0, 0.0, gyro_bias_dps], None, dt, &settings, &mut state);
        let yaw_at_stop = out_at_stop.orientation[2];

        let mut yaw_after = yaw_at_stop;
        for _ in 0..50 {
            let out = fuse_orientation_stateful([0.0, 0.0, 1.0], [0.0, 0.0, gyro_bias_dps], None, dt, &settings, &mut state);
            yaw_after = out.orientation[2];
        }

        assert!(angle_delta_deg(yaw_after, yaw_at_stop) < 4.0);
    }

    #[test]
    fn yaw_offset_is_applied_to_output() {
        let mut state = ImuFusionState::default();
        let settings = ImuSettings {
            range: ImuRange::Negative180To180,
            update_interval: Duration::from_millis(20),
            fusion: ImuFusionMethod::Madgwick,
            yaw_offset_deg: 30.0,
            mount_correction: Quaternion::IDENTITY,
            dr_velocity_damp_tau_seconds: ImuSettings::default_dr_velocity_damp_tau_seconds(),
            dr_still_velocity_zero_tau_seconds: ImuSettings::default_dr_still_velocity_zero_tau_seconds(),
            dr_max_accel_world_mps2: ImuSettings::default_dr_max_accel_world_mps2(),
            dr_max_speed_mps: ImuSettings::default_dr_max_speed_mps(),
            dr_max_position_m: ImuSettings::default_dr_max_position_m(),
            dr_lock_position: ImuSettings::default_dr_lock_position(),
        };

        let dt = 0.02;
        for _ in 0..5 {
            fuse_orientation_stateful([0.0, 0.0, 1.0], [0.0, 0.0, 0.0], None, dt, &settings, &mut state);
        }

        let out = fuse_orientation_stateful([0.0, 0.0, 1.0], [0.0, 0.0, 0.0], None, dt, &settings, &mut state);
        assert!(angle_delta_deg(out.orientation[2], 30.0) < 1.0);

        let q = Quaternion::new(out.quaternion[0], out.quaternion[1], out.quaternion[2], out.quaternion[3]);
        let (_, _, yaw) = quat_to_euler_deg(q);
        assert!(angle_delta_deg(yaw, 30.0) < 1.0);
    }

    #[test]
    fn vector_soft_deadband_preserves_multi_axis_signal() {
        let v = [0.050, 0.050, 0.000];
        let out = vec3_soft_deadband_norm(v, 0.060);
        assert!(vec3_norm(out) > 0.0);

        let small = [0.020, 0.010, 0.000];
        let small_out = vec3_soft_deadband_norm(small, 0.060);
        assert!(approx(vec3_norm(small_out), 0.0, 1e-6));
    }
}
