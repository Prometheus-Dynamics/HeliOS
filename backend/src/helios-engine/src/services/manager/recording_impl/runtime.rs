use super::*;

impl RecordingEncoderWorker {
    pub(crate) fn start(config: RecordingEncoderConfig) -> std::result::Result<Self, String> {
        let mailbox = Arc::new(LatestFrameMailbox::new(recording_frame_queue_size()));
        let rx = Arc::clone(&mailbox);
        let join = std::thread::Builder::new()
            .name("helios-recording".into())
            .stack_size(policy::recording_worker_stack_size_bytes())
            .spawn(move || run_recording_encoder(rx, config))
            .map_err(|err| format!("recording worker spawn failed: {err}"))?;
        Ok(Self { mailbox, join: Some(join) })
    }

    pub(crate) fn send_image(&self, image: Arc<image::DynamicImage>, ts_ms: u64) {
        self.mailbox.push_frame(image, ts_ms);
    }

    pub(crate) fn stop(mut self) -> std::result::Result<RecordingStats, String> {
        self.mailbox.stop();
        let join = self.join.take().ok_or_else(|| "recording worker join missing".to_string())?;
        join.join().map_err(|_| "recording worker join failed".to_string())?
    }
}

impl ShadowRecorderWorker {
    pub(crate) fn start(config: ShadowRecorderConfig) -> std::result::Result<Self, String> {
        // Small bounded queue. Shadow recorder must not stall the stream loop, but also should not
        // drop nearly all chunks under momentary IO/cpu hiccups.
        let (tx, rx) = sync_channel::<ShadowRecorderJob>(32);
        let join = std::thread::Builder::new()
            .name("helios-shadow-record".into())
            .stack_size(policy::recording_worker_stack_size_bytes())
            .spawn(move || run_shadow_recorder_worker(rx, config))
            .map_err(|err| format!("shadow recorder spawn failed: {err}"))?;
        Ok(Self { tx, join: Some(join) })
    }

    pub(crate) fn try_send_chunk(&self, data: Arc<[u8]>, ts_ms: u64) -> bool {
        match self.tx.try_send(ShadowRecorderJob::Chunk { data, ts_ms }) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => false,
            Err(TrySendError::Disconnected(_)) => false,
        }
    }

    pub(crate) fn stop(mut self) -> std::result::Result<(), String> {
        let _ = self.tx.send(ShadowRecorderJob::Stop);
        let join = self.join.take().ok_or_else(|| "shadow recorder join missing".to_string())?;
        join.join().map_err(|_| "shadow recorder join failed".to_string())?
    }
}

pub(crate) fn run_recording_encoder(rx: Arc<LatestFrameMailbox>, config: RecordingEncoderConfig) -> std::result::Result<RecordingStats, String> {
    let requested_codec = config.codec;
    let (raw_format, probe_codec, encoder) = select_recording_encoder(requested_codec, config.encoder_hint.as_deref())?;
    let desc = encoder.descriptor();
    let impl_name = desc.impl_name;
    if impl_name.eq_ignore_ascii_case("ffmpeg") {
        tracing::warn!(encoder = %impl_name, codec = ?requested_codec, "recording encoder resolved to software implementation");
    } else {
        tracing::info!(encoder = %impl_name, codec = ?requested_codec, "recording encoder initialized");
    }
    if raw_format == RawRecordingFormat::Mjpeg {
        tracing::warn!(codec = ?requested_codec, "recording encoder output appears to be MJPEG; recording will transcode");
    } else if probe_codec != requested_codec {
        tracing::warn!(requested = ?requested_codec, actual = ?probe_codec, "recording encoder fell back to alternate codec");
    }
    if let Some(parent) = config.output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| format!("recording dir create failed: {err}"))?;
    }
    let file = std::fs::File::create(&config.output_path).map_err(|err| format!("recording file open failed: {err}"))?;
    let mut runtime = RecordingEncoderRuntime {
        writer: std::io::BufWriter::new(file),
        ts_writer: config.timestamps_path.as_deref().map(RecordingTimestampWriter::open).transpose()?,
        encoder,
        codec: probe_codec,
        raw_format,
        format_tracker: None,
        pipeline_graph: config.pipeline_graph,
        pool: BufferPool::with_capacity(1, 1),
        pool_capacity: 1,
        bitstream: RecordingBitstream::Unknown,
        convert_buf: Vec::new(),
        config_cache: RecordingConfigCache::default(),
        wrote_prefix: false,
        pending_before_config: std::collections::VecDeque::new(),
        pending_bytes: 0,
        stats: RecordingStats::default(),
        last_frame_meta: None,
    };
    runtime.set_raw_format(raw_format);

    while let Some((image, ts_ms)) = rx.next() {
        runtime.encode_image(image, ts_ms)?;
    }
    runtime.flush_delayed_packets()?;

    runtime.writer.flush().map_err(|err| format!("recording flush failed: {err}"))?;
    if let Some(writer) = runtime.ts_writer.as_mut() {
        writer.flush()?;
    }
    if runtime.stats.frames == 0 {
        return Err("no encoded frames recorded".to_string());
    }
    Ok(runtime.stats)
}

pub(crate) fn run_shadow_recorder_worker(rx: StdReceiver<ShadowRecorderJob>, config: ShadowRecorderConfig) -> std::result::Result<(), String> {
    let ShadowRecorderConfig { shadow_dir, codec, segment_ms, window_ms, format_tracker } = config;
    std::fs::create_dir_all(&shadow_dir).map_err(|err| format!("shadow dir create failed: {err}"))?;
    cleanup_shadow_dir_sync(&shadow_dir)?;

    let mut segment_start_ms = current_time_ms();
    // Keep segment files readable for capture. Tune this interval to balance write pressure
    // (lower system load) against capture freshness.
    let mut last_flush_ms = segment_start_ms;
    let flush_interval_ms = shadow_flush_interval_ms();
    let config_scan_interval_ms = shadow_config_scan_interval_ms();
    let writer_capacity = shadow_writer_buffer_bytes();
    let mut last_config_scan_ms = segment_start_ms.saturating_sub(config_scan_interval_ms);
    let raw_format = RawRecordingFormat::from_codec(codec);
    format_tracker.store(raw_format.to_u8(), Ordering::Release);
    let mut writer = open_shadow_segment_sync(&shadow_dir, segment_start_ms, codec, writer_capacity)?;
    let mut bitstream = RecordingBitstream::Unknown;
    let mut config_cache = RecordingConfigCache::default();
    let mut wrote_prefix = false;
    let mut convert_buf: Vec<u8> = Vec::new();
    let mut pending_before_config: std::collections::VecDeque<Vec<u8>> = std::collections::VecDeque::new();
    let mut pending_bytes: usize = 0;
    let pending_max_bytes: usize = 4 * 1024 * 1024; // 4MiB safety cap
    let mut stats = RecordingStats { raw_format: Some(raw_format), ..RecordingStats::default() };
    stats.raw_format = Some(raw_format);

    if let Some(prefix) = config_cache.prefix(codec) {
        writer.write_all(&prefix).map_err(|err| format!("shadow segment write failed: {err}"))?;
        wrote_prefix = true;
    }

    while let Ok(job) = rx.recv() {
        match job {
            ShadowRecorderJob::Stop => break,
            ShadowRecorderJob::Chunk { data, ts_ms } => {
                // Producer timestamps each chunk at ingress; reuse it to avoid extra syscalls.
                let now_ms = ts_ms.max(last_flush_ms);
                let clock_ms = now_ms;
                if clock_ms.saturating_sub(segment_start_ms) >= segment_ms {
                    writer.flush().map_err(|err| format!("shadow segment flush failed: {err}"))?;
                    let cutoff = now_ms.saturating_sub(window_ms);
                    let _ = cleanup_shadow_segments_sync(&shadow_dir, cutoff, codec);
                    segment_start_ms = clock_ms;
                    last_flush_ms = now_ms;
                    writer = open_shadow_segment_sync(&shadow_dir, segment_start_ms, codec, writer_capacity)?;
                    bitstream = RecordingBitstream::Unknown;
                    wrote_prefix = false;
                    if let Some(prefix) = config_cache.prefix(codec) {
                        writer.write_all(&prefix).map_err(|err| format!("shadow segment write failed: {err}"))?;
                        wrote_prefix = true;
                    }
                }

                // If the upstream encoder is misconfigured and produces MJPEG, fail fast so we
                // don't silently write garbage into a .h264/.h265 segment file.
                if is_mjpeg_payload(data.as_ref()) {
                    return Err("shadow recorder received MJPEG; configure encoder_id to h264/h265".to_string());
                }

                // Treat the stream as "live encoded" and just persist the bytestream.
                // Many sources deliver a raw AnnexB bytestream in arbitrary chunk boundaries,
                // so do not require each chunk to start with a startcode.
                //
                // If we detect length-prefixed NAL units, convert to AnnexB so segments are
                // concat-friendly. If conversion fails, fall back to writing bytes as-is rather
                // than producing empty segments.
                let mut out_payload: &[u8] = data.as_ref();
                if bitstream == RecordingBitstream::Unknown {
                    if is_annexb_prefix(out_payload) || find_annexb_start(out_payload, 0).is_some() {
                        bitstream = RecordingBitstream::AnnexB;
                    } else {
                        bitstream = RecordingBitstream::LengthPrefixed;
                    }
                }
                if bitstream == RecordingBitstream::LengthPrefixed {
                    if length_prefixed_to_annexb_best_effort(out_payload, &mut convert_buf) {
                        out_payload = convert_buf.as_slice();
                    } else {
                        // Likely AnnexB bytestream split across chunks; accept raw bytes.
                        bitstream = RecordingBitstream::AnnexB;
                    }
                }

                // Cache VPS/SPS/PPS so new segments can start decodable without requiring the
                // encoder to repeat headers each segment. Once ready, scan less frequently.
                let should_scan_config = !config_cache.ready(codec) || !wrote_prefix || now_ms.saturating_sub(last_config_scan_ms) >= config_scan_interval_ms;
                if should_scan_config {
                    config_cache.update_from_annexb(codec, out_payload);
                    last_config_scan_ms = now_ms;
                }

                // If we don't yet have VPS/SPS/PPS, buffer early payloads so the very first
                // segment becomes decodable once config arrives, rather than producing an empty
                // or parameter-less stream that ffmpeg cannot remux.
                if !config_cache.ready(codec) && !wrote_prefix {
                    let mut v = Vec::with_capacity(out_payload.len());
                    v.extend_from_slice(out_payload);
                    pending_bytes = pending_bytes.saturating_add(v.len());
                    pending_before_config.push_back(v);
                    while pending_bytes > pending_max_bytes {
                        if let Some(dropped) = pending_before_config.pop_front() {
                            pending_bytes = pending_bytes.saturating_sub(dropped.len());
                        } else {
                            pending_bytes = 0;
                            break;
                        }
                    }
                    continue;
                }

                if !wrote_prefix {
                    if let Some(prefix) = config_cache.prefix(codec) {
                        writer.write_all(&prefix).map_err(|err| format!("shadow segment write failed: {err}"))?;
                        wrote_prefix = true;
                    }
                }

                if wrote_prefix && !pending_before_config.is_empty() {
                    while let Some(buf) = pending_before_config.pop_front() {
                        pending_bytes = pending_bytes.saturating_sub(buf.len());
                        writer.write_all(&buf).map_err(|err| format!("shadow segment write failed: {err}"))?;
                        stats.bytes = stats.bytes.saturating_add(buf.len() as u64);
                    }
                }

                writer.write_all(out_payload).map_err(|err| format!("shadow segment write failed: {err}"))?;
                stats.record_ts(ts_ms);
                stats.frames = stats.frames.saturating_add(1);
                stats.bytes = stats.bytes.saturating_add(out_payload.len() as u64);

                // Periodic flush so captures see up-to-date bytes even mid-segment.
                if now_ms.saturating_sub(last_flush_ms) >= flush_interval_ms {
                    writer.flush().map_err(|err| format!("shadow segment flush failed: {err}"))?;
                    last_flush_ms = now_ms;
                }
            }
        }
    }

    writer.flush().map_err(|err| format!("shadow recorder flush failed: {err}"))?;
    Ok(())
}

impl RecordingEncoderRuntime {
    pub(super) fn set_raw_format(&mut self, raw_format: RawRecordingFormat) {
        self.raw_format = raw_format;
        self.stats.raw_format = Some(raw_format);
        if let Some(tracker) = &self.format_tracker {
            tracker.store(raw_format.to_u8(), Ordering::Release);
        }
    }

    pub(super) fn ensure_rgb24_pool(&mut self, bytes: usize) -> &BufferPool {
        let need = bytes.max(1);
        if self.pool_capacity < need {
            self.pool = BufferPool::with_capacity(1, need);
            self.pool_capacity = need;
        }
        &self.pool
    }

    pub(super) fn dynamic_image_to_rgb24_frame(&mut self, image: &image::DynamicImage, ts_ms: u64) -> Option<FrameLease> {
        let width = image.width().max(1);
        let height = image.height().max(1);
        let resolution = Resolution::new(width, height)?;
        let bytes = width as usize * height as usize * 3;
        let pool = self.ensure_rgb24_pool(bytes);
        let mut lease = pool.lease();
        lease.resize(bytes);

        let out = lease.as_mut_slice();
        if !write_rgb24(image, out) {
            return None;
        }

        let stride = (width as usize * 3).max(1);
        let meta = FrameMeta::new(MediaFormat::new(FourCc::new(*b"RG24"), resolution, ColorSpace::Unknown), ts_ms);
        Some(FrameLease::single_plane(meta, lease, bytes, stride))
    }

    pub(super) fn write_encoded_payload(&mut self, payload: &[u8], ts_ms: u64) -> std::result::Result<(), String> {
        let mut raw_format = self.raw_format;
        write_payload_to_raw(
            RawPayloadWriteState {
                writer: &mut self.writer,
                raw_format: &mut raw_format,
                format_tracker: self.format_tracker.as_ref(),
                bitstream: &mut self.bitstream,
                convert_buf: &mut self.convert_buf,
                config_cache: &mut self.config_cache,
                wrote_prefix: &mut self.wrote_prefix,
                pending_before_config: &mut self.pending_before_config,
                pending_bytes: &mut self.pending_bytes,
                stats: &mut self.stats,
                ts_writer: &mut self.ts_writer,
            },
            RawPayloadWriteInput { codec: self.codec, payload, ts_ms, pending_max_bytes: 4 * 1024 * 1024, allow_mjpeg_switch: true },
        )?;
        if raw_format != self.raw_format {
            self.set_raw_format(raw_format);
        }
        Ok(())
    }

    pub(super) fn estimated_frame_gap_ms(&self) -> u64 {
        match (self.stats.frames, self.stats.first_ts_ms, self.stats.last_ts_ms) {
            (frames, Some(first), Some(last)) if frames > 1 && last > first => (last.saturating_sub(first) / (frames - 1)).max(1),
            _ => 1,
        }
    }

    pub(super) fn flush_delayed_packets(&mut self) -> std::result::Result<(), String> {
        let Some(meta) = self.last_frame_meta.clone() else {
            return Ok(());
        };
        let any = self.encoder.as_ref() as &dyn std::any::Any;
        let delayed_packets = if let Some(encoder) = any.downcast_ref::<styx::codec::ffmpeg::FfmpegH264Encoder>() {
            encoder.0.flush_encoder(&meta).map_err(|err| format!("recording encoder flush failed: {err}"))?
        } else if let Some(encoder) = any.downcast_ref::<styx::codec::ffmpeg::FfmpegH265Encoder>() {
            encoder.0.flush_encoder(&meta).map_err(|err| format!("recording encoder flush failed: {err}"))?
        } else if let Some(encoder) = any.downcast_ref::<styx::codec::ffmpeg::FfmpegMjpegEncoder>() {
            encoder.0.flush_encoder(&meta).map_err(|err| format!("recording encoder flush failed: {err}"))?
        } else if let Some(encoder) = any.downcast_ref::<styx::codec::ffmpeg::FfmpegVideoEncoder>() {
            encoder.flush_encoder(&meta).map_err(|err| format!("recording encoder flush failed: {err}"))?
        } else {
            return Ok(());
        };

        if delayed_packets.is_empty() {
            return Ok(());
        }

        let step_ms = self.estimated_frame_gap_ms();
        let mut ts_cursor = self.stats.last_ts_ms.unwrap_or(meta.timestamp);
        let delayed_frames = delayed_packets.len();
        for frame in delayed_packets {
            let planes = frame.planes();
            let Some(plane) = planes.first() else {
                continue;
            };
            ts_cursor = ts_cursor.saturating_add(step_ms);
            self.write_encoded_payload(plane.data(), ts_cursor)?;
        }
        tracing::debug!(delayed_frames, step_ms, "flushed delayed encoder packets on recording stop");
        Ok(())
    }

    pub(super) fn encode_image(&mut self, image: Arc<image::DynamicImage>, ts_ms: u64) -> std::result::Result<(), String> {
        let image = if let Some(graph) = self.pipeline_graph.as_ref() {
            match graph.process((*image).clone()) {
                Some(img) => img,
                None => return Ok(()),
            }
        } else {
            (*image).clone()
        };
        let Some(frame) = self.dynamic_image_to_rgb24_frame(&image, ts_ms) else {
            return Err("recording rgb24 conversion failed".to_string());
        };
        self.last_frame_meta = Some(frame.meta().clone());
        let encoded_frame = match self.encoder.process(frame) {
            Ok(frame) => frame,
            Err(styx::codec::CodecError::Backpressure) => return Ok(()),
            Err(err) => return Err(format!("recording encode failed: {err}")),
        };
        let planes = encoded_frame.planes();
        let Some(plane) = planes.first() else {
            return Err("recording encode produced no planes".to_string());
        };
        self.write_encoded_payload(plane.data(), ts_ms)
    }
}
