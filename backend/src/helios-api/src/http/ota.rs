use std::{env, path::PathBuf};

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Multipart, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use chrono::Utc;
use helios_updater::ipc::{MaintenanceWindow, PreflightReport, UpdateStage, UpdateState, UpdaterCommand, UpdaterStorageReport};
use helios_updater::{ManifestArtifact, ReleaseManifest};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::time::{Duration, Instant, sleep};
use tracing::warn;
use url::Url;
use utoipa::ToSchema;
use uuid::Uuid;

use super::AppState;
use super::ota_storage;
use super::storage::sanitize_name;
use super::upload_integrity;
use crate::ipc::command_id_from_context;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/upload", post(upload_update))
        .route("/stage", post(stage_update))
        .route("/preflight", post(preflight_update))
        .route("/apply", post(apply_update))
        .route("/cancel", post(cancel_update))
        .route("/state", get(updater_state))
        // OTA images are large; disable the default 2MB body cap so uploads don't get rejected.
        .route_layer(DefaultBodyLimit::disable())
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UploadUpdateResponse {
    pub filename: String,
    pub size_bytes: u64,
    pub sha256: String,
    pub image_url: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub build_id: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UploadUpdateError {
    pub error: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct StageUpdateRequest {
    pub image_url: String,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub checksum: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UpdateAckResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_id: Option<String>,
    #[serde(default)]
    pub message: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct ApplyUpdateRequest {
    #[serde(default)]
    pub requested_by: Option<String>,
    #[serde(default)]
    pub artifact_kind: Option<ApplyArtifactKind>,
    #[serde(default)]
    pub image_url: Option<String>,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub checksum: Option<String>,
    #[serde(default = "default_true")]
    pub delete_image_after_apply: bool,
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PreflightUpdateRequest {
    #[serde(default)]
    pub update_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ApplyArtifactKind {
    DiskImage,
    FrontendBundle,
    ServiceBundle,
}

impl ApplyArtifactKind {
    fn manifest_kind(self) -> &'static str {
        match self {
            Self::DiskImage => "disk-image",
            Self::FrontendBundle => "frontend-bundle",
            Self::ServiceBundle => "service-bundle",
        }
    }

    fn requires_stream_shutdown(self) -> bool {
        match self {
            Self::DiskImage => true,
            Self::FrontendBundle => false,
            Self::ServiceBundle => false,
        }
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct CancelUpdateRequest {
    /// Optional update id to cancel. When omitted, the currently active update is canceled.
    #[serde(default)]
    pub update_id: Option<String>,
    #[serde(default)]
    pub requested_by: Option<String>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct UpdateStateResponse {
    #[serde(default)]
    pub state: Option<Value>,
    pub cache_usage_bytes: u64,
    pub storage: OtaStorageReport,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OtaStorageReport {
    pub uploads: OtaUploadStorageReport,
    pub cache: OtaDirectoryStorageReport,
    pub work: OtaDirectoryStorageReport,
    pub service_releases: OtaDirectoryStorageReport,
    pub frontend_releases: OtaDirectoryStorageReport,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OtaUploadStorageReport {
    pub path: String,
    pub usage_bytes: u64,
    pub quota_bytes: u64,
    pub available_bytes: u64,
    pub remaining_bytes: u64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct OtaDirectoryStorageReport {
    pub path: String,
    #[serde(default)]
    pub usage_bytes: u64,
    #[serde(default)]
    pub available_bytes: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct PreflightUpdateResponse {
    pub update_id: String,
    pub ready: bool,
    pub report: PreflightReport,
}

#[derive(Debug, Serialize)]
pub struct ApplyConflictResponse {
    pub update_id: String,
    pub error: String,
    pub preflight: PreflightReport,
}

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

#[utoipa::path(
    post,
    path = "/ota/stage",
    tag = "OTA",
    request_body = StageUpdateRequest,
    responses(
        (status = 410, description = "Staging removed", body = UploadUpdateError)
    )
)]
pub async fn stage_update(State(_state): State<AppState>, Json(payload): Json<StageUpdateRequest>) -> impl IntoResponse {
    let _ = (&payload.image_url, payload.size_bytes, payload.checksum.as_deref());
    (StatusCode::GONE, Json(UploadUpdateError { error: "staging removed: use /ota/apply with image_url".into() })).into_response()
}

#[utoipa::path(
    post,
    path = "/ota/apply",
    tag = "OTA",
    request_body = ApplyUpdateRequest,
    responses(
        (status = 200, description = "Apply scheduled", body = UpdateAckResponse),
        (status = 400, description = "Invalid request", body = UploadUpdateError),
        (status = 503, description = "Updater unavailable", body = UploadUpdateError)
    )
)]
pub async fn apply_update(State(state): State<AppState>, Json(payload): Json<ApplyUpdateRequest>) -> impl IntoResponse {
    let _ = payload.requested_by.as_deref();
    let artifact_kind = payload.artifact_kind.unwrap_or(ApplyArtifactKind::DiskImage);
    let Some(image_url) = payload.image_url.as_deref() else {
        return (StatusCode::BAD_REQUEST, Json(UploadUpdateError { error: "image_url is required (staging removed)".into() })).into_response();
    };
    let update_id = match stage_update_for_manual_apply(&state, image_url, payload.size_bytes, payload.checksum.as_deref(), payload.delete_image_after_apply, artifact_kind).await {
        Ok(update_id) => update_id,
        Err(err) => return err.into_response(),
    };
    if let Err(err) = wait_for_staged_update(&state, update_id).await {
        let _ = cancel_update_by_id(&state, update_id).await;
        return err.into_response();
    }
    let report = match fetch_updater_preflight(&state, update_id).await {
        Ok(report) => report,
        Err(err) => {
            let _ = cancel_update_by_id(&state, update_id).await;
            return err.into_response();
        }
    };
    if !report.ready {
        let _ = cancel_update_by_id(&state, update_id).await;
        return (StatusCode::CONFLICT, Json(ApplyConflictResponse { update_id: update_id.to_string(), error: report.summary.clone(), preflight: report })).into_response();
    }
    let stopped_streams = if artifact_kind.requires_stream_shutdown() { stop_streams_for_update(&state).await.unwrap_or(0) } else { 0 };
    if let Err(err) = send_apply_release(&state, update_id).await {
        return err.into_response();
    }
    let message = if stopped_streams > 0 { format!("apply scheduled (stopped {} stream{})", stopped_streams, if stopped_streams == 1 { "" } else { "s" }) } else { "apply scheduled".to_string() };
    (StatusCode::OK, Json(UpdateAckResponse { update_id: Some(update_id.to_string()), message })).into_response()
}

pub async fn preflight_update(State(state): State<AppState>, Json(payload): Json<PreflightUpdateRequest>) -> impl IntoResponse {
    let update_id = if let Some(raw) = payload.update_id.as_deref() {
        match Uuid::parse_str(raw.trim()) {
            Ok(id) => id,
            Err(_) => return (StatusCode::BAD_REQUEST, Json(UploadUpdateError { error: "invalid update_id".into() })).into_response(),
        }
    } else {
        let updater_state = match fetch_updater_state(&state).await {
            Ok((state, _)) => state,
            Err(err) => return err.into_response(),
        };
        match updater_state {
            Some(active) => active.update_id,
            None => return (StatusCode::BAD_REQUEST, Json(UploadUpdateError { error: "no staged update is active".into() })).into_response(),
        }
    };

    match fetch_updater_preflight(&state, update_id).await {
        Ok(report) => (StatusCode::OK, Json(PreflightUpdateResponse { update_id: update_id.to_string(), ready: report.ready, report })).into_response(),
        Err(err) => err.into_response(),
    }
}

#[utoipa::path(
    post,
    path = "/ota/cancel",
    tag = "OTA",
    request_body = CancelUpdateRequest,
    responses(
        (status = 200, description = "Update canceled", body = UpdateAckResponse),
        (status = 400, description = "Invalid request", body = UploadUpdateError),
        (status = 503, description = "Updater unavailable", body = UploadUpdateError)
    )
)]
pub async fn cancel_update(State(state): State<AppState>, Json(payload): Json<CancelUpdateRequest>) -> impl IntoResponse {
    let _ = payload.requested_by.as_deref();

    let update_id = if let Some(raw) = payload.update_id.as_deref() {
        match Uuid::parse_str(raw.trim()) {
            Ok(id) => id,
            Err(_) => return (StatusCode::BAD_REQUEST, Json(UploadUpdateError { error: "invalid update_id".into() })).into_response(),
        }
    } else {
        let updater_state = match fetch_updater_state(&state).await {
            Ok((state, _)) => state,
            Err(err) => return err.into_response(),
        };
        match updater_state {
            Some(active) => active.update_id,
            None => return (StatusCode::BAD_REQUEST, Json(UploadUpdateError { error: "no staged update is active".into() })).into_response(),
        }
    };

    let command = UpdaterCommand::Cancel { command_id: command_id_from_context("ota_cancel"), update_id };
    match state.services.updater.send_updater_command(&state, command, true).await {
        Ok(_) => (StatusCode::OK, Json(UpdateAckResponse { update_id: Some(update_id.to_string()), message: "update canceled".into() })).into_response(),
        Err(error) => UploadUpdateError { error }.into_response(),
    }
}

async fn stop_streams_for_update(state: &AppState) -> Option<usize> {
    let streams = match state.engine.list_streams().await {
        Ok(streams) => streams,
        Err(err) => {
            warn!(error = %err, "failed to list streams before OTA apply");
            return None;
        }
    };
    let mut stopped = 0usize;
    for stream in streams {
        let _ = state.engine.stop_stream(stream.stream_id).await;
        stopped += 1;
    }
    if stopped > 0 {
        warn!(stopped, "stopped streams before OTA apply");
    }
    Some(stopped)
}

async fn stage_update_for_manual_apply(
    state: &AppState,
    image_url: &str,
    size_bytes: Option<u64>,
    checksum: Option<&str>,
    delete_image_after_apply: bool,
    artifact_kind: ApplyArtifactKind,
) -> Result<Uuid, UploadUpdateError> {
    let image_url = Url::parse(image_url.trim()).map_err(|_| UploadUpdateError { error: "invalid image_url".into() })?;

    let mut artifact =
        ManifestArtifact { url: image_url.clone(), filename: None, size_bytes, sha256: checksum.map(|s| s.to_string()), signature: None, kind: Some(artifact_kind.manifest_kind().into()) };
    if image_url.scheme() == "file" {
        let Ok(path) = image_url.to_file_path() else {
            return Err(UploadUpdateError { error: "invalid image path".into() });
        };
        if artifact.size_bytes.is_none()
            && let Ok(meta) = fs::metadata(&path).await
        {
            artifact.size_bytes = Some(meta.len());
        }
    }

    let update_id = Uuid::new_v4();
    let source_artifact_path = ota_storage::source_upload_path_for_image_url(&image_url).await;
    let metadata_json = serde_json::json!({
        "auto_apply": false,
        "delete_image_after_apply": delete_image_after_apply,
        "source_artifact_path": source_artifact_path,
    })
    .to_string();
    let manifest = ReleaseManifest { update_id: Some(update_id), version: None, artifacts: vec![artifact], metadata_json };
    let command = UpdaterCommand::StageRelease { command_id: command_id_from_context("ota_stage_apply"), update_id, manifest };
    state.services.updater.send_updater_command(state, command, true).await.map_err(|error| UploadUpdateError { error })?;
    Ok(update_id)
}

async fn wait_for_staged_update(state: &AppState, update_id: Uuid) -> Result<UpdateState, UploadUpdateError> {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let (active, _) = fetch_updater_state(state).await?;
        if let Some(active) = active {
            if active.update_id != update_id {
                return Err(UploadUpdateError { error: format!("different update {} became active while waiting for {}", active.update_id, update_id) });
            }
            match active.stage {
                UpdateStage::AwaitingWindow => return Ok(active),
                UpdateStage::RolledBack => {
                    return Err(UploadUpdateError { error: active.last_error.unwrap_or_else(|| format!("staging update {} failed", update_id)) });
                }
                UpdateStage::Applying | UpdateStage::Rebooting | UpdateStage::Complete => {
                    return Err(UploadUpdateError { error: format!("update {} unexpectedly entered {:?} before apply was authorized", update_id, active.stage) });
                }
                _ => {}
            }
        }
        if Instant::now() >= deadline {
            return Err(UploadUpdateError { error: format!("timed out waiting for staged update {}", update_id) });
        }
        sleep(Duration::from_millis(250)).await;
    }
}

async fn fetch_updater_preflight(state: &AppState, update_id: Uuid) -> Result<PreflightReport, UploadUpdateError> {
    state.services.updater.fetch_updater_preflight(state, update_id).await.map_err(|error| UploadUpdateError { error })
}

async fn send_apply_release(state: &AppState, update_id: Uuid) -> Result<(), UploadUpdateError> {
    let command =
        UpdaterCommand::ApplyRelease { command_id: command_id_from_context("ota_apply_release"), update_id, window: MaintenanceWindow { start: Utc::now(), duration: Duration::from_secs(1) } };
    state.services.updater.send_updater_command(state, command, true).await.map_err(|error| UploadUpdateError { error })
}

async fn cancel_update_by_id(state: &AppState, update_id: Uuid) -> Result<(), UploadUpdateError> {
    let command = UpdaterCommand::Cancel { command_id: command_id_from_context("ota_cancel_auto"), update_id };
    state.services.updater.send_updater_command(state, command, true).await.map_err(|error| UploadUpdateError { error })
}

#[utoipa::path(
    get,
    path = "/ota/state",
    tag = "OTA",
    responses(
        (status = 200, description = "Current updater state", body = UpdateStateResponse),
        (status = 503, description = "Updater unavailable", body = UploadUpdateError)
    )
)]
pub async fn updater_state(State(state): State<AppState>) -> impl IntoResponse {
    let upload_dir = match ota_storage::ensure_upload_dir() {
        Ok(dir) => dir,
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(UploadUpdateError { error: err.to_string() })).into_response(),
    };
    let upload_stats = match ota_storage::upload_storage_stats_for_dir(&upload_dir).await {
        Ok(stats) => stats,
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(UploadUpdateError { error: format!("failed to inspect OTA upload storage: {err}") })).into_response(),
    };

    match (fetch_updater_state(&state).await, fetch_updater_storage(&state).await) {
        (Ok((state, cache_usage_bytes)), Ok(storage)) => {
            let state = state.and_then(|s| serde_json::to_value(&s).ok());
            let storage = OtaStorageReport {
                uploads: OtaUploadStorageReport {
                    path: upload_stats.root.display().to_string(),
                    usage_bytes: upload_stats.usage_bytes,
                    quota_bytes: upload_stats.quota_bytes,
                    available_bytes: upload_stats.available_bytes,
                    remaining_bytes: upload_stats.remaining_bytes,
                },
                cache: ota_directory_storage_report(storage.cache),
                work: ota_directory_storage_report(storage.work),
                service_releases: ota_directory_storage_report(storage.service_releases),
                frontend_releases: ota_directory_storage_report(storage.frontend_releases),
            };
            (StatusCode::OK, Json(UpdateStateResponse { state, cache_usage_bytes, storage })).into_response()
        }
        (_, Err(err)) => err.into_response(),
        (Err(err), _) => err.into_response(),
    }
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

async fn fetch_updater_state(state: &AppState) -> Result<(Option<UpdateState>, u64), UploadUpdateError> {
    state.services.updater.fetch_updater_state(state).await.map_err(|error| UploadUpdateError { error })
}

async fn fetch_updater_storage(state: &AppState) -> Result<UpdaterStorageReport, UploadUpdateError> {
    state.services.updater.fetch_updater_storage(state).await.map_err(|error| UploadUpdateError { error })
}

fn ota_directory_storage_report(report: helios_updater::ipc::StorageDirectoryReport) -> OtaDirectoryStorageReport {
    OtaDirectoryStorageReport { path: report.path, usage_bytes: report.usage_bytes, available_bytes: report.available_bytes }
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

const fn default_true() -> bool {
    true
}

impl IntoResponse for UploadUpdateError {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::SERVICE_UNAVAILABLE, Json(self)).into_response()
    }
}
