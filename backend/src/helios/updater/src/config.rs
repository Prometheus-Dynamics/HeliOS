pub const DEFAULT_NODE_ID: &str = "node-local";
pub const DEFAULT_SERVICE_RELEASES_DIR: &str = "/var/lib/helios/releases/services";
pub const DEFAULT_SERVICE_BIN_DIR: &str = "/var/lib/helios/bin";
pub const DEFAULT_UPDATER_STATE_DIR: &str = "/var/lib/helios/updater";
pub const DEFAULT_UPDATER_STAGING_DIR: &str = "/var/lib/helios/updater/staging";
pub const DEFAULT_ORION_IPC_SOCKET_PATH: &str = "/run/orion/control.sock";
pub const DEFAULT_ORION_IPC_STREAM_SOCKET_PATH: &str = "/run/orion/control-stream.sock";
pub const DEFAULT_STORAGE_LAYOUT_ENV_PATH: &str = "/etc/helios/storage-layout.env";
pub const DEFAULT_STORAGE_LAYOUT_MANIFEST_PATH: &str = "/etc/helios/storage-layout.toml";
pub const DEFAULT_OTA_ACTIVE_PATH: &str = "/var/lib/helios/ota/active";
pub const DEFAULT_OTA_RESERVE_PATH: &str = "/var/lib/helios/ota/reserve";
pub const DEFAULT_BOOT_SUCCESS_TIMEOUT_SECS: u64 = 120;
pub const DEFAULT_HOOK_TIMEOUT_SECS: u64 = 300;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdaterConfig {
    pub node_id: String,
    pub service_releases_dir: std::path::PathBuf,
    pub service_bin_dir: std::path::PathBuf,
    pub updater_state_dir: std::path::PathBuf,
    pub updater_staging_dir: std::path::PathBuf,
    pub orion_ipc_socket_path: std::path::PathBuf,
    pub orion_ipc_stream_socket_path: std::path::PathBuf,
    pub boot_success_timeout_secs: u64,
    pub hook_timeout_secs: u64,
}

impl Default for UpdaterConfig {
    fn default() -> Self {
        Self {
            node_id: DEFAULT_NODE_ID.to_string(),
            service_releases_dir: DEFAULT_SERVICE_RELEASES_DIR.into(),
            service_bin_dir: DEFAULT_SERVICE_BIN_DIR.into(),
            updater_state_dir: DEFAULT_UPDATER_STATE_DIR.into(),
            updater_staging_dir: DEFAULT_UPDATER_STAGING_DIR.into(),
            orion_ipc_socket_path: DEFAULT_ORION_IPC_SOCKET_PATH.into(),
            orion_ipc_stream_socket_path: DEFAULT_ORION_IPC_STREAM_SOCKET_PATH.into(),
            boot_success_timeout_secs: DEFAULT_BOOT_SUCCESS_TIMEOUT_SECS,
            hook_timeout_secs: DEFAULT_HOOK_TIMEOUT_SECS,
        }
    }
}

impl UpdaterConfig {
    pub fn from_env() -> Self {
        Self::from_env_iter(std::env::vars())
    }

    fn from_env_iter<I, K, V>(iter: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let env = iter.into_iter().map(|(key, value)| (key.into(), value.into())).collect::<std::collections::BTreeMap<String, String>>();
        let mut config = Self::default();
        config.node_id = env.get("HELIOS_NODE_ID").cloned().unwrap_or_else(|| DEFAULT_NODE_ID.to_string());
        if let Some(value) = env.get("HELIOS_UPDATER_SERVICE_RELEASES_DIR").filter(|value| !value.trim().is_empty()) {
            config.service_releases_dir = value.into();
        }
        if let Some(value) = env.get("HELIOS_UPDATER_SERVICE_BIN_DIR").filter(|value| !value.trim().is_empty()) {
            config.service_bin_dir = value.into();
        }
        if let Some(value) = env.get("HELIOS_UPDATER_STATE_DIR").filter(|value| !value.trim().is_empty()) {
            config.updater_state_dir = value.into();
        }
        if let Some(value) = env.get("HELIOS_UPDATER_STAGING_DIR").filter(|value| !value.trim().is_empty()) {
            config.updater_staging_dir = value.into();
        }
        if let Some(value) = env.get("HELIOS_ORION_IPC_SOCKET").filter(|value| !value.trim().is_empty()) {
            config.orion_ipc_socket_path = value.into();
        }
        if let Some(value) = env.get("HELIOS_ORION_IPC_STREAM_SOCKET").filter(|value| !value.trim().is_empty()) {
            config.orion_ipc_stream_socket_path = value.into();
        }
        if let Some(value) = env.get("HELIOS_UPDATER_BOOT_SUCCESS_TIMEOUT_SECS").filter(|value| !value.trim().is_empty()).and_then(|value| value.parse::<u64>().ok()) {
            config.boot_success_timeout_secs = value;
        }
        if let Some(value) = env.get("HELIOS_UPDATER_HOOK_TIMEOUT_SECS").filter(|value| !value.trim().is_empty()).and_then(|value| value.parse::<u64>().ok()) {
            config.hook_timeout_secs = value;
        }
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_reads_node_id_override() {
        let config = UpdaterConfig::from_env_iter([("HELIOS_NODE_ID", "node-a")]);
        assert_eq!(config.node_id, "node-a");
    }

    #[test]
    fn config_reads_service_path_overrides() {
        let config = UpdaterConfig::from_env_iter([
            ("HELIOS_UPDATER_SERVICE_RELEASES_DIR", "/data/releases"),
            ("HELIOS_UPDATER_SERVICE_BIN_DIR", "/data/bin"),
            ("HELIOS_UPDATER_STATE_DIR", "/data/updater"),
            ("HELIOS_UPDATER_STAGING_DIR", "/data/updater/staging"),
            ("HELIOS_ORION_IPC_SOCKET", "/run/custom/control.sock"),
            ("HELIOS_ORION_IPC_STREAM_SOCKET", "/run/custom/control-stream.sock"),
            ("HELIOS_UPDATER_BOOT_SUCCESS_TIMEOUT_SECS", "600"),
            ("HELIOS_UPDATER_HOOK_TIMEOUT_SECS", "900"),
        ]);
        assert_eq!(config.service_releases_dir, std::path::PathBuf::from("/data/releases"));
        assert_eq!(config.service_bin_dir, std::path::PathBuf::from("/data/bin"));
        assert_eq!(config.updater_state_dir, std::path::PathBuf::from("/data/updater"));
        assert_eq!(config.updater_staging_dir, std::path::PathBuf::from("/data/updater/staging"));
        assert_eq!(config.orion_ipc_socket_path, std::path::PathBuf::from("/run/custom/control.sock"));
        assert_eq!(config.orion_ipc_stream_socket_path, std::path::PathBuf::from("/run/custom/control-stream.sock"));
        assert_eq!(config.boot_success_timeout_secs, 600);
        assert_eq!(config.hook_timeout_secs, 900);
    }
}
