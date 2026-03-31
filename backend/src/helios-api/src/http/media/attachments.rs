use axum::{
    Json,
    extract::{Multipart, Path},
    http::HeaderMap,
    response::IntoResponse,
};
use tokio::fs;

use super::{
    MediaItem,
    preview::{requested_range, stream_media_file},
    support::{count_imu_sidecar_samples, ensure_media_metadata, guess_content_type, load_media_metadata, map_io_error, media_dir, media_meta_dir, store_label_bytes},
    types::media_item,
    write_media_metadata,
};
use crate::http::{
    error::{ApiError, ApiResult, ErrorBody},
    storage::sanitize_name,
    upload_integrity,
};

#[utoipa::path(
    get,
    path = "/media/{name}/imu",
    tag = "Media",
    params(("name" = String, Path, description = "Media file name")),
    responses((status = 200, description = "IMU sidecar stream"), (status = 404, description = "Not found", body = ErrorBody))
)]
pub(crate) async fn fetch_media_imu(Path(name): Path<String>, headers: HeaderMap) -> ApiResult<impl IntoResponse> {
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
pub(crate) async fn attach_media_imu(Path(name): Path<String>, headers: HeaderMap, mut multipart: Multipart) -> ApiResult<impl IntoResponse> {
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
pub(crate) async fn delete_media_imu(Path(name): Path<String>) -> ApiResult<impl IntoResponse> {
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
    path = "/media/{name}/label",
    tag = "Media",
    params(("name" = String, Path, description = "Media file name")),
    responses((status = 200, description = "Label file contents"), (status = 404, description = "Not found", body = ErrorBody))
)]
pub(crate) async fn fetch_label(Path(name): Path<String>) -> ApiResult<impl IntoResponse> {
    let Some(filename) = sanitize_name(&name) else {
        return Err(ApiError::bad_request("invalid media name"));
    };
    let label_path = media_meta_dir()?.join(format!("{filename}.label"));
    if !fs::try_exists(&label_path).await.unwrap_or(false) {
        return Err(ApiError::not_found("label not found"));
    }
    let bytes = fs::read(&label_path).await.map_err(|err| map_io_error(err, "failed to read label file"))?;
    let text = String::from_utf8_lossy(&bytes).to_string();
    Ok(([(axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")], text))
}

#[utoipa::path(
    post,
    path = "/media/{name}/label",
    tag = "Media",
    params(("name" = String, Path, description = "Media file name")),
    request_body(content = String, description = "Multipart form-data with a label file part named 'label'"),
    responses((status = 200, description = "Label attached", body = MediaItem), (status = 400, description = "Invalid upload", body = ErrorBody))
)]
pub(crate) async fn attach_label(Path(name): Path<String>, headers: HeaderMap, mut multipart: Multipart) -> ApiResult<impl IntoResponse> {
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
