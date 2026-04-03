use std::collections::BTreeMap;
use std::str::FromStr;
use std::time::Duration;

use crate::dto::{SensorData, SensorKind, SensorScope, SensorSnapshot};
use crate::error::{Error, Result};
use crate::imu::{ImuFusionMethod, ImuRange, ImuSample, ImuSettings, ImuState};
use crate::orchestrator::ImuSettingsUpdate;
use crate::power::PowerReading;
use lib_math::linalg::Quaternion;
use lib_sensors::model::{AxesReading, ImuReading, PowerSnapshot, PowerSourceReading, SensorReading};
use serde_json::Value as JsonValue;

use super::SensorsService;
use super::serialize_reading;

impl SensorsService {
    pub async fn apply_imu_sample(&self, sample: ImuSample) {
        let scope = SensorScope::Device;
        if !self.has_scope_subscribers(&scope).await {
            return;
        }
        {
            let mut state = self.state.write().await;
            let imu_state = ImuState { sample: Some(sample), last_error: None, sources: Default::default() };
            apply_imu_state_to_typed_readings(state.readings.entry(scope.clone()).or_default(), &imu_state);
            apply_imu_state_to_cached_snapshot(state.serialized_readings.entry(scope.clone()).or_default(), &imu_state);
        }
        self.publish_snapshot(None, &scope).await;
    }

    pub async fn apply_imu_error(&self, message: String) {
        let scope = SensorScope::Device;
        if !self.has_scope_subscribers(&scope).await {
            return;
        }
        {
            let mut state = self.state.write().await;
            let imu_state = ImuState { sample: None, last_error: Some(message), sources: Default::default() };
            apply_imu_state_to_typed_readings(state.readings.entry(scope.clone()).or_default(), &imu_state);
            apply_imu_state_to_cached_snapshot(state.serialized_readings.entry(scope.clone()).or_default(), &imu_state);
        }
        self.publish_snapshot(None, &scope).await;
    }

    pub async fn apply_power_samples(&self, samples: Vec<PowerReading>, errors: Vec<String>) {
        let scope = SensorScope::Device;
        self.ensure_scope_registered(&scope).await;
        let sources: Vec<PowerSourceReading> = samples
            .iter()
            .map(|sample| PowerSourceReading {
                label: sample.label.clone(),
                bus: sample.bus,
                address: sample.address,
                volts: sample.volts,
                amps: sample.amps,
                watts: sample.watts,
                shunt_volts: sample.shunt,
            })
            .collect();
        let value = SensorReading::Power(PowerSnapshot::from_sources(sources, errors));
        {
            let mut state = self.state.write().await;
            state.serialized_readings.entry(scope.clone()).or_default().insert(SensorKind::Power, serialize_reading(&value));
            state.readings.entry(scope.clone()).or_default().insert(SensorKind::Power, value);
        }
        self.publish_snapshot(None, &scope).await;
    }

    pub async fn apply_power_error(&self, message: String) {
        let scope = SensorScope::Device;
        self.ensure_scope_registered(&scope).await;
        {
            let mut state = self.state.write().await;
            let reading = SensorReading::Power(PowerSnapshot::error(message));
            state.serialized_readings.entry(scope.clone()).or_default().insert(SensorKind::Power, serialize_reading(&reading));
            state.readings.entry(scope.clone()).or_default().insert(SensorKind::Power, reading);
        }
        self.publish_snapshot(None, &scope).await;
    }

    pub async fn cached_snapshot(&self, scope: &SensorScope) -> Result<SensorSnapshot> {
        self.ensure_scope_registered(scope).await;
        let values = {
            let state = self.state.read().await;
            state.serialized_readings.get(scope).cloned().unwrap_or_default()
        };
        Ok(values)
    }

    pub async fn snapshot(&self, scope: &SensorScope) -> Result<SensorSnapshot> {
        let values = self.snapshot_typed(scope).await?;
        Ok(super::serialize_readings(&values))
    }

    pub async fn snapshot_typed(&self, scope: &SensorScope) -> Result<BTreeMap<SensorKind, SensorReading>> {
        self.ensure_scope_registered(scope).await;
        let mut values = {
            let state = self.state.read().await;
            state.readings.get(scope).cloned().unwrap_or_default()
        };
        if matches!(scope, SensorScope::Device)
            && let Some(imu_state) = self.imu_state().await
        {
            apply_imu_state_to_typed_readings(&mut values, &imu_state);
        }
        Ok(values)
    }

    pub async fn update_sensor(&self, scope: &SensorScope, sensor: SensorKind, payload: SensorData) -> Result<()> {
        self.ensure_scope_registered(scope).await;
        let payload_value = payload.to_value();
        let applied_payload = if sensor == SensorKind::Imu {
            if let Some(settings) = self.apply_imu_config_payload(&payload_value).await? {
                SensorReading::Imu(Box::new(ImuReading::from_settings(&settings)))
            } else {
                SensorReading::Raw(payload_value)
            }
        } else {
            SensorReading::Raw(payload_value)
        };
        let mut state = self.state.write().await;
        state.serialized_readings.entry(scope.clone()).or_default().insert(sensor.clone(), serialize_reading(&applied_payload));
        state.readings.entry(scope.clone()).or_default().insert(sensor, applied_payload);
        Ok(())
    }

    async fn apply_imu_config_payload(&self, payload: &JsonValue) -> Result<Option<ImuSettings>> {
        let mut fusion: Option<ImuFusionMethod> = None;
        let mut range: Option<ImuRange> = None;
        let mut interval: Option<Duration> = None;
        let mut dr_velocity_damp_tau_seconds: Option<f32> = None;
        let mut dr_still_velocity_zero_tau_seconds: Option<f32> = None;
        let mut dr_max_accel_world_mps2: Option<f32> = None;
        let mut dr_max_speed_mps: Option<f32> = None;
        let mut dr_max_position_m: Option<f32> = None;
        let mut dr_lock_position: Option<bool> = None;
        let mut gravity_reference_axis: Option<[f32; 3]> = None;
        let mut snap_gravity = false;
        let mut reset_pose = false;

        match payload {
            JsonValue::String(value) => {
                fusion = Some(ImuFusionMethod::from_str(value)?);
            }
            JsonValue::Object(map) => {
                if let Some(value) = map.get("fusion").and_then(JsonValue::as_str) {
                    fusion = Some(ImuFusionMethod::from_str(value)?);
                }
                if let Some(value) = map.get("range").and_then(JsonValue::as_str) {
                    range = Some(ImuRange::from_str(value)?);
                }
                if let Some(ms) = map.get("update_interval_ms").and_then(JsonValue::as_u64)
                    && ms > 0
                {
                    interval = Some(Duration::from_millis(ms));
                }
                dr_velocity_damp_tau_seconds = map.get("dr_velocity_damp_tau_seconds").and_then(JsonValue::as_f64).map(|value| value as f32).or(dr_velocity_damp_tau_seconds);
                dr_still_velocity_zero_tau_seconds = map.get("dr_still_velocity_zero_tau_seconds").and_then(JsonValue::as_f64).map(|value| value as f32).or(dr_still_velocity_zero_tau_seconds);
                dr_max_accel_world_mps2 = map.get("dr_max_accel_world_mps2").and_then(JsonValue::as_f64).map(|value| value as f32).or(dr_max_accel_world_mps2);
                dr_max_speed_mps = map.get("dr_max_speed_mps").and_then(JsonValue::as_f64).map(|value| value as f32).or(dr_max_speed_mps);
                dr_max_position_m = map.get("dr_max_position_m").and_then(JsonValue::as_f64).map(|value| value as f32).or(dr_max_position_m);
                dr_lock_position = map.get("dr_lock_position").and_then(JsonValue::as_bool).or(dr_lock_position);
                if let Some(value) = map.get("gravity_reference_axis").and_then(JsonValue::as_str) {
                    gravity_reference_axis = Some(parse_axis_reference(value).ok_or_else(|| Error::InvalidConfig("gravity_reference_axis must be one of: +x, -x, +y, -y, +z, -z".into()))?);
                }
                if map.get("snap_gravity").and_then(JsonValue::as_bool) == Some(true) {
                    snap_gravity = true;
                }
                if map.get("reset_pose").and_then(JsonValue::as_bool) == Some(true) {
                    reset_pose = true;
                }
            }
            JsonValue::Null => return Ok(None),
            _ => return Err(Error::InvalidConfig("IMU update payload must be a string or object".into())),
        }

        if fusion.is_none()
            && range.is_none()
            && interval.is_none()
            && dr_velocity_damp_tau_seconds.is_none()
            && dr_still_velocity_zero_tau_seconds.is_none()
            && dr_max_accel_world_mps2.is_none()
            && dr_max_speed_mps.is_none()
            && dr_max_position_m.is_none()
            && dr_lock_position.is_none()
            && gravity_reference_axis.is_none()
            && !snap_gravity
            && !reset_pose
        {
            return Ok(None);
        }

        let mount_correction = if gravity_reference_axis.is_some() || snap_gravity {
            let current_settings = self.current_imu_settings().await.ok_or_else(|| Error::InvalidState("IMU runtime is not running".into()))?;
            let sample = self.imu_state().await.and_then(|state| state.sample).ok_or_else(|| Error::InvalidState("IMU sample not available".into()))?;

            if sample.is_moving {
                return Err(Error::InvalidState("IMU is moving; hold the device still to calibrate gravity alignment".into()));
            }

            let accel_norm = vec3_norm(sample.accel);
            if !accel_norm.is_finite() || !(0.7..=1.3).contains(&accel_norm) {
                return Err(Error::InvalidState(format!("unexpected accel magnitude ({accel_norm:.2}g); hold the device still before calibrating")));
            }

            let accel_unit = vec3_normalize(sample.accel).ok_or_else(|| Error::InvalidState("unable to normalize accel vector".into()))?;
            let target_axis = gravity_reference_axis.unwrap_or_else(|| dominant_axis_unit(accel_unit));
            let rotation = quat_from_unit_vectors(accel_unit, target_axis).ok_or_else(|| Error::InvalidState("unable to compute gravity alignment rotation".into()))?;
            Some((rotation * current_settings.mount_correction).normalized().map_err(|_| Error::InvalidState("unable to normalize mount correction".into()))?)
        } else {
            None
        };

        let settings = self
            .update_imu_settings(ImuSettingsUpdate {
                fusion,
                range,
                interval,
                yaw_offset_deg: None,
                mount_correction,
                dr_velocity_damp_tau_seconds,
                dr_still_velocity_zero_tau_seconds,
                dr_max_accel_world_mps2,
                dr_max_speed_mps,
                dr_max_position_m,
                dr_lock_position,
            })
            .await?;
        if reset_pose {
            self.reset_imu_pose().await?;
        }
        Ok(Some(settings))
    }
}

fn apply_imu_state_to_typed_readings(values: &mut BTreeMap<SensorKind, SensorReading>, imu_state: &ImuState) {
    if let Some(sample) = imu_state.sample.as_ref() {
        values.insert(SensorKind::Accelerometer, SensorReading::Accelerometer(AxesReading::from(sample.accel)));
        values.insert(SensorKind::Gyroscope, SensorReading::Gyroscope(AxesReading::from(sample.gyro)));
        if let Some(mag) = sample.mag {
            values.insert(SensorKind::Magnetometer, SensorReading::Magnetometer(AxesReading::from(mag)));
        } else {
            values.remove(&SensorKind::Magnetometer);
        }
        values.insert(SensorKind::Imu, SensorReading::Imu(Box::new(ImuReading::from_sample(sample))));
        return;
    }

    values.remove(&SensorKind::Accelerometer);
    values.remove(&SensorKind::Gyroscope);
    values.remove(&SensorKind::Magnetometer);
    if let Some(message) = imu_state.last_error.clone() {
        values.insert(SensorKind::Imu, SensorReading::Imu(Box::new(ImuReading::error(message))));
    } else {
        values.remove(&SensorKind::Imu);
    }
}

fn apply_imu_state_to_cached_snapshot(serialized: &mut SensorSnapshot, imu_state: &ImuState) {
    if let Some(sample) = imu_state.sample.as_ref() {
        let accel = SensorReading::Accelerometer(AxesReading::from(sample.accel));
        serialized.insert(SensorKind::Accelerometer, serialize_reading(&accel));

        let gyro = SensorReading::Gyroscope(AxesReading::from(sample.gyro));
        serialized.insert(SensorKind::Gyroscope, serialize_reading(&gyro));

        if let Some(mag) = sample.mag {
            let mag = SensorReading::Magnetometer(AxesReading::from(mag));
            serialized.insert(SensorKind::Magnetometer, serialize_reading(&mag));
        } else {
            serialized.remove(&SensorKind::Magnetometer);
        }
        let imu = SensorReading::Imu(Box::new(ImuReading::from_sample(sample)));
        serialized.insert(SensorKind::Imu, serialize_reading(&imu));
        return;
    }

    serialized.remove(&SensorKind::Accelerometer);
    serialized.remove(&SensorKind::Gyroscope);
    serialized.remove(&SensorKind::Magnetometer);
    if let Some(message) = imu_state.last_error.clone() {
        let imu = SensorReading::Imu(Box::new(ImuReading::error(message)));
        serialized.insert(SensorKind::Imu, serialize_reading(&imu));
    } else {
        serialized.remove(&SensorKind::Imu);
    }
}

fn vec3_norm(v: [f32; 3]) -> f32 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

fn vec3_normalize(v: [f32; 3]) -> Option<[f32; 3]> {
    let n = vec3_norm(v);
    if !n.is_finite() || n <= f32::EPSILON {
        return None;
    }
    Some([v[0] / n, v[1] / n, v[2] / n])
}

fn dominant_axis_unit(v: [f32; 3]) -> [f32; 3] {
    let (ax, ay, az) = (v[0].abs(), v[1].abs(), v[2].abs());
    if ax >= ay && ax >= az {
        [v[0].signum(), 0.0, 0.0]
    } else if ay >= az {
        [0.0, v[1].signum(), 0.0]
    } else {
        [0.0, 0.0, v[2].signum()]
    }
}

fn parse_axis_reference(raw: &str) -> Option<[f32; 3]> {
    let axis = raw.trim().to_ascii_lowercase();
    match axis.as_str() {
        "x" | "+x" | "x+" => Some([1.0, 0.0, 0.0]),
        "-x" | "x-" => Some([-1.0, 0.0, 0.0]),
        "y" | "+y" | "y+" => Some([0.0, 1.0, 0.0]),
        "-y" | "y-" => Some([0.0, -1.0, 0.0]),
        "z" | "+z" | "z+" => Some([0.0, 0.0, 1.0]),
        "-z" | "z-" => Some([0.0, 0.0, -1.0]),
        _ => None,
    }
}

fn quat_from_unit_vectors(from: [f32; 3], to: [f32; 3]) -> Option<Quaternion> {
    let dot = from[0] * to[0] + from[1] * to[1] + from[2] * to[2];
    if !dot.is_finite() {
        return None;
    }

    // If opposite, rotate 180° about any axis orthogonal to `from`.
    if dot <= -0.999_999 {
        let axis_a = [0.0, from[2], -from[1]];
        let axis_b = [-from[2], 0.0, from[0]];
        let axis = if vec3_norm(axis_a) >= vec3_norm(axis_b) { axis_a } else { axis_b };
        let axis = vec3_normalize(axis)?;
        return Some(Quaternion::new(0.0, axis[0], axis[1], axis[2]));
    }

    let cross = [from[1] * to[2] - from[2] * to[1], from[2] * to[0] - from[0] * to[2], from[0] * to[1] - from[1] * to[0]];
    Quaternion::new(1.0 + dot, cross[0], cross[1], cross[2]).normalized().ok()
}
