use super::*;

pub(crate) fn recording_state_to_result(state: RecordingState) -> Result<()> {
    match state {
        RecordingState::Completed(Ok(_)) => Ok(()),
        RecordingState::Completed(Err(reason)) => Err(Error::InvalidStateOwned(reason)),
        RecordingState::Running => Err(Error::InvalidState("recording still running")),
    }
}

pub(crate) fn resolve_recording_source(manifest: &ResolvedStreamConfig, source: RecordingSource) -> ResolvedRecordingSource {
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

pub(crate) fn manifest_contains_pipeline(manifest: &ResolvedStreamConfig, pipeline_id: Uuid) -> bool {
    if manifest.active_pipeline_id == Some(pipeline_id) {
        return true;
    }
    manifest.pipelines.iter().any(|binding| binding.pipeline_id == pipeline_id)
}

pub(crate) fn build_recording_pipeline_graph(manifest: &ResolvedStreamConfig, pipeline_id: Uuid, output_key: Option<&str>) -> Result<crate::graph::GraphHandle> {
    crate::graph::build_graph_handle_for_pipeline_output(manifest.host_buffer(), manifest, pipeline_id, output_key)
        .map_err(|err| Error::InvalidStateOwned(format!("recording pipeline build failed: {err}")))
}

pub(crate) fn recording_codec_ext(codec: RecordingCodec) -> &'static str {
    match codec {
        RecordingCodec::H264 => "h264",
        RecordingCodec::H265 => "h265",
    }
}

pub(crate) fn build_raw_path(output_path: &Path, codec: RecordingCodec) -> PathBuf {
    let ext = recording_codec_ext(codec);
    let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = output_path.file_stem().and_then(|value| value.to_str()).filter(|value| !value.is_empty()).unwrap_or("recording");
    let suffix = Uuid::new_v4().simple().to_string();
    parent.join(format!("{stem}.raw-{suffix}.{ext}"))
}

pub(crate) fn recording_frame_ts_path(output_path: &Path) -> PathBuf {
    let parent = output_path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = output_path.file_name().and_then(|value| value.to_str()).filter(|value| !value.is_empty()).unwrap_or("recording");
    parent.join(format!("{file_name}.frame_ts.txt"))
}

pub(crate) fn temp_output_path(output_path: &Path) -> PathBuf {
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

pub(crate) fn pretrim_output_path(output_path: &Path) -> PathBuf {
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

pub(crate) fn current_time_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|value| value.as_millis() as u64).unwrap_or(0)
}

pub(crate) fn shadow_window_ms() -> u64 {
    policy::shadow_window_ms()
}

pub(crate) fn shadow_segment_ms() -> u64 {
    policy::shadow_segment_ms()
}

pub(crate) fn shadow_flush_interval_ms() -> u64 {
    policy::shadow_flush_interval_ms()
}

pub(crate) fn shadow_writer_buffer_bytes() -> usize {
    policy::shadow_writer_buffer_bytes()
}

pub(crate) fn shadow_config_scan_interval_ms() -> u64 {
    policy::shadow_config_scan_interval_ms()
}

pub(crate) fn recording_stop_grace_ms() -> u64 {
    policy::recording_stop_grace_ms()
}

pub(crate) fn recording_frame_queue_size() -> usize {
    policy::recording_frame_queue_size()
}

pub(crate) fn keep_raw_on_record_fail() -> bool {
    policy::keep_raw_on_record_fail()
}

pub(crate) fn parse_shadow_segment_timestamp(name: &str, ext: &str) -> Option<u64> {
    let suffix = format!(".{ext}");
    let trimmed = name.strip_suffix(&suffix)?;
    let ts = trimmed.strip_prefix("segment_")?;
    ts.parse::<u64>().ok()
}

pub(crate) async fn list_shadow_segments(dir: &Path, codec: RecordingCodec) -> std::result::Result<Vec<(u64, PathBuf)>, String> {
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

pub(crate) fn cleanup_shadow_segments_sync(dir: &Path, cutoff_ms: u64, codec: RecordingCodec) -> std::result::Result<(), String> {
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

pub(crate) fn cleanup_shadow_dir_sync(dir: &Path) -> std::result::Result<(), String> {
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

pub(crate) fn open_shadow_segment_sync(dir: &Path, start_ms: u64, codec: RecordingCodec, writer_capacity: usize) -> std::result::Result<std::io::BufWriter<std::fs::File>, String> {
    let ext = recording_codec_ext(codec);
    let path = dir.join(format!("segment_{start_ms}.{ext}"));
    let file = std::fs::File::create(&path).map_err(|err| format!("shadow segment open failed: {err}"))?;
    Ok(std::io::BufWriter::with_capacity(writer_capacity, file))
}

pub(crate) fn is_annexb_prefix(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0, 0, 0, 1]) || bytes.starts_with(&[0, 0, 1])
}

pub(crate) fn is_mjpeg_payload(bytes: &[u8]) -> bool {
    bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF
}

pub(crate) fn read_be_len(bytes: &[u8], len_size: usize) -> Option<usize> {
    match len_size {
        1 => Some(*bytes.first()? as usize),
        2 => Some(u16::from_be_bytes([*bytes.first()?, *bytes.get(1)?]) as usize),
        3 => Some(((*bytes.first()? as usize) << 16) | ((*bytes.get(1)? as usize) << 8) | (*bytes.get(2)? as usize)),
        4 => Some(u32::from_be_bytes([*bytes.first()?, *bytes.get(1)?, *bytes.get(2)?, *bytes.get(3)?]) as usize),
        _ => None,
    }
}

pub(crate) fn length_prefixed_to_annexb_with_len(bytes: &[u8], len_size: usize, out: &mut Vec<u8>) -> bool {
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

pub(crate) fn length_prefixed_to_annexb_best_effort(bytes: &[u8], out: &mut Vec<u8>) -> bool {
    // Most common is 4. Some streams (notably some hardware paths) use 2.
    for len_size in [4usize, 3, 2, 1] {
        if length_prefixed_to_annexb_with_len(bytes, len_size, out) {
            return true;
        }
    }
    false
}

pub(crate) fn find_annexb_start(bytes: &[u8], from: usize) -> Option<(usize, usize)> {
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

pub(crate) struct RawPayloadWriteState<'a> {
    pub(crate) writer: &'a mut dyn Write,
    pub(crate) raw_format: &'a mut RawRecordingFormat,
    pub(crate) format_tracker: Option<&'a Arc<AtomicU8>>,
    pub(crate) bitstream: &'a mut RecordingBitstream,
    pub(crate) convert_buf: &'a mut Vec<u8>,
    pub(crate) config_cache: &'a mut RecordingConfigCache,
    pub(crate) wrote_prefix: &'a mut bool,
    pub(crate) pending_before_config: &'a mut std::collections::VecDeque<Vec<u8>>,
    pub(crate) pending_bytes: &'a mut usize,
    pub(crate) stats: &'a mut RecordingStats,
    pub(crate) ts_writer: &'a mut Option<RecordingTimestampWriter>,
}

pub(crate) struct RawPayloadWriteInput<'a> {
    pub(crate) codec: RecordingCodec,
    pub(crate) payload: &'a [u8],
    pub(crate) ts_ms: u64,
    pub(crate) pending_max_bytes: usize,
    pub(crate) allow_mjpeg_switch: bool,
}

pub(crate) fn write_payload_to_raw(state: RawPayloadWriteState<'_>, input: RawPayloadWriteInput<'_>) -> std::result::Result<(), String> {
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

pub(crate) async fn capture_shadow_segments(shadow_dir: &Path, output_path: &Path, codec: RecordingCodec, window_ms: u64) -> std::result::Result<u64, String> {
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

pub(crate) async fn run_shadow_recorder_stream(
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
