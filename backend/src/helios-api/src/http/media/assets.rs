use axum::{Json, extract::Path, http::HeaderMap, response::IntoResponse};
use tokio::fs;

use super::{
    MediaItem,
    models::remove_ai_model,
    preview::{requested_range, stream_media_file},
    support::{ensure_media_metadata, guess_content_type, load_media_metadata, map_io_error, media_dir, media_meta_dir},
    types::{UpdateMetadataRequest, media_item},
    write_media_metadata,
};
use crate::http::{
    error::{ApiError, ApiResult, ErrorBody},
    storage::sanitize_name,
};

#[utoipa::path(
    get,
    path = "/media/{name}",
    tag = "Media",
    params(("name" = String, Path, description = "Media file name")),
    responses((status = 200, description = "Media content"), (status = 404, description = "Not found", body = ErrorBody))
)]
pub(crate) async fn fetch_media(Path(name): Path<String>, headers: HeaderMap) -> ApiResult<impl IntoResponse> {
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
    delete,
    path = "/media/{name}",
    tag = "Media",
    params(("name" = String, Path, description = "Media file name")),
    responses((status = 204, description = "Media deleted"), (status = 404, description = "Not found", body = ErrorBody))
)]
pub(crate) async fn delete_media(Path(name): Path<String>) -> ApiResult<impl IntoResponse> {
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
            Ok(axum::http::StatusCode::NO_CONTENT)
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
pub(crate) async fn update_metadata(Path(name): Path<String>, Json(payload): Json<UpdateMetadataRequest>) -> ApiResult<impl IntoResponse> {
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
