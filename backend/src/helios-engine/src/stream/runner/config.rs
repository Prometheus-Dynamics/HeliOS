use std::env;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Duration;

use styx::prelude::FourCc;
use tokio::sync::broadcast;

use crate::capture::{discover_devices, find_backend_for_config, BackendKind, CaptureConfig, ModeId};
use crate::graph::GraphHandle;
use crate::ipc::{DecoderSettings, EncoderSettings};

use super::super::ShmemWriter;
use super::PreviewWorker;
use super::StreamRunner;

const ENV_ENCODED_CHANNEL_SIZE: &str = "HELIOS_ENCODED_CHANNEL_SIZE";
const DEFAULT_ENCODED_CHANNEL_SIZE: usize = 8;
const ENV_DEFAULT_LIBCAMERA_FPS: &str = "HELIOS_DEFAULT_LIBCAMERA_FPS";
// Default conservatively: high FPS at full resolution can overwhelm embedded memory/CPU and
// create pathological backpressure (appearing as "register hangs" in the UI).
// Users can override via `HELIOS_DEFAULT_LIBCAMERA_FPS` when they explicitly want higher rates.
const DEFAULT_LIBCAMERA_FPS: u32 = 30;
const ENV_VIEWER_IDLE_TIMEOUT_MS: &str = "HELIOS_STREAM_VIEWER_IDLE_TIMEOUT_MS";
const DEFAULT_VIEWER_IDLE_TIMEOUT_MS: u64 = 2_500;
const ENV_VIEWER_CHECK_INTERVAL_MS: &str = "HELIOS_STREAM_VIEWER_CHECK_INTERVAL_MS";
const DEFAULT_VIEWER_CHECK_INTERVAL_MS: u64 = 250;
const ENV_PREVIEW_ENCODE_INTERVAL_MS: &str = "HELIOS_STREAM_PREVIEW_ENCODE_INTERVAL_MS";
const DEFAULT_PREVIEW_ENCODE_INTERVAL_MS: u64 = 33;

pub struct StreamRunnerConfig {
    pub capture_config: CaptureConfig,
    pub graph: GraphHandle,
    pub encoder_id: Option<String>,
    pub decoder_id: Option<String>,
    pub encoder_settings: Option<EncoderSettings>,
    pub decoder_settings: Option<DecoderSettings>,
    pub shmem: Option<ShmemWriter>,
    pub stream_id: Option<uuid::Uuid>,
}

impl StreamRunner {
    pub fn new(config: StreamRunnerConfig) -> Self {
        let StreamRunnerConfig { mut capture_config, graph, encoder_id, mut decoder_id, encoder_settings, decoder_settings, mut shmem, stream_id } = config;
        if capture_config.backend == BackendKind::Libcamera && capture_config.target_fps.is_none() && capture_config.interval.is_none() {
            let fps = env::var(ENV_DEFAULT_LIBCAMERA_FPS).ok().and_then(|v| v.parse::<u32>().ok()).unwrap_or(DEFAULT_LIBCAMERA_FPS);
            capture_config.target_fps = Some(fps.max(1));
        }
        if let Some(fallback_mode) = find_v4l2_usb_compressed_mode_for_config(&capture_config) {
            let from = capture_config.mode.format.code;
            let to = fallback_mode.format.code;
            capture_config.mode = fallback_mode;
            if let Some(decoder) = decoder_id.as_deref() {
                if !decoder_looks_compatible_with_capture(decoder, to) {
                    if let Some(selector) = decoder_selector_for_capture_fourcc(to) {
                        tracing::warn!(from = ?from, to = ?to, decoder = %decoder, replacement = %selector, "switching decoder to match fallback V4L2 capture format");
                        decoder_id = Some(selector.to_string());
                    }
                }
            }
            tracing::warn!(from = ?from, to = ?to, "switching USB V4L2 capture mode to compressed format");
        }
        if capture_config.backend == BackendKind::Libcamera && decoder_id.as_deref().is_some_and(|id| id.eq_ignore_ascii_case("yuyv-luma") || id.eq_ignore_ascii_case("nv12-luma")) {
            // Route luma decode through the ISP (NV12 viewfinder) instead of YUYV.
            if let Some(nv12_mode) = find_nv12_mode_for_config(&capture_config) {
                capture_config.mode = nv12_mode;
                if decoder_id.as_deref().is_some_and(|id| id.eq_ignore_ascii_case("yuyv-luma")) {
                    decoder_id = Some("nv12-luma".to_string());
                }
            }
        }

        let (encoded_tx, _encoded_rx) = broadcast::channel(encoded_channel_size());
        let (raw_tx, _raw_rx) = broadcast::channel(encoded_channel_size());
        let _ = (_encoded_rx, _raw_rx);
        let decode_fps_limit = decoder_settings.as_ref().and_then(|s| s.fps_limit).filter(|v| *v > 0.0);
        let encode_fps_limit =
            encoder_id.as_ref().and_then(|_| encoder_settings.as_ref().and_then(|s| s.framerate.as_ref())).map(|fr| fr.numerator as f64 / fr.denominator.max(1) as f64).filter(|v| *v > 0.0);
        let capture_fourcc = capture_config.mode.format.code;
        let stream_label: metrics::SharedString = stream_id.map(|id| id.to_string()).unwrap_or_else(|| "unknown".to_string()).into();
        let viewer_idle_timeout = viewer_idle_timeout();
        let viewer_check_interval = viewer_check_interval();
        let preview_encode_interval = preview_encode_interval();
        // When both encoder + decoder IDs are unset, treat the stream as "codecs disabled" and do
        // not generate any preview shmem output (even passthrough MJPEG). However, file-backed
        // streams (media replay) do not rely on external codecs for preview: we can still generate
        // JPEG previews via the lightweight `PreviewWorker`.
        let codecs_disabled = encoder_id.is_none() && decoder_id.is_none() && capture_config.backend != BackendKind::File;
        if codecs_disabled {
            if let Some(writer) = shmem.as_mut() {
                let _ = writer.write(None, None, (0, 0), &[]);
            }
        }
        // Keep preview generation independent from the main encoder path. This prevents UI preview
        // activity from implicitly requiring full stream encoder throughput.
        let preview_encoder_stats = styx::codec::CodecStats::default();
        let preview_encoder_last_activity_ms = Arc::new(AtomicU64::new(0));
        let preview_worker = if !codecs_disabled {
            shmem.as_ref().map(|_| PreviewWorker::start(preview_encoder_stats.clone(), preview_encoder_last_activity_ms.clone()))
        } else {
            None
        };
        Self {
            stream_label,
            stream_id,
            capture_config,
            capture_fourcc: Some(capture_fourcc),
            session: None,
            graph,
            encode_fourcc: None,
            encoder_input_fourcc: None,
            encoder_impl: None,
            encoder_id,
            decoder_id,
            encoder_settings,
            decoder_settings,
            decode_fps_limit,
            encode_fps_limit,
            encode_configured: false,
            last_decode_wall: None,
            last_encode_wall: None,
            encoder_last_activity_ms: Arc::new(AtomicU64::new(0)),
            codecs: None,
            encoded_tx,
            raw_tx,
            shmem,
            encoder_worker: None,
            decoder_stats: styx::codec::CodecStats::default(),
            encoder_stats: styx::codec::CodecStats::default(),
            preview_encoder_stats,
            capture_stats: styx::prelude::StageMetrics::default(),
            last_capture_ts: None,
            last_capture_wall: None,
            capture_empty_since: None,
            capture_started_wall: None,
            viewer_idle_timeout,
            viewer_check_interval,
            last_viewer_check_wall: None,
            viewer_recently_active: false,
            preview_encode_interval,
            last_preview_encode_wall: None,
            preview_encoder_last_activity_ms,
            preview_worker,
            runner_memory: super::RunnerMemoryTracker::default(),
        }
    }
}

fn encoded_channel_size() -> usize {
    env::var(ENV_ENCODED_CHANNEL_SIZE).ok().and_then(|v| v.parse().ok()).filter(|v| *v > 0).unwrap_or(DEFAULT_ENCODED_CHANNEL_SIZE)
}

fn viewer_idle_timeout() -> Duration {
    let millis = env::var(ENV_VIEWER_IDLE_TIMEOUT_MS).ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(DEFAULT_VIEWER_IDLE_TIMEOUT_MS);
    Duration::from_millis(millis.clamp(250, 60_000))
}

fn viewer_check_interval() -> Duration {
    let millis = env::var(ENV_VIEWER_CHECK_INTERVAL_MS).ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(DEFAULT_VIEWER_CHECK_INTERVAL_MS);
    Duration::from_millis(millis.clamp(50, 5_000))
}

fn preview_encode_interval() -> Duration {
    let millis = env::var(ENV_PREVIEW_ENCODE_INTERVAL_MS).ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(DEFAULT_PREVIEW_ENCODE_INTERVAL_MS);
    Duration::from_millis(millis.clamp(10, 1_000))
}

fn find_nv12_mode_for_config(config: &CaptureConfig) -> Option<ModeId> {
    if config.backend != BackendKind::Libcamera {
        return None;
    }
    let devices = discover_devices();
    let backend = find_backend_for_config(config, &devices)?;
    let target = config.mode.format.resolution;
    let nv12 = FourCc::new(*b"NV12");
    backend.descriptor.modes.iter().find(|mode| mode.format.code == nv12 && mode.format.resolution.width == target.width && mode.format.resolution.height == target.height).map(|mode| mode.id.clone())
}

fn find_v4l2_usb_compressed_mode_for_config(config: &CaptureConfig) -> Option<ModeId> {
    if config.backend != BackendKind::V4l2 {
        return None;
    }
    if !config.device_keys.iter().any(|key| key.to_ascii_lowercase().contains("usb")) {
        return None;
    }
    if !looks_uncompressed_capture_fourcc(config.mode.format.code) {
        return None;
    }
    let target = config.mode.format.resolution;
    let pixels = target.width.get().saturating_mul(target.height.get());
    // Avoid rewriting low-res modes where uncompressed USB bandwidth is usually fine.
    if pixels < 1280 * 720 {
        return None;
    }

    let devices = discover_devices();
    let backend = find_backend_for_config(config, &devices)?;
    backend
        .descriptor
        .modes
        .iter()
        .filter(|mode| mode.format.resolution.width == target.width && mode.format.resolution.height == target.height)
        .filter_map(|mode| compressed_capture_score(mode.format.code).map(|score| (score, mode.id.clone())))
        .max_by_key(|(score, _)| *score)
        .map(|(_, mode)| mode)
}

fn looks_uncompressed_capture_fourcc(fourcc: FourCc) -> bool {
    matches!(&fourcc.to_u32().to_le_bytes(), b"YUYV" | b"UYVY" | b"RG24" | b"RGB3" | b"NV12")
}

fn compressed_capture_score(fourcc: FourCc) -> Option<u8> {
    match &fourcc.to_u32().to_le_bytes() {
        b"MJPG" | b"JPEG" => Some(3),
        b"H264" => Some(2),
        b"H265" | b"HEVC" => Some(1),
        _ => None,
    }
}

fn decoder_selector_for_capture_fourcc(fourcc: FourCc) -> Option<&'static str> {
    match &fourcc.to_u32().to_le_bytes() {
        b"MJPG" | b"JPEG" => Some("mjpeg"),
        b"H264" => Some("h264"),
        b"H265" | b"HEVC" => Some("h265"),
        _ => None,
    }
}

fn decoder_looks_compatible_with_capture(decoder: &str, fourcc: FourCc) -> bool {
    let decoder = decoder.trim();
    if decoder.is_empty() {
        return false;
    }
    if let Some(selector) = decoder_selector_for_capture_fourcc(fourcc) {
        return decoder.eq_ignore_ascii_case(selector) || decoder.to_ascii_lowercase().contains(selector);
    }
    true
}
