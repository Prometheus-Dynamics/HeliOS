use std::path::PathBuf;

use anyhow::Result;
use daedalus::engine::MetricsLevel;
use helios_layout::Layout;

pub const DEFAULT_NODE_ID: &str = "node-local";
const DEFAULT_PUBLISH_INTERVAL_MS: u64 = 250;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineConfig {
    pub node_id: String,
    pub engine_socket_path: PathBuf,
    pub journal_path: Option<PathBuf>,
    /// Where the node catalog (every node the loaded plugins declare) is written at startup, for
    /// helios-api's `/v1/catalog`; its path is on the `engine.runtime` resource.
    pub catalog_path: PathBuf,
    /// Where each pipeline's full plan is written: `plans` beside the catalog.
    pub plan_dir: PathBuf,
    /// helios-api's NetworkTables settings (`HELIOS_NT4_SETTINGS`), read when they change.
    pub nt4_settings_path: PathBuf,
    /// Where helios-api reads every tick's outputs live (`HELIOS_ENGINE_LIVE_SOCKET`).
    pub live_socket_path: PathBuf,
    pub plugin_dirs: Vec<PathBuf>,
    pub orion_ipc_socket_path: PathBuf,
    pub orion_ipc_stream_socket_path: PathBuf,
    /// Shortest time between two publications of graph outputs to Orion. Graphs run when their
    /// input arrives; this only coalesces what they produced in between into one publication.
    pub publish_interval_ms: u64,
    /// Daedalus telemetry level of workload graphs (`HELIOS_ENGINE_METRICS_LEVEL`). `Off` records
    /// nothing; `Detailed` adds per-node handler timing and per-edge waits, adapters and copies,
    /// published as each session's `metrics` artifact. Any level above `Off` also records
    /// Daedalus's frame-path overhead (`FrameOverheadReport`) into that artifact.
    pub metrics_level: MetricsLevel,
}

impl Default for EngineConfig {
    /// The image's configuration.
    fn default() -> Self {
        Self::from_layout(&Layout::image())
    }
}

impl EngineConfig {
    /// The defaults on the device whose layout this is.
    pub fn from_layout(layout: &Layout) -> Self {
        let catalog_path = layout.catalog_file();
        Self {
            node_id: DEFAULT_NODE_ID.to_string(),
            engine_socket_path: layout.engine_socket(),
            journal_path: None,
            plan_dir: plan_dir_for(&catalog_path, layout),
            catalog_path,
            nt4_settings_path: layout.nt4_settings_file(),
            live_socket_path: layout.engine_live_socket(),
            plugin_dirs: layout.plugin_dirs(),
            orion_ipc_socket_path: layout.orion_socket(),
            orion_ipc_stream_socket_path: layout.orion_stream_socket(),
            publish_interval_ms: DEFAULT_PUBLISH_INTERVAL_MS,
            metrics_level: MetricsLevel::Off,
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
        let env = iter.into_iter().map(|(k, v)| (k.into(), v.into())).collect::<std::collections::BTreeMap<String, String>>();
        let layout = Layout::from_vars(env.iter())?;
        let mut config = Self::from_layout(&layout);

        if let Some(value) = env.get("HELIOS_NODE_ID").or_else(|| env.get("ORION_NODE_ID")) {
            config.node_id = value.clone();
        }
        if let Some(value) = env.get("ENGINE_SOCKET").or_else(|| env.get("HELIOS_ENGINE_SOCKET")) {
            config.engine_socket_path = PathBuf::from(value);
        }
        if let Some(value) = env.get("ENGINE_JOURNAL_PATH") {
            config.journal_path = Some(PathBuf::from(value));
        }
        if let Some(value) = env.get("HELIOS_NT4_SETTINGS") {
            config.nt4_settings_path = PathBuf::from(value);
        }
        if let Some(value) = env.get("HELIOS_ENGINE_LIVE_SOCKET") {
            config.live_socket_path = PathBuf::from(value);
        }
        if let Some(value) = env.get("HELIOS_ENGINE_CATALOG_PATH") {
            config.catalog_path = PathBuf::from(value);
            config.plan_dir = plan_dir_for(&config.catalog_path, &layout);
        }
        if let Some(value) = env.get("HELIOS_DAEDALUS_PLUGIN_DIRS") {
            config.plugin_dirs = parse_path_list(value);
        }
        if let Some(value) = env.get("ORION_NODE_IPC_SOCKET") {
            config.orion_ipc_socket_path = PathBuf::from(value);
        }
        if let Some(value) = env.get("ORION_NODE_IPC_STREAM_SOCKET") {
            config.orion_ipc_stream_socket_path = PathBuf::from(value);
        }
        // `HELIOS_ENGINE_EXECUTION_INTERVAL_MS` is the name from when graphs were ticked on it.
        if let Some(value) = env.get("HELIOS_ENGINE_PUBLISH_INTERVAL_MS").or_else(|| env.get("HELIOS_ENGINE_EXECUTION_INTERVAL_MS")).and_then(|value| value.parse::<u64>().ok()) {
            config.publish_interval_ms = value.max(1);
        }
        if let Some(value) = env.get("HELIOS_ENGINE_METRICS_LEVEL") {
            match parse_metrics_level(value) {
                Some(level) => config.metrics_level = level,
                None => tracing::warn!(value = %value, "unknown HELIOS_ENGINE_METRICS_LEVEL; metrics stay off"),
            }
        }

        Ok(config)
    }
}

/// `plans` beside the catalog; the layout's plan directory when the catalog has no parent.
fn plan_dir_for(catalog_path: &std::path::Path, layout: &Layout) -> PathBuf {
    catalog_path.parent().map_or_else(|| layout.plans_dir(), |parent| parent.join("plans"))
}

/// `off`, `basic`, `timing`, `detailed`, `hardware`, `profile` or `trace` (case-insensitive).
pub fn parse_metrics_level(value: &str) -> Option<MetricsLevel> {
    Some(match value.trim().to_ascii_lowercase().as_str() {
        "off" | "none" | "0" | "" => MetricsLevel::Off,
        "basic" => MetricsLevel::Basic,
        "timing" => MetricsLevel::Timing,
        "detailed" => MetricsLevel::Detailed,
        "hardware" => MetricsLevel::Hardware,
        "profile" => MetricsLevel::Profile,
        "trace" => MetricsLevel::Trace,
        _ => return None,
    })
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
            ("ORION_NODE_IPC_SOCKET", "/tmp/orion/control.sock"),
            ("ORION_NODE_IPC_STREAM_SOCKET", "/tmp/orion/control-stream.sock"),
            ("HELIOS_ENGINE_PUBLISH_INTERVAL_MS", "500"),
            ("HELIOS_ENGINE_METRICS_LEVEL", "Detailed"),
        ])
        .expect("config");

        assert_eq!(config.node_id, "node-a");
        assert_eq!(config.engine_socket_path, PathBuf::from("/tmp/engine.sock"));
        assert_eq!(config.journal_path, Some(PathBuf::from("/tmp/engine.log")));
        assert_eq!(config.plugin_dirs, vec![PathBuf::from("/tmp/plugins-a"), PathBuf::from("/tmp/plugins-b")]);
        assert_eq!(config.orion_ipc_socket_path, PathBuf::from("/tmp/orion/control.sock"));
        assert_eq!(config.orion_ipc_stream_socket_path, PathBuf::from("/tmp/orion/control-stream.sock"));
        assert_eq!(config.publish_interval_ms, 500);
        assert_eq!(config.metrics_level, MetricsLevel::Detailed);
    }

    #[test]
    fn config_defaults_metrics_off_and_reads_the_legacy_interval() {
        let config = EngineConfig::from_env_iter([("HELIOS_ENGINE_EXECUTION_INTERVAL_MS", "40")]).expect("config");
        assert_eq!(config.metrics_level, MetricsLevel::Off);
        assert_eq!(config.publish_interval_ms, 40);
        assert_eq!(parse_metrics_level("bogus"), None);
    }

    #[test]
    fn the_image_paths_are_the_layout_defaults() {
        let config = EngineConfig::from_env_iter(std::iter::empty::<(String, String)>()).expect("config");
        assert_eq!(config, EngineConfig::default());
        assert_eq!(config.engine_socket_path, PathBuf::from("/run/helios/engine.sock"));
        assert_eq!(config.catalog_path, PathBuf::from("/var/lib/helios/state/catalog.json"));
        assert_eq!(config.plan_dir, PathBuf::from("/var/lib/helios/state/plans"));
        assert_eq!(config.nt4_settings_path, PathBuf::from("/var/lib/helios/api/nt4.json"));
        assert_eq!(config.live_socket_path, PathBuf::from("/run/helios/engine-live.sock"));
        assert_eq!(config.plugin_dirs, vec![PathBuf::from("/usr/lib/helios/plugins/daedalus"), PathBuf::from("/var/lib/helios/plugins/daedalus")]);
    }

    #[test]
    fn a_guest_layout_moves_the_plugins_and_state_and_keeps_the_sockets() {
        let config = EngineConfig::from_env_iter([("HELIOS_LAYOUT_PROFILE", "guest")]).expect("config");
        assert_eq!(config.catalog_path, PathBuf::from("/usr/local/var/lib/helios/state/catalog.json"));
        assert_eq!(config.plan_dir, PathBuf::from("/usr/local/var/lib/helios/state/plans"));
        assert_eq!(config.plugin_dirs[0], PathBuf::from("/usr/local/lib/helios/plugins/daedalus"));
        assert_eq!(config.engine_socket_path, PathBuf::from("/run/helios/engine.sock"));
    }

    #[test]
    fn the_plan_dir_follows_a_moved_catalog() {
        let config = EngineConfig::from_env_iter([("HELIOS_ENGINE_CATALOG_PATH", "/srv/catalog.json")]).expect("config");
        assert_eq!(config.plan_dir, PathBuf::from("/srv/plans"));
    }
}
