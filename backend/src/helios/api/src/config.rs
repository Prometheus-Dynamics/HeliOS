//! Runtime configuration from the environment (`/etc/default/helios-api.env` on the device).

use std::{
    collections::BTreeMap,
    net::SocketAddr,
    path::{Path, PathBuf},
};

pub const DEFAULT_BIND: &str = "127.0.0.1:5800";
pub const DEFAULT_NODE_ID: &str = "node-local";
pub const DEFAULT_ORION_SOCKET: &str = "/run/orion/control.sock";
pub const DEFAULT_ORION_STREAM_SOCKET: &str = "/run/orion/control-stream.sock";
pub const DEFAULT_STATE_DIR: &str = "/var/lib/helios/api";
pub const DEFAULT_OTA_DIR: &str = "/var/lib/helios/ota";
pub const DEFAULT_UPDATER_DIR: &str = "/var/lib/helios/updater";
/// Written by the Raze device package (`pd-device identity --write`).
pub const DEFAULT_PD_IDENTITY_PATH: &str = "/run/pd-device/identity.json";

/// systemd units the API reports and may restart.
pub const MANAGED_UNITS: &[&str] = &["orion-node.service", "helios-engine.service", "helios-peripherals.service", "helios-api.service", "helios-updater.service"];

#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub bind: SocketAddr,
    pub node_id: String,
    pub orion_socket: PathBuf,
    pub orion_stream_socket: PathBuf,
    /// Pipeline revision history and camera mounts.
    pub state_dir: PathBuf,
    pub ota_dir: PathBuf,
    pub updater_dir: PathBuf,
    /// Where OTA uploads are staged before they are applied.
    pub upload_dir: PathBuf,
    pub pd_identity_path: PathBuf,
    /// `Access-Control-Allow-Origin` value; `None` sends no CORS headers.
    pub cors_origin: Option<String>,
    /// Largest accepted OTA upload.
    pub max_upload_bytes: u64,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            bind: DEFAULT_BIND.parse().expect("default bind address"),
            node_id: DEFAULT_NODE_ID.into(),
            orion_socket: DEFAULT_ORION_SOCKET.into(),
            orion_stream_socket: DEFAULT_ORION_STREAM_SOCKET.into(),
            state_dir: DEFAULT_STATE_DIR.into(),
            ota_dir: DEFAULT_OTA_DIR.into(),
            updater_dir: DEFAULT_UPDATER_DIR.into(),
            upload_dir: Path::new(DEFAULT_OTA_DIR).join("uploads"),
            pd_identity_path: DEFAULT_PD_IDENTITY_PATH.into(),
            cors_origin: None,
            max_upload_bytes: 8 << 30,
        }
    }
}

impl ApiConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        Self::from_vars(std::env::vars())
    }

    pub fn from_vars<I, K, V>(vars: I) -> anyhow::Result<Self>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let env: BTreeMap<String, String> = vars.into_iter().map(|(k, v)| (k.into(), v.into())).filter(|(_, v)| !v.trim().is_empty()).collect();
        let mut config = Self::default();
        if let Some(bind) = env.get("HELIOS_API_BIND_ADDR").or_else(|| env.get("HELIOS_API_BIND")) {
            config.bind = bind.parse()?;
        }
        if let Some(value) = env.get("HELIOS_NODE_ID") {
            config.node_id = value.clone();
        }
        if let Some(value) = env.get("HELIOS_ORION_IPC_SOCKET") {
            config.orion_socket = value.into();
        }
        if let Some(value) = env.get("HELIOS_ORION_IPC_STREAM_SOCKET") {
            config.orion_stream_socket = value.into();
        }
        if let Some(value) = env.get("HELIOS_API_STATE_DIR") {
            config.state_dir = value.into();
        }
        if let Some(value) = env.get("HELIOS_OTA_DIR") {
            config.ota_dir = value.into();
            config.upload_dir = config.ota_dir.join("uploads");
        }
        if let Some(value) = env.get("HELIOS_UPDATER_STATE_DIR") {
            config.updater_dir = value.into();
        }
        if let Some(value) = env.get("HELIOS_API_UPLOAD_DIR") {
            config.upload_dir = value.into();
        }
        if let Some(value) = env.get("HELIOS_PD_IDENTITY_PATH") {
            config.pd_identity_path = value.into();
        }
        if let Some(value) = env.get("HELIOS_API_CORS_ORIGIN") {
            config.cors_origin = Some(value.clone());
        }
        if let Some(value) = env.get("HELIOS_API_MAX_UPLOAD_BYTES") {
            config.max_upload_bytes = value.parse()?;
        }
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_overrides_defaults() {
        let config = ApiConfig::from_vars([("HELIOS_API_BIND", "0.0.0.0:5801"), ("HELIOS_NODE_ID", "raze-1"), ("HELIOS_OTA_DIR", "/data/ota"), ("HELIOS_API_CORS_ORIGIN", "")]).expect("config");
        assert_eq!(config.bind.port(), 5801);
        assert_eq!(config.node_id, "raze-1");
        assert_eq!(config.upload_dir, PathBuf::from("/data/ota/uploads"));
        assert_eq!(config.cors_origin, None);
    }
}
