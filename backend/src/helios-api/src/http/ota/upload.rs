use super::*;

#[utoipa::path(
    post,
    path = "/ota/upload",
    tag = "OTA",
    request_body = String,
    responses(
        (status = 201, description = "Image uploaded", body = UploadUpdateResponse),
        (status = 400, description = "Invalid upload", body = UploadUpdateError),
        (status = 500, description = "Storage error", body = UploadUpdateError)
    )
)]
pub async fn upload_update(headers: HeaderMap, mut multipart: Multipart) -> impl IntoResponse {
    let upload_dir = match ota_storage::ensure_upload_dir() {
        Ok(dir) => dir,
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(UploadUpdateError { error: err.to_string() })).into_response(),
    };
    let expected_upload_bytes = match upload_integrity::expected_upload_bytes(&headers) {
        Ok(value) => value,
        Err(err) => return (StatusCode::BAD_REQUEST, Json(UploadUpdateError { error: err })).into_response(),
    };
    let upload_stats = match ota_storage::upload_storage_stats_for_dir(&upload_dir).await {
        Ok(stats) => stats,
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(UploadUpdateError { error: format!("failed to inspect OTA upload storage: {err}") })).into_response(),
    };
    if let Some(expected_bytes) = expected_upload_bytes
        && expected_bytes > upload_stats.remaining_bytes
    {
        return insufficient_storage_response(format!(
            "OTA upload store {} only has {} writable bytes remaining (usage {}, quota {}, fs available {}); upload needs {} bytes",
            upload_stats.root.display(),
            upload_stats.remaining_bytes,
            upload_stats.usage_bytes,
            upload_stats.quota_bytes,
            upload_stats.available_bytes,
            expected_bytes
        ));
    }

    let mut uploaded: Option<UploadedTemp> = None;

    loop {
        let next = match multipart.next_field().await {
            Ok(next) => next,
            Err(err) => return (StatusCode::BAD_REQUEST, Json(UploadUpdateError { error: format!("failed to read upload payload: {err}") })).into_response(),
        };
        let Some(field) = next else {
            break;
        };
        if let Some("file") = field.name() {
            if uploaded.is_some() {
                return (StatusCode::BAD_REQUEST, Json(UploadUpdateError { error: "only one file may be uploaded per request".into() })).into_response();
            }
            match process_upload_field(field, expected_upload_bytes, upload_stats.remaining_bytes).await {
                Ok(info) => uploaded = Some(info),
                Err(err) => {
                    let status = if err.contains("remaining") || err.contains("storage quota") { StatusCode::INSUFFICIENT_STORAGE } else { StatusCode::BAD_REQUEST };
                    return (status, Json(UploadUpdateError { error: err })).into_response();
                }
            }
        }
    }

    let upload = match uploaded {
        Some(info) => info,
        None => {
            return (StatusCode::BAD_REQUEST, Json(UploadUpdateError { error: "multipart payload missing file".into() })).into_response();
        }
    };

    let filename = match ota_storage::unique_upload_name(&upload_dir, &upload.filename).await {
        Ok(name) => name,
        Err(err) => {
            let _ = fs::remove_file(&upload.temp_path).await;
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(UploadUpdateError { error: err.to_string() })).into_response();
        }
    };
    let image_path = upload_dir.join(&filename);
    let post_upload_stats = match ota_storage::upload_storage_stats_for_dir(&upload_dir).await {
        Ok(stats) => stats,
        Err(err) => {
            let _ = fs::remove_file(&upload.temp_path).await;
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(UploadUpdateError { error: format!("failed to inspect OTA upload storage: {err}") })).into_response();
        }
    };
    if upload.size_bytes > post_upload_stats.remaining_bytes {
        let _ = fs::remove_file(&upload.temp_path).await;
        return insufficient_storage_response(format!(
            "OTA upload store {} only has {} writable bytes remaining (usage {}, quota {}, fs available {}); upload needs {} bytes",
            post_upload_stats.root.display(),
            post_upload_stats.remaining_bytes,
            post_upload_stats.usage_bytes,
            post_upload_stats.quota_bytes,
            post_upload_stats.available_bytes,
            upload.size_bytes
        ));
    }
    if let Some(parent) = image_path.parent() {
        let _ = fs::create_dir_all(parent).await;
    }
    if let Err(err) = fs::rename(&upload.temp_path, &image_path).await {
        // Temp uploads are written under /tmp but the data directory is typically on /var/lib/helios
        // which may be a separate filesystem. `rename` fails with EXDEV in that case, so fall back
        // to a copy+unlink.
        if err.kind() == std::io::ErrorKind::CrossesDevices {
            if let Err(copy_err) = fs::copy(&upload.temp_path, &image_path).await {
                let _ = fs::remove_file(&upload.temp_path).await;
                let status = if copy_err.kind() == std::io::ErrorKind::StorageFull { StatusCode::INSUFFICIENT_STORAGE } else { StatusCode::INTERNAL_SERVER_ERROR };
                return (status, Json(UploadUpdateError { error: format!("failed to store upload: {copy_err}") })).into_response();
            }
            let _ = fs::remove_file(&upload.temp_path).await;
        } else {
            let _ = fs::remove_file(&upload.temp_path).await;
            let status = if err.kind() == std::io::ErrorKind::StorageFull { StatusCode::INSUFFICIENT_STORAGE } else { StatusCode::INTERNAL_SERVER_ERROR };
            return (status, Json(UploadUpdateError { error: format!("failed to store upload: {err}") })).into_response();
        }
    }

    let image_url = match Url::from_file_path(&image_path) {
        Ok(url) => url.to_string(),
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(UploadUpdateError { error: "failed to build image url".into() })).into_response();
        }
    };

    let response = UploadUpdateResponse { filename, size_bytes: upload.size_bytes, sha256: upload.sha256, image_url, version: None, build_id: None };

    (StatusCode::CREATED, Json(response)).into_response()
}

#[derive(Debug)]
struct UploadedTemp {
    filename: String,
    temp_path: PathBuf,
    size_bytes: u64,
    sha256: String,
}

async fn process_upload_field(mut field: axum::extract::multipart::Field<'_>, expected_upload_bytes: Option<u64>, remaining_upload_bytes: u64) -> Result<UploadedTemp, String> {
    let filename = match field.file_name().and_then(sanitize_name) {
        Some(name) => name,
        None => return Err("upload missing filename".into()),
    };
    let temp_path = std::env::temp_dir().join(format!("helios-ota-{}", Uuid::new_v4()));
    let mut file = fs::File::create(&temp_path).await.map_err(|err| format!("failed to create upload: {err}"))?;
    let mut hasher = Sha256::new();
    let mut written: u64 = 0;
    let limit = storage_budget_bytes(max_ota_bytes(), expected_upload_bytes, remaining_upload_bytes);

    while let Some(chunk) = match field.chunk().await {
        Ok(chunk) => chunk,
        Err(err) => {
            let _ = fs::remove_file(&temp_path).await;
            return Err(format!("failed to read upload: {err}"));
        }
    } {
        written += chunk.len() as u64;
        if written > limit {
            let _ = fs::remove_file(&temp_path).await;
            return Err(format!("upload exceeds OTA storage quota: remaining writable bytes {}", limit));
        }
        if let Err(err) = file.write_all(&chunk).await {
            let _ = fs::remove_file(&temp_path).await;
            return Err(format!("failed to write upload: {err}"));
        }
        hasher.update(&chunk);
    }

    if let Err(err) = upload_integrity::validate_expected_upload_bytes(written, expected_upload_bytes) {
        let _ = fs::remove_file(&temp_path).await;
        return Err(err);
    }
    let stored_bytes = match upload_integrity::finalize_file_upload(&mut file, &temp_path, written).await {
        Ok(stored_bytes) => stored_bytes,
        Err(err) => {
            let _ = fs::remove_file(&temp_path).await;
            return Err(format!("failed to finish upload: {err}"));
        }
    };
    if let Err(err) = upload_integrity::validate_expected_upload_bytes(stored_bytes, expected_upload_bytes) {
        let _ = fs::remove_file(&temp_path).await;
        return Err(err);
    }
    let sha256 = hex::encode(hasher.finalize());
    Ok(UploadedTemp { filename, temp_path, size_bytes: stored_bytes, sha256 })
}

fn storage_budget_bytes(max_upload_bytes: u64, expected_upload_bytes: Option<u64>, remaining_upload_bytes: u64) -> u64 {
    let quota_cap = remaining_upload_bytes.min(max_upload_bytes);
    expected_upload_bytes.map(|bytes| bytes.min(quota_cap)).unwrap_or(quota_cap)
}

fn insufficient_storage_response(error: String) -> axum::response::Response {
    (StatusCode::INSUFFICIENT_STORAGE, Json(UploadUpdateError { error })).into_response()
}

fn max_ota_bytes() -> u64 {
    const DEFAULT_MB: u64 = 2 * 1024; // 2GB default
    env::var("HELIOS_OTA_MAX_UPLOAD_MB").ok().and_then(|raw| raw.parse::<u64>().ok()).filter(|v| *v > 0).map(|mb| mb.saturating_mul(1024 * 1024)).unwrap_or(DEFAULT_MB * 1024 * 1024)
}
