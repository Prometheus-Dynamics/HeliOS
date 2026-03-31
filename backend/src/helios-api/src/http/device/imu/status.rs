use helios_peripherals::dto::{JsonData, SensorKind, SensorSnapshot, SensorSnapshotTyped};
use lib_sensors::{
    dto::{ImuAxesPayload, ImuOptionsPayload, ImuOrientationPayload, ImuQuaternionPayload, ImuSourcesPayload, ImuStatusPayload},
    imu::{ImuFusionMethod, ImuRange},
    model::SensorReading,
};
use serde_json::{Map as JsonMap, Value as JsonValue};

const IMU_INTERVAL_PRESETS: &[u64] = &[5, 10, 20, 50, 100, 200, 500, 1000];

pub(crate) fn imu_status_from_snapshot(values: &SensorSnapshot) -> ImuStatusPayload {
    let options = imu_options_from_snapshot(values);
    ImuSnapshot::from_snapshot(values).into_payload(options)
}

pub(crate) fn imu_status_from_snapshot_typed(values: &SensorSnapshotTyped) -> ImuStatusPayload {
    let options = imu_options_from_snapshot_typed(values);
    let mut payload = ImuStatusPayload { options: Some(options.clone()), ..ImuStatusPayload::default() };

    payload.accel = values.get(&SensorKind::Accelerometer).and_then(|reading| match reading {
        SensorReading::Accelerometer(axes) => Some(ImuAxesPayload { x: f64::from(axes.x), y: f64::from(axes.y), z: f64::from(axes.z) }),
        _ => None,
    });
    payload.gyro = values.get(&SensorKind::Gyroscope).and_then(|reading| match reading {
        SensorReading::Gyroscope(axes) => Some(ImuAxesPayload { x: f64::from(axes.x), y: f64::from(axes.y), z: f64::from(axes.z) }),
        _ => None,
    });
    payload.mag = values.get(&SensorKind::Magnetometer).and_then(|reading| match reading {
        SensorReading::Magnetometer(axes) => Some(ImuAxesPayload { x: f64::from(axes.x), y: f64::from(axes.y), z: f64::from(axes.z) }),
        _ => None,
    });

    if let Some(SensorReading::Imu(imu)) = values.get(&SensorKind::Imu) {
        payload.orientation = imu.orientation.map(|[roll, pitch, yaw]| ImuOrientationPayload {
            roll: f64::from(roll),
            pitch: f64::from(pitch),
            yaw: f64::from(yaw),
            quaternion: imu.quaternion.map(|[w, x, y, z]| ImuQuaternionPayload { w: f64::from(w), x: f64::from(x), y: f64::from(y), z: f64::from(z) }),
        });
        payload.linear_accel = imu.linear_accel.map(|[x, y, z]| ImuAxesPayload { x: f64::from(x), y: f64::from(y), z: f64::from(z) });
        payload.corrected_world_accel_mps2 = imu.corrected_world_accel_mps2.map(|[x, y, z]| ImuAxesPayload { x: f64::from(x), y: f64::from(y), z: f64::from(z) });
        payload.velocity_world = imu.velocity_world.map(|[x, y, z]| ImuAxesPayload { x: f64::from(x), y: f64::from(y), z: f64::from(z) });
        payload.velocity_delta_world = imu.velocity_delta_world.map(|[x, y, z]| ImuAxesPayload { x: f64::from(x), y: f64::from(y), z: f64::from(z) });
        payload.linear_speed_mps = imu.linear_speed_mps.map(f64::from);
        payload.linear_speed_normalized = imu.linear_speed_normalized.map(f64::from);
        payload.angular_velocity_dps = imu.angular_velocity_dps.map(|[x, y, z]| ImuAxesPayload { x: f64::from(x), y: f64::from(y), z: f64::from(z) });
        payload.angular_speed_dps = imu.angular_speed_dps.map(f64::from);
        payload.angular_speed_normalized = imu.angular_speed_normalized.map(f64::from);
        payload.gyro_bias_dps = imu.gyro_bias_dps.map(|[x, y, z]| ImuAxesPayload { x: f64::from(x), y: f64::from(y), z: f64::from(z) });
        payload.position_world = imu.position_world.map(|[x, y, z]| ImuAxesPayload { x: f64::from(x), y: f64::from(y), z: f64::from(z) });
        payload.is_moving = imu.is_moving;
        payload.is_moving_fast = imu.is_moving_fast;
        payload.is_still = imu.is_still;
        payload.stillness_confidence = imu.stillness_confidence.map(f64::from);
        payload.rotation_contaminated = imu.rotation_contaminated;
        payload.motion_g = imu.motion_g.map(f64::from);
        payload.motion_fast_g = imu.motion_fast_g.map(f64::from);
        payload.motion_fast_threshold_g = imu.motion_fast_threshold_g.map(f64::from);
        payload.motion_noise_floor_g = imu.motion_noise_floor_g.map(f64::from);
        payload.dr_confidence = imu.dr_confidence.map(f64::from);
        payload.fusion = imu.fusion.map(|f| f.to_string());
        payload.range = imu.range.map(|r| r.to_string());
        payload.update_interval_ms = imu.update_interval_ms;
        payload.dr_velocity_damp_tau_seconds = imu.dr_velocity_damp_tau_seconds.map(f64::from);
        payload.dr_still_velocity_zero_tau_seconds = imu.dr_still_velocity_zero_tau_seconds.map(f64::from);
        payload.dr_max_accel_world_mps2 = imu.dr_max_accel_world_mps2.map(f64::from);
        payload.dr_max_speed_mps = imu.dr_max_speed_mps.map(f64::from);
        payload.dr_max_position_m = imu.dr_max_position_m.map(f64::from);
        payload.dr_lock_position = imu.dr_lock_position;
        payload.updated_at = imu.updated_at.map(|ts| ts.to_rfc3339());
        payload.dt_seconds = imu.dt_seconds.map(f64::from);
        payload.last_error = imu.last_error.clone();
        if !imu.sources.is_empty() {
            payload.sources = Some(ImuSourcesPayload { accel_gyro: imu.sources.accel_gyro.clone(), magnetometer: imu.sources.magnetometer.clone() });
        }
    }

    if payload.fusion.is_none() {
        payload.fusion = options.fusion.first().cloned();
    }
    if payload.range.is_none() {
        payload.range = options.range.first().cloned();
    }
    if payload.update_interval_ms.is_none() {
        payload.update_interval_ms = options.intervals_ms.first().copied();
    }
    if payload.orientation.is_some()
        || payload.linear_accel.is_some()
        || payload.corrected_world_accel_mps2.is_some()
        || payload.velocity_world.is_some()
        || payload.velocity_delta_world.is_some()
        || payload.angular_velocity_dps.is_some()
        || payload.position_world.is_some()
        || payload.accel.is_some()
        || payload.gyro.is_some()
        || payload.mag.is_some()
    {
        payload.has_sample = true;
    }
    payload
}

fn imu_options_from_snapshot(values: &SensorSnapshot) -> ImuOptionsPayload {
    let mut intervals: Vec<u64> = IMU_INTERVAL_PRESETS.to_vec();
    if let Some(JsonValue::Object(map)) = snapshot_json(values, &SensorKind::Imu)
        && let Some(interval) = map.get("update_interval_ms").and_then(as_u64)
    {
        intervals.push(interval);
    }
    intervals.sort_unstable();
    intervals.dedup();

    ImuOptionsPayload { fusion: ImuFusionMethod::options().iter().map(ToString::to_string).collect(), range: ImuRange::options().iter().map(ToString::to_string).collect(), intervals_ms: intervals }
}

fn imu_options_from_snapshot_typed(values: &SensorSnapshotTyped) -> ImuOptionsPayload {
    let mut intervals: Vec<u64> = IMU_INTERVAL_PRESETS.to_vec();
    if let Some(SensorReading::Imu(imu)) = values.get(&SensorKind::Imu)
        && let Some(interval) = imu.update_interval_ms
    {
        intervals.push(interval);
    }
    intervals.sort_unstable();
    intervals.dedup();

    ImuOptionsPayload { fusion: ImuFusionMethod::options().iter().map(ToString::to_string).collect(), range: ImuRange::options().iter().map(ToString::to_string).collect(), intervals_ms: intervals }
}

#[derive(Default)]
struct ImuSnapshot {
    accel: Option<ImuAxesPayload>,
    gyro: Option<ImuAxesPayload>,
    mag: Option<ImuAxesPayload>,
    orientation: Option<ImuOrientationPayload>,
    linear_accel: Option<ImuAxesPayload>,
    corrected_world_accel_mps2: Option<ImuAxesPayload>,
    velocity_world: Option<ImuAxesPayload>,
    velocity_delta_world: Option<ImuAxesPayload>,
    linear_speed_mps: Option<f64>,
    linear_speed_normalized: Option<f64>,
    angular_velocity_dps: Option<ImuAxesPayload>,
    angular_speed_dps: Option<f64>,
    angular_speed_normalized: Option<f64>,
    gyro_bias_dps: Option<ImuAxesPayload>,
    position_world: Option<ImuAxesPayload>,
    is_moving: Option<bool>,
    is_moving_fast: Option<bool>,
    is_still: Option<bool>,
    stillness_confidence: Option<f64>,
    rotation_contaminated: Option<bool>,
    motion_g: Option<f64>,
    motion_fast_g: Option<f64>,
    motion_fast_threshold_g: Option<f64>,
    motion_noise_floor_g: Option<f64>,
    dr_confidence: Option<f64>,
    fusion: Option<String>,
    range: Option<String>,
    update_interval_ms: Option<u64>,
    dr_velocity_damp_tau_seconds: Option<f64>,
    dr_still_velocity_zero_tau_seconds: Option<f64>,
    dr_max_accel_world_mps2: Option<f64>,
    dr_max_speed_mps: Option<f64>,
    dr_max_position_m: Option<f64>,
    dr_lock_position: Option<bool>,
    updated_at: Option<String>,
    dt_seconds: Option<f64>,
    last_error: Option<String>,
    sources: Option<ImuSourcesPayload>,
}

impl ImuSnapshot {
    fn from_snapshot(values: &SensorSnapshot) -> Self {
        let mut status = ImuSnapshot {
            accel: parse_axes(values.get(&SensorKind::Accelerometer)),
            gyro: parse_axes(values.get(&SensorKind::Gyroscope)),
            mag: parse_axes(values.get(&SensorKind::Magnetometer)),
            ..Default::default()
        };

        if let Some(JsonValue::Object(map)) = snapshot_json(values, &SensorKind::Imu) {
            status.orientation = parse_orientation(&map);
            status.linear_accel = map.get("linear_accel").and_then(parse_axes_object);
            status.corrected_world_accel_mps2 = map.get("corrected_world_accel_mps2").and_then(parse_axes_object);
            status.velocity_world = map.get("velocity_world").and_then(parse_axes_object);
            status.velocity_delta_world = map.get("velocity_delta_world").and_then(parse_axes_object);
            status.linear_speed_mps = map.get("linear_speed_mps").and_then(as_f64);
            status.linear_speed_normalized = map.get("linear_speed_normalized").and_then(as_f64);
            status.angular_velocity_dps = map.get("angular_velocity_dps").and_then(parse_axes_object);
            status.angular_speed_dps = map.get("angular_speed_dps").and_then(as_f64);
            status.angular_speed_normalized = map.get("angular_speed_normalized").and_then(as_f64);
            status.gyro_bias_dps = map.get("gyro_bias_dps").and_then(parse_axes_object);
            status.position_world = map.get("position_world").and_then(parse_axes_object);
            status.is_moving = map.get("is_moving").and_then(as_bool);
            status.is_moving_fast = map.get("is_moving_fast").and_then(as_bool);
            status.is_still = map.get("is_still").and_then(as_bool);
            status.stillness_confidence = map.get("stillness_confidence").and_then(as_f64);
            status.rotation_contaminated = map.get("rotation_contaminated").and_then(as_bool);
            status.motion_g = map.get("motion_g").and_then(as_f64);
            status.motion_fast_g = map.get("motion_fast_g").and_then(as_f64);
            status.motion_fast_threshold_g = map.get("motion_fast_threshold_g").and_then(as_f64);
            status.motion_noise_floor_g = map.get("motion_noise_floor_g").and_then(as_f64);
            status.dr_confidence = map.get("dr_confidence").and_then(as_f64);
            status.fusion = map.get("fusion").and_then(as_string);
            status.range = map.get("range").and_then(as_string);
            status.update_interval_ms = map.get("update_interval_ms").and_then(as_u64);
            status.dr_velocity_damp_tau_seconds = map.get("dr_velocity_damp_tau_seconds").and_then(as_f64);
            status.dr_still_velocity_zero_tau_seconds = map.get("dr_still_velocity_zero_tau_seconds").and_then(as_f64);
            status.dr_max_accel_world_mps2 = map.get("dr_max_accel_world_mps2").and_then(as_f64);
            status.dr_max_speed_mps = map.get("dr_max_speed_mps").and_then(as_f64);
            status.dr_max_position_m = map.get("dr_max_position_m").and_then(as_f64);
            status.dr_lock_position = map.get("dr_lock_position").and_then(as_bool);
            status.updated_at = map.get("updated_at").and_then(as_string);
            status.dt_seconds = map.get("dt_seconds").and_then(as_f64);
            status.last_error = map.get("error").and_then(as_string);
            if let Some(JsonValue::Object(sources)) = map.get("sources") {
                status.sources = Some(ImuSourcesPayload { accel_gyro: sources.get("accel_gyro").and_then(as_string), magnetometer: sources.get("magnetometer").and_then(as_string) });
            }
        }

        status
    }

    fn into_payload(mut self, options: ImuOptionsPayload) -> ImuStatusPayload {
        if self.fusion.is_none() {
            self.fusion = options.fusion.first().cloned();
        }
        if self.range.is_none() {
            self.range = options.range.first().cloned();
        }
        if self.update_interval_ms.is_none() {
            self.update_interval_ms = options.intervals_ms.first().copied();
        }

        let mut payload = ImuStatusPayload { options: Some(options), ..ImuStatusPayload::default() };
        payload.accel = self.accel;
        payload.gyro = self.gyro;
        payload.mag = self.mag;
        payload.orientation = self.orientation;
        payload.linear_accel = self.linear_accel;
        payload.corrected_world_accel_mps2 = self.corrected_world_accel_mps2;
        payload.velocity_world = self.velocity_world;
        payload.velocity_delta_world = self.velocity_delta_world;
        payload.linear_speed_mps = self.linear_speed_mps;
        payload.linear_speed_normalized = self.linear_speed_normalized;
        payload.angular_velocity_dps = self.angular_velocity_dps;
        payload.angular_speed_dps = self.angular_speed_dps;
        payload.angular_speed_normalized = self.angular_speed_normalized;
        payload.gyro_bias_dps = self.gyro_bias_dps;
        payload.position_world = self.position_world;
        payload.is_moving = self.is_moving;
        payload.is_moving_fast = self.is_moving_fast;
        payload.is_still = self.is_still;
        payload.stillness_confidence = self.stillness_confidence;
        payload.rotation_contaminated = self.rotation_contaminated;
        payload.motion_g = self.motion_g;
        payload.motion_fast_g = self.motion_fast_g;
        payload.motion_fast_threshold_g = self.motion_fast_threshold_g;
        payload.motion_noise_floor_g = self.motion_noise_floor_g;
        payload.dr_confidence = self.dr_confidence;
        payload.fusion = self.fusion;
        payload.range = self.range;
        payload.update_interval_ms = self.update_interval_ms;
        payload.dr_velocity_damp_tau_seconds = self.dr_velocity_damp_tau_seconds;
        payload.dr_still_velocity_zero_tau_seconds = self.dr_still_velocity_zero_tau_seconds;
        payload.dr_max_accel_world_mps2 = self.dr_max_accel_world_mps2;
        payload.dr_max_speed_mps = self.dr_max_speed_mps;
        payload.dr_max_position_m = self.dr_max_position_m;
        payload.dr_lock_position = self.dr_lock_position;
        payload.updated_at = self.updated_at;
        payload.dt_seconds = self.dt_seconds;
        payload.last_error = self.last_error;
        payload.sources = self.sources;
        if payload.orientation.is_some()
            || payload.linear_accel.is_some()
            || payload.corrected_world_accel_mps2.is_some()
            || payload.velocity_world.is_some()
            || payload.velocity_delta_world.is_some()
            || payload.angular_velocity_dps.is_some()
            || payload.position_world.is_some()
            || payload.accel.is_some()
            || payload.gyro.is_some()
            || payload.mag.is_some()
        {
            payload.has_sample = true;
        }
        payload
    }
}

fn snapshot_json(values: &SensorSnapshot, kind: &SensorKind) -> Option<JsonValue> {
    values.get(kind).and_then(|data| data.to_value().ok())
}

fn parse_axes(value: Option<&JsonData>) -> Option<ImuAxesPayload> {
    let value = value?.to_value().ok()?;
    match value {
        JsonValue::Array(values) if values.len() >= 3 => Some(ImuAxesPayload { x: as_f64(&values[0])?, y: as_f64(&values[1])?, z: as_f64(&values[2])? }),
        _ => None,
    }
}

fn parse_axes_object(value: &JsonValue) -> Option<ImuAxesPayload> {
    if let JsonValue::Object(map) = value { Some(ImuAxesPayload { x: map.get("x").and_then(as_f64)?, y: map.get("y").and_then(as_f64)?, z: map.get("z").and_then(as_f64)? }) } else { None }
}

fn parse_orientation(map: &JsonMap<String, JsonValue>) -> Option<ImuOrientationPayload> {
    let roll = map.get("roll").and_then(as_f64)?;
    let pitch = map.get("pitch").and_then(as_f64)?;
    let yaw = map.get("yaw").and_then(as_f64)?;
    let quaternion = map.get("quaternion").and_then(parse_quaternion);
    Some(ImuOrientationPayload { roll, pitch, yaw, quaternion })
}

fn parse_quaternion(value: &JsonValue) -> Option<ImuQuaternionPayload> {
    let JsonValue::Object(map) = value else { return None };
    let w = map.get("w").and_then(as_f64)?;
    let x = map.get("x").and_then(as_f64)?;
    let y = map.get("y").and_then(as_f64)?;
    let z = map.get("z").and_then(as_f64)?;
    Some(ImuQuaternionPayload { w, x, y, z })
}

fn as_f64(value: &JsonValue) -> Option<f64> {
    match value {
        JsonValue::Number(v) => v.as_f64(),
        _ => None,
    }
}

pub(super) fn as_u64(value: &JsonValue) -> Option<u64> {
    match value {
        JsonValue::Number(v) => v.as_u64().or_else(|| v.as_f64().and_then(|n| (n >= 0.0).then_some(n.round() as u64))),
        _ => None,
    }
}

fn as_string(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::String(v) => Some(v.clone()),
        _ => None,
    }
}

fn as_bool(value: &JsonValue) -> Option<bool> {
    match value {
        JsonValue::Bool(v) => Some(*v),
        _ => None,
    }
}
