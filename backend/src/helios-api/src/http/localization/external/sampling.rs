use axum::{
    Json,
    extract::{Path, State},
};
use chrono::DateTime;
use serde_json::{Map as JsonMap, Value as JsonValue};

use super::super::super::AppState;
use super::super::super::error::{ApiError, ApiResult};
use super::super::media_imu;
use super::IMU_EXTERNAL_ID;
use helios_engine::localization::types::PipelineOutputSample;
use lib_sensors::dto::{ImuAxesPayload, ImuOrientationPayload, ImuStatusPayload};

#[utoipa::path(
    get,
    path = "/localization/external/{id}/outputs/{output_key}",
    tag = "Localization",
    params(("id" = String, Path, description = "External source id"), ("output_key" = String, Path, description = "Output key")),
    responses(
        (status = 200, description = "Latest output sample", body = PipelineOutputSample),
        (status = 404, description = "No sample available", body = super::super::super::error::ErrorBody),
        (status = 400, description = "Unsupported output", body = super::super::super::error::ErrorBody)
    )
)]
pub(crate) async fn sample_external_output(State(state): State<AppState>, Path((id, output_key)): Path<(String, String)>) -> ApiResult<Json<PipelineOutputSample>> {
    let sample = fetch_external_sample(&state, &id, &output_key).await?;
    Ok(Json(sample))
}

pub(crate) async fn fetch_external_value(state: &AppState, source_id: &str, output_key: &str) -> Result<serde_json::Value, String> {
    let sample = fetch_external_sample(state, source_id, output_key).await.map_err(|err| err.to_string())?;
    Ok(sample.value)
}

pub(crate) async fn fetch_external_sample(state: &AppState, source_id: &str, output_key: &str) -> ApiResult<PipelineOutputSample> {
    match helios_engine::localization::external::fetch_sample(source_id, output_key).await {
        Ok(sample) => Ok(sample),
        Err(err) if source_id == IMU_EXTERNAL_ID && err == "external source not found" => fetch_device_imu_sample(state).await,
        Err(err) if err == "external source not found" && media_imu::is_media_imu_source_id(source_id) => media_imu::fetch_media_imu_sample(state, source_id, output_key).await,
        Err(err) if err == "external source not found" => Err(ApiError::not_found("external source not found")),
        Err(err) if err == "no sample available" => Err(ApiError::not_found("no sample available")),
        Err(err) if err == "unsupported output key" => Err(ApiError::bad_request("unsupported output key")),
        Err(err) => Err(ApiError::bad_gateway(err)),
    }
}

async fn fetch_device_imu_sample(state: &AppState) -> ApiResult<PipelineOutputSample> {
    let Some(peripherals) = state.ensure_sensors().await else {
        return Err(ApiError::service_unavailable("peripherals IPC unavailable"));
    };

    let snapshot = peripherals
        .sensor_snapshot(helios_peripherals::dto::SensorScope::Device)
        .await
        .map_err(|err| ApiError::bad_gateway(format!("failed to fetch IMU snapshot: {err}")))?
        .map_err(ApiError::bad_request)?;

    let status = super::super::super::device::imu::imu_status_from_snapshot(&snapshot);
    build_device_imu_sample(&status).ok_or_else(|| ApiError::not_found("IMU orientation unavailable"))
}

pub(super) fn build_device_imu_sample(status: &ImuStatusPayload) -> Option<PipelineOutputSample> {
    let orientation = status.orientation.as_ref()?;
    let mut payload = JsonMap::new();
    payload.insert("rotation".to_string(), rotation_value(orientation));

    if let Some(position) = status.position_world.as_ref().filter(|position| status.dr_lock_position == Some(false) && has_nonzero_axes(position)) {
        payload.insert("translation".to_string(), axes_value(position));
    }
    insert_optional_axes(&mut payload, "velocity_world", status.velocity_world.as_ref());
    insert_optional_axes(&mut payload, "velocity_delta_world", status.velocity_delta_world.as_ref());
    insert_optional_axes(&mut payload, "angular_velocity_dps", status.angular_velocity_dps.as_ref());
    insert_optional_axes(&mut payload, "gyro_bias_dps", status.gyro_bias_dps.as_ref());
    insert_optional_axes(&mut payload, "corrected_world_accel_mps2", status.corrected_world_accel_mps2.as_ref());
    insert_optional_f64(&mut payload, "linear_speed_mps", status.linear_speed_mps);
    insert_optional_f64(&mut payload, "linear_speed_normalized", status.linear_speed_normalized);
    insert_optional_f64(&mut payload, "angular_speed_dps", status.angular_speed_dps);
    insert_optional_f64(&mut payload, "angular_speed_normalized", status.angular_speed_normalized);
    insert_optional_f64(&mut payload, "stillness_confidence", status.stillness_confidence);
    insert_optional_bool(&mut payload, "is_still", status.is_still);
    insert_optional_bool(&mut payload, "rotation_contaminated", status.rotation_contaminated);
    insert_optional_f64(&mut payload, "dr_confidence", status.dr_confidence);
    insert_optional_f64(&mut payload, "motion_fast_g", status.motion_fast_g);
    insert_optional_bool(&mut payload, "is_moving_fast", status.is_moving_fast);

    if let Some(updated_at) = status.updated_at.as_deref()
        && let Ok(timestamp) = DateTime::parse_from_rfc3339(updated_at)
    {
        payload.insert("sampleTimestampMs".to_string(), serde_json::json!(timestamp.timestamp_millis().max(0) as u64));
    }

    Some(PipelineOutputSample { data_type: None, value: JsonValue::Object(payload) })
}

fn rotation_value(orientation: &ImuOrientationPayload) -> JsonValue {
    if let Some(quat) = orientation.quaternion.as_ref() {
        serde_json::json!({
            "roll": orientation.roll,
            "pitch": orientation.pitch,
            "yaw": orientation.yaw,
            "quaternion": {
                "x": quat.x,
                "y": quat.y,
                "z": quat.z,
                "w": quat.w,
            }
        })
    } else {
        serde_json::json!({
            "roll": orientation.roll,
            "pitch": orientation.pitch,
            "yaw": orientation.yaw,
        })
    }
}

fn axes_value(value: &ImuAxesPayload) -> JsonValue {
    serde_json::json!({
        "x": value.x,
        "y": value.y,
        "z": value.z,
    })
}

fn has_nonzero_axes(value: &ImuAxesPayload) -> bool {
    value.x.abs() > 1e-6 || value.y.abs() > 1e-6 || value.z.abs() > 1e-6
}

fn insert_optional_axes(payload: &mut JsonMap<String, JsonValue>, key: &str, value: Option<&ImuAxesPayload>) {
    if let Some(value) = value {
        payload.insert(key.to_string(), axes_value(value));
    }
}

fn insert_optional_bool(payload: &mut JsonMap<String, JsonValue>, key: &str, value: Option<bool>) {
    if let Some(value) = value {
        payload.insert(key.to_string(), serde_json::json!(value));
    }
}

fn insert_optional_f64(payload: &mut JsonMap<String, JsonValue>, key: &str, value: Option<f64>) {
    if let Some(value) = value {
        payload.insert(key.to_string(), serde_json::json!(value));
    }
}
