use crate::http::error_history::{ErrorHistoryEntry, record_error_entry};
use axum::{
    Json,
    extract::{
        State,
        ws::{Message, WebSocket},
    },
    response::IntoResponse,
};
use helios_peripherals::dto::{I2cInventory, JsonData, SensorKind, SensorScope, SensorSnapshot, SensorSnapshotTyped};
use helios_peripherals::ipc::{SensorCommand, SensorEvent};
use lib_ipc::types::CommandId;
use lib_sensors::dto::{ImuAxesPayload, ImuOptionsPayload, ImuOrientationPayload, ImuQuaternionPayload, ImuSourcesPayload, ImuStatusPayload, ImuUpdateRequest};
use lib_sensors::imu::{ImuFusionMethod, ImuRange};
use lib_sensors::model::SensorReading;
use serde_json::{Map as JsonMap, Value as JsonValue, json};
use tracing::{error, warn};
use uuid::Uuid;

use super::super::AppState;
use super::super::error::{ApiError, ApiResult};

#[utoipa::path(
    get,
    path = "/device/i2c",
    tag = "Device",
    responses(
        (status = 200, description = "I2C inventory", body = I2cInventory),
        (status = 400, description = "Bad request", body = super::super::error::ErrorBody),
        (status = 503, description = "Peripherals IPC unavailable", body = super::super::error::ErrorBody)
    )
)]
pub async fn i2c(State(state): State<AppState>) -> ApiResult<impl IntoResponse> {
    let Some(peripherals) = state.ensure_sensors().await else {
        return Err(ApiError::service_unavailable("peripherals IPC unavailable"));
    };

    match peripherals.i2c_inventory().await {
        Ok(Ok(inv)) => Ok(Json(inv)),
        Ok(Err(reason)) => Err(ApiError::bad_request(reason)),
        Err(err) => {
            error!(%err, "failed to send i2c inventory command");
            state.invalidate_sensors().await;
            if let Some(peripherals) = state.ensure_sensors().await {
                match peripherals.i2c_inventory().await {
                    Ok(Ok(inv)) => return Ok(Json(inv)),
                    Ok(Err(reason)) => return Err(ApiError::bad_request(reason)),
                    Err(err) => error!(%err, "failed to send i2c inventory command after reconnect"),
                }
            }
            Err(ApiError::bad_gateway("failed to fetch i2c inventory"))
        }
    }
}

#[utoipa::path(
    get,
    path = "/device/imu",
    tag = "Device",
    responses(
        (status = 200, description = "IMU status", body = ImuStatusPayload),
        (status = 400, description = "Bad request", body = super::super::error::ErrorBody),
        (status = 503, description = "Peripherals IPC unavailable", body = super::super::error::ErrorBody)
    )
)]
pub async fn imu_status(State(state): State<AppState>) -> ApiResult<impl IntoResponse> {
    let Some(peripherals) = state.ensure_sensors().await else {
        return Err(ApiError::service_unavailable("peripherals IPC unavailable"));
    };

    match peripherals.sensor_snapshot_typed(SensorScope::Device).await {
        Ok(Ok(snapshot)) => Ok(Json(imu_status_from_snapshot_typed(&snapshot))),
        Ok(Err(reason)) => {
            warn!(%reason, "typed IMU snapshot rejected; falling back to JSON snapshot");
            match peripherals.sensor_snapshot(SensorScope::Device).await {
                Ok(Ok(snapshot)) => Ok(Json(imu_status_from_snapshot(&snapshot))),
                Ok(Err(reason)) => Err(ApiError::bad_request(reason)),
                Err(err) => {
                    error!(%err, "failed to fetch IMU status");
                    Err(ApiError::bad_gateway("failed to fetch IMU status"))
                }
            }
        }
        Err(err) => {
            warn!(%err, "typed IMU snapshot failed; falling back to JSON snapshot");
            match peripherals.sensor_snapshot(SensorScope::Device).await {
                Ok(Ok(snapshot)) => Ok(Json(imu_status_from_snapshot(&snapshot))),
                Ok(Err(reason)) => Err(ApiError::bad_request(reason)),
                Err(err) => {
                    error!(%err, "failed to fetch IMU status");
                    Err(ApiError::bad_gateway("failed to fetch IMU status"))
                }
            }
        }
    }
}

#[utoipa::path(
    patch,
    path = "/device/imu",
    tag = "Device",
    request_body = ImuUpdateRequest,
    responses(
        (status = 200, description = "Updated IMU status", body = ImuStatusPayload),
        (status = 400, description = "Invalid request", body = super::super::error::ErrorBody),
        (status = 503, description = "Peripherals IPC unavailable", body = super::super::error::ErrorBody)
    )
)]
pub async fn update_imu(State(state): State<AppState>, Json(payload): Json<ImuUpdateRequest>) -> ApiResult<impl IntoResponse> {
    let Some(peripherals) = state.ensure_sensors().await else {
        return Err(ApiError::service_unavailable("peripherals IPC unavailable"));
    };

    let update = ImuUpdate::try_from_request(payload)?;
    if update.is_empty() {
        return Err(ApiError::bad_request("no IMU fields provided"));
    }

    let update_json = update.into_json();
    match peripherals.update_sensor(SensorScope::Device, SensorKind::Imu, JsonData::from_value(&update_json)).await {
        Ok(Ok(snapshot)) => match peripherals.sensor_snapshot_typed(SensorScope::Device).await {
            Ok(Ok(typed)) => Ok(Json(imu_status_from_snapshot_typed(&typed))),
            Ok(Err(reason)) => {
                warn!(%reason, "typed IMU snapshot rejected after update; returning JSON snapshot");
                Ok(Json(imu_status_from_snapshot(&snapshot)))
            }
            Err(err) => {
                warn!(%err, "typed IMU snapshot failed after update; returning JSON snapshot");
                Ok(Json(imu_status_from_snapshot(&snapshot)))
            }
        },
        Ok(Err(reason)) => Err(ApiError::bad_request(reason)),
        Err(err) => {
            error!(%err, "failed to update IMU settings");
            Err(ApiError::bad_gateway("failed to update IMU"))
        }
    }
}

pub async fn handle_imu_ws(mut socket: WebSocket, state: AppState) -> Result<(), String> {
    let error_context = WsErrorContext { request_id: Uuid::new_v4().to_string(), trace_id: Uuid::new_v4().to_string() };
    if state.ensure_sensors().await.is_none() {
        send_ws_error(&mut socket, &error_context, "peripherals IPC unavailable", "connect").await;
        return Ok(());
    }

    let conn = crate::ipc::peripherals::connect_sensors_stream().await.map_err(|err| err.to_string())?;
    let scope = SensorScope::Device;
    let mut session = conn.session;

    let subscribe = SensorCommand::Subscribe { command_id: CommandId::new(), scope: scope.clone() };
    if let Err(err) = session.send_command(conn.client.journal(), &subscribe).await {
        send_ws_error(&mut socket, &error_context, format!("failed to subscribe: {err}"), "subscribe").await;
        return Err(err.to_string());
    }

    loop {
        tokio::select! {
            event = session.next_event() => {
                match event {
                    Ok(Some(SensorEvent::Snapshot { scope: event_scope, values, .. })) if event_scope == scope => {
                        let status = imu_status_from_snapshot(&values);
                        if let Ok(payload) = serde_json::to_string(&status)
                            && socket.send(Message::Text(payload.into())).await.is_err()
                        {
                            break;
                        }
                    }
                    Ok(Some(SensorEvent::Nack { reason, .. })) => {
                        send_ws_error(&mut socket, &error_context, reason.clone(), "stream").await;
                        break;
                    }
                    Ok(Some(SensorEvent::Unsubscribed { scope: event_scope })) if event_scope == scope => break,
                    Ok(None) => break,
                    Err(err) => {
                        send_ws_error(&mut socket, &error_context, err.to_string(), "stream").await;
                        break;
                    }
                    _ => {}
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => {
                        let _ = session.send_command(conn.client.journal(), &SensorCommand::Unsubscribe { command_id: CommandId::new(), scope: scope.clone() }).await;
                        break;
                    }
                    Some(Ok(Message::Text(text))) if text.trim().eq_ignore_ascii_case("unsubscribe") => {
                        let _ = session.send_command(conn.client.journal(), &SensorCommand::Unsubscribe { command_id: CommandId::new(), scope: scope.clone() }).await;
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

#[derive(Debug, Default)]
struct WsErrorContext {
    request_id: String,
    trace_id: String,
}

async fn send_ws_error_with_context(socket: &mut WebSocket, context: &WsErrorContext, reason: impl Into<String>, operation: &str) {
    let reason = reason.into();
    let body = serde_json::json!({
        "status": "imu_stream_unavailable",
        "reason": reason.clone(),
        "code": null,
        "timestamp_ms": chrono::Utc::now().timestamp_millis().max(0) as u64,
        "source": "helios-api/http/device/imu.ws",
        "operation": operation,
        "request_id": context.request_id.as_str(),
        "trace_id": context.trace_id.as_str(),
        "retryable": null,
        "remediation": null,
        "reported_by": "helios-api"
    });
    record_error_entry(ErrorHistoryEntry {
        id: Uuid::new_v4().to_string(),
        status: None,
        code: "imu_stream_unavailable".to_string(),
        error: reason,
        details: None,
        timestamp_ms: body.get("timestamp_ms").and_then(|v| v.as_u64()).unwrap_or_default(),
        source: Some("helios-api/http/device/imu.ws".to_string()),
        operation: Some(operation.to_string()),
        request_id: Some(context.request_id.clone()),
        trace_id: Some(context.trace_id.clone()),
        retryable: None,
        remediation: None,
        reported_by: Some("helios-api".to_string()),
        transport: Some("ws".to_string()),
    });
    let _ = socket.send(Message::Text(body.to_string().into())).await;
}

async fn send_ws_error(socket: &mut WebSocket, context: &WsErrorContext, reason: impl Into<String>, operation: &str) {
    send_ws_error_with_context(socket, context, reason, operation).await;
}

#[derive(Default)]
struct ImuUpdate {
    fusion: Option<String>,
    range: Option<String>,
    update_interval_ms: Option<u64>,
    dr_velocity_damp_tau_seconds: Option<f64>,
    dr_still_velocity_zero_tau_seconds: Option<f64>,
    dr_max_accel_world_mps2: Option<f64>,
    dr_max_speed_mps: Option<f64>,
    dr_max_position_m: Option<f64>,
    dr_lock_position: Option<bool>,
    gravity_reference_axis: Option<String>,
    snap_gravity: Option<bool>,
    reset_pose: Option<bool>,
}

impl ImuUpdate {
    fn try_from_request(req: ImuUpdateRequest) -> Result<Self, Box<ApiError>> {
        let fusion = req.fusion.as_ref().and_then(|s| trimmed_string(s));
        let range = req.range.as_ref().and_then(|s| trimmed_string(s));
        let update_interval_ms = match req.update_interval_ms {
            Some(0) => return Err(Box::new(ApiError::bad_request("update_interval_ms must be greater than zero"))),
            Some(ms) => Some(ms),
            None => None,
        };
        let dr_velocity_damp_tau_seconds = parse_positive_f64(req.dr_velocity_damp_tau_seconds, "dr_velocity_damp_tau_seconds")?;
        let dr_still_velocity_zero_tau_seconds = parse_positive_f64(req.dr_still_velocity_zero_tau_seconds, "dr_still_velocity_zero_tau_seconds")?;
        let dr_max_accel_world_mps2 = parse_positive_f64(req.dr_max_accel_world_mps2, "dr_max_accel_world_mps2")?;
        let dr_max_speed_mps = parse_positive_f64(req.dr_max_speed_mps, "dr_max_speed_mps")?;
        let dr_max_position_m = parse_positive_f64(req.dr_max_position_m, "dr_max_position_m")?;
        let dr_lock_position = req.dr_lock_position;
        let gravity_reference_axis = req.gravity_reference_axis.as_ref().and_then(|s| trimmed_string(s));
        if let Some(axis) = gravity_reference_axis.as_deref()
            && !is_valid_axis_reference(axis)
        {
            return Err(Box::new(ApiError::bad_request("gravity_reference_axis must be one of: +x, -x, +y, -y, +z, -z")));
        }
        let snap_gravity = req.snap_gravity.filter(|value| *value);
        let reset_pose = req.reset_pose.filter(|value| *value);

        Ok(Self {
            fusion,
            range,
            update_interval_ms,
            dr_velocity_damp_tau_seconds,
            dr_still_velocity_zero_tau_seconds,
            dr_max_accel_world_mps2,
            dr_max_speed_mps,
            dr_max_position_m,
            dr_lock_position,
            gravity_reference_axis,
            snap_gravity,
            reset_pose,
        })
    }

    fn is_empty(&self) -> bool {
        self.fusion.is_none()
            && self.range.is_none()
            && self.update_interval_ms.is_none()
            && self.dr_velocity_damp_tau_seconds.is_none()
            && self.dr_still_velocity_zero_tau_seconds.is_none()
            && self.dr_max_accel_world_mps2.is_none()
            && self.dr_max_speed_mps.is_none()
            && self.dr_max_position_m.is_none()
            && self.dr_lock_position.is_none()
            && self.gravity_reference_axis.is_none()
            && self.snap_gravity.is_none()
            && self.reset_pose.is_none()
    }

    fn into_json(self) -> JsonValue {
        let mut map = JsonMap::new();
        if let Some(fusion) = self.fusion {
            map.insert("fusion".into(), json!(fusion));
        }
        if let Some(range) = self.range {
            map.insert("range".into(), json!(range));
        }
        if let Some(ms) = self.update_interval_ms {
            map.insert("update_interval_ms".into(), json!(ms));
        }
        if let Some(value) = self.dr_velocity_damp_tau_seconds {
            map.insert("dr_velocity_damp_tau_seconds".into(), json!(value));
        }
        if let Some(value) = self.dr_still_velocity_zero_tau_seconds {
            map.insert("dr_still_velocity_zero_tau_seconds".into(), json!(value));
        }
        if let Some(value) = self.dr_max_accel_world_mps2 {
            map.insert("dr_max_accel_world_mps2".into(), json!(value));
        }
        if let Some(value) = self.dr_max_speed_mps {
            map.insert("dr_max_speed_mps".into(), json!(value));
        }
        if let Some(value) = self.dr_max_position_m {
            map.insert("dr_max_position_m".into(), json!(value));
        }
        if let Some(value) = self.dr_lock_position {
            map.insert("dr_lock_position".into(), json!(value));
        }
        if let Some(value) = self.gravity_reference_axis {
            map.insert("gravity_reference_axis".into(), json!(value));
        }
        if let Some(value) = self.snap_gravity {
            map.insert("snap_gravity".into(), json!(value));
        }
        if let Some(value) = self.reset_pose {
            map.insert("reset_pose".into(), json!(value));
        }
        JsonValue::Object(map)
    }
}

fn trimmed_string(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

fn parse_positive_f64(value: Option<f64>, field: &str) -> Result<Option<f64>, Box<ApiError>> {
    match value {
        Some(v) if !v.is_finite() || v <= 0.0 => Err(Box::new(ApiError::bad_request(format!("{field} must be a finite number greater than zero")))),
        Some(v) => Ok(Some(v)),
        None => Ok(None),
    }
}

fn is_valid_axis_reference(raw: &str) -> bool {
    matches!(raw.trim().to_ascii_lowercase().as_str(), "x" | "+x" | "x+" | "-x" | "x-" | "y" | "+y" | "y+" | "-y" | "y-" | "z" | "+z" | "z+" | "-z" | "z-")
}

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

fn as_bool(value: &JsonValue) -> Option<bool> {
    match value {
        JsonValue::Bool(v) => Some(*v),
        _ => None,
    }
}
