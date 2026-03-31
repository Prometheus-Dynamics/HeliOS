use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use tracing::{error, warn};

use crate::http::AppState;
use crate::http::revision::{apply_revision_headers, matches_if_none_match, not_modified_response};

use super::sensors::{derive_lighting_status, inventory_has_lighting};
use super::support::{allow_inventory_refresh, cached_discover_cameras_snapshot, list_usb_sysfs, map_fan_status, refresh_inventory, sensor_ipc_timeout};
use super::types::{CameraDiscoveryResponse, ErrorBody, FanStatus, LightingStatus, PeripheralInventory, UsbPeripheral};

async fn load_peripheral_inventory_snapshot(state: &AppState) -> Result<(PeripheralInventory, u64), String> {
    state.services.hardware.load_peripheral_inventory_snapshot(state).await
}

#[utoipa::path(
    get,
    path = "/peripherals",
    tag = "Peripherals",
    responses(
        (status = 200, description = "Peripherals and cameras", body = PeripheralInventory),
        (status = 502, description = "Peripheral error", body = ErrorBody)
    )
)]
pub(crate) async fn list_peripherals(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    match load_peripheral_inventory_snapshot(&state).await {
        Ok((resp, revision)) => {
            if matches_if_none_match(&headers, revision) {
                return not_modified_response(revision);
            }
            let mut response = Json(resp).into_response();
            apply_revision_headers(response.headers_mut(), revision);
            response
        }
        Err(err) => {
            warn!("camera discovery failed: {err}");
            (StatusCode::BAD_GATEWAY, Json(ErrorBody::new("bad_gateway", err))).into_response()
        }
    }
}

#[utoipa::path(
    get,
    path = "/peripherals/cameras",
    tag = "Peripherals",
    responses(
        (status = 200, description = "Discovered cameras", body = CameraDiscoveryResponse),
        (status = 502, description = "Camera discovery failed", body = ErrorBody)
    )
)]
pub(crate) async fn list_cameras(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    match cached_discover_cameras_snapshot(&state).await {
        Ok((cameras, revision)) => {
            if matches_if_none_match(&headers, revision) {
                return not_modified_response(revision);
            }
            let mut response = Json(CameraDiscoveryResponse { cameras: cameras.devices, errors: cameras.errors }).into_response();
            apply_revision_headers(response.headers_mut(), revision);
            response
        }
        Err(err) => (StatusCode::BAD_GATEWAY, Json(ErrorBody::new("bad_gateway", err))).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/peripherals/i2c",
    tag = "Peripherals",
    responses(
        (status = 200, description = "I2C inventory", body = helios_peripherals::dto::I2cInventory),
        (status = 400, description = "Bad request", body = ErrorBody),
        (status = 503, description = "Peripherals IPC unavailable", body = ErrorBody)
    )
)]
pub(crate) async fn list_i2c(State(state): State<AppState>) -> impl IntoResponse {
    let Some(peripherals) = state.ensure_sensors().await else {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(ErrorBody::new("service_unavailable", "peripherals IPC unavailable"))).into_response();
    };

    match tokio::time::timeout(sensor_ipc_timeout(), peripherals.i2c_inventory()).await {
        Ok(Ok(Ok(inv))) => Json(inv).into_response(),
        Ok(Ok(Err(reason))) => (StatusCode::BAD_REQUEST, Json(ErrorBody::new("bad_request", reason))).into_response(),
        Ok(Err(err)) => {
            error!(%err, "failed to send i2c inventory command");
            state.invalidate_sensors().await;
            if let Some(peripherals) = state.ensure_sensors().await {
                match tokio::time::timeout(sensor_ipc_timeout(), peripherals.i2c_inventory()).await {
                    Ok(Ok(Ok(inv))) => return Json(inv).into_response(),
                    Ok(Ok(Err(reason))) => {
                        return (StatusCode::BAD_REQUEST, Json(ErrorBody::new("bad_request", reason))).into_response();
                    }
                    Ok(Err(err)) => error!(%err, "failed to send i2c inventory command after reconnect"),
                    Err(_) => error!("i2c inventory command timed out after reconnect"),
                }
            }
            (StatusCode::BAD_GATEWAY, Json(ErrorBody::new("bad_gateway", "failed to fetch i2c inventory"))).into_response()
        }
        Err(_) => {
            state.invalidate_sensors().await;
            (StatusCode::GATEWAY_TIMEOUT, Json(ErrorBody::new("timeout", "i2c inventory request timed out"))).into_response()
        }
    }
}

#[utoipa::path(
    post,
    path = "/peripherals/i2c/scan",
    tag = "Peripherals",
    responses(
        (status = 200, description = "I2C inventory after scan", body = helios_peripherals::dto::I2cInventory),
        (status = 400, description = "Bad request", body = ErrorBody),
        (status = 503, description = "Peripherals IPC unavailable", body = ErrorBody)
    )
)]
pub(crate) async fn scan_i2c(State(state): State<AppState>) -> impl IntoResponse {
    list_i2c(State(state)).await
}

#[utoipa::path(
    get,
    path = "/peripherals/usb",
    tag = "Peripherals",
    responses((status = 200, description = "USB peripherals", body = [UsbPeripheral]))
)]
pub(crate) async fn list_usb(State(state): State<AppState>) -> impl IntoResponse {
    let _ = state.ensure_sensors().await;
    Json(list_usb_sysfs())
}

#[utoipa::path(
    get,
    path = "/peripherals/fan",
    tag = "Peripherals",
    responses(
        (status = 200, description = "Fan status", body = FanStatus),
        (status = 503, description = "Peripherals IPC unavailable", body = ErrorBody)
    )
)]
pub(crate) async fn fan_status(State(state): State<AppState>) -> impl IntoResponse {
    let Some(sensors) = state.ensure_sensors().await else {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(ErrorBody::new("service_unavailable", "peripherals IPC unavailable"))).into_response();
    };

    match tokio::time::timeout(sensor_ipc_timeout(), sensors.fan_status()).await {
        Ok(Ok(Ok(status))) => Json(map_fan_status(status)).into_response(),
        Ok(Ok(Err(reason))) => (StatusCode::BAD_GATEWAY, Json(ErrorBody::new("bad_gateway", reason))).into_response(),
        Ok(Err(err)) => {
            error!(%err, "failed to fetch fan status");
            state.invalidate_sensors().await;
            if let Some(sensors) = state.ensure_sensors().await {
                match tokio::time::timeout(sensor_ipc_timeout(), sensors.fan_status()).await {
                    Ok(Ok(Ok(status))) => return Json(map_fan_status(status)).into_response(),
                    Ok(Ok(Err(reason))) => {
                        return (StatusCode::BAD_GATEWAY, Json(ErrorBody::new("bad_gateway", reason))).into_response();
                    }
                    Ok(Err(err)) => error!(%err, "failed to fetch fan status after reconnect"),
                    Err(_) => error!("fan status request timed out after reconnect"),
                }
            }
            (StatusCode::BAD_GATEWAY, Json(ErrorBody::new("bad_gateway", "failed to fetch fan status"))).into_response()
        }
        Err(_) => {
            state.invalidate_sensors().await;
            (StatusCode::GATEWAY_TIMEOUT, Json(ErrorBody::new("timeout", "fan status request timed out"))).into_response()
        }
    }
}

#[utoipa::path(
    get,
    path = "/peripherals/leds",
    tag = "Peripherals",
    responses((status = 200, description = "Lighting presence", body = LightingStatus))
)]
pub(crate) async fn led_status(State(state): State<AppState>) -> impl IntoResponse {
    let mut status = LightingStatus { present: false, last_error: None };
    let Some(sensors) = state.ensure_sensors().await else {
        return Json(status);
    };

    let mut inventory = match tokio::time::timeout(sensor_ipc_timeout(), sensors.inventory()).await {
        Ok(Ok(Ok(inv))) => Some(inv),
        Ok(Ok(Err(reason))) => {
            warn!(%reason, "sensor inventory request rejected");
            None
        }
        Ok(Err(err)) => {
            warn!(%err, "sensor inventory request failed");
            None
        }
        Err(_) => {
            state.invalidate_sensors().await;
            warn!("sensor inventory request timed out");
            None
        }
    };

    if inventory.as_ref().is_none_or(|inv| !inventory_has_lighting(inv)) && allow_inventory_refresh(&state).await {
        if let Some(refreshed) = refresh_inventory(&state, sensors.clone()).await {
            inventory = Some(refreshed);
        }
    }

    if let Some(inv) = inventory {
        status = derive_lighting_status(&inv);
    }
    Json(status)
}
