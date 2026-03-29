use std::fs::{File, OpenOptions};
use std::path::PathBuf;

use styx::prelude::FourCc;
use uuid::Uuid;

use crate::error::{Error, Result};

use super::paths::{fallback_shmem_path, preferred_shmem_path};
use super::{build_header, shmem_capacity, HEADER_SIZE};

#[cfg(target_family = "unix")]
use std::{
    fs,
    os::unix::fs::{FileExt, PermissionsExt},
    path::Path,
};

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
        let file = OpenOptions::new().create(true).read(true).write(true).truncate(true).open(&path).map_err(|_| Error::InvalidState("shmem file open failed"))?;
        let total_len = (HEADER_SIZE + capacity) as u64;
        file.set_len(total_len).map_err(|_| Error::InvalidState("shmem set len failed"))?;
        let header = build_header(0, 0, 0, (0, 0), 0);
        write_all_at(&file, &header, 0)?;
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
        // Advance the stable even sequence every frame so readers can detect each new publish.
        let seq_complete = self.seq.wrapping_add(2) & !1;
        let seq_writing = seq_complete | 1;
        self.seq = seq_complete;

        let header_writing = build_header(seq_writing, ts, len, dims, fourcc_u32);
        let header_complete = build_header(seq_complete, ts, len, dims, fourcc_u32);

        // Mark in-progress header first so readers retry until the payload + final header land.
        write_all_at(&self.file, &header_writing, 0)?;
        write_all_at(&self.file, data, HEADER_SIZE as u64)?;
        // Re-write a stable header so readers can verify the payload is consistent.
        write_all_at(&self.file, &header_complete, 0)?;
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

#[cfg(target_family = "unix")]
fn write_all_at(file: &File, mut buf: &[u8], mut offset: u64) -> Result<()> {
    while !buf.is_empty() {
        let written = file.write_at(buf, offset).map_err(|_| Error::InvalidState("shmem write failed"))?;
        if written == 0 {
            return Err(Error::InvalidState("shmem write failed"));
        }
        buf = &buf[written..];
        offset = offset.saturating_add(written as u64);
    }
    Ok(())
}

#[cfg(not(target_family = "unix"))]
fn write_all_at(file: &File, buf: &[u8], offset: u64) -> Result<()> {
    use std::io::{Seek, SeekFrom, Write};

    let mut file = file.try_clone().map_err(|_| Error::InvalidState("shmem clone failed"))?;
    file.seek(SeekFrom::Start(offset)).map_err(|_| Error::InvalidState("shmem seek failed"))?;
    file.write_all(buf).map_err(|_| Error::InvalidState("shmem write failed"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Seek, SeekFrom};

    fn read_header(file: &mut File) -> super::super::ShmemFrameHeader {
        let mut raw = [0u8; super::super::HEADER_SIZE];
        file.seek(SeekFrom::Start(0)).expect("seek");
        file.read_exact(&mut raw).expect("read");
        super::super::parse_header(&raw).expect("parse")
    }

    #[test]
    fn stable_seq_changes_on_every_write() {
        let path = std::env::temp_dir().join(format!("helios-writer-{}.frame", Uuid::new_v4()));
        let mut writer = ShmemWriter::create_at_path(path.clone(), 1024).expect("writer");
        let fourcc = FourCc::new(*b"JPEG");

        writer.write(Some(1), Some(fourcc), (1, 1), &[1]).expect("write one");
        let mut file = File::open(&path).expect("open one");
        let header_one = read_header(&mut file);

        writer.write(Some(2), Some(fourcc), (1, 1), &[2]).expect("write two");
        let mut file = File::open(&path).expect("open two");
        let header_two = read_header(&mut file);

        assert_eq!(header_one.seq, 2);
        assert_eq!(header_two.seq, 4);
        assert_ne!(header_one.seq, header_two.seq);

        drop(writer);
        let _ = std::fs::remove_file(path);
    }
}
