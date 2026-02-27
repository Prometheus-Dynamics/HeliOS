use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};

use flate2::read::GzDecoder;
use tokio::sync::mpsc;
use tokio::task;
use tracing::warn;

use crate::error::{Error, Result};
use crate::util::compression::{CompressionKind, detect_compression_kind};

const PREFIX_READ_BYTES: usize = 1024 * 1024;
const MAX_PREFIX_BYTES: usize = 4 * 1024 * 1024;
const SECTOR_BYTES: u64 = 512;

#[derive(Debug, Clone, Copy)]
pub struct StreamFlashOutcome {
    pub used_partition: bool,
}

#[derive(Debug, Clone)]
pub struct ProgressUpdate {
    pub bytes_written: u64,
    pub total_bytes: Option<u64>,
}

#[derive(Clone)]
pub struct ProgressSender {
    sender: mpsc::Sender<ProgressUpdate>,
}

impl ProgressSender {
    pub fn new(sender: mpsc::Sender<ProgressUpdate>) -> Self {
        Self { sender }
    }

    pub(crate) fn report(&self, bytes_written: u64, total_bytes: Option<u64>) {
        let _ = self.sender.blocking_send(ProgressUpdate { bytes_written, total_bytes });
    }
}

pub async fn flash_compressed_image_to_target(path: &Path, target_device: &str, target_bytes: Option<u64>, progress: Option<ProgressSender>) -> Result<StreamFlashOutcome> {
    let path = path.to_path_buf();
    let target_device = target_device.to_string();
    task::spawn_blocking(move || flash_compressed_image_to_target_blocking(&path, &target_device, target_bytes, progress)).await.map_err(|err| Error::Io(io::Error::other(err.to_string())))?
}

fn flash_compressed_image_to_target_blocking(path: &Path, target_device: &str, target_bytes: Option<u64>, progress: Option<ProgressSender>) -> Result<StreamFlashOutcome> {
    let Some(kind) = detect_compression_kind(path).map_err(Error::Io)? else {
        return Err(Error::InvalidState("stream flash requires a compressed image".into()));
    };

    let mut reader = open_decompressed_reader(path, kind)?;
    let mut prefix = read_prefix(&mut reader.reader, PREFIX_READ_BYTES)?;

    if let Some(required) = gpt_required_bytes(&prefix) {
        if required <= MAX_PREFIX_BYTES {
            read_prefix_until(&mut reader.reader, &mut prefix, required)?;
        } else {
            warn!(required, "GPT metadata exceeds prefix cap; falling back to raw stream flash");
        }
    }

    let partition = select_partition(&prefix);
    let (offset, size, used_partition) = match partition {
        Some(part) => (part.start, Some(part.size), true),
        None => (0, None, false),
    };

    if let Some(size) = size
        && let Some(target_bytes) = target_bytes
        && size > target_bytes
    {
        return Err(Error::InvalidState(format!(
            "target partition {} is {} bytes ({} MiB) but streamed rootfs is {} bytes ({} MiB)",
            target_device,
            target_bytes,
            target_bytes / (1024 * 1024),
            size,
            size / (1024 * 1024)
        )));
    }

    let total_bytes = size.or(target_bytes);
    let stream_result = stream_to_target(&mut reader.reader, &prefix, offset, size, target_device, total_bytes, progress);
    let consumed_all = stream_result.as_ref().map(|result| result.consumed_all).unwrap_or(false);
    let finish_result = reader.finish(consumed_all);
    let stream_result = stream_result?;
    finish_result?;

    if let Some(remaining) = stream_result.remaining
        && remaining > 0
    {
        return Err(Error::InvalidState("compressed stream ended before expected partition size".into()));
    }

    Ok(StreamFlashOutcome { used_partition })
}

struct DecompressedReader {
    reader: Box<dyn Read + Send>,
    child: Option<Child>,
}

impl DecompressedReader {
    fn finish(self, consumed_all: bool) -> Result<()> {
        let Some(mut child) = self.child else {
            return Ok(());
        };
        if consumed_all {
            let status = child.wait().map_err(Error::Io)?;
            if status.success() { Ok(()) } else { Err(Error::Io(io::Error::other(format!("decompression failed: {status:?}")))) }
        } else {
            let _ = child.kill();
            let _ = child.wait();
            Ok(())
        }
    }
}

fn open_decompressed_reader(path: &Path, kind: CompressionKind) -> Result<DecompressedReader> {
    match kind {
        CompressionKind::Gzip => {
            let file = std::fs::File::open(path).map_err(Error::Io)?;
            let decoder = GzDecoder::new(file);
            Ok(DecompressedReader { reader: Box::new(decoder), child: None })
        }
        CompressionKind::Xz => spawn_external("xz", path),
        CompressionKind::Zstd => spawn_external("zstd", path),
    }
}

fn spawn_external(tool: &str, path: &Path) -> Result<DecompressedReader> {
    let mut child = Command::new(tool).args(["-dc", path.to_string_lossy().as_ref()]).stdout(Stdio::piped()).spawn().map_err(Error::Io)?;
    let stdout = child.stdout.take().ok_or_else(|| Error::Io(io::Error::other("missing decompressor stdout")))?;
    Ok(DecompressedReader { reader: Box::new(stdout), child: Some(child) })
}

struct StreamCopyResult {
    consumed_all: bool,
    remaining: Option<u64>,
}

fn stream_to_target(reader: &mut dyn Read, prefix: &[u8], offset: u64, size: Option<u64>, target_device: &str, total_bytes: Option<u64>, progress: Option<ProgressSender>) -> Result<StreamCopyResult> {
    let mut out = OpenOptions::new().write(true).open(target_device).map_err(Error::Io)?;
    let mut stream_pos = 0u64;
    let mut remaining = size;
    let mut written = 0u64;

    let wrote = process_chunk(&mut out, prefix, offset, &mut stream_pos, &mut remaining).map_err(Error::Io)?;
    if wrote > 0 {
        written = written.saturating_add(wrote as u64);
        if let Some(progress) = &progress {
            progress.report(written, total_bytes);
        }
    }

    let mut buf = [0u8; 128 * 1024];
    let mut consumed_all = false;
    loop {
        if matches!(remaining, Some(0)) {
            break;
        }
        let n = reader.read(&mut buf).map_err(Error::Io)?;
        if n == 0 {
            consumed_all = true;
            break;
        }
        let wrote = process_chunk(&mut out, &buf[..n], offset, &mut stream_pos, &mut remaining).map_err(Error::Io)?;
        if wrote > 0 {
            written = written.saturating_add(wrote as u64);
            if let Some(progress) = &progress {
                progress.report(written, total_bytes);
            }
        }
    }

    out.flush().map_err(Error::Io)?;
    out.sync_all().map_err(Error::Io)?;

    Ok(StreamCopyResult { consumed_all, remaining })
}

fn process_chunk(out: &mut std::fs::File, chunk: &[u8], offset: u64, stream_pos: &mut u64, remaining: &mut Option<u64>) -> io::Result<usize> {
    let chunk_start = *stream_pos;
    let chunk_end = chunk_start.saturating_add(chunk.len() as u64);
    let mut start_index = 0usize;

    if chunk_end <= offset {
        *stream_pos = chunk_end;
        return Ok(0);
    }
    if chunk_start < offset {
        start_index = (offset - chunk_start) as usize;
    }

    let available = chunk.len().saturating_sub(start_index);
    if available == 0 {
        *stream_pos = chunk_end;
        return Ok(0);
    }

    let write_len = match remaining {
        Some(remaining) => (*remaining).min(available as u64) as usize,
        None => available,
    };

    if write_len > 0 {
        out.write_all(&chunk[start_index..start_index + write_len])?;
        if let Some(remaining) = remaining {
            *remaining = remaining.saturating_sub(write_len as u64);
        }
    }

    *stream_pos = chunk_end;
    Ok(write_len)
}

fn read_prefix(reader: &mut dyn Read, target: usize) -> io::Result<Vec<u8>> {
    let mut prefix = Vec::with_capacity(target.min(MAX_PREFIX_BYTES));
    read_prefix_until(reader, &mut prefix, target)?;
    Ok(prefix)
}

fn read_prefix_until(reader: &mut dyn Read, prefix: &mut Vec<u8>, target: usize) -> io::Result<()> {
    let mut buf = [0u8; 64 * 1024];
    while prefix.len() < target {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        prefix.extend_from_slice(&buf[..n]);
        if prefix.len() >= MAX_PREFIX_BYTES {
            break;
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct PartitionCandidate {
    start: u64,
    size: u64,
    is_linux: bool,
}

const GPT_LINUX_FS_GUID: [u8; 16] = [0xAF, 0x3D, 0xC6, 0x0F, 0x83, 0x84, 0x72, 0x47, 0x8E, 0x79, 0x3D, 0x69, 0xD8, 0x47, 0x7D, 0xE4];

fn select_partition(prefix: &[u8]) -> Option<PartitionCandidate> {
    if let Some(candidates) = parse_gpt(prefix) {
        return pick_best_candidate(candidates);
    }
    if let Some(candidates) = parse_mbr(prefix) {
        return pick_best_candidate(candidates);
    }
    None
}

fn pick_best_candidate(candidates: Vec<PartitionCandidate>) -> Option<PartitionCandidate> {
    let mut best_linux = None;
    let mut best_any = None;
    for candidate in candidates {
        if candidate.is_linux && best_linux.map(|b: PartitionCandidate| candidate.size > b.size).unwrap_or(true) {
            best_linux = Some(candidate);
        }
        if best_any.map(|b: PartitionCandidate| candidate.size > b.size).unwrap_or(true) {
            best_any = Some(candidate);
        }
    }
    best_linux.or(best_any)
}

fn parse_gpt(prefix: &[u8]) -> Option<Vec<PartitionCandidate>> {
    let header_offset = SECTOR_BYTES as usize;
    if prefix.len() < header_offset + 92 {
        return None;
    }
    let header = &prefix[header_offset..];
    if header.get(0..8) != Some(b"EFI PART") {
        return None;
    }

    let entries_lba = read_u64_le(header.get(0x48..0x50)?)?;
    let num_entries = read_u32_le(header.get(0x50..0x54)?)? as usize;
    let entry_size = read_u32_le(header.get(0x54..0x58)?)? as usize;
    if entry_size < 56 {
        return None;
    }

    let entries_offset = (entries_lba * SECTOR_BYTES) as usize;
    let table_bytes = num_entries.saturating_mul(entry_size);
    let needed = entries_offset.saturating_add(table_bytes);
    if prefix.len() < needed {
        return None;
    }

    let mut candidates = Vec::new();
    for idx in 0..num_entries {
        let base = entries_offset.saturating_add(idx.saturating_mul(entry_size));
        let entry = prefix.get(base..base + entry_size)?;
        let entry_type = entry.get(0..16)?;
        if entry_type.iter().all(|b| *b == 0) {
            continue;
        }
        let first_lba = read_u64_le(entry.get(32..40)?)?;
        let last_lba = read_u64_le(entry.get(40..48)?)?;
        if first_lba == 0 || last_lba < first_lba {
            continue;
        }
        let size = (last_lba - first_lba + 1).saturating_mul(SECTOR_BYTES);
        if size == 0 {
            continue;
        }
        candidates.push(PartitionCandidate { start: first_lba.saturating_mul(SECTOR_BYTES), size, is_linux: entry_type == GPT_LINUX_FS_GUID });
    }

    if candidates.is_empty() { None } else { Some(candidates) }
}

fn parse_mbr(prefix: &[u8]) -> Option<Vec<PartitionCandidate>> {
    if prefix.len() < 512 {
        return None;
    }
    if prefix[510] != 0x55 || prefix[511] != 0xAA {
        return None;
    }

    let mut candidates = Vec::new();
    for idx in 0..4 {
        let base = 446 + idx * 16;
        let entry = prefix.get(base..base + 16)?;
        let part_type = entry[4];
        let start_lba = u32::from_le_bytes(entry.get(8..12)?.try_into().ok()?);
        let sectors = u32::from_le_bytes(entry.get(12..16)?.try_into().ok()?);
        if sectors == 0 {
            continue;
        }
        let size = (sectors as u64).saturating_mul(SECTOR_BYTES);
        if size == 0 {
            continue;
        }
        candidates.push(PartitionCandidate { start: (start_lba as u64).saturating_mul(SECTOR_BYTES), size, is_linux: part_type == 0x83 });
    }

    if candidates.is_empty() { None } else { Some(candidates) }
}

fn gpt_required_bytes(prefix: &[u8]) -> Option<usize> {
    let header_offset = SECTOR_BYTES as usize;
    if prefix.len() < header_offset + 92 {
        return None;
    }
    let header = &prefix[header_offset..];
    if header.get(0..8) != Some(b"EFI PART") {
        return None;
    }
    let entries_lba = read_u64_le(header.get(0x48..0x50)?)?;
    let num_entries = read_u32_le(header.get(0x50..0x54)?)? as usize;
    let entry_size = read_u32_le(header.get(0x54..0x58)?)? as usize;
    if entry_size < 56 {
        return None;
    }
    let entries_offset = (entries_lba * SECTOR_BYTES) as usize;
    let table_bytes = num_entries.checked_mul(entry_size)?;
    entries_offset.checked_add(table_bytes)
}

fn read_u64_le(bytes: &[u8]) -> Option<u64> {
    let raw: [u8; 8] = bytes.try_into().ok()?;
    Some(u64::from_le_bytes(raw))
}

fn read_u32_le(bytes: &[u8]) -> Option<u32> {
    let raw: [u8; 4] = bytes.try_into().ok()?;
    Some(u32::from_le_bytes(raw))
}
