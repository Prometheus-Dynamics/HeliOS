use axum::{
    Json,
    extract::{Multipart, Path},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use tokio::fs;

use crate::http::{
    error::{ApiError, ApiResult, ErrorBody},
    storage::sanitize_name,
    upload_integrity,
};

use super::support::{
    DISABLED_SUFFIX_SUFFIX, ensure_install_dir, ensure_plugin_filename, ensure_upload_dir, finalize_upload_response, install_dir, invalidate_registry_snapshot_file, max_upload_bytes, move_upload,
    plugin_dirs, process_upload_field, store_upload, stream_file, upload_dir,
};
use super::types::{PluginInstallRequest, PluginInstallResponse, PluginToggleResponse, PluginUploadResponse};

#[utoipa::path(
    post,
    path = "/plugins/upload",
    tag = "Plugins",
    request_body(content = String, description = "Multipart form-data with exactly one .so file part"),
    responses(
        (status = 201, description = "Plugin uploaded", body = PluginUploadResponse),
        (status = 400, description = "Invalid upload", body = ErrorBody),
        (status = 500, description = "Storage error", body = ErrorBody)
    )
)]
pub(crate) async fn upload_plugin(headers: HeaderMap, mut multipart: Multipart) -> ApiResult<impl IntoResponse> {
    let max_bytes = max_upload_bytes();
    let expected_upload_bytes = upload_integrity::expected_upload_bytes(&headers).map_err(ApiError::bad_request)?;
    let mut uploaded: Option<PluginUploadResponse> = None;

    while let Some(field) = multipart.next_field().await.map_err(|err| ApiError::bad_request(format!("failed to read upload payload: {err}")))? {
        if uploaded.is_some() {
            return Err(ApiError::bad_request("only one file may be uploaded per request"));
        }
        let upload = process_upload_field(field, max_bytes, expected_upload_bytes).await?;
        let target_dir = ensure_upload_dir().await?;
        let target_path = target_dir.join(&upload.name);
        if let Err(err) = store_upload(&upload.temp_path, &target_path).await {
            let _ = fs::remove_file(&upload.temp_path).await;
            return Err(err);
        }
        let _ = fs::remove_file(&upload.temp_path).await;
        uploaded = Some(finalize_upload_response(upload));
    }

    match uploaded {
        Some(info) => Ok((StatusCode::CREATED, Json(info))),
        None => Err(ApiError::bad_request("empty upload")),
    }
}

#[utoipa::path(
    post,
    path = "/plugins/install",
    tag = "Plugins",
    request_body = PluginInstallRequest,
    responses(
        (status = 200, description = "Plugin installed", body = PluginInstallResponse),
        (status = 400, description = "Invalid request", body = ErrorBody),
        (status = 500, description = "Storage error", body = ErrorBody)
    )
)]
pub(crate) async fn install_plugin(Json(payload): Json<PluginInstallRequest>) -> ApiResult<impl IntoResponse> {
    let target_dir = ensure_install_dir().await?;
    let upload_name = sanitize_name(payload.upload_name.trim()).ok_or_else(|| ApiError::bad_request("invalid upload_name"))?;
    ensure_plugin_filename(&upload_name)?;
    let source = ensure_upload_dir().await?.join(&upload_name);
    if !source.exists() {
        return Err(ApiError::bad_request("upload_name not found"));
    }
    let target = target_dir.join(&upload_name);
    move_upload(&source, &target).await?;
    invalidate_registry_snapshot_file().await;
    Ok(Json(PluginInstallResponse { name: upload_name, installed: true }))
}

#[utoipa::path(
    get,
    path = "/plugins/{name}",
    tag = "Plugins",
    params(("name" = String, Path, description = "Plugin filename")),
    responses((status = 200, description = "Download plugin", content_type = "application/octet-stream"), (status = 404, description = "Not found", body = ErrorBody))
)]
pub(crate) async fn download_plugin(Path(name): Path<String>) -> ApiResult<Response> {
    let name = sanitize_name(&name).ok_or_else(|| ApiError::bad_request("invalid filename"))?;
    ensure_plugin_filename(&name)?;
    let path = install_dir().join(&name);
    stream_file(&path, &name).await
}

#[utoipa::path(
    get,
    path = "/plugins/uploads/{name}",
    tag = "Plugins",
    params(("name" = String, Path, description = "Uploaded plugin filename")),
    responses((status = 200, description = "Download uploaded plugin", content_type = "application/octet-stream"), (status = 404, description = "Not found", body = ErrorBody))
)]
pub(crate) async fn download_upload(Path(name): Path<String>) -> ApiResult<Response> {
    let name = sanitize_name(&name).ok_or_else(|| ApiError::bad_request("invalid filename"))?;
    ensure_plugin_filename(&name)?;
    let path = upload_dir().join(&name);
    stream_file(&path, &name).await
}

#[utoipa::path(
    delete,
    path = "/plugins/{name}",
    tag = "Plugins",
    params(("name" = String, Path, description = "Plugin filename")),
    responses((status = 204, description = "Plugin removed"), (status = 404, description = "Not found", body = ErrorBody))
)]
pub(crate) async fn delete_plugin(Path(name): Path<String>) -> ApiResult<impl IntoResponse> {
    let name = sanitize_name(&name).ok_or_else(|| ApiError::bad_request("invalid filename"))?;
    ensure_plugin_filename(&name)?;
    let path = install_dir().join(&name);
    match fs::remove_file(&path).await {
        Ok(_) => {
            invalidate_registry_snapshot_file().await;
            return Ok(StatusCode::NO_CONTENT);
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(ApiError::internal(format!("failed to delete plugin: {err}"))),
    }
    let disabled_path = install_dir().join(format!("{name}{DISABLED_SUFFIX_SUFFIX}"));
    match fs::remove_file(&disabled_path).await {
        Ok(_) => {
            invalidate_registry_snapshot_file().await;
            Ok(StatusCode::NO_CONTENT)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Err(ApiError::not_found("plugin not found")),
        Err(err) => Err(ApiError::internal(format!("failed to delete plugin: {err}"))),
    }
}

#[utoipa::path(
    post,
    path = "/plugins/{name}/disable",
    tag = "Plugins",
    params(("name" = String, Path, description = "Plugin filename")),
    responses((status = 200, description = "Plugin disabled", body = PluginToggleResponse), (status = 404, description = "Not found", body = ErrorBody))
)]
pub(crate) async fn disable_plugin(Path(name): Path<String>) -> ApiResult<impl IntoResponse> {
    let name = sanitize_name(&name).ok_or_else(|| ApiError::bad_request("invalid filename"))?;
    ensure_plugin_filename(&name)?;
    let install = install_dir();
    let source = install.join(&name);
    if fs::metadata(&source).await.is_ok() {
        let target = install.join(format!("{name}{DISABLED_SUFFIX_SUFFIX}"));
        move_upload(&source, &target).await?;
        invalidate_registry_snapshot_file().await;
        return Ok(Json(PluginToggleResponse { name, enabled: false }));
    }

    let exists_elsewhere = plugin_dirs().into_iter().filter(|dir| dir != &install).any(|dir| dir.join(&name).exists());
    if !exists_elsewhere {
        return Err(ApiError::not_found("plugin not found"));
    }

    let marker = ensure_install_dir().await?.join(format!("{name}{DISABLED_SUFFIX_SUFFIX}"));
    if fs::metadata(&marker).await.is_err() {
        let _ = fs::File::create(&marker).await.map_err(|err| ApiError::internal(format!("failed to create disable marker: {err}")))?;
    }
    invalidate_registry_snapshot_file().await;
    Ok(Json(PluginToggleResponse { name, enabled: false }))
}

#[utoipa::path(
    post,
    path = "/plugins/{name}/enable",
    tag = "Plugins",
    params(("name" = String, Path, description = "Plugin filename")),
    responses((status = 200, description = "Plugin enabled", body = PluginToggleResponse), (status = 404, description = "Not found", body = ErrorBody))
)]
pub(crate) async fn enable_plugin(Path(name): Path<String>) -> ApiResult<impl IntoResponse> {
    let name = sanitize_name(&name).ok_or_else(|| ApiError::bad_request("invalid filename"))?;
    ensure_plugin_filename(&name)?;
    let install = install_dir();
    let disabled_path = install.join(format!("{name}{DISABLED_SUFFIX_SUFFIX}"));
    if let Ok(meta) = fs::metadata(&disabled_path).await {
        if meta.len() == 0 {
            fs::remove_file(&disabled_path).await.map_err(|err| ApiError::internal(format!("failed to remove disable marker: {err}")))?;
        } else {
            let target = install.join(&name);
            move_upload(&disabled_path, &target).await?;
        }
        invalidate_registry_snapshot_file().await;
        return Ok(Json(PluginToggleResponse { name, enabled: true }));
    }

    if fs::metadata(install.join(&name)).await.is_ok() {
        return Ok(Json(PluginToggleResponse { name, enabled: true }));
    }

    let exists_elsewhere = plugin_dirs().into_iter().filter(|dir| dir != &install).any(|dir| dir.join(&name).exists());
    if exists_elsewhere {
        return Ok(Json(PluginToggleResponse { name, enabled: true }));
    }
    Err(ApiError::not_found("plugin not found"))
}

#[utoipa::path(
    get,
    path = "/plugins/disabled/{name}",
    tag = "Plugins",
    params(("name" = String, Path, description = "Disabled plugin filename")),
    responses((status = 200, description = "Download disabled plugin", content_type = "application/octet-stream"), (status = 404, description = "Not found", body = ErrorBody))
)]
pub(crate) async fn download_disabled(Path(name): Path<String>) -> ApiResult<Response> {
    let name = sanitize_name(&name).ok_or_else(|| ApiError::bad_request("invalid filename"))?;
    ensure_plugin_filename(&name)?;
    let path = install_dir().join(format!("{name}{DISABLED_SUFFIX_SUFFIX}"));
    stream_file(&path, &format!("{name}{DISABLED_SUFFIX_SUFFIX}")).await
}
