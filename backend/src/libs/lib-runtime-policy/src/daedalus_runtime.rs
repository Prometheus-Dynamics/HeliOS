use crate::BoundedU64Policy;
use crate::filesystem::{PathPolicy, SearchPathPolicy};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DaedalusRuntimePolicy {
    pub install_dir: PathPolicy,
    pub upload_dir: PathPolicy,
    pub registry_snapshot_path: PathPolicy,
    pub registry_generator_binary: PathPolicy,
    pub plugin_search_dirs: SearchPathPolicy,
    pub max_plugin_upload_mb: BoundedU64Policy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedDaedalusRuntimePolicy {
    pub install_dir: PathBuf,
    pub upload_dir: PathBuf,
    pub registry_snapshot_path: PathBuf,
    pub registry_generator_binary: PathBuf,
    pub plugin_search_dirs: Vec<PathBuf>,
    pub max_plugin_upload_bytes: u64,
}

impl DaedalusRuntimePolicy {
    pub fn resolve(self) -> ResolvedDaedalusRuntimePolicy {
        let install_dir = self.install_dir.resolve();
        let max_plugin_upload_mb = self.max_plugin_upload_mb.resolve();
        ResolvedDaedalusRuntimePolicy {
            upload_dir: self.upload_dir.resolve(),
            registry_snapshot_path: self.registry_snapshot_path.resolve(),
            registry_generator_binary: self.registry_generator_binary.resolve(),
            plugin_search_dirs: self.plugin_search_dirs.resolve(&install_dir),
            install_dir,
            max_plugin_upload_bytes: max_plugin_upload_mb.saturating_mul(1024 * 1024),
        }
    }
}

pub const HELIOS_DAEDALUS_RUNTIME_POLICY: DaedalusRuntimePolicy = DaedalusRuntimePolicy {
    install_dir: PathPolicy { env_var: "HELIOS_DAEDALUS_PLUGIN_INSTALL_DIR", default: "/var/lib/helios/plugins/daedalus" },
    upload_dir: PathPolicy { env_var: "HELIOS_DAEDALUS_PLUGIN_UPLOAD_DIR", default: "/var/lib/helios/plugins/uploads" },
    registry_snapshot_path: PathPolicy { env_var: "HELIOS_NODE_REGISTRY_SNAPSHOT_PATH", default: "/var/lib/helios/state/node-registry.snapshot.json" },
    registry_generator_binary: PathPolicy { env_var: "HELIOS_ENGINE_BIN", default: "/usr/bin/helios-engine" },
    plugin_search_dirs: SearchPathPolicy { single_env_var: "HELIOS_DAEDALUS_PLUGIN_DIR", list_env_var: "HELIOS_DAEDALUS_PLUGIN_DIRS", system_fallback: "/usr/lib/helios/plugins/daedalus" },
    max_plugin_upload_mb: BoundedU64Policy { env_var: "HELIOS_API_MAX_PLUGIN_MB", default: 64, min: 1, max: 4096 },
};
