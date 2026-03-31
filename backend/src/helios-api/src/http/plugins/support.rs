use axum::{
    body::Body,
    response::{IntoResponse, Response},
};
use daedalus::ffi::{FFI_VERSION, PLUGIN_ABI_VERSION, PluginLibrary};
use helios_engine::ipc::PluginCompatibility;
use mime_guess::MimeGuess;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use tokio::{fs, io::AsyncWriteExt};
use tokio_util::io::ReaderStream;
use uuid::Uuid;

use crate::http::error::{ApiError, ApiResult};
use crate::http::storage::sanitize_name;
use crate::http::upload_integrity;

use super::types::{PluginFile, PluginUploadResponse};

pub(crate) const PLUGIN_SUFFIX: &str = ".so";
pub(crate) const DISABLED_SUFFIX_SUFFIX: &str = ".disabled";
pub(crate) const DISABLED_SUFFIX: &str = ".so.disabled";

pub(crate) async fn list_plugins_with_suffix(dir: &Path, suffix: &str) -> ApiResult<Vec<PluginFile>> {
    let mut entries = match fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => {
            return Err(ApiError::internal(format!("failed to read plugin directory: {err}")));
        }
    };

    let mut out = Vec::new();
    loop {
        let entry = match entries.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(err) => {
                return Err(ApiError::internal(format!("failed to read plugin entry: {err}")));
            }
        };
        let meta = match entry.metadata().await {
            Ok(meta) if meta.is_file() => meta,
            Ok(_) => continue,
            Err(err) => {
                return Err(ApiError::internal(format!("failed to stat plugin file: {err}")));
            }
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

pub(crate) async fn resolve_plugin_path(dirs: &[PathBuf], name: &str, disabled: bool) -> Option<PathBuf> {
    let filename = if disabled { format!("{name}{DISABLED_SUFFIX_SUFFIX}") } else { name.to_string() };
    for dir in dirs {
        let candidate = dir.join(&filename);
        if fs::metadata(&candidate).await.is_ok() {
            return Some(candidate);
        }
    }
    None
}

fn plugin_diag_base(filename: &str, path: &Path) -> PluginCompatibility {
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

pub(crate) fn plugin_diag_from_path(filename: &str, path: &Path, disabled: bool) -> PluginCompatibility {
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
                diag.plugin_name = info.plugin_name.as_str().map(|value| value.to_string());
                diag.plugin_version = info.plugin_version.as_str().map(|value| value.to_string());
                diag.ffi_version = info.ffi_version.as_str().map(|value| value.to_string());
                diag.daedalus_version = info.daedalus_version.as_str().map(|value| value.to_string());
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

pub(crate) struct UploadedTemp {
    pub(crate) name: String,
    pub(crate) temp_path: PathBuf,
    pub(crate) size_bytes: u64,
    pub(crate) sha256: String,
}

pub(crate) async fn process_upload_field(mut field: axum::extract::multipart::Field<'_>, limit: u64, expected_upload_bytes: Option<u64>) -> ApiResult<UploadedTemp> {
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

pub(crate) async fn store_upload(source: &Path, target: &Path) -> ApiResult<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).await.map_err(|err| ApiError::internal(format!("failed to create plugin directory: {err}")))?;
    }
    if let Err(err) = fs::rename(source, target).await {
        #[cfg(unix)]
        let is_cross_device = err.raw_os_error() == Some(18);
        #[cfg(not(unix))]
        let is_cross_device = false;
        if !is_cross_device {
            return Err(ApiError::internal(format!("failed to store plugin: {err}")));
        }
        fs::copy(source, target).await.map_err(|copy_err| ApiError::internal(format!("failed to copy plugin: {copy_err}")))?;
    }
    Ok(())
}

pub(crate) async fn move_upload(source: &Path, target: &Path) -> ApiResult<()> {
    store_upload(source, target).await?;
    let _ = fs::remove_file(source).await;
    Ok(())
}

pub(crate) async fn stream_file(path: &Path, name: &str) -> ApiResult<Response> {
    let file = fs::File::open(path).await.map_err(|err| map_io_error(err, "failed to open plugin"))?;
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);
    let content_type = guess_content_type(name);
    let headers = [(axum::http::header::CONTENT_TYPE, content_type.as_str()), (axum::http::header::CONTENT_DISPOSITION, &format!("attachment; filename=\"{name}\""))];
    Ok((headers, body).into_response())
}

pub(crate) fn guess_content_type(name: &str) -> String {
    MimeGuess::from_path(name).first_or_octet_stream().to_string()
}

pub(crate) fn install_dir() -> PathBuf {
    std::env::var("HELIOS_DAEDALUS_PLUGIN_INSTALL_DIR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/var/lib/helios/plugins/daedalus"))
}

pub(crate) fn plugin_dirs() -> Vec<PathBuf> {
    if let Ok(single) = std::env::var("HELIOS_DAEDALUS_PLUGIN_DIR") {
        let trimmed = single.trim();
        if !trimmed.is_empty() {
            return vec![PathBuf::from(trimmed)];
        }
    }
    if let Ok(list) = std::env::var("HELIOS_DAEDALUS_PLUGIN_DIRS") {
        let dirs: Vec<_> = std::env::split_paths(&list).collect();
        if !dirs.is_empty() {
            return dirs;
        }
    }
    vec![install_dir(), PathBuf::from("/usr/lib/helios/plugins/daedalus")]
}

pub(crate) fn upload_dir() -> PathBuf {
    std::env::var("HELIOS_DAEDALUS_PLUGIN_UPLOAD_DIR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/var/lib/helios/plugins/uploads"))
}

pub(crate) async fn ensure_install_dir() -> ApiResult<PathBuf> {
    let dir = install_dir();
    fs::create_dir_all(&dir).await.map_err(|err| ApiError::internal(format!("failed to create plugin dir: {err}")))?;
    Ok(dir)
}

pub(crate) async fn ensure_upload_dir() -> ApiResult<PathBuf> {
    let dir = upload_dir();
    fs::create_dir_all(&dir).await.map_err(|err| ApiError::internal(format!("failed to create upload dir: {err}")))?;
    Ok(dir)
}

pub(crate) async fn invalidate_registry_snapshot_file() {
    let path = std::env::var("HELIOS_NODE_REGISTRY_SNAPSHOT_PATH").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/var/lib/helios/state/node-registry.snapshot.json"));
    let _ = fs::remove_file(path).await;
}

pub(crate) fn ensure_plugin_filename(name: &str) -> Result<(), Box<ApiError>> {
    if !name.ends_with(".so") {
        return Err(Box::new(ApiError::bad_request("plugin must be a .so file")));
    }
    Ok(())
}

pub(crate) fn map_io_error(err: std::io::Error, context: &str) -> ApiError {
    match err.kind() {
        std::io::ErrorKind::NotFound => ApiError::not_found("plugin not found"),
        _ => ApiError::internal(format!("{context}: {err}")),
    }
}

pub(crate) fn max_upload_bytes() -> u64 {
    const DEFAULT_MB: u64 = 64;
    std::env::var("HELIOS_API_MAX_PLUGIN_MB").ok().and_then(|raw| raw.parse::<u64>().ok()).filter(|value| *value > 0).map(|mb| mb.saturating_mul(1024 * 1024)).unwrap_or(DEFAULT_MB * 1024 * 1024)
}

pub(crate) fn build_compatibility_map(compatibility: Vec<PluginCompatibility>) -> BTreeMap<String, PluginCompatibility> {
    let mut compatibility_map = BTreeMap::new();
    for entry in compatibility {
        compatibility_map.insert(entry.filename.clone(), entry);
    }
    compatibility_map
}

pub(crate) fn finalize_upload_response(upload: UploadedTemp) -> PluginUploadResponse {
    PluginUploadResponse { name: upload.name, size_bytes: upload.size_bytes, sha256: upload.sha256 }
}
