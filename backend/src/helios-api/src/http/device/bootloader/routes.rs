use std::process::Command;

use axum::{Json, http::StatusCode};

use crate::http::error::{ApiError, ApiResult};

use super::support::{bootloader_status, bootloader_status_with_config, load_config, stage_update};
use super::types::{BootloaderUpdateRequest, BootloaderUpdateResponse};

#[utoipa::path(
    get,
    path = "/device/bootloader",
    tag = "Device",
    operation_id = "bootloader_status",
    responses((status = 200, description = "Bootloader firmware status", body = super::types::BootloaderStatus))
)]
pub async fn status() -> ApiResult<impl axum::response::IntoResponse> {
    let (status, message) = bootloader_status();
    let mut status = status;
    if status.status_message.is_none() {
        status.status_message = message;
    }
    Ok((StatusCode::OK, Json(status)))
}

#[utoipa::path(
    post,
    path = "/device/bootloader/update",
    tag = "Device",
    request_body = BootloaderUpdateRequest,
    responses((status = 200, description = "Bootloader firmware update staged", body = BootloaderUpdateResponse))
)]
pub async fn update(Json(req): Json<BootloaderUpdateRequest>) -> ApiResult<impl axum::response::IntoResponse> {
    if !req.confirm {
        return Err(ApiError::bad_request("confirmation required to stage bootloader update"));
    }

    let config = load_config();
    let (status, _) = bootloader_status_with_config(&config);
    if !status.supported {
        return Err(ApiError::bad_request("bootloader update not supported on this device"));
    }
    if !status.update_available {
        return Err(ApiError::bad_request("bootloader update file not available"));
    }

    stage_update(&config).map_err(|err| *err)?;

    let rebooting = req.reboot.unwrap_or(true);
    if rebooting {
        let _ = Command::new("systemctl").arg("reboot").spawn();
    }

    Ok((
        StatusCode::OK,
        Json(BootloaderUpdateResponse {
            staged: true,
            rebooting,
            message: if rebooting { "Bootloader update staged. Device rebooting to apply firmware.".to_string() } else { "Bootloader update staged. Reboot the device to apply firmware.".to_string() },
        }),
    ))
}
