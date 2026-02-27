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
}

#[utoipa::path(
    get,
    path = "/device/os",
    tag = "Device",
    responses((status = 200, description = "OS release info", body = OsReleaseInfo), (status = 404, description = "OS release not found", body = super::super::error::ErrorBody))
)]
pub async fn os_release() -> ApiResult<impl axum::response::IntoResponse> {
    let contents = tokio::fs::read_to_string("/etc/os-release").await?;
    let info = parse_os_release(&contents);
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

    OsReleaseInfo { version_id, build_id, pretty_name }
}
