use crate::filesystem::PathPolicy;
use crate::{BoolPolicy, StringPolicy};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpdaterFilesystemPolicy {
    pub socket_path: PathPolicy,
    pub journal_path: PathPolicy,
    pub data_dir: PathPolicy,
    pub frontend_releases_dir: PathPolicy,
    pub service_releases_dir: PathPolicy,
    pub service_bin_dir: PathPolicy,
    pub frontend_active_path: PathPolicy,
    pub frontend_service_unit: StringPolicy,
    pub updater_service_unit: StringPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedUpdaterFilesystemPolicy {
    pub socket_path: PathBuf,
    pub journal_path: PathBuf,
    pub data_dir: PathBuf,
    pub frontend_releases_dir: PathBuf,
    pub service_releases_dir: PathBuf,
    pub service_bin_dir: PathBuf,
    pub frontend_active_path: PathBuf,
    pub frontend_service_unit: String,
    pub updater_service_unit: String,
}

impl UpdaterFilesystemPolicy {
    pub fn resolve(self) -> ResolvedUpdaterFilesystemPolicy {
        ResolvedUpdaterFilesystemPolicy {
            socket_path: self.socket_path.resolve(),
            journal_path: self.journal_path.resolve(),
            data_dir: self.data_dir.resolve(),
            frontend_releases_dir: self.frontend_releases_dir.resolve(),
            service_releases_dir: self.service_releases_dir.resolve(),
            service_bin_dir: self.service_bin_dir.resolve(),
            frontend_active_path: self.frontend_active_path.resolve(),
            frontend_service_unit: self.frontend_service_unit.resolve(),
            updater_service_unit: self.updater_service_unit.resolve(),
        }
    }
}

pub const HELIOS_UPDATER_FILESYSTEM_POLICY: UpdaterFilesystemPolicy = UpdaterFilesystemPolicy {
    socket_path: PathPolicy { env_var: "UPDATER_SOCKET", default: "/run/helios/updater.sock" },
    journal_path: PathPolicy { env_var: "UPDATER_JOURNAL_PATH", default: "/var/lib/helios/journal/updater.log" },
    data_dir: PathPolicy { env_var: "UPDATER_DATA_DIR", default: "/var/lib/helios" },
    frontend_releases_dir: PathPolicy { env_var: "UPDATER_FRONTEND_RELEASES_DIR", default: "/opt/helios/releases/frontend" },
    service_releases_dir: PathPolicy { env_var: "UPDATER_SERVICE_RELEASES_DIR", default: "/opt/helios/releases/services" },
    service_bin_dir: PathPolicy { env_var: "UPDATER_SERVICE_BIN_DIR", default: "/usr/bin" },
    frontend_active_path: PathPolicy { env_var: "UPDATER_FRONTEND_ACTIVE_PATH", default: "/opt/helios/frontend" },
    frontend_service_unit: StringPolicy { env_var: "UPDATER_FRONTEND_SERVICE_UNIT", default: "helios-frontend.service" },
    updater_service_unit: StringPolicy { env_var: "UPDATER_UPDATER_SERVICE_UNIT", default: "helios-updater.service" },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpdaterApplyPolicy {
    pub fake_apply: BoolPolicy,
    pub allow_single_slot_inplace: BoolPolicy,
    pub single_slot_requested: BoolPolicy,
    pub stream_flash_requested: BoolPolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedUpdaterApplyPolicy {
    pub fake_apply: bool,
    pub allow_single_slot_inplace: bool,
    pub single_slot_requested: bool,
    pub stream_flash_requested: bool,
}

impl UpdaterApplyPolicy {
    pub fn resolve(self) -> ResolvedUpdaterApplyPolicy {
        ResolvedUpdaterApplyPolicy {
            fake_apply: self.fake_apply.resolve(),
            allow_single_slot_inplace: self.allow_single_slot_inplace.resolve(),
            single_slot_requested: self.single_slot_requested.resolve(),
            stream_flash_requested: self.stream_flash_requested.resolve(),
        }
    }
}

pub const HELIOS_UPDATER_APPLY_POLICY: UpdaterApplyPolicy = UpdaterApplyPolicy {
    fake_apply: BoolPolicy { env_var: "UPDATER_FAKE_APPLY", default: false },
    allow_single_slot_inplace: BoolPolicy { env_var: "UPDATER_ALLOW_SINGLE_SLOT_INPLACE", default: false },
    single_slot_requested: BoolPolicy { env_var: "UPDATER_SINGLE_SLOT", default: false },
    stream_flash_requested: BoolPolicy { env_var: "UPDATER_STREAM_FLASH", default: false },
};

pub fn updater_frontend_healthcheck_url() -> Option<String> {
    match std::env::var("UPDATER_FRONTEND_HEALTHCHECK_URL") {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
        }
        Err(_) => Some("http://127.0.0.1/".into()),
    }
}

pub fn updater_api_healthcheck_url() -> Option<String> {
    match std::env::var("UPDATER_API_HEALTHCHECK_URL") {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
        }
        Err(_) => Some("http://127.0.0.1/v1".into()),
    }
}
