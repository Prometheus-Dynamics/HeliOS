mod health;
mod paths;
#[cfg(test)]
mod tests;

use std::path::PathBuf;

pub(crate) type StorageHealthSnapshot = health::StorageHealthSnapshot;

pub(crate) fn probe_storage_health() -> StorageHealthSnapshot {
    health::probe_storage_health()
}

pub(crate) fn data_root_path() -> std::io::Result<PathBuf> {
    paths::data_root_path()
}

pub(crate) fn ensure_subdir(name: &str) -> std::io::Result<PathBuf> {
    paths::ensure_subdir(name)
}

pub(crate) async fn ensure_subdir_async(name: &str) -> std::io::Result<PathBuf> {
    paths::ensure_subdir_async(name).await
}

pub(crate) fn sanitize_name(raw: &str) -> Option<String> {
    paths::sanitize_name(raw)
}

#[cfg(test)]
pub(crate) fn set_data_root_for_tests(path: PathBuf) {
    paths::set_data_root_for_tests(path);
}
