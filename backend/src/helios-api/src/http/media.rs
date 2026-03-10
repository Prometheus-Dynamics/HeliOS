use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
};
use chrono::Utc;
use flate2::read::GzDecoder;
use helios_peripherals::AiModelHealth;
use image::GenericImageView;
use lib_ai::model::{ModelFormat, ModelId, ModelMetadata};
use lib_ipc::types::Timestamp;
use mime_guess::MimeGuess;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Cursor, Read, Seek, SeekFrom, Write};
use std::path::Path as StdPath;
use std::process::Command;
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
};
use tokio_util::io::ReaderStream;
use tracing::warn;
use utoipa::ToSchema;
use uuid::Uuid;
use zip::write::SimpleFileOptions;

use super::AppState;
use super::error::{ApiError, ApiResult, ErrorBody};
use super::storage::{self, sanitize_name};
use super::upload_integrity;

pub fn router() -> Router<AppState> {
    // Media uploads frequently exceed Axum's default 2MB body limit; handle limits ourselves.
    Router::new()
        .route("/", get(list_media).post(upload_media))
        .route("/download.zip", get(download_media_archive))
        .route("/{name}/preview", get(fetch_media_preview))
        .route("/{name}/thumbnail", get(fetch_media_thumbnail))
        .route("/{name}/imu", get(fetch_media_imu).post(attach_media_imu).delete(delete_media_imu))
        .route("/{name}", get(fetch_media).delete(delete_media))
        .route("/{name}/metadata", patch(update_metadata))
        .route("/{name}/label", get(fetch_label).post(attach_label))
        .route("/{name}/image/edits", post(apply_image_edits))
        .route_layer(DefaultBodyLimit::disable())
}

#[derive(Debug, Serialize, ToSchema)]
pub struct MediaItem {
    pub name: String,
    pub size_bytes: u64,
    pub content_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub captured_at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_codec: Option<String>,
    #[serde(default)]
    pub label_attached: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label_file_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_input_resolution: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_tensor_spec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imu_data_file_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub imu_data_samples: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ListMediaParams {
    #[serde(default)]
    pub stream_id: Option<Uuid>,
    #[serde(default)]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DownloadMediaArchiveParams {
    #[serde(default)]
    pub name: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct MediaMetadata {
    #[serde(default)]
    pub(crate) stream_id: Option<Uuid>,
    #[serde(default)]
    pub(crate) kind: Option<String>,
    #[serde(default)]
    pub(crate) captured_at_ms: Option<i64>,
    #[serde(default)]
    pub(crate) description: Option<String>,
    #[serde(default)]
    pub(crate) tags: Vec<String>,
    #[serde(default)]
    pub(crate) width: Option<u32>,
    #[serde(default)]
    pub(crate) height: Option<u32>,
    #[serde(default)]
    pub(crate) fps: Option<f32>,
    #[serde(default)]
    pub(crate) video_codec: Option<String>,
    #[serde(default)]
    pub(crate) label_file_name: Option<String>,
    #[serde(default)]
    pub(crate) model_id: Option<Uuid>,
    #[serde(default)]
    pub(crate) model_input_resolution: Option<String>,
    #[serde(default)]
    pub(crate) model_tensor_spec: Option<String>,
    #[serde(default)]
    pub(crate) imu_data_file_name: Option<String>,
    #[serde(default)]
    pub(crate) imu_data_samples: Option<u64>,
    #[serde(default)]
    pub(crate) frame_timestamps_file_name: Option<String>,
}

#[utoipa::path(
    get,
    path = "/media",
    tag = "Media",
    responses((status = 200, description = "List stored media assets", body = [MediaItem]), (status = 500, description = "Storage error", body = ErrorBody))
)]
async fn list_media(Query(params): Query<ListMediaParams>) -> ApiResult<impl IntoResponse> {
    let dir = media_dir()?;
    let meta_dir = media_meta_dir()?;

    let mut entries = match fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(err) => return Err(map_io_error(err, "failed to read media directory")),
    };
    let mut media = Vec::new();
    loop {
        let entry = match entries.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(err) => return Err(map_io_error(err, "failed to read media entry")),
        };

        let meta = match entry.metadata().await {
            Ok(meta) if meta.is_file() => meta,
            Ok(_) => continue,
            Err(err) => return Err(map_io_error(err, "failed to stat media file")),
        };
        let name = entry.file_name().to_string_lossy().to_string();
        if is_internal_media_artifact(&name) {
            continue;
        }
        let content_type = guess_content_type(&name);
        let md = ensure_media_metadata(&meta_dir, &name, &entry.path(), &content_type).await.unwrap_or_default();
        if let Some(filter_id) = params.stream_id
            && md.stream_id != Some(filter_id)
        {
            continue;
        }
        if let Some(kind) = params.kind.as_deref().map(str::trim).filter(|v| !v.is_empty())
            && md.kind.as_deref() != Some(kind)
        {
            continue;
        }
        media.push(MediaItem {
            name,
            size_bytes: meta.len(),
            content_type,
            description: md.description,
            tags: md.tags,
            stream_id: md.stream_id,
            kind: md.kind,
            captured_at_ms: md.captured_at_ms,
            width: md.width,
            height: md.height,
            fps: md.fps,
            video_codec: md.video_codec,
            label_attached: md.label_file_name.is_some(),
            label_file_name: md.label_file_name,
            model_id: md.model_id,
            model_input_resolution: md.model_input_resolution,
            model_tensor_spec: md.model_tensor_spec,
            imu_data_file_name: md.imu_data_file_name,
            imu_data_samples: md.imu_data_samples,
        });
    }

    Ok(Json(media))
}

#[utoipa::path(
    get,
    path = "/media/download.zip",
    tag = "Media",
    params(("name" = Vec<String>, Query, description = "Media file names to include in the zip archive")),
    responses(
        (status = 200, description = "Zip archive containing selected media assets"),
        (status = 400, description = "Invalid request", body = ErrorBody),
        (status = 404, description = "Missing media asset", body = ErrorBody)
    )
)]
async fn download_media_archive(Query(params): Query<DownloadMediaArchiveParams>) -> ApiResult<Response> {
    if params.name.is_empty() {
        return Err(ApiError::bad_request("select at least one media file to download"));
    }
    if params.name.len() > 512 {
        return Err(ApiError::bad_request("too many files requested in one archive"));
    }

    let mut seen = HashSet::new();
    let mut names: Vec<String> = Vec::with_capacity(params.name.len());
    for raw in params.name {
        let Some(name) = sanitize_name(raw.trim()) else {
            return Err(ApiError::bad_request("invalid media name"));
        };
        if name.is_empty() || is_internal_media_artifact(&name) {
            return Err(ApiError::bad_request("invalid media name"));
        }
        if seen.insert(name.clone()) {
            names.push(name);
        }
    }
    if names.is_empty() {
        return Err(ApiError::bad_request("select at least one media file to download"));
    }

    let dir = media_dir()?;
    let mut entries: Vec<(String, std::path::PathBuf)> = Vec::with_capacity(names.len());
    for name in names {
        let path = dir.join(&name);
        let meta = fs::metadata(&path)
            .await
            .map_err(|err| if err.kind() == std::io::ErrorKind::NotFound { ApiError::not_found(format!("media not found: {name}")) } else { map_io_error(err, "failed to read media file") })?;
        if !meta.is_file() {
            return Err(ApiError::bad_request(format!("media is not a file: {name}")));
        }
        entries.push((name, path));
    }

    let archive_file =
        tokio::task::spawn_blocking(move || build_media_archive_file(entries)).await.map_err(|err| ApiError::internal(format!("archive build task failed: {err}")))?.map_err(ApiError::internal)?;

    let archive_len = archive_file.metadata().map_err(|err| ApiError::internal(format!("failed to stat archive: {err}")))?.len();
    let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
    let archive_name = format!("media-selection-{timestamp}.zip");
    let stream = ReaderStream::new(fs::File::from_std(archive_file));
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/zip")
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::CONTENT_LENGTH, archive_len.to_string())
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{archive_name}\""))
        .body(Body::from_stream(stream))
        .map_err(|err| ApiError::internal(format!("failed to build archive response: {err}")))
}

fn is_internal_media_artifact(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".frame_ts.txt")
}

#[utoipa::path(
    post,
    path = "/media",
    tag = "Media",
    request_body(content = String, description = "Multipart form-data with exactly one file part (other fields are ignored)"),
    responses(
        (status = 201, description = "Media uploaded", body = MediaItem),
        (status = 400, description = "Invalid upload", body = ErrorBody),
        (status = 500, description = "Storage error", body = ErrorBody)
    )
)]
async fn upload_media(headers: HeaderMap, mut multipart: Multipart) -> ApiResult<impl IntoResponse> {
    let dir = media_dir()?;
    let meta_dir = media_meta_dir()?;

    let max_bytes = max_upload_bytes();
    let expected_upload_bytes = upload_integrity::expected_upload_bytes(&headers).map_err(ApiError::bad_request)?;
    let mut uploaded: Option<(String, u64, String, MediaMetadata)> = None;
    let mut pending_label: Option<(String, Vec<u8>)> = None;
    let mut pending_meta = MediaMetadata::default();
    let mut pending_model_format: Option<lib_ai::ModelFormat> = None;

    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(err) => return Err(ApiError::bad_request(format!("failed to read upload payload: {err}"))),
        };

        let field_name = field.name().unwrap_or_default().to_string();
        let file_name = field.file_name().and_then(sanitize_name);
        if file_name.is_none() {
            let value = field.text().await.unwrap_or_default();
            match field_name.as_str() {
                "kind" => pending_meta.kind = value.trim().to_string().into(),
                "description" => pending_meta.description = value.trim().to_string().into(),
                "tags" => pending_meta.tags = parse_tags(&value),
                "modelInputResolution" => pending_meta.model_input_resolution = value.trim().to_string().into(),
                "modelTensorSpec" => pending_meta.model_tensor_spec = value.trim().to_string().into(),
                _ => {}
            }
            continue;
        }

        let filename = file_name.unwrap();

        if field_name == "label" {
            let bytes = field.bytes().await.map_err(|err| ApiError::bad_request(format!("failed to read label bytes: {err}")))?;
            if bytes.len() as u64 > max_bytes.min(16 * 1024 * 1024) {
                return Err(ApiError::payload_too_large("label file exceeds limit"));
            }
            pending_label = Some((filename, bytes.to_vec()));
            continue;
        }

        if uploaded.is_some() {
            return Err(ApiError::bad_request("only one media file may be uploaded per request"));
        }

        let path = dir.join(&filename);
        let mut file = match fs::File::create(&path).await {
            Ok(file) => file,
            Err(err) => return Err(map_io_error(err, "failed to create media file")),
        };

        let mut written: u64 = 0;
        let mut field = field;
        while let Some(chunk) = match field.chunk().await {
            Ok(chunk) => chunk,
            Err(err) => {
                let _ = fs::remove_file(&path).await;
                return Err(ApiError::bad_request(format!("failed to read upload bytes: {err}")));
            }
        } {
            if chunk.is_empty() {
                continue;
            }
            written += chunk.len() as u64;
            if written > max_bytes {
                let _ = fs::remove_file(&path).await;
                return Err(ApiError::payload_too_large(format!("upload exceeds limit of {} bytes", max_bytes)));
            }
            if let Err(err) = file.write_all(&chunk).await {
                let _ = fs::remove_file(&path).await;
                return Err(map_io_error(err, "failed to write media file"));
            }
        }

        if written == 0 {
            let _ = fs::remove_file(&path).await;
            return Err(ApiError::bad_request("empty upload"));
        }
        if let Err(err) = upload_integrity::validate_expected_upload_bytes(written, expected_upload_bytes) {
            let _ = fs::remove_file(&path).await;
            return Err(ApiError::bad_request(err));
        }
        let stored_bytes = match upload_integrity::finalize_file_upload(&mut file, &path, written).await {
            Ok(stored_bytes) => stored_bytes,
            Err(err) => {
                let _ = fs::remove_file(&path).await;
                return Err(map_io_error(err, "failed to finalize media upload"));
            }
        };
        if let Err(err) = upload_integrity::validate_expected_upload_bytes(stored_bytes, expected_upload_bytes) {
            let _ = fs::remove_file(&path).await;
            return Err(ApiError::bad_request(err));
        }

        let content_type = guess_content_type(&filename);
        hydrate_dimensions(&mut pending_meta, &path, &content_type).await;
        hydrate_video_metadata(&mut pending_meta, &path, &filename, &content_type).await;
        if looks_like_model(&filename, &content_type) {
            pending_model_format = guess_model_format(&filename);
        }
        uploaded = Some((filename, stored_bytes, content_type, pending_meta.clone()));
    }

    match uploaded {
        Some((filename, written, content_type, mut meta)) => {
            if meta.captured_at_ms.is_none() {
                meta.captured_at_ms = Some(Utc::now().timestamp_millis());
            }
            if let Some((label_name, label_bytes)) = pending_label.take() {
                store_label_bytes(&meta_dir, &filename, &label_bytes).await?;
                meta.label_file_name = Some(label_name);
            }

            if (meta.model_tensor_spec.is_none() || meta.model_input_resolution.is_none())
                && let Some(format) = pending_model_format
            {
                hydrate_model_metadata(&mut meta, &dir.join(&filename), format).await;
            }

            if looks_like_model(&filename, &content_type) {
                register_ai_model_for_media(&filename, &dir.join(&filename), &content_type, &mut meta).await?;
            }

            write_media_metadata(&filename, meta.clone()).await?;
            Ok((
                StatusCode::CREATED,
                Json(MediaItem {
                    name: filename,
                    size_bytes: written,
                    content_type,
                    description: meta.description,
                    tags: meta.tags,
                    stream_id: meta.stream_id,
                    kind: meta.kind,
                    captured_at_ms: meta.captured_at_ms,
                    width: meta.width,
                    height: meta.height,
                    fps: meta.fps,
                    video_codec: meta.video_codec,
                    label_attached: meta.label_file_name.is_some(),
                    label_file_name: meta.label_file_name,
                    model_id: meta.model_id,
                    model_input_resolution: meta.model_input_resolution,
                    model_tensor_spec: meta.model_tensor_spec,
                    imu_data_file_name: meta.imu_data_file_name,
                    imu_data_samples: meta.imu_data_samples,
                }),
            ))
        }
        None => Err(ApiError::bad_request("multipart payload missing file part")),
    }
}

#[utoipa::path(
    get,
    path = "/media/{name}",
    tag = "Media",
    params(("name" = String, Path, description = "Media file name")),
    responses((status = 200, description = "Media content"), (status = 404, description = "Not found", body = ErrorBody))
)]
async fn fetch_media(Path(name): Path<String>, headers: HeaderMap) -> ApiResult<impl IntoResponse> {
    let Some(filename) = sanitize_name(&name) else {
        return Err(ApiError::bad_request("invalid media name"));
    };
    let dir = media_dir()?;
    let path = dir.join(&filename);

    let range = requested_range(&headers);
    let file = match fs::File::open(&path).await {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Err(ApiError::not_found("media not found")),
        Err(err) => return Err(map_io_error(err, "failed to read media file")),
    };
    let metadata = match file.metadata().await {
        Ok(meta) => meta,
        Err(err) => return Err(map_io_error(err, "failed to stat media file")),
    };

    let content_type = guess_content_type(path.file_name().and_then(|n| n.to_str()).unwrap_or_default());
    stream_media_file(file, metadata.len(), &content_type, range.as_deref()).await
}

#[utoipa::path(
    get,
    path = "/media/{name}/imu",
    tag = "Media",
    params(("name" = String, Path, description = "Media file name")),
    responses((status = 200, description = "IMU sidecar stream"), (status = 404, description = "Not found", body = ErrorBody))
)]
async fn fetch_media_imu(Path(name): Path<String>, headers: HeaderMap) -> ApiResult<impl IntoResponse> {
    let Some(filename) = sanitize_name(&name) else {
        return Err(ApiError::bad_request("invalid media name"));
    };
    let meta_dir = media_meta_dir()?;
    let Some(metadata) = load_media_metadata(&meta_dir, &filename).await else {
        return Err(ApiError::not_found("IMU sidecar not found"));
    };
    let Some(sidecar_name) = metadata.imu_data_file_name.and_then(|value| sanitize_name(&value)) else {
        return Err(ApiError::not_found("IMU sidecar not found"));
    };
    let sidecar_path = meta_dir.join(sidecar_name);
    let range = requested_range(&headers);
    let file = match fs::File::open(&sidecar_path).await {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Err(ApiError::not_found("IMU sidecar not found")),
        Err(err) => return Err(map_io_error(err, "failed to read IMU sidecar")),
    };
    let metadata = file.metadata().await.map_err(|err| map_io_error(err, "failed to stat IMU sidecar"))?;
    stream_media_file(file, metadata.len(), "application/x-ndjson+gzip", range.as_deref()).await
}

#[utoipa::path(
    post,
    path = "/media/{name}/imu",
    tag = "Media",
    params(("name" = String, Path, description = "Media file name")),
    request_body(content = String, description = "Multipart form-data with an IMU sidecar file part named 'imu'"),
    responses((status = 200, description = "IMU sidecar attached", body = MediaItem), (status = 400, description = "Invalid upload", body = ErrorBody))
)]
async fn attach_media_imu(Path(name): Path<String>, headers: HeaderMap, mut multipart: Multipart) -> ApiResult<impl IntoResponse> {
    let Some(filename) = sanitize_name(&name) else {
        return Err(ApiError::bad_request("invalid media name"));
    };
    let dir = media_dir()?;
    let meta_dir = media_meta_dir()?;
    let media_path = dir.join(&filename);
    let meta = fs::metadata(&media_path).await.map_err(|err| map_io_error(err, "failed to stat media file"))?;
    let content_type = guess_content_type(&filename);
    let expected_upload_bytes = upload_integrity::expected_upload_bytes(&headers).map_err(ApiError::bad_request)?;

    let mut sidecar_bytes: Option<Vec<u8>> = None;
    let mut uploaded_name: Option<String> = None;
    while let Some(field) = multipart.next_field().await.map_err(|err| ApiError::bad_request(format!("failed to read multipart: {err}")))? {
        if field.name().unwrap_or_default() != "imu" {
            continue;
        }
        uploaded_name = field.file_name().and_then(sanitize_name);
        let bytes = field.bytes().await.map_err(|err| ApiError::bad_request(format!("failed to read IMU sidecar bytes: {err}")))?;
        upload_integrity::validate_expected_upload_bytes(bytes.len() as u64, expected_upload_bytes).map_err(ApiError::bad_request)?;
        sidecar_bytes = Some(bytes.to_vec());
        break;
    }

    let Some(sidecar_bytes) = sidecar_bytes else {
        return Err(ApiError::bad_request("multipart payload missing imu part"));
    };

    let sidecar_name = uploaded_name.unwrap_or_else(|| format!("{filename}.imu.jsonl.gz"));
    let samples = count_imu_sidecar_samples(&sidecar_name, &sidecar_bytes).map_err(ApiError::bad_request)?;
    fs::write(meta_dir.join(&sidecar_name), &sidecar_bytes).await.map_err(|err| map_io_error(err, "failed to write IMU sidecar"))?;

    let mut md = ensure_media_metadata(&meta_dir, &filename, &media_path, &content_type).await.unwrap_or_default();
    md.imu_data_file_name = Some(sidecar_name);
    md.imu_data_samples = Some(samples);
    write_media_metadata(&filename, md.clone()).await?;

    Ok(Json(MediaItem {
        name: filename,
        size_bytes: meta.len(),
        content_type,
        description: md.description,
        tags: md.tags,
        stream_id: md.stream_id,
        kind: md.kind,
        captured_at_ms: md.captured_at_ms,
        width: md.width,
        height: md.height,
        fps: md.fps,
        video_codec: md.video_codec,
        label_attached: md.label_file_name.is_some(),
        label_file_name: md.label_file_name,
        model_id: md.model_id,
        model_input_resolution: md.model_input_resolution,
        model_tensor_spec: md.model_tensor_spec,
        imu_data_file_name: md.imu_data_file_name,
        imu_data_samples: md.imu_data_samples,
    }))
}

#[utoipa::path(
    delete,
    path = "/media/{name}/imu",
    tag = "Media",
    params(("name" = String, Path, description = "Media file name")),
    responses((status = 200, description = "IMU sidecar detached", body = MediaItem), (status = 404, description = "Not found", body = ErrorBody))
)]
async fn delete_media_imu(Path(name): Path<String>) -> ApiResult<impl IntoResponse> {
    let Some(filename) = sanitize_name(&name) else {
        return Err(ApiError::bad_request("invalid media name"));
    };
    let dir = media_dir()?;
    let meta_dir = media_meta_dir()?;
    let media_path = dir.join(&filename);
    let meta = fs::metadata(&media_path)
        .await
        .map_err(|err| if err.kind() == std::io::ErrorKind::NotFound { ApiError::not_found("media not found") } else { map_io_error(err, "failed to stat media file") })?;
    let content_type = guess_content_type(&filename);

    let mut md = ensure_media_metadata(&meta_dir, &filename, &media_path, &content_type).await.unwrap_or_default();
    if let Some(sidecar_name) = md.imu_data_file_name.as_deref().and_then(sanitize_name) {
        let _ = fs::remove_file(meta_dir.join(sidecar_name)).await;
    }
    md.imu_data_file_name = None;
    md.imu_data_samples = None;
    write_media_metadata(&filename, md.clone()).await?;

    Ok(Json(MediaItem {
        name: filename,
        size_bytes: meta.len(),
        content_type,
        description: md.description,
        tags: md.tags,
        stream_id: md.stream_id,
        kind: md.kind,
        captured_at_ms: md.captured_at_ms,
        width: md.width,
        height: md.height,
        fps: md.fps,
        video_codec: md.video_codec,
        label_attached: md.label_file_name.is_some(),
        label_file_name: md.label_file_name,
        model_id: md.model_id,
        model_input_resolution: md.model_input_resolution,
        model_tensor_spec: md.model_tensor_spec,
        imu_data_file_name: md.imu_data_file_name,
        imu_data_samples: md.imu_data_samples,
    }))
}

fn count_imu_sidecar_samples(sidecar_name: &str, bytes: &[u8]) -> Result<u64, String> {
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

#[utoipa::path(
    get,
    path = "/media/{name}/preview",
    tag = "Media",
    params(("name" = String, Path, description = "Media file name")),
    responses((status = 200, description = "Media preview content"), (status = 404, description = "Not found", body = ErrorBody))
)]
async fn fetch_media_preview(State(state): State<AppState>, Path(name): Path<String>, headers: HeaderMap) -> ApiResult<impl IntoResponse> {
    let Some(filename) = sanitize_name(&name) else {
        return Err(ApiError::bad_request("invalid media name"));
    };
    let dir = media_dir()?;
    let meta_dir = media_meta_dir()?;
    let path = dir.join(&filename);
    let content_type = guess_content_type(&filename);
    let range = requested_range(&headers);
    if !content_type.starts_with("video/") {
        let file = match fs::File::open(&path).await {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Err(ApiError::not_found("media not found")),
            Err(err) => return Err(map_io_error(err, "failed to read media file")),
        };
        let metadata = file.metadata().await.map_err(|err| map_io_error(err, "failed to stat media file"))?;
        return stream_media_file(file, metadata.len(), &content_type, range.as_deref()).await;
    }

    let md = ensure_media_metadata(&meta_dir, &filename, &path, &content_type).await.unwrap_or_default();
    let codec = md.video_codec.as_deref().and_then(normalize_video_codec);
    let is_raw_bitstream = matches!(content_type.as_str(), "video/h264" | "video/h265");
    let should_transcode = is_raw_bitstream || matches!(codec.as_deref(), Some("h265"));
    if !should_transcode {
        let file = match fs::File::open(&path).await {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Err(ApiError::not_found("media not found")),
            Err(err) => return Err(map_io_error(err, "failed to read media file")),
        };
        let metadata = file.metadata().await.map_err(|err| map_io_error(err, "failed to stat media file"))?;
        return stream_media_file(file, metadata.len(), &content_type, range.as_deref()).await;
    }

    let preview_fps = preview_fps_hint(&meta_dir, &md).await;
    let preview_path = media_preview_cache_path(&meta_dir, &filename, preview_fps);
    if !preview_cache_fresh(&path, &preview_path).await.unwrap_or(false) {
        let transcode_lock = state.services.media.preview_generation_lock(&filename).await;
        let _guard = transcode_lock.lock().await;
        if !preview_cache_fresh(&path, &preview_path).await.unwrap_or(false) {
            transcode_preview_h264(&path, &preview_path, preview_fps, codec.as_deref()).await.map_err(|err| ApiError::internal(format!("failed to build video preview: {err}")))?;
        }
    }

    let file = match fs::File::open(&preview_path).await {
        Ok(file) => file,
        Err(err) => return Err(map_io_error(err, "failed to read media preview")),
    };
    let metadata = file.metadata().await.map_err(|err| map_io_error(err, "failed to stat media preview"))?;
    stream_media_file(file, metadata.len(), "video/mp4", range.as_deref()).await
}

#[utoipa::path(
    get,
    path = "/media/{name}/thumbnail",
    tag = "Media",
    params(("name" = String, Path, description = "Media file name")),
    responses((status = 200, description = "Media thumbnail content"), (status = 404, description = "Not found", body = ErrorBody))
)]
async fn fetch_media_thumbnail(State(state): State<AppState>, Path(name): Path<String>) -> ApiResult<impl IntoResponse> {
    let Some(filename) = sanitize_name(&name) else {
        return Err(ApiError::bad_request("invalid media name"));
    };
    let dir = media_dir()?;
    let meta_dir = media_meta_dir()?;
    let path = dir.join(&filename);
    let content_type = guess_content_type(&filename);

    if content_type.starts_with("image/") {
        let file = match fs::File::open(&path).await {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Err(ApiError::not_found("media not found")),
            Err(err) => return Err(map_io_error(err, "failed to read media file")),
        };
        let metadata = file.metadata().await.map_err(|err| map_io_error(err, "failed to stat media file"))?;
        return stream_media_file(file, metadata.len(), &content_type, None).await;
    }
    if !content_type.starts_with("video/") {
        return Err(ApiError::bad_request("thumbnails are supported only for image and video assets"));
    }

    let md = ensure_media_metadata(&meta_dir, &filename, &path, &content_type).await.unwrap_or_default();
    let codec = md.video_codec.as_deref().and_then(normalize_video_codec);
    let thumbnail_path = media_thumbnail_cache_path(&meta_dir, &filename);
    if !preview_cache_fresh(&path, &thumbnail_path).await.unwrap_or(false) {
        let thumbnail_lock = state.services.media.thumbnail_generation_lock(&filename).await;
        let _guard = thumbnail_lock.lock().await;
        if !preview_cache_fresh(&path, &thumbnail_path).await.unwrap_or(false) {
            render_video_thumbnail_jpeg(&path, &thumbnail_path, codec.as_deref()).await.map_err(|err| ApiError::internal(format!("failed to build video thumbnail: {err}")))?;
        }
    }

    let file = match fs::File::open(&thumbnail_path).await {
        Ok(file) => file,
        Err(err) => return Err(map_io_error(err, "failed to read media thumbnail")),
    };
    let metadata = file.metadata().await.map_err(|err| map_io_error(err, "failed to stat media thumbnail"))?;
    stream_media_file(file, metadata.len(), "image/jpeg", None).await
}

#[utoipa::path(
    delete,
    path = "/media/{name}",
    tag = "Media",
    params(("name" = String, Path, description = "Media file name")),
    responses((status = 204, description = "Media deleted"), (status = 404, description = "Not found", body = ErrorBody))
)]
async fn delete_media(Path(name): Path<String>) -> ApiResult<impl IntoResponse> {
    let Some(filename) = sanitize_name(&name) else {
        return Err(ApiError::bad_request("invalid media name"));
    };
    let dir = media_dir()?;
    let path = dir.join(&filename);
    let (model_id, imu_data_file_name) = if let Ok(meta_dir) = media_meta_dir() {
        if let Some(meta) = load_media_metadata(&meta_dir, &filename).await { (meta.model_id, meta.imu_data_file_name) } else { (None, None) }
    } else {
        (None, None)
    };
    match fs::remove_file(&path).await {
        Ok(_) => {
            if let Ok(meta_dir) = media_meta_dir() {
                let _ = fs::remove_file(meta_dir.join(format!("{filename}.json"))).await;
                let _ = fs::remove_file(meta_dir.join(format!("{filename}.label"))).await;
                if let Some(sidecar_name) = imu_data_file_name.and_then(|value| sanitize_name(&value)) {
                    let _ = fs::remove_file(meta_dir.join(sidecar_name)).await;
                }
            }
            if let Some(model_id) = model_id {
                let _ = remove_ai_model(model_id).await;
            }
            Ok(StatusCode::NO_CONTENT)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(ApiError::not_found("media not found")),
        Err(err) => Err(map_io_error(err, "failed to delete media file")),
    }
}

#[allow(clippy::result_large_err)]
fn media_dir() -> Result<std::path::PathBuf, ApiError> {
    storage::ensure_subdir("media").map_err(|err| map_io_error(err, "failed to prepare media directory"))
}

#[allow(clippy::result_large_err)]
fn media_meta_dir() -> Result<std::path::PathBuf, ApiError> {
    storage::ensure_subdir("media-meta").map_err(|err| map_io_error(err, "failed to prepare media metadata directory"))
}

async fn load_media_metadata(dir: &std::path::Path, name: &str) -> Option<MediaMetadata> {
    let path = dir.join(format!("{name}.json"));
    let bytes = fs::read(&path).await.ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub(crate) async fn write_media_metadata(name: &str, metadata: MediaMetadata) -> Result<(), ApiError> {
    let Some(base) = sanitize_name(name) else {
        return Err(ApiError::bad_request("invalid media name"));
    };
    let meta_dir = media_meta_dir()?;
    let path = meta_dir.join(format!("{base}.json"));
    let bytes = serde_json::to_vec(&metadata).map_err(|err| ApiError::bad_request(format!("invalid metadata: {err}")))?;
    fs::write(&path, bytes).await.map_err(|err| map_io_error(err, "failed to write media metadata"))?;
    Ok(())
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
struct UpdateMetadataRequest {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    tags: Option<Vec<String>>,
    #[serde(default)]
    model_input_resolution: Option<String>,
    #[serde(default)]
    model_tensor_spec: Option<String>,
}

#[utoipa::path(
    patch,
    path = "/media/{name}/metadata",
    tag = "Media",
    params(("name" = String, Path, description = "Media file name")),
    request_body = UpdateMetadataRequest,
    responses((status = 200, description = "Metadata updated", body = MediaItem), (status = 400, description = "Invalid request", body = ErrorBody))
)]
async fn update_metadata(Path(name): Path<String>, Json(payload): Json<UpdateMetadataRequest>) -> ApiResult<impl IntoResponse> {
    let Some(filename) = sanitize_name(&name) else {
        return Err(ApiError::bad_request("invalid media name"));
    };

    let dir = media_dir()?;
    let meta_dir = media_meta_dir()?;
    let path = dir.join(&filename);
    let meta = fs::metadata(&path).await.map_err(|err| map_io_error(err, "failed to stat media file"))?;
    let content_type = guess_content_type(&filename);

    let mut md = ensure_media_metadata(&meta_dir, &filename, &path, &content_type).await.unwrap_or_default();
    if let Some(description) = payload.description {
        md.description = description.trim().to_string().into();
    }
    if let Some(tags) = payload.tags {
        md.tags = tags.into_iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
    }
    if let Some(value) = payload.model_input_resolution {
        md.model_input_resolution = value.trim().to_string().into();
    }
    if let Some(value) = payload.model_tensor_spec {
        md.model_tensor_spec = value.trim().to_string().into();
    }

    let mut final_name = filename.clone();
    if let Some(new_name) = payload.name.and_then(|n| sanitize_name(&n))
        && new_name != filename
    {
        let new_path = dir.join(&new_name);
        if fs::try_exists(&new_path).await.unwrap_or(false) {
            return Err(ApiError::bad_request("media name already exists"));
        }
        fs::rename(&path, &new_path).await.map_err(|err| map_io_error(err, "failed to rename media file"))?;
        let _ = fs::rename(meta_dir.join(format!("{filename}.json")), meta_dir.join(format!("{new_name}.json"))).await;
        let _ = fs::rename(meta_dir.join(format!("{filename}.label")), meta_dir.join(format!("{new_name}.label"))).await;
        final_name = new_name;
    }

    write_media_metadata(&final_name, md.clone()).await?;
    Ok(Json(MediaItem {
        name: final_name,
        size_bytes: meta.len(),
        content_type,
        description: md.description,
        tags: md.tags,
        stream_id: md.stream_id,
        kind: md.kind,
        captured_at_ms: md.captured_at_ms,
        width: md.width,
        height: md.height,
        fps: md.fps,
        video_codec: md.video_codec,
        label_attached: md.label_file_name.is_some(),
        label_file_name: md.label_file_name,
        model_id: md.model_id,
        model_input_resolution: md.model_input_resolution,
        model_tensor_spec: md.model_tensor_spec,
        imu_data_file_name: md.imu_data_file_name,
        imu_data_samples: md.imu_data_samples,
    }))
}

#[utoipa::path(
    get,
    path = "/media/{name}/label",
    tag = "Media",
    params(("name" = String, Path, description = "Media file name")),
    responses((status = 200, description = "Label file contents"), (status = 404, description = "Not found", body = ErrorBody))
)]
async fn fetch_label(Path(name): Path<String>) -> ApiResult<impl IntoResponse> {
    let Some(filename) = sanitize_name(&name) else {
        return Err(ApiError::bad_request("invalid media name"));
    };
    let meta_dir = media_meta_dir()?;
    let label_path = meta_dir.join(format!("{filename}.label"));
    if !fs::try_exists(&label_path).await.unwrap_or(false) {
        return Err(ApiError::not_found("label not found"));
    }
    let bytes = fs::read(&label_path).await.map_err(|err| map_io_error(err, "failed to read label file"))?;
    let text = String::from_utf8_lossy(&bytes).to_string();
    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text))
}

#[utoipa::path(
    post,
    path = "/media/{name}/label",
    tag = "Media",
    params(("name" = String, Path, description = "Media file name")),
    request_body(content = String, description = "Multipart form-data with a label file part named 'label'"),
    responses((status = 200, description = "Label attached", body = MediaItem), (status = 400, description = "Invalid upload", body = ErrorBody))
)]
async fn attach_label(Path(name): Path<String>, headers: HeaderMap, mut multipart: Multipart) -> ApiResult<impl IntoResponse> {
    let Some(filename) = sanitize_name(&name) else {
        return Err(ApiError::bad_request("invalid media name"));
    };
    let dir = media_dir()?;
    let meta_dir = media_meta_dir()?;
    let media_path = dir.join(&filename);
    let meta = fs::metadata(&media_path).await.map_err(|err| map_io_error(err, "failed to stat media file"))?;
    let content_type = guess_content_type(&filename);
    let expected_upload_bytes = upload_integrity::expected_upload_bytes(&headers).map_err(ApiError::bad_request)?;

    let mut label_bytes: Option<Vec<u8>> = None;
    let mut label_name: Option<String> = None;
    while let Some(field) = multipart.next_field().await.map_err(|err| ApiError::bad_request(format!("failed to read multipart: {err}")))? {
        let field_name = field.name().unwrap_or_default();
        if field_name != "label" {
            continue;
        }
        let Some(source_name) = field.file_name().and_then(sanitize_name) else {
            continue;
        };
        let bytes = field.bytes().await.map_err(|err| ApiError::bad_request(format!("failed to read label bytes: {err}")))?;
        upload_integrity::validate_expected_upload_bytes(bytes.len() as u64, expected_upload_bytes).map_err(ApiError::bad_request)?;
        label_bytes = Some(bytes.to_vec());
        label_name = Some(source_name);
        break;
    }
    let Some(label_bytes) = label_bytes else {
        return Err(ApiError::bad_request("multipart payload missing label part"));
    };

    store_label_bytes(&meta_dir, &filename, &label_bytes).await?;
    let mut md = ensure_media_metadata(&meta_dir, &filename, &media_path, &content_type).await.unwrap_or_default();
    md.label_file_name = label_name;
    write_media_metadata(&filename, md.clone()).await?;

    Ok(Json(MediaItem {
        name: filename,
        size_bytes: meta.len(),
        content_type,
        description: md.description,
        tags: md.tags,
        stream_id: md.stream_id,
        kind: md.kind,
        captured_at_ms: md.captured_at_ms,
        width: md.width,
        height: md.height,
        fps: md.fps,
        video_codec: md.video_codec,
        label_attached: md.label_file_name.is_some(),
        label_file_name: md.label_file_name,
        model_id: md.model_id,
        model_input_resolution: md.model_input_resolution,
        model_tensor_spec: md.model_tensor_spec,
        imu_data_file_name: md.imu_data_file_name,
        imu_data_samples: md.imu_data_samples,
    }))
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
struct CropRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

#[derive(Debug, Clone, Deserialize, ToSchema)]
struct ImageEditsRequest {
    #[serde(default)]
    rotate_degrees: Option<i32>,
    #[serde(default)]
    crop: Option<CropRect>,
}

#[utoipa::path(
    post,
    path = "/media/{name}/image/edits",
    tag = "Media",
    params(("name" = String, Path, description = "Media file name")),
    request_body = ImageEditsRequest,
    responses((status = 200, description = "Image updated", body = MediaItem), (status = 400, description = "Invalid request", body = ErrorBody))
)]
async fn apply_image_edits(Path(name): Path<String>, Json(payload): Json<ImageEditsRequest>) -> ApiResult<impl IntoResponse> {
    let Some(filename) = sanitize_name(&name) else {
        return Err(ApiError::bad_request("invalid media name"));
    };
    let dir = media_dir()?;
    let meta_dir = media_meta_dir()?;
    let path = dir.join(&filename);
    let content_type = guess_content_type(&filename);
    if !content_type.starts_with("image/") {
        return Err(ApiError::bad_request("asset is not an image"));
    }

    let bytes = fs::read(&path).await.map_err(|err| map_io_error(err, "failed to read image"))?;
    let content_type_for_task = content_type.clone();
    let edited = tokio::task::spawn_blocking(move || edit_image_bytes(&bytes, &content_type_for_task, payload)).await.map_err(|err| ApiError::internal(format!("image edit task failed: {err}")))??;

    let mut file = fs::File::create(&path).await.map_err(|err| map_io_error(err, "failed to write image"))?;
    file.write_all(&edited.bytes).await.map_err(|err| map_io_error(err, "failed to write image"))?;
    let size_bytes = edited.bytes.len() as u64;

    let mut md = ensure_media_metadata(&meta_dir, &filename, &path, &content_type).await.unwrap_or_default();
    md.width = Some(edited.width);
    md.height = Some(edited.height);
    write_media_metadata(&filename, md.clone()).await?;

    Ok(Json(MediaItem {
        name: filename,
        size_bytes,
        content_type,
        description: md.description,
        tags: md.tags,
        stream_id: md.stream_id,
        kind: md.kind,
        captured_at_ms: md.captured_at_ms,
        width: md.width,
        height: md.height,
        fps: md.fps,
        video_codec: md.video_codec,
        label_attached: md.label_file_name.is_some(),
        label_file_name: md.label_file_name,
        model_id: md.model_id,
        model_input_resolution: md.model_input_resolution,
        model_tensor_spec: md.model_tensor_spec,
        imu_data_file_name: md.imu_data_file_name,
        imu_data_samples: md.imu_data_samples,
    }))
}

struct EditedImage {
    bytes: Vec<u8>,
    width: u32,
    height: u32,
}

fn edit_image_bytes(bytes: &[u8], content_type: &str, payload: ImageEditsRequest) -> Result<EditedImage, Box<ApiError>> {
    let mut image = image::load_from_memory(bytes).map_err(|err| Box::new(ApiError::bad_request(format!("failed to decode image: {err}"))))?;
    if let Some(crop) = payload.crop {
        let (w, h) = image.dimensions();
        if crop.width == 0 || crop.height == 0 || crop.x >= w || crop.y >= h {
            return Err(Box::new(ApiError::bad_request("invalid crop rectangle")));
        }
        let crop_w = crop.width.min(w - crop.x);
        let crop_h = crop.height.min(h - crop.y);
        image = image.crop_imm(crop.x, crop.y, crop_w, crop_h);
    }

    let rotation = payload.rotate_degrees.unwrap_or(0).rem_euclid(360);
    if rotation != 0 {
        image = match rotation {
            90 => image.rotate90(),
            180 => image.rotate180(),
            270 => image.rotate270(),
            _ => return Err(Box::new(ApiError::bad_request("rotation must be 0/90/180/270"))),
        };
    }

    let (width, height) = image.dimensions();
    let mut out = Vec::new();
    let format = if content_type == "image/png" { image::ImageFormat::Png } else { image::ImageFormat::Jpeg };
    image.write_to(&mut Cursor::new(&mut out), format).map_err(|err| Box::new(ApiError::internal(format!("failed to encode image: {err}"))))?;
    Ok(EditedImage { bytes: out, width, height })
}

fn parse_tags(raw: &str) -> Vec<String> {
    raw.split(',').map(|value| value.trim().to_string()).filter(|value| !value.is_empty()).collect()
}

fn looks_like_model(filename: &str, content_type: &str) -> bool {
    if content_type == "application/octet-stream"
        && let Some(ext) = filename.split('.').next_back()
    {
        return matches!(ext.to_lowercase().as_str(), "tflite" | "onnx");
    }
    filename.to_lowercase().ends_with(".tflite") || filename.to_lowercase().ends_with(".onnx")
}

fn guess_model_format(filename: &str) -> Option<lib_ai::ModelFormat> {
    let ext = filename.split('.').next_back()?.to_lowercase();
    match ext.as_str() {
        "tflite" => Some(lib_ai::ModelFormat::TensorFlowLite),
        "onnx" => Some(lib_ai::ModelFormat::Onnx),
        _ => None,
    }
}

async fn hydrate_model_metadata(meta: &mut MediaMetadata, path: &std::path::Path, format: lib_ai::ModelFormat) {
    let bytes = match fs::read(path).await {
        Ok(bytes) => bytes,
        Err(_) => return,
    };
    let inspection = lib_ai::model::introspect::inspect_model(&bytes, &format);
    if meta.model_tensor_spec.is_none() {
        meta.model_tensor_spec = Some(format_tensor_spec(&inspection.inputs, &inspection.outputs));
    }
    if meta.model_input_resolution.is_none() {
        meta.model_input_resolution = derive_input_resolution(&inspection.inputs);
    }
    if !inspection.suggested_tags.is_empty() {
        merge_model_affinity_tags(&mut meta.tags, &inspection.suggested_tags);
    }
}

fn format_tensor_spec(inputs: &[lib_ai::ModelTensorMetadata], outputs: &[lib_ai::ModelTensorMetadata]) -> String {
    let format_tensors = |prefix: &str, tensors: &[lib_ai::ModelTensorMetadata]| -> String {
        let entries = tensors
            .iter()
            .enumerate()
            .map(|(idx, tensor)| {
                let name = tensor.name.clone().unwrap_or_else(|| format!("{prefix}{idx}"));
                let shape = tensor.shape.iter().map(|v| v.to_string()).collect::<Vec<_>>().join("x");
                format!("{name}:{shape}")
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("{prefix}s:{entries}")
    };
    let input_section = format_tensors("input", inputs);
    let output_section = format_tensors("output", outputs);
    format!("{input_section}|{output_section}")
}

fn derive_input_resolution(inputs: &[lib_ai::ModelTensorMetadata]) -> Option<String> {
    for tensor in inputs {
        let shape = &tensor.shape;
        if shape.len() == 4 {
            let h = shape[1];
            let w = shape[2];
            if h > 0 && w > 0 {
                return Some(format!("{w}×{h}"));
            }
        } else if shape.len() == 3 {
            let h = shape[0];
            let w = shape[1];
            if h > 0 && w > 0 {
                return Some(format!("{w}×{h}"));
            }
        }
    }
    None
}

fn merge_model_affinity_tags(existing: &mut Vec<String>, suggested: &[String]) {
    for tag in suggested {
        if !is_model_affinity_tag(tag) {
            continue;
        }
        if existing.iter().any(|value| value.eq_ignore_ascii_case(tag)) {
            continue;
        }
        existing.push(tag.clone());
    }
}

fn is_model_affinity_tag(tag: &str) -> bool {
    let normalized = tag.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return false;
    }
    if normalized.starts_with("runtime:") || normalized.starts_with("precision:") {
        return true;
    }
    matches!(normalized.as_str(), "quantized" | "edge-tpu" | "edgetpu" | "requires-edge-tpu")
}

fn needs_model_affinity_tags(tags: &[String]) -> bool {
    !tags.iter().any(|tag| is_model_affinity_tag(tag))
}

async fn hydrate_dimensions(meta: &mut MediaMetadata, path: &std::path::Path, content_type: &str) {
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
    let dims = tokio::task::spawn_blocking(move || image::load_from_memory(&bytes).ok().map(|img| img.dimensions())).await.ok().flatten();
    if let Some((w, h)) = dims {
        meta.width = Some(w);
        meta.height = Some(h);
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

async fn hydrate_video_metadata(meta: &mut MediaMetadata, path: &std::path::Path, filename: &str, content_type: &str) {
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

fn probe_video_stream(path: &std::path::Path) -> Option<VideoProbe> {
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

fn normalize_video_codec(raw: &str) -> Option<String> {
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

fn media_preview_cache_path(meta_dir: &std::path::Path, filename: &str, fps: Option<f32>) -> std::path::PathBuf {
    let fps_tag = fps.filter(|value| value.is_finite() && *value > 0.0).map(|value| format!("{value:.3}").replace('.', "_")).unwrap_or_else(|| "auto".to_string());
    meta_dir.join(format!("{filename}.preview.h264.{fps_tag}.mp4"))
}

fn media_thumbnail_cache_path(meta_dir: &std::path::Path, filename: &str) -> std::path::PathBuf {
    meta_dir.join(format!("{filename}.thumb.jpg"))
}

async fn preview_cache_fresh(source: &std::path::Path, preview: &std::path::Path) -> Result<bool, std::io::Error> {
    let source_meta = fs::metadata(source).await?;
    let preview_meta = match fs::metadata(preview).await {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };
    let source_modified = source_meta.modified().ok();
    let preview_modified = preview_meta.modified().ok();
    Ok(matches!((source_modified, preview_modified), (Some(src), Some(preview)) if preview >= src))
}

async fn transcode_preview_h264(source: &std::path::Path, output: &std::path::Path, fps_hint: Option<f32>, codec_hint: Option<&str>) -> Result<(), String> {
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
            // Pace raw bitstream inputs to the recorded cadence. `-framerate` is not reliably
            // honored by all demuxers here; `-r` before `-i` is.
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

async fn preview_fps_hint(meta_dir: &std::path::Path, meta: &MediaMetadata) -> Option<f32> {
    if let Some(frame_ts_name) = meta.frame_timestamps_file_name.as_deref().and_then(sanitize_name)
        && let Some(frame_ts_path) = resolve_frame_ts_path(meta_dir, &frame_ts_name).await
        && let Some(fps) = derive_fps_from_frame_timestamps(&frame_ts_path).await
    {
        return Some(fps);
    }
    meta.fps.filter(|value| value.is_finite() && *value > 0.0)
}

async fn resolve_frame_ts_path(meta_dir: &std::path::Path, frame_ts_name: &str) -> Option<std::path::PathBuf> {
    let meta_candidate = meta_dir.join(frame_ts_name);
    if fs::metadata(&meta_candidate).await.ok().is_some_and(|meta| meta.is_file()) {
        return Some(meta_candidate);
    }
    let media_dir = storage::ensure_subdir_async("media").await.ok()?;
    let media_candidate = media_dir.join(frame_ts_name);
    fs::metadata(&media_candidate).await.ok().and_then(|meta| meta.is_file().then_some(media_candidate))
}

async fn derive_fps_from_frame_timestamps(path: &std::path::Path) -> Option<f32> {
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

async fn render_video_thumbnail_jpeg(source: &std::path::Path, output: &std::path::Path, codec_hint: Option<&str>) -> Result<(), String> {
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

fn requested_range(headers: &HeaderMap) -> Option<String> {
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

fn build_media_archive_file(entries: Vec<(String, std::path::PathBuf)>) -> Result<std::fs::File, String> {
    let mut archive_file = tempfile::tempfile().map_err(|err| format!("failed to allocate temporary archive: {err}"))?;
    {
        let mut writer = zip::ZipWriter::new(archive_file);
        let mut copy_buffer = [0u8; 64 * 1024];
        for (name, path) in entries {
            let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            writer.start_file(name.as_str(), options).map_err(|err| format!("failed to start archive entry for {name}: {err}"))?;
            let mut input = std::fs::File::open(&path).map_err(|err| format!("failed to open media file {name}: {err}"))?;
            loop {
                let read = input.read(&mut copy_buffer).map_err(|err| format!("failed to read media file {name}: {err}"))?;
                if read == 0 {
                    break;
                }
                writer.write_all(&copy_buffer[..read]).map_err(|err| format!("failed to write archive entry for {name}: {err}"))?;
            }
        }
        archive_file = writer.finish().map_err(|err| format!("failed to finalize archive: {err}"))?;
    }
    archive_file.seek(SeekFrom::Start(0)).map_err(|err| format!("failed to rewind archive: {err}"))?;
    Ok(archive_file)
}

async fn stream_media_file(mut file: fs::File, len: u64, content_type: &str, range_header: Option<&str>) -> ApiResult<Response> {
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

async fn ensure_media_metadata(meta_dir: &std::path::Path, filename: &str, path: &std::path::Path, content_type: &str) -> Option<MediaMetadata> {
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

async fn store_label_bytes(meta_dir: &std::path::Path, filename: &str, bytes: &[u8]) -> Result<std::path::PathBuf, ApiError> {
    let path = meta_dir.join(format!("{filename}.label"));
    fs::write(&path, bytes).await.map_err(|err| map_io_error(err, "failed to write label file"))?;
    Ok(path)
}

async fn load_media_label_bytes(filename: &str) -> Result<Option<Vec<u8>>, ApiError> {
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

fn parse_label_bytes(bytes: &[u8]) -> Option<Vec<String>> {
    if bytes.is_empty() {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(parsed) = serde_json::from_str::<ModelMetadata>(trimmed)
        && !parsed.labels.is_empty()
    {
        return Some(parsed.labels);
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Ok(meta) = serde_json::from_value::<ModelMetadata>(value.clone())
            && !meta.labels.is_empty()
        {
            return Some(meta.labels);
        }
        if let Some(array) = value.get("labels").and_then(|v| v.as_array()) {
            let labels: Vec<String> = array.iter().filter_map(|entry| entry.as_str().map(|s| s.to_string())).collect();
            if !labels.is_empty() {
                return Some(labels);
            }
        }
        if let Some(array) = value.as_array() {
            let labels: Vec<String> = array.iter().filter_map(|entry| entry.as_str().map(|s| s.to_string())).collect();
            if !labels.is_empty() {
                return Some(labels);
            }
        }
    }

    let labels: Vec<String> = trimmed.lines().map(|line| line.trim()).filter(|line| !line.is_empty()).map(|line| line.to_string()).collect();
    if labels.is_empty() { None } else { Some(labels) }
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct AiModelManifest {
    #[serde(default)]
    models: Vec<AiModelManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AiModelManifestEntry {
    id: ModelId,
    format: ModelFormat,
    metadata: ModelMetadata,
    artifact: String,
    #[serde(default)]
    label_artifact: Option<String>,
    #[serde(with = "chrono::serde::ts_milliseconds")]
    created_at: Timestamp,
    #[serde(default)]
    health: AiModelHealth,
}

async fn register_ai_model_for_media(filename: &str, path: &StdPath, content_type: &str, meta: &mut MediaMetadata) -> Result<(), ApiError> {
    if !looks_like_model(filename, content_type) {
        return Ok(());
    }

    let Some(format) = guess_model_format(filename) else {
        return Ok(());
    };

    let model_dir = lib_ai::storage::default_model_dir();
    fs::create_dir_all(&model_dir).await.map_err(|err| map_io_error(err, "failed to prepare AI model directory"))?;
    let manifest_path = model_dir.join(lib_ai::storage::MANIFEST_NAME);

    let mut manifest = load_ai_model_manifest(&manifest_path).await?;
    let model_uuid = meta.model_id.unwrap_or_else(Uuid::new_v4);
    let model_id = ModelId(model_uuid);

    let label_bytes = load_media_label_bytes(filename).await?;
    let labels = label_bytes.as_deref().and_then(parse_label_bytes);

    if let Some(existing_idx) = manifest.models.iter().position(|entry| entry.id == model_id) {
        if manifest.models[existing_idx].label_artifact.is_none() {
            if let Some(labels) = labels.clone() {
                manifest.models[existing_idx].metadata.labels = labels;
            }
            if let Some(label_bytes) = label_bytes.as_ref() {
                let label_name = format!("{}.labels.json", model_id.0);
                let label_path = model_dir.join(&label_name);
                if !fs::try_exists(&label_path).await.unwrap_or(false) {
                    fs::write(&label_path, label_bytes).await.map_err(|err| map_io_error(err, "failed to store AI model label artifact"))?;
                }
                manifest.models[existing_idx].label_artifact = Some(label_name);
                save_ai_model_manifest(&manifest_path, &manifest).await?;
            }
        }
        meta.model_id = Some(model_uuid);
        return Ok(());
    }

    let artifact = artifact_name(&model_id, &format);
    let artifact_path = model_dir.join(&artifact);
    if !fs::try_exists(&artifact_path).await.unwrap_or(false) {
        fs::copy(path, &artifact_path).await.map_err(|err| map_io_error(err, "failed to store AI model artifact"))?;
    }

    let bytes = fs::read(path).await.map_err(|err| map_io_error(err, "failed to read AI model payload"))?;
    let inspection = lib_ai::model::introspect::inspect_model(&bytes, &format);

    let mut metadata = ModelMetadata { display_name: Some(model_display_name(filename)), inputs: inspection.inputs, outputs: inspection.outputs, ..Default::default() };
    if !inspection.suggested_tags.is_empty() {
        metadata.tags = inspection.suggested_tags;
    }
    if let Some(labels) = labels {
        metadata.labels = labels;
    }

    let label_artifact = if let Some(label_bytes) = label_bytes.as_ref() {
        let label_name = format!("{}.labels.json", model_id.0);
        let label_path = model_dir.join(&label_name);
        fs::write(&label_path, label_bytes).await.map_err(|err| map_io_error(err, "failed to store AI model label artifact"))?;
        Some(label_name)
    } else {
        None
    };

    let created_at = Utc::now();
    let health = AiModelHealth::ready(created_at);
    manifest.models.push(AiModelManifestEntry { id: model_id, format, metadata, artifact, label_artifact, created_at, health });

    save_ai_model_manifest(&manifest_path, &manifest).await?;
    meta.model_id = Some(model_uuid);
    Ok(())
}

async fn remove_ai_model(model_id: Uuid) -> Result<(), ApiError> {
    let model_dir = lib_ai::storage::default_model_dir();
    let manifest_path = model_dir.join(lib_ai::storage::MANIFEST_NAME);
    let mut manifest = load_ai_model_manifest(&manifest_path).await?;
    let target = ModelId(model_id);

    if let Some(idx) = manifest.models.iter().position(|entry| entry.id == target) {
        let entry = manifest.models.remove(idx);
        let artifact_path = model_dir.join(&entry.artifact);
        let _ = fs::remove_file(&artifact_path).await;
        if let Some(label_artifact) = entry.label_artifact {
            let _ = fs::remove_file(model_dir.join(label_artifact)).await;
        }
        save_ai_model_manifest(&manifest_path, &manifest).await?;
    }

    Ok(())
}

async fn load_ai_model_manifest(path: &StdPath) -> Result<AiModelManifest, ApiError> {
    match fs::read(path).await {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|err| ApiError::internal(format!("invalid AI model manifest: {err}"))),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(AiModelManifest::default()),
        Err(err) => Err(map_io_error(err, "failed to read AI model manifest")),
    }
}

async fn save_ai_model_manifest(path: &StdPath, manifest: &AiModelManifest) -> Result<(), ApiError> {
    let bytes = serde_json::to_vec_pretty(manifest).map_err(|err| ApiError::internal(format!("failed to serialize AI model manifest: {err}")))?;
    fs::write(path, bytes).await.map_err(|err| map_io_error(err, "failed to write AI model manifest"))?;
    Ok(())
}

fn artifact_name(id: &ModelId, format: &ModelFormat) -> String {
    let extension = match format {
        ModelFormat::TensorFlowLite => "tflite",
        ModelFormat::Onnx => "onnx",
        ModelFormat::Raw => "bin",
    };
    format!("{}.{}", id.0, extension)
}

fn model_display_name(filename: &str) -> String {
    StdPath::new(filename).file_stem().and_then(|stem| stem.to_str()).map(|value| value.trim()).filter(|value| !value.is_empty()).unwrap_or(filename).to_string()
}

fn map_io_error(err: std::io::Error, context: &str) -> ApiError {
    let status = if err.kind() == std::io::ErrorKind::NotFound { StatusCode::NOT_FOUND } else { StatusCode::INTERNAL_SERVER_ERROR };
    let code = if status == StatusCode::NOT_FOUND { "not_found" } else { "internal" };
    ApiError::new(status, code, format!("{context}: {err}"))
}

fn guess_content_type(name: &str) -> String {
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

fn max_upload_bytes() -> u64 {
    const DEFAULT_MB: u64 = 512;
    std::env::var("HELIOS_API_MAX_UPLOAD_MB").ok().and_then(|raw| raw.parse::<u64>().ok()).map(|mb| mb.saturating_mul(1024 * 1024)).filter(|&bytes| bytes > 0).unwrap_or(DEFAULT_MB * 1024 * 1024)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::{Body, to_bytes};
    use axum::http::{
        Request, StatusCode,
        header::{CONTENT_LENGTH, CONTENT_TYPE},
    };
    use std::io::Read;
    use std::sync::{Arc, OnceLock};
    use tower::ServiceExt;

    fn init_data_dir() -> &'static std::path::Path {
        static ROOT: OnceLock<std::path::PathBuf> = OnceLock::new();
        ROOT.get_or_init(|| {
            let dir = tempfile::tempdir().expect("tempdir").keep();
            storage::set_data_root_for_tests(dir.clone());
            dir
        })
        .as_path()
    }

    fn multipart(boundary: &str, parts: &[(&str, Option<&str>, &str, &[u8])]) -> Vec<u8> {
        // parts: (field_name, filename, content_type, bytes)
        let mut body = Vec::new();
        for (name, filename, content_type, bytes) in parts {
            body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
            match filename {
                Some(filename) => {
                    body.extend_from_slice(format!("Content-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n").as_bytes());
                    body.extend_from_slice(bytes);
                    body.extend_from_slice(b"\r\n");
                }
                None => {
                    body.extend_from_slice(format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes());
                    body.extend_from_slice(bytes);
                    body.extend_from_slice(b"\r\n");
                }
            }
        }
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        body
    }

    async fn test_app() -> Router {
        let state = Arc::new(crate::app_state::ApiAppState::new(Arc::new(crate::ipc::connect_all().await)));
        Router::new().nest("/media", router()).with_state(state)
    }

    #[tokio::test]
    async fn upload_accepts_metadata_before_file() {
        let root = init_data_dir();

        let app = test_app().await;
        let boundary = "BOUNDARY";
        let body = multipart(boundary, &[("kind", None, "text/plain", b"image"), ("files", Some("hello.png"), "image/png", b"PNGDATA")]);
        let content_len = body.len();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/media")
                    .header(CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
                    .header(CONTENT_LENGTH, content_len)
                    .body(Body::from(body))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::CREATED);
        let stored = root.join("media").join("hello.png");
        assert!(stored.exists(), "expected stored file at {}", stored.display());
        assert_eq!(std::fs::read(stored).expect("read stored file"), b"PNGDATA");
    }

    #[tokio::test]
    async fn upload_rejects_multiple_files() {
        let _ = init_data_dir();

        let app = test_app().await;
        let boundary = "BOUNDARY2";
        let body = multipart(boundary, &[("files", Some("a.txt"), "text/plain", b"A"), ("files", Some("b.txt"), "text/plain", b"B")]);
        let content_len = body.len();

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/media")
                    .header(CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
                    .header(CONTENT_LENGTH, content_len)
                    .body(Body::from(body))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn download_archive_includes_selected_files() {
        let root = init_data_dir();
        let media_dir = root.join("media");
        std::fs::create_dir_all(&media_dir).expect("create media directory");

        let name_a = format!("archive-test-a-{}.txt", Uuid::new_v4());
        let name_b = format!("archive-test-b-{}.txt", Uuid::new_v4());
        std::fs::write(media_dir.join(&name_a), b"alpha").expect("write first media file");
        std::fs::write(media_dir.join(&name_b), b"beta").expect("write second media file");

        let app = test_app().await;
        let response = app.oneshot(Request::builder().method("GET").uri(format!("/media/download.zip?name={name_a}&name={name_b}")).body(Body::empty()).expect("request")).await.expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get(CONTENT_TYPE).and_then(|value| value.to_str().ok()), Some("application/zip"));

        let body = to_bytes(response.into_body(), usize::MAX).await.expect("archive body");
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(body.to_vec())).expect("valid zip archive");

        {
            let mut file_a = archive.by_name(&name_a).expect("first archive entry");
            let mut content_a = String::new();
            file_a.read_to_string(&mut content_a).expect("read first archive entry");
            assert_eq!(content_a, "alpha");
        }

        {
            let mut file_b = archive.by_name(&name_b).expect("second archive entry");
            let mut content_b = String::new();
            file_b.read_to_string(&mut content_b).expect("read second archive entry");
            assert_eq!(content_b, "beta");
        }
    }
}
