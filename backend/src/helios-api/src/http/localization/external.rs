use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post, put},
};

use super::super::AppState;
use super::super::error::{ApiError, ApiResult};
use super::media_imu;
use helios_engine::localization::external::{ExternalLocalizationSample, ExternalLocalizationSampleRequest, ExternalLocalizationSource, ExternalLocalizationSourceUpsert};
use helios_engine::localization::types::PipelineOutputSample;

const IMU_EXTERNAL_ID: &str = "imu";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/external/sources", get(list_external_sources))
        .route("/external/sources/{id}", put(upsert_external_source).delete(delete_external_source))
        .route("/external/sources/{id}/sample", post(update_external_sample))
        .route("/external/{id}/outputs/{output_key}", get(sample_external_output))
}

#[utoipa::path(
    get,
    path = "/localization/external/sources",
    tag = "Localization",
    responses((status = 200, description = "External localization sources", body = [ExternalLocalizationSource]))
)]
pub async fn list_external_sources(State(_state): State<AppState>) -> ApiResult<Json<Vec<ExternalLocalizationSource>>> {
    let sources = helios_engine::localization::external::list_sources().await;
    Ok(Json(sources))
}

#[utoipa::path(
    put,
    path = "/localization/external/sources/{id}",
    tag = "Localization",
    params(("id" = String, Path, description = "External source id")),
    request_body = ExternalLocalizationSourceUpsert,
    responses((status = 200, description = "Upserted external source", body = ExternalLocalizationSource))
)]
pub async fn upsert_external_source(State(_state): State<AppState>, Path(id): Path<String>, Json(payload): Json<ExternalLocalizationSourceUpsert>) -> ApiResult<Json<ExternalLocalizationSource>> {
    let source = helios_engine::localization::external::upsert_source(&id, payload).await.map_err(ApiError::bad_request)?;
    Ok(Json(source))
}

#[utoipa::path(
    delete,
    path = "/localization/external/sources/{id}",
    tag = "Localization",
    params(("id" = String, Path, description = "External source id")),
    responses((status = 204, description = "External source removed"))
)]
pub async fn delete_external_source(State(_state): State<AppState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    helios_engine::localization::external::delete_source(&id).await;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(
    post,
    path = "/localization/external/sources/{id}/sample",
    tag = "Localization",
    params(("id" = String, Path, description = "External source id")),
    request_body = ExternalLocalizationSampleRequest,
    responses((status = 200, description = "Updated sample", body = ExternalLocalizationSample))
)]
pub async fn update_external_sample(State(_state): State<AppState>, Path(id): Path<String>, Json(payload): Json<ExternalLocalizationSampleRequest>) -> ApiResult<Json<ExternalLocalizationSample>> {
    let sample = helios_engine::localization::external::update_sample(&id, payload).await.map_err(ApiError::bad_request)?;
    Ok(Json(sample))
}

#[utoipa::path(
    get,
    path = "/localization/external/{id}/outputs/{output_key}",
    tag = "Localization",
    params(("id" = String, Path, description = "External source id"), ("output_key" = String, Path, description = "Output key")),
    responses(
        (status = 200, description = "Latest output sample", body = PipelineOutputSample),
        (status = 404, description = "No sample available", body = super::super::error::ErrorBody),
        (status = 400, description = "Unsupported output", body = super::super::error::ErrorBody)
    )
)]
pub async fn sample_external_output(State(state): State<AppState>, Path((id, output_key)): Path<(String, String)>) -> ApiResult<Json<PipelineOutputSample>> {
    let sample = fetch_external_sample(&state, &id, &output_key).await?;
    Ok(Json(sample))
}

pub(crate) async fn list_external_sources_snapshot() -> Vec<ExternalLocalizationSource> {
    helios_engine::localization::external::list_sources().await
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

    let status = super::super::device::imu::imu_status_from_snapshot(&snapshot);
    let Some(orientation) = status.orientation else {
        return Err(ApiError::not_found("IMU orientation unavailable"));
    };

    let rotation = if let Some(quat) = orientation.quaternion {
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
    };
    // IMU position is usually clamped/zeroed (`dr_lock_position=true`) and should not be treated
    // as an absolute robot-field translation source for localization.
    // Only publish translation when dead-reckoned position is explicitly unlocked *and* non-zero.
    let translation = status.position_world.filter(|position| status.dr_lock_position == Some(false) && (position.x.abs() > 1e-6 || position.y.abs() > 1e-6 || position.z.abs() > 1e-6));
    let velocity = status.velocity_world.map(|value| {
        serde_json::json!({
            "x": value.x,
            "y": value.y,
            "z": value.z,
        })
    });
    let velocity_delta = status.velocity_delta_world.map(|value| {
        serde_json::json!({
            "x": value.x,
            "y": value.y,
            "z": value.z,
        })
    });
    let angular_velocity = status.angular_velocity_dps.map(|value| {
        serde_json::json!({
            "x": value.x,
            "y": value.y,
            "z": value.z,
        })
    });
    let gyro_bias = status.gyro_bias_dps.map(|value| {
        serde_json::json!({
            "x": value.x,
            "y": value.y,
            "z": value.z,
        })
    });
    let corrected_world_accel = status.corrected_world_accel_mps2.map(|value| {
        serde_json::json!({
            "x": value.x,
            "y": value.y,
            "z": value.z,
        })
    });

    let mut payload = serde_json::Map::new();
    payload.insert("rotation".to_string(), rotation);
    if let Some(position) = translation {
        payload.insert(
            "translation".to_string(),
            serde_json::json!({
                "x": position.x,
                "y": position.y,
                "z": position.z,
            }),
        );
    }
    if let Some(velocity) = velocity {
        payload.insert("velocity_world".to_string(), velocity);
    }
    if let Some(velocity_delta) = velocity_delta {
        payload.insert("velocity_delta_world".to_string(), velocity_delta);
    }
    if let Some(angular_velocity) = angular_velocity {
        payload.insert("angular_velocity_dps".to_string(), angular_velocity);
    }
    if let Some(gyro_bias) = gyro_bias {
        payload.insert("gyro_bias_dps".to_string(), gyro_bias);
    }
    if let Some(corrected_world_accel) = corrected_world_accel {
        payload.insert("corrected_world_accel_mps2".to_string(), corrected_world_accel);
    }
    if let Some(value) = status.linear_speed_mps {
        payload.insert("linear_speed_mps".to_string(), serde_json::json!(value));
    }
    if let Some(value) = status.linear_speed_normalized {
        payload.insert("linear_speed_normalized".to_string(), serde_json::json!(value));
    }
    if let Some(value) = status.angular_speed_dps {
        payload.insert("angular_speed_dps".to_string(), serde_json::json!(value));
    }
    if let Some(value) = status.angular_speed_normalized {
        payload.insert("angular_speed_normalized".to_string(), serde_json::json!(value));
    }
    if let Some(value) = status.stillness_confidence {
        payload.insert("stillness_confidence".to_string(), serde_json::json!(value));
    }
    if let Some(value) = status.is_still {
        payload.insert("is_still".to_string(), serde_json::json!(value));
    }
    if let Some(value) = status.rotation_contaminated {
        payload.insert("rotation_contaminated".to_string(), serde_json::json!(value));
    }
    if let Some(dr_confidence) = status.dr_confidence {
        payload.insert("dr_confidence".to_string(), serde_json::json!(dr_confidence));
    }
    if let Some(motion_fast_g) = status.motion_fast_g {
        payload.insert("motion_fast_g".to_string(), serde_json::json!(motion_fast_g));
    }
    if let Some(is_moving_fast) = status.is_moving_fast {
        payload.insert("is_moving_fast".to_string(), serde_json::json!(is_moving_fast));
    }

    Ok(PipelineOutputSample { data_type: None, value: serde_json::Value::Object(payload) })
}
