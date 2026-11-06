//! OS updates (OTA): upload the build's `.img.xz`, stage it into an inactive slot, boot it on trial,
//! then confirm it or roll back.
//!
//! HeliOS has no updater of its own. The Raze device package's slots writer (`/usr/lib/board/slots`,
//! Atlas's `devices/raze` package) installs a slots build's whole-disk image: `slots stage-image
//! <file.img.xz> --sha256 <hex>` checks the image and installs the slot it carries (xz, zst, gz or
//! raw, read from the file's magic). `slots apply` boots that slot once on trial, `slots confirm`
//! keeps a healthy trial (run on every boot by `board-update-confirm.service`, and by `POST
//! /v1/update/confirm`), `slots rollback` makes the previous slot the default again, and `slots
//! boot-once <id>` (`POST /v1/update/slots/switch`) boots another installed slot on trial. A slot
//! bundle tar is no longer built or accepted: the `.img.xz` is the only update.
//!
//! The writer's exit status is its answer: 0 done, 3 refused with nothing changed (wrong sha256, the
//! running or default slot, no room, the device lock), anything else failed. The stage's answer
//! within 15 s (its checks come before the slow unpack) is the upload request's: a refusal is a 409
//! with the writer's message, a failure a 422. A later answer is the task's `failed` step, read from
//! `/v1/update/status`.
//!
//! This module is a thin adapter: it stores uploads on the data partition, runs the writer through
//! `systemd-run` (its own unit, so a reboot it asks for does not depend on helios-api), and reports
//! the writer's `/run/board/update.json`.
//!
//! Restarts: the writer's apply, boot-once and rollback restart the board with `systemctl reboot`
//! (it runs the OS's `/etc/board/update.d/pre-reboot` hook first, and stops services cleanly). HeliOS
//! has no hook of its own: an upload is fsynced before it is acknowledged, and the writer keeps the
//! update state in its own files, so nothing here has to flush before a restart.

use std::{convert::Infallible, path::PathBuf, sync::Arc};

use axum::{
    Json,
    body::{Body, Bytes},
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
};
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::{
    SharedState,
    board_update::{self, BoardUpdateStatus},
    capabilities::{self, feature},
    config::ApiConfig,
    error::{ApiError, ApiResult},
    events::{ApiEvent, sse_stream},
    host::now_ms,
};

const META_FILE: &str = "upload.json";

// --- uploads -------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Upload {
    pub id: String,
    pub filename: String,
    pub size_bytes: u64,
    pub sha256: String,
    /// Where the image is on the device; `POST /v1/update/apply` takes it.
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

/// Where a ustar header's magic is (a tar's first block: the name, then `ustar` at 257).
const TAR_MAGIC_AT: usize = 257;

/// Whether an upload's first bytes are a tar: a slot bundle, which nothing builds any more. The
/// writer reads an image by its magic instead, so a tar is refused here rather than at stage time.
fn is_tar(head: &[u8]) -> bool {
    head.len() >= TAR_MAGIC_AT + 5 && &head[TAR_MAGIC_AT..TAR_MAGIC_AT + 5] == b"ustar"
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
    prune_uploads(config).await;
    let partial = config.upload_dir.join(format!(".partial-{}-{}", now_ms(), std::process::id()));
    let mut file = tokio::fs::File::create(&partial).await?;
    let mut hasher = Sha256::new();
    let mut size: u64 = 0;
    let mut head: Vec<u8> = Vec::with_capacity(TAR_MAGIC_AT + 5);
    let result: ApiResult<()> = async {
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| ApiError::bad_request(format!("upload interrupted: {error}")))?;
            size += chunk.len() as u64;
            if size > config.max_upload_bytes {
                return Err(ApiError::new(crate::error::ErrorCode::PayloadTooLarge, format!("upload exceeds {} bytes", config.max_upload_bytes)));
            }
            if head.len() < TAR_MAGIC_AT + 5 {
                head.extend_from_slice(&chunk[..chunk.len().min(TAR_MAGIC_AT + 5 - head.len())]);
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
    if is_tar(&head) {
        let _ = tokio::fs::remove_file(&partial).await;
        return Err(ApiError::unprocessable("this is a tar (a slot bundle); upload the build's .img.xz, the only update image"));
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

/// How long an upload is kept. An update is installed from its upload within minutes (Atlas's
/// gateway install has the board's writer read it in place, and leaves it), so an older one is
/// only taking up /data.
pub const UPLOAD_KEEP: std::time::Duration = std::time::Duration::from_secs(60 * 60);

/// Removes uploads, and partial uploads, older than [`UPLOAD_KEEP`]. Runs at start and before
/// each new upload.
pub async fn prune_uploads(config: &ApiConfig) {
    let Ok(mut entries) = tokio::fs::read_dir(&config.upload_dir).await else { return };
    let now = std::time::SystemTime::now();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let stale = entry.metadata().await.ok().and_then(|meta| meta.modified().ok()).and_then(|modified| now.duration_since(modified).ok()).is_some_and(|age| age > UPLOAD_KEEP);
        if !stale {
            continue;
        }
        let path = entry.path();
        let removed = if path.is_dir() { tokio::fs::remove_dir_all(&path).await } else { tokio::fs::remove_file(&path).await };
        match removed {
            Ok(()) => tracing::info!(path = %path.display(), "removed an old upload"),
            Err(error) => tracing::warn!(path = %path.display(), %error, "could not remove an old upload"),
        }
    }
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
    capabilities::require(&state, feature::UPDATES).await?;
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

/// What helios-api is doing with the slots writer right now (or did last).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UpdateTask {
    pub update_id: String,
    /// The upload being staged (`None` for a confirm or a rollback).
    pub upload_id: Option<String>,
    pub version: Option<String>,
    pub sha256: Option<String>,
    /// staging, staged (no reboot asked), applying (`slots apply` started: the board reboots into
    /// the staged slot on trial), confirming, rolling-back, done (the confirm or rollback ran),
    /// failed.
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
    /// Informational; the writer takes the version from the slot bundle's `slot.json`.
    pub version: Option<String>,
    pub sha256: Option<String>,
    /// With an upload: stage it, then boot the slot on trial (default). `false` stops after staging.
    /// Without one: boot the staged slot on trial (the default, and the only choice).
    pub reboot: Option<bool>,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct ApplyResponse {
    pub update_id: String,
    pub version: Option<String>,
    pub sha256: Option<String>,
    pub image_url: Option<String>,
    pub reboot: bool,
    pub message: String,
}

/// `POST /v1/update/confirm`, `/rollback` and `/slots/switch`: the writer's command has started in
/// the background.
#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct ActionResponse {
    pub update_id: String,
    /// `confirm`, `rollback` or `switch`.
    pub action: String,
    pub message: String,
}

pub fn version_for(upload: &Upload, requested: Option<String>) -> String {
    requested
        .filter(|v| !v.trim().is_empty())
        .or_else(|| upload.version.clone())
        .or_else(|| board_update::infer_version_from_path(std::path::Path::new(&upload.filename)))
        .unwrap_or_else(|| format!("upload-{}", &upload.sha256[..12]))
}

/// The device package's slots writer is installed and runnable. A device without a device package
/// (a guest) has none.
fn tool_installed(config: &ApiConfig) -> bool {
    use std::os::unix::fs::PermissionsExt;
    config.board.as_ref().is_some_and(|board| std::fs::metadata(&board.update_tool).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
}

/// `slots <args>`, through `systemd-run` on the device so the command lives in its own unit (a
/// reboot it asks for does not depend on helios-api). `wait`: block until it ends; otherwise only
/// start it (apply and rollback reboot). Without a device package this is an error.
fn slots_command(config: &ApiConfig, args: &[&str], wait: bool) -> std::io::Result<tokio::process::Command> {
    let Some(board) = config.board.as_ref() else {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "the device package writer is not available on this device"));
    };
    let mut command;
    if config.board_update_systemd_run {
        command = tokio::process::Command::new("systemd-run");
        command.args(["--quiet", "--collect", "--description=HeliOS OS update (slots writer)"]);
        if wait {
            command.args(["--wait", "--pipe"]);
        }
        command.arg("--").arg(&board.update_tool);
    } else {
        command = tokio::process::Command::new(&board.update_tool);
    }
    command.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(false);
    Ok(command)
}

/// The writer's last words on stderr (it logs `slots: <reason>`), else the exit status.
fn command_error(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    stderr
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.trim_start_matches("slots:").trim().to_string())
        .unwrap_or_else(|| format!("the device package writer exited with {}", output.status))
}

/// The writer's exit status for a refusal: it changed nothing.
const REFUSED_EXIT: i32 = 3;

/// How long an upload's stage may answer before its request returns. The writer checks the sha256
/// and the room before it unpacks the image, so its refusals come well inside this (the hash of the
/// image is the long part). Past it the request is accepted, and the stage runs on in the task.
const STAGE_ANSWER_WINDOW: std::time::Duration = std::time::Duration::from_secs(15);

/// How long a command that reboots waits after its request is answered: the 202 reaches the client
/// before the network goes down with the board.
const REBOOT_GRACE: std::time::Duration = std::time::Duration::from_millis(500);

/// The writer's commands `POST /v1/update/slots/{command}` runs.
const SLOT_COMMANDS: [&str; 4] = ["set-default", "remove", "pin", "unpin"];

/// A writer command that did not succeed.
#[derive(Debug, Clone, PartialEq, Eq)]
enum WriterError {
    /// Exit 3: refused with nothing changed; the writer's reason.
    Refused(String),
    /// Any other exit, or the writer could not run; the writer's reason.
    Failed(String),
}

impl WriterError {
    fn message(&self) -> &str {
        match self {
            Self::Refused(message) | Self::Failed(message) => message,
        }
    }

    fn into_api(self) -> ApiError {
        match self {
            Self::Refused(message) => ApiError::conflict(message),
            Self::Failed(message) => ApiError::unprocessable(message),
        }
    }
}

/// Runs one `slots` command; its error is the writer's reason (or why it could not run).
async fn run_writer(config: &ApiConfig, args: &[&str], wait: bool) -> Result<(), WriterError> {
    let output = match slots_command(config, args, wait) {
        Ok(mut command) => command.output().await,
        Err(error) => Err(error),
    };
    match output {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) if output.status.code() == Some(REFUSED_EXIT) => Err(WriterError::Refused(command_error(&output))),
        Ok(output) => Err(WriterError::Failed(command_error(&output))),
        Err(error) => Err(WriterError::Failed(format!("could not run the device package writer: {error}"))),
    }
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

fn new_task(id_prefix: &str, upload: Option<&Upload>, version: Option<String>, step: &str, reboot: bool) -> UpdateTask {
    UpdateTask {
        update_id: format!("{}-{}", upload.map_or(id_prefix, |u| u.id.as_str()), now_ms()),
        upload_id: upload.map(|u| u.id.clone()),
        version,
        sha256: upload.map(|u| u.sha256.clone()),
        step: step.into(),
        reboot,
        error: None,
        started_at_ms: now_ms(),
    }
}

/// The update lock: one stage, apply, confirm or rollback at a time from helios-api (the writer has
/// its own lock too).
fn take_update_lock(state: &SharedState) -> ApiResult<tokio::sync::OwnedMutexGuard<()>> {
    state.update_lock.clone().try_lock_owned().map_err(|_| ApiError::conflict("another update command is still running"))
}

/// What every writer command needs: the writer installed, nothing staging, and a slots board. A
/// trial is an error unless `allow_trial` (only a confirm is asked of one).
fn ensure_writer_ready(config: &ApiConfig, allow_trial: bool) -> ApiResult<Option<BoardUpdateStatus>> {
    if !tool_installed(config) {
        return Err(match config.board.as_ref() {
            Some(board) => ApiError::not_available(
                format!("the device package writer ({}) is not installed", board.update_tool.display()),
                "the Raze board package's slots writer (/usr/lib/board/slots, Atlas devices/raze with the dynamic slots layout)",
            ),
            None => ApiError::not_available("device updates are not available on this device", "a device package with an update writer (the owner image)"),
        });
    }
    let current = read_board_status(config);
    if let Some(current) = &current {
        if current.state == "staging" {
            return Err(ApiError::conflict("an update is being staged"));
        }
        if !allow_trial && current.on_trial() {
            return Err(ApiError::conflict("the last update is still on trial; wait for it to be confirmed or rolled back"));
        }
        if !current.on_slots_layout() {
            return Err(ApiError::unprocessable("this board is not on the slots layout; reflash it over USB with a current image first"));
        }
    }
    Ok(current)
}

/// Runs one writer command in the background for `task`, holding the update lock until it ends.
/// `done_step` is the task's step when the command succeeds.
fn spawn_writer(state: &SharedState, mut task: UpdateTask, args: Vec<String>, wait: bool, done_step: &'static str, guard: tokio::sync::OwnedMutexGuard<()>) {
    set_task(state, task.clone());
    publish_status(state, "started");
    let state = state.clone();
    tokio::spawn(async move {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        if matches!(args.first(), Some(&("apply" | "boot-once" | "rollback"))) {
            tokio::time::sleep(REBOOT_GRACE).await;
        }
        let result = run_writer(&state.config, &args, wait).await;
        let change = match result {
            Ok(()) => {
                task.step = done_step.into();
                done_step
            }
            Err(error) => {
                tracing::warn!(error = error.message(), step = %task.step, "slots writer command failed");
                task.step = "failed".into();
                task.error = Some(error.message().to_string());
                "failed"
            }
        };
        set_task(&state, task);
        publish_status(&state, change);
        drop(guard);
    });
}

/// Stages an uploaded image, then (with `reboot`) boots the slot on trial. The stage's writer result
/// goes to `answer` as soon as the stage has one: the upload's request waits for it (up to
/// [`STAGE_ANSWER_WINDOW`]).
async fn stage_and_apply(state: SharedState, upload: Upload, mut task: UpdateTask, guard: tokio::sync::OwnedMutexGuard<()>, answer: tokio::sync::oneshot::Sender<Result<(), WriterError>>) {
    let image = upload.path(&state.config).display().to_string();
    let staged = run_writer(&state.config, &["stage-image", &image, "--sha256", &upload.sha256], true).await;
    let change = match &staged {
        Ok(()) => {
            tracing::info!(version = ?task.version, "update staged");
            // The slot holds the image now; free the data partition.
            let _ = tokio::fs::remove_dir_all(state.config.upload_dir.join(&upload.id)).await;
            task.step = if task.reboot { "applying".into() } else { "staged".into() };
            if task.reboot { "applying" } else { "staged" }
        }
        Err(WriterError::Refused(reason)) => {
            tracing::info!(%reason, "update stage refused; nothing changed");
            // The writer marks its state as staging before it checks the running and default slot
            // (after it has unpacked the image); `status` clears that mark.
            let _ = run_writer(&state.config, &["status"], true).await;
            task.step = "failed".into();
            task.error = Some(reason.clone());
            "failed"
        }
        Err(error) => {
            tracing::warn!(error = error.message(), "update stage failed");
            task.step = "failed".into();
            task.error = Some(error.message().to_string());
            "failed"
        }
    };
    set_task(&state, task.clone());
    publish_status(&state, change);
    let _ = answer.send(staged);
    if change != "applying" {
        drop(guard);
        return;
    }
    // `slots apply` boots the staged slot once (on trial) and reboots: started in its own unit and not
    // awaited.
    tokio::time::sleep(REBOOT_GRACE).await;
    match run_writer(&state.config, &["apply"], false).await {
        Ok(()) => tracing::info!(version = ?task.version, "update applied; rebooting into the staged slot on trial"),
        Err(error) => {
            task.step = "failed".into();
            task.error = Some(error.message().to_string());
            set_task(&state, task);
            publish_status(&state, "failed");
        }
    }
    drop(guard);
}

/// Stages an uploaded image, and with `reboot` boots the slot on trial. A refusal or a failure the
/// writer gives within [`STAGE_ANSWER_WINDOW`] is this request's error (409 for a refusal, 422 for a
/// failure); a later one is the task's `failed` step.
async fn apply_upload(state: &SharedState, upload: Upload, version: Option<String>, sha256: Option<String>, reboot: bool) -> ApiResult<ApplyResponse> {
    if let Some(expected) = sha256.filter(|s| !s.is_empty())
        && !expected.eq_ignore_ascii_case(&upload.sha256)
    {
        return Err(ApiError::unprocessable(format!("checksum mismatch: expected {expected}, upload has {}", upload.sha256)));
    }
    ensure_writer_ready(&state.config, false)?;
    let guard = take_update_lock(state)?;
    let version = version_for(&upload, version);
    let task = new_task("", Some(&upload), Some(version.clone()), "staging", reboot);
    let response = ApplyResponse {
        update_id: task.update_id.clone(),
        version: Some(version),
        sha256: Some(upload.sha256.clone()),
        image_url: Some(upload.image_url.clone()),
        reboot,
        message: if reboot {
            "staging the image into an inactive slot; the board then reboots into it on trial and keeps it once healthy".into()
        } else {
            "staging the image into an inactive slot; boot it later with POST /v1/update/apply (no body)".into()
        },
    };
    set_task(state, task.clone());
    publish_status(state, "staging");
    let (answer, answered) = tokio::sync::oneshot::channel();
    tokio::spawn(stage_and_apply(state.clone(), upload, task, guard, answer));
    if let Ok(Ok(Err(refusal))) = tokio::time::timeout(STAGE_ANSWER_WINDOW, answered).await {
        return Err(refusal.into_api());
    }
    Ok(response)
}

/// Boots the slot the writer has staged, once, on trial.
async fn apply_staged(state: &SharedState) -> ApiResult<ApplyResponse> {
    let current = ensure_writer_ready(&state.config, false)?;
    let staged = current.as_ref().and_then(|current| current.slot_staged.clone());
    let Some(staged) = staged else {
        return Err(ApiError::conflict("no slot is staged: upload a slot bundle and apply it"));
    };
    let guard = take_update_lock(state)?;
    let version = current.and_then(|current| current.version_staged);
    let task = new_task(&staged, None, version.clone(), "applying", true);
    let response =
        ApplyResponse { update_id: task.update_id.clone(), version, sha256: None, image_url: None, reboot: true, message: format!("booting slot {staged} on trial; the board keeps it once healthy") };
    spawn_writer(state, task, vec!["apply".into()], false, "applying", guard);
    Ok(response)
}

/// Keeps the trial slot the board is running (`slots confirm`, the same as the boot-time check).
async fn start_confirm(state: &SharedState) -> ApiResult<ActionResponse> {
    let current = ensure_writer_ready(&state.config, true)?;
    let on_trial_now = current.as_ref().is_some_and(|c| c.on_trial() && c.slot_trial == c.slot_active);
    if !on_trial_now {
        let on_trial = current.as_ref().is_some_and(BoardUpdateStatus::on_trial);
        return Err(ApiError::conflict(if on_trial {
            "the trial slot has not booted yet; it can be confirmed only once the board runs it"
        } else {
            "no slot is on trial; there is nothing to confirm"
        }));
    }
    let guard = take_update_lock(state)?;
    let task = new_task("confirm", None, None, "confirming", false);
    let response = ActionResponse { update_id: task.update_id.clone(), action: "confirm".into(), message: "checking the trial slot's health; it is kept when healthy".into() };
    spawn_writer(state, task, vec!["confirm".into()], true, "done", guard);
    Ok(response)
}

/// Boots another installed slot on trial (`slots boot-once <id>`): the board reboots into it, and it
/// is kept once it is judged healthy, as an upload's trial is.
async fn start_switch(state: &SharedState, requested: Option<String>) -> ApiResult<ActionResponse> {
    ensure_board_status(&state.config).await;
    let current = ensure_writer_ready(&state.config, false)?.ok_or_else(|| ApiError::unprocessable("the device package writer has published no state yet"))?;
    let slot_id = |slot: &serde_json::Value| slot.get("id").and_then(|id| id.as_str()).map(str::to_string);
    let target = match requested {
        Some(id) => id,
        None => {
            let others: Vec<String> = current.slots.iter().filter_map(slot_id).filter(|id| current.slot_active.as_ref() != Some(id)).collect();
            match others.as_slice() {
                [only] => only.clone(),
                [] => return Err(ApiError::conflict("no other slot is installed to boot")),
                _ => return Err(ApiError::unprocessable(format!("several other slots are installed ({}): name the slot to boot as {{\"slot\": \"<id>\"}}", others.join(", ")))),
            }
        }
    };
    let entry = current.slots.iter().find(|slot| slot_id(slot).as_deref() == Some(target.as_str())).ok_or_else(|| ApiError::not_found(format!("no installed slot {target}")))?;
    if current.slot_active.as_deref() == Some(target.as_str()) {
        return Err(ApiError::conflict(format!("{target} is the running slot")));
    }
    let version = entry.get("version").and_then(|version| version.as_str()).map(str::to_string);
    let guard = take_update_lock(state)?;
    let task = new_task(&target, None, version, "applying", true);
    let response = ActionResponse { update_id: task.update_id.clone(), action: "switch".into(), message: format!("booting slot {target} on trial; the board keeps it once healthy") };
    spawn_writer(state, task, vec!["boot-once".into(), target], false, "applying", guard);
    Ok(response)
}

/// Makes the previous slot the default again and reboots into it.
async fn start_rollback(state: &SharedState) -> ApiResult<ActionResponse> {
    let current = ensure_writer_ready(&state.config, false)?;
    if current.as_ref().and_then(|c| c.slot_previous.clone()).is_none() {
        return Err(ApiError::conflict("there is no previous slot to go back to"));
    }
    let guard = take_update_lock(state)?;
    let task = new_task("rollback", None, None, "rolling-back", false);
    let response = ActionResponse { update_id: task.update_id.clone(), action: "rollback".into(), message: "making the previous slot the default and rebooting into it".into() };
    spawn_writer(state, task, vec!["rollback".into()], true, "done", guard);
    Ok(response)
}

pub async fn apply(State(state): State<SharedState>, Json(request): Json<ApplyRequest>) -> ApiResult<(StatusCode, Json<ApplyResponse>)> {
    capabilities::require(&state, feature::UPDATES).await?;
    let upload = match (&request.upload_id, &request.image_url) {
        (Some(id), _) => Some(find_upload(&state.config, id).await?),
        (None, Some(url)) => Some(upload_for_url(&state.config, url).await?),
        (None, None) => None,
    };
    let reboot = request.reboot.unwrap_or(true);
    let response = match upload {
        Some(upload) => apply_upload(&state, upload, request.version, request.sha256, reboot).await?,
        None if reboot => apply_staged(&state).await?,
        None => return Err(ApiError::bad_request("reboot: false stages an upload; give upload_id or image_url")),
    };
    Ok((StatusCode::ACCEPTED, Json(response)))
}

pub async fn confirm(State(state): State<SharedState>) -> ApiResult<(StatusCode, Json<ActionResponse>)> {
    capabilities::require(&state, feature::UPDATES).await?;
    Ok((StatusCode::ACCEPTED, Json(start_confirm(&state).await?)))
}

pub async fn rollback(State(state): State<SharedState>) -> ApiResult<(StatusCode, Json<ActionResponse>)> {
    capabilities::require(&state, feature::UPDATES).await?;
    Ok((StatusCode::ACCEPTED, Json(start_rollback(&state).await?)))
}

/// The body of `POST /v1/update/slots/switch` (optional): the slot to boot.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct SwitchRequest {
    pub slot: Option<String>,
}

/// `POST /v1/update/slots/switch`: boots another installed slot on trial. With no body it is the
/// only other installed slot (an error when there are several).
pub async fn switch_slot(State(state): State<SharedState>, body: Bytes) -> ApiResult<(StatusCode, Json<ActionResponse>)> {
    capabilities::require(&state, feature::SLOTS).await?;
    let request =
        if body.iter().all(u8::is_ascii_whitespace) { SwitchRequest::default() } else { serde_json::from_slice(&body).map_err(|error| ApiError::bad_request(format!("switch body: {error}")))? };
    Ok((StatusCode::ACCEPTED, Json(start_switch(&state, request.slot).await?)))
}

/// `POST /v1/update/slots/{command}`: `set-default`, `remove`, `pin` or `unpin` on an installed slot
/// (`{"slot": "<id>"}`). The writer's refusal is a 409 with its reason.
pub async fn slot_command(State(state): State<SharedState>, Path(command): Path<String>, Json(request): Json<SwitchRequest>) -> ApiResult<StatusCode> {
    capabilities::require(&state, feature::SLOTS).await?;
    if !SLOT_COMMANDS.contains(&command.as_str()) {
        return Err(ApiError::not_found(format!("no slot command {command}; there are {}", SLOT_COMMANDS.join(", "))));
    }
    let slot = request.slot.ok_or_else(|| ApiError::bad_request("name the slot as {\"slot\": \"<id>\"}"))?;
    super::check_id(&slot)?;
    ensure_board_status(&state.config).await;
    let current = ensure_writer_ready(&state.config, false)?.ok_or_else(|| ApiError::unprocessable("the device package writer has published no state yet"))?;
    if !current.slots.iter().any(|entry| entry.get("id").and_then(|id| id.as_str()) == Some(slot.as_str())) {
        return Err(ApiError::not_found(format!("no installed slot {slot}")));
    }
    let _guard = take_update_lock(&state)?;
    run_writer(&state.config, &[&command, &slot], true).await.map_err(WriterError::into_api)?;
    publish_status(&state, "slots");
    Ok(StatusCode::NO_CONTENT)
}

// --- status --------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Slots {
    /// The slot this system booted from.
    pub active: Option<String>,
    /// The slot holding a staged (or on-trial) update.
    pub staged: Option<String>,
    /// `slots` (the dynamic-slots layout) or `none` (a guest).
    pub layout: String,
    /// The slot a plain boot starts, the one before it, and a pending trial.
    pub default: Option<String>,
    pub previous: Option<String>,
    pub trial: Option<String>,
    /// Room for more slots, and the space installs must leave free.
    pub free_bytes: Option<u64>,
    pub reserve_bytes: Option<u64>,
    /// Every installed slot, as the device package's writer lists it.
    pub installed: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UpdateStatus {
    /// The writer's state: idle, staging, staged, rebooting, trying, confirmed, rolled-back, error.
    /// A reboot into a trial shows as `trying` (until the trial is judged); `unknown` when the writer
    /// has published none.
    pub phase: String,
    /// The same, in Atlas's stage vocabulary.
    pub stage: String,
    pub progress_percent: Option<u8>,
    pub last_error: Option<String>,
    /// The device package's slots writer is installed.
    pub updater_available: bool,
    pub slots: Slots,
    pub version_active: Option<String>,
    pub version_staged: Option<String>,
    /// What helios-api last asked the writer to do.
    pub task: Option<UpdateTask>,
}

fn read_board_status(config: &ApiConfig) -> Option<BoardUpdateStatus> {
    let board = config.board.as_ref()?;
    board_update::read_status(&board.update_status)
}

fn current_task(state: &SharedState) -> Option<UpdateTask> {
    state.update_task.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone()
}

pub fn status_from(config: &ApiConfig, task: Option<UpdateTask>) -> UpdateStatus {
    let board = read_board_status(config);
    let running = task.as_ref().is_some_and(|t| t.step == "staging");
    let (phase, stage, progress_percent) = match &board {
        // Staging asked for, but the writer has not written its first state yet: it checks the
        // bundle's SHA-256 first.
        Some(board) if running && !matches!(board.state.as_str(), "staging" | "error") => ("staging".to_string(), "verifying".to_string(), Some(0)),
        Some(board) => (board.phase(), board.atlas_stage().to_string(), board.percent()),
        None if running => ("staging".to_string(), "verifying".to_string(), Some(0)),
        None => ("unknown".to_string(), "unknown".to_string(), None),
    };
    let task_error = task.as_ref().filter(|t| t.step == "failed").and_then(|t| t.error.clone());
    let last_error = board.as_ref().filter(|board| matches!(board.state.as_str(), "error" | "rolled-back") || board.error.is_some()).and_then(|board| board.error.clone()).or(task_error);
    UpdateStatus {
        phase,
        stage,
        progress_percent,
        last_error,
        updater_available: tool_installed(config),
        slots: Slots {
            active: board.as_ref().and_then(|board| board.slot_active.clone()),
            staged: board.as_ref().and_then(|board| board.slot_staged.clone()),
            layout: board.as_ref().and_then(|board| board.layout.clone()).unwrap_or_else(|| if config.board.is_some() { "slots" } else { "none" }.into()),
            default: board.as_ref().and_then(|board| board.slot_default.clone()),
            previous: board.as_ref().and_then(|board| board.slot_previous.clone()),
            trial: board.as_ref().and_then(|board| board.slot_trial.clone()),
            free_bytes: board.as_ref().and_then(|board| board.free_bytes),
            reserve_bytes: board.as_ref().and_then(|board| board.reserve_bytes),
            installed: board.as_ref().map(|board| board.slots.clone()).unwrap_or_default(),
        },
        version_active: board.as_ref().and_then(|board| board.version_active.clone()),
        version_staged: board.as_ref().and_then(|board| board.version_staged.clone()),
        task,
    }
}

/// Before the writer has run on this boot `/run/board/update.json` may be missing; `slots status`
/// writes it.
async fn ensure_board_status(config: &ApiConfig) {
    let Some(board) = config.board.as_ref() else {
        return;
    };
    if board.update_status.exists() || !tool_installed(config) {
        return;
    }
    let mut command = tokio::process::Command::new(&board.update_tool);
    command.arg("status").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    let _ = tokio::time::timeout(std::time::Duration::from_secs(10), command.status()).await;
}

pub async fn current_status(state: &SharedState) -> UpdateStatus {
    ensure_board_status(&state.config).await;
    status_from(&state.config, current_task(state))
}

pub async fn status(State(state): State<SharedState>) -> Json<UpdateStatus> {
    Json(current_status(&state).await)
}

/// Update events only (`event: update`), starting with the current status.
pub async fn events(State(state): State<SharedState>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let receiver = state.events.subscribe();
    let first = ApiEvent { kind: "update".into(), at_ms: now_ms(), data: serde_json::to_value(current_status(&state).await).unwrap_or_default() };
    Sse::new(sse_stream(receiver, Some(["update".to_string()].into()), vec![first]).take_until(state.shutdown.clone().cancelled_owned())).keep_alive(KeepAlive::default())
}

/// Shared by the API state: one update command at a time from helios-api (the writer has its own lock).
pub fn new_update_lock() -> Arc<tokio::sync::Mutex<()>> {
    Arc::new(tokio::sync::Mutex::new(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn config(dir: &std::path::Path) -> ApiConfig {
        ApiConfig { upload_dir: dir.join("uploads"), board: Some(test_board(dir)), board_update_systemd_run: false, ui_dir: None, max_upload_bytes: 4096, ..ApiConfig::default() }
    }

    /// The image's board contract, with the writer's files in `dir`.
    fn test_board(dir: &std::path::Path) -> helios_layout::BoardContract {
        helios_layout::BoardContract { update_tool: dir.join("slots"), update_status: dir.join("update.json"), ..helios_layout::BoardContract::image() }
    }

    fn chunks(parts: &[&'static [u8]]) -> impl Stream<Item = Result<Bytes, Infallible>> + Unpin {
        futures_util::stream::iter(parts.iter().map(|p| Ok(Bytes::from_static(p))).collect::<Vec<_>>())
    }

    /// A writer status as `slots status` prints it (the fields the API reads).
    fn writer_status(state: &str, active: &str, staged: &str, default: &str, previous: &str, trial: &str) -> String {
        let staged_version = if staged.is_empty() { "" } else { "v2" };
        serde_json::json!({
            "state": state, "layout": "slots", "slot_active": active, "slot_staged": staged, "slot_default": default,
            "slot_previous": previous, "slot_trial": trial, "version_active": "v1", "version_staged": staged_version,
            "progress": if state == "staged" { 1000 } else { 0 }, "error": "", "free_bytes": 1 << 30, "reserve_bytes": 1 << 20,
            "slots": [{ "id": active, "version": "v1" }],
        })
        .to_string()
    }

    /// Nothing staged, booted from `helios-a`.
    fn idle() -> String {
        writer_status("idle", "helios-a", "", "helios-a", "", "")
    }

    #[tokio::test]
    async fn uploads_older_than_the_keep_time_are_pruned() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config(dir.path());
        let old = store_upload(&config, "old.img.xz", None, None, chunks(&[b"old"])).await.expect("old upload");
        let aged = std::time::SystemTime::now() - UPLOAD_KEEP - std::time::Duration::from_secs(60);
        std::fs::File::open(config.upload_dir.join(&old.id)).and_then(|dir| dir.set_modified(aged)).expect("age the upload");
        let new = store_upload(&config, "new.img.xz", None, None, chunks(&[b"new"])).await.expect("new upload");
        assert_eq!(read_uploads(&config).await, vec![new.clone()]);
        prune_uploads(&config).await;
        assert_eq!(read_uploads(&config).await, vec![new]);
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

        let wrong = store_upload(&config, "x.img.xz", Some(&"0".repeat(64)), None, chunks(&[b"abc"])).await.expect_err("checksum mismatch");
        assert_eq!(wrong.code, crate::error::ErrorCode::Unprocessable);
        let big = store_upload(&config, "x.img.xz", None, None, chunks(&[&[0u8; 4097]])).await.expect_err("too large");
        assert_eq!(big.code, crate::error::ErrorCode::PayloadTooLarge);
        assert!(upload_for_url(&config, "file:///etc/passwd").await.is_err());
    }

    #[tokio::test]
    async fn a_slot_bundle_tar_is_refused_at_upload() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config(dir.path());
        // A ustar header block (the magic at 257) followed by the bundle's first member.
        let mut tar = vec![0u8; 1024];
        tar[..9].copy_from_slice(b"slot.json");
        tar[257..263].copy_from_slice(b"ustar\0");
        let error = store_upload(
            &config,
            "helios-raze-v2.img.xz",
            None,
            None,
            futures_util::stream::iter(vec![Ok::<_, Infallible>(Bytes::copy_from_slice(&tar[..512])), Ok(Bytes::copy_from_slice(&tar[512..]))]),
        )
        .await
        .expect_err("a tar");
        assert_eq!(error.code, crate::error::ErrorCode::Unprocessable);
        assert!(error.message.contains(".img.xz"), "{}", error.message);
        assert!(read_uploads(&config).await.is_empty(), "nothing is kept");
        // Detected by content, not by name: an xz image is kept whatever it is called.
        let image = store_upload(&config, "helios-raze-v2.slot.tar", None, None, chunks(&[b"\xfd7zXZ\0"])).await.expect("an xz image");
        assert_eq!(image.filename, "helios-raze-v2.slot.tar");
    }

    #[test]
    fn status_without_the_writer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let status = status_from(&config(dir.path()), None);
        assert_eq!(status.phase, "unknown");
        assert!(!status.updater_available);
        assert_eq!(status.slots.active, None);
        assert_eq!(status.slots.layout, "slots");
    }

    #[test]
    fn status_follows_the_writer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config(dir.path());
        let board = config.board.as_ref().expect("board");
        std::fs::write(
            &board.update_status,
            r#"{"state":"staging","layout":"slots","slot_active":"helios-a","slot_staged":"helios-b","version_active":"v1","version_staged":"","progress":600,"error":""}"#,
        )
        .expect("write");
        let status = status_from(&config, None);
        assert_eq!(status.phase, "staging");
        assert_eq!(status.stage, "installing");
        assert_eq!(status.progress_percent, Some(60));
        assert_eq!(status.slots.staged.as_deref(), Some("helios-b"));

        std::fs::write(&board.update_status, r#"{"state":"rolled-back","layout":"slots","slot_active":"helios-a","slot_staged":"helios-b","version_active":"v1","version_staged":"v2","progress":1000,"error":"slot helios-b failed its health check"}"#).expect("write");
        let status = status_from(&config, None);
        assert_eq!(status.stage, "rolled_back");
        assert_eq!(status.last_error.as_deref(), Some("slot helios-b failed its health check"));
    }

    /// A stand-in for `/usr/lib/board/slots`: it records its arguments, and each command writes the
    /// state the test gave it (`after-<command>.json`), as the real writer rewrites `update.json`.
    /// `stage-image` checks its image against `--sha256` as the real one does (exit 3: refused, nothing
    /// changed). With `stage_failure` it fails `stage-image` instead, with that exit status and reason.
    fn fake_writer(dir: &std::path::Path, stage_failure: Option<(i32, &str)>) -> PathBuf {
        // A command is logged once it has written its state, so a logged call is a finished one.
        let script = r#"#!/bin/sh
dir='DIR'
case "$1" in
status) cat "$dir/update.json"; exit 0 ;;
stage | stage-image)
	STAGE_FAILURE
	if [ "$(sha256sum "$2" | cut -d' ' -f1)" != "$4" ]; then
		echo "slots: the image doesn't match --sha256; nothing changed" >&2
		exit 3
	fi
	cp "$dir/after-stage-image.json" "$dir/update.json" ;;
apply | boot-once | confirm | rollback) cp "$dir/after-$1.json" "$dir/update.json" ;;
set-default | remove | pin | unpin)
	STAGE_FAILURE
	;;
*) echo "slots: unknown command $1" >&2; exit 2 ;;
esac
echo "$*" >> "$dir/calls.log"
"#;
        let failure = match stage_failure {
            Some((code, reason)) => format!(r#"echo "slots: {reason}" >&2; exit {code}"#),
            None => String::new(),
        };
        let text = script.replace("DIR", &dir.display().to_string()).replace("STAGE_FAILURE", &failure);
        let path = dir.join("slots");
        std::fs::write(&path, text).expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    fn calls(dir: &std::path::Path) -> Vec<String> {
        std::fs::read_to_string(dir.join("calls.log")).unwrap_or_default().lines().map(str::to_string).collect()
    }

    fn set_writer_state(dir: &std::path::Path, name: &str, json: &str) {
        std::fs::write(dir.join(name), json).expect("write");
    }

    async fn wait_for_step(state: &SharedState, step: &str) -> UpdateTask {
        for _ in 0..300 {
            if let Some(task) = current_task(state)
                && task.step == step
            {
                return task;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("task never reached {step}: {:?}", current_task(state));
    }

    /// Waits until the fake writer has logged `count` commands. The wait is for that condition, with
    /// a generous budget: a loaded machine can run the writer's shell script long after the task's
    /// step changed.
    async fn wait_for_calls(dir: &std::path::Path, count: usize) -> Vec<String> {
        for _ in 0..2000 {
            let logged = calls(dir);
            if logged.len() >= count {
                return logged;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        calls(dir)
    }

    #[tokio::test]
    async fn apply_stages_the_image_then_boots_it_on_trial() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = config(dir.path());
        config.board.as_mut().expect("board").update_tool = fake_writer(dir.path(), None);
        set_writer_state(dir.path(), "update.json", &idle());
        set_writer_state(dir.path(), "after-stage-image.json", &writer_status("staged", "helios-a", "helios-b", "helios-a", "", ""));
        set_writer_state(dir.path(), "after-apply.json", &writer_status("rebooting", "helios-a", "helios-b", "helios-a", "", "helios-b"));
        let state = crate::AppState::new(config.clone());
        let upload = store_upload(&config, "helios-raze-v2.img.xz", None, None, chunks(&[b"image"])).await.expect("upload");

        let response = apply_upload(&state, upload.clone(), None, Some(upload.sha256.clone()), true).await.expect("apply");
        assert_eq!(response.version.as_deref(), Some("v2"));
        wait_for_step(&state, "applying").await;
        let logged = wait_for_calls(dir.path(), 2).await;
        let image = upload.path(&config).display().to_string();
        assert_eq!(logged, vec![format!("stage-image {image} --sha256 {}", upload.sha256), "apply".to_string()]);
        assert!(!upload.path(&config).exists(), "a staged upload is removed");
        let status = status_from(&config, current_task(&state));
        assert_eq!(status.phase, "trying");
        assert_eq!(status.stage, "rebooting");
        assert_eq!(status.slots.trial.as_deref(), Some("helios-b"));
    }

    #[tokio::test]
    async fn a_refused_stage_is_a_conflict_and_nothing_boots() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = config(dir.path());
        config.board.as_mut().expect("board").update_tool = fake_writer(dir.path(), Some((3, "the running slot can't be replaced")));
        set_writer_state(dir.path(), "update.json", &idle());
        let state = crate::AppState::new(config.clone());
        let upload = store_upload(&config, "helios-raze-v2.img.xz", None, None, chunks(&[b"image"])).await.expect("upload");
        let error = apply_upload(&state, upload.clone(), None, None, true).await.expect_err("refused");
        assert_eq!(error.code, crate::error::ErrorCode::Conflict);
        assert_eq!(error.message, "the running slot can't be replaced");
        let task = current_task(&state).expect("task");
        assert_eq!(task.step, "failed");
        assert_eq!(task.error.as_deref(), Some("the running slot can't be replaced"));
        assert!(calls(dir.path()).is_empty(), "nothing is staged or booted");
        assert!(upload.path(&config).exists(), "a refused upload is kept");
    }

    #[tokio::test]
    async fn a_failed_stage_is_unprocessable_and_nothing_boots() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = config(dir.path());
        config.board.as_mut().expect("board").update_tool = fake_writer(dir.path(), Some((1, "the image doesn't unpack: not a slots build's image")));
        set_writer_state(dir.path(), "update.json", &idle());
        let state = crate::AppState::new(config.clone());
        let upload = store_upload(&config, "helios-raze-v2.img.xz", None, None, chunks(&[b"image"])).await.expect("upload");
        let error = apply_upload(&state, upload.clone(), None, None, true).await.expect_err("failed");
        assert_eq!(error.code, crate::error::ErrorCode::Unprocessable);
        assert_eq!(error.message, "the image doesn't unpack: not a slots build's image");
        let task = current_task(&state).expect("task");
        assert_eq!(task.step, "failed");
        assert_eq!(status_from(&config, Some(task)).last_error.as_deref(), Some("the image doesn't unpack: not a slots build's image"));
        assert!(!calls(dir.path()).iter().any(|c| c == "apply"), "nothing is booted");
        assert!(upload.path(&config).exists(), "a failed upload is kept");
    }

    #[tokio::test]
    async fn an_image_changed_after_the_upload_is_refused_by_the_writer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = config(dir.path());
        config.board.as_mut().expect("board").update_tool = fake_writer(dir.path(), None);
        set_writer_state(dir.path(), "update.json", &idle());
        let state = crate::AppState::new(config.clone());
        let upload = store_upload(&config, "helios-raze-v2.img.xz", None, None, chunks(&[b"image"])).await.expect("upload");
        std::fs::write(upload.path(&config), b"tampered").expect("tamper");
        let error = apply_upload(&state, upload, None, None, true).await.expect_err("refused");
        assert_eq!(error.code, crate::error::ErrorCode::Conflict);
        assert_eq!(error.message, "the image doesn't match --sha256; nothing changed");
        assert!(!calls(dir.path()).iter().any(|c| c == "apply"));
    }

    #[tokio::test]
    async fn a_wrong_checksum_is_refused_before_the_writer_runs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = config(dir.path());
        config.board.as_mut().expect("board").update_tool = fake_writer(dir.path(), None);
        set_writer_state(dir.path(), "update.json", &idle());
        let state = crate::AppState::new(config.clone());
        let upload = store_upload(&config, "helios-raze-v2.img.xz", None, None, chunks(&[b"image"])).await.expect("upload");
        let error = apply_upload(&state, upload, None, Some("0".repeat(64)), true).await.expect_err("checksum");
        assert_eq!(error.code, crate::error::ErrorCode::Unprocessable);
        assert!(calls(dir.path()).is_empty(), "the writer is not run");
    }

    #[tokio::test]
    async fn apply_without_a_staged_slot_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = config(dir.path());
        config.board.as_mut().expect("board").update_tool = fake_writer(dir.path(), None);
        set_writer_state(dir.path(), "update.json", &idle());
        let state = crate::AppState::new(config);
        let error = apply_staged(&state).await.expect_err("nothing staged");
        assert_eq!(error.code, crate::error::ErrorCode::Conflict);
        assert!(error.message.contains("no slot is staged"), "{}", error.message);
        assert!(calls(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn apply_of_a_staged_slot_boots_it_on_trial() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = config(dir.path());
        config.board.as_mut().expect("board").update_tool = fake_writer(dir.path(), None);
        set_writer_state(dir.path(), "update.json", &writer_status("staged", "helios-a", "helios-b", "helios-a", "", ""));
        set_writer_state(dir.path(), "after-apply.json", &writer_status("rebooting", "helios-a", "helios-b", "helios-a", "", "helios-b"));
        let state = crate::AppState::new(config);
        let response = apply_staged(&state).await.expect("apply");
        assert_eq!(response.version.as_deref(), Some("v2"));
        wait_for_step(&state, "applying").await;
        assert_eq!(wait_for_calls(dir.path(), 1).await, vec!["apply".to_string()]);
    }

    #[tokio::test]
    async fn a_trial_blocks_a_new_update_and_a_rollback() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = config(dir.path());
        config.board.as_mut().expect("board").update_tool = fake_writer(dir.path(), None);
        set_writer_state(dir.path(), "update.json", &writer_status("rebooting", "helios-a", "helios-b", "helios-a", "helios-a", "helios-b"));
        let state = crate::AppState::new(config.clone());
        let upload = store_upload(&config, "image.img.xz", None, None, chunks(&[b"image"])).await.expect("upload");
        assert_eq!(apply_upload(&state, upload, None, None, true).await.expect_err("on trial").code, crate::error::ErrorCode::Conflict);
        assert_eq!(apply_staged(&state).await.expect_err("on trial").code, crate::error::ErrorCode::Conflict);
        assert_eq!(start_rollback(&state).await.expect_err("on trial").code, crate::error::ErrorCode::Conflict);
        assert!(calls(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn confirm_keeps_a_healthy_trial() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = config(dir.path());
        config.board.as_mut().expect("board").update_tool = fake_writer(dir.path(), None);
        // The board runs the trial slot now.
        set_writer_state(dir.path(), "update.json", &writer_status("trying", "helios-b", "helios-b", "helios-a", "helios-a", "helios-b"));
        set_writer_state(dir.path(), "after-confirm.json", &writer_status("confirmed", "helios-b", "", "helios-b", "helios-a", ""));
        let state = crate::AppState::new(config.clone());
        let response = start_confirm(&state).await.expect("confirm");
        assert_eq!(response.action, "confirm");
        wait_for_step(&state, "done").await;
        assert_eq!(calls(dir.path()), vec!["confirm".to_string()]);
        let status = status_from(&config, current_task(&state));
        assert_eq!(status.phase, "confirmed");
        assert_eq!(status.slots.trial, None);
        assert_eq!(status.slots.default.as_deref(), Some("helios-b"));
    }

    #[tokio::test]
    async fn confirm_refuses_when_no_trial_is_running() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = config(dir.path());
        config.board.as_mut().expect("board").update_tool = fake_writer(dir.path(), None);
        let state = crate::AppState::new(config.clone());
        // Nothing on trial.
        set_writer_state(dir.path(), "update.json", &idle());
        assert_eq!(start_confirm(&state).await.expect_err("no trial").code, crate::error::ErrorCode::Conflict);
        // A trial the board has not booted yet: the old slot is still running.
        set_writer_state(dir.path(), "update.json", &writer_status("rebooting", "helios-a", "helios-b", "helios-a", "", "helios-b"));
        let error = start_confirm(&state).await.expect_err("trial not booted");
        assert_eq!(error.code, crate::error::ErrorCode::Conflict);
        assert!(calls(dir.path()).is_empty(), "the writer is not asked to judge a trial that has not run");
    }

    /// A writer state with two installed slots: `helios-a` running and `helios-b` the default.
    fn two_slots(state: &str, trial: &str) -> String {
        serde_json::json!({
            "state": state, "layout": "slots", "slot_active": "helios-a", "slot_staged": "", "slot_default": "helios-b",
            "slot_previous": "", "slot_trial": trial, "version_active": "v1", "version_staged": "", "progress": 0, "error": "",
            "free_bytes": 1 << 30, "reserve_bytes": 1 << 20,
            "slots": [{ "id": "helios-a", "version": "v1" }, { "id": "helios-b", "version": "v2" }],
        })
        .to_string()
    }

    /// A board running `helios-a` with `helios-b` installed too, and the fake writer.
    fn board_with_two_slots(dir: &std::path::Path, failure: Option<(i32, &str)>) -> SharedState {
        let mut config = config(dir);
        config.board.as_mut().expect("board").update_tool = fake_writer(dir, failure);
        let mut status: serde_json::Value = serde_json::from_str(&idle()).expect("status");
        status["slots"] = serde_json::json!([{ "id": "helios-a", "version": "v1" }, { "id": "helios-b", "version": "v2" }]);
        set_writer_state(dir, "update.json", &status.to_string());
        set_writer_state(dir, "after-boot-once.json", &status.to_string());
        crate::AppState::new(config)
    }

    fn slot_request(slot: &str) -> Json<SwitchRequest> {
        Json(SwitchRequest { slot: Some(slot.into()) })
    }

    #[tokio::test]
    async fn slot_commands_run_on_an_installed_slot_and_publish_the_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = board_with_two_slots(dir.path(), None);
        let mut events = state.events.subscribe();
        for command in SLOT_COMMANDS {
            let status = slot_command(State(state.clone()), Path(command.to_string()), slot_request("helios-b")).await.expect("command");
            assert_eq!(status, StatusCode::NO_CONTENT);
            let event = events.recv().await.expect("event");
            assert_eq!((event.kind.as_str(), event.data["change"].as_str()), ("update", Some("slots")));
        }
        assert_eq!(calls(dir.path()), SLOT_COMMANDS.map(|command| format!("{command} helios-b")));
    }

    #[tokio::test]
    async fn slot_commands_refuse_what_the_writer_must_not_see() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = board_with_two_slots(dir.path(), None);
        let run = |command: &str, request: Json<SwitchRequest>| slot_command(State(state.clone()), Path(command.to_string()), request);
        assert_eq!(run("apply", slot_request("helios-b")).await.expect_err("not a slot command").code, crate::error::ErrorCode::NotFound);
        assert_eq!(run("pin", slot_request("helios-c")).await.expect_err("not installed").code, crate::error::ErrorCode::NotFound);
        assert_eq!(run("pin", slot_request("--all")).await.expect_err("not installed").code, crate::error::ErrorCode::NotFound);
        assert_eq!(run("pin", slot_request("a b")).await.expect_err("bad id").code, crate::error::ErrorCode::BadRequest);
        assert_eq!(run("pin", Json(SwitchRequest::default())).await.expect_err("no slot").code, crate::error::ErrorCode::BadRequest);
        assert!(calls(dir.path()).is_empty(), "the writer ran nothing");
    }

    #[tokio::test]
    async fn a_refused_slot_command_is_a_conflict_with_the_writers_reason() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = board_with_two_slots(dir.path(), Some((3, "helios-b is the default slot")));
        let error = slot_command(State(state), Path("remove".into()), slot_request("helios-b")).await.expect_err("refused");
        assert_eq!(error.code, crate::error::ErrorCode::Conflict);
        assert_eq!(error.message, "helios-b is the default slot");
    }

    /// The writer reboots the board: it must not start before the request has its answer.
    #[tokio::test]
    async fn a_reboot_starts_only_after_the_request_is_answered() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = board_with_two_slots(dir.path(), None);
        let response = start_switch(&state, Some("helios-b".into())).await.expect("switch");
        assert_eq!(response.action, "switch");
        assert!(calls(dir.path()).is_empty(), "nothing ran before the answer");
        assert_eq!(wait_for_calls(dir.path(), 1).await, vec!["boot-once helios-b".to_string()]);
    }

    #[tokio::test]
    async fn switch_boots_the_other_slot_on_trial() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = config(dir.path());
        config.board.as_mut().expect("board").update_tool = fake_writer(dir.path(), None);
        set_writer_state(dir.path(), "update.json", &two_slots("idle", ""));
        set_writer_state(dir.path(), "after-boot-once.json", &writer_status("rebooting", "helios-a", "", "helios-b", "helios-a", "helios-b"));
        let state = crate::AppState::new(config.clone());
        // No body: the only other installed slot.
        let response = start_switch(&state, None).await.expect("switch");
        assert_eq!(response.action, "switch");
        wait_for_step(&state, "applying").await;
        assert_eq!(wait_for_calls(dir.path(), 1).await, vec!["boot-once helios-b".to_string()]);
        let status = status_from(&config, current_task(&state));
        assert_eq!(status.slots.trial.as_deref(), Some("helios-b"));
    }

    #[tokio::test]
    async fn switch_refuses_the_running_slot_an_unknown_slot_and_an_ambiguous_choice() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = config(dir.path());
        config.board.as_mut().expect("board").update_tool = fake_writer(dir.path(), None);
        set_writer_state(dir.path(), "update.json", &two_slots("idle", ""));
        let state = crate::AppState::new(config.clone());
        let running = start_switch(&state, Some("helios-a".into())).await.expect_err("running slot");
        assert_eq!(running.code, crate::error::ErrorCode::Conflict);
        let unknown = start_switch(&state, Some("helios-z".into())).await.expect_err("unknown slot");
        assert_eq!(unknown.code, crate::error::ErrorCode::NotFound);
        // Three slots and no name: the switch can't tell which one to boot.
        let mut three: serde_json::Value = serde_json::from_str(&two_slots("idle", "")).expect("json");
        three["slots"].as_array_mut().expect("slots").push(serde_json::json!({ "id": "helios-c", "version": "v3" }));
        set_writer_state(dir.path(), "update.json", &three.to_string());
        let ambiguous = start_switch(&state, None).await.expect_err("ambiguous");
        assert_eq!(ambiguous.code, crate::error::ErrorCode::Unprocessable);
        assert!(calls(dir.path()).is_empty(), "the writer is not asked to boot anything");
    }

    #[tokio::test]
    async fn switch_is_refused_on_a_trial() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = config(dir.path());
        config.board.as_mut().expect("board").update_tool = fake_writer(dir.path(), None);
        set_writer_state(dir.path(), "update.json", &two_slots("trying", "helios-b"));
        let state = crate::AppState::new(config);
        assert_eq!(start_switch(&state, None).await.expect_err("trial").code, crate::error::ErrorCode::Conflict);
        assert!(calls(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn rollback_makes_the_previous_slot_the_default() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = config(dir.path());
        config.board.as_mut().expect("board").update_tool = fake_writer(dir.path(), None);
        set_writer_state(dir.path(), "update.json", &writer_status("confirmed", "helios-b", "", "helios-b", "helios-a", ""));
        set_writer_state(dir.path(), "after-rollback.json", &writer_status("rebooting", "helios-b", "", "helios-a", "helios-b", ""));
        let state = crate::AppState::new(config);
        let response = start_rollback(&state).await.expect("rollback");
        assert_eq!(response.action, "rollback");
        wait_for_step(&state, "done").await;
        assert_eq!(calls(dir.path()), vec!["rollback".to_string()]);
    }

    #[tokio::test]
    async fn rollback_without_a_previous_slot_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = config(dir.path());
        config.board.as_mut().expect("board").update_tool = fake_writer(dir.path(), None);
        set_writer_state(dir.path(), "update.json", &idle());
        let state = crate::AppState::new(config);
        let error = start_rollback(&state).await.expect_err("no previous");
        assert_eq!(error.code, crate::error::ErrorCode::Conflict);
        assert!(calls(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn a_board_off_the_slots_layout_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut config = config(dir.path());
        config.board.as_mut().expect("board").update_tool = fake_writer(dir.path(), None);
        // An A/B image's writer state: no slots layout.
        set_writer_state(dir.path(), "update.json", r#"{"state":"idle","slot_active":"A","slot_staged":"","version_active":"v1","progress":0,"error":""}"#);
        let state = crate::AppState::new(config.clone());
        let upload = store_upload(&config, "image.img.xz", None, None, chunks(&[b"image"])).await.expect("upload");
        let error = apply_upload(&state, upload, None, None, true).await.expect_err("not slots");
        assert_eq!(error.code, crate::error::ErrorCode::Unprocessable);
        assert!(calls(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn apply_refuses_without_the_writer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = config(dir.path());
        let state = crate::AppState::new(config.clone());
        let upload = store_upload(&config, "image.img.xz", None, None, chunks(&[b"image"])).await.expect("upload");
        let error = apply_upload(&state, upload, None, None, true).await.expect_err("no writer");
        assert_eq!(error.code, crate::error::ErrorCode::NotAvailable);
    }

    #[test]
    fn filenames_are_sanitized() {
        assert_eq!(sanitize_filename("/tmp/../evil name.tar"), "evil_name.tar");
        assert_eq!(sanitize_filename(".."), "helios-update.img.xz");
    }
}
