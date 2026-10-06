//! OS updates (OTA): upload an image, apply it, follow it.
//!
//! Applying hands the image to helios-updater exactly as `heliosctl update apply` does (an Orion
//! artifact + `helios.system.update.v1` workload); the updater stages it into the spare slot,
//! trial-boots it and confirms or rolls back. Status comes from the updater's Orion resources and
//! the boot scripts' files under `/var/lib/helios/ota`.
//!
//! `/v1/ota/*` is the contract Atlas Hardware Manager's HTTP OTA client speaks (multipart
//! upload, `image_url` apply, lenient state polling).

use std::{
    collections::BTreeMap,
    convert::Infallible,
    path::{Path as FsPath, PathBuf},
};

use axum::{
    Json,
    body::{Body, Bytes},
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
};
use futures_util::{Stream, StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::{
    SharedState,
    config::ApiConfig,
    error::{ApiError, ApiResult},
    events::{ApiEvent, sse_stream},
    host::now_ms,
    orion::{StateView, label_map},
};

const UPDATER_RUNTIME_PREFIX: &str = "updater.runtime.";
const EXECUTION_TYPE: &str = "system.update.execution";
const META_FILE: &str = "upload.json";

// --- uploads -------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Upload {
    pub id: String,
    pub filename: String,
    pub size_bytes: u64,
    pub sha256: String,
    /// Where the image is on the device; `POST /v1/update/apply` and `/v1/ota/apply` take it.
    pub image_url: String,
    pub uploaded_at_ms: u64,
    pub version: Option<String>,
}

pub fn sanitize_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let clean: String = base.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' }).collect();
    let clean = clean.trim_start_matches('.').to_string();
    if clean.is_empty() { "helios-update.img".into() } else { clean.chars().take(128).collect() }
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit())
}

/// Stream an upload to disk, hashing as it goes; verify `expected_sha256` when given.
pub async fn store_upload<S, E>(config: &ApiConfig, filename: &str, expected_sha256: Option<&str>, version: Option<String>, mut stream: S) -> ApiResult<Upload>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
    E: std::fmt::Display,
{
    if let Some(expected) = expected_sha256
        && !valid_sha256(expected)
    {
        return Err(ApiError::bad_request("sha256 must be 64 hex characters"));
    }
    let filename = sanitize_filename(filename);
    tokio::fs::create_dir_all(&config.upload_dir).await?;
    let partial = config.upload_dir.join(format!(".partial-{}-{}", now_ms(), std::process::id()));
    let mut file = tokio::fs::File::create(&partial).await?;
    let mut hasher = Sha256::new();
    let mut size: u64 = 0;
    let result: ApiResult<()> = async {
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| ApiError::bad_request(format!("upload interrupted: {error}")))?;
            size += chunk.len() as u64;
            if size > config.max_upload_bytes {
                return Err(ApiError::new(crate::error::ErrorCode::PayloadTooLarge, format!("upload exceeds {} bytes", config.max_upload_bytes)));
            }
            hasher.update(&chunk);
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
        file.sync_all().await?;
        Ok(())
    }
    .await;
    if let Err(error) = result {
        let _ = tokio::fs::remove_file(&partial).await;
        return Err(error);
    }
    if size == 0 {
        let _ = tokio::fs::remove_file(&partial).await;
        return Err(ApiError::bad_request("empty upload"));
    }
    let sha256 = hex::encode(hasher.finalize());
    if let Some(expected) = expected_sha256
        && !expected.eq_ignore_ascii_case(&sha256)
    {
        let _ = tokio::fs::remove_file(&partial).await;
        return Err(ApiError::unprocessable(format!("sha256 mismatch: expected {expected}, received {sha256}")));
    }
    let id = sha256[..16].to_string();
    let dir = config.upload_dir.join(&id);
    tokio::fs::create_dir_all(&dir).await?;
    let path = dir.join(&filename);
    tokio::fs::rename(&partial, &path).await?;
    let upload = Upload { id, filename, size_bytes: size, sha256, image_url: format!("file://{}", path.display()), uploaded_at_ms: now_ms(), version };
    let meta = serde_json::to_vec_pretty(&upload).map_err(|error| ApiError::internal(error.to_string()))?;
    tokio::fs::write(dir.join(META_FILE), meta).await?;
    Ok(upload)
}

pub async fn read_uploads(config: &ApiConfig) -> Vec<Upload> {
    let Ok(mut entries) = tokio::fs::read_dir(&config.upload_dir).await else { return Vec::new() };
    let mut out = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Ok(bytes) = tokio::fs::read(entry.path().join(META_FILE)).await
            && let Ok(upload) = serde_json::from_slice::<Upload>(&bytes)
        {
            out.push(upload);
        }
    }
    out.sort_by_key(|u| std::cmp::Reverse(u.uploaded_at_ms));
    out
}

async fn find_upload(config: &ApiConfig, id: &str) -> ApiResult<Upload> {
    if !id.chars().all(|c| c.is_ascii_hexdigit()) || id.len() != 16 {
        return Err(ApiError::bad_request(format!("invalid upload id {id:?}")));
    }
    let bytes = tokio::fs::read(config.upload_dir.join(id).join(META_FILE)).await.map_err(|_| ApiError::not_found(format!("no upload {id}")))?;
    serde_json::from_slice(&bytes).map_err(|error| ApiError::internal(format!("upload {id} metadata is corrupt: {error}")))
}

/// An `image_url` from an upload response, resolved to the upload (only images uploaded here).
async fn upload_for_url(config: &ApiConfig, image_url: &str) -> ApiResult<Upload> {
    let path = image_url.strip_prefix("file://").ok_or_else(|| ApiError::bad_request("image_url must be a file:// URL returned by an upload"))?;
    let path = PathBuf::from(path);
    let root = tokio::fs::canonicalize(&config.upload_dir).await.map_err(|_| ApiError::not_found("no uploads on this device"))?;
    let resolved = tokio::fs::canonicalize(&path).await.map_err(|_| ApiError::not_found(format!("{image_url} does not exist")))?;
    let id = resolved
        .strip_prefix(&root)
        .ok()
        .and_then(|rest| rest.components().next())
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .ok_or_else(|| ApiError::bad_request("image_url is not an upload"))?;
    find_upload(config, &id).await
}

#[derive(Debug, Deserialize, Default)]
pub struct UploadParams {
    pub filename: Option<String>,
    pub sha256: Option<String>,
    pub version: Option<String>,
}

fn header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_string)
}

/// `POST /v1/update/uploads`: the image as the raw request body (`application/octet-stream`).
pub async fn upload_raw(State(state): State<SharedState>, Query(params): Query<UploadParams>, headers: HeaderMap, body: Body) -> ApiResult<(StatusCode, Json<Upload>)> {
    let filename = params.filename.or_else(|| header(&headers, "x-helios-filename")).unwrap_or_else(|| "helios-update.img".into());
    let sha256 = params.sha256.or_else(|| header(&headers, "x-helios-sha256"));
    let version = params.version.or_else(|| header(&headers, "x-helios-version"));
    let upload = store_upload(&state.config, &filename, sha256.as_deref(), version, body.into_data_stream()).await?;
    state.events.publish("update", serde_json::json!({ "change": "uploaded", "upload": upload }));
    Ok((StatusCode::CREATED, Json(upload)))
}

pub async fn list_uploads(State(state): State<SharedState>) -> Json<Vec<Upload>> {
    Json(read_uploads(&state.config).await)
}

pub async fn delete_upload(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    let upload = find_upload(&state.config, &id).await?;
    tokio::fs::remove_dir_all(state.config.upload_dir.join(&upload.id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// --- apply ---------------------------------------------------------------------

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ApplyRequest {
    pub upload_id: Option<String>,
    pub image_url: Option<String>,
    pub version: Option<String>,
    pub sha256: Option<String>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct ApplyResponse {
    pub update_id: String,
    pub artifact_id: String,
    pub version: String,
    pub sha256: String,
    pub image_url: String,
    pub message: String,
}

pub fn version_for(upload: &Upload, requested: Option<String>) -> String {
    requested
        .filter(|v| !v.trim().is_empty())
        .or_else(|| upload.version.clone())
        .or_else(|| heliosctl::update::infer_version_from_path(FsPath::new(&upload.filename)))
        .unwrap_or_else(|| format!("upload-{}", &upload.sha256[..12]))
}

async fn apply_upload(state: &SharedState, upload: Upload, version: Option<String>, sha256: Option<String>) -> ApiResult<ApplyResponse> {
    if let Some(expected) = sha256.filter(|s| !s.is_empty())
        && !expected.eq_ignore_ascii_case(&upload.sha256)
    {
        return Err(ApiError::unprocessable(format!("checksum mismatch: expected {expected}, upload has {}", upload.sha256)));
    }
    let _guard = state.update_lock.try_lock().map_err(|_| ApiError::conflict("another update is being prepared"))?;
    let version = version_for(&upload, version);
    let image = state.config.upload_dir.join(&upload.id).join(&upload.filename);
    let options = heliosctl::update::PrepareUpdateOptions {
        image,
        version: Some(version),
        artifact_id: None,
        workload_id: None,
        node_id: state.config.node_id.clone(),
        known_sha256: Some(upload.sha256.clone()),
    };
    let prepared = tokio::task::spawn_blocking(move || heliosctl::update::prepare_update(options))
        .await
        .map_err(|error| ApiError::internal(error.to_string()))?
        .map_err(|error| ApiError::unprocessable(format!("image cannot be applied: {error:#}")))?;
    let client = state.orion.raw_client()?;
    let summary = heliosctl::update::submit_update(&client, prepared).await.map_err(|error| ApiError::backend(format!("{error:#}")))?;
    let response = ApplyResponse {
        message: format!("update {} submitted; helios-updater stages it into the spare slot and trial-boots it", summary.version),
        update_id: summary.workload_id,
        artifact_id: summary.artifact_id,
        version: summary.version,
        sha256: summary.sha256,
        image_url: summary.image_url,
    };
    state.events.publish("update", serde_json::json!({ "change": "submitted", "update_id": response.update_id, "version": response.version }));
    Ok(response)
}

pub async fn apply(State(state): State<SharedState>, Json(request): Json<ApplyRequest>) -> ApiResult<(StatusCode, Json<ApplyResponse>)> {
    let upload = match (&request.upload_id, &request.image_url) {
        (Some(id), _) => find_upload(&state.config, id).await?,
        (None, Some(url)) => upload_for_url(&state.config, url).await?,
        (None, None) => return Err(ApiError::bad_request("upload_id or image_url is required")),
    };
    Ok((StatusCode::ACCEPTED, Json(apply_upload(&state, upload, request.version, request.sha256).await?)))
}

pub async fn switch_slot() -> ApiError {
    ApiError::not_available(
        "switching boot slots without an update is not available",
        "helios-updater accepting a slot-switch request (trial boot of the reserve slot with the same confirm/rollback path)",
    )
}

// --- status --------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UpdateExecution {
    pub update_id: String,
    pub artifact_id: Option<String>,
    pub version: Option<String>,
    pub artifact_class: Option<String>,
    pub phase: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Slots {
    pub active: Option<String>,
    pub reserve: Option<String>,
    pub pending: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BootConfirm {
    pub request_id: Option<String>,
    pub status: Option<String>,
    pub selector: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UpdateStatus {
    /// helios-updater's phase: idle, preflight, downloading, staging, switching_boot,
    /// awaiting_boot_success, finalizing, rolling_back, completed, failed; `unknown` without Orion.
    pub phase: String,
    /// The same, in Atlas's stage vocabulary.
    pub stage: String,
    pub progress_percent: Option<u8>,
    pub last_error: Option<String>,
    pub orion_reachable: bool,
    pub updater_running: bool,
    pub active: Option<UpdateExecution>,
    pub executions: Vec<UpdateExecution>,
    pub slots: Slots,
    pub boot_confirm: BootConfirm,
    pub repartition: BTreeMap<String, Option<String>>,
}

pub fn atlas_stage(phase: &str) -> &'static str {
    match phase {
        "preflight" => "verifying",
        "downloading" => "downloading",
        "staging" => "installing",
        "switching_boot" => "committing",
        "awaiting_boot_success" => "rebooting",
        "finalizing" => "finalizing",
        "completed" => "complete",
        "rolling_back" => "rolled_back",
        "failed" => "failed",
        "idle" => "idle",
        _ => "unknown",
    }
}

pub fn progress_for(phase: &str) -> Option<u8> {
    match phase {
        "preflight" => Some(5),
        "downloading" => Some(20),
        "staging" => Some(50),
        "switching_boot" => Some(80),
        "awaiting_boot_success" => Some(90),
        "finalizing" => Some(95),
        "completed" => Some(100),
        _ => None,
    }
}

fn is_terminal(phase: &str) -> bool {
    matches!(phase, "completed" | "failed" | "idle")
}

pub fn status_from(config: &ApiConfig, view: Option<&StateView>) -> UpdateStatus {
    let summary = heliosctl::ota_state::update_summary_at(&config.ota_dir, &config.updater_dir).unwrap_or_default();
    let runtime = view.and_then(|v| v.resources.get(&format!("{UPDATER_RUNTIME_PREFIX}{}", config.node_id)));
    let runtime_phase = runtime.and_then(|r| label_map(&r.labels).get("helios.updater.phase").cloned());
    let executions: Vec<UpdateExecution> = view
        .map(|v| {
            v.resources
                .values()
                .filter(|r| r.resource_type.as_str() == EXECUTION_TYPE)
                .map(|r| {
                    let labels = label_map(&r.labels);
                    let field = |key: &str| r.state.as_ref().and_then(|s| s.config.as_ref()).and_then(|c| c.payload.get(key)).and_then(|v| v.as_str()).map(str::to_string);
                    UpdateExecution {
                        update_id: r.realized_for_workload_id.as_ref().map(ToString::to_string).unwrap_or_else(|| r.resource_id.to_string()),
                        artifact_id: field("artifact_id"),
                        version: labels.get("helios.update.version").cloned(),
                        artifact_class: labels.get("helios.update.artifact_class").cloned(),
                        phase: field("phase").unwrap_or_else(|| "unknown".into()),
                        message: field("message"),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let active = executions.iter().find(|e| !is_terminal(&e.phase)).or_else(|| executions.last()).cloned();
    let phase = match (&active, view) {
        (Some(execution), _) if !is_terminal(&execution.phase) => execution.phase.clone(),
        (_, Some(_)) => runtime_phase.clone().or_else(|| active.as_ref().map(|e| e.phase.clone())).unwrap_or_else(|| "idle".into()),
        (_, None) => "unknown".into(),
    };
    let last_error = active
        .as_ref()
        .filter(|e| e.phase == "failed" || e.phase == "rolling_back")
        .and_then(|e| e.message.clone())
        .or_else(|| summary.confirm_status.as_deref().filter(|s| s.contains("fail") || s.contains("rollback") || s.contains("rolled")).map(|s| format!("boot confirm: {s}")));
    UpdateStatus {
        stage: atlas_stage(&phase).to_string(),
        progress_percent: progress_for(&phase),
        phase,
        last_error,
        orion_reachable: view.is_some(),
        updater_running: runtime.is_some(),
        active,
        executions,
        slots: Slots { active: summary.active, reserve: summary.reserve, pending: summary.pending },
        boot_confirm: BootConfirm { request_id: summary.confirm_request_id, status: summary.confirm_status, selector: summary.confirm_selector },
        repartition: BTreeMap::from([("request_id".to_string(), summary.repartition_request_id), ("status".to_string(), summary.repartition_status)]),
    }
}

async fn current_status(state: &SharedState) -> UpdateStatus {
    let view = state.orion.view().await.ok();
    status_from(&state.config, view.as_ref())
}

pub async fn status(State(state): State<SharedState>) -> Json<UpdateStatus> {
    Json(current_status(&state).await)
}

/// Update events only (`event: update`), starting with the current status.
pub async fn events(State(state): State<SharedState>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let receiver = state.events.subscribe();
    let first = ApiEvent { kind: "update".into(), at_ms: now_ms(), data: serde_json::to_value(current_status(&state).await).unwrap_or_default() };
    Sse::new(sse_stream(receiver, Some(["update".to_string()].into()), vec![first])).keep_alive(KeepAlive::default())
}

// --- Atlas's HTTP OTA client -----------------------------------------------------

#[derive(Debug, Serialize)]
pub struct AtlasUploadResponse {
    pub image_url: String,
    pub filename: String,
    pub size_bytes: u64,
    pub sha256: String,
}

/// `POST /v1/ota/upload`: multipart/form-data with the image in a part named `file`.
pub async fn atlas_upload(State(state): State<SharedState>, mut multipart: Multipart) -> ApiResult<Json<AtlasUploadResponse>> {
    while let Some(field) = multipart.next_field().await.map_err(|error| ApiError::bad_request(error.to_string()))? {
        if field.name() != Some("file") {
            continue;
        }
        let filename = field.file_name().map(str::to_string).unwrap_or_else(|| "helios-update.img".into());
        let stream = field.map_err(|error| error.to_string());
        let upload = store_upload(&state.config, &filename, None, None, Box::pin(stream)).await?;
        state.events.publish("update", serde_json::json!({ "change": "uploaded", "upload": upload }));
        return Ok(Json(AtlasUploadResponse { image_url: upload.image_url, filename: upload.filename, size_bytes: upload.size_bytes, sha256: upload.sha256 }));
    }
    Err(ApiError::bad_request("multipart body has no part named \"file\""))
}

#[derive(Debug, Deserialize)]
pub struct AtlasApplyRequest {
    #[serde(default)]
    pub requested_by: Option<String>,
    pub image_url: String,
    #[serde(default)]
    pub size_bytes: Option<u64>,
    #[serde(default)]
    pub checksum: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AtlasApplyResponse {
    pub update_id: String,
    pub message: String,
}

pub async fn atlas_apply(State(state): State<SharedState>, Json(request): Json<AtlasApplyRequest>) -> ApiResult<Json<AtlasApplyResponse>> {
    let upload = upload_for_url(&state.config, &request.image_url).await?;
    if let Some(size) = request.size_bytes
        && size != upload.size_bytes
    {
        return Err(ApiError::unprocessable(format!("size mismatch: expected {size} bytes, upload has {}", upload.size_bytes)));
    }
    if let Some(by) = &request.requested_by {
        tracing::info!(requested_by = %by, image = %request.image_url, "OTA apply requested");
    }
    let response = apply_upload(&state, upload, None, request.checksum).await?;
    Ok(Json(AtlasApplyResponse { update_id: response.update_id, message: response.message }))
}

/// `GET /v1/ota/state`: `{"state": {"update_id", "stage", "progress_percent", "last_error"}, ...}`.
pub async fn atlas_state(State(state): State<SharedState>) -> Json<serde_json::Value> {
    let status = current_status(&state).await;
    Json(serde_json::json!({
        "state": {
            "update_id": status.active.as_ref().map(|a| a.update_id.clone()),
            "stage": status.stage,
            "phase": status.phase,
            "progress_percent": status.progress_percent,
            "last_error": status.last_error,
        },
        "slots": status.slots,
        "boot_confirm": status.boot_confirm,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(dir: &FsPath) -> ApiConfig {
        ApiConfig { upload_dir: dir.join("uploads"), ota_dir: dir.join("ota"), updater_dir: dir.join("updater"), max_upload_bytes: 64, ..ApiConfig::default() }
    }

    fn chunks(parts: &[&'static [u8]]) -> impl Stream<Item = Result<Bytes, Infallible>> + Unpin {
        futures_util::stream::iter(parts.iter().map(|p| Ok(Bytes::from_static(p))).collect::<Vec<_>>())
    }

    #[tokio::test]
    async fn uploads_hash_verify_and_list() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config(dir.path());
        let upload = store_upload(&config, "../helios-raze-v2026.4.0.img", None, None, chunks(&[b"hello ", b"world"])).await.expect("upload");
        assert_eq!(upload.sha256, "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9");
        assert_eq!(upload.filename, "helios-raze-v2026.4.0.img");
        assert_eq!(upload.size_bytes, 11);
        assert!(upload.image_url.starts_with("file://"));
        assert_eq!(read_uploads(&config).await, vec![upload.clone()]);
        assert_eq!(upload_for_url(&config, &upload.image_url).await.expect("by url").id, upload.id);
        assert_eq!(version_for(&upload, None), "v2026.4.0");

        let wrong = store_upload(&config, "x.img", Some(&"0".repeat(64)), None, chunks(&[b"abc"])).await.expect_err("checksum mismatch");
        assert_eq!(wrong.code, crate::error::ErrorCode::Unprocessable);
        let big = store_upload(&config, "x.img", None, None, chunks(&[&[0u8; 65]])).await.expect_err("too large");
        assert_eq!(big.code, crate::error::ErrorCode::PayloadTooLarge);
        assert!(upload_for_url(&config, "file:///etc/passwd").await.is_err());
    }

    #[test]
    fn status_without_orion_reads_ota_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config(dir.path());
        std::fs::create_dir_all(&config.ota_dir).expect("mkdir");
        std::fs::write(config.ota_dir.join("active"), "A").expect("write");
        std::fs::write(config.ota_dir.join("confirm-result.env"), "HELIOS_UPDATE_CONFIRM_STATUS=rolled-back\n").expect("write");
        let status = status_from(&config, None);
        assert_eq!(status.phase, "unknown");
        assert!(!status.orion_reachable);
        assert_eq!(status.slots.active.as_deref(), Some("A"));
        assert!(status.last_error.expect("error").contains("rolled-back"));
    }

    #[test]
    fn status_follows_updater_executions() {
        use orion::control_plane::{ResourceConfigState, ResourceRecord, ResourceState, TypedConfigValue};
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config(dir.path());
        let mut view = StateView::default();
        view.resources.insert(
            "updater.runtime.node-local".into(),
            ResourceRecord::builder("updater.runtime.node-local", "system.update.runtime", "provider.updater.node-local").label("helios.updater.phase=idle").build(),
        );
        let execution = ResourceRecord::builder("update.execution.update.node-local.v2", EXECUTION_TYPE, "provider.updater.node-local")
            .label("helios.update.version=v2")
            .state(
                ResourceState::new(0)
                    .with_config(ResourceConfigState::new().field("phase", TypedConfigValue::String("staging".into())).field("message", TypedConfigValue::String("writing slot B".into()))),
            )
            .build();
        view.resources.insert("update.execution.update.node-local.v2".into(), execution);
        let status = status_from(&config, Some(&view));
        assert_eq!(status.phase, "staging");
        assert_eq!(status.stage, "installing");
        assert_eq!(status.progress_percent, Some(50));
        assert!(status.updater_running);
        assert_eq!(status.active.expect("active").version.as_deref(), Some("v2"));
    }

    #[test]
    fn filenames_are_sanitized() {
        assert_eq!(sanitize_filename("/tmp/../evil name.img"), "evil_name.img");
        assert_eq!(sanitize_filename(".."), "helios-update.img");
    }
}
