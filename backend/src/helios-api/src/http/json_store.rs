use crate::api_observability::RuntimeLockRegistrySnapshot;
use once_cell::sync::Lazy;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::fs;
use tokio::sync::Mutex;
use uuid::Uuid;

struct JsonStoreRuntime {
    locks: Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>,
    peak_entries: AtomicU64,
    acquires: AtomicU64,
    pruned_entries: AtomicU64,
}

fn json_store_runtime() -> &'static JsonStoreRuntime {
    // This stays process-global so unrelated request handlers coordinate atomic writes for the
    // same path without threading a lock registry through every caller. The registry prunes
    // itself when the last waiter for a path releases its Arc, so it remains bounded to the
    // currently active set of in-flight writers.
    static RUNTIME: Lazy<JsonStoreRuntime> =
        Lazy::new(|| JsonStoreRuntime { locks: Mutex::new(HashMap::new()), peak_entries: AtomicU64::new(0), acquires: AtomicU64::new(0), pruned_entries: AtomicU64::new(0) });
    &RUNTIME
}

async fn lock_for(path: &Path) -> Arc<Mutex<()>> {
    let runtime = json_store_runtime();
    let mut locks = runtime.locks.lock().await;
    let lock = locks.entry(path.to_path_buf()).or_insert_with(|| Arc::new(Mutex::new(()))).clone();
    runtime.acquires.fetch_add(1, Ordering::Relaxed);
    update_peak_entries(&runtime.peak_entries, locks.len() as u64);
    lock
}

async fn release_lock(path: &Path, lock: Arc<Mutex<()>>) {
    if Arc::strong_count(&lock) != 2 {
        return;
    }
    let mut locks = json_store_runtime().locks.lock().await;
    if locks.get(path).is_some_and(|existing| Arc::ptr_eq(existing, &lock) && Arc::strong_count(existing) == 2) {
        locks.remove(path);
        json_store_runtime().pruned_entries.fetch_add(1, Ordering::Relaxed);
    }
}

fn update_peak_entries(peak: &AtomicU64, observed: u64) {
    let mut current = peak.load(Ordering::Relaxed);
    while observed > current {
        match peak.compare_exchange(current, observed, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(actual) => current = actual,
        }
    }
}

pub(crate) async fn runtime_snapshot() -> RuntimeLockRegistrySnapshot {
    let runtime = json_store_runtime();
    let active_entries = runtime.locks.lock().await.len() as u64;
    RuntimeLockRegistrySnapshot {
        active_entries,
        peak_entries: runtime.peak_entries.load(Ordering::Relaxed),
        acquires: runtime.acquires.load(Ordering::Relaxed),
        pruned_entries: runtime.pruned_entries.load(Ordering::Relaxed),
    }
}

pub(crate) async fn read_json_or_default<T>(path: &Path) -> T
where
    T: DeserializeOwned + Default,
{
    let Ok(bytes) = fs::read(path).await else {
        return T::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

pub(crate) async fn update_json<T, F, Fut>(path: PathBuf, updater: F) -> std::io::Result<T>
where
    T: Serialize + DeserializeOwned + Default,
    F: FnOnce(T) -> Fut,
    Fut: Future<Output = T>,
{
    let lock = lock_for(&path).await;
    let _guard = lock.lock().await;

    let result = async {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let current: T = read_json_or_default(&path).await;
        let next = updater(current).await;

        let bytes = serde_json::to_vec(&next).map_err(std::io::Error::other)?;
        write_atomic(&path, &bytes).await?;
        Ok(next)
    }
    .await;
    drop(_guard);
    release_lock(&path, lock).await;
    result
}

pub(crate) async fn update_bytes<T, F, Fut, D, E>(path: PathBuf, default: T, decode: D, encode: E, updater: F) -> std::io::Result<T>
where
    F: FnOnce(T) -> Fut,
    Fut: Future<Output = T>,
    D: Fn(&[u8]) -> std::io::Result<T>,
    E: Fn(&T) -> std::io::Result<Vec<u8>>,
{
    let lock = lock_for(&path).await;
    let _guard = lock.lock().await;

    let result = async {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let current = match fs::read(&path).await {
            Ok(bytes) => decode(&bytes)?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => default,
            Err(err) => return Err(err),
        };
        let next = updater(current).await;

        let bytes = encode(&next)?;
        write_atomic(&path, &bytes).await?;
        Ok(next)
    }
    .await;
    drop(_guard);
    release_lock(&path, lock).await;
    result
}

pub(crate) async fn write_json<T>(path: PathBuf, value: &T) -> std::io::Result<()>
where
    T: Serialize,
{
    let lock = lock_for(&path).await;
    let _guard = lock.lock().await;

    let result = async {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        let bytes = serde_json::to_vec(value).map_err(std::io::Error::other)?;
        write_atomic(&path, &bytes).await
    }
    .await;
    drop(_guard);
    release_lock(&path, lock).await;
    result
}

async fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let file_name = path.file_name().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "json_store path missing filename"))?.to_string_lossy();
    let tmp_path = path.with_file_name(format!("{file_name}.tmp-{}", Uuid::new_v4()));

    fs::write(&tmp_path, bytes).await?;
    if let Err(err) = fs::rename(&tmp_path, path).await {
        let _ = fs::remove_file(&tmp_path).await;
        return Err(err);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{json_store_runtime, lock_for, release_lock, runtime_snapshot};
    use std::path::PathBuf;

    #[tokio::test]
    async fn lock_registry_prunes_idle_paths() {
        let before = runtime_snapshot().await;
        let path = PathBuf::from("/tmp/helios-json-store-test.json");
        let lock = lock_for(&path).await;
        let guard = lock.lock().await;
        drop(guard);
        release_lock(&path, lock).await;

        assert!(!json_store_runtime().locks.lock().await.contains_key(&path));
        let after = runtime_snapshot().await;
        assert_eq!(after.active_entries, 0);
        assert!(after.acquires >= before.acquires + 1);
        assert!(after.pruned_entries >= before.pruned_entries + 1);
        assert!(after.peak_entries >= before.peak_entries.max(1));
    }
}
