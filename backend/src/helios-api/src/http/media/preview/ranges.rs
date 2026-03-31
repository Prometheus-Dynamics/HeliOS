use axum::{
    body::Body,
    http::{HeaderMap, StatusCode, header},
    response::Response,
};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncSeekExt},
};
use tokio_util::io::ReaderStream;

use crate::http::error::{ApiError, ApiResult};

use super::super::support::map_io_error;

pub(crate) fn requested_range(headers: &HeaderMap) -> Option<String> {
    headers.get(header::RANGE).and_then(|value| value.to_str().ok()).map(str::trim).filter(|value| !value.is_empty()).map(ToOwned::to_owned)
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MediaRange {
    Full,
    Partial { start: u64, end: u64 },
}

pub(crate) fn parse_media_range(range_header: Option<&str>, len: u64) -> Result<MediaRange, ()> {
    let Some(raw_header) = range_header else {
        return Ok(MediaRange::Full);
    };
    if len == 0 {
        return Err(());
    }
    let raw_header = raw_header.trim();
    let Some(spec) = raw_header.strip_prefix("bytes=") else {
        return Err(());
    };
    if spec.contains(',') {
        return Err(());
    }
    let Some((start_text, end_text)) = spec.split_once('-') else {
        return Err(());
    };
    let start_text = start_text.trim();
    let end_text = end_text.trim();
    if start_text.is_empty() {
        let suffix_len = end_text.parse::<u64>().map_err(|_| ())?;
        if suffix_len == 0 {
            return Err(());
        }
        let clamped = suffix_len.min(len);
        let start = len - clamped;
        return Ok(MediaRange::Partial { start, end: len - 1 });
    }

    let start = start_text.parse::<u64>().map_err(|_| ())?;
    if start >= len {
        return Err(());
    }
    let end = if end_text.is_empty() {
        len - 1
    } else {
        let parsed_end = end_text.parse::<u64>().map_err(|_| ())?;
        if parsed_end < start {
            return Err(());
        }
        parsed_end.min(len - 1)
    };
    Ok(MediaRange::Partial { start, end })
}

pub(crate) async fn stream_media_file(mut file: fs::File, len: u64, content_type: &str, range_header: Option<&str>) -> ApiResult<Response> {
    let range = parse_media_range(range_header, len);
    if range.is_err() {
        return Response::builder()
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(header::CACHE_CONTROL, "no-store")
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CONTENT_RANGE, format!("bytes */{len}"))
            .body(Body::empty())
            .map_err(|err| ApiError::internal(format!("failed to build range response: {err}")));
    }
    match range.unwrap_or(MediaRange::Full) {
        MediaRange::Full => {
            let stream = ReaderStream::new(file);
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, content_type)
                .header(header::CACHE_CONTROL, "no-store")
                .header(header::ACCEPT_RANGES, "bytes")
                .header(header::CONTENT_LENGTH, len.to_string())
                .body(Body::from_stream(stream))
                .map_err(|err| ApiError::internal(format!("failed to stream media: {err}")))
        }
        MediaRange::Partial { start, end } => {
            file.seek(std::io::SeekFrom::Start(start)).await.map_err(|err| map_io_error(err, "failed to seek media file"))?;
            let chunk_len = end.saturating_sub(start) + 1;
            let stream = ReaderStream::new(file.take(chunk_len));
            Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header(header::CONTENT_TYPE, content_type)
                .header(header::CACHE_CONTROL, "no-store")
                .header(header::ACCEPT_RANGES, "bytes")
                .header(header::CONTENT_LENGTH, chunk_len.to_string())
                .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"))
                .body(Body::from_stream(stream))
                .map_err(|err| ApiError::internal(format!("failed to stream media range: {err}")))
        }
    }
}
