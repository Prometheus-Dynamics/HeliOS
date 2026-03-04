use std::sync::atomic::Ordering;
use std::sync::mpsc::TrySendError;
use std::sync::Arc;
use std::time::{Duration, Instant};

use metrics::histogram;
use styx::capture::prelude::RecvOutcome;
use styx::codec::decoder::{frame_lease_to_dynamic_image, frame_to_dynamic_image};
use styx::codec::CodecKind;
use tracing::trace_span;

use crate::error::{Error, Result};

use super::StreamRunner;
use styx::prelude::{transform_packed_frame, FourCc, FrameTransform, Rotation90};

const CAPTURE_IDLE_SLEEP: Duration = Duration::from_millis(1);
const DEFAULT_CAPTURE_STALL_MS: u64 = 1_500;
const DEFAULT_CAPTURE_ACTIVE_STALL_MS: u64 = 1_000;
const DEFAULT_CAPTURE_FIRST_FRAME_STALL_MS: u64 = 8_000;

impl StreamRunner {
    fn control_value_to_u64(value: &styx::core::controls::ControlValue) -> Option<u64> {
        match value {
            styx::core::controls::ControlValue::Uint(v) => Some(*v as u64),
            styx::core::controls::ControlValue::Int(v) if *v >= 0 => Some(*v as u64),
            _ => None,
        }
    }

    fn file_replay_has_offset_start_frame(&self) -> bool {
        if self.capture_config.backend != styx::BackendKind::File {
            return false;
        }
        let Some(session) = self.session.as_ref() else {
            return false;
        };
        let Some(descriptor) = session.descriptor() else {
            return false;
        };

        descriptor.controls.iter().any(|meta| {
            if !meta.name.ends_with(".start_frame") {
                return false;
            }
            let configured = self.capture_config.controls.iter().find(|ctl| ctl.id == meta.id.0).map(|ctl| &ctl.value).unwrap_or(&meta.default);
            Self::control_value_to_u64(configured).is_some_and(|value| value > 0)
        })
    }

    fn preview_output_resolution_hint(&self) -> Option<(u32, u32)> {
        self.encoder_settings.as_ref().and_then(|settings| settings.output_resolution.as_ref()).and_then(|resolution| {
            if resolution.width > 0 && resolution.height > 0 {
                Some((resolution.width, resolution.height))
            } else {
                None
            }
        })
    }

    fn decoder_frame_transform(&self) -> Option<FrameTransform> {
        let settings = self.decoder_settings.as_ref()?;
        let rotation_raw = settings.rotation_degrees.unwrap_or(0);
        let mirror = settings.mirror_horizontal.unwrap_or(false);
        let rotation = match rotation_raw.rem_euclid(360) {
            0 => Rotation90::Deg0,
            90 => Rotation90::Deg90,
            180 => Rotation90::Deg180,
            270 => Rotation90::Deg270,
            _ => {
                tracing::warn!(rotation_raw, "invalid decoder rotation; expected 0/90/180/270");
                return None;
            }
        };
        if rotation == Rotation90::Deg0 && !mirror {
            return None;
        }
        Some(FrameTransform { rotation, mirror })
    }

    fn apply_image_transform(image: image::DynamicImage, transform: FrameTransform) -> image::DynamicImage {
        let rotated = match transform.rotation {
            Rotation90::Deg0 => image,
            Rotation90::Deg90 => image.rotate90(),
            Rotation90::Deg180 => image.rotate180(),
            Rotation90::Deg270 => image.rotate270(),
        };
        if transform.mirror {
            rotated.fliph()
        } else {
            rotated
        }
    }

    fn encoded_family(fourcc: FourCc) -> Option<&'static str> {
        match &fourcc.to_u32().to_le_bytes() {
            b"H264" => Some("h264"),
            b"H265" | b"HEVC" => Some("h265"),
            b"MJPG" | b"JPEG" => Some("mjpeg"),
            _ => None,
        }
    }

    fn software_encoder_fps_cap() -> Option<f64> {
        // Cap software ffmpeg encoder throughput by default so capture/pipeline work remains
        // realtime under sustained encoded demand (for example shadow recorder subscriptions).
        // Set to 0 (or negative) to disable the cap.
        std::env::var("HELIOS_SOFTWARE_ENCODER_FPS_CAP").ok().and_then(|value| value.parse::<f64>().ok()).or(Some(12.0)).filter(|value| value.is_finite() && *value > 0.0)
    }

    fn effective_encode_fps_limit(&self) -> Option<f64> {
        let mut limit = self.encode_fps_limit;
        if self.encoder_impl.as_deref().is_some_and(|impl_name| impl_name.eq_ignore_ascii_case("ffmpeg")) {
            if let Some(cap) = Self::software_encoder_fps_cap() {
                limit = Some(limit.map_or(cap, |value| value.min(cap)));
            }
        }
        limit
    }

    fn can_passthrough_encoded_capture(&self, capture_fourcc: FourCc) -> bool {
        if self.graph.has_executor() || !Self::is_encoded_preview_fourcc(capture_fourcc) {
            return false;
        }
        let Some(encode_fourcc) = self.encode_fourcc else {
            return false;
        };
        if Self::encoded_family(capture_fourcc) != Self::encoded_family(encode_fourcc) {
            return false;
        }
        let tuning_requires_transcode = self.encoder_settings.as_ref().is_some_and(|settings| {
            settings.bitrate.is_some_and(|value| value > 0)
                || settings.gop.is_some_and(|value| value > 0)
                || settings.framerate.as_ref().is_some_and(|rate| rate.numerator > 0 && rate.denominator > 0)
                || settings.output_resolution.as_ref().is_some_and(|resolution| resolution.width > 0 && resolution.height > 0)
        });
        !tuning_requires_transcode
    }

    fn non_looping_file_replay(&self) -> bool {
        if self.capture_config.backend != styx::BackendKind::File {
            return false;
        }
        match &self.capture_config.handle {
            styx::BackendHandle::File { loop_forever, .. } => !*loop_forever,
            _ => false,
        }
    }

    /// Pull one frame from capture and forward to the host bridge.
    pub fn pump_host_once(&mut self) -> Result<bool> {
        let span = trace_span!("stream_pump", stream = %self.stream_label());
        let _guard = span.enter();

        self.poll_preview_worker();

        let Some(session) = &self.session else {
            return Err(Error::InvalidState("stream not running"));
        };
        let pump_start = Instant::now();
        let handle = session.handle().ok_or(Error::InvalidState("capture handle missing"))?;
        let mut outcome = handle.recv();
        // If the capture thread hasn't produced a frame yet, back off very briefly
        // and retry quickly to avoid missing frames.
        if matches!(outcome, RecvOutcome::Empty) {
            std::thread::sleep(CAPTURE_IDLE_SLEEP);
            outcome = handle.recv();
        }
        match outcome {
            RecvOutcome::Data(frame) => {
                self.capture_empty_since = None;
                if self.last_capture_wall.is_none() {
                    if let Some(started) = self.capture_started_wall {
                        let startup_ms = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
                        tracing::info!(startup_ms, "stream capture delivered first frame");
                    }
                }
                // If the capture backend is already producing an encoded bitstream (MJPEG/H264/H265),
                // publish it into shmem directly so HTTP preview endpoints can stream with minimal CPU.
                // Skip this fast-path when a graph is active so preview shows processed output.
                if !self.graph.has_executor() {
                    self.try_write_shmem_preview_from_capture(&frame);
                }
                // Capture metrics should reflect frame cadence, not queue wait time.
                // Prefer source timestamps when available; fall back to wall clock deltas.
                let now = Instant::now();
                let ts = frame.meta().timestamp;
                if let Some(prev) = self.last_capture_ts {
                    let ts_delta = ts.saturating_sub(prev);
                    // Libcamera timestamps are commonly in microseconds; some backends use nanoseconds.
                    // Detect likely units based on the delta magnitude for a single frame.
                    if (1_000..=1_000_000).contains(&ts_delta) {
                        self.capture_stats.record(Duration::from_micros(ts_delta));
                    } else if (1_000_000..=5_000_000_000).contains(&ts_delta) {
                        self.capture_stats.record(Duration::from_nanos(ts_delta));
                    } else if let Some(prev_wall) = self.last_capture_wall {
                        self.capture_stats.record(now.saturating_duration_since(prev_wall));
                    }
                } else if let Some(prev_wall) = self.last_capture_wall {
                    self.capture_stats.record(now.saturating_duration_since(prev_wall));
                }
                self.last_capture_ts = Some(ts);
                self.last_capture_wall = Some(now);

                let capture_fourcc = frame.meta().format.code;
                let can_passthrough_encoded = self.can_passthrough_encoded_capture(capture_fourcc);

                // Shadow recorder (and other encoded subscribers) want a bytestream. When the capture
                // backend already produces an encoded stream, forward it directly into the encoded
                // broadcast so recording can be nearly-zero CPU (no decode/re-encode).
                //
                // Keep this limited to "no graph" mode: once a graph is active, callers expect the
                // encoded stream to reflect processed output, which requires the encoder worker.
                if can_passthrough_encoded && self.encoder_demand() {
                    if let Some(plane) = frame.planes().first() {
                        let data = Arc::<[u8]>::from(plane.data());
                        let _ = self.encoded_tx.send(crate::stream::EncodedFrame { data, ts_ms: Self::unix_now_ms() });
                        self.encoder_stats.inc_processed();
                        // Feed throughput samples for encoded pass-through paths where the encoder
                        // worker is bypassed and thus would not call `record_duration`.
                        self.encoder_stats.record_duration(Duration::from_micros(1));
                        self.last_encode_wall = Some(Instant::now());
                        self.encoder_last_activity_ms.store(Self::unix_now_ms(), Ordering::Relaxed);
                    }
                }
                // When the backend already provides encoded frames and there is no graph, skip
                // expensive decode/graph work unless someone is subscribed to decoded frames.
                if can_passthrough_encoded {
                    let decoded_demand = self.raw_tx.receiver_count() > 0 || self.graph.host().receiver_count() > 0;
                    if !decoded_demand {
                        return Ok(true);
                    }
                }

                if let Some(limit) = self.decode_fps_limit {
                    let min_gap = Duration::from_secs_f64(1.0 / limit.max(f64::EPSILON));
                    if let Some(last) = self.last_decode_wall {
                        if last.elapsed() < min_gap {
                            return Ok(true);
                        }
                    }
                    self.last_decode_wall = Some(Instant::now());
                }
                let decode_start = Instant::now();
                let mut transform_applied = false;
                let transform = self.decoder_frame_transform();
                let mut frame = frame;
                if let Some(transform) = transform {
                    if let Ok(transformed) = transform_packed_frame(&frame, transform) {
                        frame = transformed;
                        transform_applied = true;
                    }
                }

                let image = match frame_lease_to_dynamic_image(frame) {
                    Ok(mut img) => {
                        if let Some(transform) = transform {
                            if !transform_applied {
                                img = Self::apply_image_transform(img, transform);
                            }
                        }
                        img
                    }
                    Err(frame) => {
                        // Strict semantics: if the user hasn't selected a decoder, do not do any
                        // codec-backed decode for encoded capture inputs.
                        if self.decoder_id.is_none() {
                            tracing::debug!(fourcc = ?capture_fourcc, "decode disabled; skipping frame");
                            return Ok(true);
                        }
                        // Try decoding via codec registry if direct conversion failed (e.g. encoded input).
                        let codecs = self.ensure_codecs_for_decode()?;
                        let fourcc = frame.meta().format.code;
                        match codecs.process_auto_kind(fourcc, CodecKind::Decoder, frame) {
                            Ok(decoded) => {
                                let mut decoded = decoded;
                                if let Some(transform) = transform {
                                    if let Ok(transformed) = transform_packed_frame(&decoded, transform) {
                                        decoded = transformed;
                                        transform_applied = true;
                                    }
                                }
                                match frame_lease_to_dynamic_image(decoded) {
                                    Ok(mut img) => {
                                        if let Some(transform) = transform {
                                            if !transform_applied {
                                                img = Self::apply_image_transform(img, transform);
                                            }
                                        }
                                        img
                                    }
                                    Err(decoded) => {
                                        let decoded_fourcc = decoded.meta().format.code;
                                        let decoded_planes = decoded.planes();
                                        let decoded_plane_lens: Vec<_> = decoded_planes.iter().map(|p| p.data().len()).collect();
                                        let decoded_plane_strides: Vec<_> = decoded_planes.iter().map(|p| p.stride()).collect();
                                        drop(decoded_planes);

                                        match frame_to_dynamic_image(&decoded) {
                                            Some(mut img) => {
                                                if let Some(transform) = transform {
                                                    if !transform_applied {
                                                        img = Self::apply_image_transform(img, transform);
                                                    }
                                                }
                                                img
                                            }
                                            None => {
                                                self.decoder_stats.inc_errors();
                                                tracing::warn!(
                                                    fourcc = ?decoded_fourcc,
                                                    plane_lens = ?decoded_plane_lens,
                                                    plane_strides = ?decoded_plane_strides,
                                                    "decoded frame conversion failed"
                                                );
                                                return Ok(true);
                                            }
                                        }
                                    }
                                }
                            }
                            Err(err) => {
                                self.decoder_stats.inc_errors();
                                tracing::warn!(error = ?err, "frame decode failed for fourcc {:?}", fourcc);
                                return Ok(true);
                            }
                        }
                    }
                };
                self.decoder_stats.inc_processed();
                self.record_decode_ms(decode_start);
                self.last_decode_wall = Some(Instant::now());

                if self.raw_tx.receiver_count() > 0 {
                    let _ = self.raw_tx.send(Arc::new(image.clone()));
                }

                // Let the graph/executor transform the frame.
                let preview_active = self.preview_demand();
                let should_write_preview = self.encode_fourcc.is_none() || preview_active;
                let graph_start = Instant::now();
                let processed = match self.process_assigned_graphs(image) {
                    Some(img) => img,
                    None => {
                        // If the graph fails to produce an output (e.g. execution error),
                        // keep the preview alive with the last good frame instead of cloning
                        // the current decoded frame (full-res clones can inflate RSS).
                        if should_write_preview && self.preview_worker.is_some() && self.shmem.is_some() {
                            if let Some(fallback) = self.last_preview_frame.clone() {
                                let host = self.graph.host();
                                if host.receiver_count() > 0 {
                                    host.send_frame(fallback.clone());
                                }
                                self.try_write_shmem_preview_from_image(fallback, ts);
                            }
                        }
                        return Ok(true);
                    }
                };
                let graph_ms = graph_start.elapsed().as_secs_f64() * 1000.0;
                histogram!("helios.stream.graph_ms", "stream" => self.stream_label.clone()).record(graph_ms);

                // If an encoder is configured, ensure the codec registry (and encoder output fourcc)
                // is initialized even when we didn't need the registry for decode.
                if self.encoder_id.is_some() && (self.encode_fourcc.is_none() || self.encoder_demand()) {
                    match self.ensure_codecs_for_decode() {
                        Ok(codecs) => {
                            if let Err(err) = self.ensure_encoder_selected(&codecs, capture_fourcc) {
                                tracing::warn!(error = %err, "encoder selection failed");
                            }
                        }
                        Err(err) => tracing::warn!(error = %err, "codec registry init failed"),
                    }
                }

                // Wrap in Arc so preview + encoder can share without cloning pixel buffers.
                let processed = Arc::new(processed);

                // Only publish to host subscribers when at least one is connected; otherwise this is
                // wasted work and can inflate memory usage via broadcast buffering for full-res frames.
                let host = self.graph.host();
                if host.receiver_count() > 0 {
                    host.send_frame(processed.clone());
                }
                if should_write_preview && self.preview_worker.is_some() && self.shmem.is_some() {
                    self.try_write_shmem_preview_from_image(processed.clone(), ts);
                }
                if preview_active {
                    self.last_preview_frame = Some(processed.clone());
                }

                // Encoder is an optional side-channel; if it’s backed up, drop frames *before*
                // doing any extra work.
                if self.encode_fourcc.is_some() {
                    // Start the encoder only when something is actively subscribed to encoded output.
                    // Preview shmem generation is handled independently by the preview worker.
                    let wants_encode = self.encoder_demand();
                    if wants_encode {
                        if can_passthrough_encoded {
                            self.stop_encoder_worker();
                        } else {
                            if let Err(err) = self.ensure_encoder_worker_started() {
                                tracing::warn!(error = %err, "encoder worker start failed");
                            }
                            if let Some(worker) = self.encoder_worker.as_ref() {
                                if let Some(limit) = self.effective_encode_fps_limit() {
                                    let min_gap = Duration::from_secs_f64(1.0 / limit.max(f64::EPSILON));
                                    if let Some(last) = self.last_encode_wall {
                                        if last.elapsed() < min_gap {
                                            self.encoder_stats.inc_backpressure();
                                            return Ok(true);
                                        }
                                    }
                                }
                                // If the encoder queue is full, just drop this frame for encoding.
                                if !worker.try_send_image(processed.clone(), ts) {
                                    self.encoder_stats.inc_backpressure();
                                } else {
                                    self.last_encode_wall = Some(Instant::now());
                                }
                            }
                        }
                    } else {
                        self.stop_encoder_worker();
                    }
                }
                Ok(true)
            }
            RecvOutcome::Empty => {
                let first_frame_stall_ms =
                    std::env::var("HELIOS_CAPTURE_FIRST_FRAME_STALL_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(DEFAULT_CAPTURE_FIRST_FRAME_STALL_MS).clamp(1_000, 120_000);
                let now = Instant::now();
                let awaiting_first_frame = self.last_capture_wall.is_none();
                let first_frame_elapsed_ms = self.capture_started_wall.map(|started| now.saturating_duration_since(started).as_millis().min(u64::MAX as u128) as u64);
                if awaiting_first_frame {
                    if let Some(elapsed_ms) = first_frame_elapsed_ms {
                        if elapsed_ms < first_frame_stall_ms {
                            return Ok(false);
                        }
                        tracing::warn!(first_frame_stall_ms, first_frame_elapsed_ms = elapsed_ms, capture_started_wall_ms = elapsed_ms, "capture queue remained empty while waiting for first frame");
                        return Err(Error::InvalidState("capture stalled"));
                    }
                }
                let stall_ms = std::env::var("HELIOS_CAPTURE_STALL_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(DEFAULT_CAPTURE_STALL_MS).clamp(50, 10_000);
                let active_stall_ms = std::env::var("HELIOS_CAPTURE_ACTIVE_STALL_MS").ok().and_then(|v| v.parse::<u64>().ok()).unwrap_or(DEFAULT_CAPTURE_ACTIVE_STALL_MS).clamp(250, 10_000);
                let interactive_demand = self.preview_demand() || self.encoder_demand() || self.graph.host().receiver_count() > 0;
                let file_offset_start = self.file_replay_has_offset_start_frame();
                // Seeking to late frames on software-decoded file replay can take multiple seconds
                // before first output. Do not apply the aggressive interactive stall threshold in
                // that state or we can end up in endless capture restart thrash.
                let interactive_fast_recovery = interactive_demand && !file_offset_start;
                let effective_stall_ms = if interactive_fast_recovery { stall_ms.min(active_stall_ms) } else { stall_ms };
                let last_interval_ms = self.capture_stats.last_millis().unwrap_or(0.0);
                let config_interval_ms = self
                    .capture_config
                    .target_fps
                    .filter(|fps| *fps > 0)
                    .map(|fps| 1000.0 / fps as f64)
                    .or_else(|| {
                        self.capture_config.interval.map(|interval| {
                            let num = interval.numerator.get() as f64;
                            let den = interval.denominator.get() as f64;
                            if den > 0.0 {
                                1000.0 * num / den
                            } else {
                                0.0
                            }
                        })
                    })
                    .unwrap_or(0.0);
                let cadence_ms = if last_interval_ms > 0.0 { last_interval_ms } else { config_interval_ms };
                let cadence_stall_ms = if cadence_ms > 0.0 { (cadence_ms * 4.0).round() as u64 } else { 0 };
                // Keep a higher floor when idle/background, but recover aggressively in interactive mode.
                let stall_floor_ms = if interactive_fast_recovery { 750 } else { 2_000 };
                let replay_seek_floor_ms = if file_offset_start { 12_000 } else { 0 };
                let stall_threshold_ms = effective_stall_ms.max(cadence_stall_ms).max(stall_floor_ms).max(replay_seek_floor_ms).min(60_000);
                let non_looping_file_replay = self.non_looping_file_replay();
                let now = Instant::now();
                let empty_since = self.capture_empty_since.get_or_insert(now);
                if now.saturating_duration_since(*empty_since) >= Duration::from_millis(stall_threshold_ms) {
                    // Non-looping media replay naturally reaches EOF and then stays empty.
                    // Keep the stream alive/idle instead of thrashing capture restarts forever.
                    if non_looping_file_replay {
                        tracing::info!(stall_threshold_ms, base_stall_ms = stall_ms, effective_stall_ms, interactive_demand, cadence_stall_ms, "file replay reached EOF; keeping stream idle");
                        *empty_since = now;
                        std::thread::sleep(Duration::from_millis(100));
                        return Ok(false);
                    }
                    // A permanently-empty capture queue appears as a frozen stream even though the
                    // thread is "alive". Treat this as recoverable so the stream worker restart
                    // path can reinitialize capture.
                    tracing::warn!(
                        stall_threshold_ms,
                        base_stall_ms = stall_ms,
                        effective_stall_ms,
                        stall_floor_ms,
                        file_offset_start,
                        interactive_demand,
                        cadence_stall_ms,
                        last_interval_ms,
                        config_interval_ms,
                        last_capture_ts = ?self.last_capture_ts,
                        last_capture_wall_ms = self.last_capture_wall.map(|ts| ts.elapsed().as_millis()),
                        target_fps = ?self.capture_config.target_fps,
                        interval = ?self.capture_config.interval,
                        "capture queue empty beyond stall threshold"
                    );
                    return Err(Error::InvalidState("capture stalled"));
                }
                Ok(false)
            }
            RecvOutcome::Closed => Err(Error::InvalidState("capture closed")),
        }
        .inspect(|_result| {
            let pump_ms = pump_start.elapsed().as_secs_f64() * 1000.0;
            histogram!("helios.stream.pump_ms", "stream" => self.stream_label.clone()).record(pump_ms);
        })
    }

    fn try_write_shmem_preview_from_image(&mut self, image: Arc<image::DynamicImage>, ts: u64) {
        let now = Instant::now();
        if let Some(last) = self.last_preview_encode_wall {
            if now.saturating_duration_since(last) < self.preview_encode_interval {
                return;
            }
        }
        let Some(worker) = self.preview_worker.as_ref() else {
            return;
        };
        let output_resolution = self.preview_output_resolution_hint();
        match worker.req_tx.try_send(super::PreviewEncodeRequest { ts, image, output_resolution }) {
            Ok(()) => {
                self.last_preview_encode_wall = Some(now);
            }
            Err(TrySendError::Full(_)) => {}
            Err(TrySendError::Disconnected(req)) => {
                tracing::warn!("preview worker channel disconnected; restarting preview worker");
                self.preview_worker = Some(super::PreviewWorker::start());
                if let Some(worker) = self.preview_worker.as_ref() {
                    if worker.req_tx.try_send(req).is_ok() {
                        self.last_preview_encode_wall = Some(now);
                    }
                }
            }
        }
    }

    fn try_write_shmem_preview_from_capture(&mut self, frame: &styx::prelude::FrameLease) {
        // If both encoder + decoder are disabled, do not expose any preview output.
        if self.encoder_id.is_none() && self.decoder_id.is_none() {
            return;
        }
        if !self.preview_demand() {
            return;
        }
        let Some(shmem) = self.shmem.as_mut() else {
            return;
        };
        let meta = frame.meta();
        let fourcc = meta.format.code;
        if !Self::is_encoded_preview_fourcc(fourcc) {
            return;
        }
        let res = meta.format.resolution;
        let dims = (res.width.get(), res.height.get());
        let planes = frame.planes();
        let Some(plane) = planes.first() else {
            return;
        };
        let data = plane.data();
        let ts = meta.timestamp;
        if let Err(err) = shmem.write(Some(ts), Some(fourcc), dims, data) {
            tracing::warn!(error = %err, fourcc = ?fourcc, "preview shmem write failed");
        }
    }

    fn poll_preview_worker(&mut self) {
        let Some(worker) = self.preview_worker.as_ref() else {
            return;
        };
        let Some(shmem) = self.shmem.as_mut() else {
            // If the encoder worker claimed shmem, drop any pending results.
            while worker.res_rx.try_recv().is_ok() {}
            return;
        };

        // Drain to keep only the latest frame (channel is size-1, but be robust).
        while let Ok(res) = worker.res_rx.try_recv() {
            let fourcc = FourCc::new(*b"JPEG");
            if let Err(err) = shmem.write(Some(res.ts), Some(fourcc), res.dims, &res.jpeg) {
                tracing::warn!(error = %err, "preview shmem write failed");
                break;
            }
        }
    }
}
