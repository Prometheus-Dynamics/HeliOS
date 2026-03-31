use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
    response::IntoResponse,
};
use tokio::{fs, io::AsyncWriteExt};

use super::{
    AppState, MediaItem,
    preview::{
        media_preview_cache_path, media_thumbnail_cache_path, normalize_video_codec, preview_cache_fresh, preview_fps_hint, render_video_thumbnail_jpeg, requested_range, stream_media_file,
        transcode_preview_h264,
    },
    support::{ensure_media_metadata, guess_content_type, map_io_error, media_dir, media_meta_dir},
    types::{ImageEditsRequest, media_item},
    write_media_metadata,
};
use crate::http::error::{ApiError, ApiResult, ErrorBody};
use crate::http::storage::sanitize_name;

#[utoipa::path(
    get,
    path = "/media/{name}/preview",
    tag = "Media",
    params(("name" = String, Path, description = "Media file name")),
    responses((status = 200, description = "Media preview content"), (status = 404, description = "Not found", body = ErrorBody))
)]
pub(crate) async fn fetch_media_preview(State(state): State<AppState>, Path(name): Path<String>, headers: HeaderMap) -> ApiResult<impl IntoResponse> {
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
pub(crate) async fn fetch_media_thumbnail(State(state): State<AppState>, Path(name): Path<String>) -> ApiResult<impl IntoResponse> {
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
    post,
    path = "/media/{name}/image/edits",
    tag = "Media",
    params(("name" = String, Path, description = "Media file name")),
    request_body = ImageEditsRequest,
    responses((status = 200, description = "Image updated", body = MediaItem), (status = 400, description = "Invalid request", body = ErrorBody))
)]
pub(crate) async fn apply_image_edits(Path(name): Path<String>, Json(payload): Json<ImageEditsRequest>) -> ApiResult<impl IntoResponse> {
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
