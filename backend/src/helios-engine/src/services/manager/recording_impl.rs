use super::*;

pub(super) fn recording_state_to_result(state: RecordingState) -> Result<()> {
    match state {
        RecordingState::Completed(Ok(_)) => Ok(()),
        RecordingState::Completed(Err(reason)) => Err(Error::InvalidStateOwned(reason)),
        RecordingState::Running => Err(Error::InvalidState("recording still running")),
    }
}

pub(super) fn resolve_recording_source(manifest: &ResolvedStreamConfig, source: RecordingSource) -> ResolvedRecordingSource {
    match source {
        RecordingSource::Multiplex => ResolvedRecordingSource::Multiplex,
        RecordingSource::Raw => ResolvedRecordingSource::Raw,
        RecordingSource::Pipeline { pipeline_id, output_key } => {
            // Allow the built-in RAW pipeline id even when `pipeline_enabled=false` cleared
            // pipelines from the manifest; this enables explicit RAW outputs like "undistorted".
            let matches_manifest = pipeline_id.filter(|id| *id == RAW_STREAM_PIPELINE_UUID || manifest_contains_pipeline(manifest, *id));
            let pipeline_id = matches_manifest.or(manifest.active_pipeline_id).or_else(|| manifest.pipelines.first().map(|p| p.pipeline_id));
            match pipeline_id {
                Some(pipeline_id) => ResolvedRecordingSource::Pipeline { pipeline_id, output_key },
                None => {
                    tracing::warn!("recording pipeline missing; falling back to multiplex");
                    ResolvedRecordingSource::Multiplex
                }
            }
        }
    }
}

pub(super) fn manifest_contains_pipeline(manifest: &ResolvedStreamConfig, pipeline_id: Uuid) -> bool {
    if manifest.active_pipeline_id == Some(pipeline_id) {
        return true;
    }
    manifest.pipelines.iter().any(|binding| binding.pipeline_id == pipeline_id)
}

pub(super) fn build_recording_pipeline_graph(manifest: &ResolvedStreamConfig, pipeline_id: Uuid, output_key: Option<&str>) -> Result<crate::graph::GraphHandle> {
    crate::graph::build_graph_handle_for_pipeline_output(manifest.host_buffer(), manifest, pipeline_id, output_key)
        .map_err(|err| Error::InvalidStateOwned(format!("recording pipeline build failed: {err}")))
}

pub(super) fn recording_codec_ext(codec: RecordingCodec) -> &'static str {
    match codec {
        RecordingCodec::H264 => "h264",
        RecordingCodec::H265 => "h265",
    }
}

pub(super) fn build_raw_path(output_path: &Path, codec: RecordingCodec) -> PathBuf {
    let ext = recording_codec_ext(codec);
    let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = output_path.file_stem().and_then(|value| value.to_str()).filter(|value| !value.is_empty()).unwrap_or("recording");
    let suffix = Uuid::new_v4().simple().to_string();
    parent.join(format!("{stem}.raw-{suffix}.{ext}"))
}

pub(super) fn recording_frame_ts_path(output_path: &Path) -> PathBuf {
    let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = output_path.file_name().and_then(|value| value.to_str()).filter(|value| !value.is_empty()).unwrap_or("recording");
    parent.join(format!("{file_name}.frame_ts.txt"))
}

pub(super) fn temp_output_path(output_path: &Path) -> PathBuf {
    // Keep the real extension as the final suffix so tools like ffmpeg can infer the container.
    // E.g. `video.mp4` -> `video.part.mp4` (NOT `video.mp4.part`).
    let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
    let ext = output_path.extension().and_then(|ext| ext.to_str()).filter(|ext| !ext.is_empty());
    let stem = output_path.file_stem().and_then(|value| value.to_str()).filter(|value| !value.is_empty()).unwrap_or("output");
    match ext {
        Some(ext) => parent.join(format!("{stem}.part.{ext}")),
        None => parent.join(format!("{stem}.part")),
    }
}

pub(super) fn pretrim_output_path(output_path: &Path) -> PathBuf {
    // Unique temp file next to the final output (so rename is atomic within filesystem).
    let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
    let ext = output_path.extension().and_then(|ext| ext.to_str()).filter(|ext| !ext.is_empty());
    let stem = output_path.file_stem().and_then(|value| value.to_str()).filter(|value| !value.is_empty()).unwrap_or("output");
    let suffix = Uuid::new_v4().simple().to_string();
    match ext {
        Some(ext) => parent.join(format!("{stem}.pretrim-{suffix}.{ext}")),
        None => parent.join(format!("{stem}.pretrim-{suffix}")),
    }
}

pub(super) fn current_time_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|value| value.as_millis() as u64).unwrap_or(0)
}

pub(super) fn shadow_window_ms() -> u64 {
    policy::shadow_window_ms()
}

pub(super) fn shadow_segment_ms() -> u64 {
    policy::shadow_segment_ms()
}

pub(super) fn shadow_flush_interval_ms() -> u64 {
    policy::shadow_flush_interval_ms()
}

pub(super) fn shadow_writer_buffer_bytes() -> usize {
    policy::shadow_writer_buffer_bytes()
}

pub(super) fn shadow_config_scan_interval_ms() -> u64 {
    policy::shadow_config_scan_interval_ms()
}

pub(super) fn recording_stop_grace_ms() -> u64 {
    policy::recording_stop_grace_ms()
}

pub(super) fn recording_frame_queue_size() -> usize {
    policy::recording_frame_queue_size()
}

pub(super) fn keep_raw_on_record_fail() -> bool {
    policy::keep_raw_on_record_fail()
}

pub(super) fn parse_shadow_segment_timestamp(name: &str, ext: &str) -> Option<u64> {
    let suffix = format!(".{ext}");
    let trimmed = name.strip_suffix(&suffix)?;
    let ts = trimmed.strip_prefix("segment_")?;
    ts.parse::<u64>().ok()
}

pub(super) async fn list_shadow_segments(dir: &Path, codec: RecordingCodec) -> std::result::Result<Vec<(u64, PathBuf)>, String> {
    let ext = recording_codec_ext(codec);
    let mut entries = fs::read_dir(dir).await.map_err(|err| format!("shadow dir read failed: {err}"))?;
    let mut segments = Vec::new();
    loop {
        let entry = entries.next_entry().await.map_err(|err| format!("shadow dir entry failed: {err}"))?;
        let Some(entry) = entry else { break };
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        let Some(ts) = parse_shadow_segment_timestamp(&name, ext) else { continue };
        let meta = entry.metadata().await.map_err(|err| format!("shadow segment stat failed: {err}"))?;
        if meta.is_file() && meta.len() > 0 {
            segments.push((ts, entry.path()));
        }
    }
    segments.sort_by_key(|(ts, _)| *ts);
    Ok(segments)
}

pub(super) fn cleanup_shadow_segments_sync(dir: &Path, cutoff_ms: u64, codec: RecordingCodec) -> std::result::Result<(), String> {
    let ext = recording_codec_ext(codec);
    let entries = std::fs::read_dir(dir).map_err(|err| format!("shadow dir read failed: {err}"))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("shadow dir entry failed: {err}"))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(ts) = parse_shadow_segment_timestamp(&name, ext) else { continue };
        if ts < cutoff_ms {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    Ok(())
}

pub(super) fn cleanup_shadow_dir_sync(dir: &Path) -> std::result::Result<(), String> {
    if std::fs::metadata(dir).is_err() {
        return Ok(());
    }
    let entries = std::fs::read_dir(dir).map_err(|err| format!("shadow dir read failed: {err}"))?;
    for entry in entries {
        let entry = entry.map_err(|err| format!("shadow dir entry failed: {err}"))?;
        let meta = entry.metadata().map_err(|err| format!("shadow segment stat failed: {err}"))?;
        if meta.is_file() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    Ok(())
}

pub(super) fn open_shadow_segment_sync(dir: &Path, start_ms: u64, codec: RecordingCodec, writer_capacity: usize) -> std::result::Result<std::io::BufWriter<std::fs::File>, String> {
    let ext = recording_codec_ext(codec);
    let path = dir.join(format!("segment_{start_ms}.{ext}"));
    let file = std::fs::File::create(&path).map_err(|err| format!("shadow segment open failed: {err}"))?;
    Ok(std::io::BufWriter::with_capacity(writer_capacity, file))
}

pub(super) fn is_annexb_prefix(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0, 0, 0, 1]) || bytes.starts_with(&[0, 0, 1])
}

pub(super) fn is_mjpeg_payload(bytes: &[u8]) -> bool {
    bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF
}

pub(super) fn read_be_len(bytes: &[u8], len_size: usize) -> Option<usize> {
    match len_size {
        1 => Some(*bytes.first()? as usize),
        2 => Some(u16::from_be_bytes([*bytes.first()?, *bytes.get(1)?]) as usize),
        3 => Some(((*bytes.first()? as usize) << 16) | ((*bytes.get(1)? as usize) << 8) | (*bytes.get(2)? as usize)),
        4 => Some(u32::from_be_bytes([*bytes.first()?, *bytes.get(1)?, *bytes.get(2)?, *bytes.get(3)?]) as usize),
        _ => None,
    }
}

pub(super) fn length_prefixed_to_annexb_with_len(bytes: &[u8], len_size: usize, out: &mut Vec<u8>) -> bool {
    if len_size == 0 || bytes.len() < len_size {
        return false;
    }
    out.clear();
    // Approx reserve: add 4 bytes start-code per NAL; worst-case NAL count is bytes/len_size.
    out.reserve(bytes.len().saturating_add((bytes.len() / len_size).saturating_mul(4)));

    let mut cursor = bytes;
    while cursor.len() >= len_size {
        let Some(len) = read_be_len(cursor, len_size) else {
            out.clear();
            return false;
        };
        cursor = &cursor[len_size..];
        if len == 0 || len > cursor.len() {
            out.clear();
            return false;
        }
        out.extend_from_slice(&[0, 0, 0, 1]);
        out.extend_from_slice(&cursor[..len]);
        cursor = &cursor[len..];
    }

    if !cursor.is_empty() || out.is_empty() {
        out.clear();
        return false;
    }
    true
}

pub(super) fn length_prefixed_to_annexb_best_effort(bytes: &[u8], out: &mut Vec<u8>) -> bool {
    // Most common is 4. Some streams (notably some hardware paths) use 2.
    for len_size in [4usize, 3, 2, 1] {
        if length_prefixed_to_annexb_with_len(bytes, len_size, out) {
            return true;
        }
    }
    false
}

pub(super) fn find_annexb_start(bytes: &[u8], from: usize) -> Option<(usize, usize)> {
    if bytes.len() < 3 {
        return None;
    }
    let mut i = from;
    while i + 3 <= bytes.len() {
        if bytes[i] == 0 && bytes[i + 1] == 0 {
            if bytes[i + 2] == 1 {
                return Some((i, i + 3));
            }
            if i + 3 < bytes.len() && bytes[i + 2] == 0 && bytes[i + 3] == 1 {
                return Some((i, i + 4));
            }
        }
        i += 1;
    }
    None
}

pub(super) struct RawPayloadWriteState<'a> {
    writer: &'a mut dyn Write,
    raw_format: &'a mut RawRecordingFormat,
    format_tracker: Option<&'a Arc<AtomicU8>>,
    bitstream: &'a mut RecordingBitstream,
    convert_buf: &'a mut Vec<u8>,
    config_cache: &'a mut RecordingConfigCache,
    wrote_prefix: &'a mut bool,
    pending_before_config: &'a mut std::collections::VecDeque<Vec<u8>>,
    pending_bytes: &'a mut usize,
    stats: &'a mut RecordingStats,
    ts_writer: &'a mut Option<RecordingTimestampWriter>,
}

pub(super) struct RawPayloadWriteInput<'a> {
    codec: RecordingCodec,
    payload: &'a [u8],
    ts_ms: u64,
    pending_max_bytes: usize,
    allow_mjpeg_switch: bool,
}

pub(super) fn write_payload_to_raw(state: RawPayloadWriteState<'_>, input: RawPayloadWriteInput<'_>) -> std::result::Result<(), String> {
    let RawPayloadWriteInput { codec, payload, ts_ms, pending_max_bytes, allow_mjpeg_switch } = input;
    if *state.raw_format != RawRecordingFormat::Mjpeg && is_mjpeg_payload(payload) {
        if !allow_mjpeg_switch {
            return Err("encoded payload was MJPEG but H264/H265 was expected".to_string());
        }
        tracing::warn!(codec = ?codec, "recording encoder output appears to be MJPEG; treating as MJPEG");
        *state.raw_format = RawRecordingFormat::Mjpeg;
        state.stats.raw_format = Some(RawRecordingFormat::Mjpeg);
        if let Some(tracker) = state.format_tracker {
            tracker.store(RawRecordingFormat::Mjpeg.to_u8(), Ordering::Release);
        }
    }

    if matches!(*state.raw_format, RawRecordingFormat::Mjpeg) {
        state.writer.write_all(payload).map_err(|err| format!("recording write failed: {err}"))?;
        state.stats.record_ts(ts_ms);
        state.stats.frames = state.stats.frames.saturating_add(1);
        state.stats.bytes = state.stats.bytes.saturating_add(payload.len() as u64);
        if let Some(writer) = state.ts_writer.as_mut() {
            writer.record(ts_ms)?;
        }
        return Ok(());
    }

    let mut out_payload = payload;
    if *state.bitstream == RecordingBitstream::Unknown {
        *state.bitstream = if is_annexb_prefix(out_payload) { RecordingBitstream::AnnexB } else { RecordingBitstream::LengthPrefixed };
    }
    if *state.bitstream == RecordingBitstream::LengthPrefixed {
        if length_prefixed_to_annexb_best_effort(out_payload, state.convert_buf) {
            out_payload = state.convert_buf.as_slice();
        } else if is_annexb_prefix(out_payload) {
            *state.bitstream = RecordingBitstream::AnnexB;
        } else if let Some((start, _)) = find_annexb_start(out_payload, 0) {
            // Some sources prepend non-startcode bytes; best-effort salvage if AnnexB is present.
            *state.bitstream = RecordingBitstream::AnnexB;
            out_payload = &out_payload[start..];
        } else {
            // Drop malformed payload.
            return Ok(());
        }
    }

    state.config_cache.update_from_annexb(codec, out_payload);
    if !*state.wrote_prefix {
        if let Some(prefix) = state.config_cache.prefix(codec) {
            state.writer.write_all(&prefix).map_err(|err| format!("recording write failed: {err}"))?;
            *state.wrote_prefix = true;
            while let Some(buf) = state.pending_before_config.pop_front() {
                *state.pending_bytes = state.pending_bytes.saturating_sub(buf.len());
                state.writer.write_all(&buf).map_err(|err| format!("recording write failed: {err}"))?;
                state.stats.bytes = state.stats.bytes.saturating_add(buf.len() as u64);
            }
        } else {
            let mut v = Vec::with_capacity(out_payload.len());
            v.extend_from_slice(out_payload);
            *state.pending_bytes = state.pending_bytes.saturating_add(v.len());
            state.pending_before_config.push_back(v);
            while *state.pending_bytes > pending_max_bytes {
                if let Some(dropped) = state.pending_before_config.pop_front() {
                    *state.pending_bytes = state.pending_bytes.saturating_sub(dropped.len());
                } else {
                    *state.pending_bytes = 0;
                    break;
                }
            }
            return Ok(());
        }
    }

    state.writer.write_all(out_payload).map_err(|err| format!("recording write failed: {err}"))?;
    state.stats.record_ts(ts_ms);
    state.stats.frames = state.stats.frames.saturating_add(1);
    state.stats.bytes = state.stats.bytes.saturating_add(out_payload.len() as u64);
    if let Some(writer) = state.ts_writer.as_mut() {
        writer.record(ts_ms)?;
    }
    Ok(())
}

pub(super) async fn capture_shadow_segments(shadow_dir: &Path, output_path: &Path, codec: RecordingCodec, window_ms: u64) -> std::result::Result<u64, String> {
    let segments = list_shadow_segments(shadow_dir, codec).await?;
    if segments.is_empty() {
        return Err("shadow recorder has no segments".to_string());
    }
    let now_ms = current_time_ms();
    let cutoff = now_ms.saturating_sub(window_ms);
    // Select segments that plausibly cover the requested window ending "now".
    //
    // Important: segments are *usually* rotated at fixed `segment_ms`, but rotation only occurs
    // when new encoded chunks arrive. Under encoder backpressure / jitter, a segment can be longer
    // than `segment_ms`, so "cutoff - segment_ms" is not a reliable way to pick the overlapping
    // segment. Instead, include the most recent segment at/before the cutoff plus everything after.
    let idx = segments.iter().position(|(ts, _)| *ts >= cutoff).unwrap_or(segments.len());
    let start_index = idx.saturating_sub(1);
    let selection = &segments[start_index..];
    if selection.is_empty() {
        return Err("shadow recorder has no segments in window".to_string());
    }

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent).await.map_err(|err| format!("shadow output dir create failed: {err}"))?;
    }
    let file = File::create(output_path).await.map_err(|err| format!("shadow output open failed: {err}"))?;
    let mut writer = BufWriter::new(file);
    let mut total_bytes = 0u64;

    // Ensure the concatenated bitstream begins with codec config so ffmpeg can parse it even if
    // the first selected segment starts mid-stream.
    if let Some(prefix) = probe_prefix_from_segments(selection, codec).await {
        writer.write_all(&prefix).await.map_err(|err| format!("shadow output write failed: {err}"))?;
        total_bytes = total_bytes.saturating_add(prefix.len() as u64);
    }
    for (_, path) in selection {
        let mut reader = File::open(path).await.map_err(|err| format!("shadow segment open failed: {err}"))?;
        let written = io::copy(&mut reader, &mut writer).await.map_err(|err| format!("shadow segment copy failed: {err}"))?;
        total_bytes = total_bytes.saturating_add(written);
    }
    writer.flush().await.map_err(|err| format!("shadow output flush failed: {err}"))?;
    if total_bytes == 0 {
        return Err("shadow output empty".to_string());
    }
    Ok(total_bytes)
}

pub(super) async fn run_shadow_recorder_stream(
    rx: &mut Receiver<EncodedFrame>,
    worker: ShadowRecorderWorker,
    mut stop_rx: oneshot::Receiver<()>,
    consumer_touch: Option<Arc<AtomicU64>>,
) -> std::result::Result<(), String> {
    loop {
        tokio::select! {
            _ = &mut stop_rx => break,
            recv = rx.recv() => match recv {
                Ok(chunk) => {
                    if let Some(touch) = consumer_touch.as_ref() {
                        touch.store(current_time_ms(), Ordering::Relaxed);
                    }
                    let _ = worker.try_send_chunk(chunk.data, chunk.ts_ms);
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    }
    worker.stop()
}

impl RecordingEncoderWorker {
    pub(super) fn start(config: RecordingEncoderConfig) -> std::result::Result<Self, String> {
        let mailbox = Arc::new(LatestFrameMailbox::new(recording_frame_queue_size()));
        let rx = Arc::clone(&mailbox);
        let join = std::thread::Builder::new()
            .name("helios-recording".into())
            .stack_size(policy::recording_worker_stack_size_bytes())
            .spawn(move || run_recording_encoder(rx, config))
            .map_err(|err| format!("recording worker spawn failed: {err}"))?;
        Ok(Self { mailbox, join: Some(join) })
    }

    pub(super) fn send_image(&self, image: Arc<image::DynamicImage>, ts_ms: u64) {
        self.mailbox.push_frame(image, ts_ms);
    }

    pub(super) fn stop(mut self) -> std::result::Result<RecordingStats, String> {
        self.mailbox.stop();
        let join = self.join.take().ok_or_else(|| "recording worker join missing".to_string())?;
        join.join().map_err(|_| "recording worker join failed".to_string())?
    }
}

impl ShadowRecorderWorker {
    pub(super) fn start(config: ShadowRecorderConfig) -> std::result::Result<Self, String> {
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

    pub(super) fn try_send_chunk(&self, data: Arc<[u8]>, ts_ms: u64) -> bool {
        match self.tx.try_send(ShadowRecorderJob::Chunk { data, ts_ms }) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => false,
            Err(TrySendError::Disconnected(_)) => false,
        }
    }

    pub(super) fn stop(mut self) -> std::result::Result<(), String> {
        let _ = self.tx.send(ShadowRecorderJob::Stop);
        let join = self.join.take().ok_or_else(|| "shadow recorder join missing".to_string())?;
        join.join().map_err(|_| "shadow recorder join failed".to_string())?
    }
}

pub(super) fn run_recording_encoder(rx: Arc<LatestFrameMailbox>, config: RecordingEncoderConfig) -> std::result::Result<RecordingStats, String> {
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

pub(super) fn run_shadow_recorder_worker(rx: StdReceiver<ShadowRecorderJob>, config: ShadowRecorderConfig) -> std::result::Result<(), String> {
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

pub(super) fn encoder_output_matches(codec: RecordingCodec, desc: &styx::codec::CodecDescriptor) -> bool {
    let name = desc.name.to_ascii_lowercase();
    let impl_name = desc.impl_name.to_ascii_lowercase();
    let output = String::from_utf8_lossy(&desc.output.to_u32().to_le_bytes()).to_ascii_lowercase();
    match codec {
        RecordingCodec::H264 => ["h264", "avc"].iter().any(|token| name.contains(token) || impl_name.contains(token) || output.contains(token)),
        RecordingCodec::H265 => ["h265", "hevc"].iter().any(|token| name.contains(token) || impl_name.contains(token) || output.contains(token)),
    }
}

pub(super) fn raw_format_from_desc(desc: &styx::codec::CodecDescriptor) -> Option<RawRecordingFormat> {
    let name = desc.name.to_ascii_lowercase();
    let impl_name = desc.impl_name.to_ascii_lowercase();
    let output = String::from_utf8_lossy(&desc.output.to_u32().to_le_bytes()).to_ascii_lowercase();
    if ["mjpeg", "jpeg", "mjpg"].iter().any(|token| name.contains(token) || impl_name.contains(token) || output.contains(token)) {
        return Some(RawRecordingFormat::Mjpeg);
    }
    if ["h264", "avc"].iter().any(|token| name.contains(token) || impl_name.contains(token) || output.contains(token)) {
        return Some(RawRecordingFormat::H264);
    }
    if ["h265", "hevc"].iter().any(|token| name.contains(token) || impl_name.contains(token) || output.contains(token)) {
        return Some(RawRecordingFormat::H265);
    }
    None
}

pub(super) fn select_encoder_for_codec(codec: RecordingCodec, encoder_hint: Option<&str>, allow_mjpeg: bool) -> std::result::Result<(RawRecordingFormat, Arc<dyn Codec>), String> {
    let registry = CodecRegistry::with_enabled_codecs().map_err(|err| format!("codec registry init failed: {err}"))?;
    let handle = registry.handle();
    let input = FourCc::new(*b"RG24");
    let targets: &[&str] = match codec {
        RecordingCodec::H264 => &["h264", "avc"],
        RecordingCodec::H265 => &["h265", "hevc"],
    };
    handle.set_policy(CodecPolicy::builder(input).prefer_hardware(true).build());

    let mut last_mismatch: Option<String> = None;
    let check_encoder = |encoder: Arc<dyn Codec>, label: &str| -> std::result::Result<(RawRecordingFormat, Arc<dyn Codec>), String> {
        let desc = encoder.descriptor();
        if let Some(raw_format) = raw_format_from_desc(desc) {
            if raw_format.as_codec() == Some(codec) {
                return Ok((raw_format, encoder));
            }
            if raw_format == RawRecordingFormat::Mjpeg && allow_mjpeg {
                return Ok((raw_format, encoder));
            }
        }
        if encoder_output_matches(codec, desc) {
            return Ok((RawRecordingFormat::from_codec(codec), encoder));
        }
        Err(format!("encoder '{label}' did not produce {:?}", codec))
    };

    if let Some(hint) = encoder_hint.map(str::trim).filter(|value| !value.is_empty()) {
        if let Ok(encoder) = handle.lookup_named_kind(input, CodecKind::Encoder, hint).or_else(|_| handle.lookup_auto_kind_by_name(input, CodecKind::Encoder, hint)) {
            match check_encoder(encoder, hint) {
                Ok(found) => return Ok(found),
                Err(err) => {
                    tracing::warn!(requested = ?codec, hint = %hint, error = %err, "recording encoder hint did not match requested codec; trying codec family fallbacks");
                    last_mismatch = Some(err);
                }
            }
        }
    }

    for target in targets {
        if let Ok(encoder) = handle.lookup_auto_kind_by_name(input, CodecKind::Encoder, target) {
            match check_encoder(encoder, target) {
                Ok(found) => return Ok(found),
                Err(err) => {
                    last_mismatch = Some(err);
                    continue;
                }
            }
        }
    }

    if allow_mjpeg {
        for target in ["mjpeg", "jpeg", "mjpg"] {
            if let Ok(encoder) = handle.lookup_auto_kind_by_name(input, CodecKind::Encoder, target) {
                return Ok((RawRecordingFormat::Mjpeg, encoder));
            }
        }
    }

    if let Some(err) = last_mismatch {
        Err(err)
    } else {
        Err(format!("encoder not found for {:?}", codec))
    }
}

pub(super) fn select_recording_encoder(preferred: RecordingCodec, encoder_hint: Option<&str>) -> std::result::Result<(RawRecordingFormat, RecordingCodec, Arc<dyn Codec>), String> {
    if let Ok((raw_format, encoder)) = select_encoder_for_codec(preferred, encoder_hint, false) {
        let probe_codec = raw_format.as_codec().unwrap_or(preferred);
        return Ok((raw_format, probe_codec, encoder));
    }
    let fallback = match preferred {
        RecordingCodec::H264 => RecordingCodec::H265,
        RecordingCodec::H265 => RecordingCodec::H264,
    };
    if let Ok((raw_format, encoder)) = select_encoder_for_codec(fallback, encoder_hint, false) {
        let probe_codec = raw_format.as_codec().unwrap_or(fallback);
        return Ok((raw_format, probe_codec, encoder));
    }
    let (raw_format, encoder) = select_encoder_for_codec(preferred, encoder_hint, true)?;
    let probe_codec = raw_format.as_codec().unwrap_or(preferred);
    Ok((raw_format, probe_codec, encoder))
}

pub(super) struct RecordingFrameParams<'a> {
    pub(super) output_path: &'a Path,
    pub(super) raw_path: &'a Path,
    pub(super) container: RecordingContainer,
    pub(super) codec: RecordingCodec,
    pub(super) duration_ms: Option<u64>,
    pub(super) fps: Option<f32>,
    pub(super) settings: Option<crate::ipc::RecordingSettings>,
    pub(super) encoder_hint: Option<String>,
    pub(super) frame_ts_path: Option<PathBuf>,
}

pub(super) async fn record_frame_stream(mut source: RecordingFrameSource, mut stop_rx: oneshot::Receiver<()>, params: RecordingFrameParams<'_>) -> std::result::Result<RecordingStats, String> {
    let RecordingFrameParams { output_path, raw_path, container, codec, duration_ms, fps, settings, encoder_hint, frame_ts_path } = params;
    let record_path = if matches!(container, RecordingContainer::Mp4) { raw_path } else { output_path };
    if let Some(parent) = record_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|err| format!("recording dir create failed: {err}"))?;
    }

    let rewrite_timestamps_path = frame_ts_path.clone();
    let worker = RecordingEncoderWorker::start(RecordingEncoderConfig {
        output_path: record_path.to_path_buf(),
        codec,
        pipeline_graph: match &source {
            RecordingFrameSource::Pipeline { graph, .. } => Some(graph.clone()),
            _ => None,
        },
        encoder_hint: encoder_hint.clone(),
        timestamps_path: frame_ts_path,
    })?;

    // Capture wall span is kept for diagnostics/fallback, but MP4 remux timing should prefer
    // observed frame cadence so playback speed matches what was actually encoded.
    let wall_start_ms = current_time_ms();
    let fps_drop = settings.as_ref().and_then(|s| s.fps).filter(|v| *v > 0.0);
    let min_gap = fps_drop.map(|value| Duration::from_secs_f64(1.0 / f64::from(value.max(f32::EPSILON))));
    let mut last_write: Option<Instant> = None;
    let deadline = duration_ms.filter(|ms| *ms > 0).map(|ms| Instant::now() + Duration::from_millis(ms));
    let stop_grace = Duration::from_millis(recording_stop_grace_ms());
    let mut stop_deadline: Option<Instant> = None;

    loop {
        let recv_fut = match &mut source {
            RecordingFrameSource::Multiplex { rx } => rx.recv(),
            RecordingFrameSource::Raw { rx } => rx.recv(),
            RecordingFrameSource::Pipeline { rx, .. } => rx.recv(),
        };

        if let Some(stop_until) = stop_deadline {
            if let Some(duration_until) = deadline {
                tokio::select! {
                    _ = sleep_until(stop_until) => break,
                    _ = sleep_until(duration_until) => break,
                    recv = recv_fut => match recv {
                        Ok(image) => {
                            if let Some(min_gap) = min_gap {
                                if let Some(last) = last_write {
                                    if last.elapsed() < min_gap {
                                        continue;
                                    }
                                }
                                last_write = Some(Instant::now());
                            }
                            let ts_ms = current_time_ms();
                            worker.send_image(image, ts_ms);
                        }
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    }
                }
            } else {
                tokio::select! {
                    _ = sleep_until(stop_until) => break,
                    recv = recv_fut => match recv {
                        Ok(image) => {
                            if let Some(min_gap) = min_gap {
                                if let Some(last) = last_write {
                                    if last.elapsed() < min_gap {
                                        continue;
                                    }
                                }
                                last_write = Some(Instant::now());
                            }
                            let ts_ms = current_time_ms();
                            worker.send_image(image, ts_ms);
                        }
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    }
                }
            }
        } else if let Some(duration_until) = deadline {
            tokio::select! {
                _ = &mut stop_rx => {
                    if stop_grace.is_zero() {
                        break;
                    }
                    stop_deadline = Some(Instant::now() + stop_grace);
                    continue;
                }
                _ = sleep_until(duration_until) => break,
                recv = recv_fut => match recv {
                    Ok(image) => {
                        if let Some(min_gap) = min_gap {
                            if let Some(last) = last_write {
                                if last.elapsed() < min_gap {
                                    continue;
                                }
                            }
                            last_write = Some(Instant::now());
                        }
                        let ts_ms = current_time_ms();
                        worker.send_image(image, ts_ms);
                    }
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => break,
                }
            }
        } else {
            tokio::select! {
                _ = &mut stop_rx => {
                    if stop_grace.is_zero() {
                        break;
                    }
                    stop_deadline = Some(Instant::now() + stop_grace);
                    continue;
                }
                recv = recv_fut => match recv {
                    Ok(image) => {
                        if let Some(min_gap) = min_gap {
                            if let Some(last) = last_write {
                                if last.elapsed() < min_gap {
                                    continue;
                                }
                            }
                            last_write = Some(Instant::now());
                        }
                        let ts_ms = current_time_ms();
                        worker.send_image(image, ts_ms);
                    }
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => break,
                }
            }
        }
    }

    let wall_end_ms = current_time_ms();
    let stats = tokio::task::spawn_blocking(move || worker.stop()).await.map_err(|_| "recording worker join failed".to_string())??;
    if let Some(path) = rewrite_timestamps_path.as_deref() {
        maybe_rewrite_encoded_frame_timestamps(path, &stats, wall_start_ms, wall_end_ms).await?;
    }

    if matches!(container, RecordingContainer::Mp4) {
        let wall_span_ms = wall_end_ms.saturating_sub(wall_start_ms);
        // Fall back to wall span only when we don't have a reliable per-frame timestamp span.
        let wall_derived_fps = if stats.frames > 0 && wall_span_ms > 0 { Some((stats.frames as f32 * 1000.0) / (wall_span_ms as f32)) } else { None };
        let derived_fps = stats.derived_fps().or(wall_derived_fps);
        let remux_fps = derived_fps.or(fps);
        let transcode = settings.as_ref().is_some_and(|s| s.bitrate_bps.is_some() || s.gop.is_some() || s.quality.is_some() || s.max_width.is_some() || s.max_height.is_some());
        let raw_format = stats.raw_format.unwrap_or_else(|| RawRecordingFormat::from_codec(codec));
        let needs_transcode = transcode || raw_format.as_codec() != Some(codec);
        let raw_path = raw_path.to_path_buf();
        let output_path = output_path.to_path_buf();
        let settings = if needs_transcode { settings } else { None };
        if needs_transcode {
            finalize_recording_mp4(raw_path, output_path, raw_format, codec, remux_fps, settings).await?;
        } else {
            finalize_recording_mp4(raw_path, output_path, raw_format, codec, remux_fps, None).await?;
        }
    }

    Ok(stats)
}

pub(super) fn write_encoded_chunks_to_raw(
    path: PathBuf,
    codec: RecordingCodec,
    timestamps_path: Option<PathBuf>,
    rx: std::sync::mpsc::Receiver<(Arc<[u8]>, u64)>,
) -> std::result::Result<RecordingStats, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| format!("recording dir create failed: {err}"))?;
    }
    let file = std::fs::File::create(&path).map_err(|err| format!("recording file open failed: {err}"))?;
    let mut writer = std::io::BufWriter::new(file);

    let mut raw_format = RawRecordingFormat::from_codec(codec);
    let mut bitstream = RecordingBitstream::Unknown;
    let mut convert_buf = Vec::new();
    let mut config_cache = RecordingConfigCache::default();
    let mut wrote_prefix = false;
    let mut pending_before_config: std::collections::VecDeque<Vec<u8>> = std::collections::VecDeque::new();
    let mut pending_bytes: usize = 0;
    let pending_max_bytes: usize = 4 * 1024 * 1024;
    let mut ts_writer = timestamps_path.as_deref().map(RecordingTimestampWriter::open).transpose()?;
    let mut stats = RecordingStats { raw_format: Some(raw_format), ..RecordingStats::default() };

    while let Ok((payload, ts_ms)) = rx.recv() {
        write_payload_to_raw(
            RawPayloadWriteState {
                writer: &mut writer,
                raw_format: &mut raw_format,
                format_tracker: None,
                bitstream: &mut bitstream,
                convert_buf: &mut convert_buf,
                config_cache: &mut config_cache,
                wrote_prefix: &mut wrote_prefix,
                pending_before_config: &mut pending_before_config,
                pending_bytes: &mut pending_bytes,
                stats: &mut stats,
                ts_writer: &mut ts_writer,
            },
            RawPayloadWriteInput { codec, payload: payload.as_ref(), ts_ms, pending_max_bytes, allow_mjpeg_switch: true },
        )?;
    }

    writer.flush().map_err(|err| format!("recording flush failed: {err}"))?;
    if let Some(writer) = ts_writer.as_mut() {
        writer.flush()?;
    }
    if stats.frames == 0 {
        return Err("no encoded frames recorded".to_string());
    }
    Ok(stats)
}

pub(super) async fn record_encoded_stream(
    mut rx: Receiver<EncodedFrame>,
    mut stop_rx: oneshot::Receiver<()>,
    output_path: &Path,
    codec: RecordingCodec,
    duration_ms: Option<u64>,
    timestamps_path: Option<PathBuf>,
    consumer_touch: Option<Arc<AtomicU64>>,
) -> std::result::Result<RecordingStats, String> {
    let (tx, write_rx) = std::sync::mpsc::channel::<(Arc<[u8]>, u64)>();
    let output_path = output_path.to_path_buf();
    let write_task = tokio::task::spawn_blocking(move || write_encoded_chunks_to_raw(output_path, codec, timestamps_path, write_rx));
    // Encoded stream chunk timestamps are source-dependent and may not be milliseconds.
    // Stamp chunks with local wall clock so recording sidecars are in real-time ms.
    let mut last_wall_ts_ms = current_time_ms();

    let stop_grace = Duration::from_millis(recording_stop_grace_ms());
    let deadline = duration_ms.filter(|ms| *ms > 0).map(|ms| Instant::now() + Duration::from_millis(ms));
    let mut stop_deadline: Option<Instant> = None;

    loop {
        if let Some(stop_until) = stop_deadline {
            if let Some(duration_until) = deadline {
                tokio::select! {
                    _ = sleep_until(stop_until) => break,
                    _ = sleep_until(duration_until) => break,
                    recv = rx.recv() => match recv {
                        Ok(chunk) => {
                            if let Some(touch) = consumer_touch.as_ref() {
                                touch.store(current_time_ms(), Ordering::Relaxed);
                            }
                            let wall_ts_ms = current_time_ms().max(last_wall_ts_ms);
                            last_wall_ts_ms = wall_ts_ms;
                            if tx.send((chunk.data, wall_ts_ms)).is_err() {
                                break;
                            }
                        }
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    }
                }
            } else {
                tokio::select! {
                    _ = sleep_until(stop_until) => break,
                    recv = rx.recv() => match recv {
                        Ok(chunk) => {
                            if let Some(touch) = consumer_touch.as_ref() {
                                touch.store(current_time_ms(), Ordering::Relaxed);
                            }
                            let wall_ts_ms = current_time_ms().max(last_wall_ts_ms);
                            last_wall_ts_ms = wall_ts_ms;
                            if tx.send((chunk.data, wall_ts_ms)).is_err() {
                                break;
                            }
                        }
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    }
                }
            }
        } else if let Some(duration_until) = deadline {
            tokio::select! {
                _ = &mut stop_rx => {
                    if stop_grace.is_zero() {
                        break;
                    }
                    stop_deadline = Some(Instant::now() + stop_grace);
                    continue;
                }
                _ = sleep_until(duration_until) => break,
                recv = rx.recv() => match recv {
                    Ok(chunk) => {
                        if let Some(touch) = consumer_touch.as_ref() {
                            touch.store(current_time_ms(), Ordering::Relaxed);
                        }
                        let wall_ts_ms = current_time_ms().max(last_wall_ts_ms);
                        last_wall_ts_ms = wall_ts_ms;
                        if tx.send((chunk.data, wall_ts_ms)).is_err() {
                            break;
                        }
                    }
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => break,
                }
            }
        } else {
            tokio::select! {
                _ = &mut stop_rx => {
                    if stop_grace.is_zero() {
                        break;
                    }
                    stop_deadline = Some(Instant::now() + stop_grace);
                    continue;
                }
                recv = rx.recv() => match recv {
                    Ok(chunk) => {
                        let wall_ts_ms = current_time_ms().max(last_wall_ts_ms);
                        last_wall_ts_ms = wall_ts_ms;
                        if tx.send((chunk.data, wall_ts_ms)).is_err() {
                            break;
                        }
                    }
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => break,
                }
            }
        }
    }

    drop(tx);
    write_task.await.map_err(|_| "recording writer join failed".to_string())?
}

pub(super) struct EncodedRecordingSessionRequest {
    pub(super) rx: Receiver<EncodedFrame>,
    pub(super) stop_rx: oneshot::Receiver<()>,
    pub(super) output_path: PathBuf,
    pub(super) raw_path: PathBuf,
    pub(super) container: RecordingContainer,
    pub(super) source_codec: RecordingCodec,
    pub(super) target_codec: RecordingCodec,
    pub(super) duration_ms: Option<u64>,
    pub(super) fps: Option<f32>,
    pub(super) settings: Option<crate::ipc::RecordingSettings>,
    pub(super) timestamps_path: Option<PathBuf>,
    pub(super) consumer_touch: Option<Arc<AtomicU64>>,
}

pub(super) async fn record_encoded_session(request: EncodedRecordingSessionRequest) -> std::result::Result<RecordingStats, String> {
    let EncodedRecordingSessionRequest { rx, stop_rx, output_path, raw_path, container, source_codec, target_codec, duration_ms, fps, settings, timestamps_path, consumer_touch } = request;
    let record_path = if matches!(container, RecordingContainer::Mp4) { raw_path.as_path() } else { output_path.as_path() };
    let wall_start_ms = current_time_ms();
    let rewrite_timestamps_path = timestamps_path.clone();
    let stats = record_encoded_stream(rx, stop_rx, record_path, source_codec, duration_ms, timestamps_path, consumer_touch).await?;
    let wall_end_ms = current_time_ms();
    if let Some(path) = rewrite_timestamps_path.as_deref() {
        maybe_rewrite_encoded_frame_timestamps(path, &stats, wall_start_ms, wall_end_ms).await?;
    }

    if matches!(container, RecordingContainer::Mp4) {
        let wall_span_ms = wall_end_ms.saturating_sub(wall_start_ms);
        let wall_derived_fps = if stats.frames > 0 && wall_span_ms > 0 { Some((stats.frames as f32 * 1000.0) / (wall_span_ms as f32)) } else { None };
        let derived_fps = stats.derived_fps().or(wall_derived_fps);
        let remux_fps = derived_fps.or(fps);
        let transcode = settings.as_ref().is_some_and(|s| s.bitrate_bps.is_some() || s.gop.is_some() || s.quality.is_some() || s.max_width.is_some() || s.max_height.is_some());
        let raw_format = stats.raw_format.unwrap_or_else(|| RawRecordingFormat::from_codec(source_codec));
        let needs_transcode = transcode || raw_format.as_codec() != Some(target_codec);
        let settings = if needs_transcode { settings } else { None };
        if needs_transcode {
            finalize_recording_mp4(raw_path, output_path, raw_format, target_codec, remux_fps, settings).await?;
        } else {
            finalize_recording_mp4(raw_path, output_path, raw_format, target_codec, remux_fps, None).await?;
        }
    }

    Ok(stats)
}

pub(super) async fn maybe_rewrite_encoded_frame_timestamps(path: &Path, stats: &RecordingStats, wall_start_ms: u64, wall_end_ms: u64) -> std::result::Result<(), String> {
    if !policy::rewrite_encoded_frame_timestamps_to_wall_enabled() {
        return Ok(());
    }
    if stats.frames == 0 {
        return Ok(());
    }
    let wall_span_ms = wall_end_ms.saturating_sub(wall_start_ms);
    if wall_span_ms == 0 {
        return Ok(());
    }
    let recorded_span_ms = match (stats.first_ts_ms, stats.last_ts_ms) {
        (Some(first), Some(last)) if last > first => Some(last.saturating_sub(first)),
        _ => None,
    };
    // Optional legacy behavior: normalize encoded timestamp sidecars to wall-clock capture span.
    //
    // Disabled by default because stretching timestamps to wall span can make playback cadence
    // diverge from the actual encoded frame cadence.
    tracing::info!(
        path = %path.display(),
        frames = stats.frames,
        wall_span_ms,
        recorded_span_ms = ?recorded_span_ms,
        "rewriting encoded frame timestamps to wall-clock span"
    );
    rewrite_timestamp_file(path, wall_start_ms, wall_end_ms, stats.frames).await
}

pub(super) async fn rewrite_timestamp_file(path: &Path, start_ms: u64, end_ms: u64, frames: u64) -> std::result::Result<(), String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| format!("recording timestamp dir create failed: {err}"))?;
        }
        let file = std::fs::File::create(&path).map_err(|err| format!("recording timestamp file rewrite failed: {err}"))?;
        let mut writer = std::io::BufWriter::new(file);
        if frames <= 1 || end_ms <= start_ms {
            writeln!(writer, "{start_ms}").map_err(|err| format!("recording timestamp rewrite failed: {err}"))?;
        } else {
            let span = end_ms.saturating_sub(start_ms) as u128;
            let denom = (frames.saturating_sub(1)) as u128;
            for index in 0..frames {
                let offset = (span.saturating_mul(index as u128)) / denom;
                let ts = start_ms.saturating_add(offset as u64);
                writeln!(writer, "{ts}").map_err(|err| format!("recording timestamp rewrite failed: {err}"))?;
            }
        }
        writer.flush().map_err(|err| format!("recording timestamp rewrite flush failed: {err}"))
    })
    .await
    .map_err(|_| "recording timestamp rewrite task failed".to_string())?
}

#[allow(dead_code)]
pub(super) async fn probe_prefix_from_segments(selection: &[(u64, PathBuf)], codec: RecordingCodec) -> Option<Vec<u8>> {
    let mut cache = RecordingConfigCache::default();
    let mut scanned = 0usize;
    let max_scan = 1024 * 1024; // 1MiB
    let mut buf = vec![0u8; 256 * 1024];
    for (_, path) in selection {
        if cache.ready(codec) || scanned >= max_scan {
            break;
        }
        let mut file = File::open(path).await.ok()?;
        loop {
            if cache.ready(codec) || scanned >= max_scan {
                break;
            }
            let n = file.read(&mut buf).await.ok()?;
            if n == 0 {
                break;
            }
            scanned = scanned.saturating_add(n);
            cache.update_from_annexb(codec, &buf[..n]);
        }
    }
    cache.prefix(codec)
}

pub(super) struct ShadowRecordingSessionRequest {
    pub(super) stream_id: Uuid,
    pub(super) shadow_dir: PathBuf,
    pub(super) codec: RecordingCodec,
    pub(super) output_path: PathBuf,
    pub(super) raw_path: PathBuf,
    pub(super) container: RecordingContainer,
    pub(super) started_at_ms: u64,
    pub(super) duration_ms: Option<u64>,
    pub(super) fps: Option<f32>,
    pub(super) settings: Option<crate::ipc::RecordingSettings>,
    pub(super) stop_rx: oneshot::Receiver<()>,
}

pub(super) async fn record_shadow_segments_session(request: ShadowRecordingSessionRequest) -> std::result::Result<RecordingStats, String> {
    let ShadowRecordingSessionRequest { stream_id: _stream_id, shadow_dir, codec, output_path, raw_path, container, started_at_ms, duration_ms, fps, settings, mut stop_rx } = request;
    if fs::metadata(&shadow_dir).await.is_err() {
        return Err("shadow recorder data not found".to_string());
    }

    let record_path = if matches!(container, RecordingContainer::Mp4) { raw_path.as_path() } else { output_path.as_path() };
    let wall_start_ms = started_at_ms;

    // Wait for the requested window to elapse, then do a single shadow capture ending "now".
    // This avoids racing the shadow writer's buffered IO while it's still actively appending to
    // the live segment file.
    let deadline = duration_ms.filter(|ms| *ms > 0).map(|ms| Instant::now() + Duration::from_millis(ms));
    let stop_grace = Duration::from_millis(recording_stop_grace_ms());
    let mut stop_deadline: Option<Instant> = None;

    loop {
        match (stop_deadline, deadline) {
            (Some(stop_until), Some(dl)) => {
                tokio::select! {
                    _ = sleep_until(stop_until) => break,
                    _ = sleep_until(dl) => break,
                }
            }
            (Some(stop_until), None) => {
                tokio::select! {
                    _ = sleep_until(stop_until) => break,
                }
            }
            (None, Some(dl)) => {
                tokio::select! {
                    _ = &mut stop_rx => {
                        stop_deadline = Some(Instant::now() + stop_grace);
                    }
                    _ = sleep_until(dl) => break,
                }
            }
            (None, None) => {
                tokio::select! {
                    _ = &mut stop_rx => {
                        stop_deadline = Some(Instant::now() + stop_grace);
                    }
                }
            }
        }
    }

    // Give the shadow worker a chance to flush the latest bytes to disk.
    // (Shadow flush interval is 250ms; this keeps regular recordings from "missing the tail".)
    tokio::time::sleep(Duration::from_millis(400)).await;

    let wall_end_ms = current_time_ms();
    let window_ms = if let Some(ms) = duration_ms.filter(|ms| *ms > 0) { ms } else { wall_end_ms.saturating_sub(started_at_ms) };
    if window_ms == 0 {
        return Err("recording duration must be > 0".to_string());
    }
    let max_window = shadow_window_ms();
    if window_ms > max_window {
        return Err(format!("recording duration ({window_ms}ms) exceeds shadow window ({max_window}ms); increase HELIOS_SHADOW_WINDOW_MS"));
    }

    let preroll_ms = shadow_segment_ms().max(1_000);
    let capture_ms = window_ms.saturating_add(preroll_ms).min(max_window);
    let total_bytes = capture_shadow_segments(&shadow_dir, record_path, codec, capture_ms).await?;

    if matches!(container, RecordingContainer::Mp4) {
        let raw_format = RawRecordingFormat::from_codec(codec);
        // Shadow segments can start mid-GOP; pre-roll improves the chance ffmpeg sees an IDR
        // before the requested window. We then trim down to the requested duration.
        let forced_fps = probe_raw_frames(&raw_path, raw_format).await.ok().map(|frames| {
            let secs = (capture_ms as f32 / 1000.0).max(0.001);
            (frames as f32 / secs).clamp(1.0, 240.0)
        });
        let fps = forced_fps.or(fps);
        let pretrim = pretrim_output_path(&output_path);
        finalize_recording_mp4(raw_path, pretrim.clone(), raw_format, codec, fps, settings).await?;
        let trim_res = trim_mp4_to_last_window(&pretrim, &output_path, window_ms, codec).await;
        let _ = tokio::fs::remove_file(&pretrim).await;
        trim_res?;
    }

    Ok(RecordingStats { frames: 0, bytes: total_bytes, first_ts_ms: Some(wall_start_ms), last_ts_ms: Some(wall_end_ms), raw_format: Some(RawRecordingFormat::from_codec(codec)) })
}

pub(super) async fn finalize_recording_mp4(
    raw_path: PathBuf,
    output_path: PathBuf,
    raw_format: RawRecordingFormat,
    codec: RecordingCodec,
    fps: Option<f32>,
    settings: Option<crate::ipc::RecordingSettings>,
) -> std::result::Result<(), String> {
    if let Some(parent) = output_path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|err| format!("recording output dir create failed: {err}"))?;
    }
    let temp_path = temp_output_path(&output_path);
    let transcode =
        settings.as_ref().is_some_and(|s| s.bitrate_bps.is_some() || s.gop.is_some() || s.quality.is_some() || s.max_width.is_some() || s.max_height.is_some()) || raw_format.as_codec() != Some(codec);
    let result = if transcode { transcode_raw_to_mp4(&raw_path, &temp_path, raw_format, codec, fps, settings.as_ref()).await } else { remux_raw_to_mp4(&raw_path, &temp_path, raw_format, fps).await };
    if let Err(err) = result {
        let _ = tokio::fs::remove_file(&temp_path).await;
        // Default to cleanup: raw bitstreams can be large and are typically not useful to keep.
        if !keep_raw_on_record_fail() {
            let _ = tokio::fs::remove_file(&raw_path).await;
        }
        return Err(err);
    }

    // Guardrail: ffmpeg can sometimes produce a "valid" MP4 with zero streams when the input
    // is missing codec parameters (VPS/SPS/PPS) or contains no packets. Treat that as failure.
    let meta = tokio::fs::metadata(&temp_path).await.map_err(|err| format!("recording output stat failed: {err}"))?;
    if meta.len() < 1024 {
        let _ = tokio::fs::remove_file(&temp_path).await;
        if !keep_raw_on_record_fail() {
            let _ = tokio::fs::remove_file(&raw_path).await;
        }
        return Err("ffmpeg produced empty mp4 (no streams)".to_string());
    }

    if let Err(err) = tokio::fs::rename(&temp_path, &output_path).await {
        tracing::warn!(error = %err, "recording output rename failed; retrying");
        let _ = tokio::fs::remove_file(&output_path).await;
        if let Err(err2) = tokio::fs::rename(&temp_path, &output_path).await {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(format!("recording output rename failed: {err2}"));
        }
    }
    let _ = tokio::fs::remove_file(&raw_path).await;
    Ok(())
}

pub(super) async fn probe_raw_frames(raw_path: &Path, raw_format: RawRecordingFormat) -> std::result::Result<u64, String> {
    let raw = raw_path.to_path_buf();
    let demux = raw_format.ffmpeg_demux().to_string();
    tokio::task::spawn_blocking(move || {
        fn run(raw: &Path, demux: &str, count_mode: &str, field: &str) -> std::result::Result<u64, String> {
            let mut cmd = Command::new("ffprobe");
            cmd.arg("-v").arg("error");
            cmd.arg("-f").arg(demux);
            cmd.arg(count_mode);
            cmd.arg("-select_streams").arg("v:0");
            cmd.arg("-show_entries").arg(format!("stream={field}"));
            cmd.arg("-of").arg("default=nw=1:nk=1");
            cmd.arg(raw);
            let output_res = cmd.output().map_err(|err| format!("ffprobe launch failed: {err}"))?;
            if !output_res.status.success() {
                let stderr = String::from_utf8_lossy(&output_res.stderr);
                let reason = stderr.trim();
                if reason.is_empty() {
                    return Err(format!("ffprobe failed with status {}", output_res.status));
                }
                return Err(format!("ffprobe failed: {reason}"));
            }
            let stdout = String::from_utf8_lossy(&output_res.stdout);
            let trimmed = stdout.trim();
            trimmed.parse::<u64>().map_err(|_| format!("ffprobe returned non-numeric frame count: {trimmed:?}"))
        }

        // Prefer decoding-backed frame counts. When the stream begins mid-GOP, `nb_read_frames`
        // matches what ffmpeg can actually output, while packets may include unusable slices.
        run(&raw, &demux, "-count_frames", "nb_read_frames").or_else(|_| run(&raw, &demux, "-count_packets", "nb_read_packets"))
    })
    .await
    .map_err(|_| "ffprobe task failed".to_string())?
}

pub(super) async fn trim_mp4_to_last_window(input_path: &Path, output_path: &Path, window_ms: u64, codec: RecordingCodec) -> std::result::Result<(), String> {
    if window_ms == 0 {
        return Err("trim window must be > 0".to_string());
    }
    let input = input_path.to_path_buf();
    let output = output_path.to_path_buf();
    let temp = temp_output_path(&output);
    let temp_for_cmd = temp.clone();
    let secs = (window_ms as f64 / 1000.0).max(0.001);
    let run_res = tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y").arg("-loglevel").arg("error");
        cmd.arg("-sseof").arg(format!("-{secs:.03}"));
        cmd.arg("-i").arg(&input);
        cmd.arg("-t").arg(format!("{secs:.03}"));
        cmd.arg("-c:v").arg("copy");
        if matches!(codec, RecordingCodec::H265) {
            cmd.arg("-tag:v").arg("hvc1");
        }
        cmd.arg("-movflags").arg("+faststart");
        cmd.arg(&temp_for_cmd);
        let output_res = cmd.output().map_err(|err| format!("ffmpeg launch failed: {err}"))?;
        if !output_res.status.success() {
            let stderr = String::from_utf8_lossy(&output_res.stderr);
            let reason = stderr.trim();
            if reason.is_empty() {
                return Err(format!("ffmpeg failed with status {}", output_res.status));
            }
            return Err(format!("ffmpeg failed: {reason}"));
        }
        Ok::<_, String>(())
    })
    .await
    .map_err(|_| "ffmpeg task failed".to_string())?;
    if let Err(err) = run_res {
        let _ = tokio::fs::remove_file(&temp).await;
        return Err(err);
    }

    let meta = tokio::fs::metadata(&temp).await.map_err(|err| format!("trim output stat failed: {err}"))?;
    if meta.len() < 1024 {
        let _ = tokio::fs::remove_file(&temp).await;
        return Err("ffmpeg produced empty trimmed mp4 (no streams)".to_string());
    }

    if let Err(err) = tokio::fs::rename(&temp, &output).await {
        tracing::warn!(error = %err, "trim output rename failed; retrying");
        let _ = tokio::fs::remove_file(&output).await;
        if let Err(err2) = tokio::fs::rename(&temp, &output).await {
            let _ = tokio::fs::remove_file(&temp).await;
            return Err(format!("trim output rename failed: {err2}"));
        }
    }
    Ok(())
}

pub(super) async fn remux_raw_to_mp4(raw_path: &Path, output_path: &Path, raw_format: RawRecordingFormat, fps: Option<f32>) -> std::result::Result<(), String> {
    let raw = raw_path.to_path_buf();
    let output = output_path.to_path_buf();
    let fps_value = fps.filter(|value| value.is_finite() && *value > 0.0).map(|v| v.clamp(1.0, 240.0)).unwrap_or(30.0);
    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y").arg("-loglevel").arg("error");
        cmd.arg("-fflags").arg("+genpts");
        cmd.arg("-r").arg(format!("{fps_value:.03}"));
        cmd.arg("-f").arg(raw_format.ffmpeg_demux());
        cmd.arg("-i").arg(&raw);
        cmd.arg("-c:v").arg("copy");
        if matches!(raw_format, RawRecordingFormat::H265) {
            cmd.arg("-tag:v").arg("hvc1");
        }
        cmd.arg("-movflags").arg("+faststart");
        cmd.arg(&output);
        let output_res = cmd.output().map_err(|err| format!("ffmpeg launch failed: {err}"))?;
        if output_res.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output_res.stderr);
        let reason = stderr.trim();
        if reason.is_empty() {
            Err(format!("ffmpeg failed with status {}", output_res.status))
        } else {
            Err(format!("ffmpeg failed: {reason}"))
        }
    })
    .await
    .map_err(|_| "ffmpeg task failed".to_string())?
}

pub(super) async fn transcode_raw_to_mp4(
    raw_path: &Path,
    output_path: &Path,
    raw_format: RawRecordingFormat,
    codec: RecordingCodec,
    fps: Option<f32>,
    settings: Option<&crate::ipc::RecordingSettings>,
) -> std::result::Result<(), String> {
    let raw = raw_path.to_path_buf();
    let output = output_path.to_path_buf();
    let fps_value = fps.filter(|value| value.is_finite() && *value > 0.0).map(|v| v.clamp(1.0, 240.0)).unwrap_or(30.0);
    let cfg = settings.cloned().unwrap_or_default();
    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y").arg("-loglevel").arg("error");
        cmd.arg("-fflags").arg("+genpts");
        cmd.arg("-r").arg(format!("{fps_value:.03}"));
        cmd.arg("-f").arg(raw_format.ffmpeg_demux());
        cmd.arg("-i").arg(&raw);
        if cfg.max_width.is_some() || cfg.max_height.is_some() {
            let width = cfg.max_width.unwrap_or(0);
            let height = cfg.max_height.unwrap_or(0);
            let scale = if width > 0 && height > 0 {
                format!("scale='min({width},iw)':'min({height},ih)':force_original_aspect_ratio=decrease")
            } else if width > 0 {
                format!("scale='min({width},iw)':-2:force_original_aspect_ratio=decrease")
            } else if height > 0 {
                format!("scale=-2:'min({height},ih)':force_original_aspect_ratio=decrease")
            } else {
                String::new()
            };
            if !scale.is_empty() {
                cmd.arg("-vf").arg(scale);
            }
        }
        match codec {
            RecordingCodec::H264 => {
                cmd.arg("-c:v").arg("libx264");
                // Keep output browser-friendly; some players reject High 4:4:4 profiles.
                cmd.arg("-pix_fmt").arg("yuv420p");
                cmd.arg("-profile:v").arg("high");
            }
            RecordingCodec::H265 => {
                cmd.arg("-c:v").arg("libx265");
            }
        }
        if let Some(bitrate) = cfg.bitrate_bps.filter(|v| *v > 0) {
            cmd.arg("-b:v").arg(bitrate.to_string());
        } else if let Some(crf) = cfg.quality.filter(|v| *v <= 51) {
            cmd.arg("-crf").arg(crf.to_string());
        }
        if let Some(gop) = cfg.gop.filter(|v| *v > 0) {
            cmd.arg("-g").arg(gop.to_string());
        }
        cmd.arg("-preset").arg("veryfast");
        if matches!(codec, RecordingCodec::H265) {
            cmd.arg("-tag:v").arg("hvc1");
        }
        cmd.arg("-movflags").arg("+faststart");
        cmd.arg(&output);
        let output_res = cmd.output().map_err(|err| format!("ffmpeg launch failed: {err}"))?;
        if output_res.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output_res.stderr);
        let reason = stderr.trim();
        if reason.is_empty() {
            Err(format!("ffmpeg failed with status {}", output_res.status))
        } else {
            Err(format!("ffmpeg failed: {reason}"))
        }
    })
    .await
    .map_err(|_| "ffmpeg task failed".to_string())?
}

pub(super) async fn receive_snapshot_source_frame(rx: &mut Receiver<Arc<image::DynamicImage>>, pipeline_graph: Option<&crate::graph::GraphHandle>) -> Result<image::DynamicImage> {
    let deadline = Instant::now() + SNAPSHOT_SOURCE_TIMEOUT;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(Error::Timeout);
        }
        let next = timeout(remaining, rx.recv()).await;
        let frame = match next {
            Ok(Ok(frame)) => frame,
            Ok(Err(RecvError::Lagged(_))) => continue,
            Ok(Err(RecvError::Closed)) => return Err(Error::InvalidState("snapshot source stream closed")),
            Err(_) => return Err(Error::Timeout),
        };
        let image = (*frame).clone();
        if let Some(graph) = pipeline_graph {
            if let Some(processed) = graph.process(image) {
                return Ok(processed);
            }
            continue;
        }
        return Ok(image);
    }
}

pub(super) fn encode_snapshot_jpeg(image: &image::DynamicImage, quality: u8) -> Result<Vec<u8>> {
    let width = image.width().max(1);
    let height = image.height().max(1);
    let wanted = width as usize * height as usize * 3;
    let mut rgb = vec![0u8; wanted];
    if !write_rgb24(image, &mut rgb) {
        return Err(Error::InvalidState("snapshot rgb24 conversion failed"));
    }
    use image::codecs::jpeg::JpegEncoder;
    use image::ColorType;

    let mut jpeg = Vec::<u8>::new();
    let quality = quality.clamp(1, 100);
    let mut enc = JpegEncoder::new_with_quality(&mut jpeg, quality);
    enc.encode(&rgb, width, height, ColorType::Rgb8.into()).map_err(|_| Error::InvalidState("snapshot jpeg encode failed"))?;
    Ok(jpeg)
}

pub(super) fn write_rgb24(image: &image::DynamicImage, out: &mut [u8]) -> bool {
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
            for row in 0..height {
                let src_row = row.saturating_mul(width).saturating_mul(4);
                let dst_row = row.saturating_mul(width).saturating_mul(3);
                for col in 0..width {
                    let src = src_row.saturating_add(col.saturating_mul(4));
                    let dst = dst_row.saturating_add(col.saturating_mul(3));
                    if src + 3 < raw.len() && dst + 2 < out.len() {
                        out[dst] = raw[src];
                        out[dst + 1] = raw[src + 1];
                        out[dst + 2] = raw[src + 2];
                    }
                }
            }
            true
        }
        image::DynamicImage::ImageLuma8(buf) => {
            let raw = buf.as_raw();
            if raw.len() != width.saturating_mul(height) {
                return false;
            }
            for row in 0..height {
                let src_row = row.saturating_mul(width);
                let dst_row = row.saturating_mul(width).saturating_mul(3);
                for col in 0..width {
                    let src = src_row.saturating_add(col);
                    let dst = dst_row.saturating_add(col.saturating_mul(3));
                    if src < raw.len() && dst + 2 < out.len() {
                        let v = raw[src];
                        out[dst] = v;
                        out[dst + 1] = v;
                        out[dst + 2] = v;
                    }
                }
            }
            true
        }
        _ => false,
    }
}
