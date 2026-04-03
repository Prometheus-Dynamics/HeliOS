use std::collections::BTreeMap;

use lib_ipc::types::Timestamp;
use lib_lighting::{LightingAnimation, LightingColor, LightingCommand, LightingRuntimeState};
use lib_sensors::fan_config::{FanConfig, FanCurvePoint, FanMode, FanStatus};
use lib_sensors::imu::{ImuFusionMethod, ImuRange};
use lib_sensors::model::{AxesReading, FanSnapshot, ImuReading, ImuSourceLabels, PowerSnapshot, PowerSourceReading, SensorReading};
use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};

use crate::dto::{JsonData, SensorKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
#[serde(transparent)]
pub struct TimestampMicros(pub i64);

impl From<Timestamp> for TimestampMicros {
    fn from(value: Timestamp) -> Self {
        Self(value.timestamp_micros())
    }
}

impl From<TimestampMicros> for Timestamp {
    fn from(value: TimestampMicros) -> Self {
        chrono::DateTime::<chrono::Utc>::from_timestamp_micros(value.0).unwrap_or_else(|| chrono::DateTime::<chrono::Utc>::from_timestamp(0, 0).expect("unix epoch"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct LightingColorWire {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub w: u8,
}

impl From<LightingColor> for LightingColorWire {
    fn from(value: LightingColor) -> Self {
        Self { r: value.r, g: value.g, b: value.b, w: value.w }
    }
}

impl From<LightingColorWire> for LightingColor {
    fn from(value: LightingColorWire) -> Self {
        Self { r: value.r, g: value.g, b: value.b, w: value.w }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LightingAnimationWire {
    Off,
    Chase { color: LightingColorWire, speed_hz: f32 },
    Pulse { color: LightingColorWire, low: u8, high: u8, period_ms: u32 },
    Rainbow { speed_hz: f32 },
    BreathingRainbow { speed_hz: f32, low: u8, high: u8, period_ms: u32 },
}

impl From<LightingAnimation> for LightingAnimationWire {
    fn from(value: LightingAnimation) -> Self {
        match value {
            LightingAnimation::Off => Self::Off,
            LightingAnimation::Chase { color, speed_hz } => Self::Chase { color: color.into(), speed_hz },
            LightingAnimation::Pulse { color, low, high, period_ms } => Self::Pulse { color: color.into(), low, high, period_ms },
            LightingAnimation::Rainbow { speed_hz } => Self::Rainbow { speed_hz },
            LightingAnimation::BreathingRainbow { speed_hz, low, high, period_ms } => Self::BreathingRainbow { speed_hz, low, high, period_ms },
        }
    }
}

impl From<LightingAnimationWire> for LightingAnimation {
    fn from(value: LightingAnimationWire) -> Self {
        match value {
            LightingAnimationWire::Off => Self::Off,
            LightingAnimationWire::Chase { color, speed_hz } => Self::Chase { color: color.into(), speed_hz },
            LightingAnimationWire::Pulse { color, low, high, period_ms } => Self::Pulse { color: color.into(), low, high, period_ms },
            LightingAnimationWire::Rainbow { speed_hz } => Self::Rainbow { speed_hz },
            LightingAnimationWire::BreathingRainbow { speed_hz, low, high, period_ms } => Self::BreathingRainbow { speed_hz, low, high, period_ms },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct LightingCommandWire {
    pub frame: Option<Vec<LightingColorWire>>,
    pub brightness: Option<u8>,
    pub animation: Option<LightingAnimationWire>,
}

impl From<LightingCommand> for LightingCommandWire {
    fn from(value: LightingCommand) -> Self {
        Self { frame: value.frame.map(|colors| colors.into_iter().map(Into::into).collect()), brightness: value.brightness, animation: value.animation.map(Into::into) }
    }
}

impl From<LightingCommandWire> for LightingCommand {
    fn from(value: LightingCommandWire) -> Self {
        Self { frame: value.frame.map(|colors| colors.into_iter().map(Into::into).collect()), brightness: value.brightness, animation: value.animation.map(Into::into) }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct LightingRuntimeStateWire {
    pub command: LightingCommandWire,
    pub updated_at_ms: u64,
}

impl From<LightingRuntimeState> for LightingRuntimeStateWire {
    fn from(value: LightingRuntimeState) -> Self {
        Self { command: value.command.into(), updated_at_ms: value.updated_at_ms }
    }
}

impl From<LightingRuntimeStateWire> for LightingRuntimeState {
    fn from(value: LightingRuntimeStateWire) -> Self {
        Self { command: value.command.into(), updated_at_ms: value.updated_at_ms }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct FanCurvePointWire {
    pub temp_c: f32,
    pub percent: u8,
}

impl From<FanCurvePoint> for FanCurvePointWire {
    fn from(value: FanCurvePoint) -> Self {
        Self { temp_c: value.temp_c, percent: value.percent }
    }
}

impl From<FanCurvePointWire> for FanCurvePoint {
    fn from(value: FanCurvePointWire) -> Self {
        Self { temp_c: value.temp_c, percent: value.percent }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
#[serde(rename_all = "snake_case")]
pub enum FanModeWire {
    Disabled,
    Manual,
    #[default]
    Curve,
}

impl From<FanMode> for FanModeWire {
    fn from(value: FanMode) -> Self {
        match value {
            FanMode::Disabled => Self::Disabled,
            FanMode::Manual => Self::Manual,
            FanMode::Curve => Self::Curve,
        }
    }
}

impl From<FanModeWire> for FanMode {
    fn from(value: FanModeWire) -> Self {
        match value {
            FanModeWire::Disabled => Self::Disabled,
            FanModeWire::Manual => Self::Manual,
            FanModeWire::Curve => Self::Curve,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct FanConfigWire {
    pub enabled: bool,
    pub pwm_path: String,
    pub tacho_path: Option<String>,
    pub min_percent: u8,
    pub max_percent: u8,
    pub manual_percent: Option<u8>,
    pub poll_interval_ms: u64,
    pub invert_pwm: bool,
    pub curve: Vec<FanCurvePointWire>,
}

impl From<FanConfig> for FanConfigWire {
    fn from(value: FanConfig) -> Self {
        Self {
            enabled: value.enabled,
            pwm_path: value.pwm_path,
            tacho_path: value.tacho_path,
            min_percent: value.min_percent,
            max_percent: value.max_percent,
            manual_percent: value.manual_percent,
            poll_interval_ms: value.poll_interval_ms,
            invert_pwm: value.invert_pwm,
            curve: value.curve.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<FanConfigWire> for FanConfig {
    fn from(value: FanConfigWire) -> Self {
        Self {
            enabled: value.enabled,
            pwm_path: value.pwm_path,
            tacho_path: value.tacho_path,
            min_percent: value.min_percent,
            max_percent: value.max_percent,
            manual_percent: value.manual_percent,
            poll_interval_ms: value.poll_interval_ms,
            invert_pwm: value.invert_pwm,
            curve: value.curve.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct FanStatusWire {
    pub mode: FanModeWire,
    pub target_percent: u8,
    pub temperature_c: Option<f32>,
    pub rpm: Option<u32>,
    pub path_in_use: Option<String>,
    pub last_error: Option<String>,
    pub updated_at_ms: Option<u64>,
}

impl From<FanStatus> for FanStatusWire {
    fn from(value: FanStatus) -> Self {
        Self {
            mode: value.mode.into(),
            target_percent: value.target_percent,
            temperature_c: value.temperature_c,
            rpm: value.rpm,
            path_in_use: value.path_in_use,
            last_error: value.last_error,
            updated_at_ms: value.updated_at_ms,
        }
    }
}

impl From<FanStatusWire> for FanStatus {
    fn from(value: FanStatusWire) -> Self {
        Self {
            mode: value.mode.into(),
            target_percent: value.target_percent,
            temperature_c: value.temperature_c,
            rpm: value.rpm,
            path_in_use: value.path_in_use,
            last_error: value.last_error,
            updated_at_ms: value.updated_at_ms,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImuFusionMethodWire {
    #[default]
    MadgwickNoMag,
    Madgwick,
}

impl From<ImuFusionMethod> for ImuFusionMethodWire {
    fn from(value: ImuFusionMethod) -> Self {
        match value {
            ImuFusionMethod::MadgwickNoMag => Self::MadgwickNoMag,
            ImuFusionMethod::Madgwick => Self::Madgwick,
        }
    }
}

impl From<ImuFusionMethodWire> for ImuFusionMethod {
    fn from(value: ImuFusionMethodWire) -> Self {
        match value {
            ImuFusionMethodWire::MadgwickNoMag => Self::MadgwickNoMag,
            ImuFusionMethodWire::Madgwick => Self::Madgwick,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImuRangeWire {
    ZeroTo360,
    #[default]
    Negative180To180,
}

impl From<ImuRange> for ImuRangeWire {
    fn from(value: ImuRange) -> Self {
        match value {
            ImuRange::ZeroTo360 => Self::ZeroTo360,
            ImuRange::Negative180To180 => Self::Negative180To180,
        }
    }
}

impl From<ImuRangeWire> for ImuRange {
    fn from(value: ImuRangeWire) -> Self {
        match value {
            ImuRangeWire::ZeroTo360 => Self::ZeroTo360,
            ImuRangeWire::Negative180To180 => Self::Negative180To180,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct AxesReadingWire {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl From<AxesReading> for AxesReadingWire {
    fn from(value: AxesReading) -> Self {
        Self { x: value.x, y: value.y, z: value.z }
    }
}

impl From<AxesReadingWire> for AxesReading {
    fn from(value: AxesReadingWire) -> Self {
        Self { x: value.x, y: value.y, z: value.z }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct ImuSourceLabelsWire {
    pub accel_gyro: Option<String>,
    pub magnetometer: Option<String>,
}

impl From<ImuSourceLabels> for ImuSourceLabelsWire {
    fn from(value: ImuSourceLabels) -> Self {
        Self { accel_gyro: value.accel_gyro, magnetometer: value.magnetometer }
    }
}

impl From<ImuSourceLabelsWire> for ImuSourceLabels {
    fn from(value: ImuSourceLabelsWire) -> Self {
        Self { accel_gyro: value.accel_gyro, magnetometer: value.magnetometer }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct ImuReadingWire {
    pub orientation: Option<[f32; 3]>,
    pub quaternion: Option<[f32; 4]>,
    pub linear_accel: Option<[f32; 3]>,
    pub corrected_world_accel_mps2: Option<[f32; 3]>,
    pub velocity_world: Option<[f32; 3]>,
    pub velocity_delta_world: Option<[f32; 3]>,
    pub linear_speed_mps: Option<f32>,
    pub linear_speed_normalized: Option<f32>,
    pub angular_velocity_dps: Option<[f32; 3]>,
    pub angular_speed_dps: Option<f32>,
    pub angular_speed_normalized: Option<f32>,
    pub gyro_bias_dps: Option<[f32; 3]>,
    pub position_world: Option<[f32; 3]>,
    pub is_moving: Option<bool>,
    pub is_moving_fast: Option<bool>,
    pub is_still: Option<bool>,
    pub stillness_confidence: Option<f32>,
    pub rotation_contaminated: Option<bool>,
    pub motion_g: Option<f32>,
    pub motion_fast_g: Option<f32>,
    pub motion_fast_threshold_g: Option<f32>,
    pub motion_noise_floor_g: Option<f32>,
    pub dr_confidence: Option<f32>,
    pub fusion: Option<ImuFusionMethodWire>,
    pub dt_seconds: Option<f32>,
    pub updated_at: Option<TimestampMicros>,
    pub update_interval_ms: Option<u64>,
    pub dr_velocity_damp_tau_seconds: Option<f32>,
    pub dr_still_velocity_zero_tau_seconds: Option<f32>,
    pub dr_max_accel_world_mps2: Option<f32>,
    pub dr_max_speed_mps: Option<f32>,
    pub dr_max_position_m: Option<f32>,
    pub dr_lock_position: Option<bool>,
    pub range: Option<ImuRangeWire>,
    pub sources: ImuSourceLabelsWire,
    pub last_error: Option<String>,
}

impl From<ImuReading> for ImuReadingWire {
    fn from(value: ImuReading) -> Self {
        Self {
            orientation: value.orientation,
            quaternion: value.quaternion,
            linear_accel: value.linear_accel,
            corrected_world_accel_mps2: value.corrected_world_accel_mps2,
            velocity_world: value.velocity_world,
            velocity_delta_world: value.velocity_delta_world,
            linear_speed_mps: value.linear_speed_mps,
            linear_speed_normalized: value.linear_speed_normalized,
            angular_velocity_dps: value.angular_velocity_dps,
            angular_speed_dps: value.angular_speed_dps,
            angular_speed_normalized: value.angular_speed_normalized,
            gyro_bias_dps: value.gyro_bias_dps,
            position_world: value.position_world,
            is_moving: value.is_moving,
            is_moving_fast: value.is_moving_fast,
            is_still: value.is_still,
            stillness_confidence: value.stillness_confidence,
            rotation_contaminated: value.rotation_contaminated,
            motion_g: value.motion_g,
            motion_fast_g: value.motion_fast_g,
            motion_fast_threshold_g: value.motion_fast_threshold_g,
            motion_noise_floor_g: value.motion_noise_floor_g,
            dr_confidence: value.dr_confidence,
            fusion: value.fusion.map(Into::into),
            dt_seconds: value.dt_seconds,
            updated_at: value.updated_at.map(Into::into),
            update_interval_ms: value.update_interval_ms,
            dr_velocity_damp_tau_seconds: value.dr_velocity_damp_tau_seconds,
            dr_still_velocity_zero_tau_seconds: value.dr_still_velocity_zero_tau_seconds,
            dr_max_accel_world_mps2: value.dr_max_accel_world_mps2,
            dr_max_speed_mps: value.dr_max_speed_mps,
            dr_max_position_m: value.dr_max_position_m,
            dr_lock_position: value.dr_lock_position,
            range: value.range.map(Into::into),
            sources: value.sources.into(),
            last_error: value.last_error,
        }
    }
}

impl From<ImuReadingWire> for ImuReading {
    fn from(value: ImuReadingWire) -> Self {
        Self {
            orientation: value.orientation,
            quaternion: value.quaternion,
            linear_accel: value.linear_accel,
            corrected_world_accel_mps2: value.corrected_world_accel_mps2,
            velocity_world: value.velocity_world,
            velocity_delta_world: value.velocity_delta_world,
            linear_speed_mps: value.linear_speed_mps,
            linear_speed_normalized: value.linear_speed_normalized,
            angular_velocity_dps: value.angular_velocity_dps,
            angular_speed_dps: value.angular_speed_dps,
            angular_speed_normalized: value.angular_speed_normalized,
            gyro_bias_dps: value.gyro_bias_dps,
            position_world: value.position_world,
            is_moving: value.is_moving,
            is_moving_fast: value.is_moving_fast,
            is_still: value.is_still,
            stillness_confidence: value.stillness_confidence,
            rotation_contaminated: value.rotation_contaminated,
            motion_g: value.motion_g,
            motion_fast_g: value.motion_fast_g,
            motion_fast_threshold_g: value.motion_fast_threshold_g,
            motion_noise_floor_g: value.motion_noise_floor_g,
            dr_confidence: value.dr_confidence,
            fusion: value.fusion.map(Into::into),
            dt_seconds: value.dt_seconds,
            updated_at: value.updated_at.map(Into::into),
            update_interval_ms: value.update_interval_ms,
            dr_velocity_damp_tau_seconds: value.dr_velocity_damp_tau_seconds,
            dr_still_velocity_zero_tau_seconds: value.dr_still_velocity_zero_tau_seconds,
            dr_max_accel_world_mps2: value.dr_max_accel_world_mps2,
            dr_max_speed_mps: value.dr_max_speed_mps,
            dr_max_position_m: value.dr_max_position_m,
            dr_lock_position: value.dr_lock_position,
            range: value.range.map(Into::into),
            sources: value.sources.into(),
            last_error: value.last_error,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct PowerSourceReadingWire {
    pub label: String,
    pub bus: u32,
    pub address: u8,
    pub watts: f32,
    pub volts: f32,
    pub amps: f32,
    pub shunt_volts: f32,
}

impl From<PowerSourceReading> for PowerSourceReadingWire {
    fn from(value: PowerSourceReading) -> Self {
        Self { label: value.label, bus: value.bus, address: value.address, watts: value.watts, volts: value.volts, amps: value.amps, shunt_volts: value.shunt_volts }
    }
}

impl From<PowerSourceReadingWire> for PowerSourceReading {
    fn from(value: PowerSourceReadingWire) -> Self {
        Self { label: value.label, bus: value.bus, address: value.address, watts: value.watts, volts: value.volts, amps: value.amps, shunt_volts: value.shunt_volts }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct PowerSnapshotWire {
    pub total_watts: f64,
    pub total_amps: f64,
    pub volts: Option<f64>,
    pub updated_at: Option<TimestampMicros>,
    pub sources: Vec<PowerSourceReadingWire>,
    pub errors: Vec<String>,
    pub error: Option<String>,
}

impl From<PowerSnapshot> for PowerSnapshotWire {
    fn from(value: PowerSnapshot) -> Self {
        Self {
            total_watts: value.total_watts,
            total_amps: value.total_amps,
            volts: value.volts,
            updated_at: value.updated_at.map(Into::into),
            sources: value.sources.into_iter().map(Into::into).collect(),
            errors: value.errors,
            error: value.error,
        }
    }
}

impl From<PowerSnapshotWire> for PowerSnapshot {
    fn from(value: PowerSnapshotWire) -> Self {
        Self {
            total_watts: value.total_watts,
            total_amps: value.total_amps,
            volts: value.volts,
            updated_at: value.updated_at.map(Into::into),
            sources: value.sources.into_iter().map(Into::into).collect(),
            errors: value.errors,
            error: value.error,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub struct FanSnapshotWire {
    pub status: FanStatusWire,
}

impl From<FanSnapshot> for FanSnapshotWire {
    fn from(value: FanSnapshot) -> Self {
        Self { status: value.status.into() }
    }
}

impl From<FanSnapshotWire> for FanSnapshot {
    fn from(value: FanSnapshotWire) -> Self {
        Self { status: value.status.into() }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
pub enum SensorReadingWire {
    Accelerometer(AxesReadingWire),
    Gyroscope(AxesReadingWire),
    Magnetometer(AxesReadingWire),
    Imu(Box<ImuReadingWire>),
    Power(PowerSnapshotWire),
    Fan(FanSnapshotWire),
    Raw(JsonData),
}

impl From<SensorReading> for SensorReadingWire {
    fn from(value: SensorReading) -> Self {
        match value {
            SensorReading::Accelerometer(axes) => Self::Accelerometer(axes.into()),
            SensorReading::Gyroscope(axes) => Self::Gyroscope(axes.into()),
            SensorReading::Magnetometer(axes) => Self::Magnetometer(axes.into()),
            SensorReading::Imu(reading) => Self::Imu(Box::new((*reading).into())),
            SensorReading::Power(snapshot) => Self::Power(snapshot.into()),
            SensorReading::Fan(snapshot) => Self::Fan(snapshot.into()),
            SensorReading::Raw(value) => Self::Raw(JsonData::from_value(&value)),
        }
    }
}

impl From<SensorReadingWire> for SensorReading {
    fn from(value: SensorReadingWire) -> Self {
        match value {
            SensorReadingWire::Accelerometer(axes) => Self::Accelerometer(axes.into()),
            SensorReadingWire::Gyroscope(axes) => Self::Gyroscope(axes.into()),
            SensorReadingWire::Magnetometer(axes) => Self::Magnetometer(axes.into()),
            SensorReadingWire::Imu(reading) => Self::Imu(Box::new((*reading).into())),
            SensorReadingWire::Power(snapshot) => Self::Power(snapshot.into()),
            SensorReadingWire::Fan(snapshot) => Self::Fan(snapshot.into()),
            SensorReadingWire::Raw(value) => Self::Raw(value.to_value()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, Archive, RkyvSerialize, RkyvDeserialize)]
#[serde(transparent)]
pub struct SensorSnapshotTypedWire(pub BTreeMap<SensorKind, SensorReadingWire>);

impl From<BTreeMap<SensorKind, SensorReading>> for SensorSnapshotTypedWire {
    fn from(value: BTreeMap<SensorKind, SensorReading>) -> Self {
        Self(value.into_iter().map(|(kind, reading)| (kind, reading.into())).collect())
    }
}

impl From<SensorSnapshotTypedWire> for BTreeMap<SensorKind, SensorReading> {
    fn from(value: SensorSnapshotTypedWire) -> Self {
        value.0.into_iter().map(|(kind, reading)| (kind, reading.into())).collect()
    }
}
