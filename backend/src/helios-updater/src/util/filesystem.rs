use std::io;
use std::path::Path;

use crate::error::{Error, Result};
use tokio::fs as tokio_fs;
use tokio::process::Command;

pub async fn ensure_directory(path: &Path) -> Result<()> {
    tokio_fs::create_dir_all(path).await.map_err(Error::Io)
}

pub async fn sync_filesystem(_path: &Path) -> Result<()> {
    let status = Command::new("sync").status().await.map_err(Error::Io)?;
    if status.success() { Ok(()) } else { Err(Error::Io(io::Error::other("sync failed"))) }
}
