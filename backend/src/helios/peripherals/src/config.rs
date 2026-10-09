use std::path::PathBuf;

pub const DEFAULT_NODE_ID: &str = "node-local";
pub const DEFAULT_IPC_DIR_ENV: &str = "HELIOS_IPC_DIR";
pub const DEFAULT_IPC_DIR_NAME: &str = "helios-ipc";
const DEFAULT_ORION_IPC_SOCKET_PATH: &str = "/run/orion/control.sock";
const DEFAULT_ORION_IPC_STREAM_SOCKET_PATH: &str = "/run/orion/control-stream.sock";
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
    /// The lemnosd subscription period, and the most often readings are republished to Orion.
    pub lemnosd_reading_interval_ms: u64,
    /// How long the first lemnosd connection waits for the service before it keeps trying in the
    /// background.
    pub lemnosd_startup_wait_ms: u64,
    pub lease_ttl_ms: u64,
    pub camera_idle_pause_ms: u64,
}

impl Default for PeripheralConfig {
    fn default() -> Self {
        let ipc_dir = default_ipc_dir();
        Self {
            node_id: DEFAULT_NODE_ID.to_string(),
            stream_dir: default_stream_dir_for(&ipc_dir),
            ipc_dir,
            orion_ipc_socket_path: PathBuf::from(DEFAULT_ORION_IPC_SOCKET_PATH),
            orion_ipc_stream_socket_path: PathBuf::from(DEFAULT_ORION_IPC_STREAM_SOCKET_PATH),
            enable_linux_probes: true,
            lemnosd_socket_path: PathBuf::from(lemnos_ipc::DEFAULT_SOCKET),
            lemnosd_reading_interval_ms: 250,
            lemnosd_startup_wait_ms: 5_000,
            lease_ttl_ms: 5_000,
            camera_idle_pause_ms: 2_000,
        }
    }
}

impl PeripheralConfig {
    pub fn from_env() -> Self {
        Self::from_env_iter(std::env::vars())
    }

    fn from_env_iter<I, K, V>(iter: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let env = iter.into_iter().map(|(k, v)| (k.into(), v.into())).collect::<std::collections::BTreeMap<String, String>>();
        let mut config = Self { node_id: env.get("HELIOS_NODE_ID").cloned().unwrap_or_else(|| DEFAULT_NODE_ID.to_string()), ..Self::default() };
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
        config
    }
}

fn default_ipc_dir() -> PathBuf {
    std::env::temp_dir().join(DEFAULT_IPC_DIR_NAME)
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
        ]);
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
    }

    #[test]
    fn config_defaults_stream_dir_from_ipc_dir_and_lemnosd_to_its_socket() {
        let config = PeripheralConfig::from_env_iter([(DEFAULT_IPC_DIR_ENV, "/tmp/helios-peripherals")]);
        assert_eq!(config.stream_dir, PathBuf::from("/tmp/helios-peripherals/streams"));
        assert_eq!(config.lemnosd_socket_path, PathBuf::from("/run/lemnos/lemnosd.sock"));
        assert_eq!(config.lemnosd_reading_interval_ms, 250);
    }
}
