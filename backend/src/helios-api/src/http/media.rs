mod archive;
mod models;
mod preview;
mod support;
#[cfg(test)]
mod tests;
mod types;

pub(crate) use support::write_media_metadata;
pub use types::MediaItem;
pub(crate) use types::MediaMetadata;

use archive::build_media_archive_file;
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, Query, RawQuery, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
};
use chrono::Utc;
use std::collections::HashSet;
use tokio::{fs, io::AsyncWriteExt};
use tokio_util::io::ReaderStream;

use self::{
    models::{guess_model_format, hydrate_model_metadata, looks_like_model, register_ai_model_for_media, remove_ai_model},
    preview::{
        media_preview_cache_path, media_thumbnail_cache_path, normalize_video_codec, preview_cache_fresh, preview_fps_hint, render_video_thumbnail_jpeg, requested_range, stream_media_file,
        transcode_preview_h264,
    },
    support::{
        count_imu_sidecar_samples, ensure_media_metadata, guess_content_type, is_internal_media_artifact, load_media_metadata, map_io_error, max_upload_bytes, media_dir, media_meta_dir, parse_tags,
        store_label_bytes,
    },
    types::{ImageEditsRequest, ListMediaParams, UpdateMetadataRequest, media_item, parse_download_media_archive_params},
};
use super::{
    AppState,
    error::{ApiError, ApiResult, ErrorBody},
    storage::sanitize_name,
    upload_integrity,
};

pub fn router() -> Router<AppState> {
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

#[utoipa::path(
    get,
    path = "/media",
    tag = "Media",
    responses((status = 200, description = "List stored media assets", body = [MediaItem]), (status = 500, description = "Storage error", body = ErrorBody))
)]
async fn list_media(Query(params): Query<ListMediaParams>) -> ApiResult<impl IntoResponse> {
    let dir = media_dir()?;
    let meta_dir = media_meta_dir()?;

    let mut entries = fs::read_dir(dir).await.map_err(|err| map_io_error(err, "failed to read media directory"))?;
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
        if let Some(kind) = params.kind.as_deref().map(str::trim).filter(|value| !value.is_empty())
            && md.kind.as_deref() != Some(kind)
        {
            continue;
        }
        media.push(media_item(name, meta.len(), content_type, md));
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
async fn download_media_archive(RawQuery(raw_query): RawQuery) -> ApiResult<Response> {
    let params = parse_download_media_archive_params(raw_query.as_deref());
    if params.name.is_empty() {
        return Err(ApiError::bad_request("select at least one media file to download"));
    }
    if params.name.len() > 512 {
        return Err(ApiError::bad_request("too many files requested in one archive"));
    }

    let mut seen = HashSet::new();
    let mut names = Vec::with_capacity(params.name.len());
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
    let mut entries = Vec::with_capacity(names.len());
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
    let mut pending_model_format = None;

    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            Err(err) => {
                return Err(ApiError::bad_request(format!("failed to read upload payload: {err}")));
            }
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

        let filename = file_name.expect("checked above");
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
        let mut file = fs::File::create(&path).await.map_err(|err| map_io_error(err, "failed to create media file"))?;

        let mut written = 0u64;
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

            let media_path = dir.join(&filename);
            if let Some(format) = pending_model_format
                && (meta.model_tensor_spec.is_none() || meta.model_input_resolution.is_none())
            {
                hydrate_model_metadata(&mut meta, &media_path, format).await;
            }
            if looks_like_model(&filename, &content_type) {
                register_ai_model_for_media(&filename, &media_path, &content_type, &mut meta).await?;
            }

            write_media_metadata(&filename, meta.clone()).await?;
            Ok((StatusCode::CREATED, Json(media_item(filename, written, content_type, meta))))
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
    let path = media_dir()?.join(&filename);
    let range = requested_range(&headers);
    let file = match fs::File::open(&path).await {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(ApiError::not_found("media not found"));
        }
        Err(err) => return Err(map_io_error(err, "failed to read media file")),
    };
    let metadata = file.metadata().await.map_err(|err| map_io_error(err, "failed to stat media file"))?;
    let content_type = guess_content_type(path.file_name().and_then(|value| value.to_str()).unwrap_or_default());
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
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(ApiError::not_found("IMU sidecar not found"));
        }
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

    let mut sidecar_bytes = None;
    let mut uploaded_name = None;
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
    Ok(Json(media_item(filename, meta.len(), content_type, md)))
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
    Ok(Json(media_item(filename, meta.len(), content_type, md)))
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
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(ApiError::not_found("media not found"));
            }
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
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(ApiError::not_found("media not found"));
            }
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

    let file = fs::File::open(&preview_path).await.map_err(|err| map_io_error(err, "failed to read media preview"))?;
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
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Err(ApiError::not_found("media not found"));
            }
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

    let file = fs::File::open(&thumbnail_path).await.map_err(|err| map_io_error(err, "failed to read media thumbnail"))?;
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
    let path = media_dir()?.join(&filename);
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
        md.tags = tags.into_iter().map(|value| value.trim().to_string()).filter(|value| !value.is_empty()).collect();
    }
    if let Some(value) = payload.model_input_resolution {
        md.model_input_resolution = value.trim().to_string().into();
    }
    if let Some(value) = payload.model_tensor_spec {
        md.model_tensor_spec = value.trim().to_string().into();
    }

    let mut final_name = filename.clone();
    if let Some(new_name) = payload.name.and_then(|value| sanitize_name(&value))
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
    Ok(Json(media_item(final_name, meta.len(), content_type, md)))
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
    let label_path = media_meta_dir()?.join(format!("{filename}.label"));
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

    let mut label_bytes = None;
    let mut label_name = None;
    while let Some(field) = multipart.next_field().await.map_err(|err| ApiError::bad_request(format!("failed to read multipart: {err}")))? {
        if field.name().unwrap_or_default() != "label" {
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
    Ok(Json(media_item(filename, meta.len(), content_type, md)))
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
    let (edited_bytes, width, height) = crate::api_tools_client::image_edit(&bytes, content_type.clone(), payload.rotate_degrees, payload.crop.map(Into::into)).await?;

    let mut file = fs::File::create(&path).await.map_err(|err| map_io_error(err, "failed to write image"))?;
    file.write_all(&edited_bytes).await.map_err(|err| map_io_error(err, "failed to write image"))?;

    let mut md = ensure_media_metadata(&meta_dir, &filename, &path, &content_type).await.unwrap_or_default();
    md.width = Some(width);
    md.height = Some(height);
    write_media_metadata(&filename, md.clone()).await?;
    Ok(Json(media_item(filename, edited_bytes.len() as u64, content_type, md)))
}
