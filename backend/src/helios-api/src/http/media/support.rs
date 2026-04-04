use chrono::Utc;
use flate2::read::GzDecoder;
use lib_runtime_policy::HELIOS_API_UPLOAD_POLICY;
use lib_schema_migration::{SyncSchemaPlan, normalize_to_current};
use mime_guess::MimeGuess;
use std::io::{BufRead, BufReader, Cursor};
use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::warn;

use crate::http::{
    error::ApiError,
    storage::{self, sanitize_name},
};

use super::{
    models::{guess_model_format, hydrate_model_metadata, looks_like_model, needs_model_affinity_tags, register_ai_model_for_media},
    preview::{hydrate_dimensions, hydrate_video_metadata},
    types::MediaMetadata,
};

const CURRENT_MEDIA_METADATA_SCHEMA_VERSION: u32 = 1;

const MEDIA_METADATA_SCHEMA_PLAN: SyncSchemaPlan<serde_json::Value> = SyncSchemaPlan::strict("media metadata", CURRENT_MEDIA_METADATA_SCHEMA_VERSION);

pub(super) fn is_internal_media_artifact(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".frame_ts.txt")
}

#[allow(clippy::result_large_err)]
pub(super) fn media_dir() -> Result<PathBuf, ApiError> {
    storage::ensure_subdir("media").map_err(|err| map_io_error(err, "failed to prepare media directory"))
}

#[allow(clippy::result_large_err)]
pub(super) fn media_meta_dir() -> Result<PathBuf, ApiError> {
    storage::ensure_subdir("media-meta").map_err(|err| map_io_error(err, "failed to prepare media metadata directory"))
}

fn decode_media_metadata(bytes: &[u8]) -> Option<MediaMetadata> {
    let raw = serde_json::from_slice::<serde_json::Value>(bytes).ok()?;
    let migrated = normalize_to_current(raw, &MEDIA_METADATA_SCHEMA_PLAN).ok()?;
    serde_json::from_value(migrated).ok()
}

pub(crate) async fn load_media_metadata(dir: &Path, name: &str) -> Option<MediaMetadata> {
    let path = dir.join(format!("{name}.json"));
    let bytes = fs::read(&path).await.ok()?;
    decode_media_metadata(&bytes)
}

pub(crate) async fn load_named_media_metadata(name: &str) -> Option<MediaMetadata> {
    let base = sanitize_name(name)?;
    let meta_dir = media_meta_dir().ok()?;
    load_media_metadata(&meta_dir, &base).await
}

pub(crate) async fn write_media_metadata(name: &str, metadata: MediaMetadata) -> Result<(), ApiError> {
    let Some(base) = sanitize_name(name) else {
        return Err(ApiError::bad_request("invalid media name"));
    };
    let meta_dir = media_meta_dir()?;
    let path = meta_dir.join(format!("{base}.json"));
    let mut metadata = metadata;
    metadata.schema_version = CURRENT_MEDIA_METADATA_SCHEMA_VERSION;
    let bytes = serde_json::to_vec(&metadata).map_err(|err| ApiError::bad_request(format!("invalid metadata: {err}")))?;
    fs::write(&path, bytes).await.map_err(|err| map_io_error(err, "failed to write media metadata"))?;
    Ok(())
}

pub(super) async fn ensure_media_metadata(meta_dir: &Path, filename: &str, path: &Path, content_type: &str) -> Option<MediaMetadata> {
    let mut md = load_media_metadata(meta_dir, filename).await.unwrap_or_default();
    if md.captured_at_ms.is_none() {
        if let Ok(meta) = fs::metadata(path).await {
            let modified_ms = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as i64);
            md.captured_at_ms = modified_ms.or_else(|| Some(Utc::now().timestamp_millis()));
        } else {
            md.captured_at_ms = Some(Utc::now().timestamp_millis());
        }
    }
    hydrate_dimensions(&mut md, path, content_type).await;
    hydrate_video_metadata(&mut md, path, filename, content_type).await;
    if md.label_file_name.is_none() {
        let label_path = meta_dir.join(format!("{filename}.label"));
        if fs::try_exists(&label_path).await.unwrap_or(false) {
            md.label_file_name = Some("labels.txt".to_string());
        }
    }
    if looks_like_model(filename, content_type)
        && (md.model_tensor_spec.is_none() || md.model_input_resolution.is_none() || needs_model_affinity_tags(&md.tags))
        && let Some(format) = guess_model_format(filename)
    {
        hydrate_model_metadata(&mut md, path, format).await;
    }
    if md.model_id.is_none() && looks_like_model(filename, content_type) {
        let _ = register_ai_model_for_media(filename, path, content_type, &mut md).await;
    }
    let _ = write_media_metadata(filename, md.clone()).await;
    Some(md)
}

pub(super) async fn store_label_bytes(meta_dir: &Path, filename: &str, bytes: &[u8]) -> Result<PathBuf, ApiError> {
    let path = meta_dir.join(format!("{filename}.label"));
    fs::write(&path, bytes).await.map_err(|err| map_io_error(err, "failed to write label file"))?;
    Ok(path)
}

pub(super) async fn load_media_label_bytes(filename: &str) -> Result<Option<Vec<u8>>, ApiError> {
    let meta_dir = match media_meta_dir() {
        Ok(dir) => dir,
        Err(err) => {
            warn!(error = ?err, "failed to resolve media metadata directory for labels");
            return Ok(None);
        }
    };
    let label_path = meta_dir.join(format!("{filename}.label"));
    if !fs::try_exists(&label_path).await.unwrap_or(false) {
        return Ok(None);
    }
    let bytes = fs::read(&label_path).await.map_err(|err| map_io_error(err, "failed to read media label file"))?;
    if bytes.is_empty() { Ok(None) } else { Ok(Some(bytes)) }
}

pub(super) fn count_imu_sidecar_samples(sidecar_name: &str, bytes: &[u8]) -> Result<u64, String> {
    let gz_magic = bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b;
    let gz_hint = sidecar_name.to_ascii_lowercase().ends_with(".gz");
    if gz_magic || gz_hint {
        let decoder = GzDecoder::new(Cursor::new(bytes));
        let reader = BufReader::new(decoder);
        return count_imu_jsonl_lines(reader);
    }
    count_imu_jsonl_lines(BufReader::new(Cursor::new(bytes)))
}

fn count_imu_jsonl_lines<R: BufRead>(reader: R) -> Result<u64, String> {
    let mut count: u64 = 0;
    for line in reader.lines() {
        let line = line.map_err(|err| format!("failed to read IMU sidecar: {err}"))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        serde_json::from_str::<serde_json::Value>(trimmed).map_err(|err| format!("invalid IMU sidecar JSONL: {err}"))?;
        count = count.saturating_add(1);
    }
    Ok(count)
}

pub(super) fn parse_tags(raw: &str) -> Vec<String> {
    raw.split(',').map(|value| value.trim().to_string()).filter(|value| !value.is_empty()).collect()
}

pub(super) fn map_io_error(err: std::io::Error, context: &str) -> ApiError {
    let status = if err.kind() == std::io::ErrorKind::NotFound { axum::http::StatusCode::NOT_FOUND } else { axum::http::StatusCode::INTERNAL_SERVER_ERROR };
    let code = if status == axum::http::StatusCode::NOT_FOUND { "not_found" } else { "internal" };
    ApiError::new(status, code, format!("{context}: {err}"))
}

pub(super) fn guess_content_type(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".h264") || lower.ends_with(".avc") {
        return "video/h264".to_string();
    }
    if lower.ends_with(".h265") || lower.ends_with(".hevc") {
        return "video/h265".to_string();
    }
    let guess: MimeGuess = mime_guess::from_path(name);
    guess.first_or_octet_stream().essence_str().to_string()
}

pub(super) fn max_upload_bytes() -> u64 {
    HELIOS_API_UPLOAD_POLICY.resolve().media_upload_bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn decode_media_metadata_rejects_missing_schema_version() {
        let raw = serde_json::json!({
            "stream_id": Uuid::nil(),
            "kind": "recording",
            "captured_at_ms": 123
        });

        let parsed = decode_media_metadata(&serde_json::to_vec(&raw).expect("encode"));
        assert!(parsed.is_none());
    }
}
