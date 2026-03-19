use std::io;
use std::path::Path;

use axum::http::HeaderMap;
use tokio::fs;
use tokio::io::AsyncWriteExt;

pub(crate) const EXPECTED_UPLOAD_BYTES_HEADER: &str = "x-helios-upload-bytes";

pub(crate) fn expected_upload_bytes(headers: &HeaderMap) -> Result<Option<u64>, String> {
    let Some(value) = headers.get(EXPECTED_UPLOAD_BYTES_HEADER) else {
        return Ok(None);
    };
    let raw = value.to_str().map_err(|_| format!("invalid {EXPECTED_UPLOAD_BYTES_HEADER} header"))?;
    let parsed = raw.trim().parse::<u64>().map_err(|_| format!("invalid {EXPECTED_UPLOAD_BYTES_HEADER} header"))?;
    Ok(Some(parsed))
}

pub(crate) fn validate_expected_upload_bytes(received: u64, expected: Option<u64>) -> Result<(), String> {
    if let Some(expected) = expected
        && received != expected
    {
        return Err(format!("upload truncated or incomplete: expected {expected} bytes but received {received}"));
    }
    Ok(())
}

pub(crate) async fn finalize_file_upload(file: &mut fs::File, path: &Path, written: u64) -> io::Result<u64> {
    file.flush().await?;
    file.sync_all().await?;

    let actual = fs::metadata(path).await?.len();
    if actual != written {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("stored file size mismatch after upload: wrote {written} bytes but found {actual}")));
    }

    Ok(actual)
}
