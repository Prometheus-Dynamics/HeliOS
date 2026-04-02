use std::{collections::BTreeMap, path::Path};

use tokio::process::Command;

use crate::http::{
    error::{ApiError, ApiResult},
    persisted_files,
};

use super::UsbPowerSettings;

const USB_POWER_ENV_PATH: &str = "/var/lib/helios/usb-power.env";
const USB_POWER_SCRIPT_PATH: &str = "/usr/local/bin/helios-usb-power-setup.sh";

pub(super) fn parse_env_file(contents: &str) -> BTreeMap<String, String> {
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

pub(super) fn parse_bool(value: Option<&str>, default: bool) -> bool {
    let Some(value) = value else {
        return default;
    };
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => true,
        "0" | "false" | "no" | "off" => false,
        _ => default,
    }
}

fn parse_u32(value: Option<&str>) -> Option<u32> {
    value.and_then(|raw| raw.trim().parse::<u32>().ok())
}

pub(super) async fn load_settings() -> UsbPowerSettings {
    let Ok(raw) = tokio::fs::read_to_string(Path::new(USB_POWER_ENV_PATH)).await else {
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

fn render_settings(settings: &UsbPowerSettings) -> String {
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
    out
}

pub(super) async fn persist_settings(settings: &UsbPowerSettings) -> ApiResult<()> {
    let out = render_settings(settings);
    persisted_files::write_canonical(Path::new(USB_POWER_ENV_PATH), out.as_bytes()).await.map_err(|err| ApiError::internal(format!("failed to write usb power config: {err}")))?;
    Ok(())
}

pub(super) async fn apply_usb_power() -> ApiResult<()> {
    if !Path::new(USB_POWER_SCRIPT_PATH).exists() {
        return Err(ApiError::service_unavailable("usb power setup script not available"));
    }
    let raw = tokio::fs::read_to_string(Path::new(USB_POWER_ENV_PATH)).await.map_err(|err| ApiError::internal(format!("failed to read usb power config: {err}")))?;
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

#[cfg(test)]
pub(super) fn render_settings_for_tests(settings: &UsbPowerSettings) -> String {
    render_settings(settings)
}
