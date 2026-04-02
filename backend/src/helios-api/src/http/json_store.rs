use once_cell::sync::Lazy;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::sync::Mutex;
use uuid::Uuid;

static FILE_LOCKS: Lazy<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = Lazy::new(|| Mutex::new(HashMap::new()));

async fn lock_for(path: &Path) -> Arc<Mutex<()>> {
    let mut locks = FILE_LOCKS.lock().await;
    locks.entry(path.to_path_buf()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
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

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    let current: T = read_json_or_default(&path).await;
    let next = updater(current).await;

    let bytes = serde_json::to_vec(&next).map_err(std::io::Error::other)?;
    write_atomic(&path, &bytes).await?;
    Ok(next)
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

pub(crate) async fn write_json<T>(path: PathBuf, value: &T) -> std::io::Result<()>
where
    T: Serialize,
{
    let lock = lock_for(&path).await;
    let _guard = lock.lock().await;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    let bytes = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    write_atomic(&path, &bytes).await
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
