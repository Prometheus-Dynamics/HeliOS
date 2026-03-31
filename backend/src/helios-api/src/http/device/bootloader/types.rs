use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

pub(super) const CONFIG_PATH: &str = "/etc/helios/bootloader.conf";
pub(super) const DEFAULT_UPDATE_FILE: &str = "/usr/share/helios/bootloader/pieeprom-2025-12-08.upd";
pub(super) const DEFAULT_UPDATE_SIG: &str = "/usr/share/helios/bootloader/pieeprom-2025-12-08.sig";
pub(super) const DEFAULT_BOOT_PARTITION: &str = "/dev/disk/by-label/BOOT";
pub(super) const BOOT_MOUNT: &str = "/boot";

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
pub(super) struct BootloaderConfig {
    pub(super) required_version: Option<String>,
    pub(super) update_file: PathBuf,
    pub(super) update_sig: Option<PathBuf>,
    pub(super) boot_partition: PathBuf,
}
