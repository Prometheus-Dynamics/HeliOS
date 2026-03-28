use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use image::codecs::jpeg::JpegEncoder;
use image::ColorType;
use styx::prelude::FrameLease;
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

fn preview_worker_trim_interval() -> Duration {
    let millis = std::env::var("HELIOS_PREVIEW_WORKER_TRIM_INTERVAL_MS").ok().and_then(|raw| raw.parse::<u64>().ok()).unwrap_or(1_000).clamp(50, 60_000);
    Duration::from_millis(millis)
}

fn preview_worker_rgb_retain_cap() -> usize {
    std::env::var("HELIOS_PREVIEW_WORKER_RGB_RETAIN_BYTES").ok().and_then(|raw| raw.parse::<usize>().ok()).unwrap_or(1280 * 720 * 3).max(320 * 240 * 3)
}

fn preview_worker_gray_retain_cap() -> usize {
    std::env::var("HELIOS_PREVIEW_WORKER_GRAY_RETAIN_BYTES").ok().and_then(|raw| raw.parse::<usize>().ok()).unwrap_or(1280 * 720).max(320 * 240)
}

fn preview_worker_jpeg_retain_cap() -> usize {
    std::env::var("HELIOS_PREVIEW_WORKER_JPEG_RETAIN_BYTES").ok().and_then(|raw| raw.parse::<usize>().ok()).unwrap_or(2 * 1024 * 1024).max(256 * 1024)
}

fn preview_worker_trim_vec(vec: &mut Vec<u8>, retain_cap: usize) {
    vec.clear();
    if vec.capacity() > retain_cap {
        vec.shrink_to(retain_cap);
    }
}

fn compact_preview_worker_buffers(gray: &mut Vec<u8>, rgb: &mut Vec<u8>, jpeg: &mut Vec<u8>) {
    preview_worker_trim_vec(gray, preview_worker_gray_retain_cap());
    preview_worker_trim_vec(rgb, preview_worker_rgb_retain_cap());
    preview_worker_trim_vec(jpeg, preview_worker_jpeg_retain_cap());
}

fn release_preview_worker_buffers(gray: &mut Vec<u8>, rgb: &mut Vec<u8>, jpeg: &mut Vec<u8>) {
    *gray = Vec::new();
    *rgb = Vec::new();
    *jpeg = Vec::new();
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct LastFrameDemandSnapshot {
    pub raw_receiver_count: u64,
    pub host_receiver_count: u64,
    pub preview_demand_active: bool,
    pub encode_demand_active: bool,
    pub graph_sample_demand_active: bool,
    pub needs_decoded_image: bool,
    pub graph_has_image_output: bool,
    pub graph_has_executor: bool,
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
    pub(super) managed_encoded_consumer_count: Arc<AtomicU64>,
    pub(super) managed_encoded_consumer_last_seen_ms: Arc<AtomicU64>,
    pub(super) codecs: Option<styx::codec::CodecRegistryHandle>,
    pub(super) encoded_tx: broadcast::Sender<crate::stream::EncodedFrame>,
    pub(super) raw_tx: broadcast::Sender<Arc<image::DynamicImage>>,
    pub(super) shmem: Option<ShmemWriter>,
    pub(super) encoder_worker: Option<EncoderWorker>,
    pub(super) decoder_stats: styx::codec::CodecStats,
    pub(super) encoder_stats: styx::codec::CodecStats,
    pub(super) preview_encoder_stats: styx::codec::CodecStats,
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
    pub(super) last_preview_work_wall: Option<Instant>,
    pub(super) last_preview_encode_wall: Option<Instant>,
    pub(super) preview_encoder_last_activity_ms: Arc<AtomicU64>,
    pub(super) preview_worker: Option<PreviewWorker>,
    pub(super) last_idle_compaction_wall: Option<Instant>,
    pub(super) last_frame_demand: Mutex<LastFrameDemandSnapshot>,
    pub(super) runner_memory: RunnerMemoryTracker,
}

#[derive(Debug, Default)]
pub(super) struct RunnerMemoryTracker {
    pub(super) current_decoded_frame_bytes: u64,
    pub(super) peak_decoded_frame_bytes: u64,
    pub(super) current_raw_clone_bytes: u64,
    pub(super) peak_raw_clone_bytes: u64,
    pub(super) current_processed_frame_bytes: u64,
    pub(super) peak_processed_frame_bytes: u64,
    pub(super) current_frame_working_set_bytes: u64,
    pub(super) peak_frame_working_set_bytes: u64,
}

impl RunnerMemoryTracker {
    fn update_peak(slot: &mut u64, value: u64) {
        if value > *slot {
            *slot = value;
        }
    }

    pub(super) fn reset_current(&mut self) {
        self.current_decoded_frame_bytes = 0;
        self.current_raw_clone_bytes = 0;
        self.current_processed_frame_bytes = 0;
        self.current_frame_working_set_bytes = 0;
    }

    pub(super) fn set_decoded_frame_bytes(&mut self, bytes: u64) {
        self.current_decoded_frame_bytes = bytes;
        Self::update_peak(&mut self.peak_decoded_frame_bytes, bytes);
        self.refresh_total();
    }

    pub(super) fn set_raw_clone_bytes(&mut self, bytes: u64) {
        self.current_raw_clone_bytes = bytes;
        Self::update_peak(&mut self.peak_raw_clone_bytes, bytes);
        self.refresh_total();
    }

    pub(super) fn set_processed_frame_bytes(&mut self, bytes: u64) {
        self.current_processed_frame_bytes = bytes;
        Self::update_peak(&mut self.peak_processed_frame_bytes, bytes);
        self.refresh_total();
    }

    fn refresh_total(&mut self) {
        self.current_frame_working_set_bytes = self.current_decoded_frame_bytes.saturating_add(self.current_raw_clone_bytes).saturating_add(self.current_processed_frame_bytes);
        Self::update_peak(&mut self.peak_frame_working_set_bytes, self.current_frame_working_set_bytes);
    }

    pub(super) fn snapshot(&self) -> crate::stream::StreamRunnerMemoryMetrics {
        crate::stream::StreamRunnerMemoryMetrics {
            current_decoded_frame_bytes: self.current_decoded_frame_bytes,
            peak_decoded_frame_bytes: self.peak_decoded_frame_bytes,
            current_raw_clone_bytes: self.current_raw_clone_bytes,
            peak_raw_clone_bytes: self.peak_raw_clone_bytes,
            current_processed_frame_bytes: self.current_processed_frame_bytes,
            peak_processed_frame_bytes: self.peak_processed_frame_bytes,
            current_frame_working_set_bytes: self.current_frame_working_set_bytes,
            peak_frame_working_set_bytes: self.peak_frame_working_set_bytes,
        }
    }
}

pub(super) struct PreviewWorker {
    pub(super) req_tx: std::sync::mpsc::SyncSender<PreviewEncodeRequest>,
    pub(super) res_rx: std::sync::mpsc::Receiver<PreviewEncodeResult>,
    pub(super) recycle_tx: std::sync::mpsc::SyncSender<Vec<u8>>,
    join: std::thread::JoinHandle<()>,
}

pub(super) enum PreviewEncodeSource {
    Image(Arc<image::DynamicImage>),
    Gray(Arc<image::GrayImage>),
    Frame(FrameLease),
}

pub(super) struct PreviewEncodeRequest {
    pub(super) ts: u64,
    pub(super) source: PreviewEncodeSource,
    pub(super) output_resolution: Option<(u32, u32)>,
}

pub(super) struct PreviewEncodeResult {
    pub(super) ts: u64,
    pub(super) dims: (u32, u32),
    pub(super) jpeg: Vec<u8>,
}

impl PreviewWorker {
    pub(super) fn start(stats: styx::codec::CodecStats, activity_ms: Arc<AtomicU64>) -> Self {
        let (req_tx, req_rx) = std::sync::mpsc::sync_channel::<PreviewEncodeRequest>(1);
        let (res_tx, res_rx) = std::sync::mpsc::sync_channel::<PreviewEncodeResult>(1);
        let (recycle_tx, recycle_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(1);
        let quality = preview_jpeg_quality();
        let trim_interval = preview_worker_trim_interval();
        let join = std::thread::spawn(move || {
            use std::sync::mpsc::{RecvTimeoutError, TrySendError};

            let mut gray = Vec::<u8>::new();
            let mut rgb = Vec::<u8>::new();
            let mut jpeg = Vec::<u8>::new();
            let mut last_trim = Instant::now();

            loop {
                let req = match req_rx.recv_timeout(trim_interval) {
                    Ok(req) => req,
                    Err(RecvTimeoutError::Timeout) => {
                        while recycle_rx.try_recv().is_ok() {}
                        release_preview_worker_buffers(&mut gray, &mut rgb, &mut jpeg);
                        last_trim = Instant::now();
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                };
                if let Ok(recycled) = recycle_rx.try_recv() {
                    jpeg = recycled;
                }
                let encode_start = Instant::now();
                let dims = match encode_preview_request(&req, quality, &mut gray, &mut rgb, &mut jpeg) {
                    Some(dims) => dims,
                    None => {
                        stats.inc_errors();
                        continue;
                    }
                };

                stats.inc_processed();
                stats.record_duration(encode_start.elapsed());
                activity_ms.store(StreamRunner::unix_now_ms(), Ordering::Relaxed);
                let ready = std::mem::take(&mut jpeg);
                match res_tx.try_send(PreviewEncodeResult { ts: req.ts, dims, jpeg: ready }) {
                    Ok(()) => {}
                    // Drop stale results if the consumer is behind, but keep the owned buffer so
                    // the worker can reuse its capacity on the next encode.
                    Err(TrySendError::Full(result)) => {
                        stats.inc_backpressure();
                        jpeg = result.jpeg;
                    }
                    Err(TrySendError::Disconnected(_result)) => {
                        break;
                    }
                }

                if last_trim.elapsed() >= trim_interval {
                    compact_preview_worker_buffers(&mut gray, &mut rgb, &mut jpeg);
                    last_trim = Instant::now();
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

fn encode_preview_request(req: &PreviewEncodeRequest, quality: u8, gray: &mut Vec<u8>, rgb: &mut Vec<u8>, jpeg: &mut Vec<u8>) -> Option<(u32, u32)> {
    match &req.source {
        PreviewEncodeSource::Frame(frame) => {
            let dims = (frame.meta().format.resolution.width.get(), frame.meta().format.resolution.height.get());
            let needs_resize = req.output_resolution.is_some_and(|(target_width, target_height)| {
                let target_width = target_width.max(1);
                let target_height = target_height.max(1);
                target_width != dims.0 || target_height != dims.1
            });
            if !needs_resize {
                if let Some(dims) = encode_preview_frame_direct(frame, quality, gray, rgb, jpeg) {
                    return Some(dims);
                }
            }

            let image = styx::codec::decoder::frame_to_dynamic_image(frame)?;
            encode_preview_dynamic_image(&image, req.output_resolution, quality, rgb, jpeg)
        }
        PreviewEncodeSource::Gray(image) => encode_preview_gray_image(image.as_ref(), req.output_resolution, quality, jpeg),
        PreviewEncodeSource::Image(image) => encode_preview_dynamic_image(image.as_ref(), req.output_resolution, quality, rgb, jpeg),
    }
}

fn encode_preview_gray_image(image: &image::GrayImage, output_resolution: Option<(u32, u32)>, quality: u8, jpeg: &mut Vec<u8>) -> Option<(u32, u32)> {
    let resized = output_resolution.and_then(|(target_width, target_height)| {
        let target_width = target_width.max(1);
        let target_height = target_height.max(1);
        if target_width == image.width() && target_height == image.height() {
            None
        } else {
            Some(image::imageops::resize(image, target_width, target_height, image::imageops::FilterType::Triangle))
        }
    });
    let source = resized.as_ref().unwrap_or(image);
    let width = source.width().max(1);
    let height = source.height().max(1);

    jpeg.clear();
    let mut enc = JpegEncoder::new_with_quality(jpeg, quality);
    if enc.encode(source.as_raw(), width, height, ColorType::L8.into()).is_err() {
        return None;
    }
    Some((width, height))
}

fn encode_preview_dynamic_image(image: &image::DynamicImage, output_resolution: Option<(u32, u32)>, quality: u8, rgb: &mut Vec<u8>, jpeg: &mut Vec<u8>) -> Option<(u32, u32)> {
    if matches!(image, image::DynamicImage::ImageLuma8(_) | image::DynamicImage::ImageLumaA8(_)) {
        return lib_cv::modules::image::luma::with_luma8_frame(image, |gray| encode_preview_gray_image(gray, output_resolution, quality, jpeg));
    }

    let resized = output_resolution.and_then(|(target_width, target_height)| {
        let target_width = target_width.max(1);
        let target_height = target_height.max(1);
        if target_width == image.width() && target_height == image.height() {
            None
        } else {
            Some(image.resize_exact(target_width, target_height, image::imageops::FilterType::Triangle))
        }
    });
    let source: &image::DynamicImage = resized.as_ref().unwrap_or(image);
    let width = source.width().max(1);
    let height = source.height().max(1);
    let wanted = width as usize * height as usize * 3;
    if rgb.len() != wanted {
        rgb.resize(wanted, 0);
    }
    if !write_rgb24(source, rgb) {
        return None;
    }

    jpeg.clear();
    let mut enc = JpegEncoder::new_with_quality(jpeg, quality);
    if enc.encode(&rgb[..wanted], width, height, ColorType::Rgb8.into()).is_err() {
        return None;
    }
    Some((width, height))
}

fn encode_preview_frame_direct(frame: &FrameLease, quality: u8, gray: &mut Vec<u8>, rgb: &mut Vec<u8>, jpeg: &mut Vec<u8>) -> Option<(u32, u32)> {
    let meta = frame.meta();
    let width = meta.format.resolution.width.get().max(1);
    let height = meta.format.resolution.height.get().max(1);
    let planes = frame.planes();
    let plane = planes.first()?;
    let code = meta.format.code;

    jpeg.clear();
    match &code.to_u32().to_le_bytes() {
        b"R8  " | b"GREY" => {
            let row_bytes = width as usize;
            let stride = plane.stride().max(row_bytes);
            let plane_len = stride.checked_mul(height as usize)?;
            if plane.data().len() < plane_len {
                return None;
            }
            let packed = if stride == row_bytes {
                &plane.data()[..row_bytes * height as usize]
            } else {
                let wanted = row_bytes * height as usize;
                if gray.len() != wanted {
                    gray.resize(wanted, 0);
                }
                for row in 0..height as usize {
                    let src_off = row * stride;
                    let dst_off = row * row_bytes;
                    gray[dst_off..dst_off + row_bytes].copy_from_slice(&plane.data()[src_off..src_off + row_bytes]);
                }
                &gray[..wanted]
            };
            let mut enc = JpegEncoder::new_with_quality(jpeg, quality);
            if enc.encode(packed, width, height, ColorType::L8.into()).is_err() {
                return None;
            }
        }
        b"RG24" => {
            let row_bytes = width as usize * 3;
            let stride = plane.stride().max(row_bytes);
            let plane_len = stride.checked_mul(height as usize)?;
            if plane.data().len() < plane_len {
                return None;
            }
            let packed = if stride == row_bytes {
                &plane.data()[..row_bytes * height as usize]
            } else {
                let wanted = row_bytes * height as usize;
                if rgb.len() != wanted {
                    rgb.resize(wanted, 0);
                }
                for row in 0..height as usize {
                    let src_off = row * stride;
                    let dst_off = row * row_bytes;
                    rgb[dst_off..dst_off + row_bytes].copy_from_slice(&plane.data()[src_off..src_off + row_bytes]);
                }
                &rgb[..wanted]
            };
            let mut enc = JpegEncoder::new_with_quality(jpeg, quality);
            if enc.encode(packed, width, height, ColorType::Rgb8.into()).is_err() {
                return None;
            }
        }
        b"RGBA" => {
            let row_pixels = width as usize;
            let row_bytes = row_pixels * 4;
            let stride = plane.stride().max(row_bytes);
            let plane_len = stride.checked_mul(height as usize)?;
            if plane.data().len() < plane_len {
                return None;
            }
            let wanted = row_pixels * height as usize * 3;
            if rgb.len() != wanted {
                rgb.resize(wanted, 0);
            }
            for row in 0..height as usize {
                let src_row = &plane.data()[row * stride..row * stride + row_bytes];
                let dst_row = &mut rgb[row * row_pixels * 3..(row + 1) * row_pixels * 3];
                let mut di = 0usize;
                for px in src_row.chunks_exact(4) {
                    dst_row[di] = px[0];
                    dst_row[di + 1] = px[1];
                    dst_row[di + 2] = px[2];
                    di += 3;
                }
            }
            let mut enc = JpegEncoder::new_with_quality(jpeg, quality);
            if enc.encode(&rgb[..wanted], width, height, ColorType::Rgb8.into()).is_err() {
                return None;
            }
        }
        _ => return None,
    }
    Some((width, height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use styx::prelude::{BufferPool, ColorSpace, FourCc, FrameMeta, MediaFormat, Resolution};

    #[test]
    fn preview_frame_direct_encodes_luma_without_dynamic_image() {
        let mut buf = BufferPool::with_limits(1, 4, 1).lease();
        buf.resize(4);
        buf.as_mut_slice().copy_from_slice(&[0, 64, 128, 255]);
        let frame = FrameLease::single_plane(FrameMeta::new(MediaFormat::new(FourCc::new(*b"GREY"), Resolution::new(2, 2).unwrap(), ColorSpace::Unknown), 42), buf, 4, 2);

        let mut gray = Vec::new();
        let mut rgb = Vec::new();
        let mut jpeg = Vec::new();
        let dims = encode_preview_frame_direct(&frame, 90, &mut gray, &mut rgb, &mut jpeg);
        assert_eq!(dims, Some((2, 2)));
        assert!(!jpeg.is_empty());
    }
}
