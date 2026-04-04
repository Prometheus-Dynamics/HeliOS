use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Duration;

use lib_runtime_policy::HELIOS_ENGINE_STREAM_RUNTIME_POLICY;
use styx::prelude::FourCc;
use tokio::sync::broadcast;

use crate::capture::{discover_devices, find_backend_for_config, BackendKind, CaptureConfig, ModeId};
use crate::graph::GraphHandle;
use crate::ipc::{DecoderSettings, EncoderSettings};

use super::super::ShmemWriter;
use super::StreamRunner;

pub struct StreamRunnerConfig {
    pub capture_config: CaptureConfig,
    pub graph: GraphHandle,
    pub encoder_id: Option<String>,
    pub decoder_id: Option<String>,
    pub encoder_settings: Option<EncoderSettings>,
    pub decoder_settings: Option<DecoderSettings>,
    pub preview_jpeg_quality: u8,
    pub shmem: Option<ShmemWriter>,
    pub stream_id: Option<uuid::Uuid>,
}

impl StreamRunner {
    pub fn new(config: StreamRunnerConfig) -> Self {
        let StreamRunnerConfig { mut capture_config, graph, encoder_id, mut decoder_id, encoder_settings, decoder_settings, preview_jpeg_quality: _preview_jpeg_quality, mut shmem, stream_id } =
            config;
        let runtime_policy = HELIOS_ENGINE_STREAM_RUNTIME_POLICY.resolve();
        if capture_config.backend == BackendKind::Libcamera && capture_config.target_fps.is_none() && capture_config.interval.is_none() {
            capture_config.target_fps = Some(runtime_policy.default_libcamera_fps.max(1));
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
        if decoder_id.is_none() && capture_config.backend == BackendKind::Libcamera && capture_config.mode.format.code == FourCc::new(*b"NV12") && graph.prefers_grayscale_input() {
            tracing::info!("auto-selecting nv12-luma decoder for grayscale graph");
            decoder_id = Some("nv12-luma".to_string());
        }

        let (encoded_tx, _encoded_rx) = broadcast::channel(encoded_channel_size());
        let (raw_tx, _raw_rx) = broadcast::channel(encoded_channel_size());
        let _ = (_encoded_rx, _raw_rx);
        let decode_fps_limit = decoder_settings.as_ref().and_then(|s| s.fps_limit).filter(|v| *v > 0.0);
        let encode_fps_limit =
            encoder_id.as_ref().and_then(|_| encoder_settings.as_ref().and_then(|s| s.framerate())).map(|fr| fr.numerator as f64 / fr.denominator.max(1) as f64).filter(|v| *v > 0.0);
        let capture_fourcc = capture_config.mode.format.code;
        let stream_label: metrics::SharedString = stream_id.map(|id| id.to_string()).unwrap_or_else(|| "unknown".to_string()).into();
        let viewer_idle_timeout = viewer_idle_timeout();
        let viewer_check_interval = viewer_check_interval();
        // When both encoder + decoder IDs are unset, treat the stream as "codecs disabled" and do
        // not generate any preview shmem output.
        let codecs_disabled = encoder_id.is_none() && decoder_id.is_none() && capture_config.backend != BackendKind::File;
        if codecs_disabled {
            if let Some(writer) = shmem.as_mut() {
                let _ = writer.write(None, None, (0, 0), &[]);
            }
        }
        let preview_transport_stats = Arc::new(std::sync::Mutex::new(super::PreviewTransportStats::default()));
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
            managed_encoded_consumer_count: Arc::new(AtomicU64::new(0)),
            managed_encoded_consumer_last_seen_ms: Arc::new(AtomicU64::new(0)),
            codecs: None,
            encoded_tx,
            raw_tx,
            shmem,
            encoder_worker: None,
            decoder_stats: styx::codec::CodecStats::default(),
            encoder_stats: styx::codec::CodecStats::default(),
            capture_stats: styx::prelude::StageMetrics::default(),
            graph_stage_stats: styx::prelude::StageMetrics::default(),
            last_capture_ts: None,
            last_capture_wall: None,
            last_graph_wall: None,
            capture_empty_since: None,
            capture_started_wall: None,
            viewer_idle_timeout,
            viewer_check_interval,
            last_viewer_check_wall: None,
            viewer_recently_active: false,
            preview_transport_stats,
            last_idle_compaction_wall: None,
            last_demand_state: std::sync::Mutex::new(crate::ipc::StreamDemandRuntimeState::default()),
            runner_memory: super::RunnerMemoryTracker::default(),
        }
    }
}

fn encoded_channel_size() -> usize {
    HELIOS_ENGINE_STREAM_RUNTIME_POLICY.resolve().encoded_channel_size
}

fn viewer_idle_timeout() -> Duration {
    Duration::from_millis(HELIOS_ENGINE_STREAM_RUNTIME_POLICY.resolve().viewer_idle_timeout_ms)
}

fn viewer_check_interval() -> Duration {
    Duration::from_millis(HELIOS_ENGINE_STREAM_RUNTIME_POLICY.resolve().viewer_check_interval_ms)
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

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;
    use std::sync::Arc;

    use image::DynamicImage;
    use styx::core::format::{ColorSpace, Interval, MediaFormat, Resolution};

    use super::*;
    use crate::capture::BackendHandle;
    use crate::graph::{GraphExecutor, GraphHandle, HostBridgeHandle};

    struct MockGraphExecutor {
        prefers_grayscale_input: bool,
    }

    impl GraphExecutor for MockGraphExecutor {
        fn process(&self, image: DynamicImage) -> Option<DynamicImage> {
            Some(image)
        }

        fn prefers_grayscale_input(&self) -> bool {
            self.prefers_grayscale_input
        }
    }

    fn sample_config() -> CaptureConfig {
        CaptureConfig {
            device_keys: vec![],
            device_identity: None,
            backend: BackendKind::Libcamera,
            handle: BackendHandle::Libcamera { id: "camera".to_string() },
            mode: ModeId { format: MediaFormat::new(FourCc::new(*b"NV12"), Resolution::new(1280, 800).unwrap(), ColorSpace::Srgb), interval: None },
            target_fps: Some(60),
            interval: None,
            controls: vec![],
            enable_tdn_output: false,
        }
    }

    fn sample_graph(prefers_grayscale_input: bool) -> GraphHandle {
        let (host, rx) = HostBridgeHandle::new(1);
        let _ = rx;
        GraphHandle::with_executor(host, Arc::new(MockGraphExecutor { prefers_grayscale_input }))
    }

    #[test]
    fn libcamera_target_fps_maps_to_interval() {
        let config = sample_config();
        assert_eq!(config.effective_interval_for_backend(BackendKind::Libcamera), Some(Interval { numerator: NonZeroU32::new(1).unwrap(), denominator: NonZeroU32::new(60).unwrap() }));
    }

    #[test]
    fn non_libcamera_target_fps_still_maps_to_interval() {
        let mut config = sample_config();
        config.backend = BackendKind::V4l2;
        assert_eq!(config.effective_interval_for_backend(BackendKind::V4l2), Some(Interval { numerator: NonZeroU32::new(1).unwrap(), denominator: NonZeroU32::new(60).unwrap() }));
    }

    #[test]
    fn auto_selects_nv12_luma_for_grayscale_graphs() {
        let runner = crate::stream::runner::StreamRunner::new(StreamRunnerConfig {
            capture_config: sample_config(),
            graph: sample_graph(true),
            encoder_id: None,
            decoder_id: None,
            encoder_settings: None,
            decoder_settings: None,
            preview_jpeg_quality: 65,
            shmem: None,
            stream_id: None,
        });

        assert_eq!(runner.decoder_id.as_deref(), Some("nv12-luma"));
    }

    #[test]
    fn keeps_decoder_unset_for_non_grayscale_graphs() {
        let runner = crate::stream::runner::StreamRunner::new(StreamRunnerConfig {
            capture_config: sample_config(),
            graph: sample_graph(false),
            encoder_id: None,
            decoder_id: None,
            encoder_settings: None,
            decoder_settings: None,
            preview_jpeg_quality: 65,
            shmem: None,
            stream_id: None,
        });

        assert_eq!(runner.decoder_id, None);
    }
}
