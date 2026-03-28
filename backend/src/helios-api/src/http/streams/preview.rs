use axum::{
    Json,
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use once_cell::sync::Lazy;
use std::convert::Infallible;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio::task;
use tokio::time::Duration;
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use crate::http::AppState;
use crate::http::streams::snapshot::capture_snapshot_jpeg_without_preview_fallback;
use helios_engine::ipc::{EngineErrorCode, RecordingSource};
use helios_engine::stream::{ShmemFrameHeader, read_latest_frame_with_header, read_latest_header, touch_stream_preview};

use super::mjpeg;
use super::types::StreamFormatInfo;
use super::util::engine_error_body;
use super::util::fourcc_to_format;

static PREVIEW_POLL: Lazy<std::time::Duration> = Lazy::new(|| {
    std::env::var("HELIOS_PREVIEW_POLL_MS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .map(std::time::Duration::from_millis)
        .map(|d| d.clamp(std::time::Duration::from_millis(5), std::time::Duration::from_millis(100)))
        .unwrap_or_else(|| std::time::Duration::from_millis(10))
});

static PREVIEW_OUTAGE: Lazy<std::time::Duration> = Lazy::new(|| {
    // When streams restart (or the engine crashes and later restores streams), the shmem preview
    // map can temporarily disappear. Keep HTTP preview connections alive long enough for the
    // stream to come back instead of failing fast and requiring manual client intervention.
    std::env::var("HELIOS_PREVIEW_OUTAGE_MS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .map(std::time::Duration::from_millis)
        // Cap aggressively: if preview is down for more than ~15s, something is wrong and we
        // want clients to reconnect (and surface an error) rather than hanging forever.
        .map(|d| d.clamp(std::time::Duration::from_secs(1), std::time::Duration::from_secs(15)))
        .unwrap_or_else(|| std::time::Duration::from_secs(15))
});

static PREVIEW_FORMAT_WARMUP: Lazy<std::time::Duration> = Lazy::new(|| {
    std::env::var("HELIOS_PREVIEW_FORMAT_WARMUP_MS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .map(std::time::Duration::from_millis)
        .map(|d| d.clamp(std::time::Duration::from_millis(0), std::time::Duration::from_secs(2)))
        .unwrap_or_else(|| std::time::Duration::from_millis(350))
});

fn is_mjpeg_fourcc(fourcc: styx::prelude::FourCc) -> bool {
    matches!(&fourcc.to_u32().to_le_bytes(), b"MJPG" | b"JPEG")
}

async fn prefer_jpeg_preview_header(id: Uuid, header: ShmemFrameHeader) -> ShmemFrameHeader {
    if header.len == 0 || header.fourcc.to_u32() == 0 || is_mjpeg_fourcc(header.fourcc) {
        return header;
    }
    let warmup = *PREVIEW_FORMAT_WARMUP;
    if warmup.is_zero() {
        return header;
    }
    match tokio::task::spawn_blocking(move || {
        let deadline = Instant::now() + warmup;
        let mut best = header;
        while Instant::now() < deadline {
            let _ = touch_stream_preview(id);
            if let Ok(candidate) = read_latest_header(id)
                && candidate.len > 0
                && candidate.fourcc.to_u32() != 0
            {
                if is_mjpeg_fourcc(candidate.fourcc) {
                    return candidate;
                }
                best = candidate;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        best
    })
    .await
    {
        Ok(updated) => updated,
        Err(_) => header,
    }
}

pub(crate) async fn stream_format(id: Uuid) -> Response {
    let header = match tokio::task::spawn_blocking(move || read_latest_header(id)).await {
        Ok(Ok(header)) => header,
        Ok(Err(_)) | Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };
    if header.len == 0 || header.fourcc.to_u32() == 0 {
        return StatusCode::NOT_FOUND.into_response();
    }
    Json(StreamFormatInfo { fourcc: header.fourcc.to_string(), format: fourcc_to_format(header.fourcc).to_string(), width: header.width, height: header.height }).into_response()
}

pub(crate) async fn preview_stream(state: AppState, id: Uuid) -> Response {
    let header = match tokio::time::timeout(
        Duration::from_secs(2),
        tokio::task::spawn_blocking(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                let _ = touch_stream_preview(id);
                match read_latest_header(id) {
                    Ok(header) if header.len > 0 && header.fourcc.to_u32() != 0 => return Ok(header),
                    Ok(_) | Err(_) => {
                        if Instant::now() >= deadline {
                            return read_latest_header(id);
                        }
                        std::thread::sleep(Duration::from_millis(25));
                    }
                }
            }
        }),
    )
    .await
    {
        Ok(Ok(Ok(header))) => header,
        _ => return (StatusCode::NOT_FOUND, Json(engine_error_body(Some(EngineErrorCode::NotFound), "preview unavailable"))).into_response(),
    };
    let header = prefer_jpeg_preview_header(id, header).await;
    if header.len == 0 || header.fourcc.to_u32() == 0 {
        return (StatusCode::NOT_FOUND, Json(engine_error_body(Some(EngineErrorCode::NotFound), "preview unavailable"))).into_response();
    }

    if is_mjpeg_fourcc(header.fourcc) {
        return mjpeg::mjpeg_stream(state.clone(), id).await;
    }

    let (tx, rx) = mpsc::channel::<Result<Bytes, Infallible>>(4);
    let poll = *PREVIEW_POLL;
    let outage = *PREVIEW_OUTAGE;
    task::spawn_blocking(move || {
        let mut last_seq = 0u64;
        let mut first_read_error_at: Option<Instant> = None;
        let mut last_touch = Instant::now().checked_sub(Duration::from_secs(10)).unwrap_or_else(Instant::now);
        loop {
            if tx.is_closed() {
                break;
            }
            if last_touch.elapsed() >= Duration::from_millis(500) {
                let _ = touch_stream_preview(id);
                last_touch = Instant::now();
            }
            let (hdr, payload) = match read_latest_frame_with_header(id) {
                Ok((hdr, payload)) => (hdr, payload),
                Err(_) => {
                    let now = Instant::now();
                    first_read_error_at.get_or_insert(now);
                    // Transient shmem errors can occur during reconfigure/restart; keep the
                    // preview connection alive and only give up after an extended outage.
                    if first_read_error_at.is_some_and(|ts| now.saturating_duration_since(ts) >= outage) {
                        break;
                    }
                    std::thread::sleep(poll);
                    continue;
                }
            };
            first_read_error_at = None;
            if hdr.len == 0 || hdr.fourcc.to_u32() == 0 || hdr.seq == last_seq {
                std::thread::sleep(poll);
                continue;
            }
            last_seq = hdr.seq;
            let len = payload.len().min(u32::MAX as usize) as u32;
            if tx.blocking_send(Ok(Bytes::copy_from_slice(&len.to_le_bytes()))).is_err() {
                break;
            }
            if tx.blocking_send(Ok(Bytes::from(payload))).is_err() {
                break;
            }
        }
    });

    let body = Body::from_stream(ReceiverStream::new(rx));
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CACHE_CONTROL, "no-store, no-cache")
        .header(header::PRAGMA, "no-cache")
        .header("x-encoded-fourcc", header.fourcc.to_string())
        .header("x-encoded-format", fourcc_to_format(header.fourcc))
        .header("x-encoded-width", header.width.to_string())
        .header("x-encoded-height", header.height.to_string())
        .body(body)
        .unwrap()
}

pub(crate) async fn frame_jpeg(state: AppState, id: Uuid) -> Response {
    match capture_snapshot_jpeg_without_preview_fallback(&state, id, Some(RecordingSource::Raw)).await {
        Ok(jpeg) => (StatusCode::OK, [(header::CONTENT_TYPE, "image/jpeg"), (header::CACHE_CONTROL, "no-store, no-cache")], Body::from(jpeg)).into_response(),
        Err(err) => err.into_response(),
    }
}

pub(crate) async fn latest_frame_jpeg_bytes(id: Uuid) -> Result<Vec<u8>, String> {
    match tokio::task::spawn_blocking(move || {
        // Allow enough time for a newly-started stream to produce its first preview frame.
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let _ = touch_stream_preview(id);
            match read_latest_frame_with_header(id) {
                Ok(value) => return Ok(value),
                Err(err) => {
                    if Instant::now() >= deadline {
                        return Err(err.to_string());
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
    })
    .await
    {
        Ok(Ok((header, bytes))) => preview_jpeg_from_encoded(header, bytes),
        Ok(Err(err)) => Err(err),
        Err(err) => Err(err.to_string()),
    }
}

fn preview_jpeg_from_encoded(header: ShmemFrameHeader, bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    match &header.fourcc.to_u32().to_le_bytes() {
        b"MJPG" | b"JPEG" => return Ok(bytes),
        _ => {}
    }
    Err("preview unavailable".into())
}
