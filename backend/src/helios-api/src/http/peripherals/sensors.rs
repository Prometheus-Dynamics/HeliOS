use std::path::Path;

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use serde_json::{Map, Value};
use tracing::error;

use crate::http::AppState;

use super::invalidate_peripheral_inventory_cache;
use super::support::{sensor_ipc_timeout, usb_warnings_from_path};
use super::types::{
    ConfigureSensorAliasRequest, ConfigureSensorFirmwareRequest, ErrorBody, LightingStatus, SensorPeripheral, SensorPeripheralFirmwareOption, SensorPeripheralFirmwareStatus, SensorPeripheralWarning,
};

#[utoipa::path(
    get,
    path = "/peripherals/sensors",
    tag = "Peripherals",
    responses(
        (status = 200, description = "Sensor peripherals inventory", body = [SensorPeripheral]),
        (status = 503, description = "Peripherals IPC unavailable", body = ErrorBody)
    )
)]
pub(crate) async fn list_sensors(State(state): State<AppState>) -> impl IntoResponse {
    let Some(sensors) = state.ensure_sensors().await else {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(ErrorBody::new("service_unavailable", "peripherals IPC unavailable"))).into_response();
    };

    match tokio::time::timeout(sensor_ipc_timeout(), sensors.inventory()).await {
        Ok(Ok(Ok(inv))) => Json(map_sensor_inventory(&inv)).into_response(),
        Ok(Ok(Err(reason))) => (StatusCode::BAD_GATEWAY, Json(ErrorBody::new("bad_gateway", reason))).into_response(),
        Ok(Err(err)) => {
            error!(%err, "failed to fetch sensor inventory");
            state.invalidate_sensors().await;
            if let Some(sensors) = state.ensure_sensors().await {
                match tokio::time::timeout(sensor_ipc_timeout(), sensors.inventory()).await {
                    Ok(Ok(Ok(inv))) => return Json(map_sensor_inventory(&inv)).into_response(),
                    Ok(Ok(Err(reason))) => {
                        return (StatusCode::BAD_GATEWAY, Json(ErrorBody::new("bad_gateway", reason))).into_response();
                    }
                    Ok(Err(err)) => error!(%err, "failed to fetch sensor inventory after reconnect"),
                    Err(_) => error!("sensor inventory request timed out after reconnect"),
                }
            }
            (StatusCode::BAD_GATEWAY, Json(ErrorBody::new("bad_gateway", "failed to fetch sensor inventory"))).into_response()
        }
        Err(_) => {
            state.invalidate_sensors().await;
            (StatusCode::GATEWAY_TIMEOUT, Json(ErrorBody::new("timeout", "sensor inventory request timed out"))).into_response()
        }
    }
}

#[utoipa::path(
    post,
    path = "/peripherals/sensors/firmware",
    tag = "Peripherals",
    request_body = ConfigureSensorFirmwareRequest,
    responses(
        (status = 204, description = "Firmware configuration accepted"),
        (status = 400, description = "Bad request", body = ErrorBody),
        (status = 503, description = "Peripherals IPC unavailable", body = ErrorBody)
    )
)]
pub(crate) async fn configure_sensor_firmware(State(state): State<AppState>, Json(req): Json<ConfigureSensorFirmwareRequest>) -> impl IntoResponse {
    let Some(sensors) = state.ensure_sensors().await else {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(ErrorBody::new("service_unavailable", "peripherals IPC unavailable"))).into_response();
    };

    let device_id = req.device_id.trim();
    let firmware = req.firmware.trim();
    if device_id.is_empty() || firmware.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(ErrorBody::new("bad_request", "device_id and firmware are required"))).into_response();
    }

    match sensors.configure_firmware(device_id.to_string(), firmware.to_string()).await {
        Ok(Ok(())) => {
            invalidate_peripheral_inventory_cache(&state).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(Err(reason)) => {
            let normalized = reason.to_lowercase();
            if normalized.contains("timed out") || normalized.contains("timeout") {
                (StatusCode::GATEWAY_TIMEOUT, Json(ErrorBody::new("timeout", reason))).into_response()
            } else {
                (StatusCode::BAD_REQUEST, Json(ErrorBody::new("bad_request", reason))).into_response()
            }
        }
        Err(err) => {
            error!(%err, "failed to send configure firmware command");
            (StatusCode::BAD_GATEWAY, Json(ErrorBody::new("bad_gateway", "failed to configure firmware"))).into_response()
        }
    }
}

#[utoipa::path(
    post,
    path = "/peripherals/sensors/alias",
    tag = "Peripherals",
    request_body = ConfigureSensorAliasRequest,
    responses(
        (status = 204, description = "Alias configuration accepted"),
        (status = 400, description = "Bad request", body = ErrorBody),
        (status = 503, description = "Peripherals IPC unavailable", body = ErrorBody)
    )
)]
pub(crate) async fn configure_sensor_alias(State(state): State<AppState>, Json(req): Json<ConfigureSensorAliasRequest>) -> impl IntoResponse {
    let Some(sensors) = state.ensure_sensors().await else {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(ErrorBody::new("service_unavailable", "peripherals IPC unavailable"))).into_response();
    };

    let hardware_key = req.hardware_key.trim();
    if hardware_key.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(ErrorBody::new("bad_request", "hardware_key is required"))).into_response();
    }

    match sensors.configure_alias(hardware_key.to_string(), req.alias).await {
        Ok(Ok(())) => {
            invalidate_peripheral_inventory_cache(&state).await;
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(Err(reason)) => (StatusCode::BAD_REQUEST, Json(ErrorBody::new("bad_request", reason))).into_response(),
        Err(err) => {
            error!(%err, "failed to send configure alias command");
            (StatusCode::BAD_GATEWAY, Json(ErrorBody::new("bad_gateway", "failed to configure alias"))).into_response()
        }
    }
}

pub(crate) fn derive_lighting_status(inv: &helios_peripherals::dto::SensorInventory) -> LightingStatus {
    let present = inv.sensors.iter().any(|sensor| sensor.metadata.as_ref().map(|metadata| metadata.json.to_lowercase().contains("lighting")).unwrap_or(false));
    LightingStatus { present, last_error: None }
}

pub(crate) fn map_sensor_inventory(inv: &helios_peripherals::dto::SensorInventory) -> Vec<SensorPeripheral> {
    inv.sensors.iter().filter_map(map_sensor_descriptor).collect()
}

pub(super) fn inventory_has_lighting(inv: &helios_peripherals::dto::SensorInventory) -> bool {
    inv.sensors.iter().any(|sensor| {
        let backend = sensor.backend.to_ascii_lowercase();
        backend.contains("led") || backend.contains("lighting")
    })
}

fn map_sensor_descriptor(desc: &helios_peripherals::dto::SensorDescriptor) -> Option<SensorPeripheral> {
    if !desc.present {
        return None;
    }

    let info = desc
        .info
        .as_ref()
        .and_then(|info| info.to_value().ok())
        .and_then(|value| match value {
            Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default();
    let metadata = desc
        .metadata
        .as_ref()
        .and_then(|meta| meta.to_value().ok())
        .and_then(|value| match value {
            Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default();

    let alias = meta_string(&metadata, "alias");
    let product = meta_string(&metadata, "product");
    let name = alias.clone().or(product.clone()).unwrap_or_else(|| format!("{} sensor", desc.backend));

    let firmware = map_firmware_status(&metadata);
    let status = meta_string(&metadata, "status").or_else(|| meta_string(&metadata, "firmware_status")).or_else(|| firmware.as_ref().and_then(|fw| fw.status.clone()));
    let interval = None;
    let r#type = if desc.backend == "coral" { Some("Accelerator".into()) } else { product.clone() };

    let warnings = sensor_warnings(&info);
    let telemetry = metadata.get("telemetry").cloned();

    Some(SensorPeripheral {
        name,
        driver_namespace: desc.backend.clone(),
        driver_camera_id: desc.identifier.clone(),
        present: true,
        status,
        interval,
        r#type,
        hardware_id: meta_string(&metadata, "hardware_id"),
        hardware_key: meta_string(&metadata, "hardware_key"),
        alias_identity: meta_string(&metadata, "alias_identity"),
        alias,
        firmware,
        warnings,
        telemetry,
    })
}

fn meta_string(meta: &Map<String, Value>, key: &str) -> Option<String> {
    match meta.get(key) {
        Some(Value::String(value)) => {
            let trimmed = value.trim();
            if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
        }
        _ => None,
    }
}

fn sensor_warnings(info: &Map<String, Value>) -> Vec<SensorPeripheralWarning> {
    let Some(path) = meta_string(info, "path") else {
        return Vec::new();
    };
    let warnings = usb_warnings_from_path(Path::new(&path));
    warnings.into_iter().map(|warning| SensorPeripheralWarning { code: warning.code, message: warning.message }).collect()
}

fn map_firmware_status(meta: &Map<String, Value>) -> Option<SensorPeripheralFirmwareStatus> {
    let options = match meta.get("firmware_options") {
        Some(Value::Array(entries)) => entries
            .iter()
            .filter_map(|entry| match entry {
                Value::Object(map) => {
                    let name = meta_string(map, "name")?;
                    let variant = meta_string(map, "variant").unwrap_or_else(|| name.to_lowercase());
                    let path = meta_string(map, "path");
                    Some(SensorPeripheralFirmwareOption { name, variant, path })
                }
                _ => None,
            })
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };

    let status = meta_string(meta, "firmware_status").or_else(|| meta_string(meta, "status"));
    let mode = meta_string(meta, "mode");
    let active = meta_string(meta, "firmware_active");
    let desired = meta_string(meta, "firmware_desired");
    let last_error = meta_string(meta, "firmware_error").or_else(|| meta_string(meta, "last_error"));

    if status.is_none() && mode.is_none() && active.is_none() && desired.is_none() && last_error.is_none() && options.is_empty() {
        return None;
    }

    Some(SensorPeripheralFirmwareStatus { status, mode, active, desired, last_error, options })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use helios_peripherals::dto::{JsonData, SensorDescriptor, SensorInventory};

    use super::{derive_lighting_status, inventory_has_lighting, map_firmware_status, map_sensor_inventory, meta_string};

    fn json_data(value: serde_json::Value) -> JsonData {
        JsonData::from_value(&value)
    }

    #[test]
    fn meta_string_trims_and_rejects_empty_values() {
        let meta = serde_json::Map::from_iter([("alias".into(), json!("  front-left  ")), ("empty".into(), json!("   "))]);

        assert_eq!(meta_string(&meta, "alias").as_deref(), Some("front-left"));
        assert!(meta_string(&meta, "empty").is_none());
        assert!(meta_string(&meta, "missing").is_none());
    }

    #[test]
    fn map_firmware_status_collects_options_and_status() {
        let meta = serde_json::Map::from_iter([
            ("firmware_status".into(), json!("active")),
            ("firmware_active".into(), json!("stable")),
            ("firmware_desired".into(), json!("beta")),
            (
                "firmware_options".into(),
                json!([
                    {"name": "Stable", "variant": "stable", "path": "/tmp/stable.bin"},
                    {"name": "Beta"}
                ]),
            ),
        ]);

        let status = map_firmware_status(&meta).expect("firmware status");
        assert_eq!(status.status.as_deref(), Some("active"));
        assert_eq!(status.active.as_deref(), Some("stable"));
        assert_eq!(status.desired.as_deref(), Some("beta"));
        assert_eq!(status.options.len(), 2);
        assert_eq!(status.options[0].variant, "stable");
        assert_eq!(status.options[1].variant, "beta");
    }

    #[test]
    fn map_sensor_inventory_skips_missing_descriptors_and_keeps_aliases() {
        let inventory = SensorInventory {
            sensors: vec![
                SensorDescriptor { backend: "imu".into(), identifier: "imu-0".into(), present: false, info: None, metadata: None, stream_id: None, value: None },
                SensorDescriptor {
                    backend: "coral".into(),
                    identifier: "coral-0".into(),
                    present: true,
                    info: Some(json_data(json!({}))),
                    metadata: Some(json_data(json!({
                        "alias": "Front Coral",
                        "product": "Edge TPU",
                        "hardware_key": "usb:1",
                        "firmware_status": "ready"
                    }))),
                    stream_id: None,
                    value: None,
                },
            ],
        };

        let mapped = map_sensor_inventory(&inventory);
        assert_eq!(mapped.len(), 1);
        assert_eq!(mapped[0].name, "Front Coral");
        assert_eq!(mapped[0].driver_namespace, "coral");
        assert_eq!(mapped[0].hardware_key.as_deref(), Some("usb:1"));
        assert_eq!(mapped[0].status.as_deref(), Some("ready"));
    }

    #[test]
    fn lighting_helpers_detect_metadata_and_backend_signals() {
        let inventory = SensorInventory {
            sensors: vec![
                SensorDescriptor {
                    backend: "led-strip".into(),
                    identifier: "led-0".into(),
                    present: true,
                    info: None,
                    metadata: Some(json_data(json!({"product": "Lighting Controller"}))),
                    stream_id: None,
                    value: None,
                },
                SensorDescriptor {
                    backend: "imu".into(),
                    identifier: "imu-0".into(),
                    present: true,
                    info: None,
                    metadata: Some(json_data(json!({"notes": "lighting ready"}))),
                    stream_id: None,
                    value: None,
                },
            ],
        };

        assert!(inventory_has_lighting(&inventory));
        assert!(derive_lighting_status(&inventory).present);
    }
}
