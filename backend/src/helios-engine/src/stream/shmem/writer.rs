use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::PathBuf;

use styx::prelude::FourCc;
use uuid::Uuid;

use crate::error::{Error, Result};

use super::paths::{fallback_shmem_path, preferred_shmem_path};
use super::{build_header, shmem_capacity, HEADER_SIZE, MAGIC};

#[cfg(target_family = "unix")]
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

#[cfg(target_family = "unix")]
fn ensure_shared_dir_permissions(dir: &Path) {
    let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o1777));
}

/// Shared-memory writer exposing the latest encoded frame with a fixed header.
pub struct ShmemWriter {
    path: PathBuf,
    file: File,
    capacity: usize,
    seq: u64,
}

impl ShmemWriter {
    pub fn create(stream_id: Uuid) -> Result<Self> {
        Self::create_with_capacity(stream_id, shmem_capacity())
    }

    pub fn create_with_capacity(stream_id: Uuid, capacity: usize) -> Result<Self> {
        let preferred = preferred_shmem_path(stream_id);
        match Self::create_at_path(preferred.clone(), capacity) {
            Ok(writer) => Ok(writer),
            Err(preferred_err) => {
                let fallback = fallback_shmem_path(stream_id);
                if fallback == preferred {
                    return Err(preferred_err);
                }
                log_preferred_unavailable(&preferred, &preferred_err);
                Self::create_at_path(fallback, capacity)
            }
        }
    }

    fn create_at_path(path: PathBuf, capacity: usize) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| Error::InvalidState("shmem dir create failed"))?;
            #[cfg(target_family = "unix")]
            ensure_shared_dir_permissions(parent);
        }
        let mut file = OpenOptions::new().create(true).read(true).write(true).truncate(true).open(&path).map_err(|_| Error::InvalidState("shmem file open failed"))?;
        let total_len = (HEADER_SIZE + capacity) as u64;
        file.set_len(total_len).map_err(|_| Error::InvalidState("shmem set len failed"))?;
        // Seed the header so readers see a consistent magic even before the first frame is written.
        file.seek(SeekFrom::Start(0)).map_err(|_| Error::InvalidState("shmem seek failed"))?;
        file.write_all(&MAGIC).map_err(|_| Error::InvalidState("shmem write failed"))?;
        file.write_all(&0u64.to_le_bytes()).map_err(|_| Error::InvalidState("shmem write failed"))?; // seq
        file.write_all(&0u64.to_le_bytes()).map_err(|_| Error::InvalidState("shmem write failed"))?; // ts
        file.write_all(&0u32.to_le_bytes()).map_err(|_| Error::InvalidState("shmem write failed"))?; // len
        file.write_all(&0u32.to_le_bytes()).map_err(|_| Error::InvalidState("shmem write failed"))?; // w
        file.write_all(&0u32.to_le_bytes()).map_err(|_| Error::InvalidState("shmem write failed"))?; // h
        file.write_all(&0u32.to_le_bytes()).map_err(|_| Error::InvalidState("shmem write failed"))?; // fourcc
        file.write_all(&0u32.to_le_bytes()).map_err(|_| Error::InvalidState("shmem write failed"))?; // reserved
        file.flush().map_err(|_| Error::InvalidState("shmem flush failed"))?;
        Ok(Self { path, file, capacity, seq: 0 })
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn write(&mut self, ts: Option<u64>, fourcc: Option<FourCc>, dims: (u32, u32), data: &[u8]) -> Result<()> {
        if data.len() > self.capacity {
            return Err(Error::InvalidState("frame exceeds shmem capacity"));
        }
        let len = data.len() as u32;
        let fourcc_u32 = fourcc.map(|cc| cc.to_u32()).unwrap_or_default();
        let ts = ts.unwrap_or_default();
        // Sequence uses LSB as a seqlock: odd while writing, even when complete.
        let seq_base = self.seq.wrapping_add(1);
        self.seq = seq_base;
        let seq_writing = seq_base | 1;
        let seq_complete = (seq_base.wrapping_add(1)) & !1;

        let header_writing = build_header(seq_writing, ts, len, dims, fourcc_u32);
        let header_complete = build_header(seq_complete, ts, len, dims, fourcc_u32);

        self.file.seek(SeekFrom::Start(0)).map_err(|_| Error::InvalidState("shmem seek failed"))?;
        // Mark in-progress header first so readers retry until the payload + final header land.
        self.file.write_all(&header_writing).map_err(|_| Error::InvalidState("shmem write failed"))?;
        self.file.write_all(data).map_err(|_| Error::InvalidState("shmem write failed"))?;
        // Re-write a stable header so readers can verify the payload is consistent.
        self.file.seek(SeekFrom::Start(0)).map_err(|_| Error::InvalidState("shmem seek failed"))?;
        self.file.write_all(&header_complete).map_err(|_| Error::InvalidState("shmem write failed"))?;
        self.file.flush().map_err(|_| Error::InvalidState("shmem flush failed"))?;
        Ok(())
    }
}

impl Drop for ShmemWriter {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(feature = "runtime")]
fn log_preferred_unavailable(preferred: &PathBuf, err: &Error) {
    tracing::warn!(path = ?preferred, error = %err, "preferred shmem path unavailable; falling back");
}

#[cfg(not(feature = "runtime"))]
fn log_preferred_unavailable(_preferred: &PathBuf, _err: &Error) {}
