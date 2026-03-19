use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use tokio::process::Command;
use utoipa::ToSchema;

use super::super::error::{ApiError, ApiResult, ErrorBody};
use crate::http::persisted_files;

const USB_POWER_ENV_PATH: &str = "/var/lib/helios/usb-power.env";
const LEGACY_USB_POWER_ENV_PATH: &str = "/etc/helios/usb-power.env";
const USB_POWER_SCRIPT_PATH: &str = "/usr/local/bin/helios-usb-power-setup.sh";

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UsbPowerSettings {
    pub enabled: bool,
    pub usb_a_gpio: Option<u32>,
    pub usb_c_gpio: Option<u32>,
    pub usb_a_active_high: bool,
    pub usb_c_active_high: bool,
    pub usb_a_enabled: bool,
    pub usb_c_enabled: bool,
    #[serde(default = "default_true")]
    pub tuning_enabled: bool,
    #[serde(default = "default_true")]
    pub disable_autosuspend: bool,
    #[serde(default = "default_true")]
    pub disable_usb2_lpm: bool,
    #[serde(default = "default_true")]
    pub force_power_control_on: bool,
}

impl Default for UsbPowerSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            usb_a_gpio: None,
            usb_c_gpio: None,
            usb_a_active_high: true,
            usb_c_active_high: true,
            usb_a_enabled: true,
            usb_c_enabled: true,
            tuning_enabled: true,
            disable_autosuspend: true,
            disable_usb2_lpm: true,
            force_power_control_on: true,
        }
    }
}

#[utoipa::path(
    get,
    path = "/device/usb-power",
    tag = "Device",
    responses((status = 200, description = "USB power settings", body = UsbPowerSettings))
)]
pub async fn get_usb_power_settings() -> ApiResult<impl IntoResponse> {
    Ok(Json(load_settings().await))
}

#[utoipa::path(
    post,
    path = "/device/usb-power",
    tag = "Device",
    request_body = UsbPowerSettings,
    responses(
        (status = 204, description = "USB power settings updated"),
        (status = 400, description = "Invalid request", body = ErrorBody),
        (status = 503, description = "USB power control unavailable", body = ErrorBody),
        (status = 502, description = "USB power command failed", body = ErrorBody)
    )
)]
pub async fn set_usb_power_settings(Json(payload): Json<UsbPowerSettings>) -> ApiResult<StatusCode> {
    persist_settings(&payload).await?;
    apply_usb_power().await?;
    Ok(StatusCode::NO_CONTENT)
}

fn parse_env_file(contents: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        out.insert(key.trim().to_string(), value.trim().to_string());
    }
    out
}

fn parse_bool(value: Option<&str>, default: bool) -> bool {
    let Some(value) = value else {
        return default;
    };
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => default,
    }
}

fn default_true() -> bool {
    true
}

fn parse_u32(value: Option<&str>) -> Option<u32> {
    value.and_then(|raw| raw.trim().parse::<u32>().ok())
}

async fn load_settings() -> UsbPowerSettings {
    let Ok(raw) = persisted_files::read_to_string(Path::new(USB_POWER_ENV_PATH), Some(Path::new(LEGACY_USB_POWER_ENV_PATH))).await else {
        return UsbPowerSettings::default();
    };
    let env = parse_env_file(&raw);
    UsbPowerSettings {
        enabled: parse_bool(env.get("USB_POWER_ENABLED").map(|v| v.as_str()), true),
        usb_a_gpio: parse_u32(env.get("USB_POWER_USB_A_GPIO").map(|v| v.as_str())),
        usb_c_gpio: parse_u32(env.get("USB_POWER_USB_C_GPIO").map(|v| v.as_str())),
        usb_a_active_high: parse_bool(env.get("USB_POWER_USB_A_ACTIVE_HIGH").map(|v| v.as_str()), true),
        usb_c_active_high: parse_bool(env.get("USB_POWER_USB_C_ACTIVE_HIGH").map(|v| v.as_str()), true),
        usb_a_enabled: parse_bool(env.get("USB_POWER_USB_A_ENABLED").map(|v| v.as_str()), true),
        usb_c_enabled: parse_bool(env.get("USB_POWER_USB_C_ENABLED").map(|v| v.as_str()), true),
        tuning_enabled: parse_bool(env.get("USB_POWER_TUNING_ENABLED").map(|v| v.as_str()), true),
        disable_autosuspend: parse_bool(env.get("USB_POWER_DISABLE_AUTOSUSPEND").map(|v| v.as_str()), true),
        disable_usb2_lpm: parse_bool(env.get("USB_POWER_DISABLE_USB2_LPM").map(|v| v.as_str()), true),
        force_power_control_on: parse_bool(env.get("USB_POWER_FORCE_POWER_CONTROL_ON").map(|v| v.as_str()), true),
    }
}

async fn persist_settings(settings: &UsbPowerSettings) -> ApiResult<()> {
    let mut out = String::from("# Updated by API\n");
    out.push_str(&format!("USB_POWER_ENABLED={}\n", if settings.enabled { 1 } else { 0 }));
    if let Some(value) = settings.usb_a_gpio {
        out.push_str(&format!("USB_POWER_USB_A_GPIO={value}\n"));
    }
    if let Some(value) = settings.usb_c_gpio {
        out.push_str(&format!("USB_POWER_USB_C_GPIO={value}\n"));
    }
    out.push_str(&format!("USB_POWER_USB_A_ACTIVE_HIGH={}\n", if settings.usb_a_active_high { 1 } else { 0 }));
    out.push_str(&format!("USB_POWER_USB_C_ACTIVE_HIGH={}\n", if settings.usb_c_active_high { 1 } else { 0 }));
    out.push_str(&format!("USB_POWER_USB_A_ENABLED={}\n", if settings.usb_a_enabled { 1 } else { 0 }));
    out.push_str(&format!("USB_POWER_USB_C_ENABLED={}\n", if settings.usb_c_enabled { 1 } else { 0 }));
    out.push_str(&format!("USB_POWER_TUNING_ENABLED={}\n", if settings.tuning_enabled { 1 } else { 0 }));
    out.push_str(&format!("USB_POWER_DISABLE_AUTOSUSPEND={}\n", if settings.disable_autosuspend { 1 } else { 0 }));
    out.push_str(&format!("USB_POWER_DISABLE_USB2_LPM={}\n", if settings.disable_usb2_lpm { 1 } else { 0 }));
    out.push_str(&format!("USB_POWER_FORCE_POWER_CONTROL_ON={}\n", if settings.force_power_control_on { 1 } else { 0 }));
    persisted_files::write_mirrored(Path::new(USB_POWER_ENV_PATH), Some(Path::new(LEGACY_USB_POWER_ENV_PATH)), out.as_bytes())
        .await
        .map_err(|err| ApiError::internal(format!("failed to write usb power config: {err}")))?;
    Ok(())
}

async fn apply_usb_power() -> ApiResult<()> {
    if !Path::new(USB_POWER_SCRIPT_PATH).exists() {
        return Err(ApiError::service_unavailable("usb power setup script not available"));
    }
    let raw = persisted_files::read_to_string(Path::new(USB_POWER_ENV_PATH), Some(Path::new(LEGACY_USB_POWER_ENV_PATH)))
        .await
        .map_err(|err| ApiError::internal(format!("failed to read usb power config: {err}")))?;
    let env = parse_env_file(&raw);
    let mut cmd = Command::new(USB_POWER_SCRIPT_PATH);
    for (key, value) in env {
        cmd.env(key, value);
    }
    let status = cmd.status().await.map_err(|err| ApiError::bad_gateway(format!("failed to run usb power setup: {err}")))?;
    if !status.success() {
        return Err(ApiError::bad_gateway(format!("usb power setup exited with {}", status)));
    }
    Ok(())
}
