use std::io;
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
pub struct PersistentDirPolicy {
    pub env_vars: &'static [&'static str],
    pub candidates: &'static [&'static str],
}

impl PersistentDirPolicy {
    pub fn resolve(self) -> io::Result<PathBuf> {
        for env_var in self.env_vars {
            let Ok(raw) = std::env::var(env_var) else {
                continue;
            };
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                continue;
            }
            let path = PathBuf::from(trimmed);
            ensure_directory(&path).map_err(|err| io::Error::new(err.kind(), format!("failed to prepare {} from {env_var}: {err}", path.display())))?;
            return Ok(path);
        }

        let mut last_error = None;
        for candidate in self.candidates {
            let path = PathBuf::from(candidate);
            match ensure_directory(&path) {
                Ok(()) => return Ok(path),
                Err(err) => {
                    last_error = Some(io::Error::new(err.kind(), format!("failed to prepare persistent directory {}: {err}", path.display())));
                }
            }
        }

        Err(last_error.unwrap_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no persistent directory candidates configured")))
    }
}

fn ensure_directory(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)?;
    let metadata = std::fs::metadata(path)?;
    if metadata.is_dir() { Ok(()) } else { Err(io::Error::other(format!("{} is not a directory", path.display()))) }
}

pub const HELIOS_API_DATA_ROOT_POLICY: PersistentDirPolicy = PersistentDirPolicy { env_vars: &["HELIOS_API_DATA_DIR"], candidates: &["/data/helios/api", "/var/lib/helios/api"] };

pub const HELIOS_PIPELINE_DATA_ROOT_POLICY: PersistentDirPolicy =
    PersistentDirPolicy { env_vars: &["HELIOS_PIPELINE_DIR", "HELIOS_API_DATA_DIR"], candidates: &["/data/helios/api", "/var/lib/helios/api"] };

pub const HELIOS_SHADOW_RECORD_DATA_ROOT_POLICY: PersistentDirPolicy =
    PersistentDirPolicy { env_vars: &["HELIOS_SHADOW_RECORD_DIR", "HELIOS_API_DATA_DIR"], candidates: &["/data/helios/api", "/var/lib/helios/api"] };
