use super::*;

pub(crate) fn encoder_output_matches(codec: RecordingCodec, desc: &styx::codec::CodecDescriptor) -> bool {
    let name = desc.name.to_ascii_lowercase();
    let impl_name = desc.impl_name.to_ascii_lowercase();
    let output = String::from_utf8_lossy(&desc.output.to_u32().to_le_bytes()).to_ascii_lowercase();
    match codec {
        RecordingCodec::H264 => ["h264", "avc"].iter().any(|token| name.contains(token) || impl_name.contains(token) || output.contains(token)),
        RecordingCodec::H265 => ["h265", "hevc"].iter().any(|token| name.contains(token) || impl_name.contains(token) || output.contains(token)),
    }
}

pub(crate) fn raw_format_from_desc(desc: &styx::codec::CodecDescriptor) -> Option<RawRecordingFormat> {
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

pub(crate) fn select_encoder_for_codec(codec: RecordingCodec, encoder_hint: Option<&str>, allow_mjpeg: bool) -> std::result::Result<(RawRecordingFormat, Arc<dyn Codec>), String> {
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

pub(crate) fn select_recording_encoder(preferred: RecordingCodec, encoder_hint: Option<&str>) -> std::result::Result<(RawRecordingFormat, RecordingCodec, Arc<dyn Codec>), String> {
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

pub(crate) struct RecordingFrameParams<'a> {
    pub(crate) output_path: &'a Path,
    pub(crate) raw_path: &'a Path,
    pub(crate) container: RecordingContainer,
    pub(crate) codec: RecordingCodec,
    pub(crate) duration_ms: Option<u64>,
    pub(crate) fps: Option<f32>,
    pub(crate) settings: Option<crate::ipc::RecordingSettings>,
    pub(crate) encoder_hint: Option<String>,
    pub(crate) frame_ts_path: Option<PathBuf>,
}

pub(crate) async fn record_frame_stream(mut source: RecordingFrameSource, mut stop_rx: oneshot::Receiver<()>, params: RecordingFrameParams<'_>) -> std::result::Result<RecordingStats, String> {
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

pub(crate) fn write_encoded_chunks_to_raw(
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

pub(crate) async fn record_encoded_stream(
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

pub(crate) struct EncodedRecordingSessionRequest {
    pub(crate) rx: Receiver<EncodedFrame>,
    pub(crate) stop_rx: oneshot::Receiver<()>,
    pub(crate) output_path: PathBuf,
    pub(crate) raw_path: PathBuf,
    pub(crate) container: RecordingContainer,
    pub(crate) source_codec: RecordingCodec,
    pub(crate) target_codec: RecordingCodec,
    pub(crate) duration_ms: Option<u64>,
    pub(crate) fps: Option<f32>,
    pub(crate) settings: Option<crate::ipc::RecordingSettings>,
    pub(crate) timestamps_path: Option<PathBuf>,
    pub(crate) consumer_touch: Option<Arc<AtomicU64>>,
}

pub(crate) async fn record_encoded_session(request: EncodedRecordingSessionRequest) -> std::result::Result<RecordingStats, String> {
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

pub(crate) async fn maybe_rewrite_encoded_frame_timestamps(path: &Path, stats: &RecordingStats, wall_start_ms: u64, wall_end_ms: u64) -> std::result::Result<(), String> {
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

pub(crate) async fn rewrite_timestamp_file(path: &Path, start_ms: u64, end_ms: u64, frames: u64) -> std::result::Result<(), String> {
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
pub(crate) async fn probe_prefix_from_segments(selection: &[(u64, PathBuf)], codec: RecordingCodec) -> Option<Vec<u8>> {
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

pub(crate) struct ShadowRecordingSessionRequest {
    pub(crate) stream_id: Uuid,
    pub(crate) shadow_dir: PathBuf,
    pub(crate) codec: RecordingCodec,
    pub(crate) output_path: PathBuf,
    pub(crate) raw_path: PathBuf,
    pub(crate) container: RecordingContainer,
    pub(crate) started_at_ms: u64,
    pub(crate) duration_ms: Option<u64>,
    pub(crate) fps: Option<f32>,
    pub(crate) settings: Option<crate::ipc::RecordingSettings>,
    pub(crate) stop_rx: oneshot::Receiver<()>,
}

pub(crate) async fn record_shadow_segments_session(request: ShadowRecordingSessionRequest) -> std::result::Result<RecordingStats, String> {
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
