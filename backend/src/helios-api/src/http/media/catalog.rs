use axum::{
    Json,
    body::Body,
    extract::{Multipart, Query, RawQuery},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use std::collections::HashSet;
use tokio::{fs, io::AsyncWriteExt};
use tokio_util::io::ReaderStream;

use super::{
    MediaItem, MediaMetadata,
    archive::build_media_archive_file,
    models::{guess_model_format, hydrate_model_metadata, looks_like_model, register_ai_model_for_media},
    support::{ensure_media_metadata, guess_content_type, is_internal_media_artifact, map_io_error, max_upload_bytes, media_dir, media_meta_dir, parse_tags, store_label_bytes},
    types::{ListMediaParams, media_item, parse_download_media_archive_params},
    write_media_metadata,
};
use crate::http::{
    error::{ApiError, ApiResult, ErrorBody},
    storage::sanitize_name,
    upload_integrity,
};

#[utoipa::path(
    get,
    path = "/media",
    tag = "Media",
    responses((status = 200, description = "List stored media assets", body = [MediaItem]), (status = 500, description = "Storage error", body = ErrorBody))
)]
pub(crate) async fn list_media(Query(params): Query<ListMediaParams>) -> ApiResult<impl IntoResponse> {
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
pub(crate) async fn download_media_archive(RawQuery(raw_query): RawQuery) -> ApiResult<Response> {
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
pub(crate) async fn upload_media(headers: HeaderMap, mut multipart: Multipart) -> ApiResult<impl IntoResponse> {
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
