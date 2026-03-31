use axum::{
    body::Body,
    http::{HeaderMap, StatusCode, header},
    response::Response,
};
use serde::Deserialize;
use std::{
    io::{BufReader, SeekFrom},
    path::{Path, PathBuf},
    process::Command,
};
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncSeekExt},
};
use tokio_util::io::ReaderStream;

use crate::http::{
    error::{ApiError, ApiResult},
    storage::{self, sanitize_name},
};

use super::{support::map_io_error, types::MediaMetadata};

pub(super) async fn hydrate_dimensions(meta: &mut MediaMetadata, path: &Path, content_type: &str) {
    if !content_type.starts_with("image/") {
        return;
    }
    if meta.width.is_some() && meta.height.is_some() {
        return;
    }
    let bytes = match fs::read(path).await {
        Ok(bytes) => bytes,
        Err(_) => return,
    };
    if let Ok((width, height)) = crate::api_tools_client::image_dimensions(&bytes).await
        && let (Some(width), Some(height)) = (width, height)
    {
        meta.width = Some(width);
        meta.height = Some(height);
    }
}

#[derive(Debug, Deserialize)]
struct FfprobeOutput {
    #[serde(default)]
    streams: Vec<FfprobeStream>,
}

#[derive(Debug, Deserialize)]
struct FfprobeStream {
    width: Option<u32>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    codec_name: Option<String>,
    codec_tag_string: Option<String>,
}

#[derive(Debug, Default)]
struct VideoProbe {
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<f32>,
    codec: Option<String>,
}

pub(super) async fn hydrate_video_metadata(meta: &mut MediaMetadata, path: &Path, filename: &str, content_type: &str) {
    if !content_type.starts_with("video/") {
        return;
    }
    if meta.video_codec.is_none() {
        meta.video_codec = infer_video_codec_hint(filename, content_type);
    }
    if meta.video_codec.is_some() && meta.width.is_some() && meta.height.is_some() {
        return;
    }
    let file = path.to_path_buf();
    let probe = tokio::task::spawn_blocking(move || probe_video_stream(&file)).await.ok().flatten();
    let Some(probe) = probe else {
        return;
    };
    if meta.width.is_none() {
        meta.width = probe.width;
    }
    if meta.height.is_none() {
        meta.height = probe.height;
    }
    if meta.fps.is_none() {
        meta.fps = probe.fps;
    }
    if let Some(codec) = probe.codec {
        meta.video_codec = Some(codec);
    }
}

fn probe_video_stream(path: &Path) -> Option<VideoProbe> {
    let output = Command::new("ffprobe")
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v:0")
        .arg("-show_entries")
        .arg("stream=width,height,avg_frame_rate,r_frame_rate,codec_name,codec_tag_string")
        .arg("-of")
        .arg("json")
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let parsed = serde_json::from_slice::<FfprobeOutput>(&output.stdout).ok()?;
    let stream = parsed.streams.into_iter().next()?;
    let fps = stream.avg_frame_rate.as_deref().and_then(parse_ffprobe_ratio).or_else(|| stream.r_frame_rate.as_deref().and_then(parse_ffprobe_ratio)).filter(|value| value.is_finite() && *value > 0.0);
    let codec = stream.codec_name.as_deref().or(stream.codec_tag_string.as_deref()).and_then(normalize_video_codec);
    Some(VideoProbe { width: stream.width, height: stream.height, fps, codec })
}

fn parse_ffprobe_ratio(raw: &str) -> Option<f32> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some((num, den)) = trimmed.split_once('/') {
        let numerator = num.trim().parse::<f32>().ok()?;
        let denominator = den.trim().parse::<f32>().ok()?;
        if denominator.abs() <= f32::EPSILON {
            return None;
        }
        let value = numerator / denominator;
        return if value.is_finite() && value > 0.0 { Some(value) } else { None };
    }
    trimmed.parse::<f32>().ok().filter(|value| value.is_finite() && *value > 0.0)
}

pub(super) fn normalize_video_codec(raw: &str) -> Option<String> {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    let mapped = match normalized.as_str() {
        "hevc" | "hvc1" | "hev1" => "h265",
        "h264" | "avc1" | "avc3" => "h264",
        _ => normalized.as_str(),
    };
    Some(mapped.to_string())
}

fn infer_video_codec_hint(filename: &str, content_type: &str) -> Option<String> {
    let lower_name = filename.to_ascii_lowercase();
    let lower_type = content_type.to_ascii_lowercase();
    if lower_name.ends_with(".h265") || lower_name.ends_with(".hevc") || lower_type.contains("h265") || lower_type.contains("hevc") {
        return Some("h265".to_string());
    }
    if lower_name.ends_with(".h264") || lower_name.ends_with(".avc") || lower_type.contains("h264") || lower_type.contains("avc") {
        return Some("h264".to_string());
    }
    None
}

pub(super) fn media_preview_cache_path(meta_dir: &Path, filename: &str, fps: Option<f32>) -> PathBuf {
    let fps_tag = fps.filter(|value| value.is_finite() && *value > 0.0).map(|value| format!("{value:.3}").replace('.', "_")).unwrap_or_else(|| "auto".to_string());
    meta_dir.join(format!("{filename}.preview.h264.{fps_tag}.mp4"))
}

pub(super) fn media_thumbnail_cache_path(meta_dir: &Path, filename: &str) -> PathBuf {
    meta_dir.join(format!("{filename}.thumb.jpg"))
}

pub(super) async fn preview_cache_fresh(source: &Path, preview: &Path) -> Result<bool, std::io::Error> {
    let source_meta = fs::metadata(source).await?;
    let preview_meta = match fs::metadata(preview).await {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };
    let source_modified = source_meta.modified().ok();
    let preview_modified = preview_meta.modified().ok();
    Ok(matches!(
        (source_modified, preview_modified),
        (Some(src), Some(preview)) if preview >= src
    ))
}

pub(super) async fn transcode_preview_h264(source: &Path, output: &Path, fps_hint: Option<f32>, codec_hint: Option<&str>) -> Result<(), String> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).await.map_err(|err| format!("preview dir create failed: {err}"))?;
    }
    let temp = output.with_extension("tmp.mp4");
    let source_path = source.to_path_buf();
    let temp_path = temp.to_path_buf();
    let ext = source_path.extension().and_then(|value| value.to_str()).unwrap_or_default().to_ascii_lowercase();
    let codec_hint = codec_hint.and_then(normalize_video_codec);
    let fps_hint = fps_hint.filter(|value| value.is_finite() && *value > 0.0);
    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y").arg("-loglevel").arg("error");
        cmd.arg("-fflags").arg("+genpts");
        if let Some(input_format) = ffmpeg_input_format_hint(&ext, codec_hint.as_deref()) {
            cmd.arg("-f").arg(input_format);
        }
        if let Some(fps) = fps_hint {
            cmd.arg("-r").arg(format!("{fps:.6}"));
        }
        cmd.arg("-i").arg(&source_path);
        cmd.arg("-an");
        cmd.arg("-c:v").arg("libx264");
        cmd.arg("-pix_fmt").arg("yuv420p");
        cmd.arg("-profile:v").arg("high");
        cmd.arg("-preset").arg("veryfast");
        cmd.arg("-movflags").arg("+faststart");
        cmd.arg(&temp_path);
        let out = cmd.output().map_err(|err| format!("ffmpeg launch failed: {err}"))?;
        if out.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        let reason = stderr.trim();
        if reason.is_empty() { Err(format!("ffmpeg failed with status {}", out.status)) } else { Err(format!("ffmpeg failed: {reason}")) }
    })
    .await
    .map_err(|_| "ffmpeg preview task failed".to_string())??;
    if let Err(err) = fs::rename(&temp, output).await {
        let _ = fs::remove_file(output).await;
        fs::rename(&temp, output).await.map_err(|err2| format!("preview rename failed after retry ({err}): {err2}"))?;
    }
    Ok(())
}

pub(super) async fn preview_fps_hint(meta_dir: &Path, meta: &MediaMetadata) -> Option<f32> {
    if let Some(frame_ts_name) = meta.frame_timestamps_file_name.as_deref().and_then(sanitize_name)
        && let Some(frame_ts_path) = resolve_frame_ts_path(meta_dir, &frame_ts_name).await
        && let Some(fps) = derive_fps_from_frame_timestamps(&frame_ts_path).await
    {
        return Some(fps);
    }
    meta.fps.filter(|value| value.is_finite() && *value > 0.0)
}

async fn resolve_frame_ts_path(meta_dir: &Path, frame_ts_name: &str) -> Option<PathBuf> {
    let meta_candidate = meta_dir.join(frame_ts_name);
    if fs::metadata(&meta_candidate).await.ok().is_some_and(|meta| meta.is_file()) {
        return Some(meta_candidate);
    }
    let media_dir = storage::ensure_subdir_async("media").await.ok()?;
    let media_candidate = media_dir.join(frame_ts_name);
    fs::metadata(&media_candidate).await.ok().and_then(|meta| meta.is_file().then_some(media_candidate))
}

async fn derive_fps_from_frame_timestamps(path: &Path) -> Option<f32> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(path).ok()?;
        let reader = std::io::BufRead::lines(BufReader::new(file));
        let mut count = 0u64;
        let mut first = None;
        let mut last = None;
        for line in reader {
            let line = line.ok()?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let ts = trimmed.parse::<u64>().ok()?;
            if first.is_none() {
                first = Some(ts);
            }
            last = Some(ts);
            count = count.saturating_add(1);
        }
        let first = first?;
        let last = last?;
        if count <= 1 || last <= first {
            return None;
        }
        let span_ms = last.saturating_sub(first) as f64;
        if span_ms <= f64::EPSILON {
            return None;
        }
        let fps = (count as f64 * 1000.0) / span_ms;
        if !fps.is_finite() || fps <= 0.0 {
            return None;
        }
        Some((fps as f32).clamp(1.0, 240.0))
    })
    .await
    .ok()
    .flatten()
}

pub(super) async fn render_video_thumbnail_jpeg(source: &Path, output: &Path, codec_hint: Option<&str>) -> Result<(), String> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).await.map_err(|err| format!("thumbnail dir create failed: {err}"))?;
    }
    let temp = output.with_extension("tmp.jpg");
    let source_path = source.to_path_buf();
    let temp_path = temp.to_path_buf();
    let ext = source_path.extension().and_then(|value| value.to_str()).unwrap_or_default().to_ascii_lowercase();
    let codec_hint = codec_hint.and_then(normalize_video_codec);
    tokio::task::spawn_blocking(move || {
        let mut cmd = Command::new("ffmpeg");
        cmd.arg("-y").arg("-loglevel").arg("error");
        if let Some(input_format) = ffmpeg_input_format_hint(&ext, codec_hint.as_deref()) {
            cmd.arg("-f").arg(input_format);
        }
        cmd.arg("-i").arg(&source_path);
        cmd.arg("-frames:v").arg("1");
        cmd.arg("-vf").arg("scale='min(640,iw)':-2");
        cmd.arg("-q:v").arg("5");
        cmd.arg(&temp_path);
        let out = cmd.output().map_err(|err| format!("ffmpeg launch failed: {err}"))?;
        if out.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        let reason = stderr.trim();
        if reason.is_empty() { Err(format!("ffmpeg failed with status {}", out.status)) } else { Err(format!("ffmpeg failed: {reason}")) }
    })
    .await
    .map_err(|_| "ffmpeg thumbnail task failed".to_string())??;
    if let Err(err) = fs::rename(&temp, output).await {
        let _ = fs::remove_file(output).await;
        fs::rename(&temp, output).await.map_err(|err2| format!("thumbnail rename failed after retry ({err}): {err2}"))?;
    }
    Ok(())
}

fn ffmpeg_input_format_hint(ext: &str, codec_hint: Option<&str>) -> Option<&'static str> {
    match codec_hint {
        Some("h265") => Some("hevc"),
        Some("h264") => Some("h264"),
        Some("mjpeg") | Some("mjpg") | Some("jpeg") => Some("mjpeg"),
        _ => {
            if ext == "h265" || ext == "hevc" {
                Some("hevc")
            } else if ext == "h264" || ext == "avc" {
                Some("h264")
            } else if ext == "mjpeg" || ext == "mjpg" || ext == "jpeg" {
                Some("mjpeg")
            } else {
                None
            }
        }
    }
}

pub(super) fn requested_range(headers: &HeaderMap) -> Option<String> {
    headers.get(header::RANGE).and_then(|value| value.to_str().ok()).map(str::trim).filter(|value| !value.is_empty()).map(ToOwned::to_owned)
}

enum MediaRange {
    Full,
    Partial { start: u64, end: u64 },
}

fn parse_media_range(range_header: Option<&str>, len: u64) -> Result<MediaRange, ()> {
    let Some(raw_header) = range_header else {
        return Ok(MediaRange::Full);
    };
    if len == 0 {
        return Err(());
    }
    let raw_header = raw_header.trim();
    let Some(spec) = raw_header.strip_prefix("bytes=") else {
        return Err(());
    };
    if spec.contains(',') {
        return Err(());
    }
    let Some((start_text, end_text)) = spec.split_once('-') else {
        return Err(());
    };
    let start_text = start_text.trim();
    let end_text = end_text.trim();
    if start_text.is_empty() {
        let suffix_len = end_text.parse::<u64>().map_err(|_| ())?;
        if suffix_len == 0 {
            return Err(());
        }
        let clamped = suffix_len.min(len);
        let start = len - clamped;
        return Ok(MediaRange::Partial { start, end: len - 1 });
    }

    let start = start_text.parse::<u64>().map_err(|_| ())?;
    if start >= len {
        return Err(());
    }
    let end = if end_text.is_empty() {
        len - 1
    } else {
        let parsed_end = end_text.parse::<u64>().map_err(|_| ())?;
        if parsed_end < start {
            return Err(());
        }
        parsed_end.min(len - 1)
    };
    Ok(MediaRange::Partial { start, end })
}

pub(super) async fn stream_media_file(mut file: fs::File, len: u64, content_type: &str, range_header: Option<&str>) -> ApiResult<Response> {
    let range = parse_media_range(range_header, len);
    if range.is_err() {
        return Response::builder()
            .status(StatusCode::RANGE_NOT_SATISFIABLE)
            .header(header::CACHE_CONTROL, "no-store")
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CONTENT_RANGE, format!("bytes */{len}"))
            .body(Body::empty())
            .map_err(|err| ApiError::internal(format!("failed to build range response: {err}")));
    }
    match range.unwrap_or(MediaRange::Full) {
        MediaRange::Full => {
            let stream = ReaderStream::new(file);
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, content_type)
                .header(header::CACHE_CONTROL, "no-store")
                .header(header::ACCEPT_RANGES, "bytes")
                .header(header::CONTENT_LENGTH, len.to_string())
                .body(Body::from_stream(stream))
                .map_err(|err| ApiError::internal(format!("failed to stream media: {err}")))
        }
        MediaRange::Partial { start, end } => {
            file.seek(SeekFrom::Start(start)).await.map_err(|err| map_io_error(err, "failed to seek media file"))?;
            let chunk_len = end.saturating_sub(start) + 1;
            let stream = ReaderStream::new(file.take(chunk_len));
            Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header(header::CONTENT_TYPE, content_type)
                .header(header::CACHE_CONTROL, "no-store")
                .header(header::ACCEPT_RANGES, "bytes")
                .header(header::CONTENT_LENGTH, chunk_len.to_string())
                .header(header::CONTENT_RANGE, format!("bytes {start}-{end}/{len}"))
                .body(Body::from_stream(stream))
                .map_err(|err| ApiError::internal(format!("failed to stream media range: {err}")))
        }
    }
}
