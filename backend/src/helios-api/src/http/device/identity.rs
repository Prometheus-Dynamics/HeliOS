use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::super::error::{ApiError, ApiResult};

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HostnamePayload {
    pub hostname: String,
}

fn validate_hostname(value: &str) -> Result<(), Box<ApiError>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(Box::new(ApiError::bad_request("hostname is required")));
    }
    if trimmed.len() > 63 {
        return Err(Box::new(ApiError::bad_request("hostname must be <= 63 characters")));
    }
    if !trimmed.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-') || trimmed.starts_with('-') || trimmed.ends_with('-') {
        return Err(Box::new(ApiError::bad_request("hostname must use letters, numbers, and hyphens")));
    }
    Ok(())
}

#[utoipa::path(
    get,
    path = "/device/hostname",
    tag = "Device",
    responses((status = 200, description = "Device hostname", body = HostnamePayload), (status = 502, description = "Hostname unavailable", body = super::super::error::ErrorBody))
)]
pub async fn hostname() -> ApiResult<impl IntoResponse> {
    let hostname = lib_net::get_hostname().map_err(ApiError::from)?;
    Ok(Json(HostnamePayload { hostname: hostname.to_string_lossy().trim().to_string() }))
}

#[utoipa::path(
    post,
    path = "/device/hostname",
    tag = "Device",
    request_body(content = HostnamePayload, content_type = "application/json"),
    responses((status = 204, description = "Hostname updated"), (status = 400, description = "Invalid request", body = super::super::error::ErrorBody), (status = 502, description = "Hostname unavailable", body = super::super::error::ErrorBody))
)]
pub async fn set_hostname(Json(payload): Json<HostnamePayload>) -> ApiResult<impl IntoResponse> {
    validate_hostname(&payload.hostname)?;
    lib_net::set_hostname(payload.hostname.trim()).map_err(ApiError::from)?;
    Ok(StatusCode::NO_CONTENT)
}
