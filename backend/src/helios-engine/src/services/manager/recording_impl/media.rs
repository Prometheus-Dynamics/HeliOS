use super::*;

pub(crate) async fn finalize_recording_mp4(
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

pub(crate) async fn probe_raw_frames(raw_path: &Path, raw_format: RawRecordingFormat) -> std::result::Result<u64, String> {
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

pub(crate) async fn trim_mp4_to_last_window(input_path: &Path, output_path: &Path, window_ms: u64, codec: RecordingCodec) -> std::result::Result<(), String> {
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

pub(crate) async fn remux_raw_to_mp4(raw_path: &Path, output_path: &Path, raw_format: RawRecordingFormat, fps: Option<f32>) -> std::result::Result<(), String> {
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

pub(crate) async fn transcode_raw_to_mp4(
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

pub(crate) async fn receive_snapshot_source_frame(rx: &mut Receiver<Arc<image::DynamicImage>>, pipeline_graph: Option<&crate::graph::GraphHandle>) -> Result<image::DynamicImage> {
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

pub(crate) fn encode_snapshot_jpeg(image: &image::DynamicImage, quality: u8) -> Result<Vec<u8>> {
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

pub(crate) fn write_rgb24(image: &image::DynamicImage, out: &mut [u8]) -> bool {
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
