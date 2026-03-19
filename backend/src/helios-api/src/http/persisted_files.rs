use std::io;
use std::path::{Path, PathBuf};

use tokio::fs;
use uuid::Uuid;

pub(crate) const DATA_ROOT: &str = "/var/lib/helios";
pub(crate) const LEGACY_HELIOS_ETC_DIR: &str = "/etc/helios";

pub(crate) fn data_root_file(name: &str) -> PathBuf {
    Path::new(DATA_ROOT).join(name)
}

pub(crate) fn legacy_helios_etc_file(name: &str) -> PathBuf {
    Path::new(LEGACY_HELIOS_ETC_DIR).join(name)
}

pub(crate) async fn read(path: &Path, fallback: Option<&Path>) -> io::Result<Vec<u8>> {
    match fs::read(path).await {
        Ok(bytes) => Ok(bytes),
        Err(err) if err.kind() == io::ErrorKind::NotFound => match fallback {
            Some(fallback) => fs::read(fallback).await,
            None => Err(err),
        },
        Err(err) => Err(err),
    }
}

pub(crate) async fn read_to_string(path: &Path, fallback: Option<&Path>) -> io::Result<String> {
    match fs::read_to_string(path).await {
        Ok(raw) => Ok(raw),
        Err(err) if err.kind() == io::ErrorKind::NotFound => match fallback {
            Some(fallback) => fs::read_to_string(fallback).await,
            None => Err(err),
        },
        Err(err) => Err(err),
    }
}

pub(crate) async fn write_mirrored(path: &Path, legacy: Option<&Path>, bytes: &[u8]) -> io::Result<()> {
    write_atomic(path, bytes).await?;
    if let Some(legacy) = legacy {
        write_atomic(legacy, bytes).await?;
    }
    Ok(())
}

pub(crate) async fn remove_mirrored(path: &Path, legacy: Option<&Path>) -> io::Result<()> {
    remove_if_exists(path).await?;
    if let Some(legacy) = legacy {
        remove_if_exists(legacy).await?;
    }
    Ok(())
}

pub(crate) async fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

pub(crate) async fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    let file_name = path.file_name().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "persisted path missing filename"))?.to_string_lossy();
    let tmp_path = path.with_file_name(format!("{file_name}.tmp-{}", Uuid::new_v4()));

    fs::write(&tmp_path, bytes).await?;
    if let Err(err) = fs::rename(&tmp_path, path).await {
        let _ = fs::remove_file(&tmp_path).await;
        return Err(err);
    }

    Ok(())
}
