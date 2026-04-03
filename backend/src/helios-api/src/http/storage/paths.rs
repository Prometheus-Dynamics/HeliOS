use lib_runtime_policy::HELIOS_API_DATA_ROOT_POLICY;
use once_cell::sync::Lazy;
use std::io;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::OnceLock;
use tracing::warn;

use super::health::is_mountpoint;

#[cfg(test)]
static TEST_DATA_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Resolve a writable persistent data directory for HTTP handlers. Supports the
/// explicit `HELIOS_API_DATA_DIR` override, otherwise requires the canonical
/// mounted data roots.
static DATA_ROOT: Lazy<Result<PathBuf, String>> = Lazy::new(|| resolve_data_root().map_err(|err| err.to_string()));

fn resolve_data_root() -> io::Result<PathBuf> {
    let root = HELIOS_API_DATA_ROOT_POLICY.resolve()?;
    warn_if_unmounted_data_root(&root);
    Ok(root)
}

fn warn_if_unmounted_data_root(path: &Path) {
    let mount_root = Path::new("/var/lib/helios");
    if !path.starts_with(mount_root) {
        return;
    }
    if !mount_root.exists() {
        warn!(
            data_root = %path.display(),
            mount_root = %mount_root.display(),
            "data root is under /var/lib/helios but the mount point is missing; data may land on rootfs"
        );
        return;
    }
    if !is_mountpoint(mount_root) {
        warn!(
            data_root = %path.display(),
            mount_root = %mount_root.display(),
            "data root is under /var/lib/helios but the DATA partition is not mounted; data may be hidden after mount"
        );
    }
}

fn data_root() -> io::Result<PathBuf> {
    #[cfg(test)]
    if let Some(path) = TEST_DATA_ROOT.get() {
        return Ok(path.clone());
    }
    DATA_ROOT.as_ref().cloned().map_err(|err| io::Error::other(err.clone()))
}

/// Expose the resolved data root for callers that need to locate the data partition.
pub fn data_root_path() -> io::Result<PathBuf> {
    data_root()
}

#[cfg(test)]
pub fn set_data_root_for_tests(path: PathBuf) {
    let _ = TEST_DATA_ROOT.set(path);
}

/// Ensure a named subdirectory exists and return its path.
pub fn ensure_subdir(name: &str) -> std::io::Result<PathBuf> {
    let dir = data_root()?.join(name);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Async version of [`ensure_subdir`], avoiding blocking calls inside async handlers.
pub async fn ensure_subdir_async(name: &str) -> std::io::Result<PathBuf> {
    let dir = data_root()?.join(name);
    tokio::fs::create_dir_all(&dir).await?;
    Ok(dir)
}

/// Sanitize a user-provided file name by stripping paths and trimming whitespace.
pub fn sanitize_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    std::path::Path::new(trimmed).file_name().map(|name| name.to_string_lossy().to_string())
}
