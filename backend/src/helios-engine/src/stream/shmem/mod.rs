#[cfg(feature = "runtime")]
mod cleanup;
mod paths;
mod reader;
mod viewer;
mod writer;

#[cfg(feature = "runtime")]
pub use cleanup::{cleanup_all_stream_files, cleanup_stream_files};
pub use paths::preview_heartbeat_path;
pub use paths::shmem_path;
pub use paths::viewer_heartbeat_path;
#[cfg(feature = "runtime")]
pub use reader::read_latest_frame_async;
pub use reader::{read_latest_frame, read_latest_frame_with_header, read_latest_frame_with_header_if_newer_than, read_latest_header};
pub use viewer::{preview_active_recently, touch_stream_preview, touch_stream_viewer, viewer_active_recently};
pub use writer::ShmemWriter;

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

use crate::error::{Error, Result};
use lib_runtime_policy::{ResolvedEngineShmemPolicy, HELIOS_ENGINE_SHMEM_POLICY};
use styx::prelude::FourCc;

const HEADER_SIZE: usize = 40; // magic(4) + seq(u64) + ts(u64) + len(u32) + w(u32) + h(u32) + fourcc(u32) + reserved(u32)
const MAGIC: [u8; 4] = *b"SHM1";

/// Metadata describing the latest frame stored in the shmem map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShmemFrameHeader {
    pub seq: u64,
    pub ts: u64,
    pub len: u32,
    pub width: u32,
    pub height: u32,
    pub fourcc: FourCc,
}

fn shmem_policy() -> &'static ResolvedEngineShmemPolicy {
    static VALUE: OnceLock<ResolvedEngineShmemPolicy> = OnceLock::new();
    VALUE.get_or_init(|| HELIOS_ENGINE_SHMEM_POLICY.resolve())
}

fn shmem_capacity() -> usize {
    shmem_policy().capacity_bytes
}

fn shmem_read_timeout() -> Duration {
    shmem_policy().read_timeout
}

fn shmem_read_retry() -> Duration {
    shmem_policy().read_retry
}

pub(super) fn shmem_dir_override() -> Option<&'static PathBuf> {
    shmem_policy().dir.as_ref()
}

fn build_header(seq: u64, ts: u64, len: u32, dims: (u32, u32), fourcc_u32: u32) -> [u8; HEADER_SIZE] {
    let (w, h) = dims;
    let mut header = [0u8; HEADER_SIZE];
    header[..4].copy_from_slice(&MAGIC);
    header[4..12].copy_from_slice(&seq.to_le_bytes());
    header[12..20].copy_from_slice(&ts.to_le_bytes());
    header[20..24].copy_from_slice(&len.to_le_bytes());
    header[24..28].copy_from_slice(&w.to_le_bytes());
    header[28..32].copy_from_slice(&h.to_le_bytes());
    header[32..36].copy_from_slice(&fourcc_u32.to_le_bytes());
    header
}

fn parse_header(header: &[u8; HEADER_SIZE]) -> Result<ShmemFrameHeader> {
    if header[..4] != MAGIC {
        return Err(Error::InvalidState("frame map magic mismatch"));
    }
    let seq = u64::from_le_bytes(header[4..12].try_into().unwrap());
    let ts = u64::from_le_bytes(header[12..20].try_into().unwrap());
    let len = u32::from_le_bytes(header[20..24].try_into().unwrap());
    let width = u32::from_le_bytes(header[24..28].try_into().unwrap());
    let height = u32::from_le_bytes(header[28..32].try_into().unwrap());
    let fourcc_u32 = u32::from_le_bytes(header[32..36].try_into().unwrap());
    Ok(ShmemFrameHeader { seq, ts, len, width, height, fourcc: FourCc::from(fourcc_u32) })
}
