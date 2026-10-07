//! Runtime configuration from the environment (`/etc/default/helios-api.env` on the device).

use std::{collections::BTreeMap, net::SocketAddr, path::PathBuf};

pub const DEFAULT_BIND: &str = "127.0.0.1:5800";
pub const DEFAULT_NODE_ID: &str = "node-local";
pub const DEFAULT_ORION_SOCKET: &str = "/run/orion/control.sock";
pub const DEFAULT_ORION_STREAM_SOCKET: &str = "/run/orion/control-stream.sock";
pub const DEFAULT_STATE_DIR: &str = "/var/lib/helios/api";
/// OTA uploads: on the data partition (`/var/lib/helios` is bind-mounted from `/data/helios`),
/// since `/run` is RAM and the device package's writer reads the image from a file.
pub const DEFAULT_UPLOAD_DIR: &str = "/var/lib/helios/updates";
/// The UI's static build (`ui/`), served next to the API on the same port.
pub const DEFAULT_UI_DIR: &str = "/usr/share/helios/ui";
/// Written by the Raze device package (`pd-device identity --write`).
pub const DEFAULT_PD_IDENTITY_PATH: &str = "/run/pd-device/identity.json";

/// systemd units the API reports and may restart.
pub const MANAGED_UNITS: &[&str] = &["orion-node.service", "helios-engine.service", "helios-peripherals.service", "helios-api.service"];

#[derive(Debug, Clone)]
pub struct ApiConfig {
    pub bind: SocketAddr,
    pub node_id: String,
    pub orion_socket: PathBuf,
    pub orion_stream_socket: PathBuf,
    /// Pipeline revision history and camera mounts.
    pub state_dir: PathBuf,
    /// Where OTA uploads are kept until they are staged.
    pub upload_dir: PathBuf,
    /// The device package's update CLI (`/usr/lib/pd-device/update`).
    pub pd_update_tool: PathBuf,
    /// Its state file (`/run/pd-device/update.json`).
    pub pd_update_status: PathBuf,
    /// Its live copy progress while staging (`/run/pd-device/update/progress`).
    pub pd_update_progress: PathBuf,
    /// Run the update CLI through `systemd-run`, outside helios-api's cgroup, so that stopping
    /// helios-api (the pre-reboot hook does) cannot kill it. Off runs it directly (tests).
    pub pd_update_systemd_run: bool,
    /// The UI's static files; `None` (or a missing directory) serves the API only.
    pub ui_dir: Option<PathBuf>,
    pub pd_identity_path: PathBuf,
    /// `Access-Control-Allow-Origin` value; `None` sends no CORS headers.
    pub cors_origin: Option<String>,
    /// Largest accepted OTA upload.
    pub max_upload_bytes: u64,
    /// Device security state (password hash, API token hashes). Absent means open.
    pub auth_file: PathBuf,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            bind: DEFAULT_BIND.parse().expect("default bind address"),
            node_id: DEFAULT_NODE_ID.into(),
            orion_socket: DEFAULT_ORION_SOCKET.into(),
            orion_stream_socket: DEFAULT_ORION_STREAM_SOCKET.into(),
            state_dir: DEFAULT_STATE_DIR.into(),
            upload_dir: DEFAULT_UPLOAD_DIR.into(),
            pd_update_tool: crate::pd_update::PD_UPDATE_TOOL.into(),
            pd_update_status: crate::pd_update::PD_UPDATE_STATUS.into(),
            pd_update_progress: crate::pd_update::PD_UPDATE_COPY_PROGRESS.into(),
            pd_update_systemd_run: true,
            ui_dir: Some(DEFAULT_UI_DIR.into()),
            pd_identity_path: DEFAULT_PD_IDENTITY_PATH.into(),
            cors_origin: None,
            max_upload_bytes: 8 << 30,
            auth_file: crate::auth_state::DEFAULT_AUTH_FILE.into(),
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
        if let Some(value) = env.get("HELIOS_API_UPLOAD_DIR") {
            config.upload_dir = value.into();
        }
        if let Some(value) = env.get("HELIOS_PD_UPDATE_TOOL") {
            config.pd_update_tool = value.into();
        }
        if let Some(value) = env.get("HELIOS_API_UI_DIR") {
            config.ui_dir = if value == "off" { None } else { Some(value.into()) };
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
        if let Some(value) = env.get(crate::auth_state::AUTH_FILE_ENV) {
            config.auth_file = value.into();
        }
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_overrides_defaults() {
        let config = ApiConfig::from_vars([
            ("HELIOS_API_BIND", "0.0.0.0:5800"),
            ("HELIOS_NODE_ID", "raze-1"),
            ("HELIOS_API_UPLOAD_DIR", "/data/updates"),
            ("HELIOS_API_CORS_ORIGIN", ""),
            ("HELIOS_API_UI_DIR", "off"),
        ])
        .expect("config");
        assert_eq!(config.bind.port(), 5800);
        assert_eq!(config.node_id, "raze-1");
        assert_eq!(config.upload_dir, PathBuf::from("/data/updates"));
        assert_eq!(config.cors_origin, None);
        assert_eq!(config.ui_dir, None);
    }
}
