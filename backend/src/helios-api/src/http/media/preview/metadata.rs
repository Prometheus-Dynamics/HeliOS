use serde::Deserialize;
use std::{
    io::BufReader,
    path::{Path, PathBuf},
    process::Command,
};
use tokio::fs;

use crate::http::{
    media::MediaMetadata,
    storage::{self, sanitize_name},
};

pub(crate) async fn hydrate_dimensions(meta: &mut MediaMetadata, path: &Path, content_type: &str) {
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

pub(crate) async fn hydrate_video_metadata(meta: &mut MediaMetadata, path: &Path, filename: &str, content_type: &str) {
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

pub(crate) fn parse_ffprobe_ratio(raw: &str) -> Option<f32> {
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

pub(crate) fn normalize_video_codec(raw: &str) -> Option<String> {
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

pub(crate) async fn preview_fps_hint(meta_dir: &Path, meta: &MediaMetadata) -> Option<f32> {
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
