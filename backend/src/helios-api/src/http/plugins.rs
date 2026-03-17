use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Multipart, Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use daedalus::ffi::{FFI_VERSION, PLUGIN_ABI_VERSION, PluginLibrary};
use helios_engine::ipc::PluginCompatibility;
use mime_guess::MimeGuess;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path as StdPath, PathBuf};
use std::time::Duration;
use tokio::{fs, io::AsyncWriteExt};
use tokio_util::io::ReaderStream;
use utoipa::ToSchema;
use uuid::Uuid;

use super::AppState;
use super::error::{ApiError, ApiResult, ErrorBody};
use super::storage::sanitize_name;
use super::upload_integrity;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_plugins))
        .route("/upload", post(upload_plugin))
        .route("/install", post(install_plugin))
        .route("/disabled/{name}", get(download_disabled))
        .route("/uploads/{name}", get(download_upload))
        .route("/{name}/disable", post(disable_plugin))
        .route("/{name}/enable", post(enable_plugin))
        .route("/{name}", get(download_plugin).delete(delete_plugin))
        .route_layer(DefaultBodyLimit::disable())
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PluginFile {
    pub name: String,
    pub size_bytes: u64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PluginListResponse {
    pub installed: Vec<PluginFile>,
    pub disabled: Vec<PluginFile>,
    pub uploads: Vec<PluginFile>,
    pub engine_available: bool,
    pub compatibility: Vec<PluginCompatibility>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PluginUploadResponse {
    pub name: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PluginInstallRequest {
    #[serde(default)]
    pub upload_name: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PluginInstallResponse {
    pub name: String,
    pub installed: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PluginToggleResponse {
    pub name: String,
    pub enabled: bool,
}

#[utoipa::path(
    get,
    path = "/plugins",
    tag = "Plugins",
    responses((status = 200, description = "List plugins", body = PluginListResponse))
)]
async fn list_plugins(State(state): State<AppState>) -> ApiResult<impl IntoResponse> {
    let mut dirs = plugin_dirs();
    let install = install_dir();
    dirs.retain(|dir| dir != &install);
    dirs.insert(0, install);
    let mut installed_by_name = std::collections::BTreeMap::<String, PluginFile>::new();
    let mut disabled_by_name = std::collections::BTreeMap::<String, PluginFile>::new();
    for dir in &dirs {
        for file in list_plugins_with_suffix(dir, PLUGIN_SUFFIX).await? {
            installed_by_name.entry(file.name.clone()).or_insert(file);
        }
        for file in list_plugins_with_suffix(dir, DISABLED_SUFFIX).await? {
            disabled_by_name.entry(file.name.clone()).or_insert(file);
        }
    }

    let mut disabled = Vec::new();
    for (name, file) in disabled_by_name {
        if installed_by_name.contains_key(&name) || file.size_bytes > 0 {
            disabled.push(file);
        }
    }

    let mut installed = Vec::new();
    for (name, file) in installed_by_name {
        if disabled.iter().any(|disabled| disabled.name == name) {
            continue;
        }
        installed.push(file);
    }

    let uploads = list_plugins_with_suffix(&upload_dir(), PLUGIN_SUFFIX).await?;

    const IPC_TIMEOUT: Duration = Duration::from_secs(3);
    let engine_available = state.engine.list_streams_with_timeout(IPC_TIMEOUT).await.is_ok();
    let mut compatibility = Vec::new();
    if let Ok(snapshot) = state.services.pipelines.load_registry_snapshot_from_disk_or_helper().await {
        compatibility = snapshot.plugin_compatibility;
    }

    let mut compatibility_map: BTreeMap<String, PluginCompatibility> = BTreeMap::new();
    for entry in compatibility {
        compatibility_map.insert(entry.filename.clone(), entry);
    }

    for plugin in &installed {
        if compatibility_map.contains_key(&plugin.name) {
            continue;
        }
        if let Some(path) = resolve_plugin_path(&dirs, &plugin.name, false).await {
            let diag = plugin_diag_from_path(&plugin.name, &path, false);
            compatibility_map.insert(plugin.name.clone(), diag);
        }
    }
    for plugin in &disabled {
        if compatibility_map.contains_key(&plugin.name) {
            continue;
        }
        if let Some(path) = resolve_plugin_path(&dirs, &plugin.name, true).await {
            let diag = plugin_diag_from_path(&plugin.name, &path, true);
            compatibility_map.insert(plugin.name.clone(), diag);
        }
    }

    let compatibility = compatibility_map.into_values().collect();

    Ok(Json(PluginListResponse { installed, disabled, uploads, engine_available, compatibility }))
}

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
async fn upload_plugin(headers: HeaderMap, mut multipart: Multipart) -> ApiResult<impl IntoResponse> {
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
        uploaded = Some(PluginUploadResponse { name: upload.name, size_bytes: upload.size_bytes, sha256: upload.sha256 });
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
async fn install_plugin(Json(payload): Json<PluginInstallRequest>) -> ApiResult<impl IntoResponse> {
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
async fn download_plugin(Path(name): Path<String>) -> ApiResult<Response> {
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
async fn download_upload(Path(name): Path<String>) -> ApiResult<Response> {
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
async fn delete_plugin(Path(name): Path<String>) -> ApiResult<impl IntoResponse> {
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
async fn disable_plugin(Path(name): Path<String>) -> ApiResult<impl IntoResponse> {
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
async fn enable_plugin(Path(name): Path<String>) -> ApiResult<impl IntoResponse> {
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
async fn download_disabled(Path(name): Path<String>) -> ApiResult<Response> {
    let name = sanitize_name(&name).ok_or_else(|| ApiError::bad_request("invalid filename"))?;
    ensure_plugin_filename(&name)?;
    let path = install_dir().join(format!("{name}{DISABLED_SUFFIX_SUFFIX}"));
    stream_file(&path, &format!("{name}{DISABLED_SUFFIX_SUFFIX}")).await
}

async fn list_plugins_with_suffix(dir: &std::path::Path, suffix: &str) -> ApiResult<Vec<PluginFile>> {
    let mut entries = match fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(ApiError::internal(format!("failed to read plugin directory: {err}"))),
    };

    let mut out = Vec::new();
    loop {
        let entry = match entries.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(err) => return Err(ApiError::internal(format!("failed to read plugin entry: {err}"))),
        };
        let meta = match entry.metadata().await {
            Ok(meta) if meta.is_file() => meta,
            Ok(_) => continue,
            Err(err) => return Err(ApiError::internal(format!("failed to stat plugin file: {err}"))),
        };
        let name = entry.file_name().to_string_lossy().to_string();
        if name.ends_with(suffix) {
            let display = if suffix == DISABLED_SUFFIX { name.trim_end_matches(DISABLED_SUFFIX_SUFFIX).to_string() } else { name };
            out.push(PluginFile { name: display, size_bytes: meta.len() });
        }
    }

    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

async fn resolve_plugin_path(dirs: &[PathBuf], name: &str, disabled: bool) -> Option<PathBuf> {
    let filename = if disabled { format!("{name}{DISABLED_SUFFIX_SUFFIX}") } else { name.to_string() };
    for dir in dirs {
        let candidate = dir.join(&filename);
        if fs::metadata(&candidate).await.is_ok() {
            return Some(candidate);
        }
    }
    None
}

fn plugin_diag_base(filename: &str, path: &StdPath) -> PluginCompatibility {
    PluginCompatibility {
        filename: filename.to_string(),
        plugin_name: None,
        plugin_version: None,
        status: "ok".to_string(),
        reason: None,
        expected_daedalus_version: daedalus::version().to_string(),
        daedalus_version: None,
        expected_ffi_version: FFI_VERSION.to_string(),
        ffi_version: None,
        expected_abi_version: PLUGIN_ABI_VERSION,
        abi_version: None,
        path: path.display().to_string(),
    }
}

fn plugin_diag_from_path(filename: &str, path: &StdPath, disabled: bool) -> PluginCompatibility {
    let mut diag = plugin_diag_base(filename, path);
    #[allow(unsafe_code)]
    match unsafe { PluginLibrary::load(path) } {
        Ok(lib) => {
            diag.abi_version = lib.abi_version();
            if diag.abi_version.is_none() {
                diag.status = "missing_abi".to_string();
                diag.reason = Some("missing ABI version symbol".to_string());
            } else if diag.abi_version != Some(PLUGIN_ABI_VERSION) {
                diag.status = "abi_mismatch".to_string();
                diag.reason = Some(format!("ABI mismatch (expected {}, got {})", PLUGIN_ABI_VERSION, diag.abi_version.unwrap_or_default()));
            }

            if let Some(info) = lib.info() {
                diag.plugin_name = info.plugin_name.as_str().map(|v| v.to_string());
                diag.plugin_version = info.plugin_version.as_str().map(|v| v.to_string());
                diag.ffi_version = info.ffi_version.as_str().map(|v| v.to_string());
                diag.daedalus_version = info.daedalus_version.as_str().map(|v| v.to_string());
            } else if diag.status == "ok" {
                diag.status = "missing_info".to_string();
                diag.reason = Some("missing plugin info symbol".to_string());
            }

            if diag.status == "ok" && diag.ffi_version.as_deref() != Some(FFI_VERSION) {
                let actual = diag.ffi_version.clone().unwrap_or_else(|| "unknown".to_string());
                diag.status = "ffi_mismatch".to_string();
                diag.reason = Some(format!("FFI mismatch (expected {FFI_VERSION}, got {actual})"));
            }

            if diag.status == "ok" && diag.daedalus_version.as_deref() != Some(daedalus::version()) {
                let actual = diag.daedalus_version.clone().unwrap_or_else(|| "unknown".to_string());
                diag.status = "daedalus_mismatch".to_string();
                diag.reason = Some(format!("Daedalus version mismatch (expected {}, got {})", daedalus::version(), actual));
            }
        }
        Err(err) => {
            diag.status = "load_failed".to_string();
            diag.reason = Some(err.to_string());
        }
    }

    if disabled && diag.status == "ok" {
        diag.status = "disabled".to_string();
        diag.reason = Some("plugin disabled".to_string());
    }
    diag
}

struct UploadedTemp {
    name: String,
    temp_path: std::path::PathBuf,
    size_bytes: u64,
    sha256: String,
}

async fn process_upload_field(mut field: axum::extract::multipart::Field<'_>, limit: u64, expected_upload_bytes: Option<u64>) -> ApiResult<UploadedTemp> {
    let filename = field.file_name().and_then(sanitize_name).ok_or_else(|| ApiError::bad_request("upload missing filename"))?;
    ensure_plugin_filename(&filename)?;
    let temp_path = std::env::temp_dir().join(format!("helios-plugin-{}", Uuid::new_v4()));
    let mut file = fs::File::create(&temp_path).await.map_err(|err| ApiError::internal(format!("failed to create upload: {err}")))?;
    let mut hasher = Sha256::new();
    let mut written: u64 = 0;

    while let Some(chunk) = match field.chunk().await {
        Ok(chunk) => chunk,
        Err(err) => {
            let _ = fs::remove_file(&temp_path).await;
            return Err(ApiError::bad_request(format!("failed to read upload: {err}")));
        }
    } {
        written += chunk.len() as u64;
        if written > limit {
            let _ = fs::remove_file(&temp_path).await;
            return Err(ApiError::payload_too_large(format!("upload exceeds limit of {} bytes", limit)));
        }
        if let Err(err) = file.write_all(&chunk).await {
            let _ = fs::remove_file(&temp_path).await;
            return Err(ApiError::internal(format!("failed to write upload: {err}")));
        }
        hasher.update(&chunk);
    }

    if let Err(err) = upload_integrity::validate_expected_upload_bytes(written, expected_upload_bytes) {
        let _ = fs::remove_file(&temp_path).await;
        return Err(ApiError::bad_request(err));
    }
    let stored_bytes = match upload_integrity::finalize_file_upload(&mut file, &temp_path, written).await {
        Ok(stored_bytes) => stored_bytes,
        Err(err) => {
            let _ = fs::remove_file(&temp_path).await;
            return Err(ApiError::internal(format!("failed to finish upload: {err}")));
        }
    };
    if let Err(err) = upload_integrity::validate_expected_upload_bytes(stored_bytes, expected_upload_bytes) {
        let _ = fs::remove_file(&temp_path).await;
        return Err(ApiError::bad_request(err));
    }
    let sha256 = hex::encode(hasher.finalize());
    Ok(UploadedTemp { name: filename, temp_path, size_bytes: stored_bytes, sha256 })
}

async fn store_upload(source: &std::path::Path, target: &std::path::Path) -> ApiResult<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).await.map_err(|err| ApiError::internal(format!("failed to create plugin directory: {err}")))?;
    }
    if let Err(err) = fs::rename(source, target).await {
        #[cfg(unix)]
        let is_cross_device = err.raw_os_error() == Some(18); // EXDEV
        #[cfg(not(unix))]
        let is_cross_device = false;
        if !is_cross_device {
            return Err(ApiError::internal(format!("failed to store plugin: {err}")));
        }
        fs::copy(source, target).await.map_err(|copy_err| ApiError::internal(format!("failed to copy plugin: {copy_err}")))?;
    }
    Ok(())
}

async fn move_upload(source: &std::path::Path, target: &std::path::Path) -> ApiResult<()> {
    store_upload(source, target).await?;
    let _ = fs::remove_file(source).await;
    Ok(())
}

async fn stream_file(path: &std::path::Path, name: &str) -> ApiResult<Response> {
    let file = fs::File::open(path).await.map_err(|err| map_io_error(err, "failed to open plugin"))?;
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);
    let content_type = guess_content_type(name);
    let headers = [(header::CONTENT_TYPE, content_type.as_str()), (header::CONTENT_DISPOSITION, &format!("attachment; filename=\"{name}\""))];
    Ok((headers, body).into_response())
}

fn guess_content_type(name: &str) -> String {
    let guess = MimeGuess::from_path(name);
    guess.first_or_octet_stream().to_string()
}

fn install_dir() -> std::path::PathBuf {
    std::env::var("HELIOS_DAEDALUS_PLUGIN_INSTALL_DIR").map(std::path::PathBuf::from).unwrap_or_else(|_| std::path::PathBuf::from("/var/lib/helios/plugins/daedalus"))
}

fn plugin_dirs() -> Vec<std::path::PathBuf> {
    if let Ok(single) = std::env::var("HELIOS_DAEDALUS_PLUGIN_DIR") {
        let trimmed = single.trim();
        if !trimmed.is_empty() {
            return vec![std::path::PathBuf::from(trimmed)];
        }
    }
    if let Ok(list) = std::env::var("HELIOS_DAEDALUS_PLUGIN_DIRS") {
        let dirs: Vec<_> = std::env::split_paths(&list).collect();
        if !dirs.is_empty() {
            return dirs;
        }
    }
    vec![install_dir(), std::path::PathBuf::from("/usr/lib/helios/plugins/daedalus")]
}

fn upload_dir() -> std::path::PathBuf {
    std::env::var("HELIOS_DAEDALUS_PLUGIN_UPLOAD_DIR").map(std::path::PathBuf::from).unwrap_or_else(|_| std::path::PathBuf::from("/var/lib/helios/plugins/uploads"))
}

async fn ensure_install_dir() -> ApiResult<std::path::PathBuf> {
    let dir = install_dir();
    fs::create_dir_all(&dir).await.map_err(|err| ApiError::internal(format!("failed to create plugin dir: {err}")))?;
    Ok(dir)
}

async fn ensure_upload_dir() -> ApiResult<std::path::PathBuf> {
    let dir = upload_dir();
    fs::create_dir_all(&dir).await.map_err(|err| ApiError::internal(format!("failed to create upload dir: {err}")))?;
    Ok(dir)
}

async fn invalidate_registry_snapshot_file() {
    let path = std::env::var("HELIOS_NODE_REGISTRY_SNAPSHOT_PATH").map(std::path::PathBuf::from).unwrap_or_else(|_| std::path::PathBuf::from("/var/lib/helios/state/node-registry.snapshot.json"));
    let _ = fs::remove_file(path).await;
}

fn ensure_plugin_filename(name: &str) -> Result<(), Box<ApiError>> {
    if !name.ends_with(".so") {
        return Err(Box::new(ApiError::bad_request("plugin must be a .so file")));
    }
    Ok(())
}

const PLUGIN_SUFFIX: &str = ".so";
const DISABLED_SUFFIX_SUFFIX: &str = ".disabled";
const DISABLED_SUFFIX: &str = ".so.disabled";

fn map_io_error(err: std::io::Error, context: &str) -> ApiError {
    match err.kind() {
        std::io::ErrorKind::NotFound => ApiError::not_found("plugin not found"),
        _ => ApiError::internal(format!("{context}: {err}")),
    }
}

fn max_upload_bytes() -> u64 {
    const DEFAULT_MB: u64 = 64;
    std::env::var("HELIOS_API_MAX_PLUGIN_MB").ok().and_then(|raw| raw.parse::<u64>().ok()).filter(|v| *v > 0).map(|mb| mb.saturating_mul(1024 * 1024)).unwrap_or(DEFAULT_MB * 1024 * 1024)
}
