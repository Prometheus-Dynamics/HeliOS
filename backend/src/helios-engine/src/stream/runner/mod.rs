use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
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

fn preview_submit_interval_for_fps(max_fps: Option<f64>) -> Option<Duration> {
    let fps = max_fps?;
    if !fps.is_finite() || fps <= 0.0 {
        return None;
    }
    Some(Duration::from_secs_f64(1.0 / fps.clamp(1.0, 120.0)))
}

fn preview_default_submit_interval() -> Option<Duration> {
    let fps = std::env::var("HELIOS_PREVIEW_MAX_FPS").ok().and_then(|raw| raw.parse::<f64>().ok())?;
    preview_submit_interval_for_fps((fps > 0.0).then_some(fps))
}

pub(super) fn preview_submit_due(last_submit_wall: Option<Instant>, interval: Option<Duration>, now: Instant) -> bool {
    !interval.is_some_and(|interval| last_submit_wall.is_some_and(|last| now.saturating_duration_since(last) < interval))
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
    pub(super) capture_stats: styx::prelude::StageMetrics,
    pub(super) graph_stage_stats: styx::prelude::StageMetrics,
    pub(super) last_capture_ts: Option<u64>,
    pub(super) last_capture_wall: Option<Instant>,
    pub(super) last_graph_wall: Option<Instant>,
    pub(super) capture_empty_since: Option<Instant>,
    pub(super) capture_started_wall: Option<Instant>,
    pub(super) viewer_idle_timeout: std::time::Duration,
    pub(super) viewer_check_interval: std::time::Duration,
    pub(super) last_viewer_check_wall: Option<Instant>,
    pub(super) viewer_recently_active: bool,
    pub(super) last_preview_submit_wall: Option<Instant>,
    pub(super) preview_submit_interval: Option<Duration>,
    pub(super) preview_transport_stats: Arc<Mutex<PreviewTransportStats>>,
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

#[derive(Debug, Default)]
pub(super) struct PreviewTransportTiming {
    samples: u64,
    total_ns: u128,
    last_ns: u64,
}

impl PreviewTransportTiming {
    fn average_ms(&self) -> f64 {
        if self.samples == 0 {
            0.0
        } else {
            (self.total_ns as f64 / self.samples as f64) / 1_000_000.0
        }
    }

    fn last_ms(&self) -> f64 {
        self.last_ns as f64 / 1_000_000.0
    }
}

#[derive(Debug, Default)]
pub(super) struct PreviewTransportStats {
    queue_wait: PreviewTransportTiming,
    publish: PreviewTransportTiming,
    end_to_end: PreviewTransportTiming,
    replaced_pending_frames: u64,
}

impl PreviewTransportStats {
    pub(super) fn snapshot(&self) -> crate::stream::StreamPreviewTransportMetrics {
        crate::stream::StreamPreviewTransportMetrics {
            queue_average_time_ms: self.queue_wait.average_ms(),
            queue_last_time_ms: self.queue_wait.last_ms(),
            publish_average_time_ms: self.publish.average_ms(),
            publish_last_time_ms: self.publish.last_ms(),
            end_to_end_average_time_ms: self.end_to_end.average_ms(),
            end_to_end_last_time_ms: self.end_to_end.last_ms(),
            replaced_pending_frames: self.replaced_pending_frames,
        }
    }

    pub(super) fn has_samples(&self) -> bool {
        self.queue_wait.samples > 0 || self.publish.samples > 0 || self.end_to_end.samples > 0 || self.replaced_pending_frames > 0
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_submit_interval_for_fps_disables_non_positive_limits() {
        assert_eq!(preview_submit_interval_for_fps(None), None);
        assert_eq!(preview_submit_interval_for_fps(Some(0.0)), None);
        assert_eq!(preview_submit_interval_for_fps(Some(-1.0)), None);
    }

    #[test]
    fn preview_submit_interval_for_fps_computes_expected_rate() {
        assert_eq!(preview_submit_interval_for_fps(Some(30.0)), Some(Duration::from_secs_f64(1.0 / 30.0)));
        assert_eq!(preview_submit_interval_for_fps(Some(240.0)), Some(Duration::from_secs_f64(1.0 / 120.0)));
    }

    #[test]
    fn preview_submit_due_allows_first_frame_and_after_interval() {
        let now = Instant::now();
        assert!(preview_submit_due(None, Some(Duration::from_millis(33)), now));
        assert!(preview_submit_due(Some(now - Duration::from_millis(40)), Some(Duration::from_millis(33)), now));
    }

    #[test]
    fn preview_submit_due_blocks_frames_inside_interval() {
        let now = Instant::now();
        assert!(!preview_submit_due(Some(now - Duration::from_millis(10)), Some(Duration::from_millis(33)), now));
        assert!(preview_submit_due(Some(now - Duration::from_millis(10)), None, now));
    }
}
