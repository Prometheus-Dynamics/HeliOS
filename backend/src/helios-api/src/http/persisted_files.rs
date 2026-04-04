use std::io;
use std::path::{Path, PathBuf};

use lib_runtime_policy::HELIOS_TEAM_FILE_POLICY;
use tokio::fs;
use uuid::Uuid;

pub(crate) const DATA_ROOT: &str = "/var/lib/helios";

pub(crate) fn data_root_file(name: &str) -> PathBuf {
    Path::new(DATA_ROOT).join(name)
}

pub(crate) fn team_file_path() -> PathBuf {
    HELIOS_TEAM_FILE_POLICY.resolve()
}

pub(crate) async fn read_team_number() -> io::Result<Option<u32>> {
    let path = team_file_path();
    let content = fs::read_to_string(&path).await?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let parsed = trimmed.parse::<u32>().map_err(|err| io::Error::new(io::ErrorKind::InvalidData, format!("invalid team file {}: {err}", path.display())))?;
    if parsed == 0 {
        return Ok(None);
    }
    Ok(Some(parsed))
}

pub(crate) async fn write_team_number(team: u32) -> io::Result<()> {
    let path = team_file_path();
    let body = format!("{team}\n");
    write_canonical(&path, body.as_bytes()).await
}

pub(crate) async fn clear_team_number() -> io::Result<()> {
    let path = team_file_path();
    remove_canonical(&path).await
}

pub(crate) async fn write_canonical(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic(path, bytes).await
}

pub(crate) async fn remove_canonical(path: &Path) -> io::Result<()> {
    remove_if_exists(path).await
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
