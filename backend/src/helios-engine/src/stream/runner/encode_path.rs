use std::fs;
use std::process::Command;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use metrics::histogram;
use styx::codec::ffmpeg::{FfmpegEncoderOptions, FfmpegH264Encoder, FfmpegH265Encoder, FfmpegMjpegEncoder};
use styx::codec::{Codec, CodecKind, CodecPolicy, CodecRegistry};
use styx::prelude::FourCc;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::capture::{CaptureControlInfo, CaptureControlValue, CaptureDescriptor, CaptureSession, ControlAssignment};
use crate::error::{Error, Result};
use crate::graph::GraphHandle;
use crate::stream::{
    CodecMetrics, EncodedFrame, StreamBufferPoolMetrics, StreamExternalBackingMetrics, StreamMemoryMetrics, StreamMetrics, StreamPackedPoolMetrics, StreamProcessMemoryMetrics,
    StreamQueueMemoryMetrics, StreamStagingCopyMetrics,
};

use super::super::encode::{stage_to_capture_metrics, to_codec_metrics};
use super::super::encoder_worker::EncoderWorkerStart;
use super::StreamRunner;

impl StreamRunner {
    fn parse_proc_key_bytes(text: &str, key: &str) -> Option<u64> {
        text.lines().find_map(|line| {
            let trimmed = line.trim_start();
            if !trimmed.starts_with(key) {
                return None;
            }
            let value = trimmed[key.len()..].trim();
            let number = value.split_whitespace().next().and_then(|raw| raw.parse::<u64>().ok())?;
            if value.contains("kB") {
                Some(number.saturating_mul(1024))
            } else {
                Some(number)
            }
        })
    }

    fn process_memory_metrics() -> Option<StreamProcessMemoryMetrics> {
        let status = fs::read_to_string("/proc/self/status").ok()?;
        let rss_bytes = Self::parse_proc_key_bytes(&status, "VmRSS:")?;

        Some(StreamProcessMemoryMetrics {
            sampled_at_ms: Self::unix_now_ms(),
            rss_bytes,
            pss_bytes: fs::read_to_string("/proc/self/smaps_rollup").ok().and_then(|text| Self::parse_proc_key_bytes(&text, "Pss:")),
            rss_anon_bytes: Self::parse_proc_key_bytes(&status, "RssAnon:").unwrap_or(0),
            rss_file_bytes: Self::parse_proc_key_bytes(&status, "RssFile:").unwrap_or(0),
            rss_shmem_bytes: Self::parse_proc_key_bytes(&status, "RssShmem:").unwrap_or(0),
            vm_data_bytes: Self::parse_proc_key_bytes(&status, "VmData:").unwrap_or(0),
            swap_bytes: Self::parse_proc_key_bytes(&status, "VmSwap:").unwrap_or(0),
        })
    }

    fn styx_pool_metrics(stats: styx::core::buffer::BufferPoolStats) -> StreamBufferPoolMetrics {
        StreamBufferPoolMetrics {
            chunk_size_bytes: stats.chunk_size as u64,
            free_buffers: stats.free as u64,
            free_bytes: stats.free_bytes as u64,
            max_free_buffers: stats.max_free as u64,
            retained_buffers: stats.retained as u64,
            retained_bytes: stats.retained_bytes as u64,
            in_use_buffers: stats.in_use as u64,
            in_use_bytes: stats.in_use_bytes as u64,
            peak_in_use_buffers: stats.peak_in_use as u64,
            peak_in_use_bytes: stats.peak_in_use_bytes as u64,
            hits: stats.hits,
            misses: stats.misses,
            allocations: stats.allocations,
        }
    }

    fn styx_memory_metrics(&self) -> Option<StreamMemoryMetrics> {
        let process = Self::process_memory_metrics();
        let runner = Some(self.runner_memory.snapshot()).filter(|metrics| {
            metrics.current_decoded_frame_bytes > 0
                || metrics.peak_decoded_frame_bytes > 0
                || metrics.current_raw_clone_bytes > 0
                || metrics.peak_raw_clone_bytes > 0
                || metrics.current_processed_frame_bytes > 0
                || metrics.peak_processed_frame_bytes > 0
                || metrics.current_frame_working_set_bytes > 0
                || metrics.peak_frame_working_set_bytes > 0
        });
        let pool_stats = self.session.as_ref().and_then(|session| session.handle()).map(|handle| handle.memory_stats());
        let capture_queue = pool_stats.as_ref().and_then(|stats| stats.capture_queue.as_ref().map(|queue| StreamQueueMemoryMetrics { depth: queue.depth, capacity: queue.capacity }));
        let external_backings: Vec<StreamExternalBackingMetrics> = pool_stats
            .as_ref()
            .map(|stats| {
                stats
                    .external_backings
                    .iter()
                    .map(|backing| StreamExternalBackingMetrics {
                        label: backing.label.clone(),
                        current_buffers: backing.current_buffers,
                        current_bytes: backing.current_bytes,
                        peak_buffers: backing.peak_buffers,
                        peak_bytes: backing.peak_bytes,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let transform_pool = pool_stats.as_ref().and_then(|stats| stats.transform_pool.clone()).map(Self::styx_pool_metrics);
        let image_pool = pool_stats.as_ref().and_then(|stats| stats.image_pool.clone()).map(Self::styx_pool_metrics);
        let packed_pools: Vec<StreamPackedPoolMetrics> = pool_stats
            .as_ref()
            .map(|stats| stats.packed_pools.iter().map(|pool| StreamPackedPoolMetrics { min_len_bytes: pool.min_len as u64, pool: Self::styx_pool_metrics(pool.stats.clone()) }).collect())
            .unwrap_or_default();
        let staging_copy = pool_stats
            .as_ref()
            .and_then(|stats| stats.staging_copy.as_ref().map(|staging| StreamStagingCopyMetrics { copies: staging.copies, bytes: staging.bytes, peak_copy_bytes: staging.peak_copy_bytes }));

        if process.is_none()
            && runner.is_none()
            && capture_queue.is_none()
            && external_backings.is_empty()
            && transform_pool.is_none()
            && image_pool.is_none()
            && packed_pools.is_empty()
            && staging_copy.is_none()
        {
            return None;
        }

        Some(StreamMemoryMetrics { process, runner, capture_queue, external_backings, transform_pool, image_pool, packed_pools, staging_copy })
    }

    fn metrics_stale_base_ms() -> u64 {
        std::env::var("HELIOS_STREAM_METRICS_STALE_MS").ok().and_then(|raw| raw.parse::<u64>().ok()).unwrap_or(1_500).clamp(250, 60_000)
    }

    fn stale_threshold_for_fps(fps: f64) -> Duration {
        let base = Self::metrics_stale_base_ms();
        let cadence_ms = if fps.is_finite() && fps > 0.0 { (4_000.0 / fps).round() as u64 } else { 0 };
        Duration::from_millis(base.max(cadence_ms).min(60_000))
    }

    fn mark_capture_metrics_stale(metrics: &mut crate::capture::CaptureStageMetrics, age: Duration) {
        metrics.fps = 0.0;
        metrics.average_time_ms = 0.0;
        metrics.last_time_ms = metrics.last_time_ms.max(age.as_secs_f64() * 1000.0);
    }

    fn mark_codec_metrics_stale(metrics: &mut CodecMetrics, age: Duration) {
        metrics.fps = 0.0;
        metrics.average_time_ms = 0.0;
        metrics.last_time_ms = metrics.last_time_ms.max(age.as_secs_f64() * 1000.0);
    }

    fn fourcc_code(fourcc: FourCc) -> String {
        String::from_utf8_lossy(&fourcc.to_u32().to_le_bytes()).to_string()
    }

    pub(super) fn unix_now_ms() -> u64 {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => duration.as_millis().min(u64::MAX as u128) as u64,
            Err(_) => 0,
        }
    }

    pub(super) fn ensure_encoder_selected(&mut self, handle: &styx::codec::CodecRegistryHandle, _capture_fourcc: FourCc) -> Result<()> {
        if self.encoder_id.is_none() || self.encode_fourcc.is_some() {
            return Ok(());
        }

        let encoder_selector = self.encoder_id.clone().unwrap_or_default();
        if encoder_selector.trim().is_empty() {
            return Err(Error::InvalidState("encoder selection was empty"));
        }
        let selector = encoder_selector.trim();

        // Resolve the user's selection either as an implementation name (e.g. "h264_v4l2m2m")
        // or as an algorithm family name (e.g. "h264", "h265", "mjpeg").
        //
        // Default to encoding from RG24 to keep output format consistent regardless of graph.
        let default_enc_input = FourCc::new(*b"RG24");
        let candidates = [default_enc_input];

        let mut chosen_input = None;
        let mut chosen_encoder = None;
        for input in candidates {
            // Prefer hardware encoders when available for the chosen family.
            handle.set_policy(CodecPolicy::builder(input).prefer_hardware(true).build());
            let resolved = handle
                .lookup_named_kind(input, CodecKind::Encoder, selector)
                .or_else(|_| handle.lookup_auto_kind_by_name(input, CodecKind::Encoder, selector))
                .or_else(|_| handle.lookup_auto_kind(input, CodecKind::Encoder));
            if let Ok(enc) = resolved {
                chosen_input = Some(input);
                chosen_encoder = Some(enc);
                break;
            }
        }

        let Some(enc_input) = chosen_input else {
            return Err(Error::InvalidState("encoder not found"));
        };
        let encoder = chosen_encoder.expect("encoder must exist when input was chosen");

        let desc = encoder.descriptor();
        let resolved_impl = desc.impl_name.to_string();
        self.encoder_impl = Some(resolved_impl.clone());
        self.encoder_input_fourcc = Some(enc_input);
        self.encode_fourcc = Some(desc.output);

        // Log the resolved encoder impl so on-device profiling can confirm we're on hardware codecs.
        let impl_lower = resolved_impl.to_ascii_lowercase();
        let looks_hw = impl_lower.contains("v4l2") || impl_lower.contains("vaapi") || impl_lower.contains("nvenc") || impl_lower.contains("qsv");
        if looks_hw {
            tracing::info!(encoder = %resolved_impl, input = ?enc_input, output = ?desc.output, "stream encoder selected (hardware)");
        } else {
            tracing::warn!(encoder = %resolved_impl, input = ?enc_input, output = ?desc.output, "stream encoder selected (may be software)");
        }

        let policy = CodecPolicy::builder(enc_input).ordered_impls([resolved_impl]).prefer_hardware(true).build();
        handle.set_policy(policy);
        Ok(())
    }

    pub(super) fn process_assigned_graphs(&self, image: image::DynamicImage, require_image_output: bool) -> Option<image::DynamicImage> {
        self.graph.process_with_options(image, crate::graph::GraphProcessOptions { require_image_output })
    }

    pub fn start(&mut self) -> Result<()> {
        if self.session.is_some() {
            return Err(Error::InvalidState("stream already running"));
        }
        // Reset capture timing state so a previous stall doesn't immediately re-trigger on restart.
        self.capture_empty_since = None;
        self.last_capture_ts = None;
        self.last_capture_wall = None;
        self.capture_started_wall = None;
        self.encoder_last_activity_ms.store(0, Ordering::Relaxed);
        let capture_fourcc = self.capture_input_fourcc().ok_or(Error::InvalidState("capture format unknown"))?;
        self.capture_fourcc = Some(capture_fourcc);

        tracing::info!(capture_fourcc = ?capture_fourcc, "starting stream runner");
        let session = CaptureSession::start(self.capture_config.clone()).map_err(|err| Error::InvalidStateOwned(err.to_string()))?;
        self.session = Some(session);
        self.capture_started_wall = Some(Instant::now());
        tracing::info!("capture session attached to stream runner");
        // `stop()` tears down the preview worker, and capture recovery restarts reuse this same
        // runner instance. Recreate the worker on start so preview shmem resumes after restarts.
        if self.preview_worker.is_none() && self.shmem.is_some() && self.preview_generation_enabled() {
            self.preview_worker = Some(super::PreviewWorker::start());
            self.last_preview_encode_wall = None;
            tracing::info!("preview worker restarted");
        }

        if (self.encoder_id.is_some() || self.decoder_id.is_some()) && self.codecs.is_none() {
            tracing::info!(encoder = self.encoder_id.as_deref().unwrap_or("none"), decoder = self.decoder_id.as_deref().unwrap_or("none"), "initializing codec registry");
            let registry = match CodecRegistry::with_enabled_codecs() {
                Ok(registry) => registry,
                Err(_) => {
                    self.stop();
                    return Err(Error::InvalidState("codec registry init failed"));
                }
            };
            let handle = registry.handle();
            tracing::info!("codec registry initialized");

            if let Some(dec_id) = self.decoder_id.as_ref() {
                tracing::info!(decoder = %dec_id, "resolving decoder");
                let selector = dec_id.trim();
                if selector.is_empty() {
                    return Err(Error::InvalidState("decoder selection was empty"));
                }

                // Resolve the selection either as an implementation name (e.g. "jpeg-decoder")
                // or as an algorithm family name (e.g. "mjpeg", "h264", "h265").
                let decoder = handle
                    .lookup_named_kind(capture_fourcc, CodecKind::Decoder, selector)
                    .or_else(|_| handle.lookup_auto_kind_by_name(capture_fourcc, CodecKind::Decoder, selector))
                    .map_err(|_| {
                        let capture = Self::fourcc_code(capture_fourcc);
                        Error::InvalidStateOwned(format!("decoder '{selector}' not available for capture {capture} ({capture_fourcc:?})"))
                    })?;

                let resolved_impl = decoder.descriptor().impl_name.to_string();
                self.decoder_id = Some(resolved_impl.clone());
                let policy_input = capture_fourcc;
                let policy = CodecPolicy::builder(policy_input).ordered_impls([resolved_impl.clone()]).build();
                handle.set_policy(policy);
                tracing::info!(decoder = %resolved_impl, input = ?policy_input, "decoder policy applied");
            }

            self.ensure_encoder_selected(&handle, capture_fourcc)?;

            self.codecs = Some(handle);
        }

        // Do not start the encoder worker eagerly. Encoding is a side-channel that should only
        // run when something is subscribed to the encoded stream (see `encoder_demand()`).
        // Preview/JPEG shmem is handled independently.
        tracing::info!("stream runner started");
        Ok(())
    }

    pub fn set_graph(&mut self, graph: GraphHandle) {
        self.graph = graph;
    }

    pub fn set_calibration(&mut self, calibration: Option<crate::ipc::StreamCalibration>) {
        self.graph.set_calibration(calibration);
    }

    pub fn set_pipeline_inputs(&mut self, pipeline_id: Option<Uuid>, inputs: &std::collections::BTreeMap<String, Option<serde_json::Value>>) {
        self.graph.set_pipeline_inputs(pipeline_id, inputs);
    }

    pub fn set_codecs(&mut self, decoder_id: Option<String>, encoder_id: Option<String>) {
        let decoder_id = if self.capture_config.backend == styx::BackendKind::Libcamera && self.capture_config.mode.format.code == FourCc::new(*b"NV12") {
            decoder_id.map(|id| if id.eq_ignore_ascii_case("yuyv-luma") { "nv12-luma".to_string() } else { id })
        } else {
            decoder_id
        };
        let decoder_changed = self.decoder_id != decoder_id;
        let encoder_changed = self.encoder_id != encoder_id;

        self.decoder_id = decoder_id;
        self.encoder_id = encoder_id;
        self.encoder_impl = None;

        self.encode_configured = false;
        self.encoder_input_fourcc = None;
        self.encode_fourcc = None;

        // Reset per-stage stats so subsequent metrics samples reflect the new configuration.
        self.decoder_stats = styx::codec::CodecStats::default();
        self.encoder_stats = styx::codec::CodecStats::default();
        self.last_decode_wall = None;
        self.last_encode_wall = None;
        self.encoder_last_activity_ms.store(0, Ordering::Relaxed);

        // Encoder worker depends on the selected codec; stop it and let the pump restart it when demanded.
        self.stop_encoder_worker();

        if decoder_changed || encoder_changed {
            // Pixel conversion helpers inside Styx cache per-thread buffers (including in the Rayon
            // global thread pool). Clear them when swapping codecs so RSS can fall back down after
            // benchmark loops.
            styx::codec::decoder::clear_packed_frame_pools_all_threads();
        }

        // If a codec registry is already initialized for this stream, update policies in-place
        // rather than recreating the registry. Reinitializing the registry can be very expensive
        // (and some codec backends don't reliably release all allocations), which can look like a
        // "leak" during codec benchmarks.
        if let Some(handle) = self.codecs.as_ref() {
            let capture_fourcc = self.capture_input_fourcc();
            if let Some(fourcc) = capture_fourcc {
                let policy = match self.decoder_id.as_ref().map(|s| s.trim()).filter(|s| !s.is_empty()) {
                    Some(impl_name) => CodecPolicy::builder(fourcc).ordered_impls([impl_name.to_string()]).build(),
                    None => CodecPolicy::builder(fourcc).prefer_hardware(true).build(),
                };
                handle.set_policy(policy);
            }

            // Reset encoder policy to "auto" for RG24 until ensure_encoder_selected runs.
            handle.set_policy(CodecPolicy::builder(FourCc::new(*b"RG24")).prefer_hardware(true).build());
        }
    }

    pub fn stop(&mut self) {
        self.stop_encoder_worker();
        self.encoder_last_activity_ms.store(0, Ordering::Relaxed);
        self.runner_memory.reset_current();
        self.capture_started_wall = None;
        self.capture_empty_since = None;
        self.last_capture_ts = None;
        self.last_capture_wall = None;
        if let Some(worker) = self.preview_worker.take() {
            worker.stop();
        }
        if let Some(mut session) = self.session.take() {
            session.stop();
        }

        // Clear per-thread packed frame pools so repeated start/stop + codec switching doesn't
        // permanently retain peak allocations.
        styx::codec::decoder::clear_packed_frame_pools_all_threads();
    }

    pub(crate) fn stop_capture_for_restart(&mut self) {
        self.encoder_last_activity_ms.store(0, Ordering::Relaxed);
        self.runner_memory.reset_current();
        self.capture_started_wall = None;
        self.capture_empty_since = None;
        self.last_capture_ts = None;
        self.last_capture_wall = None;
        if let Some(mut session) = self.session.take() {
            session.stop();
        }
        styx::codec::decoder::clear_packed_frame_pools_all_threads();
    }

    pub fn set_control(&mut self, id: crate::ipc::ControlId, value: CaptureControlValue) -> Result<()> {
        let Some(session) = &self.session else {
            return Err(Error::InvalidState("stream not running"));
        };
        session.set_control(id, value).map_err(|err| Error::InvalidStateOwned(err.to_string()))?;
        Ok(())
    }

    pub fn sync_capture_controls(&mut self, controls: Vec<ControlAssignment>, enable_tdn_output: bool) -> Result<()> {
        self.capture_config.controls = controls;
        self.capture_config.enable_tdn_output = enable_tdn_output;
        // File backend playback controls (speed/start/stop frame, image duration) are consumed
        // when the decode worker is configured. Reconfigure immediately so control updates apply
        // during active playback instead of waiting for the file loop boundary.
        if self.capture_config.backend == styx::BackendKind::File {
            if let Some(session) = self.session.as_mut() {
                session.reconfigure(self.capture_config.clone()).map_err(|err| Error::InvalidStateOwned(err.to_string()))?;
            }
        }
        Ok(())
    }

    pub fn controls(&self) -> Vec<CaptureControlInfo> {
        self.session.as_ref().map(|s| s.controls()).unwrap_or_default()
    }

    pub fn metrics(&self) -> StreamMetrics {
        let now = Instant::now();
        let mut capture = stage_to_capture_metrics(&self.capture_stats);
        let mut host = stage_to_capture_metrics(&self.graph.host().metrics());

        if let Some(last_capture) = self.last_capture_wall {
            let age = now.saturating_duration_since(last_capture);
            if age >= Self::stale_threshold_for_fps(capture.fps) {
                Self::mark_capture_metrics_stale(&mut capture, age);
            }
            if age >= Self::stale_threshold_for_fps(host.fps) {
                Self::mark_capture_metrics_stale(&mut host, age);
            }
        }

        let pipeline = self.graph.pipeline_metrics();
        let pipeline_instances = self.graph.pipeline_metrics_by_pipeline();
        let memory = self.styx_memory_metrics();
        let mut encoder = self.encode_fourcc.map(|_| to_codec_metrics(&self.encoder_stats));
        if let Some(metrics) = encoder.as_mut() {
            let activity_ms = self.encoder_last_activity_ms.load(Ordering::Relaxed);
            if activity_ms > 0 {
                let age = Duration::from_millis(Self::unix_now_ms().saturating_sub(activity_ms));
                if age >= Self::stale_threshold_for_fps(metrics.fps) {
                    Self::mark_codec_metrics_stale(metrics, age);
                }
            } else if let Some(last_encode) = self.last_encode_wall {
                let age = now.saturating_duration_since(last_encode);
                if age >= Self::stale_threshold_for_fps(metrics.fps) {
                    Self::mark_codec_metrics_stale(metrics, age);
                }
            }
        }
        // Always return a decoder metrics object so the UI can distinguish "not running yet"
        // from "running but currently idle / disabled downstream".
        let mut decoder = Some(to_codec_metrics(&self.decoder_stats));
        if let Some(metrics) = decoder.as_mut() {
            if let Some(last_decode) = self.last_decode_wall {
                let age = now.saturating_duration_since(last_decode);
                if age >= Self::stale_threshold_for_fps(metrics.fps) {
                    Self::mark_codec_metrics_stale(metrics, age);
                }
            }
        }
        StreamMetrics { capture, host, encoder, decoder, memory, pipeline, pipeline_instances }
    }

    pub fn descriptor(&self) -> Option<&CaptureDescriptor> {
        self.session.as_ref().and_then(|s| s.descriptor())
    }

    pub(crate) fn is_running(&self) -> bool {
        self.session.is_some()
    }

    pub fn host(&self) -> GraphHandle {
        self.graph.clone()
    }

    pub fn subscribe_encoded(&self) -> broadcast::Receiver<EncodedFrame> {
        self.encoded_tx.subscribe()
    }

    pub fn encoded_sender(&self) -> broadcast::Sender<EncodedFrame> {
        self.encoded_tx.clone()
    }

    pub fn subscribe_raw(&self) -> broadcast::Receiver<Arc<image::DynamicImage>> {
        self.raw_tx.subscribe()
    }

    pub fn raw_sender(&self) -> broadcast::Sender<Arc<image::DynamicImage>> {
        self.raw_tx.clone()
    }

    pub(super) fn encoder_demand(&mut self) -> bool {
        // Encoder work is an optional side-channel for encoded consumers.
        // Preview should stay on the lightweight preview worker path.
        self.encoded_tx.receiver_count() > 0
    }

    pub(super) fn preview_demand(&mut self) -> bool {
        let Some(stream_id) = self.stream_id else {
            return false;
        };
        // If both encoder + decoder are disabled, treat preview as disabled too.
        if self.encoder_id.is_none() && self.decoder_id.is_none() {
            return false;
        }

        // Avoid hitting the filesystem (mtime checks) on every frame.
        let now = Instant::now();
        if let Some(last) = self.last_viewer_check_wall {
            if now.saturating_duration_since(last) < self.viewer_check_interval {
                return self.viewer_recently_active;
            }
        }

        self.viewer_recently_active = crate::stream::preview_active_recently(stream_id, self.viewer_idle_timeout);
        self.last_viewer_check_wall = Some(now);
        self.viewer_recently_active
    }

    pub(super) fn stop_encoder_worker(&mut self) {
        if let Some(worker) = self.encoder_worker.take() {
            if let Some(shmem) = worker.stop() {
                self.shmem = Some(shmem);
            }
        }
    }

    pub(super) fn ensure_encoder_worker_started(&mut self) -> Result<()> {
        if self.encode_fourcc.is_none() {
            return Ok(());
        }
        if self.encoder_worker.is_some() {
            return Ok(());
        }
        let Some(encode_output) = self.encode_fourcc else {
            return Ok(());
        };
        let encode_input = self.encoder_input_fourcc.unwrap_or(FourCc::new(*b"RG24"));
        let codecs = self.ensure_codecs_for_decode()?;
        if !self.encode_configured {
            if let Err(err) = self.configure_encoder(&codecs, encode_input, encode_output) {
                tracing::warn!(error = %err, "failed to apply encoder settings");
            } else {
                self.encode_configured = true;
            }
        }
        // Keep preview shmem owned by the preview worker when available so `/preview` and
        // `/stream.mjpeg` do not depend on ffmpeg encoder throughput.
        let use_preview_shmem = self.preview_worker.is_some();
        let encoder_shmem = if use_preview_shmem { None } else { self.shmem.take() };
        let encoder_selector = self.encoder_id.clone().unwrap_or_default();
        let encoder = self
            .build_ffmpeg_encoder_for_worker(encode_input)
            .or_else(|| {
                codecs
                    .lookup_named_kind(encode_input, CodecKind::Encoder, encoder_selector.as_str())
                    .or_else(|_| codecs.lookup_auto_kind_by_name(encode_input, CodecKind::Encoder, encoder_selector.as_str()))
                    .ok()
            })
            .ok_or(Error::InvalidState("selected encoder not available"))?;
        let worker = super::super::encoder_worker::EncoderWorker::start(EncoderWorkerStart {
            stream_label: self.stream_label.as_ref().to_string(),
            encoder,
            encode_input,
            encode_output,
            encoder_settings: self.encoder_settings.clone(),
            shmem: encoder_shmem,
            encoded_tx: self.encoded_tx.clone(),
            encoder_stats: self.encoder_stats.clone(),
            activity_ms: self.encoder_last_activity_ms.clone(),
        })?;
        self.encoder_worker = Some(worker);
        Ok(())
    }

    pub(crate) fn uses_usb_v4l2_capture(&self) -> bool {
        self.capture_config.backend == styx::BackendKind::V4l2 && self.capture_config.device_keys.iter().any(|key| key.to_ascii_lowercase().contains("usb"))
    }

    pub(crate) fn try_usb_power_recovery(&self) -> bool {
        if !self.uses_usb_v4l2_capture() {
            return false;
        }
        let script = std::env::var("HELIOS_USB_POWER_SETUP_SCRIPT").unwrap_or_else(|_| "/usr/local/bin/helios-usb-power-setup.sh".to_string());
        if !std::path::Path::new(&script).exists() {
            tracing::warn!(script = %script, "usb power recovery script not found");
            return false;
        }
        let settle_ms = std::env::var("HELIOS_USB_POWER_RECOVERY_SETTLE_MS").ok().and_then(|raw| raw.parse::<u64>().ok()).unwrap_or(2_500).clamp(100, 10_000);
        let run = |enabled: bool| -> bool {
            let status = Command::new(&script).env("USB_POWER_USB_A_ENABLED", if enabled { "1" } else { "0" }).status();
            match status {
                Ok(status) if status.success() => true,
                Ok(status) => {
                    tracing::warn!(script = %script, enabled, status = ?status.code(), "usb power recovery command failed");
                    false
                }
                Err(err) => {
                    tracing::warn!(script = %script, enabled, error = %err, "usb power recovery command failed");
                    false
                }
            }
        };

        tracing::warn!(script = %script, settle_ms, "attempting usb power cycle recovery for stalled capture");
        if !run(false) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(settle_ms));
        if !run(true) {
            return false;
        }
        // Give the USB bus a moment to enumerate before restarting capture.
        std::thread::sleep(Duration::from_millis(settle_ms));
        true
    }

    fn software_encoder_thread_limit() -> usize {
        if let Some(value) = std::env::var("HELIOS_SOFTWARE_ENCODER_THREADS").ok().and_then(|raw| raw.parse::<usize>().ok()).filter(|value| *value > 0) {
            return value;
        }

        // Software H.264/H.265 fallback was previously pinned to 1 thread, which can collapse
        // throughput and cause persistent encoder backpressure on CM5. Keep one CPU for capture/
        // graph work while still allowing parallel encode.
        let cores = std::thread::available_parallelism().map(|value| value.get()).unwrap_or(1);
        let reserve = if cores >= 4 { 1 } else { 0 };
        cores.saturating_sub(reserve).clamp(1, 4)
    }

    fn build_ffmpeg_encoder_for_worker(&self, encode_input: FourCc) -> Option<Arc<dyn Codec>> {
        if !self.encoder_impl.as_deref().is_some_and(|impl_name| impl_name.eq_ignore_ascii_case("ffmpeg")) {
            return None;
        }

        let mut opts = FfmpegEncoderOptions::default();
        if let Some(settings) = self.encoder_settings.as_ref() {
            if let Some(bitrate) = settings.bitrate.filter(|value| *value > 0) {
                opts.bitrate = bitrate;
            }
            if let Some(gop) = settings.gop.filter(|value| *value > 0) {
                opts.gop = Some(gop);
            }
            if let Some(rate) = settings.framerate.as_ref().and_then(|rate| if rate.numerator > 0 && rate.denominator > 0 { Some((rate.numerator, rate.denominator)) } else { None }) {
                opts.framerate = Some(rate);
            }
            opts.thread_count = settings.thread_count.filter(|value| *value > 0);
            if let Some(out) = settings.output_resolution.as_ref().and_then(|res| styx::prelude::Resolution::new(res.width, res.height)) {
                opts.output_resolution = Some(out);
            }
        }
        if opts.thread_count.is_none() {
            opts.thread_count = Some(Self::software_encoder_thread_limit());
        }

        let make = |codec: FourCc| -> Option<Arc<dyn Codec>> {
            if codec == FourCc::new(*b"H264") {
                return FfmpegH264Encoder::with_options_for_input(encode_input, opts).ok().map(|encoder| Arc::new(encoder) as Arc<dyn Codec>);
            }
            if matches!(&codec.to_u32().to_le_bytes(), b"H265" | b"HEVC") {
                return FfmpegH265Encoder::with_options_for_input(encode_input, opts).ok().map(|encoder| Arc::new(encoder) as Arc<dyn Codec>);
            }
            if matches!(&codec.to_u32().to_le_bytes(), b"MJPG" | b"JPEG") {
                return FfmpegMjpegEncoder::with_options_for_input(encode_input, opts).ok().map(|encoder| Arc::new(encoder) as Arc<dyn Codec>);
            }
            None
        };

        self.encode_fourcc.and_then(make)
    }

    pub(super) fn stream_label(&self) -> &str {
        self.stream_label.as_ref()
    }

    fn preview_generation_enabled(&self) -> bool {
        !(self.encoder_id.is_none() && self.decoder_id.is_none() && self.capture_config.backend != styx::BackendKind::File)
    }

    pub(super) fn is_encoded_preview_fourcc(fourcc: FourCc) -> bool {
        matches!(&fourcc.to_u32().to_le_bytes(), b"MJPG" | b"JPEG" | b"H264" | b"H265" | b"HEVC")
    }

    pub(super) fn record_decode_ms(&self, decode_start: Instant) {
        self.decoder_stats.record_duration(decode_start.elapsed());
        let decode_ms = decode_start.elapsed().as_secs_f64() * 1000.0;
        histogram!("helios.stream.decode_ms", "stream" => self.stream_label.clone()).record(decode_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::StreamRunner;

    #[test]
    fn parse_proc_key_bytes_reads_kib_and_plain_values() {
        let text = "VmRSS:\t1234 kB\nThreads:\t7\n";
        assert_eq!(StreamRunner::parse_proc_key_bytes(text, "VmRSS:"), Some(1_263_616));
        assert_eq!(StreamRunner::parse_proc_key_bytes(text, "Threads:"), Some(7));
        assert_eq!(StreamRunner::parse_proc_key_bytes(text, "VmSize:"), None);
    }
}
