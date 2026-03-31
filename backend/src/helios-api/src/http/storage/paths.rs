use once_cell::sync::Lazy;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::OnceLock;
use tracing::{info, warn};

use super::health::is_mountpoint;

#[cfg(test)]
static TEST_DATA_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Resolve a writable data directory for HTTP handlers. Defaults to a temp dir
/// but can be overridden with `HELIOS_API_DATA_DIR`.
static DATA_ROOT: Lazy<PathBuf> = Lazy::new(|| {
    if let Ok(dir) = std::env::var("HELIOS_API_DATA_DIR") {
        let path = PathBuf::from(dir);
        info!(path = %path.display(), "using HELIOS_API_DATA_DIR for persistent storage");
        warn_if_unmounted_data_root(&path);
        return path;
    }
    let root = default_data_root();
    warn_if_unmounted_data_root(&root);
    root
});

fn default_data_root() -> PathBuf {
    // TEMP_SHIM: storage-temp-data-root-fallback
    // Keep the temp-dir escape hatch only until every boot path guarantees a mounted persistent data root before API start.
    let candidates = [PathBuf::from("/data/helios/api"), PathBuf::from("/var/lib/helios/api"), std::env::temp_dir().join("helios-api")];

    for candidate in candidates {
        if ensure_dir(&candidate) {
            if candidate.starts_with("/data") || candidate.starts_with("/var/lib") {
                info!(path = %candidate.display(), "using persistent data directory");
            } else {
                warn!(path = %candidate.display(), "using temporary data directory; persisted streams/pipelines may be lost on restart");
            }
            return candidate;
        }
    }

    let fallback = std::env::temp_dir().join("helios-api");
    warn!(path = %fallback.display(), "using temporary data directory; persisted streams/pipelines may be lost on restart");
    fallback
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

fn ensure_dir(path: &PathBuf) -> bool {
    std::fs::create_dir_all(path).and_then(|_| std::fs::metadata(path)).map(|meta| meta.is_dir()).unwrap_or(false)
}

fn data_root() -> PathBuf {
    #[cfg(test)]
    if let Some(path) = TEST_DATA_ROOT.get() {
        return path.clone();
    }
    DATA_ROOT.clone()
}

/// Expose the resolved data root for callers that need to locate the data partition.
pub fn data_root_path() -> PathBuf {
    data_root()
}

#[cfg(test)]
pub fn set_data_root_for_tests(path: PathBuf) {
    let _ = TEST_DATA_ROOT.set(path);
}

/// Ensure a named subdirectory exists and return its path.
pub fn ensure_subdir(name: &str) -> std::io::Result<PathBuf> {
    let dir = data_root().join(name);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Async version of [`ensure_subdir`], avoiding blocking calls inside async handlers.
pub async fn ensure_subdir_async(name: &str) -> std::io::Result<PathBuf> {
    let dir = data_root().join(name);
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
