//! OS updates (OTA): upload an image, stage and apply it, follow it.
//!
//! HeliOS has no updater of its own. The Raze device package's A/B writer
//! (`/usr/lib/pd-device/update`, Atlas `docs/ota.md`) takes the same `.img.xz` that is flashed:
//! `stage` checks its SHA-256 and copies its boot and root slot A into the board's inactive
//! slot, `apply` runs the image's pre-reboot hook and trial-boots that slot, and
//! `pd-device-update-confirm.service` keeps it once `/etc/pd-device/update-health` passes (or
//! the board falls back by itself). This module is a thin adapter: it stores uploads on the data
//! partition, runs `stage` and `apply` through `systemd-run` (outside helios-api's cgroup: the
//! pre-reboot hook stops helios-api), and reports the writer's `/run/pd-device/update.json`.
//!
//! `/v1/ota/*` is the contract Atlas Hardware Manager's HTTP OTA client speaks (multipart
//! upload, `image_url` apply, lenient state polling).

use std::{convert::Infallible, path::PathBuf, sync::Arc};

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
    pd_update::{self, PdUpdateStatus},
};

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

impl Upload {
    fn path(&self, config: &ApiConfig) -> PathBuf {
        config.upload_dir.join(&self.id).join(&self.filename)
    }
}

pub fn sanitize_filename(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let clean: String = base.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' }).collect();
    let clean = clean.trim_start_matches('.').to_string();
    if clean.is_empty() { "helios-update.img.xz".into() } else { clean.chars().take(128).collect() }
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
    let filename = params.filename.or_else(|| header(&headers, "x-helios-filename")).unwrap_or_else(|| "helios-update.img.xz".into());
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

// --- stage and apply -----------------------------------------------------------

/// What helios-api is doing with the device package's writer right now (or did last).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UpdateTask {
    pub update_id: String,
    pub upload_id: String,
    pub version: String,
    pub sha256: String,
    /// staging, staged (no reboot asked), applying (`update apply` started: the board reboots
    /// into the staged slot on trial), failed.
    pub step: String,
    pub reboot: bool,
    pub error: Option<String>,
    pub started_at_ms: u64,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ApplyRequest {
    pub upload_id: Option<String>,
    pub image_url: Option<String>,
    /// Informational; the writer takes the version from the new root's os-release.
    pub version: Option<String>,
    pub sha256: Option<String>,
    /// Reboot into the staged slot once staged (default). `false` stops after staging.
    pub reboot: Option<bool>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct ApplyResponse {
    pub update_id: String,
    pub version: String,
    pub sha256: String,
    pub image_url: String,
    pub reboot: bool,
    pub message: String,
}

pub fn version_for(upload: &Upload, requested: Option<String>) -> String {
    requested
        .filter(|v| !v.trim().is_empty())
        .or_else(|| upload.version.clone())
        .or_else(|| pd_update::infer_version_from_path(std::path::Path::new(&upload.filename)))
        .unwrap_or_else(|| format!("upload-{}", &upload.sha256[..12]))
}

fn tool_installed(config: &ApiConfig) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(&config.pd_update_tool).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// `update <args>`, through `systemd-run` on the device so the command lives in its own unit.
/// `wait`: block until it ends (stage); otherwise only start it (apply, which reboots).
fn pd_update_command(config: &ApiConfig, args: &[&str], wait: bool) -> tokio::process::Command {
    let mut command;
    if config.pd_update_systemd_run {
        command = tokio::process::Command::new("systemd-run");
        command.args(["--quiet", "--collect", "--description=HeliOS OS update (device package writer)"]);
        if wait {
            command.args(["--wait", "--pipe"]);
        }
        command.arg("--").arg(&config.pd_update_tool);
    } else {
        command = tokio::process::Command::new(&config.pd_update_tool);
    }
    command.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(false);
    command
}

/// The writer's last words on stderr (it logs `pd-device: <reason>`), else the exit status.
fn command_error(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    stderr
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.trim_start_matches("pd-device:").trim().to_string())
        .unwrap_or_else(|| format!("the device package updater exited with {}", output.status))
}

fn set_task(state: &SharedState, task: UpdateTask) {
    *state.update_task.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(task);
}

fn publish_status(state: &SharedState, change: &str) {
    let status = status_from(&state.config, current_task(state));
    let mut data = serde_json::to_value(status).unwrap_or_default();
    if let Some(object) = data.as_object_mut() {
        object.insert("change".into(), change.into());
    }
    state.events.publish("update", data);
}

async fn stage_and_apply(state: SharedState, upload: Upload, mut task: UpdateTask, _guard: tokio::sync::OwnedMutexGuard<()>) {
    let image = upload.path(&state.config).display().to_string();
    let result = pd_update_command(&state.config, &["stage", &image, "--sha256", &upload.sha256], true).output().await;
    match result {
        Ok(output) if output.status.success() => {
            tracing::info!(version = %task.version, "update staged");
            // The slot holds the image now; free the data partition.
            let _ = tokio::fs::remove_dir_all(state.config.upload_dir.join(&upload.id)).await;
            task.step = if task.reboot { "applying".into() } else { "staged".into() };
        }
        Ok(output) => {
            task.step = "failed".into();
            task.error = Some(command_error(&output));
        }
        Err(error) => {
            task.step = "failed".into();
            task.error = Some(format!("could not run the device package updater: {error}"));
        }
    }
    set_task(&state, task.clone());
    publish_status(&state, if task.step == "failed" { "failed" } else { "staged" });
    if task.step != "applying" {
        if let Some(error) = &task.error {
            tracing::warn!(%error, "update stage failed");
        }
        return;
    }
    // `update apply` runs the pre-reboot hook (which stops helios-api, among others) and
    // reboots into the staged slot on trial; it is started in its own unit and not awaited.
    match pd_update_command(&state.config, &["apply"], false).output().await {
        Ok(output) if output.status.success() => tracing::info!(version = %task.version, "update applied; rebooting into the staged slot on trial"),
        Ok(output) => {
            task.step = "failed".into();
            task.error = Some(command_error(&output));
        }
        Err(error) => {
            task.step = "failed".into();
            task.error = Some(format!("could not run the device package updater: {error}"));
        }
    }
    if task.step == "failed" {
        set_task(&state, task);
        publish_status(&state, "failed");
    }
}

async fn apply_upload(state: &SharedState, upload: Upload, version: Option<String>, sha256: Option<String>, reboot: bool) -> ApiResult<ApplyResponse> {
    if let Some(expected) = sha256.filter(|s| !s.is_empty())
        && !expected.eq_ignore_ascii_case(&upload.sha256)
    {
        return Err(ApiError::unprocessable(format!("checksum mismatch: expected {expected}, upload has {}", upload.sha256)));
    }
    if !tool_installed(&state.config) {
        return Err(ApiError::not_available(
            format!("the device package updater ({}) is not installed", state.config.pd_update_tool.display()),
            "the Raze device package's A/B updater (Atlas devices/raze 1.4.0 or newer) and an A/B disk layout",
        ));
    }
    if let Some(current) = read_pd_status(&state.config) {
        if current.state == "trying" {
            return Err(ApiError::conflict("the last update is still on trial; wait for it to be confirmed or rolled back"));
        }
        if current.state == "staging" {
            return Err(ApiError::conflict("an update is being staged"));
        }
        if !current.on_ab_layout() {
            return Err(ApiError::unprocessable("this board is not on the A/B layout; reflash it over USB with an A/B image first"));
        }
    }
    let guard = state.update_lock.clone().try_lock_owned().map_err(|_| ApiError::conflict("another update is being staged"))?;
    let version = version_for(&upload, version);
    let task = UpdateTask {
        update_id: format!("{}-{}", upload.id, now_ms()),
        upload_id: upload.id.clone(),
        version: version.clone(),
        sha256: upload.sha256.clone(),
        step: "staging".into(),
        reboot,
        error: None,
        started_at_ms: now_ms(),
    };
    set_task(state, task.clone());
    publish_status(state, "staging");
    let response = ApplyResponse {
        update_id: task.update_id.clone(),
        version,
        sha256: upload.sha256.clone(),
        image_url: upload.image_url.clone(),
        reboot,
        message: if reboot {
            "staging the image into the inactive slot; the board then reboots into it on trial and keeps it once healthy".into()
        } else {
            "staging the image into the inactive slot; apply it later with `update apply` or POST /v1/update/apply".into()
        },
    };
    tokio::spawn(stage_and_apply(state.clone(), upload, task, guard));
    Ok(response)
}

pub async fn apply(State(state): State<SharedState>, Json(request): Json<ApplyRequest>) -> ApiResult<(StatusCode, Json<ApplyResponse>)> {
    let upload = match (&request.upload_id, &request.image_url) {
        (Some(id), _) => find_upload(&state.config, id).await?,
        (None, Some(url)) => upload_for_url(&state.config, url).await?,
        (None, None) => return Err(ApiError::bad_request("upload_id or image_url is required")),
    };
    Ok((StatusCode::ACCEPTED, Json(apply_upload(&state, upload, request.version, request.sha256, request.reboot.unwrap_or(true)).await?)))
}

pub async fn switch_slot() -> ApiError {
    ApiError::not_available("switching boot slots without an update is not available", "the device package's writer offering a trial boot of the other slot without staging an image")
}

// --- status --------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Slots {
    /// A or B: the slot this system booted from; `unknown` off the A/B layout.
    pub active: Option<String>,
    /// The slot holding a staged (or on-trial) update.
    pub staged: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UpdateStatus {
    /// The device package writer's state: idle, staging, staged, trying, confirmed,
    /// rolled-back, error; `unknown` when it has published none.
    pub phase: String,
    /// The same, in Atlas's stage vocabulary.
    pub stage: String,
    pub progress_percent: Option<u8>,
    pub last_error: Option<String>,
    /// `/usr/lib/pd-device/update` is installed.
    pub updater_available: bool,
    pub slots: Slots,
    pub version_active: Option<String>,
    pub version_staged: Option<String>,
    /// What helios-api last asked the writer to do.
    pub task: Option<UpdateTask>,
}

fn read_pd_status(config: &ApiConfig) -> Option<PdUpdateStatus> {
    pd_update::read_status_at(&config.pd_update_status, &config.pd_update_progress)
}

fn current_task(state: &SharedState) -> Option<UpdateTask> {
    state.update_task.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

pub fn status_from(config: &ApiConfig, task: Option<UpdateTask>) -> UpdateStatus {
    let pd = read_pd_status(config);
    let running = task.as_ref().is_some_and(|t| t.step == "staging");
    let (phase, stage, progress_percent) = match &pd {
        // Staging asked for, but the writer has not written its first state yet.
        Some(pd) if running && !matches!(pd.state.as_str(), "staging" | "error") => ("staging".to_string(), "verifying".to_string(), Some(0)),
        Some(pd) => (pd.state.clone(), pd.atlas_stage().to_string(), pd.percent()),
        None if running => ("staging".to_string(), "verifying".to_string(), Some(0)),
        None => ("unknown".to_string(), "unknown".to_string(), None),
    };
    let task_error = task.as_ref().filter(|t| t.step == "failed").and_then(|t| t.error.clone());
    let last_error = pd.as_ref().filter(|pd| matches!(pd.state.as_str(), "error" | "rolled-back") || pd.error.is_some()).and_then(|pd| pd.error.clone()).or(task_error);
    UpdateStatus {
        phase,
        stage,
        progress_percent,
        last_error,
        updater_available: tool_installed(config),
        slots: Slots { active: pd.as_ref().and_then(|pd| pd.slot_active.clone()), staged: pd.as_ref().and_then(|pd| pd.slot_staged.clone()) },
        version_active: pd.as_ref().and_then(|pd| pd.version_active.clone()),
        version_staged: pd.as_ref().and_then(|pd| pd.version_staged.clone()),
        task,
    }
}

/// Before the writer has run on this boot `/run/pd-device/update.json` may be missing;
/// `update status` writes it (it reads the state kept on p1).
async fn ensure_pd_status(config: &ApiConfig) {
    if config.pd_update_status.exists() || !tool_installed(config) {
        return;
    }
    let mut command = tokio::process::Command::new(&config.pd_update_tool);
    command.arg("status").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    let _ = tokio::time::timeout(std::time::Duration::from_secs(10), command.status()).await;
}

pub async fn current_status(state: &SharedState) -> UpdateStatus {
    ensure_pd_status(&state.config).await;
    status_from(&state.config, current_task(state))
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
        let filename = field.file_name().map(str::to_string).unwrap_or_else(|| "helios-update.img.xz".into());
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
    let response = apply_upload(&state, upload, None, request.checksum, true).await?;
    Ok(Json(AtlasApplyResponse { update_id: response.update_id, message: response.message }))
}

/// `GET /v1/ota/state`: `{"state": {"update_id", "stage", "progress_percent", "last_error"}, ...}`.
pub async fn atlas_state(State(state): State<SharedState>) -> Json<serde_json::Value> {
    let status = current_status(&state).await;
    Json(serde_json::json!({
        "state": {
            "update_id": status.task.as_ref().map(|t| t.update_id.clone()),
            "stage": status.stage,
            "phase": status.phase,
            "progress_percent": status.progress_percent,
            "last_error": status.last_error,
        },
        "slots": status.slots,
        "version_active": status.version_active,
        "version_staged": status.version_staged,
    }))
}

/// Shared by the API state: one stage at a time from helios-api (the writer has its own lock).
pub fn new_update_lock() -> Arc<tokio::sync::Mutex<()>> {
    Arc::new(tokio::sync::Mutex::new(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn config(dir: &std::path::Path) -> ApiConfig {
        ApiConfig {
            upload_dir: dir.join("uploads"),
            pd_update_tool: dir.join("pd-update"),
            pd_update_status: dir.join("update.json"),
            pd_update_progress: dir.join("progress"),
            pd_update_systemd_run: false,
            ui_dir: None,
            max_upload_bytes: 64,
            ..ApiConfig::default()
        }
    }

    fn chunks(parts: &[&'static [u8]]) -> impl Stream<Item = Result<Bytes, Infallible>> + Unpin {
        futures_util::stream::iter(parts.iter().map(|p| Ok(Bytes::from_static(p))).collect::<Vec<_>>())
    }

    #[tokio::test]
    async fn uploads_hash_verify_and_list() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config(dir.path());
        let upload = store_upload(&config, "../helios-full-raze-v2026.4.0.img.xz", None, None, chunks(&[b"hello ", b"world"])).await.expect("upload");
        assert_eq!(upload.sha256, "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9");
        assert_eq!(upload.filename, "helios-full-raze-v2026.4.0.img.xz");
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
    fn status_without_the_writer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let status = status_from(&config(dir.path()), None);
        assert_eq!(status.phase, "unknown");
        assert!(!status.updater_available);
        assert_eq!(status.slots.active, None);
    }

    #[test]
    fn status_follows_the_writer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config(dir.path());
        std::fs::write(&config.pd_update_status, r#"{"state":"staging","slot_active":"A","slot_staged":"B","version_active":"v1","version_staged":"","progress":100,"error":""}"#).expect("write");
        std::fs::write(&config.pd_update_progress, "1000").expect("write");
        let status = status_from(&config, None);
        assert_eq!(status.phase, "staging");
        assert_eq!(status.stage, "installing");
        assert_eq!(status.progress_percent, Some(95));
        assert_eq!(status.slots.staged.as_deref(), Some("B"));

        std::fs::write(
            &config.pd_update_status,
            r#"{"state":"rolled-back","slot_active":"A","slot_staged":"B","version_active":"v1","version_staged":"v2","progress":1000,"error":"the health check failed on v2"}"#,
        )
        .expect("write");
        let status = status_from(&config, None);
        assert_eq!(status.stage, "rolled_back");
        assert_eq!(status.last_error.as_deref(), Some("the health check failed on v2"));
    }

    /// A stand-in for `/usr/lib/pd-device/update` that records its arguments and writes the
    /// writer's state file as the real one does.
    fn fake_writer(dir: &std::path::Path, stage_ok: bool) -> PathBuf {
        let log = dir.join("calls.log");
        let status = dir.join("update.json");
        let stage = if stage_ok {
            format!(r#"printf '%s\n' '{{"state":"staged","slot_active":"A","slot_staged":"B","version_active":"v1","version_staged":"v2","progress":1000,"error":""}}' > {}"#, status.display())
        } else {
            "echo 'pd-device: the image failed its SHA-256 check (damaged or incomplete copy)' >&2; exit 1".to_string()
        };
        let script = format!(
            "#!/bin/sh\necho \"$*\" >> {log}\ncase \"$1\" in\nstage) {stage} ;;\napply) printf '%s\\n' '{{\"state\":\"trying\",\"slot_active\":\"A\",\"slot_staged\":\"B\",\"version_active\":\"v1\",\"version_staged\":\"v2\",\"progress\":1000,\"error\":\"\"}}' > {status} ;;\nesac\n",
            log = log.display(),
            status = status.display(),
        );
        let path = dir.join("pd-update");
        std::fs::write(&path, script).expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        std::fs::write(&status, r#"{"state":"idle","slot_active":"A","slot_staged":"","version_active":"v1","version_staged":"","progress":0,"error":""}"#).expect("write");
        path
    }

    async fn wait_for_step(state: &SharedState, step: &str) -> UpdateTask {
        for _ in 0..200 {
            if let Some(task) = current_task(state)
                && task.step == step
            {
                return task;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("task never reached {step}: {:?}", current_task(state));
    }

    #[tokio::test]
    async fn apply_stages_then_applies_through_the_writer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = config(dir.path());
        config.pd_update_tool = fake_writer(dir.path(), true);
        let state = crate::AppState::new(config.clone());
        let upload = store_upload(&config, "helios-full-raze-v2.img.xz", None, None, chunks(&[b"image"])).await.expect("upload");
        let response = apply_upload(&state, upload.clone(), None, Some(upload.sha256.clone()), true).await.expect("apply");
        assert_eq!(response.version, "v2");
        wait_for_step(&state, "applying").await;
        for _ in 0..200 {
            if std::fs::read_to_string(dir.path().join("calls.log")).unwrap_or_default().lines().count() == 2 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let calls = std::fs::read_to_string(dir.path().join("calls.log")).expect("calls");
        let image = upload.path(&config).display().to_string();
        assert_eq!(calls, format!("stage {image} --sha256 {}\napply\n", upload.sha256));
        assert!(!upload.path(&config).exists(), "a staged upload is removed");
        let status = status_from(&config, current_task(&state));
        assert_eq!(status.phase, "trying");
        assert_eq!(status.stage, "rebooting");
    }

    #[tokio::test]
    async fn a_failed_stage_is_reported_and_not_applied() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = config(dir.path());
        config.pd_update_tool = fake_writer(dir.path(), false);
        let state = crate::AppState::new(config.clone());
        let upload = store_upload(&config, "image.img.xz", None, None, chunks(&[b"image"])).await.expect("upload");
        apply_upload(&state, upload.clone(), None, None, true).await.expect("apply");
        let task = wait_for_step(&state, "failed").await;
        assert_eq!(task.error.as_deref(), Some("the image failed its SHA-256 check (damaged or incomplete copy)"));
        let calls = std::fs::read_to_string(dir.path().join("calls.log")).expect("calls");
        assert!(!calls.contains("apply"));
        assert!(upload.path(&config).exists(), "a failed upload is kept");
        assert_eq!(status_from(&config, Some(task)).last_error.as_deref(), Some("the image failed its SHA-256 check (damaged or incomplete copy)"));
    }

    #[tokio::test]
    async fn apply_refuses_without_the_writer_or_during_a_trial() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config(dir.path());
        let state = crate::AppState::new(config.clone());
        let upload = store_upload(&config, "image.img.xz", None, None, chunks(&[b"image"])).await.expect("upload");
        let error = apply_upload(&state, upload.clone(), None, None, true).await.expect_err("no writer");
        assert_eq!(error.code, crate::error::ErrorCode::NotAvailable);

        let mut config = config.clone();
        config.pd_update_tool = fake_writer(dir.path(), true);
        std::fs::write(&config.pd_update_status, r#"{"state":"trying","slot_active":"B","slot_staged":"B","version_active":"v1","version_staged":"v2","progress":1000,"error":""}"#).expect("write");
        let state = crate::AppState::new(config);
        let error = apply_upload(&state, upload, None, None, true).await.expect_err("on trial");
        assert_eq!(error.code, crate::error::ErrorCode::Conflict);
    }

    #[test]
    fn filenames_are_sanitized() {
        assert_eq!(sanitize_filename("/tmp/../evil name.img"), "evil_name.img");
        assert_eq!(sanitize_filename(".."), "helios-update.img.xz");
    }
}
