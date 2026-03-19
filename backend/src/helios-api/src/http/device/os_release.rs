use axum::{Json, http::StatusCode};
use serde::Serialize;
use utoipa::ToSchema;

use super::super::error::ApiResult;

#[derive(Debug, Serialize, ToSchema)]
pub struct OsReleaseInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pretty_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_root: Option<String>,
}

#[utoipa::path(
    get,
    path = "/device/os",
    tag = "Device",
    responses((status = 200, description = "OS release info", body = OsReleaseInfo), (status = 404, description = "OS release not found", body = super::super::error::ErrorBody))
)]
pub async fn os_release() -> ApiResult<impl axum::response::IntoResponse> {
    let contents = tokio::fs::read_to_string("/etc/os-release").await?;
    let mut info = parse_os_release(&contents);
    if info.version_id.is_none() {
        info.version_id = read_text_value("/etc/helios/version").await;
    }
    if info.build_id.is_none() {
        info.build_id = read_text_value("/etc/helios/build-id").await.or_else(|| info.version_id.clone());
    }
    info.active_root = read_active_root().await;
    Ok((StatusCode::OK, Json(info)))
}

fn parse_os_release(contents: &str) -> OsReleaseInfo {
    let mut version_id = None;
    let mut build_id = None;
    let mut pretty_name = None;

    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, raw_value)) = line.split_once('=') else {
            continue;
        };
        let value = raw_value.trim().trim_matches('"').trim_matches('\'').trim().to_string();
        if value.is_empty() {
            continue;
        }
        match key.trim() {
            "VERSION_ID" => version_id = Some(value),
            "BUILD_ID" => build_id = Some(value),
            "PRETTY_NAME" => pretty_name = Some(value),
            _ => {}
        }
    }

    OsReleaseInfo { version_id, build_id, pretty_name, active_root: None }
}

async fn read_active_root() -> Option<String> {
    if let Some(slot) = read_active_root_from_cmdline().await {
        return Some(slot);
    }

    const ACTIVE_ROOT_PATHS: &[&str] = &["/var/lib/helios/ota/active", "/boot/helios/ota/active", "/mnt/boot/helios/ota/active"];

    for path in ACTIVE_ROOT_PATHS {
        let Ok(contents) = tokio::fs::read_to_string(path).await else {
            continue;
        };
        let value = contents.trim();
        if value.is_empty() {
            continue;
        }
        return Some(value.to_string());
    }

    None
}

async fn read_active_root_from_cmdline() -> Option<String> {
    let cmdline = tokio::fs::read_to_string("/proc/cmdline").await.ok()?;
    let root = cmdline.split_whitespace().find_map(|token| token.strip_prefix("root=")).map(str::trim).filter(|value| !value.is_empty())?;

    match root {
        "/dev/mmcblk0p2" | "/dev/mmcblk1p2" | "/dev/sda2" => Some("ROOT_A".to_string()),
        "/dev/mmcblk0p3" | "/dev/mmcblk1p3" | "/dev/sda3" => Some("ROOT_B".to_string()),
        "/dev/helios-rootfs" => read_text_value("/var/lib/helios/ota/active").await.or_else(|| Some("ROOT_A".to_string())),
        _ => None,
    }
}

async fn read_text_value(path: &str) -> Option<String> {
    let Ok(contents) = tokio::fs::read_to_string(path).await else {
        return None;
    };
    let value = contents.trim();
    if value.is_empty() {
        return None;
    }
    Some(value.to_string())
}
