use std::collections::BTreeMap;

use lib_sensors::dto::{ImuAxesPayload, ImuOptionsPayload, ImuOrientationPayload, ImuQuaternionPayload, ImuSourcesPayload, ImuStatusPayload};
use lib_sensors::imu::{ImuFusionMethod, ImuRange};
use lib_sensors::model::SensorReading;
use serde_json::{Map as JsonMap, Value as JsonValue};

use crate::dto::{JsonData, SensorKind, SensorSnapshot};

pub const IMU_INTERVAL_PRESETS: &[u64] = &[5, 10, 20, 50, 100, 200, 500, 1000];

pub fn imu_status_from_snapshot(values: &SensorSnapshot) -> ImuStatusPayload {
    let options = imu_options(values);
    ImuSnapshot::from(values).into_payload(options)
}

pub fn imu_status_from_typed(values: &BTreeMap<SensorKind, SensorReading>) -> ImuStatusPayload {
    let options = imu_options_from_typed(values);
    ImuSnapshotTyped::from(values).into_payload(options)
}

pub fn imu_options(values: &SensorSnapshot) -> ImuOptionsPayload {
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

pub fn imu_options_from_typed(values: &BTreeMap<SensorKind, SensorReading>) -> ImuOptionsPayload {
    let mut intervals: Vec<u64> = IMU_INTERVAL_PRESETS.to_vec();
    if let Some(SensorReading::Imu(reading)) = values.get(&SensorKind::Imu)
        && let Some(interval) = reading.update_interval_ms
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
    fusion: Option<String>,
    range: Option<String>,
    update_interval_ms: Option<u64>,
    updated_at: Option<String>,
    dt_seconds: Option<f64>,
    last_error: Option<String>,
    sources: Option<ImuSourcesPayload>,
}

impl ImuSnapshot {
    fn from(values: &SensorSnapshot) -> Self {
        let accel = parse_axes(values.get(&SensorKind::Accelerometer));
        let gyro = parse_axes(values.get(&SensorKind::Gyroscope));
        let mag = parse_axes(values.get(&SensorKind::Magnetometer));

        let (orientation, fusion, range, update_interval_ms, updated_at, dt_seconds, last_error, sources) = if let Some(JsonValue::Object(map)) = snapshot_json(values, &SensorKind::Imu) {
            let sources = map.get("sources").and_then(|value| {
                let JsonValue::Object(sources) = value else { return None };
                Some(ImuSourcesPayload { accel_gyro: sources.get("accel_gyro").and_then(as_string), magnetometer: sources.get("magnetometer").and_then(as_string) })
            });

            (
                parse_orientation(&map),
                map.get("fusion").and_then(as_string),
                map.get("range").and_then(as_string),
                map.get("update_interval_ms").and_then(as_u64),
                map.get("updated_at").and_then(as_string),
                map.get("dt_seconds").and_then(as_f64),
                map.get("error").and_then(as_string),
                sources,
            )
        } else {
            (None, None, None, None, None, None, None, None)
        };

        Self { accel, gyro, mag, orientation, fusion, range, update_interval_ms, updated_at, dt_seconds, last_error, sources }
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
        payload.fusion = self.fusion;
        payload.range = self.range;
        payload.update_interval_ms = self.update_interval_ms;
        payload.updated_at = self.updated_at;
        payload.dt_seconds = self.dt_seconds;
        payload.last_error = self.last_error;
        payload.sources = self.sources;
        if payload.orientation.is_some() || payload.accel.is_some() || payload.gyro.is_some() || payload.mag.is_some() {
            payload.has_sample = true;
        }
        payload
    }
}

fn snapshot_json(values: &SensorSnapshot, kind: &SensorKind) -> Option<JsonValue> {
    values.get(kind).map(|data| data.to_value())
}

fn parse_axes(value: Option<&JsonData>) -> Option<ImuAxesPayload> {
    let value = value?.to_value();
    match value {
        JsonValue::Array(values) if values.len() >= 3 => Some(ImuAxesPayload { x: as_f64(&values[0])?, y: as_f64(&values[1])?, z: as_f64(&values[2])? }),
        _ => None,
    }
}

#[derive(Default)]
struct ImuSnapshotTyped {
    accel: Option<ImuAxesPayload>,
    gyro: Option<ImuAxesPayload>,
    mag: Option<ImuAxesPayload>,
    orientation: Option<ImuOrientationPayload>,
    fusion: Option<String>,
    range: Option<String>,
    update_interval_ms: Option<u64>,
    updated_at: Option<String>,
    dt_seconds: Option<f64>,
    last_error: Option<String>,
    sources: Option<ImuSourcesPayload>,
}

impl ImuSnapshotTyped {
    fn from(values: &BTreeMap<SensorKind, SensorReading>) -> Self {
        let accel = values.get(&SensorKind::Accelerometer).and_then(|reading| match reading {
            SensorReading::Accelerometer(axes) => Some(ImuAxesPayload { x: f64::from(axes.x), y: f64::from(axes.y), z: f64::from(axes.z) }),
            _ => None,
        });
        let gyro = values.get(&SensorKind::Gyroscope).and_then(|reading| match reading {
            SensorReading::Gyroscope(axes) => Some(ImuAxesPayload { x: f64::from(axes.x), y: f64::from(axes.y), z: f64::from(axes.z) }),
            _ => None,
        });
        let mag = values.get(&SensorKind::Magnetometer).and_then(|reading| match reading {
            SensorReading::Magnetometer(axes) => Some(ImuAxesPayload { x: f64::from(axes.x), y: f64::from(axes.y), z: f64::from(axes.z) }),
            _ => None,
        });

        let (orientation, fusion, range, update_interval_ms, updated_at, dt_seconds, last_error, sources) = if let Some(SensorReading::Imu(imu)) = values.get(&SensorKind::Imu) {
            let sources = (!imu.sources.is_empty()).then(|| ImuSourcesPayload { accel_gyro: imu.sources.accel_gyro.clone(), magnetometer: imu.sources.magnetometer.clone() });

            (
                imu.orientation.map(|[roll, pitch, yaw]| ImuOrientationPayload {
                    roll: f64::from(roll),
                    pitch: f64::from(pitch),
                    yaw: f64::from(yaw),
                    quaternion: imu.quaternion.map(|[w, x, y, z]| ImuQuaternionPayload { w: f64::from(w), x: f64::from(x), y: f64::from(y), z: f64::from(z) }),
                }),
                imu.fusion.map(|f| f.to_string()),
                imu.range.map(|r| r.to_string()),
                imu.update_interval_ms,
                imu.updated_at.map(|ts| ts.to_rfc3339()),
                imu.dt_seconds.map(f64::from),
                imu.last_error.clone(),
                sources,
            )
        } else {
            (None, None, None, None, None, None, None, None)
        };

        Self { accel, gyro, mag, orientation, fusion, range, update_interval_ms, updated_at, dt_seconds, last_error, sources }
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
        payload.fusion = self.fusion;
        payload.range = self.range;
        payload.update_interval_ms = self.update_interval_ms;
        payload.updated_at = self.updated_at;
        payload.dt_seconds = self.dt_seconds;
        payload.last_error = self.last_error;
        payload.sources = self.sources;
        if payload.orientation.is_some() || payload.accel.is_some() || payload.gyro.is_some() || payload.mag.is_some() {
            payload.has_sample = true;
        }
        payload
    }
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

fn as_u64(value: &JsonValue) -> Option<u64> {
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
