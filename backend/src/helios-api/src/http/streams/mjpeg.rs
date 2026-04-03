use axum::{
    Json,
    body::Body,
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use bytes::Bytes;
use lib_runtime_policy::HELIOS_API_STREAMS_POLICY;
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio::time::Duration;
use tracing::warn;
use uuid::Uuid;

use crate::api_observability::{RuntimeBroadcastCounters, RuntimeTopicBroadcastSnapshot};
use crate::http::AppState;
use helios_engine::ipc::EngineErrorCode;
use helios_engine::stream::{read_latest_frame_with_header_if_newer_than, read_latest_header, touch_stream_preview};

use super::util::engine_error_body;

#[derive(Default)]
pub(crate) struct MjpegFeedsState {
    feeds: tokio::sync::Mutex<HashMap<Uuid, broadcast::Sender<Bytes>>>,
    metrics: Arc<RuntimeBroadcastCounters>,
}

pub(crate) struct MjpegFeedSubscription {
    rx: broadcast::Receiver<Bytes>,
    metrics: Arc<RuntimeBroadcastCounters>,
}

impl MjpegFeedsState {
    pub(crate) async fn subscribe(self: Arc<Self>, stream_id: Uuid) -> MjpegFeedSubscription {
        let mut feeds = self.feeds.lock().await;
        if let Some(sender) = feeds.get(&stream_id) {
            return MjpegFeedSubscription { rx: sender.subscribe(), metrics: self.metrics.clone() };
        }

        let (sender, rx) = broadcast::channel(8);
        feeds.insert(stream_id, sender.clone());
        drop(feeds);

        tokio::spawn(run_mjpeg_feed(self.clone(), stream_id, sender));
        MjpegFeedSubscription { rx, metrics: self.metrics.clone() }
    }

    async fn remove(&self, stream_id: Uuid) {
        let mut feeds = self.feeds.lock().await;
        feeds.remove(&stream_id);
    }

    pub(crate) async fn snapshot(&self) -> RuntimeTopicBroadcastSnapshot {
        let feeds = self.feeds.lock().await;
        let topics = feeds.len() as u64;
        let subscribers = feeds.values().map(|sender| sender.receiver_count() as u64).sum();
        self.metrics.snapshot_topics(topics, subscribers)
    }
}

fn mjpeg_part_header() -> Bytes {
    Bytes::from_static(b"--frame\r\nContent-Type: image/jpeg\r\n\r\n")
}

fn mjpeg_part_footer() -> Bytes {
    Bytes::from_static(b"\r\n")
}

fn mjpeg_poll_for_preview_fps(max_fps: Option<f64>) -> Duration {
    let fps = max_fps.filter(|value| value.is_finite() && *value > 0.0).unwrap_or(15.0).clamp(1.0, 120.0);
    Duration::from_secs_f64((1.0 / fps) / 4.0).clamp(Duration::from_millis(5), Duration::from_millis(100))
}

fn mjpeg_poll() -> Duration {
    let policy = HELIOS_API_STREAMS_POLICY.resolve();
    policy.mjpeg_poll_ms.or(policy.preview_poll_ms).or(policy.mjpeg_interval_ms).map(Duration::from_millis).unwrap_or_else(|| mjpeg_poll_for_preview_fps(policy.preview_max_fps))
}

fn mjpeg_outage() -> Duration {
    Duration::from_millis(HELIOS_API_STREAMS_POLICY.resolve().preview_outage_ms)
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

async fn recv_next_frame(rx: &mut MjpegFeedSubscription) -> Result<Bytes, Response> {
    loop {
        match rx.rx.recv().await {
            Ok(bytes) if !bytes.is_empty() => return Ok(bytes),
            Ok(_) => continue,
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                rx.metrics.record_lagged(skipped);
                continue;
            }
            Err(broadcast::error::RecvError::Closed) => {
                return Err((StatusCode::NOT_FOUND, Json(engine_error_body(Some(EngineErrorCode::NotFound), "mjpeg stream closed"))).into_response());
            }
        }
    }
}

async fn run_mjpeg_feed(feeds: Arc<MjpegFeedsState>, stream_id: Uuid, sender: broadcast::Sender<Bytes>) {
    let metrics = feeds.metrics.clone();
    let poll = mjpeg_poll();
    let sender_for_loop = sender.clone();
    let loop_result = tokio::task::spawn_blocking(move || run_mjpeg_loop(stream_id, sender_for_loop, poll, metrics)).await;
    if let Err(err) = loop_result {
        warn!(stream_id = %stream_id, error = %err, "mjpeg feed task panicked");
    }

    feeds.remove(stream_id).await;
}

fn run_mjpeg_loop(stream_id: Uuid, sender: broadcast::Sender<Bytes>, poll: Duration, metrics: Arc<RuntimeBroadcastCounters>) {
    let outage = mjpeg_outage();
    let mut first_unavailable_at: Option<std::time::Instant> = None;
    let mut last_touch = std::time::Instant::now().checked_sub(Duration::from_secs(10)).unwrap_or_else(std::time::Instant::now);
    let mut last_seq = 0u64;
    let mut saw_receiver = sender.receiver_count() > 0;
    while sender.receiver_count() > 0 {
        saw_receiver |= sender.receiver_count() > 0;
        if last_touch.elapsed() >= Duration::from_millis(500) {
            // Keep the preview heartbeat alive so the engine continues producing preview frames.
            let _ = touch_stream_preview(stream_id);
            last_touch = std::time::Instant::now();
        }

        let header = match read_latest_header(stream_id) {
            Ok(header) => header,
            Err(_) => {
                let now = std::time::Instant::now();
                first_unavailable_at.get_or_insert(now);
                if first_unavailable_at.is_some_and(|ts| now.saturating_duration_since(ts) >= outage) {
                    break;
                }
                std::thread::sleep(poll);
                continue;
            }
        };

        if header.seq == last_seq || header.len == 0 || header.fourcc.to_u32() == 0 {
            std::thread::sleep(poll);
            continue;
        }

        // During stream reconfigure/restart the preview shmem format can transiently switch away
        // from MJPEG. Keep the feed alive and let the loop recover once JPEG frames resume.
        if !matches!(&header.fourcc.to_u32().to_le_bytes(), b"MJPG" | b"JPEG") {
            let now = std::time::Instant::now();
            first_unavailable_at.get_or_insert(now);
            if first_unavailable_at.is_some_and(|ts| now.saturating_duration_since(ts) >= outage) {
                break;
            }
            std::thread::sleep(poll);
            continue;
        }

        let next = match read_latest_frame_with_header_if_newer_than(stream_id, last_seq) {
            Ok(next) => next,
            Err(_) => {
                let now = std::time::Instant::now();
                first_unavailable_at.get_or_insert(now);
                if first_unavailable_at.is_some_and(|ts| now.saturating_duration_since(ts) >= outage) {
                    break;
                }
                std::thread::sleep(poll);
                continue;
            }
        };
        let Some((frame_header, bytes)) = next else {
            std::thread::sleep(poll);
            continue;
        };

        if !matches!(&frame_header.fourcc.to_u32().to_le_bytes(), b"MJPG" | b"JPEG") {
            std::thread::sleep(poll);
            continue;
        }

        first_unavailable_at = None;
        if frame_header.seq == last_seq || bytes.is_empty() || sender.receiver_count() == 0 {
            std::thread::sleep(poll);
            continue;
        }
        last_seq = frame_header.seq;

        // Send the raw JPEG bytes; the HTTP body wraps them with multipart boundaries per-client.
        if sender.send(Bytes::from(bytes)).is_ok() {
            metrics.record_sent();
        } else {
            metrics.record_no_receiver_drop();
        }
    }

    if saw_receiver && sender.receiver_count() == 0 {
        metrics.record_idle_shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mjpeg_poll_for_preview_fps_tracks_preview_rate() {
        assert_eq!(mjpeg_poll_for_preview_fps(Some(30.0)), Duration::from_secs_f64((1.0 / 30.0) / 4.0));
        assert_eq!(mjpeg_poll_for_preview_fps(Some(15.0)), Duration::from_secs_f64((1.0 / 15.0) / 4.0));
    }

    #[test]
    fn mjpeg_poll_for_preview_fps_clamps_extremes() {
        assert_eq!(mjpeg_poll_for_preview_fps(Some(240.0)), Duration::from_millis(5));
        assert_eq!(mjpeg_poll_for_preview_fps(Some(0.0)), Duration::from_secs_f64((1.0 / 15.0) / 4.0));
    }
}
