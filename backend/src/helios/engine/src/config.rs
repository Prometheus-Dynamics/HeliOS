use std::path::PathBuf;

pub const DEFAULT_NODE_ID: &str = "node-local";
const DEFAULT_ORION_IPC_SOCKET_PATH: &str = "/run/orion/control.sock";
const DEFAULT_ORION_IPC_STREAM_SOCKET_PATH: &str = "/run/orion/control-stream.sock";
const DEFAULT_ENGINE_SOCKET_PATH: &str = "/run/helios/engine.sock";
const DEFAULT_ENGINE_STREAM_DIR: &str = "/run/helios/engine-streams";
const DEFAULT_EXECUTION_INTERVAL_MS: u64 = 250;
const DEFAULT_PLUGIN_DIRS: &[&str] = &["/usr/lib/helios/plugins/daedalus", "/var/lib/helios/plugins/daedalus"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineConfig {
    pub node_id: String,
    pub engine_socket_path: PathBuf,
    pub journal_path: Option<PathBuf>,
    pub plugin_dirs: Vec<PathBuf>,
    pub stream_dir: PathBuf,
    pub orion_ipc_socket_path: PathBuf,
    pub orion_ipc_stream_socket_path: PathBuf,
    pub execution_interval_ms: u64,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            node_id: DEFAULT_NODE_ID.to_string(),
            engine_socket_path: PathBuf::from(DEFAULT_ENGINE_SOCKET_PATH),
            journal_path: None,
            plugin_dirs: DEFAULT_PLUGIN_DIRS.iter().map(PathBuf::from).collect(),
            stream_dir: PathBuf::from(DEFAULT_ENGINE_STREAM_DIR),
            orion_ipc_socket_path: PathBuf::from(DEFAULT_ORION_IPC_SOCKET_PATH),
            orion_ipc_stream_socket_path: PathBuf::from(DEFAULT_ORION_IPC_STREAM_SOCKET_PATH),
            execution_interval_ms: DEFAULT_EXECUTION_INTERVAL_MS,
        }
    }
}

impl EngineConfig {
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
        let mut config = Self::default();

        if let Some(value) = env.get("HELIOS_NODE_ID").or_else(|| env.get("ORION_NODE_ID")) {
            config.node_id = value.clone();
        }
        if let Some(value) = env.get("ENGINE_SOCKET").or_else(|| env.get("HELIOS_ENGINE_SOCKET")) {
            config.engine_socket_path = PathBuf::from(value);
        }
        if let Some(value) = env.get("ENGINE_JOURNAL_PATH") {
            config.journal_path = Some(PathBuf::from(value));
        }
        if let Some(value) = env.get("HELIOS_DAEDALUS_PLUGIN_DIRS") {
            config.plugin_dirs = parse_path_list(value);
        }
        if let Some(value) = env.get("HELIOS_ENGINE_STREAM_DIR") {
            config.stream_dir = PathBuf::from(value);
        }
        if let Some(value) = env.get("ORION_NODE_IPC_SOCKET") {
            config.orion_ipc_socket_path = PathBuf::from(value);
        }
        if let Some(value) = env.get("ORION_NODE_IPC_STREAM_SOCKET") {
            config.orion_ipc_stream_socket_path = PathBuf::from(value);
        }
        if let Some(value) = env.get("HELIOS_ENGINE_EXECUTION_INTERVAL_MS").and_then(|value| value.parse::<u64>().ok()) {
            config.execution_interval_ms = value.max(1);
        }

        config
    }
}

fn parse_path_list(value: &str) -> Vec<PathBuf> {
    value.split(':').map(str::trim).filter(|value| !value.is_empty()).map(PathBuf::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_reads_plugin_dirs_and_orion_paths() {
        let config = EngineConfig::from_env_iter([
            ("HELIOS_NODE_ID", "node-a"),
            ("ENGINE_SOCKET", "/tmp/engine.sock"),
            ("ENGINE_JOURNAL_PATH", "/tmp/engine.log"),
            ("HELIOS_DAEDALUS_PLUGIN_DIRS", "/tmp/plugins-a:/tmp/plugins-b"),
            ("HELIOS_ENGINE_STREAM_DIR", "/tmp/engine-streams"),
            ("ORION_NODE_IPC_SOCKET", "/tmp/orion/control.sock"),
            ("ORION_NODE_IPC_STREAM_SOCKET", "/tmp/orion/control-stream.sock"),
            ("HELIOS_ENGINE_EXECUTION_INTERVAL_MS", "500"),
        ]);

        assert_eq!(config.node_id, "node-a");
        assert_eq!(config.engine_socket_path, PathBuf::from("/tmp/engine.sock"));
        assert_eq!(config.journal_path, Some(PathBuf::from("/tmp/engine.log")));
        assert_eq!(config.plugin_dirs, vec![PathBuf::from("/tmp/plugins-a"), PathBuf::from("/tmp/plugins-b")]);
        assert_eq!(config.stream_dir, PathBuf::from("/tmp/engine-streams"));
        assert_eq!(config.orion_ipc_socket_path, PathBuf::from("/tmp/orion/control.sock"));
        assert_eq!(config.orion_ipc_stream_socket_path, PathBuf::from("/tmp/orion/control-stream.sock"));
        assert_eq!(config.execution_interval_ms, 500);
    }
}
