//! Where recordings go: a mounted, writable USB drive (found under the configured roots), else
//! the fallback directory, and the free space on it against the reserve.

use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

use crate::settings::RecorderSettings;

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("recordings directory {0} is not writable")]
    NotWritable(PathBuf),
    #[error("{free} bytes free on the recordings volume, the reserve is {reserve}")]
    ReserveNotMet { free: u64, reserve: u64 },
    #[error("storage: {0}")]
    Io(String),
}

/// The directory recordings are written to now: `storage_dir` when set, else the first writable
/// mounted USB drive under `usb_roots` (a child of a root, or a grandchild such as
/// `/run/media/<user>/<label>`), else `fallback_dir`. The directory is created when missing.
pub fn resolve_dir(settings: &RecorderSettings) -> Result<PathBuf, StorageError> {
    if let Some(dir) = &settings.storage_dir {
        return ready(dir.clone());
    }
    for candidate in usb_candidates(&settings.usb_roots) {
        if writable(&candidate) {
            return ready(candidate);
        }
    }
    ready(settings.fallback_dir.clone())
}

/// Mount points one or two levels below each root (`/media/<label>`, `/run/media/<user>/<label>`).
fn usb_candidates(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for root in roots {
        for first in subdirs(root) {
            if is_mount_point(&first) {
                found.push(first.clone());
            }
            for second in subdirs(&first) {
                if is_mount_point(&second) {
                    found.push(second);
                }
            }
        }
    }
    found
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut dirs: Vec<PathBuf> = entries.filter_map(Result::ok).map(|entry| entry.path()).filter(|path| path.is_dir()).collect();
    dirs.sort();
    dirs
}

/// A directory on another device than its parent is a mount point.
fn is_mount_point(dir: &Path) -> bool {
    match (fs::metadata(dir), dir.parent().and_then(|parent| fs::metadata(parent).ok())) {
        (Ok(own), Some(parent)) => own.dev() != parent.dev(),
        _ => false,
    }
}

fn ready(dir: PathBuf) -> Result<PathBuf, StorageError> {
    fs::create_dir_all(&dir).map_err(|error| StorageError::Io(format!("{}: {error}", dir.display())))?;
    if !writable(&dir) {
        return Err(StorageError::NotWritable(dir));
    }
    Ok(dir)
}

/// Whether a file can be created in `dir` (probed with a file that is removed again).
pub fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".helios-recorder-probe-{}", std::process::id()));
    match fs::File::create(&probe) {
        Ok(_) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Bytes available to an unprivileged writer on the volume holding `dir`.
pub fn free_bytes(dir: &Path) -> Result<u64, StorageError> {
    let stat = nix::sys::statvfs::statvfs(dir).map_err(|error| StorageError::Io(format!("statvfs {}: {error}", dir.display())))?;
    Ok(stat.blocks_available() * stat.fragment_size())
}

/// Fails when the volume holds less than `reserve` bytes free.
pub fn check_reserve(dir: &Path, reserve: u64) -> Result<u64, StorageError> {
    let free = free_bytes(dir)?;
    if free < reserve {
        return Err(StorageError::ReserveNotMet { free, reserve });
    }
    Ok(free)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_is_created_and_used_when_no_usb_drive_is_mounted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let fallback = dir.path().join("recordings");
        let settings = RecorderSettings { usb_roots: vec![dir.path().join("no-media")], fallback_dir: fallback.clone(), ..RecorderSettings::default() };
        assert_eq!(resolve_dir(&settings).expect("dir"), fallback);
        assert!(fallback.is_dir());
    }

    #[test]
    fn an_explicit_storage_dir_wins() {
        let dir = tempfile::tempdir().expect("tempdir");
        let chosen = dir.path().join("usb");
        let settings = RecorderSettings { storage_dir: Some(chosen.clone()), ..RecorderSettings::default() };
        assert_eq!(resolve_dir(&settings).expect("dir"), chosen);
    }

    #[test]
    fn the_reserve_is_checked_against_free_space() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(free_bytes(dir.path()).expect("free") > 0);
        assert!(check_reserve(dir.path(), 0).is_ok());
        let refused = check_reserve(dir.path(), u64::MAX).expect_err("refused");
        assert!(matches!(refused, StorageError::ReserveNotMet { .. }));
    }
}
