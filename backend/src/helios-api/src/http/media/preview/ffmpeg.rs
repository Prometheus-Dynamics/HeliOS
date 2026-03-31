use std::path::{Path, PathBuf};
use std::process::Command;

use tokio::fs;

use super::metadata::normalize_video_codec;

pub(crate) fn media_preview_cache_path(meta_dir: &Path, filename: &str, fps: Option<f32>) -> PathBuf {
    let fps_tag = fps.filter(|value| value.is_finite() && *value > 0.0).map(|value| format!("{value:.3}").replace('.', "_")).unwrap_or_else(|| "auto".to_string());
    meta_dir.join(format!("{filename}.preview.h264.{fps_tag}.mp4"))
}

pub(crate) fn media_thumbnail_cache_path(meta_dir: &Path, filename: &str) -> PathBuf {
    meta_dir.join(format!("{filename}.thumb.jpg"))
}

pub(crate) async fn preview_cache_fresh(source: &Path, preview: &Path) -> Result<bool, std::io::Error> {
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

pub(crate) async fn transcode_preview_h264(source: &Path, output: &Path, fps_hint: Option<f32>, codec_hint: Option<&str>) -> Result<(), String> {
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

pub(crate) async fn render_video_thumbnail_jpeg(source: &Path, output: &Path, codec_hint: Option<&str>) -> Result<(), String> {
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
