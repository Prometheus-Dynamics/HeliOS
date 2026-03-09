use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use once_cell::sync::Lazy;
use serde::Deserialize;
use serde::Serialize;
use std::fs;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};
use tokio::task;
use tracing::{error, warn};
use utoipa::ToSchema;

use super::AppState;
use crate::ipc::peripherals::SensorsConnection;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_peripherals))
        .route("/cameras", get(list_cameras))
        .route("/i2c", get(list_i2c))
        .route("/i2c/scan", post(scan_i2c))
        .route("/usb", get(list_usb))
        .route("/fan", get(fan_status))
        .route("/leds", get(led_status))
        .route("/sensors", get(list_sensors))
        .route("/sensors/firmware", post(configure_sensor_firmware))
        .route("/sensors/alias", post(configure_sensor_alias))
}

#[derive(Clone, Serialize, ToSchema)]
pub struct PeripheralErrors {
    #[serde(default)]
    cameras: Vec<String>,
    #[serde(default)]
    i2c: Vec<String>,
    #[serde(default)]
    usb: Vec<String>,
    #[serde(default)]
    fan: Vec<String>,
    #[serde(default)]
    lighting: Vec<String>,
}

#[derive(Clone, Serialize, ToSchema)]
pub struct PeripheralInventory {
    cameras: Vec<helios_engine::capture::DiscoveredDevice>,
    #[serde(default)]
    sensors: Vec<SensorPeripheral>,
    #[serde(default)]
    errors: PeripheralErrors,
    #[serde(default)]
    i2c: Option<helios_peripherals::dto::I2cInventory>,
    #[serde(default)]
    usb: Vec<UsbPeripheral>,
    #[serde(default)]
    lighting: Option<LightingStatus>,
    #[serde(default)]
    fan: Option<FanStatus>,
}

#[derive(Clone, Serialize, ToSchema)]
pub struct UsbPeripheral {
    id: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    present: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<UsbPeripheralWarning>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct UsbPeripheralWarning {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Serialize, ToSchema)]
pub struct LightingStatus {
    #[serde(default)]
    present: bool,
    #[serde(default)]
    last_error: Option<String>,
}

#[derive(Clone, Serialize, ToSchema)]
pub struct FanStatus {
    #[serde(default)]
    present: bool,
    #[serde(default)]
    rpm: Option<u32>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    target_percent: Option<u8>,
    #[serde(default)]
    last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SensorPeripheralFirmwareOption {
    pub name: String,
    pub variant: String,
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SensorPeripheralWarning {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SensorPeripheralFirmwareStatus {
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub active: Option<String>,
    #[serde(default)]
    pub desired: Option<String>,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub options: Vec<SensorPeripheralFirmwareOption>,
}

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SensorPeripheral {
    pub name: String,
    pub driver_namespace: String,
    pub driver_camera_id: String,
    #[serde(default)]
    pub present: bool,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub interval: Option<String>,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub hardware_id: Option<String>,
    #[serde(default)]
    pub hardware_key: Option<String>,
    #[serde(default)]
    pub alias_identity: Option<String>,
    #[serde(default)]
    pub alias: Option<String>,
    #[serde(default)]
    pub firmware: Option<SensorPeripheralFirmwareStatus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<SensorPeripheralWarning>,
    #[serde(default)]
    #[schema(value_type = Option<Object>)]
    pub telemetry: Option<serde_json::Value>,
}

#[derive(Serialize, ToSchema)]
pub struct ErrorBody {
    pub code: String,
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl ErrorBody {
    fn new(code: impl Into<String>, error: impl Into<String>) -> Self {
        Self { code: code.into(), error: error.into(), details: None }
    }
}

#[derive(Clone, Serialize, ToSchema)]
pub struct CameraDiscoveryResponse {
    cameras: Vec<helios_engine::capture::DiscoveredDevice>,
    #[serde(default)]
    errors: Vec<String>,
}

#[derive(Clone)]
struct PeripheralInventoryCacheEntry {
    fetched_at: Instant,
    payload: PeripheralInventory,
}

fn peripheral_inventory_cache() -> &'static RwLock<Option<PeripheralInventoryCacheEntry>> {
    static CACHE: OnceLock<RwLock<Option<PeripheralInventoryCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

fn peripheral_inventory_refresh_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn peripheral_inventory_cache_ttl() -> Duration {
    static VALUE: OnceLock<Duration> = OnceLock::new();
    *VALUE.get_or_init(|| read_timeout_env("HELIOS_PERIPHERALS_CACHE_MS", 1_000, 0, 10_000))
}

async fn invalidate_peripheral_inventory_cache() {
    *peripheral_inventory_cache().write().await = None;
}

async fn load_peripheral_inventory_cached(state: &AppState) -> Result<PeripheralInventory, String> {
    let ttl = peripheral_inventory_cache_ttl();
    if ttl != Duration::from_millis(0)
        && let Some(entry) = peripheral_inventory_cache().read().await.clone()
        && entry.fetched_at.elapsed() < ttl
    {
        return Ok(entry.payload);
    }

    let _refresh_guard = peripheral_inventory_refresh_lock().lock().await;
    if ttl != Duration::from_millis(0)
        && let Some(entry) = peripheral_inventory_cache().read().await.clone()
        && entry.fetched_at.elapsed() < ttl
    {
        return Ok(entry.payload);
    }

    let payload = build_peripheral_inventory(state).await?;
    *peripheral_inventory_cache().write().await = Some(PeripheralInventoryCacheEntry { fetched_at: Instant::now(), payload: payload.clone() });
    Ok(payload)
}

async fn build_peripheral_inventory(state: &AppState) -> Result<PeripheralInventory, String> {
    let mut errors = PeripheralErrors { cameras: Vec::new(), i2c: Vec::new(), usb: Vec::new(), fan: Vec::new(), lighting: Vec::new() };
    let cameras = cached_discover_cameras().await?;
    let usb = list_usb_sysfs();
    let (sensors, i2c, fan, lighting) = match state.ensure_sensors().await {
        Some(sensors) => {
            let (inventory_res, i2c_res, fan_res) = tokio::join!(
                tokio::time::timeout(sensor_ipc_timeout(), sensors.inventory()),
                tokio::time::timeout(sensor_ipc_timeout(), sensors.i2c_inventory()),
                tokio::time::timeout(sensor_ipc_timeout(), sensors.fan_status())
            );

            let mut inventory = match inventory_res {
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

            if inventory.as_ref().is_none_or(|inv| inventory_needs_refresh(inv, &usb))
                && allow_inventory_refresh().await
                && let Some(refreshed) = refresh_inventory(state, sensors.clone()).await
            {
                inventory = Some(refreshed);
            }

            let sensor_peripherals = inventory.as_ref().map(map_sensor_inventory).unwrap_or_default();
            let i2c_inv = match i2c_res {
                Ok(Ok(Ok(inv))) => Some(inv),
                Ok(Ok(Err(reason))) => {
                    errors.i2c.push(reason);
                    None
                }
                Ok(Err(err)) => {
                    errors.i2c.push(err.to_string());
                    None
                }
                Err(_) => {
                    state.invalidate_sensors().await;
                    errors.i2c.push("i2c inventory request timed out".into());
                    None
                }
            };
            let fan = match fan_res {
                Ok(Ok(Ok(status))) => Some(map_fan_status(status)),
                Ok(Ok(Err(reason))) => {
                    errors.fan.push(reason);
                    None
                }
                Ok(Err(err)) => {
                    errors.fan.push(err.to_string());
                    None
                }
                Err(_) => {
                    state.invalidate_sensors().await;
                    errors.fan.push("fan status request timed out".into());
                    None
                }
            };
            let lighting = inventory.as_ref().map(derive_lighting_status);
            (sensor_peripherals, i2c_inv, fan, lighting)
        }
        None => (Vec::new(), None, None, None),
    };

    errors.cameras = cameras.errors;
    Ok(PeripheralInventory { cameras: cameras.devices, sensors, errors, i2c, usb, lighting, fan })
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
async fn list_peripherals(State(state): State<AppState>) -> impl IntoResponse {
    match load_peripheral_inventory_cached(&state).await {
        Ok(resp) => Json(resp).into_response(),
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
async fn list_cameras() -> impl IntoResponse {
    match cached_discover_cameras().await {
        Ok(cameras) => Json(CameraDiscoveryResponse { cameras: cameras.devices, errors: cameras.errors }).into_response(),
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
async fn list_i2c(State(state): State<AppState>) -> impl IntoResponse {
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
                    Ok(Ok(Err(reason))) => return (StatusCode::BAD_REQUEST, Json(ErrorBody::new("bad_request", reason))).into_response(),
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
async fn scan_i2c(State(state): State<AppState>) -> impl IntoResponse {
    list_i2c(State(state)).await
}

#[utoipa::path(
    get,
    path = "/peripherals/usb",
    tag = "Peripherals",
    responses((status = 200, description = "USB peripherals", body = [UsbPeripheral]))
)]
async fn list_usb(State(state): State<AppState>) -> impl IntoResponse {
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
async fn fan_status(State(state): State<AppState>) -> impl IntoResponse {
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
                    Ok(Ok(Err(reason))) => return (StatusCode::BAD_GATEWAY, Json(ErrorBody::new("bad_gateway", reason))).into_response(),
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
async fn led_status(State(state): State<AppState>) -> impl IntoResponse {
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

    if inventory.as_ref().is_none_or(|inv| !inventory_has_lighting(inv))
        && allow_inventory_refresh().await
        && let Some(refreshed) = refresh_inventory(&state, sensors.clone()).await
    {
        inventory = Some(refreshed);
    }

    if let Some(inv) = inventory {
        status = derive_lighting_status(&inv);
    }
    Json(status)
}

#[utoipa::path(
    get,
    path = "/peripherals/sensors",
    tag = "Peripherals",
    responses(
        (status = 200, description = "Sensor peripherals inventory", body = [SensorPeripheral]),
        (status = 503, description = "Peripherals IPC unavailable", body = ErrorBody)
    )
)]
async fn list_sensors(State(state): State<AppState>) -> impl IntoResponse {
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
                    Ok(Ok(Err(reason))) => return (StatusCode::BAD_GATEWAY, Json(ErrorBody::new("bad_gateway", reason))).into_response(),
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

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ConfigureSensorFirmwareRequest {
    pub device_id: String,
    pub firmware: String,
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
async fn configure_sensor_firmware(State(state): State<AppState>, Json(req): Json<ConfigureSensorFirmwareRequest>) -> impl IntoResponse {
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
            invalidate_peripheral_inventory_cache().await;
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

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct ConfigureSensorAliasRequest {
    pub hardware_key: String,
    pub alias: String,
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
async fn configure_sensor_alias(State(state): State<AppState>, Json(req): Json<ConfigureSensorAliasRequest>) -> impl IntoResponse {
    let Some(sensors) = state.ensure_sensors().await else {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(ErrorBody::new("service_unavailable", "peripherals IPC unavailable"))).into_response();
    };

    let hardware_key = req.hardware_key.trim();
    if hardware_key.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(ErrorBody::new("bad_request", "hardware_key is required"))).into_response();
    }

    match sensors.configure_alias(hardware_key.to_string(), req.alias).await {
        Ok(Ok(())) => {
            invalidate_peripheral_inventory_cache().await;
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(Err(reason)) => (StatusCode::BAD_REQUEST, Json(ErrorBody::new("bad_request", reason))).into_response(),
        Err(err) => {
            error!(%err, "failed to send configure alias command");
            (StatusCode::BAD_GATEWAY, Json(ErrorBody::new("bad_gateway", "failed to configure alias"))).into_response()
        }
    }
}

fn derive_lighting_status(inv: &helios_peripherals::dto::SensorInventory) -> LightingStatus {
    let present = inv.sensors.iter().any(|s| s.metadata.as_ref().map(|m| m.json.to_lowercase().contains("lighting")).unwrap_or(false));
    LightingStatus { present, last_error: None }
}

fn map_sensor_inventory(inv: &helios_peripherals::dto::SensorInventory) -> Vec<SensorPeripheral> {
    inv.sensors.iter().filter_map(map_sensor_descriptor).collect()
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
            serde_json::Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default();
    let metadata = desc
        .metadata
        .as_ref()
        .and_then(|meta| meta.to_value().ok())
        .and_then(|value| match value {
            serde_json::Value::Object(map) => Some(map),
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

fn meta_string(meta: &serde_json::Map<String, serde_json::Value>, key: &str) -> Option<String> {
    match meta.get(key) {
        Some(serde_json::Value::String(value)) => {
            let trimmed = value.trim();
            if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
        }
        _ => None,
    }
}

fn sensor_warnings(info: &serde_json::Map<String, serde_json::Value>) -> Vec<SensorPeripheralWarning> {
    let Some(path) = meta_string(info, "path") else {
        return Vec::new();
    };
    let warnings = usb_warnings_from_path(Path::new(&path));
    warnings.into_iter().map(|warning| SensorPeripheralWarning { code: warning.code, message: warning.message }).collect()
}

fn map_firmware_status(meta: &serde_json::Map<String, serde_json::Value>) -> Option<SensorPeripheralFirmwareStatus> {
    let options = match meta.get("firmware_options") {
        Some(serde_json::Value::Array(entries)) => entries
            .iter()
            .filter_map(|entry| match entry {
                serde_json::Value::Object(map) => {
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

fn list_usb_sysfs() -> Vec<UsbPeripheral> {
    let mut out = Vec::new();
    let root = Path::new("/sys/bus/usb/devices");
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return out,
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        // Skip interface nodes (e.g. "1-1:1.0") and only consider device nodes.
        if name.contains(':') {
            continue;
        }
        let path = entry.path();
        let vendor = read_hex_u16(path.join("idVendor").as_path());
        let product = read_hex_u16(path.join("idProduct").as_path());
        let (Some(vendor), Some(product)) = (vendor, product) else {
            continue;
        };

        // Filter out root hubs / host controllers.
        //
        // On CM5 we want this endpoint to represent externally attached devices; otherwise
        // it shows "nonsense" like xHCI host controllers even when nothing is plugged in.
        // Root hubs are typically Linux Foundation (0x1d6b) and class 0x09.
        if vendor == 0x1D6B {
            continue;
        }
        if let Some(class) = read_hex_u8(path.join("bDeviceClass").as_path())
            && class == 0x09
        {
            continue;
        }

        let manufacturer = read_string(path.join("manufacturer").as_path());
        let product_name = read_string(path.join("product").as_path());
        let description = match (manufacturer.as_deref(), product_name.as_deref()) {
            (Some(m), Some(p)) => Some(format!("{m} {p}")),
            (Some(m), None) => Some(m.to_string()),
            (None, Some(p)) => Some(p.to_string()),
            _ => None,
        };

        let desc_lower = description.as_deref().unwrap_or_default().to_lowercase();
        let is_coral = matches!(
            (vendor, product),
            // Global Unichip / Coral USB accelerator
            (0x1A6E, 0x089A)
            // Google / Coral Edge TPU (common ids)
            | (0x18D1, 0x9302)
            | (0x18D1, 0x9301)
        ) || desc_lower.contains("coral")
            || desc_lower.contains("edge tpu")
            || desc_lower.contains("edgetpu");
        let kind = if is_coral { Some("coral".into()) } else { Some("usb".into()) };
        let description = if is_coral && description.is_none() { Some("Coral Edge TPU".into()) } else { description };
        let warnings = usb_warnings_from_path(&path);
        out.push(UsbPeripheral { id: format!("{vendor:04x}:{product:04x}@{name}"), kind, description, present: true, warnings });
    }

    out
}

fn read_string(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn read_hex_u16(path: &Path) -> Option<u16> {
    let raw = fs::read_to_string(path).ok()?;
    let trimmed = raw.trim().trim_start_matches("0x");
    u16::from_str_radix(trimmed, 16).ok()
}

fn read_hex_u8(path: &Path) -> Option<u8> {
    let raw = fs::read_to_string(path).ok()?;
    let trimmed = raw.trim().trim_start_matches("0x");
    u8::from_str_radix(trimmed, 16).ok()
}

fn usb_warnings_from_path(device_path: &Path) -> Vec<UsbPeripheralWarning> {
    let mut warnings = Vec::new();

    if let Some(speed) = read_usb_speed(device_path)
        && speed < 5_000.0
    {
        warnings.push(UsbPeripheralWarning { code: "usb_speed_low".into(), message: "USB 2.0/1.x link detected; use a quality USB 3.0 cable and connect directly to a USB 3 port.".into() });
    }

    if usb_has_external_hub_parent(device_path) {
        warnings.push(UsbPeripheralWarning {
            code: "usb_hub_power".into(),
            message: "Device is connected through a USB hub; hubs can under-power peripherals and cause flaky USB events. Prefer a direct USB port.".into(),
        });
    }

    if let Some(undervoltage) = read_system_undervoltage()
        && undervoltage
    {
        warnings.push(UsbPeripheralWarning {
            code: "system_undervoltage".into(),
            message: "System undervoltage detected; USB devices can reset or drop to lower speeds. Check the power supply and cabling.".into(),
        });
    }

    warnings
}

fn read_usb_speed(device_path: &Path) -> Option<f32> {
    let raw = fs::read_to_string(device_path.join("speed")).ok()?;
    raw.trim().parse::<f32>().ok()
}

fn usb_has_external_hub_parent(device_path: &Path) -> bool {
    let root = Path::new("/sys/bus/usb/devices");
    let mut path = device_path;
    loop {
        if let (Some(class), Some(vendor)) = (read_hex_u8(path.join("bDeviceClass").as_path()), read_hex_u16(path.join("idVendor").as_path()))
            && class == 0x09
            && vendor != 0x1D6B
        {
            return true;
        }
        let Some(parent) = path.parent() else {
            break;
        };
        if parent == path || parent == root {
            break;
        }
        path = parent;
    }
    false
}

fn read_system_undervoltage() -> Option<bool> {
    let raw = fs::read_to_string("/sys/devices/platform/soc/firmware/get_throttled").ok()?;
    let trimmed = raw.trim().trim_start_matches("0x");
    let value = u32::from_str_radix(trimmed, 16).ok()?;
    Some((value & 0x1) != 0 || (value & (1 << 16)) != 0)
}

fn map_fan_status(status: lib_sensors::fan_config::FanStatus) -> FanStatus {
    FanStatus { present: true, rpm: status.rpm, mode: Some(format!("{:?}", status.mode)), target_percent: Some(status.target_percent), last_error: status.last_error }
}

fn safe_discover_cameras() -> Result<helios_engine::capture::DiscoveryResult, String> {
    catch_unwind(AssertUnwindSafe(helios_engine::capture::discover_devices_with_errors)).map_err(|_| "camera discovery panicked".to_string())
}

struct CachedDiscovery {
    result: helios_engine::capture::DiscoveryResult,
    at: Instant,
}

static CAMERA_CACHE: Lazy<Mutex<Option<CachedDiscovery>>> = Lazy::new(|| Mutex::new(None));
static INVENTORY_REFRESH_AT: Lazy<Mutex<Instant>> = Lazy::new(|| Mutex::new(Instant::now() - INVENTORY_REFRESH_MIN));

const INVENTORY_REFRESH_MIN: Duration = Duration::from_secs(10);

fn camera_refresh_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn read_timeout_env(var: &str, default_ms: u64, min_ms: u64, max_ms: u64) -> Duration {
    let ms = std::env::var(var).ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(default_ms);
    Duration::from_millis(ms.clamp(min_ms, max_ms))
}

fn sensor_ipc_timeout() -> Duration {
    static VALUE: OnceLock<Duration> = OnceLock::new();
    *VALUE.get_or_init(|| read_timeout_env("HELIOS_PERIPHERALS_TIMEOUT_MS", 1_500, 250, 15_000))
}

fn camera_discovery_timeout() -> Duration {
    static VALUE: OnceLock<Duration> = OnceLock::new();
    *VALUE.get_or_init(|| read_timeout_env("HELIOS_CAMERA_DISCOVERY_TIMEOUT_MS", 1_500, 250, 20_000))
}

fn sensor_refresh_timeout() -> Duration {
    static VALUE: OnceLock<Duration> = OnceLock::new();
    *VALUE.get_or_init(|| read_timeout_env("HELIOS_PERIPHERALS_REFRESH_TIMEOUT_MS", 2_500, 500, 20_000))
}

async fn cached_discover_cameras() -> Result<helios_engine::capture::DiscoveryResult, String> {
    const TTL: Duration = Duration::from_secs(5);

    {
        let guard = CAMERA_CACHE.lock().await;
        if let Some(cached) = guard.as_ref()
            && cached.at.elapsed() < TTL
        {
            return Ok(cached.result.clone());
        }
    }

    let _refresh_guard = camera_refresh_lock().lock().await;
    {
        let guard = CAMERA_CACHE.lock().await;
        if let Some(cached) = guard.as_ref()
            && cached.at.elapsed() < TTL
        {
            return Ok(cached.result.clone());
        }
    }

    let handle = task::spawn_blocking(safe_discover_cameras);
    let discovery = match tokio::time::timeout(camera_discovery_timeout(), handle).await {
        Ok(joined) => match joined {
            Ok(result) => result,
            Err(_) => Err("camera discovery task panicked".to_string()),
        },
        Err(_) => Err("camera discovery timed out".to_string()),
    };

    match discovery {
        Ok(result) => {
            let mut guard = CAMERA_CACHE.lock().await;
            *guard = Some(CachedDiscovery { result: result.clone(), at: Instant::now() });
            Ok(result)
        }
        Err(err) => {
            let mut fallback = {
                let guard = CAMERA_CACHE.lock().await;
                guard.as_ref().map(|entry| entry.result.clone()).unwrap_or_else(|| helios_engine::capture::DiscoveryResult { devices: Vec::new(), errors: Vec::new() })
            };
            if fallback.errors.iter().all(|entry| entry != &err) {
                fallback.errors.push(err);
                if fallback.errors.len() > 5 {
                    let drain = fallback.errors.len() - 5;
                    fallback.errors.drain(0..drain);
                }
            }
            let mut guard = CAMERA_CACHE.lock().await;
            *guard = Some(CachedDiscovery { result: fallback.clone(), at: Instant::now() });
            Ok(fallback)
        }
    }
}

async fn allow_inventory_refresh() -> bool {
    let mut guard = INVENTORY_REFRESH_AT.lock().await;
    if guard.elapsed() < INVENTORY_REFRESH_MIN {
        return false;
    }
    *guard = Instant::now();
    true
}

async fn refresh_inventory(state: &AppState, sensors: Arc<SensorsConnection>) -> Option<helios_peripherals::dto::SensorInventory> {
    match tokio::time::timeout(sensor_refresh_timeout(), sensors.discover(true)).await {
        Ok(Ok(Ok(inv))) => return Some(inv),
        Ok(Ok(Err(reason))) => {
            warn!(%reason, "sensor inventory refresh rejected");
            return None;
        }
        Ok(Err(err)) => {
            warn!(%err, "sensor inventory refresh failed");
        }
        Err(_) => {
            warn!("sensor inventory refresh timed out");
        }
    }

    state.invalidate_sensors().await;
    let sensors = state.ensure_sensors().await?;
    match tokio::time::timeout(sensor_refresh_timeout(), sensors.discover(true)).await {
        Ok(Ok(Ok(inv))) => Some(inv),
        Ok(Ok(Err(reason))) => {
            warn!(%reason, "sensor inventory refresh rejected after reconnect");
            None
        }
        Ok(Err(err)) => {
            warn!(%err, "sensor inventory refresh failed after reconnect");
            None
        }
        Err(_) => {
            warn!("sensor inventory refresh timed out after reconnect");
            None
        }
    }
}

fn inventory_needs_refresh(inv: &helios_peripherals::dto::SensorInventory, usb: &[UsbPeripheral]) -> bool {
    if inv.sensors.is_empty() {
        return true;
    }
    if usb_has_coral(usb) && !inventory_has_coral(inv) {
        return true;
    }
    if !inventory_has_lighting(inv) {
        return true;
    }
    false
}

fn inventory_has_coral(inv: &helios_peripherals::dto::SensorInventory) -> bool {
    inv.sensors.iter().any(|sensor| sensor.backend.eq_ignore_ascii_case("coral"))
}

fn inventory_has_lighting(inv: &helios_peripherals::dto::SensorInventory) -> bool {
    inv.sensors.iter().any(|sensor| {
        let backend = sensor.backend.to_ascii_lowercase();
        backend.contains("led") || backend.contains("lighting")
    })
}

fn usb_has_coral(devices: &[UsbPeripheral]) -> bool {
    devices.iter().any(|device| match device.kind.as_deref() {
        Some(kind) => kind.eq_ignore_ascii_case("coral"),
        None => false,
    })
}
