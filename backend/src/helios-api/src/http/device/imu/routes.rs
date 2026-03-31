use axum::{Json, extract::State, response::IntoResponse};
use helios_peripherals::dto::{I2cInventory, JsonData, SensorKind, SensorScope};
use lib_sensors::dto::{ImuStatusPayload, ImuUpdateRequest};
use tracing::{error, warn};

use crate::http::{
    AppState,
    error::{ApiError, ApiResult},
};

use super::{
    status::{imu_status_from_snapshot, imu_status_from_snapshot_typed},
    update::ImuUpdate,
};

#[utoipa::path(
    get,
    path = "/device/i2c",
    tag = "Device",
    responses(
        (status = 200, description = "I2C inventory", body = I2cInventory),
        (status = 400, description = "Bad request", body = crate::http::error::ErrorBody),
        (status = 503, description = "Peripherals IPC unavailable", body = crate::http::error::ErrorBody)
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
        (status = 400, description = "Bad request", body = crate::http::error::ErrorBody),
        (status = 503, description = "Peripherals IPC unavailable", body = crate::http::error::ErrorBody)
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
        (status = 400, description = "Invalid request", body = crate::http::error::ErrorBody),
        (status = 503, description = "Peripherals IPC unavailable", body = crate::http::error::ErrorBody)
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
