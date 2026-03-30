use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::error::{Error, Result};

use super::paths::shmem_path;
#[cfg(feature = "runtime")]
use super::MAGIC;
use super::{parse_header, shmem_read_retry, shmem_read_timeout, ShmemFrameHeader, HEADER_SIZE};

fn read_raw_header(file: &mut File) -> Result<[u8; HEADER_SIZE]> {
    file.seek(SeekFrom::Start(0)).map_err(|_| Error::InvalidState("frame map seek failed"))?;
    let mut header = [0u8; HEADER_SIZE];
    file.read_exact(&mut header).map_err(|_| Error::InvalidState("frame map header read failed"))?;
    Ok(header)
}

fn wait_or_timeout(start: Instant, timeout: Duration, retry_delay: Duration) -> Result<()> {
    if start.elapsed() < timeout {
        std::thread::sleep(retry_delay);
        Ok(())
    } else {
        Err(Error::Timeout)
    }
}

fn open_frame_file(stream_id: Uuid) -> Result<File> {
    let path = shmem_path(stream_id);
    File::open(&path).map_err(|_| Error::NotFound("frame map not found"))
}

enum PayloadRead {
    Ready((ShmemFrameHeader, Vec<u8>)),
    Retry,
}

fn stable_header_loop(file: &mut File, start: Instant, timeout: Duration, retry_delay: Duration) -> Result<ShmemFrameHeader> {
    loop {
        let header = read_raw_header(file)?;
        if header.iter().all(|&b| b == 0) {
            wait_or_timeout(start, timeout, retry_delay)?;
            continue;
        }
        let hdr1 = parse_header(&header)?;
        if (hdr1.seq & 1) == 1 || hdr1.len == 0 {
            wait_or_timeout(start, timeout, retry_delay)?;
            continue;
        }

        let header2 = read_raw_header(file)?;
        let hdr2 = parse_header(&header2)?;
        if hdr1.seq == hdr2.seq && (hdr2.seq & 1) == 0 && hdr1.len == hdr2.len {
            return Ok(hdr2);
        }

        wait_or_timeout(start, timeout, retry_delay)?;
    }
}

pub fn read_latest_header(stream_id: Uuid) -> Result<ShmemFrameHeader> {
    let mut file = open_frame_file(stream_id)?;
    let start = Instant::now();
    let timeout = shmem_read_timeout();
    let retry_delay = shmem_read_retry();

    match stable_header_loop(&mut file, start, timeout, retry_delay) {
        Ok(hdr) => Ok(hdr),
        Err(Error::Timeout) => Err(Error::NotFound("frame map not initialized")),
        Err(err) => Err(err),
    }
}

fn read_payload_for_header(
    file: &mut File,
    header: ShmemFrameHeader,
    start: Instant,
    timeout: Duration,
    retry_delay: Duration,
) -> Result<PayloadRead> {
    let len = header.len as usize;
    let file_len = file.metadata().map_err(|_| Error::InvalidState("frame map metadata failed"))?.len();
    if HEADER_SIZE as u64 + len as u64 > file_len {
        if wait_or_timeout(start, timeout, retry_delay).is_ok() {
            return Ok(PayloadRead::Retry);
        }
        return Err(Error::InvalidState("frame map length invalid"));
    }

    let mut buf = vec![0u8; len];
    file.read_exact(&mut buf).map_err(|_| Error::InvalidState("frame map payload read failed"))?;

    let header2 = read_raw_header(file)?;
    let hdr2 = parse_header(&header2)?;
    let stable = header.seq == hdr2.seq && (hdr2.seq & 1) == 0 && header.len == hdr2.len;
    if stable {
        return Ok(PayloadRead::Ready((hdr2, buf)));
    }

    if wait_or_timeout(start, timeout, retry_delay).is_ok() {
        return Ok(PayloadRead::Retry);
    }
    Err(Error::InvalidState("frame map unstable"))
}

fn read_latest_frame_with_header_inner(stream_id: Uuid, last_seq: Option<u64>) -> Result<Option<(ShmemFrameHeader, Vec<u8>)>> {
    let mut file = open_frame_file(stream_id)?;
    let start = Instant::now();
    let timeout = shmem_read_timeout();
    let retry_delay = shmem_read_retry();

    loop {
        let header = match stable_header_loop(&mut file, start, timeout, retry_delay) {
            Ok(header) => header,
            Err(Error::Timeout) => return Err(Error::NotFound("frame map not initialized")),
            Err(err) => return Err(err),
        };
        if last_seq == Some(header.seq) {
            return Ok(None);
        }
        match read_payload_for_header(&mut file, header, start, timeout, retry_delay) {
            Ok(PayloadRead::Ready(frame)) => return Ok(Some(frame)),
            Ok(PayloadRead::Retry) => continue,
            Err(err) => return Err(err),
        }
    }
}

pub fn read_latest_frame_with_header_if_newer_than(stream_id: Uuid, last_seq: u64) -> Result<Option<(ShmemFrameHeader, Vec<u8>)>> {
    read_latest_frame_with_header_inner(stream_id, Some(last_seq))
}

pub fn read_latest_frame_with_header(stream_id: Uuid) -> Result<(ShmemFrameHeader, Vec<u8>)> {
    match read_latest_frame_with_header_inner(stream_id, None) {
        Ok(Some(frame)) => Ok(frame),
        Ok(None) => Err(Error::NotFound("frame map not initialized")),
        Err(err) => Err(err),
    }
}

pub fn read_latest_frame(stream_id: Uuid) -> Result<Vec<u8>> {
    read_latest_frame_with_header(stream_id).map(|(_, bytes)| bytes)
}

#[cfg(feature = "runtime")]
pub async fn read_latest_frame_async(stream_id: Uuid) -> Result<Vec<u8>> {
    use tokio::fs::File as AsyncFile;
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    use tokio::time::sleep;

    let path = shmem_path(stream_id);
    let mut file = AsyncFile::open(&path).await.map_err(|_| Error::NotFound("frame map not found"))?;
    let start = Instant::now();
    let timeout = shmem_read_timeout();
    let retry_delay = shmem_read_retry();

    loop {
        file.seek(SeekFrom::Start(0)).await.map_err(|_| Error::InvalidState("frame map seek failed"))?;
        let mut header = [0u8; HEADER_SIZE];
        if file.read_exact(&mut header).await.is_err() {
            return Err(Error::InvalidState("frame map header read failed"));
        }
        let all_zero = header.iter().all(|&b| b == 0);
        if all_zero {
            if start.elapsed() < timeout {
                sleep(retry_delay).await;
                continue;
            } else {
                return Err(Error::NotFound("frame map not initialized"));
            }
        }
        if header[..4] != MAGIC {
            return Err(Error::InvalidState("frame map magic mismatch"));
        }
        let seq1 = u64::from_le_bytes(header[4..12].try_into().unwrap());
        let len = u32::from_le_bytes(header[20..24].try_into().unwrap()) as usize;
        if (seq1 & 1) == 1 || len == 0 {
            if start.elapsed() < timeout {
                sleep(retry_delay).await;
                continue;
            } else {
                return Err(Error::NotFound("frame map not initialized"));
            }
        }
        let file_len = file.metadata().await.map_err(|_| Error::InvalidState("frame map metadata failed"))?.len();
        if HEADER_SIZE as u64 + len as u64 > file_len {
            if start.elapsed() < timeout {
                sleep(retry_delay).await;
                continue;
            } else {
                return Err(Error::InvalidState("frame map length invalid"));
            }
        }
        let mut buf = vec![0u8; len];
        file.read_exact(&mut buf).await.map_err(|_| Error::InvalidState("frame map payload read failed"))?;

        // Re-read the header to ensure the writer finished and the payload matches.
        file.seek(SeekFrom::Start(0)).await.map_err(|_| Error::InvalidState("frame map seek failed"))?;
        if file.read_exact(&mut header).await.is_err() {
            return Err(Error::InvalidState("frame map header read failed"));
        }
        let seq2 = u64::from_le_bytes(header[4..12].try_into().unwrap());
        let len2 = u32::from_le_bytes(header[20..24].try_into().unwrap()) as usize;
        let magic_ok = header[..4] == MAGIC;
        let stable = seq1 == seq2 && (seq2 & 1) == 0 && len == len2;
        if magic_ok && stable {
            return Ok(buf);
        }

        if start.elapsed() < timeout {
            sleep(retry_delay).await;
            continue;
        } else {
            return Err(Error::InvalidState("frame map unstable"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn read_blocks_until_first_frame_is_available() {
        let stream_id = Uuid::new_v4();
        let mut writer = crate::stream::ShmemWriter::create(stream_id).expect("create shmem");
        let path = writer.path().clone();

        let reader = std::thread::spawn(move || crate::stream::read_latest_frame(stream_id));

        // Give the reader a brief head start so it encounters an empty header first.
        std::thread::sleep(Duration::from_millis(100));
        let payload = b"test-frame-payload".to_vec();
        let fourcc = styx::prelude::FourCc::from_str("MJPG").unwrap();
        writer.write(Some(1), Some(fourcc), (1, 1), &payload).expect("write frame");

        let bytes = reader.join().expect("reader join").expect("reader ok");
        assert_eq!(bytes, payload);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn read_latest_frame_with_header_if_newer_than_skips_same_seq() {
        let stream_id = Uuid::new_v4();
        let mut writer = crate::stream::ShmemWriter::create(stream_id).expect("create shmem");
        let path = writer.path().clone();

        let payload = b"test-frame-payload".to_vec();
        let fourcc = styx::prelude::FourCc::from_str("MJPG").unwrap();
        writer.write(Some(1), Some(fourcc), (1, 1), &payload).expect("write frame");
        let first_seq = read_latest_header(stream_id).expect("first header").seq;

        let same = read_latest_frame_with_header_if_newer_than(stream_id, first_seq).expect("same seq read");
        assert!(same.is_none());

        writer.write(Some(2), Some(fourcc), (1, 1), &payload).expect("write second frame");
        let newer = read_latest_frame_with_header_if_newer_than(stream_id, first_seq).expect("new seq read");
        let (header, bytes) = newer.expect("newer frame");
        assert!(header.seq > first_seq);
        assert_eq!(bytes, payload);

        let _ = std::fs::remove_file(path);
    }
}
