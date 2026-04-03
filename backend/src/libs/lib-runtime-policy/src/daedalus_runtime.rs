use crate::BoundedU64Policy;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathPolicy {
    pub env_var: &'static str,
    pub default: &'static str,
}

impl PathPolicy {
    pub fn resolve(self) -> PathBuf {
        std::env::var(self.env_var).ok().map(|value| value.trim().to_string()).filter(|value| !value.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(self.default))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SearchPathPolicy {
    pub single_env_var: &'static str,
    pub list_env_var: &'static str,
    pub system_fallback: &'static str,
}

impl SearchPathPolicy {
    pub fn resolve(self, install_dir: &Path) -> Vec<PathBuf> {
        if let Ok(single) = std::env::var(self.single_env_var) {
            let trimmed = single.trim();
            if !trimmed.is_empty() {
                return vec![PathBuf::from(trimmed)];
            }
        }

        if let Ok(list) = std::env::var(self.list_env_var) {
            let dirs: Vec<_> = std::env::split_paths(&list).collect();
            if !dirs.is_empty() {
                return dirs;
            }
        }

        let mut dirs = vec![install_dir.to_path_buf(), PathBuf::from(self.system_fallback)];
        dirs.dedup();
        dirs
    }
}

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
