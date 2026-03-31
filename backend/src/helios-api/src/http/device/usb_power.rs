mod support;
#[cfg(test)]
mod tests;
mod types;

use axum::{Json, http::StatusCode, response::IntoResponse};

use crate::http::error::{ApiResult, ErrorBody};

pub use types::UsbPowerSettings;

use support::{apply_usb_power, load_settings, persist_settings};

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
