use super::state::fetch_updater_state;
use super::*;

#[utoipa::path(
    post,
    path = "/ota/stage",
    tag = "OTA",
    request_body = StageUpdateRequest,
    responses(
        (status = 200, description = "Update staged", body = UpdateAckResponse),
        (status = 400, description = "Invalid request", body = UploadUpdateError),
        (status = 503, description = "Updater unavailable", body = UploadUpdateError)
    )
)]
pub async fn stage_update(State(_state): State<AppState>, Json(payload): Json<StageUpdateRequest>) -> impl IntoResponse {
    let artifact_kind = payload.artifact_kind.unwrap_or(ApplyArtifactKind::DiskImage);
    match stage_and_wait_for_update(&_state, &payload.image_url, artifact_kind, payload.size_bytes, payload.checksum.as_deref(), payload.delete_image_after_apply).await {
        Ok(update_id) => (StatusCode::OK, Json(UpdateAckResponse { update_id: Some(update_id.to_string()), message: "update staged".into() })).into_response(),
        Err(err) => err.into_response(),
    }
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
    let update_id = match resolve_apply_request_mode(&payload) {
        Ok(ApplyRequestMode::Existing(update_id)) => update_id,
        Ok(ApplyRequestMode::Transient { image_url, artifact_kind, size_bytes, checksum, delete_image_after_apply }) => {
            match stage_and_wait_for_update(&state, image_url, artifact_kind, size_bytes, checksum, delete_image_after_apply).await {
                Ok(update_id) => update_id,
                Err(err) => return err.into_response(),
            }
        }
        Err(err) => return (StatusCode::BAD_REQUEST, Json(err)).into_response(),
    };
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
    let stopped_streams = if preflight_requires_stream_shutdown(&report) { stop_streams_for_update(&state).await.unwrap_or(0) } else { 0 };
    if let Err(err) = send_apply_release(&state, update_id).await {
        return err.into_response();
    }
    let message = if stopped_streams > 0 { format!("apply scheduled (stopped {} stream{})", stopped_streams, if stopped_streams == 1 { "" } else { "s" }) } else { "apply scheduled".to_string() };
    (StatusCode::OK, Json(UpdateAckResponse { update_id: Some(update_id.to_string()), message })).into_response()
}

pub async fn preflight_update(State(state): State<AppState>, Json(payload): Json<PreflightUpdateRequest>) -> impl IntoResponse {
    match resolve_preflight_request_mode(&payload) {
        Ok(PreflightRequestMode::Active) => {
            let update_id = match active_update_id(&state).await {
                Ok(id) => id,
                Err(err) => return err.into_response(),
            };
            match fetch_updater_preflight(&state, update_id).await {
                Ok(report) => (StatusCode::OK, Json(PreflightUpdateResponse { update_id: update_id.to_string(), ready: report.ready, transient: false, report })).into_response(),
                Err(err) => err.into_response(),
            }
        }
        Ok(PreflightRequestMode::Existing(update_id)) => match fetch_updater_preflight(&state, update_id).await {
            Ok(report) => (StatusCode::OK, Json(PreflightUpdateResponse { update_id: update_id.to_string(), ready: report.ready, transient: false, report })).into_response(),
            Err(err) => err.into_response(),
        },
        Ok(PreflightRequestMode::Transient { image_url, artifact_kind, size_bytes, checksum }) => match run_transient_preflight(&state, image_url, artifact_kind, size_bytes, checksum).await {
            Ok(response) => (StatusCode::OK, Json(response)).into_response(),
            Err(err) => err.into_response(),
        },
        Err(err) => (StatusCode::BAD_REQUEST, Json(err)).into_response(),
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

pub(super) fn resolve_apply_request_mode(payload: &ApplyUpdateRequest) -> Result<ApplyRequestMode<'_>, UploadUpdateError> {
    let update_id = payload.update_id.as_deref().map(str::trim).filter(|value| !value.is_empty());
    let image_url = payload.image_url.as_deref().map(str::trim).filter(|value| !value.is_empty());

    if update_id.is_some() && image_url.is_some() {
        return Err(UploadUpdateError { error: "provide either update_id or image_url for /ota/apply, not both".into() });
    }
    if update_id.is_none() && image_url.is_none() {
        return Err(UploadUpdateError { error: "image_url or update_id is required".into() });
    }
    if image_url.is_none() && (payload.artifact_kind.is_some() || payload.size_bytes.is_some() || payload.checksum.as_deref().is_some_and(|value| !value.trim().is_empty())) {
        return Err(UploadUpdateError { error: "artifact_kind, size_bytes, and checksum are only valid when image_url is provided".into() });
    }

    if let Some(raw) = update_id {
        return match Uuid::parse_str(raw) {
            Ok(id) => Ok(ApplyRequestMode::Existing(id)),
            Err(_) => Err(UploadUpdateError { error: "invalid update_id".into() }),
        };
    }

    Ok(ApplyRequestMode::Transient {
        image_url: image_url.expect("image_url presence checked above"),
        artifact_kind: payload.artifact_kind.unwrap_or(ApplyArtifactKind::DiskImage),
        size_bytes: payload.size_bytes,
        checksum: payload.checksum.as_deref().map(str::trim).filter(|value| !value.is_empty()),
        delete_image_after_apply: payload.delete_image_after_apply,
    })
}

fn preflight_requires_stream_shutdown(report: &PreflightReport) -> bool {
    matches!(report.artifact_kind.as_deref(), Some(kind) if kind == ApplyArtifactKind::DiskImage.manifest_kind())
}

pub(super) fn resolve_preflight_request_mode(payload: &PreflightUpdateRequest) -> Result<PreflightRequestMode<'_>, UploadUpdateError> {
    let update_id = payload.update_id.as_deref().map(str::trim).filter(|value| !value.is_empty());
    let image_url = payload.image_url.as_deref().map(str::trim).filter(|value| !value.is_empty());

    if update_id.is_some() && image_url.is_some() {
        return Err(UploadUpdateError { error: "provide either update_id or image_url for /ota/preflight, not both".into() });
    }
    if image_url.is_none() && (payload.artifact_kind.is_some() || payload.size_bytes.is_some() || payload.checksum.as_deref().is_some_and(|value| !value.trim().is_empty())) {
        return Err(UploadUpdateError { error: "artifact_kind, size_bytes, and checksum are only valid when image_url is provided".into() });
    }

    if let Some(raw) = update_id {
        return match Uuid::parse_str(raw) {
            Ok(id) => Ok(PreflightRequestMode::Existing(id)),
            Err(_) => Err(UploadUpdateError { error: "invalid update_id".into() }),
        };
    }

    if let Some(image_url) = image_url {
        return Ok(PreflightRequestMode::Transient {
            image_url,
            artifact_kind: payload.artifact_kind.unwrap_or(ApplyArtifactKind::DiskImage),
            size_bytes: payload.size_bytes,
            checksum: payload.checksum.as_deref().map(str::trim).filter(|value| !value.is_empty()),
        });
    }

    Ok(PreflightRequestMode::Active)
}

async fn active_update_id(state: &AppState) -> Result<Uuid, UploadUpdateError> {
    let updater_state = fetch_updater_state(state).await?.0;
    match updater_state {
        Some(active) => Ok(active.update_id),
        None => Err(UploadUpdateError { error: "no staged update is active".into() }),
    }
}

async fn run_transient_preflight(
    state: &AppState,
    image_url: &str,
    artifact_kind: ApplyArtifactKind,
    size_bytes: Option<u64>,
    checksum: Option<&str>,
) -> Result<PreflightUpdateResponse, UploadUpdateError> {
    let update_id = stage_update_for_manual_apply(state, image_url, size_bytes, checksum, false, artifact_kind).await?;
    let preflight_result = transient_preflight_inner(state, update_id).await;
    let cleanup_result = ensure_transient_preflight_cleared(state, update_id).await;

    match (preflight_result, cleanup_result) {
        (Ok(report), Ok(())) => Ok(PreflightUpdateResponse { update_id: update_id.to_string(), ready: report.ready, transient: true, report }),
        (Err(err), Ok(())) => Err(err),
        (Ok(_), Err(cleanup_err)) => Err(cleanup_err),
        (Err(err), Err(cleanup_err)) => Err(UploadUpdateError { error: format!("{}; cleanup failed: {}", err.error, cleanup_err.error) }),
    }
}

async fn transient_preflight_inner(state: &AppState, update_id: Uuid) -> Result<PreflightReport, UploadUpdateError> {
    wait_for_staged_update(state, update_id).await?;
    fetch_updater_preflight(state, update_id).await
}

async fn ensure_transient_preflight_cleared(state: &AppState, update_id: Uuid) -> Result<(), UploadUpdateError> {
    let active = fetch_updater_state(state).await?.0;
    if !matches!(active, Some(current) if current.update_id == update_id) {
        return Ok(());
    }
    cancel_update_by_id(state, update_id).await?;
    wait_for_update_cleared(state, update_id).await
}

async fn stage_and_wait_for_update(
    state: &AppState,
    image_url: &str,
    artifact_kind: ApplyArtifactKind,
    size_bytes: Option<u64>,
    checksum: Option<&str>,
    delete_image_after_apply: bool,
) -> Result<Uuid, UploadUpdateError> {
    let update_id = stage_update_for_manual_apply(state, image_url, size_bytes, checksum, delete_image_after_apply, artifact_kind).await?;
    if let Err(err) = wait_for_staged_update(state, update_id).await {
        let _ = cancel_update_by_id(state, update_id).await;
        return Err(err);
    }
    Ok(update_id)
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
    let metadata_json = ReleaseManifestMetadata::manual_stage(delete_image_after_apply, source_artifact_path).encode_json().map_err(|error| UploadUpdateError { error })?;
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

async fn wait_for_update_cleared(state: &AppState, update_id: Uuid) -> Result<(), UploadUpdateError> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let (active, _) = fetch_updater_state(state).await?;
        if !matches!(active, Some(current) if current.update_id == update_id) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(UploadUpdateError { error: format!("timed out waiting for transient preflight cleanup of update {}", update_id) });
        }
        sleep(Duration::from_millis(200)).await;
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
