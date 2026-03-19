use axum::{
    Json,
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::time::Duration;
use tracing::warn;
use uuid::Uuid;

use crate::http::AppState;
use helios_engine::ipc::EngineErrorCode;
use helios_engine::stream::{read_latest_frame_with_header, touch_stream_preview, touch_stream_viewer};

use super::util::engine_error_body;

#[derive(Default)]
pub(crate) struct MjpegFeedsState {
    feeds: tokio::sync::Mutex<HashMap<Uuid, broadcast::Sender<Bytes>>>,
}

impl MjpegFeedsState {
    pub(crate) async fn subscribe(self: Arc<Self>, stream_id: Uuid) -> broadcast::Receiver<Bytes> {
        let mut feeds = self.feeds.lock().await;
        if let Some(sender) = feeds.get(&stream_id) {
            return sender.subscribe();
        }

        let (sender, rx) = broadcast::channel(8);
        feeds.insert(stream_id, sender.clone());
        drop(feeds);

        tokio::spawn(run_mjpeg_feed(self.clone(), stream_id, sender));
        rx
    }

    async fn remove(&self, stream_id: Uuid) {
        let mut feeds = self.feeds.lock().await;
        feeds.remove(&stream_id);
    }
}

fn mjpeg_part_header() -> Bytes {
    Bytes::from_static(b"--frame\r\nContent-Type: image/jpeg\r\n\r\n")
}

fn mjpeg_part_footer() -> Bytes {
    Bytes::from_static(b"\r\n")
}

fn mjpeg_interval() -> Duration {
    std::env::var("HELIOS_MJPEG_INTERVAL_MS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .map(Duration::from_millis)
        .map(|d| d.clamp(Duration::from_millis(20), Duration::from_millis(500)))
        .unwrap_or_else(|| Duration::from_millis(33))
}

fn mjpeg_outage() -> Duration {
    // Align with the encoded preview outage setting so both preview formats tolerate stream restarts.
    std::env::var("HELIOS_PREVIEW_OUTAGE_MS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .map(Duration::from_millis)
        .map(|d| d.clamp(Duration::from_secs(1), Duration::from_secs(15)))
        .unwrap_or_else(|| Duration::from_secs(15))
}

pub(crate) async fn mjpeg_stream(state: AppState, id: Uuid) -> Response {
    let mut rx = state.services.streams.subscribe_mjpeg_feed(id).await;
    let first_frame = match tokio::time::timeout(Duration::from_secs(2), recv_next_frame(&mut rx)).await {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(err)) => return err.into_response(),
        Err(_) => {
            return (StatusCode::NOT_FOUND, Json(engine_error_body(Some(EngineErrorCode::NotFound), "mjpeg stream unavailable"))).into_response();
        }
    };

    // Stream in three chunks per frame (header + jpeg + footer) to avoid copying the JPEG bytes
    // into a freshly allocated multipart buffer every frame.
    let body_stream = futures::stream::unfold((rx, Some(first_frame), 0u8), move |(mut rx, mut pending, phase)| async move {
        match phase {
            0 => {
                if pending.is_none() {
                    pending = match recv_next_frame(&mut rx).await {
                        Ok(bytes) => Some(bytes),
                        Err(_) => return None,
                    };
                }
                Some((Ok::<Bytes, Infallible>(mjpeg_part_header()), (rx, pending, 1)))
            }
            1 => {
                let bytes = pending?;
                Some((Ok::<Bytes, Infallible>(bytes), (rx, None, 2)))
            }
            2 => Some((Ok::<Bytes, Infallible>(mjpeg_part_footer()), (rx, None, 0))),
            _ => None,
        }
    });

    let body = Body::from_stream(body_stream);
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "multipart/x-mixed-replace; boundary=frame")
        .header(header::CACHE_CONTROL, "no-store, no-cache")
        .header(header::PRAGMA, "no-cache")
        .body(body)
        .unwrap()
}

async fn recv_next_frame(rx: &mut broadcast::Receiver<Bytes>) -> Result<Bytes, Response> {
    loop {
        match rx.recv().await {
            Ok(bytes) if !bytes.is_empty() => return Ok(bytes),
            Ok(_) => continue,
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => {
                return Err((StatusCode::NOT_FOUND, Json(engine_error_body(Some(EngineErrorCode::NotFound), "mjpeg stream closed"))).into_response());
            }
        }
    }
}

async fn run_mjpeg_feed(feeds: Arc<MjpegFeedsState>, stream_id: Uuid, sender: broadcast::Sender<Bytes>) {
    let interval = mjpeg_interval();
    let sender_for_loop = sender.clone();
    let loop_result = tokio::task::spawn_blocking(move || run_mjpeg_loop(stream_id, sender_for_loop, interval)).await;
    if let Err(err) = loop_result {
        warn!(stream_id = %stream_id, error = %err, "mjpeg feed task panicked");
    }

    feeds.remove(stream_id).await;
}

fn run_mjpeg_loop(stream_id: Uuid, sender: broadcast::Sender<Bytes>, interval: Duration) {
    let outage = mjpeg_outage();
    let mut first_unavailable_at: Option<std::time::Instant> = None;
    let mut last_touch = std::time::Instant::now().checked_sub(Duration::from_secs(10)).unwrap_or_else(std::time::Instant::now);
    while sender.receiver_count() > 0 {
        std::thread::sleep(interval);

        if last_touch.elapsed() >= Duration::from_millis(500) {
            // Keep the preview heartbeat alive so the engine continues producing preview frames.
            let _ = touch_stream_preview(stream_id);
            // Also mark a viewer heartbeat so other demand heuristics can stay consistent.
            let _ = touch_stream_viewer(stream_id);
            last_touch = std::time::Instant::now();
        }
        let (header, bytes) = match read_latest_frame_with_header(stream_id) {
            Ok((header, bytes)) => (header, bytes),
            Err(_) => {
                let now = std::time::Instant::now();
                first_unavailable_at.get_or_insert(now);
                if first_unavailable_at.is_some_and(|ts| now.saturating_duration_since(ts) >= outage) {
                    break;
                }
                continue;
            }
        };

        // During stream reconfigure/restart the preview shmem format can transiently switch away
        // from MJPEG. Keep the feed alive and let the loop recover once JPEG frames resume.
        if !matches!(&header.fourcc.to_u32().to_le_bytes(), b"MJPG" | b"JPEG") {
            let now = std::time::Instant::now();
            first_unavailable_at.get_or_insert(now);
            if first_unavailable_at.is_some_and(|ts| now.saturating_duration_since(ts) >= outage) {
                break;
            }
            continue;
        }
        first_unavailable_at = None;

        if bytes.is_empty() || sender.receiver_count() == 0 {
            continue;
        }

        // Send the raw JPEG bytes; the HTTP body wraps them with multipart boundaries per-client.
        let _ = sender.send(Bytes::from(bytes));
    }
}
