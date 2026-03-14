use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::broadcast;
use uuid::Uuid;

use crate::error::{Error, Result};

use crate::capture::{CaptureConfig, CaptureSession};
use crate::graph::GraphHandle;
use crate::ipc::{DecoderSettings, EncoderSettings};

use super::encoder_worker::EncoderWorker;
use super::ShmemWriter;

mod config;
mod encode_path;
mod pump;

pub use config::StreamRunnerConfig;

fn preview_jpeg_quality() -> u8 {
    std::env::var("HELIOS_PREVIEW_JPEG_QUALITY").ok().and_then(|raw| raw.parse::<u8>().ok()).unwrap_or(95).clamp(1, 100)
}

pub struct StreamRunner {
    stream_label: metrics::SharedString,
    pub(super) stream_id: Option<Uuid>,
    pub(super) capture_config: CaptureConfig,
    pub(super) capture_fourcc: Option<styx::prelude::FourCc>,
    pub(super) session: Option<CaptureSession>,
    pub(super) graph: GraphHandle,
    pub(super) encode_fourcc: Option<styx::prelude::FourCc>,
    pub(super) encoder_input_fourcc: Option<styx::prelude::FourCc>,
    /// Resolved encoder implementation name after codec selection (e.g. "ffmpeg", "h265_v4l2m2m").
    /// Kept separate from `encoder_id`, which remains the user's selector (e.g. "h265").
    pub(super) encoder_impl: Option<String>,
    pub(super) encoder_id: Option<String>,
    pub(super) decoder_id: Option<String>,
    pub(super) encoder_settings: Option<EncoderSettings>,
    pub(super) decoder_settings: Option<DecoderSettings>,
    pub(super) decode_fps_limit: Option<f64>,
    pub(super) encode_fps_limit: Option<f64>,
    pub(super) encode_configured: bool,
    pub(super) last_decode_wall: Option<Instant>,
    pub(super) last_encode_wall: Option<Instant>,
    pub(super) encoder_last_activity_ms: Arc<AtomicU64>,
    pub(super) codecs: Option<styx::codec::CodecRegistryHandle>,
    pub(super) encoded_tx: broadcast::Sender<crate::stream::EncodedFrame>,
    pub(super) raw_tx: broadcast::Sender<Arc<image::DynamicImage>>,
    pub(super) shmem: Option<ShmemWriter>,
    pub(super) encoder_worker: Option<EncoderWorker>,
    pub(super) decoder_stats: styx::codec::CodecStats,
    pub(super) encoder_stats: styx::codec::CodecStats,
    pub(super) capture_stats: styx::prelude::StageMetrics,
    pub(super) last_capture_ts: Option<u64>,
    pub(super) last_capture_wall: Option<Instant>,
    pub(super) capture_empty_since: Option<Instant>,
    pub(super) capture_started_wall: Option<Instant>,
    pub(super) viewer_idle_timeout: std::time::Duration,
    pub(super) viewer_check_interval: std::time::Duration,
    pub(super) last_viewer_check_wall: Option<Instant>,
    pub(super) viewer_recently_active: bool,
    pub(super) preview_encode_interval: Duration,
    pub(super) last_preview_encode_wall: Option<Instant>,
    pub(super) preview_worker: Option<PreviewWorker>,
}

pub(super) struct PreviewWorker {
    pub(super) req_tx: std::sync::mpsc::SyncSender<PreviewEncodeRequest>,
    pub(super) res_rx: std::sync::mpsc::Receiver<PreviewEncodeResult>,
    pub(super) recycle_tx: std::sync::mpsc::SyncSender<Vec<u8>>,
    join: std::thread::JoinHandle<()>,
}

pub(super) struct PreviewEncodeRequest {
    pub(super) ts: u64,
    pub(super) image: Arc<image::DynamicImage>,
    pub(super) output_resolution: Option<(u32, u32)>,
}

pub(super) struct PreviewEncodeResult {
    pub(super) ts: u64,
    pub(super) dims: (u32, u32),
    pub(super) jpeg: Vec<u8>,
}

impl PreviewWorker {
    pub(super) fn start() -> Self {
        let (req_tx, req_rx) = std::sync::mpsc::sync_channel::<PreviewEncodeRequest>(1);
        let (res_tx, res_rx) = std::sync::mpsc::sync_channel::<PreviewEncodeResult>(1);
        let (recycle_tx, recycle_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(1);
        let quality = preview_jpeg_quality();
        let join = std::thread::spawn(move || {
            use image::codecs::jpeg::JpegEncoder;
            use image::ColorType;
            use std::sync::mpsc::TrySendError;

            let mut rgb = Vec::<u8>::new();
            let mut jpeg = Vec::<u8>::new();

            while let Ok(req) = req_rx.recv() {
                if let Ok(recycled) = recycle_rx.try_recv() {
                    jpeg = recycled;
                }
                let resized = req.output_resolution.and_then(|(target_width, target_height)| {
                    let target_width = target_width.max(1);
                    let target_height = target_height.max(1);
                    if target_width == req.image.width() && target_height == req.image.height() {
                        None
                    } else {
                        Some(req.image.resize_exact(target_width, target_height, image::imageops::FilterType::Triangle))
                    }
                });
                let source: &image::DynamicImage = resized.as_ref().unwrap_or(req.image.as_ref());

                let width = source.width().max(1);
                let height = source.height().max(1);
                let wanted = width as usize * height as usize * 3;
                if rgb.len() != wanted {
                    rgb.resize(wanted, 0);
                }
                if !write_rgb24(source, &mut rgb) {
                    continue;
                }

                jpeg.clear();
                // `Vec<u8>` implements `Write`; keep capacity to avoid allocator churn.
                let mut enc = JpegEncoder::new_with_quality(&mut jpeg, quality);
                if enc.encode(&rgb, width, height, ColorType::Rgb8.into()).is_err() {
                    continue;
                }
                let dims = (width, height);
                let ready = std::mem::take(&mut jpeg);
                match res_tx.try_send(PreviewEncodeResult { ts: req.ts, dims, jpeg: ready }) {
                    Ok(()) => {}
                    // Drop stale results if the consumer is behind, but keep the owned buffer so
                    // the worker can reuse its capacity on the next encode.
                    Err(TrySendError::Full(result)) => {
                        jpeg = result.jpeg;
                    }
                    Err(TrySendError::Disconnected(result)) => {
                        jpeg = result.jpeg;
                        break;
                    }
                }
            }
        });
        Self { req_tx, res_rx, recycle_tx, join }
    }

    pub(super) fn stop(self) {
        drop(self.req_tx);
        drop(self.recycle_tx);
        let _ = self.join.join();
    }
}

impl StreamRunner {
    pub fn stream_id(&self) -> Option<Uuid> {
        self.stream_id
    }

    pub fn snapshot_jpeg(&self, quality: u8) -> Result<Vec<u8>> {
        let _ = quality;
        if let Some(stream_id) = self.stream_id {
            if let Ok((header, bytes)) = crate::stream::read_latest_frame_with_header(stream_id) {
                if matches!(&header.fourcc.to_u32().to_le_bytes(), b"MJPG" | b"JPEG") {
                    return Ok(bytes);
                }
            }
        }
        Err(Error::NotFound("preview frame unavailable"))
    }
}

fn write_rgb24(image: &image::DynamicImage, out: &mut [u8]) -> bool {
    let width = image.width() as usize;
    let height = image.height() as usize;
    let want = width.saturating_mul(height).saturating_mul(3);
    if out.len() < want {
        return false;
    }

    match image {
        image::DynamicImage::ImageRgb8(buf) => {
            let raw = buf.as_raw();
            if raw.len() != want {
                return false;
            }
            out[..want].copy_from_slice(raw);
            true
        }
        image::DynamicImage::ImageRgba8(buf) => {
            let raw = buf.as_raw();
            if raw.len() != width.saturating_mul(height).saturating_mul(4) {
                return false;
            }
            let mut di = 0usize;
            for src in raw.chunks_exact(4) {
                out[di] = src[0];
                out[di + 1] = src[1];
                out[di + 2] = src[2];
                di += 3;
            }
            true
        }
        image::DynamicImage::ImageLuma8(buf) => {
            let raw = buf.as_raw();
            if raw.len() != width.saturating_mul(height) {
                return false;
            }
            let mut di = 0usize;
            for &g in raw {
                out[di] = g;
                out[di + 1] = g;
                out[di + 2] = g;
                di += 3;
            }
            true
        }
        image::DynamicImage::ImageLumaA8(buf) => {
            let raw = buf.as_raw();
            if raw.len() != width.saturating_mul(height).saturating_mul(2) {
                return false;
            }
            let mut di = 0usize;
            for src in raw.chunks_exact(2) {
                let g = src[0];
                out[di] = g;
                out[di + 1] = g;
                out[di + 2] = g;
                di += 3;
            }
            true
        }
        _ => {
            let rgb = image.to_rgb8();
            let raw = rgb.as_raw();
            if raw.len() != want {
                return false;
            }
            out[..want].copy_from_slice(raw);
            true
        }
    }
}
