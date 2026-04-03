use std::path::{Path as StdPath, PathBuf};

use tokio::fs;
use uuid::Uuid;

use crate::http::media::{load_named_media_metadata, write_media_metadata};
use crate::http::storage;

use super::state::ActiveRecordingSession;

pub(super) fn imu_sidecar_file_name(media_name: &str) -> String {
    format!("{media_name}.imu.jsonl.gz")
}

pub(super) fn frame_timestamps_file_name(media_name: &str) -> String {
    format!("{media_name}.frame_ts.txt")
}

pub(super) async fn media_meta_dir_async() -> Result<PathBuf, String> {
    storage::ensure_subdir_async("media-meta").await.map_err(|err| format!("failed to prepare media metadata directory: {err}"))
}

pub(super) async fn update_media_recording_fps_from_frame_ts(name: &str) -> Result<(), String> {
    let Some(base) = storage::sanitize_name(name) else {
        return Err("invalid media name".to_string());
    };
    let mut metadata = load_named_media_metadata(&base).await.unwrap_or_default();
    let Some(frame_ts_name) = metadata.frame_timestamps_file_name.as_deref().and_then(storage::sanitize_name) else {
        return Ok(());
    };
    let meta_dir = media_meta_dir_async().await?;
    let media_dir = storage::ensure_subdir_async("media").await.map_err(|err| format!("failed to access media storage: {err}"))?;
    let candidates = [meta_dir.join(&frame_ts_name), media_dir.join(&frame_ts_name)];
    let mut derived_fps = None;
    for candidate in candidates {
        if let Some(fps) = derive_fps_from_frame_ts_file(&candidate).await {
            derived_fps = Some(fps);
            break;
        }
    }
    if let Some(fps) = derived_fps {
        metadata.fps = Some(fps);
        write_media_metadata(&base, metadata).await.map_err(|err| err.to_string())?;
    }
    Ok(())
}

async fn derive_fps_from_frame_ts_file(path: &StdPath) -> Option<f32> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let file = std::fs::File::open(path).ok()?;
        let reader = std::io::BufRead::lines(std::io::BufReader::new(file));
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

pub(super) async fn update_media_imu_sidecar(name: &str, sidecar_name: Option<String>, samples: Option<u64>) -> Result<(), String> {
    let Some(base) = storage::sanitize_name(name) else {
        return Err("invalid media name".to_string());
    };
    let mut metadata = load_named_media_metadata(&base).await.unwrap_or_default();
    metadata.imu_data_file_name = sidecar_name;
    metadata.imu_data_samples = samples;
    write_media_metadata(&base, metadata).await.map_err(|err| err.to_string())
}

pub(super) async fn clear_media_imu_sidecar(name: &str) {
    if let Err(err) = update_media_imu_sidecar(name, None, None).await {
        tracing::warn!(media_name = %name, error = %err, "failed to clear IMU sidecar metadata");
    }
}

pub(super) async fn cleanup_failed_recording_artifacts(session: &ActiveRecordingSession) -> Result<(), String> {
    let _ = fs::remove_file(&session.output_path).await;
    let Some(base) = storage::sanitize_name(&session.media_name) else {
        return Err("invalid media name".to_string());
    };
    let meta_dir = storage::ensure_subdir_async("media-meta").await.map_err(|err| format!("failed to prepare media metadata directory: {err}"))?;
    let _ = fs::remove_file(meta_dir.join(format!("{base}.json"))).await;
    let _ = fs::remove_file(meta_dir.join(imu_sidecar_file_name(&base))).await;
    let _ = fs::remove_file(session.output_path.with_file_name(frame_timestamps_file_name(&base))).await;
    Ok(())
}

pub(super) fn temp_output_path(output_path: &StdPath) -> PathBuf {
    let temp_ext = output_path.extension().and_then(|ext| ext.to_str()).filter(|ext| !ext.is_empty()).map(|ext| format!("{ext}.part")).unwrap_or_else(|| "part".to_string());
    output_path.with_extension(temp_ext)
}

pub(super) fn recording_extension(container: helios_engine::ipc::RecordingContainer, codec: helios_engine::ipc::RecordingCodec) -> &'static str {
    match container {
        helios_engine::ipc::RecordingContainer::Mp4 => "mp4",
        helios_engine::ipc::RecordingContainer::Raw => match codec {
            helios_engine::ipc::RecordingCodec::H264 => "h264",
            helios_engine::ipc::RecordingCodec::H265 => "h265",
        },
    }
}

pub(super) fn content_type_for_extension(ext: &str) -> String {
    match ext {
        "mp4" => "video/mp4".to_string(),
        "h264" => "video/h264".to_string(),
        "h265" => "video/h265".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}

pub(super) fn ensure_extension(base: &str, ext: &str) -> Option<String> {
    let trimmed = base.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.to_ascii_lowercase().ends_with(&format!(".{ext}")) {
        return Some(trimmed.to_string());
    }
    let stem = trimmed.rsplit_once('.').map(|(stem, _)| stem).unwrap_or(trimmed);
    Some(format!("{stem}.{ext}"))
}

pub(super) async fn unique_media_name(dir: &StdPath, filename: &str) -> Result<String, std::io::Error> {
    let candidate = filename.to_string();
    if !tokio::fs::try_exists(dir.join(&candidate)).await.unwrap_or(false) {
        return Ok(candidate);
    }

    let suffix = Uuid::new_v4().simple().to_string();
    let mut attempts = 0;
    loop {
        attempts += 1;
        let next = append_suffix(filename, &suffix[..8], attempts);
        if !tokio::fs::try_exists(dir.join(&next)).await.unwrap_or(false) {
            return Ok(next);
        }
        if attempts > 5 {
            return Ok(format!("{}-{}", suffix, filename));
        }
    }
}

pub(super) fn append_suffix(filename: &str, suffix: &str, attempt: usize) -> String {
    let suffix = if attempt <= 1 { suffix.to_string() } else { format!("{suffix}-{attempt}") };
    if let Some(stripped) = filename.strip_suffix(".tar.gz") {
        return format!("{stripped}-{suffix}.tar.gz");
    }
    if let Some((stem, ext)) = filename.rsplit_once('.') {
        return format!("{stem}-{suffix}.{ext}");
    }
    format!("{filename}-{suffix}")
}
