use axum::{Json, http::StatusCode};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use utoipa::ToSchema;

use super::super::error::{ApiError, ApiResult};

const CONFIG_PATH: &str = "/etc/helios/bootloader.conf";
const DEFAULT_UPDATE_FILE: &str = "/usr/share/helios/bootloader/pieeprom-2025-12-08.upd";
const DEFAULT_UPDATE_SIG: &str = "/usr/share/helios/bootloader/pieeprom-2025-12-08.sig";
const DEFAULT_BOOT_PARTITION: &str = "/dev/disk/by-label/BOOT";
const BOOT_MOUNT: &str = "/boot";

#[derive(Debug, Serialize, ToSchema)]
pub struct BootloaderStatus {
    pub supported: bool,
    pub current_version: Option<String>,
    pub required_version: Option<String>,
    pub needs_update: bool,
    pub update_available: bool,
    pub update_file: Option<String>,
    pub update_sig: Option<String>,
    pub staged: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_message: Option<String>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct BootloaderUpdateRequest {
    pub confirm: bool,
    #[serde(default)]
    pub reboot: Option<bool>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BootloaderUpdateResponse {
    pub staged: bool,
    pub rebooting: bool,
    pub message: String,
}

#[derive(Debug, Clone)]
struct BootloaderConfig {
    required_version: Option<String>,
    update_file: PathBuf,
    update_sig: Option<PathBuf>,
    boot_partition: PathBuf,
}

#[utoipa::path(
    get,
    path = "/device/bootloader",
    tag = "Device",
    operation_id = "bootloader_status",
    responses((status = 200, description = "Bootloader firmware status", body = BootloaderStatus))
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

fn bootloader_status() -> (BootloaderStatus, Option<String>) {
    let config = load_config();
    bootloader_status_with_config(&config)
}

fn bootloader_status_with_config(config: &BootloaderConfig) -> (BootloaderStatus, Option<String>) {
    let model = read_device_tree_string("/proc/device-tree/model");
    let supported = model.as_deref().map(is_supported_model).unwrap_or(false);
    let current_version = read_device_tree_string("/proc/device-tree/chosen/bootloader/version");

    let required_version = config.required_version.clone();
    let update_available = config.update_file.exists();
    let update_file = update_available.then(|| config.update_file.display().to_string());
    let update_sig = config.update_sig.as_ref().and_then(|path| path.exists().then(|| path.display().to_string()));

    let needs_update = supported && update_available && required_version.as_deref().map(|required| current_version.as_deref().map(|current| current != required).unwrap_or(true)).unwrap_or(false);

    let staged = is_mountpoint(Path::new(BOOT_MOUNT)) && Path::new(BOOT_MOUNT).join("pieeprom.upd").exists();

    let message = if !supported {
        Some("Bootloader updates are not supported on this device.".to_string())
    } else if !update_available {
        Some("Bootloader update file is missing on this image.".to_string())
    } else if needs_update {
        Some("Bootloader update required to enable RP1 peripherals.".to_string())
    } else {
        None
    };

    (BootloaderStatus { supported, current_version, required_version, needs_update, update_available, update_file, update_sig, staged, status_message: None }, message)
}

fn load_config() -> BootloaderConfig {
    let mut config = BootloaderConfig {
        required_version: None,
        update_file: PathBuf::from(DEFAULT_UPDATE_FILE),
        update_sig: Some(PathBuf::from(DEFAULT_UPDATE_SIG)),
        boot_partition: PathBuf::from(DEFAULT_BOOT_PARTITION),
    };

    let Ok(raw) = fs::read_to_string(CONFIG_PATH) else {
        return config;
    };

    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        match key {
            "required_version" => config.required_version = Some(value.to_string()),
            "update_file" => config.update_file = PathBuf::from(value),
            "update_sig" => config.update_sig = Some(PathBuf::from(value)),
            "boot_partition" => config.boot_partition = PathBuf::from(value),
            _ => {}
        }
    }

    config
}

fn read_device_tree_string(path: &str) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let trimmed = text.trim_matches('\0').trim();
    if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
}

fn is_supported_model(model: &str) -> bool {
    let lower = model.to_lowercase();
    lower.contains("compute module 5") || lower.contains("raspberry pi 5")
}

fn is_mountpoint(path: &Path) -> bool {
    let Ok(target) = path.canonicalize() else {
        return false;
    };
    let Ok(mounts) = fs::read_to_string("/proc/self/mountinfo") else {
        return false;
    };
    for line in mounts.lines() {
        let mut fields = line.split_whitespace();
        let _id = fields.next();
        let _parent = fields.next();
        let _major_minor = fields.next();
        let _root = fields.next();
        let mount_point = fields.next();
        if let Some(mount_point) = mount_point
            && mount_point == target.to_string_lossy()
        {
            return true;
        }
    }
    false
}

fn stage_update(config: &BootloaderConfig) -> Result<(), Box<ApiError>> {
    let boxed = |err: ApiError| Box::new(err);
    if !config.update_file.exists() {
        return Err(boxed(ApiError::bad_request("bootloader update file missing")));
    }
    if !config.boot_partition.exists() {
        return Err(boxed(ApiError::bad_request("boot partition device not found")));
    }

    let mount_path = Path::new(BOOT_MOUNT);
    fs::create_dir_all(mount_path).map_err(|err| boxed(ApiError::internal(format!("failed to prepare boot mount: {err}"))))?;

    let was_mounted = is_mountpoint(mount_path);
    if !was_mounted {
        let status = Command::new("mount")
            .arg("-o")
            .arg("rw")
            .arg(&config.boot_partition)
            .arg(mount_path)
            .status()
            .map_err(|err| boxed(ApiError::internal(format!("failed to mount boot partition: {err}"))))?;
        if !status.success() {
            return Err(boxed(ApiError::internal(format!("mount failed with status: {status}"))));
        }
    }

    let target_upd = mount_path.join("pieeprom.upd");
    fs::copy(&config.update_file, &target_upd).map_err(|err| boxed(ApiError::internal(format!("failed to stage pieeprom.upd: {err}"))))?;

    if let Some(sig_path) = &config.update_sig
        && sig_path.exists()
    {
        let target_sig = mount_path.join("pieeprom.sig");
        fs::copy(sig_path, &target_sig).map_err(|err| boxed(ApiError::internal(format!("failed to stage pieeprom.sig: {err}"))))?;
    }

    let _ = Command::new("sync").status();

    if !was_mounted {
        let _ = Command::new("umount").arg(mount_path).status();
    }

    Ok(())
}
