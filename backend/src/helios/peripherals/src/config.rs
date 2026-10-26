use std::{collections::BTreeMap, path::PathBuf};

use anyhow::Result;
use helios_layout::Layout;

pub const DEFAULT_NODE_ID: &str = "node-local";
pub const DEFAULT_IPC_DIR_ENV: &str = "HELIOS_IPC_DIR";
/// The client name HeliOS gives lemnosd. The fan's board `writers` must list it (Atlas's Raze
/// board.toml does).
pub const LEMNOSD_CLIENT_NAME: &str = "helios";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeripheralConfig {
    pub node_id: String,
    pub ipc_dir: PathBuf,
    pub stream_dir: PathBuf,
    pub orion_ipc_socket_path: PathBuf,
    pub orion_ipc_stream_socket_path: PathBuf,
    /// Probe local cameras (Styx).
    pub enable_linux_probes: bool,
    /// lemnosd's socket (sensors, fan, status light).
    pub lemnosd_socket_path: PathBuf,
    /// The board definition lemnosd runs (`/etc/lemnos/board.toml`): its `raw_clients` says whether
    /// HeliOS may claim raw access, which decides whether the `lemnos.raw` resource is published.
    pub lemnosd_board_file: PathBuf,
    /// The lemnosd subscription period, and the most often readings are republished to Orion.
    pub lemnosd_reading_interval_ms: u64,
    /// How long the first lemnosd connection waits for the service before it keeps trying in the
    /// background.
    pub lemnosd_startup_wait_ms: u64,
    pub lease_ttl_ms: u64,
    pub camera_idle_pause_ms: u64,
    /// The camera services carry the sensor's raw frames (16-bit Bayer, zero copy) as a companion
    /// of every processed frame, so a raw consumer (a recording) joins without restarting the
    /// capture. Costs four more front-end buffers per camera (about 8 MB at 1280x800).
    pub camera_sensor_raw: bool,
    /// Low-priority clients a camera capture is sized for beyond those connected when it starts:
    /// the preview, a calibration capture, a recording and its capture-last ring. Each costs one
    /// more set of the capture's buffers (5.8 MB at 1280x800 with NV12 + a 640x400 output).
    pub camera_spare_clients: usize,
}

impl Default for PeripheralConfig {
    /// The image's configuration.
    fn default() -> Self {
        Self::from_layout(&Layout::image())
    }
}

impl PeripheralConfig {
    /// The defaults on the device whose layout this is.
    pub fn from_layout(layout: &Layout) -> Self {
        let ipc_dir = layout.ipc_dir();
        Self {
            node_id: DEFAULT_NODE_ID.to_string(),
            stream_dir: default_stream_dir_for(&ipc_dir),
            ipc_dir,
            orion_ipc_socket_path: layout.orion_socket(),
            orion_ipc_stream_socket_path: layout.orion_stream_socket(),
            enable_linux_probes: true,
            lemnosd_socket_path: layout.lemnos_socket(),
            lemnosd_board_file: layout.lemnos_board_file(),
            lemnosd_reading_interval_ms: 250,
            lemnosd_startup_wait_ms: 5_000,
            lease_ttl_ms: 5_000,
            camera_idle_pause_ms: 2_000,
            camera_sensor_raw: true,
            camera_spare_clients: 4,
        }
    }

    pub fn from_env() -> Result<Self> {
        Self::from_env_iter(std::env::vars())
    }

    fn from_env_iter<I, K, V>(iter: I) -> Result<Self>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let env = iter.into_iter().map(|(k, v)| (k.into(), v.into())).collect::<BTreeMap<String, String>>();
        let layout = Layout::from_vars(env.iter())?;
        let mut config = Self { node_id: env.get("HELIOS_NODE_ID").cloned().unwrap_or_else(|| DEFAULT_NODE_ID.to_string()), ..Self::from_layout(&layout) };
        if let Some(ipc_dir) = env.get(DEFAULT_IPC_DIR_ENV) {
            config.ipc_dir = PathBuf::from(ipc_dir);
            config.stream_dir = default_stream_dir_for(&config.ipc_dir);
        }
        if let Some(stream_dir) = env.get("HELIOS_PERIPHERAL_STREAM_DIR") {
            config.stream_dir = PathBuf::from(stream_dir);
        }
        if let Some(path) = env.get("ORION_NODE_IPC_SOCKET") {
            config.orion_ipc_socket_path = PathBuf::from(path);
        }
        if let Some(path) = env.get("ORION_NODE_IPC_STREAM_SOCKET") {
            config.orion_ipc_stream_socket_path = PathBuf::from(path);
        }
        if let Some(value) = env.get("HELIOS_ENABLE_LINUX_PROBES") {
            config.enable_linux_probes = !matches!(value.as_str(), "0" | "false" | "False" | "FALSE" | "no" | "NO");
        }
        if let Some(path) = env.get("HELIOS_PERIPHERALS_LEMNOSD_SOCKET").filter(|path| !path.is_empty()) {
            config.lemnosd_socket_path = PathBuf::from(path);
        }
        if let Some(value) = env.get("HELIOS_PERIPHERALS_LEMNOSD_READING_MS").and_then(|v| v.parse::<u64>().ok()).filter(|value| *value > 0) {
            config.lemnosd_reading_interval_ms = value;
        }
        if let Some(value) = env.get("HELIOS_PERIPHERALS_LEMNOSD_WAIT_MS").and_then(|v| v.parse::<u64>().ok()) {
            config.lemnosd_startup_wait_ms = value;
        }
        if let Some(value) = env.get("HELIOS_LEASE_TTL_MS").or_else(|| env.get("HELIOS_CLAIM_TTL_MS")).and_then(|v| v.parse::<u64>().ok()) {
            config.lease_ttl_ms = value;
        }
        if let Some(value) = env.get("HELIOS_CAMERA_IDLE_PAUSE_MS").and_then(|v| v.parse::<u64>().ok()) {
            config.camera_idle_pause_ms = value;
        }
        if let Some(value) = env.get("HELIOS_CAMERA_SENSOR_RAW") {
            config.camera_sensor_raw = !matches!(value.as_str(), "0" | "false" | "False" | "FALSE" | "no" | "NO");
        }
        if let Some(value) = env.get("HELIOS_CAMERA_SPARE_CLIENTS").and_then(|v| v.parse::<usize>().ok()) {
            config.camera_spare_clients = value;
        }
        Ok(config)
    }
}

fn default_stream_dir_for(root: impl Into<PathBuf>) -> PathBuf {
    root.into().join("streams")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_reads_node_id_override() {
        let config = PeripheralConfig::from_env_iter([
            ("HELIOS_NODE_ID", "node-a"),
            (DEFAULT_IPC_DIR_ENV, "/tmp/helios-peripherals"),
            ("HELIOS_PERIPHERAL_STREAM_DIR", "/tmp/helios-peripherals/streams-alt"),
            ("ORION_NODE_IPC_SOCKET", "/tmp/orion/control.sock"),
            ("ORION_NODE_IPC_STREAM_SOCKET", "/tmp/orion/control-stream.sock"),
            ("HELIOS_ENABLE_LINUX_PROBES", "false"),
            ("HELIOS_PERIPHERALS_LEMNOSD_SOCKET", "/tmp/lemnos/lemnosd.sock"),
            ("HELIOS_PERIPHERALS_LEMNOSD_READING_MS", "125"),
            ("HELIOS_PERIPHERALS_LEMNOSD_WAIT_MS", "1500"),
            ("HELIOS_LEASE_TTL_MS", "7500"),
            ("HELIOS_CAMERA_IDLE_PAUSE_MS", "3000"),
            ("HELIOS_CAMERA_SENSOR_RAW", "0"),
            ("HELIOS_CAMERA_SPARE_CLIENTS", "6"),
        ])
        .expect("config");
        assert_eq!(config.node_id, "node-a");
        assert_eq!(config.ipc_dir, PathBuf::from("/tmp/helios-peripherals"));
        assert_eq!(config.stream_dir, PathBuf::from("/tmp/helios-peripherals/streams-alt"));
        assert_eq!(config.orion_ipc_socket_path, PathBuf::from("/tmp/orion/control.sock"));
        assert_eq!(config.orion_ipc_stream_socket_path, PathBuf::from("/tmp/orion/control-stream.sock"));
        assert!(!config.enable_linux_probes);
        assert_eq!(config.lemnosd_socket_path, PathBuf::from("/tmp/lemnos/lemnosd.sock"));
        assert_eq!(config.lemnosd_reading_interval_ms, 125);
        assert_eq!(config.lemnosd_startup_wait_ms, 1_500);
        assert_eq!(config.lease_ttl_ms, 7_500);
        assert_eq!(config.camera_idle_pause_ms, 3_000);
        assert!(!config.camera_sensor_raw);
        assert_eq!(config.camera_spare_clients, 6);
    }

    #[test]
    fn camera_raw_is_on_and_four_spare_clients_by_default() {
        let config = PeripheralConfig::from_env_iter([(DEFAULT_IPC_DIR_ENV, "/tmp/helios-peripherals")]).expect("config");
        assert!(config.camera_sensor_raw);
        assert_eq!(config.camera_spare_clients, 4);
    }

    #[test]
    fn config_defaults_stream_dir_from_ipc_dir_and_lemnosd_to_its_socket() {
        let config = PeripheralConfig::from_env_iter([(DEFAULT_IPC_DIR_ENV, "/tmp/helios-peripherals")]).expect("config");
        assert_eq!(config.stream_dir, PathBuf::from("/tmp/helios-peripherals/streams"));
        assert_eq!(config.lemnosd_socket_path, PathBuf::from("/run/lemnos/lemnosd.sock"));
        assert_eq!(config.lemnosd_reading_interval_ms, 250);
    }

    #[test]
    fn the_image_lemnosd_socket_is_the_lemnos_client_default() {
        // The layout pins the path (`helios-layout`); this keeps it equal to lemnos's own default.
        assert_eq!(Layout::image().lemnos_socket(), PathBuf::from(lemnos_ipc::DEFAULT_SOCKET));
    }

    #[test]
    fn a_guest_layout_keeps_the_host_sockets_and_drops_nothing_else() {
        let config = PeripheralConfig::from_env_iter([("HELIOS_LAYOUT_PROFILE", "guest")]).expect("config");
        assert_eq!(config.orion_ipc_socket_path, PathBuf::from("/run/orion/control.sock"));
        assert_eq!(config.lemnosd_socket_path, PathBuf::from("/run/lemnos/lemnosd.sock"));
        assert_eq!(config.camera_spare_clients, 4);
    }
}
