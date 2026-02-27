use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::Result;

pub fn tar_gz_dir(src_dir: &Path, out_file: &Path) -> Result<()> {
    let tar_gz = File::create(out_file)?;
    let enc = flate2::write::GzEncoder::new(tar_gz, flate2::Compression::new(6));
    let mut tar = tar::Builder::new(enc);
    tar.follow_symlinks(false);
    tar.append_dir_all(".", src_dir)?;
    let enc = tar.into_inner()?; // finish tar into gzip
    let file = enc.finish()?; // finish gzip, get underlying file
                              // Ensure tarball contents hit the device before we return
    file.sync_all()?;
    sync_parent_dir(out_file)?;
    Ok(())
}

pub fn ensure_dir(path: &Path) -> Result<()> {
    if !path.exists() {
        fs::create_dir_all(path)?;
        // Best-effort: flush parent directory so the new dir entry is durable
        if let Some(parent) = path.parent() {
            sync_dir(parent)?;
        }
    }
    Ok(())
}

pub fn write_bytes(path: &Path, data: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    // Use explicit file handle + fsync to avoid delayed writeback issues
    let mut f = OpenOptions::new().create(true).truncate(true).write(true).open(path)?;
    f.write_all(data)?;
    f.sync_all()?;
    // Flush the containing directory so the file size update is persistent
    sync_parent_dir(path)?;
    Ok(())
}

pub fn write_str(path: &Path, data: &str) -> Result<()> {
    write_bytes(path, data.as_bytes())
}

pub fn try_glob_copy(patterns: &[&str], base_out: &Path) -> Result<Vec<PathBuf>> {
    let mut copied = Vec::new();
    for pat in patterns {
        for path in (glob::glob(pat)?).flatten() {
            let rel = sanitize_rel(&path);
            let out = base_out.join(rel);
            if let Some(parent) = out.parent() {
                ensure_dir(parent)?;
            }
            if path.is_file() && std::fs::copy(&path, &out).is_ok() {
                // Flush copied file and directory entry to minimize data loss
                sync_path(&out)?;
                sync_parent_dir(&out)?;
                copied.push(out);
            }
        }
    }
    Ok(copied)
}

fn sanitize_rel(p: &Path) -> PathBuf {
    // Prevent path traversal when mirroring under bundle directory
    let mut parts = Vec::new();
    for c in p.components() {
        match c {
            std::path::Component::Normal(s) => parts.push(s.to_owned()),
            std::path::Component::CurDir => {}
            std::path::Component::RootDir | std::path::Component::ParentDir | std::path::Component::Prefix(_) => {}
        }
    }
    let mut rel = PathBuf::new();
    for p in parts {
        rel.push(p);
    }
    rel
}

fn sync_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        sync_dir(parent)?;
    }
    Ok(())
}

fn sync_dir(dir: &Path) -> Result<()> {
    // Best-effort directory fsync; errors are propagated to callers
    let d = File::open(dir)?;
    d.sync_all()?;
    Ok(())
}

fn sync_path(path: &Path) -> Result<()> {
    // Fsync a file given its path
    let f = OpenOptions::new().read(true).open(path)?;
    f.sync_all()?;
    Ok(())
}
