use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Result;

pub fn ensure_dir(path: &Path) -> Result<()> {
    if !path.exists() {
        fs::create_dir_all(path)?;
    }
    Ok(())
}

pub fn write_bytes(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    let mut file = OpenOptions::new().create(true).truncate(true).write(true).open(path)?;
    file.write_all(data)?;
    file.sync_all()?;
    Ok(())
}

pub fn write_str(path: &Path, data: &str) -> Result<()> {
    write_bytes(path, data.as_bytes())
}

pub fn try_glob_copy(patterns: &[&str], base_out: &Path) -> Result<Vec<PathBuf>> {
    let mut copied = Vec::new();
    for pattern in patterns {
        for path in (glob::glob(pattern)?).flatten() {
            let rel = sanitize_rel(&path);
            let out = base_out.join(rel);
            if let Some(parent) = out.parent() {
                ensure_dir(parent)?;
            }
            if path.is_file() && fs::copy(&path, &out).is_ok() {
                copied.push(out);
            }
        }
    }
    Ok(copied)
}

pub fn tar_gz_dir(src_dir: &Path, out_file: &Path) -> Result<()> {
    let tar_gz = File::create(out_file)?;
    let enc = flate2::write::GzEncoder::new(tar_gz, flate2::Compression::new(6));
    let mut tar = tar::Builder::new(enc);
    tar.follow_symlinks(false);
    tar.append_dir_all(".", src_dir)?;
    let enc = tar.into_inner()?;
    let file = enc.finish()?;
    file.sync_all()?;
    Ok(())
}

fn sanitize_rel(path: &Path) -> PathBuf {
    let mut rel = PathBuf::new();
    for component in path.components() {
        if let std::path::Component::Normal(part) = component {
            rel.push(part);
        }
    }
    rel
}
