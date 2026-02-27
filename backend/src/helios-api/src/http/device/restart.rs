use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tracing::{debug, warn};
use utoipa::ToSchema;

use crate::http::error::ErrorBody;

#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct DeviceOperationAckResponse {
    pub status: String,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
pub struct RestartRequest {
    pub target: RestartTargetId,
    #[serde(default)]
    pub requested_by: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum RestartTargetId {
    Api,
    Engine,
    Peripherals,
    Device,
}

fn target_unit(target: RestartTargetId) -> Option<&'static str> {
    match target {
        RestartTargetId::Api => Some("helios-api.service"),
        RestartTargetId::Engine => Some("helios-engine.service"),
        RestartTargetId::Peripherals => Some("helios-peripherals.service"),
        RestartTargetId::Device => None,
    }
}

async fn systemctl(args: &[&str]) -> std::io::Result<std::process::Output> {
    Command::new("systemctl").args(args).output().await
}

fn ota_pending_marker_exists() -> bool {
    std::fs::metadata("/var/lib/helios/ota/pending").is_ok() || std::fs::metadata("/root/helios-updater/ota/pending").is_ok()
}

async fn reboot_with_args(args: &[&str]) -> std::io::Result<std::process::Output> {
    for cmd in ["/sbin/reboot", "/usr/sbin/reboot", "reboot"] {
        match Command::new(cmd).args(args).output().await {
            Ok(output) => return Ok(output),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        }
    }
    systemctl(&["reboot", "--no-block"]).await
}

async fn reboot_tryboot() -> std::io::Result<std::process::Output> {
    reboot_with_args(&["tryboot"]).await
}

async fn reboot_now() -> std::io::Result<std::process::Output> {
    reboot_with_args(&[]).await
}

#[utoipa::path(
    post,
    path = "/device/restart",
    tag = "Device",
    request_body = RestartRequest,
    responses(
        (status = 200, description = "Restart queued", body = DeviceOperationAckResponse),
        (status = 502, description = "systemctl failed", body = ErrorBody)
    )
)]
pub async fn restart(Json(req): Json<RestartRequest>) -> Response {
    if let Some(requested_by) = req.requested_by.as_deref() {
        debug!(requested_by, target = ?req.target, "restart requested");
    } else {
        debug!(target = ?req.target, "restart requested");
    }

    let output = match req.target {
        RestartTargetId::Device => {
            if ota_pending_marker_exists() {
                reboot_tryboot().await
            } else {
                reboot_now().await
            }
        }
        target => {
            let Some(unit) = target_unit(target) else {
                warn!(target = ?target, "restart target missing unit");
                return (StatusCode::BAD_GATEWAY, Json(ErrorBody::new("bad_gateway", "invalid restart target"))).into_response();
            };
            systemctl(&["restart", "--no-block", unit]).await
        }
    };

    match output {
        Ok(output) if output.status.success() => Json(DeviceOperationAckResponse { status: "queued".into() }).into_response(),
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let message = if !stderr.is_empty() {
                stderr
            } else if !stdout.is_empty() {
                stdout
            } else {
                format!("systemctl exited with {}", output.status)
            };
            (StatusCode::BAD_GATEWAY, Json(ErrorBody::new("bad_gateway", message))).into_response()
        }
        Err(err) => (StatusCode::BAD_GATEWAY, Json(ErrorBody::new("bad_gateway", err.to_string()))).into_response(),
    }
}
