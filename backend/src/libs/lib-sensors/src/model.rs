use bincode::{Decode, Encode};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map as JsonMap, Value as JsonValue, json};
use uuid::Uuid;

use crate::imu::{ImuFusionMethod, ImuRange, ImuSample, ImuSettings, ImuSources};
use crate::{fan_config, led_config, sensor_config};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, Encode, Decode)]
pub struct AxesReading {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl From<[f32; 3]> for AxesReading {
    fn from(value: [f32; 3]) -> Self {
        Self { x: value[0], y: value[1], z: value[2] }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, Encode, Decode)]
pub struct ImuSourceLabels {
    pub accel_gyro: Option<String>,
    pub magnetometer: Option<String>,
}

impl ImuSourceLabels {
    pub fn is_empty(&self) -> bool {
        self.accel_gyro.is_none() && self.magnetometer.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, Encode, Decode)]
pub struct ImuReading {
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
    #[bincode(with_serde)]
    pub fusion: Option<ImuFusionMethod>,
    pub dt_seconds: Option<f32>,
    #[bincode(with_serde)]
    pub updated_at: Option<DateTime<Utc>>,
    pub update_interval_ms: Option<u64>,
    pub dr_velocity_damp_tau_seconds: Option<f32>,
    pub dr_still_velocity_zero_tau_seconds: Option<f32>,
    pub dr_max_accel_world_mps2: Option<f32>,
    pub dr_max_speed_mps: Option<f32>,
    pub dr_max_position_m: Option<f32>,
    pub dr_lock_position: Option<bool>,
    #[bincode(with_serde)]
    pub range: Option<ImuRange>,
    pub sources: ImuSourceLabels,
    pub last_error: Option<String>,
}

impl ImuReading {
    pub fn from_sample(sample: &ImuSample) -> Self {
        Self {
            orientation: Some(sample.orientation),
            quaternion: Some(sample.quaternion),
            linear_accel: Some(sample.linear_accel),
            corrected_world_accel_mps2: Some(sample.corrected_world_accel_mps2),
            velocity_world: Some(sample.velocity_world),
            velocity_delta_world: Some(sample.velocity_delta_world),
            linear_speed_mps: Some(sample.linear_speed_mps),
            linear_speed_normalized: Some(sample.linear_speed_normalized),
            angular_velocity_dps: Some(sample.angular_velocity_dps),
            angular_speed_dps: Some(sample.angular_speed_dps),
            angular_speed_normalized: Some(sample.angular_speed_normalized),
            gyro_bias_dps: Some(sample.gyro_bias_dps),
            position_world: Some(sample.position_world),
            is_moving: Some(sample.is_moving),
            is_moving_fast: Some(sample.is_moving_fast),
            is_still: Some(sample.is_still),
            stillness_confidence: Some(sample.stillness_confidence),
            rotation_contaminated: Some(sample.rotation_contaminated),
            motion_g: Some(sample.motion_g),
            motion_fast_g: Some(sample.motion_fast_g),
            motion_fast_threshold_g: Some(sample.motion_fast_threshold_g),
            motion_noise_floor_g: Some(sample.motion_noise_floor_g),
            dr_confidence: Some(sample.dr_confidence),
            fusion: Some(sample.fusion),
            dt_seconds: Some(sample.dt_seconds),
            updated_at: Some(sample.updated_at),
            update_interval_ms: Some(sample.update_interval.as_millis() as u64),
            dr_velocity_damp_tau_seconds: Some(sample.dr_velocity_damp_tau_seconds),
            dr_still_velocity_zero_tau_seconds: Some(sample.dr_still_velocity_zero_tau_seconds),
            dr_max_accel_world_mps2: Some(sample.dr_max_accel_world_mps2),
            dr_max_speed_mps: Some(sample.dr_max_speed_mps),
            dr_max_position_m: Some(sample.dr_max_position_m),
            dr_lock_position: Some(sample.dr_lock_position),
            range: Some(sample.range),
            sources: ImuSourceLabels { accel_gyro: sample.sources.accel_gyro.clone(), magnetometer: sample.sources.magnetometer.clone() },
            last_error: None,
        }
    }

    pub fn from_settings(settings: &ImuSettings) -> Self {
        Self {
            orientation: None,
            quaternion: None,
            linear_accel: None,
            corrected_world_accel_mps2: None,
            velocity_world: None,
            velocity_delta_world: None,
            linear_speed_mps: None,
            linear_speed_normalized: None,
            angular_velocity_dps: None,
            angular_speed_dps: None,
            angular_speed_normalized: None,
            gyro_bias_dps: None,
            position_world: None,
            is_moving: None,
            is_moving_fast: None,
            is_still: None,
            stillness_confidence: None,
            rotation_contaminated: None,
            motion_g: None,
            motion_fast_g: None,
            motion_fast_threshold_g: None,
            motion_noise_floor_g: None,
            dr_confidence: None,
            fusion: Some(settings.fusion),
            dt_seconds: None,
            updated_at: None,
            update_interval_ms: Some(settings.update_interval.as_millis() as u64),
            dr_velocity_damp_tau_seconds: Some(settings.dr_velocity_damp_tau_seconds),
            dr_still_velocity_zero_tau_seconds: Some(settings.dr_still_velocity_zero_tau_seconds),
            dr_max_accel_world_mps2: Some(settings.dr_max_accel_world_mps2),
            dr_max_speed_mps: Some(settings.dr_max_speed_mps),
            dr_max_position_m: Some(settings.dr_max_position_m),
            dr_lock_position: Some(settings.dr_lock_position),
            range: Some(settings.range),
            sources: ImuSourceLabels::default(),
            last_error: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self { last_error: Some(message.into()), updated_at: Some(Utc::now()), ..Self::default() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, Encode, Decode)]
pub struct PowerSourceReading {
    pub label: String,
    pub bus: u32,
    pub address: u8,
    pub watts: f32,
    pub volts: f32,
    pub amps: f32,
    pub shunt_volts: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, Encode, Decode)]
pub struct PowerSnapshot {
    pub total_watts: f64,
    pub total_amps: f64,
    pub volts: Option<f64>,
    #[bincode(with_serde)]
    pub updated_at: Option<DateTime<Utc>>,
    pub sources: Vec<PowerSourceReading>,
    pub errors: Vec<String>,
    pub error: Option<String>,
}

impl PowerSnapshot {
    pub fn from_sources(sources: Vec<PowerSourceReading>, errors: Vec<String>) -> Self {
        let total_watts: f64 = sources.iter().map(|entry| entry.watts as f64).sum();
        let total_amps: f64 = sources.iter().map(|entry| entry.amps as f64).sum();
        let volts = sources.first().map(|entry| entry.volts as f64);
        let updated_at = if sources.is_empty() && errors.is_empty() { None } else { Some(Utc::now()) };
        Self { total_watts, total_amps, volts, updated_at, sources, errors, error: None }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self { error: Some(message.into()), updated_at: Some(Utc::now()), ..Self::default() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, Encode, Decode)]
pub struct FanSnapshot {
    #[bincode(with_serde)]
    pub status: fan_config::FanStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Encode, Decode)]
pub enum SensorReading {
    Accelerometer(AxesReading),
    Gyroscope(AxesReading),
    Magnetometer(AxesReading),
    Imu(Box<ImuReading>),
    Power(PowerSnapshot),
    Fan(FanSnapshot),
    Raw(#[bincode(with_serde)] JsonValue),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default, Encode, Decode)]
pub struct SensorDescriptorModel {
    pub backend: String,
    pub identifier: String,
    pub present: bool,
    #[bincode(with_serde)]
    pub info: JsonMap<String, JsonValue>,
    #[bincode(with_serde)]
    pub metadata: JsonMap<String, JsonValue>,
    #[bincode(with_serde)]
    pub stream_id: Option<Uuid>,
    pub value: Option<SensorReading>,
}

pub fn json_from_axes(value: &AxesReading) -> JsonValue {
    json!([value.x, value.y, value.z])
}

pub fn json_from_imu(value: &ImuReading) -> JsonValue {
    let mut map = JsonMap::new();
    if let Some([roll, pitch, yaw]) = value.orientation {
        map.insert("roll".into(), json!(roll));
        map.insert("pitch".into(), json!(pitch));
        map.insert("yaw".into(), json!(yaw));
    }
    if let Some([w, x, y, z]) = value.quaternion {
        let mut quat = JsonMap::new();
        quat.insert("w".into(), json!(w));
        quat.insert("x".into(), json!(x));
        quat.insert("y".into(), json!(y));
        quat.insert("z".into(), json!(z));
        map.insert("quaternion".into(), JsonValue::Object(quat));
    }
    if let Some([x, y, z]) = value.linear_accel {
        let mut axes = JsonMap::new();
        axes.insert("x".into(), json!(x));
        axes.insert("y".into(), json!(y));
        axes.insert("z".into(), json!(z));
        map.insert("linear_accel".into(), JsonValue::Object(axes));
    }
    if let Some([x, y, z]) = value.corrected_world_accel_mps2 {
        let mut axes = JsonMap::new();
        axes.insert("x".into(), json!(x));
        axes.insert("y".into(), json!(y));
        axes.insert("z".into(), json!(z));
        map.insert("corrected_world_accel_mps2".into(), JsonValue::Object(axes));
    }
    if let Some([x, y, z]) = value.velocity_world {
        let mut axes = JsonMap::new();
        axes.insert("x".into(), json!(x));
        axes.insert("y".into(), json!(y));
        axes.insert("z".into(), json!(z));
        map.insert("velocity_world".into(), JsonValue::Object(axes));
    }
    if let Some([x, y, z]) = value.velocity_delta_world {
        let mut axes = JsonMap::new();
        axes.insert("x".into(), json!(x));
        axes.insert("y".into(), json!(y));
        axes.insert("z".into(), json!(z));
        map.insert("velocity_delta_world".into(), JsonValue::Object(axes));
    }
    if let Some(linear_speed_mps) = value.linear_speed_mps {
        map.insert("linear_speed_mps".into(), json!(linear_speed_mps));
    }
    if let Some(linear_speed_normalized) = value.linear_speed_normalized {
        map.insert("linear_speed_normalized".into(), json!(linear_speed_normalized));
    }
    if let Some([x, y, z]) = value.angular_velocity_dps {
        let mut axes = JsonMap::new();
        axes.insert("x".into(), json!(x));
        axes.insert("y".into(), json!(y));
        axes.insert("z".into(), json!(z));
        map.insert("angular_velocity_dps".into(), JsonValue::Object(axes));
    }
    if let Some(angular_speed_dps) = value.angular_speed_dps {
        map.insert("angular_speed_dps".into(), json!(angular_speed_dps));
    }
    if let Some(angular_speed_normalized) = value.angular_speed_normalized {
        map.insert("angular_speed_normalized".into(), json!(angular_speed_normalized));
    }
    if let Some([x, y, z]) = value.gyro_bias_dps {
        let mut axes = JsonMap::new();
        axes.insert("x".into(), json!(x));
        axes.insert("y".into(), json!(y));
        axes.insert("z".into(), json!(z));
        map.insert("gyro_bias_dps".into(), JsonValue::Object(axes));
    }
    if let Some([x, y, z]) = value.position_world {
        let mut axes = JsonMap::new();
        axes.insert("x".into(), json!(x));
        axes.insert("y".into(), json!(y));
        axes.insert("z".into(), json!(z));
        map.insert("position_world".into(), JsonValue::Object(axes));
    }
    if let Some(is_moving) = value.is_moving {
        map.insert("is_moving".into(), json!(is_moving));
    }
    if let Some(is_moving_fast) = value.is_moving_fast {
        map.insert("is_moving_fast".into(), json!(is_moving_fast));
    }
    if let Some(is_still) = value.is_still {
        map.insert("is_still".into(), json!(is_still));
    }
    if let Some(stillness_confidence) = value.stillness_confidence {
        map.insert("stillness_confidence".into(), json!(stillness_confidence));
    }
    if let Some(rotation_contaminated) = value.rotation_contaminated {
        map.insert("rotation_contaminated".into(), json!(rotation_contaminated));
    }
    if let Some(motion_g) = value.motion_g {
        map.insert("motion_g".into(), json!(motion_g));
    }
    if let Some(motion_fast_g) = value.motion_fast_g {
        map.insert("motion_fast_g".into(), json!(motion_fast_g));
    }
    if let Some(motion_fast_threshold_g) = value.motion_fast_threshold_g {
        map.insert("motion_fast_threshold_g".into(), json!(motion_fast_threshold_g));
    }
    if let Some(motion_noise_floor_g) = value.motion_noise_floor_g {
        map.insert("motion_noise_floor_g".into(), json!(motion_noise_floor_g));
    }
    if let Some(dr_confidence) = value.dr_confidence {
        map.insert("dr_confidence".into(), json!(dr_confidence));
    }
    if let Some(fusion) = value.fusion {
        map.insert("fusion".into(), json!(fusion.to_string()));
    }
    if let Some(dt) = value.dt_seconds {
        map.insert("dt_seconds".into(), json!(dt));
    }
    if let Some(updated_at) = value.updated_at {
        map.insert("updated_at".into(), json!(updated_at.to_rfc3339()));
    }
    if let Some(interval) = value.update_interval_ms {
        map.insert("update_interval_ms".into(), json!(interval));
    }
    if let Some(value) = value.dr_velocity_damp_tau_seconds {
        map.insert("dr_velocity_damp_tau_seconds".into(), json!(value));
    }
    if let Some(value) = value.dr_still_velocity_zero_tau_seconds {
        map.insert("dr_still_velocity_zero_tau_seconds".into(), json!(value));
    }
    if let Some(value) = value.dr_max_accel_world_mps2 {
        map.insert("dr_max_accel_world_mps2".into(), json!(value));
    }
    if let Some(value) = value.dr_max_speed_mps {
        map.insert("dr_max_speed_mps".into(), json!(value));
    }
    if let Some(value) = value.dr_max_position_m {
        map.insert("dr_max_position_m".into(), json!(value));
    }
    if let Some(value) = value.dr_lock_position {
        map.insert("dr_lock_position".into(), json!(value));
    }
    if let Some(range) = value.range {
        map.insert("range".into(), json!(range.to_string()));
    }
    if !value.sources.is_empty() {
        let mut sources = JsonMap::new();
        if let Some(accel) = value.sources.accel_gyro.clone() {
            sources.insert("accel_gyro".into(), json!(accel));
        }
        if let Some(mag) = value.sources.magnetometer.clone() {
            sources.insert("magnetometer".into(), json!(mag));
        }
        map.insert("sources".into(), JsonValue::Object(sources));
    }
    if let Some(error) = value.last_error.as_ref() {
        map.insert("error".into(), json!(error));
    }
    JsonValue::Object(map)
}

pub fn json_from_power_source(value: &PowerSourceReading) -> JsonValue {
    let mut map = JsonMap::new();
    map.insert("label".into(), json!(&value.label));
    map.insert("bus".into(), json!(value.bus));
    map.insert("address".into(), json!(format!("0x{:02X}", value.address)));
    map.insert("watts".into(), json!(value.watts));
    map.insert("volts".into(), json!(value.volts));
    map.insert("amps".into(), json!(value.amps));
    map.insert("shunt_volts".into(), json!(value.shunt_volts));
    JsonValue::Object(map)
}

pub fn json_from_power_snapshot(value: &PowerSnapshot) -> JsonValue {
    if let Some(error) = value.error.as_ref() {
        let mut root = JsonMap::new();
        root.insert("error".into(), json!(error));
        return JsonValue::Object(root);
    }
    let mut root = JsonMap::new();
    root.insert("watts".into(), json!(value.total_watts));
    root.insert("amps".into(), json!(value.total_amps));
    root.insert("volts".into(), json!(value.volts.unwrap_or(0.0)));
    if let Some(updated_at) = value.updated_at {
        root.insert("updated_at".into(), json!(updated_at.to_rfc3339()));
    }
    if !value.sources.is_empty() {
        let sources = value.sources.iter().map(json_from_power_source).collect();
        root.insert("sources".into(), JsonValue::Array(sources));
    }
    if !value.errors.is_empty() {
        let list = value.errors.iter().map(|msg| json!(msg)).collect();
        root.insert("errors".into(), JsonValue::Array(list));
    }
    JsonValue::Object(root)
}

pub fn json_from_fan_snapshot(value: &FanSnapshot) -> JsonValue {
    let status = &value.status;
    let mut info = JsonMap::new();
    info.insert("mode".into(), json!(format!("{:?}", status.mode).to_ascii_lowercase()));
    info.insert("target_percent".into(), json!(status.target_percent));
    if let Some(temp) = status.temperature_c {
        info.insert("temperature_c".into(), json!(temp));
    }
    if let Some(rpm) = status.rpm {
        info.insert("rpm".into(), json!(rpm));
    }
    if let Some(path) = status.path_in_use.as_ref() {
        info.insert("pwm_path".into(), json!(path));
    }
    if let Some(last_error) = status.last_error.as_ref() {
        info.insert("error".into(), json!(last_error));
    }
    if let Some(ts) = status.updated_at_ms {
        info.insert("updated_at_ms".into(), json!(ts));
    }
    JsonValue::Object(info)
}

pub fn json_from_sensor_reading(value: &SensorReading) -> JsonValue {
    match value {
        SensorReading::Accelerometer(axes) | SensorReading::Gyroscope(axes) | SensorReading::Magnetometer(axes) => json_from_axes(axes),
        SensorReading::Imu(reading) => json_from_imu(reading.as_ref()),
        SensorReading::Power(snapshot) => json_from_power_snapshot(snapshot),
        SensorReading::Fan(snapshot) => json_from_fan_snapshot(snapshot),
        SensorReading::Raw(raw) => raw.clone(),
    }
}

pub fn axes_from_sources(sources: &ImuSources) -> ImuSourceLabels {
    ImuSourceLabels { accel_gyro: sources.accel_gyro.clone(), magnetometer: sources.magnetometer.clone() }
}

pub fn axes_reading(sample_axes: [f32; 3]) -> AxesReading {
    AxesReading::from(sample_axes)
}

pub fn descriptor_from_device(device: &sensor_config::SensorDeviceCfg) -> SensorDescriptorModel {
    let mut info = JsonMap::new();
    info.insert("bus".to_string(), json!(device.bus));
    info.insert("address".to_string(), json!(device.address));

    if let Some(shunt) = device.shunt_resistance {
        info.insert("shunt_resistance".to_string(), json!(shunt));
    }
    if let Some(max_current) = device.max_current {
        info.insert("max_current".to_string(), json!(max_current));
    }
    if let Some(scale) = device.bus_scale {
        info.insert("bus_scale".to_string(), json!(scale));
    }
    if let Some(offset) = device.bus_offset {
        info.insert("bus_offset".to_string(), json!(offset));
    }

    let identifier = format!("i2c-{}-{:02x}", device.bus, device.address);
    let mut metadata = JsonMap::new();
    let device_type = classify_sensor_device(device);
    metadata.insert("product".into(), json!(&device.driver));
    metadata.insert("alias".into(), json!(&device.driver));
    metadata.insert("alias_identity".into(), json!(&device.driver));
    metadata.insert("type".into(), json!(device_type));
    metadata.insert("status".into(), json!("Configured"));
    metadata.insert("hardware_id".into(), json!(&identifier));

    SensorDescriptorModel { backend: device.driver.clone(), identifier, present: true, info, metadata, stream_id: None, value: None }
}

pub fn descriptor_from_fan_status(status: &fan_config::FanStatus) -> SensorDescriptorModel {
    let mut info = JsonMap::new();
    info.insert("mode".into(), json!(format!("{:?}", status.mode).to_ascii_lowercase()));
    info.insert("target_percent".into(), json!(status.target_percent));
    if let Some(temp) = status.temperature_c {
        info.insert("temperature_c".into(), json!(temp));
    }
    if let Some(rpm) = status.rpm {
        info.insert("rpm".into(), json!(rpm));
    }
    if let Some(path) = status.path_in_use.as_ref() {
        info.insert("pwm_path".into(), json!(path));
    }

    let mut metadata = JsonMap::new();
    metadata.insert("type".into(), json!("Fan"));
    metadata.insert("status".into(), json!("Online"));

    SensorDescriptorModel { backend: "pwm-fan".into(), identifier: "fan0".into(), present: true, info, metadata, stream_id: None, value: None }
}

pub fn descriptor_from_led_config(config: &led_config::LedConfig) -> SensorDescriptorModel {
    let mut info = JsonMap::new();
    info.insert("gpio".into(), json!(config.gpio));
    info.insert("count".into(), json!(config.count));
    info.insert("color_order".into(), json!(&config.color_order));
    info.insert("frequency_hz".into(), json!(config.frequency_hz));
    info.insert("use_pwm".into(), json!(config.use_pwm));
    if let Some(brightness) = config.brightness {
        info.insert("brightness".into(), json!(brightness));
    }

    let mut metadata = JsonMap::new();
    let alias = config.label.clone().unwrap_or_else(|| "Status LEDs".into());
    metadata.insert("alias".into(), json!(alias.clone()));
    metadata.insert("alias_identity".into(), json!(alias));
    metadata.insert("product".into(), json!(&config.protocol));
    metadata.insert("type".into(), json!("Lighting"));
    metadata.insert("status".into(), json!(if config.enabled { "Ready" } else { "Disabled" }));
    metadata.insert("hardware_id".into(), json!(format!("gpio{}-{}leds", config.gpio, config.count)));
    metadata.insert("mode".into(), json!(if config.use_pwm { "pwm" } else { "gpio" }));

    SensorDescriptorModel { backend: "led-strip".into(), identifier: format!("led-gpio{}", config.gpio), present: config.enabled, info, metadata, stream_id: None, value: None }
}

fn classify_sensor_device(device: &sensor_config::SensorDeviceCfg) -> &'static str {
    let normalized = device.driver.to_ascii_lowercase();
    if normalized.contains("bmi") {
        return match device.address {
            0x18 | 0x19 => "Accelerometer",
            0x68 | 0x69 => "Gyroscope",
            _ => "Accelerometer/Gyroscope",
        };
    }
    if normalized.contains("icm") {
        return "IMU";
    }
    if normalized.contains("bmm") {
        return "Magnetometer";
    }
    if normalized.contains("ina") {
        return "Power";
    }
    "Sensor"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imu_sample_to_json() {
        let sample = ImuSample {
            accel: [1.0, 2.0, 3.0],
            gyro: [4.0, 5.0, 6.0],
            mag: Some([7.0, 8.0, 9.0]),
            orientation: [0.1, 0.2, 0.3],
            quaternion: [1.0, 0.0, 0.0, 0.0],
            linear_accel: [0.01, -0.02, 0.03],
            corrected_world_accel_mps2: [0.2, -0.1, 0.05],
            is_moving: true,
            is_moving_fast: true,
            is_still: false,
            stillness_confidence: 0.18,
            rotation_contaminated: false,
            motion_g: 0.04,
            motion_fast_g: 0.05,
            motion_fast_threshold_g: 0.02,
            motion_noise_floor_g: 0.01,
            fusion: ImuFusionMethod::Madgwick,
            velocity_world: [0.1, -0.1, 0.0],
            velocity_delta_world: [0.02, -0.01, 0.0],
            linear_speed_mps: 0.1414,
            linear_speed_normalized: 0.03535,
            angular_velocity_dps: [3.0, -2.0, 1.0],
            angular_speed_dps: 3.7417,
            angular_speed_normalized: 0.0624,
            gyro_bias_dps: [0.2, 0.1, -0.05],
            position_world: [0.5, 0.1, -0.2],
            dr_confidence: 0.82,
            dr_velocity_damp_tau_seconds: ImuSettings::default_dr_velocity_damp_tau_seconds(),
            dr_still_velocity_zero_tau_seconds: ImuSettings::default_dr_still_velocity_zero_tau_seconds(),
            dr_max_accel_world_mps2: ImuSettings::default_dr_max_accel_world_mps2(),
            dr_max_speed_mps: ImuSettings::default_dr_max_speed_mps(),
            dr_max_position_m: ImuSettings::default_dr_max_position_m(),
            dr_lock_position: ImuSettings::default_dr_lock_position(),
            dt_seconds: 0.01,
            updated_at: Utc::now(),
            update_interval: std::time::Duration::from_millis(5),
            range: ImuRange::ZeroTo360,
            sources: ImuSources { accel_gyro: Some("icm".into()), magnetometer: Some("bmm".into()) },
        };
        let reading = ImuReading::from_sample(&sample);
        let value = json_from_imu(&reading);
        match value {
            JsonValue::Object(map) => {
                assert_eq!(map.get("fusion"), Some(&json!("madgwick")));
                assert_eq!(map.get("range"), Some(&json!("zero_to_360")));
                assert!(map.contains_key("sources"));
            }
            other => panic!("unexpected value: {other:?}"),
        }
    }

    #[test]
    fn power_snapshot_to_json() {
        let snapshot = PowerSnapshot::from_sources(vec![PowerSourceReading { label: "ina".into(), bus: 1, address: 0x40, watts: 2.5, volts: 12.0, amps: 0.2, shunt_volts: 0.1 }], vec![]);
        let value = json_from_power_snapshot(&snapshot);
        match value {
            JsonValue::Object(map) => {
                assert_eq!(map.get("watts"), Some(&json!(2.5)));
                let amps = map.get("amps").and_then(|v| v.as_f64()).expect("amps number");
                assert!((amps - 0.2).abs() < 1e-6);
                assert!(map.contains_key("sources"));
            }
            other => panic!("unexpected value: {other:?}"),
        }
    }
}
