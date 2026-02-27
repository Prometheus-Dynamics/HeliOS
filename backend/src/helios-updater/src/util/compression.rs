use std::ffi::OsStr;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use tokio::fs as tokio_fs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::task;

use crate::error::{Error, Result};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompressionKind {
    Gzip,
    Xz,
    Zstd,
}

pub async fn decompress_if_needed(path: &Path, out_dir: &Path) -> Result<(PathBuf, bool)> {
    match detect_compression_kind(path).map_err(Error::Io)? {
        None => Ok((path.to_path_buf(), false)),
        Some(CompressionKind::Gzip) => decompress_gzip(path, out_dir).await,
        Some(CompressionKind::Xz | CompressionKind::Zstd) => decompress_external(path, out_dir).await,
    }
}

async fn decompress_gzip(path: &Path, out_dir: &Path) -> Result<(PathBuf, bool)> {
    let name = path.file_name().and_then(OsStr::to_str).unwrap_or("image");
    let out = out_dir.join(format!("decomp-{}.raw", name));
    task::spawn_blocking({
        let path = path.to_path_buf();
        let out = out.clone();
        move || -> io::Result<()> {
            use std::io::Write;
            let reader = fs::File::open(&path)?;
            let mut decoder = GzDecoder::new(reader);
            let mut writer = fs::File::create(&out)?;
            let mut buf = [0u8; 64 * 1024];
            loop {
                let n = decoder.read(&mut buf).map_err(io::Error::other)?;
                if n == 0 {
                    break;
                }
                writer.write_all(&buf[..n])?;
            }
            writer.flush()?;
            Ok(())
        }
    })
    .await
    .map_err(|err| Error::Io(io::Error::other(err.to_string())))??;
    Ok((out, true))
}

async fn decompress_external(path: &Path, out_dir: &Path) -> Result<(PathBuf, bool)> {
    let tool = match detect_compression_kind(path).map_err(Error::Io)? {
        Some(CompressionKind::Xz) => "xz",
        Some(CompressionKind::Zstd) => "zstd",
        _ => unreachable!(),
    };
    let name = path.file_name().and_then(OsStr::to_str).unwrap_or("image");
    let out = out_dir.join(format!("decomp-{}.raw", name));
    let mut child = Command::new(tool).args(["-dc", path.to_string_lossy().as_ref()]).stdout(std::process::Stdio::piped()).spawn().map_err(Error::Io)?;
    let mut stdout = child.stdout.take().ok_or_else(|| Error::Io(io::Error::other("missing stdout")))?;
    let mut out_file = tokio_fs::File::create(&out).await.map_err(Error::Io)?;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = stdout.read(&mut buf).await.map_err(Error::Io)?;
        if n == 0 {
            break;
        }
        out_file.write_all(&buf[..n]).await.map_err(Error::Io)?;
    }
    let status = child.wait().await.map_err(Error::Io)?;
    if status.success() { Ok((out, true)) } else { Err(Error::Io(io::Error::other(format!("{} decompression failed: {:?}", tool, status)))) }
}

pub fn detect_compression_kind(path: &Path) -> io::Result<Option<CompressionKind>> {
    let mut f = fs::File::open(path)?;
    let mut head = [0u8; 6];
    let n = f.read(&mut head)?;
    let h = &head[..n.min(6)];
    if h.len() >= 2 && h[0] == 0x1F && h[1] == 0x8B {
        return Ok(Some(CompressionKind::Gzip));
    }
    if h.len() >= 6 && h[0..6] == [0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00] {
        return Ok(Some(CompressionKind::Xz));
    }
    if h.len() >= 4 && h[0..4] == [0x28, 0xB5, 0x2F, 0xFD] {
        return Ok(Some(CompressionKind::Zstd));
    }
    Ok(None)
}
