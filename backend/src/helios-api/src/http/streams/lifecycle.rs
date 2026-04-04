use axum::{
    Json,
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use helios_engine::capture::CaptureDescriptor;
use helios_engine::ipc::{EngineErrorCode, EngineEvent, ResolvedStreamConfig, StreamManifest, StreamRuntimeCapabilities, StreamSummary};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::Duration;
use uuid::Uuid;

use crate::http::AppState;
use crate::http::identity_tokens;
use crate::http::revision::{apply_revision_headers, matches_if_none_match, not_modified_response};
use crate::http::streams_persist;
use crate::http::validation::validation_error_response;

use super::sensor_bench;
use super::types::{CodecInfo, StartStreamResponse, StreamInfo, StreamInspectInfo, StreamUpdateAction, UpdateStreamResponse};
use super::util::{
    apply_effective_pipeline_layout, build_stream_info, camera_id_for_manifest, engine_error_body, engine_error_body_with_retryable, list_streams_timeout, map_client_error,
    normalize_pipeline_manifest, normalize_stream_encoder_manifest,
};
use super::validation::validate_stream_manifest_with_runtime;
use super::wait::{wait_for_stream_gone, wait_for_stream_started};
mod helpers;
pub(crate) use helpers::ensure_descriptor_has_mode;
use helpers::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManifestMergeMode {
    CreateRun,
    UpdateExisting,
}

#[derive(Debug, Clone)]
struct PreparedCreateStream {
    requested_id: Uuid,
    manifest: StreamManifest,
    resolved: ResolvedStreamConfig,
    runtime: StreamRuntimeCapabilities,
}

#[derive(Debug, Clone)]
struct PreparedStreamUpdateTransaction {
    stream_id: Uuid,
    manifest: StreamManifest,
    resolved: ResolvedStreamConfig,
    runtime: StreamRuntimeCapabilities,
    previous_running_manifest: Option<StreamManifest>,
}

#[derive(Debug, Clone)]
struct ExistingStreamTarget {
    camera_id: String,
    previous_running_manifest: Option<StreamManifest>,
}

#[derive(Debug, Clone)]
struct StartedStreamRun {
    stream_id: Uuid,
    descriptor: CaptureDescriptor,
}

pub(super) async fn persist_effective_stream_manifest(state: &AppState, camera_id_override: Option<&str>, stream_id: Uuid, requested_manifest: &StreamManifest) -> std::io::Result<()> {
    let (mut effective_manifest, descriptor_snapshot) = state
        .engine
        .list_streams()
        .await
        .ok()
        .and_then(|streams| streams.into_iter().find(|stream| stream.stream_id == stream_id).map(|stream| (stream.manifest, Some(stream.descriptor))))
        .unwrap_or_else(|| (requested_manifest.resolve(), None));
    effective_manifest.capture = helios_engine::capture::canonicalize_capture_config(&effective_manifest.capture);
    match descriptor_snapshot {
        Some(descriptor_snapshot) => {
            if let Some(camera_id) = camera_id_override {
                streams_persist::persist_resolved_config_with_descriptor_checked(camera_id, Some(stream_id), effective_manifest, descriptor_snapshot).await
            } else {
                streams_persist::persist_resolved_config_with_descriptor_auto_camera_id_checked(Some(stream_id), effective_manifest, descriptor_snapshot).await.map(|_| ())
            }
        }
        None => {
            if let Some(camera_id) = camera_id_override {
                streams_persist::persist_resolved_config_checked(camera_id, Some(stream_id), effective_manifest).await
            } else {
                streams_persist::persist_resolved_config_auto_camera_id_checked(Some(stream_id), effective_manifest).await.map(|_| ())
            }
        }
    }
}

async fn lookup_existing_stream_target(state: &AppState, requested_id: Uuid) -> Option<ExistingStreamTarget> {
    let previous_running_manifest = state
        .engine
        .list_streams()
        .await
        .ok()
        .and_then(|active| active.into_iter().find(|stream| stream.stream_id == requested_id && !stream.manifest.internal).map(|stream| stream.manifest.to_requested_manifest()));

    let persisted = streams_persist::list_persisted_records().await;
    let persisted_camera_id = persisted.into_iter().find_map(|record| {
        let Some(manifest) = record.requested_manifest() else {
            return None;
        };
        if manifest.internal {
            return None;
        }
        let record_stream_id = record.stream_id().unwrap_or_else(|| streams_persist::derived_stream_id(&record.camera_id));
        (record_stream_id == requested_id).then_some(record.camera_id)
    });

    let camera_id = persisted_camera_id.or_else(|| previous_running_manifest.as_ref().map(camera_id_for_manifest))?;
    Some(ExistingStreamTarget { camera_id, previous_running_manifest })
}

async fn ensure_unique_stream_identity(state: &AppState, manifest: &StreamManifest, requested_id: Uuid, _camera_id: &str) -> Option<Response> {
    let requested_tokens = stream_identity_token_set(requested_id, manifest);

    if let Ok(active) = state.engine.list_streams().await {
        for stream in active {
            if stream.manifest.internal {
                continue;
            }
            let existing_camera_id = camera_id_for_manifest(&stream.manifest.to_requested_manifest());
            if stream.stream_id == requested_id {
                // `PUT /streams/{id}` may change the effective camera identity. Reusing the same
                // stream ID for that edit is valid; only other stream IDs should conflict here.
                continue;
            }

            let existing_tokens = stream_identity_token_set(stream.stream_id, &stream.manifest.to_requested_manifest());

            if let Some(tok) = requested_tokens.intersection(&existing_tokens).next().cloned() {
                let msg = format!("stream identity token already in use: token=\"{tok}\" conflicts with running stream id=\"{}\" (camera=\"{}\")", stream.stream_id, existing_camera_id);
                return Some((StatusCode::CONFLICT, Json(engine_error_body(Some(EngineErrorCode::Conflict), msg))).into_response());
            }
        }
    }

    let persisted = streams_persist::list_persisted_records().await;
    for record in persisted {
        let Some(record_manifest) = record.requested_manifest() else {
            continue;
        };
        if record_manifest.internal {
            continue;
        }
        let record_stream_id = record.stream_id().unwrap_or_else(|| streams_persist::derived_stream_id(&record.camera_id));
        if record_stream_id == requested_id {
            // Persisted records can migrate to a new camera_id during an update. Skip the
            // self-record here and continue checking for conflicts against other stream IDs.
            continue;
        }

        let existing_tokens = stream_identity_token_set(record_stream_id, &record_manifest);

        if let Some(tok) = requested_tokens.intersection(&existing_tokens).next().cloned() {
            let msg = format!("stream identity token already in use: token=\"{tok}\" conflicts with persisted stream id=\"{}\" (camera=\"{}\")", record_stream_id, record.camera_id);
            return Some((StatusCode::CONFLICT, Json(engine_error_body(Some(EngineErrorCode::Conflict), msg))).into_response());
        }
    }
    None
}

async fn merge_stream_manifest_state(state: &AppState, manifest: &mut StreamManifest, camera_id_override: Option<&str>, mode: ManifestMergeMode) {
    let camera_id = camera_id_override.map(|value| value.to_string()).unwrap_or_else(|| camera_id_for_manifest(manifest));
    let requested_id = manifest.identity.id;

    let mut base_stream_id: Option<Uuid> = None;
    let mut base_manifest: Option<StreamManifest> = None;

    if matches!(mode, ManifestMergeMode::UpdateExisting)
        && let Ok(running) = state.engine.list_streams().await
    {
        if let Some(id) = requested_id
            && let Some(existing) = running.iter().find(|s| s.stream_id == id && !s.manifest.internal)
        {
            base_stream_id = Some(existing.stream_id);
            base_manifest = Some(existing.manifest.to_requested_manifest());
        }

        if base_manifest.is_none()
            && let Some(existing) = running.iter().find(|s| !s.manifest.internal && manifests_conflict(&s.manifest, manifest))
        {
            base_stream_id = Some(existing.stream_id);
            base_manifest = Some(existing.manifest.to_requested_manifest());
        }
    }

    if base_manifest.is_none() {
        base_manifest = streams_persist::load_manifest(&camera_id).await;
    }

    // If our base manifest came from persisted state (rather than a live running stream),
    // we treat it as a "best effort" default and avoid silently inheriting pipeline/multiplex
    // configuration when the client did not ask for it. This prevents multiplex layouts from
    // "leaking" into brand new stream setups.
    let base_from_persist = base_stream_id.is_none() && base_manifest.is_some();

    // If the client did not specify an ID but a conflicting stream is already running for this
    // camera, reuse that stream's ID. This keeps the stream identifier stable across UI-driven
    // reconfigures and avoids making it look like the stream "died" (the old ID gets stopped).
    if matches!(mode, ManifestMergeMode::UpdateExisting)
        && requested_id.is_none()
        && let Some(id) = base_stream_id
    {
        manifest.identity.id = Some(id);
    }

    let Some(base) = base_manifest else {
        apply_new_ov9782_defaults(manifest);
        if manifest.pose.is_none() {
            manifest.pose = Some(default_identity_rig_pose());
        }
        return;
    };

    if manifest.pose.is_none() {
        manifest.pose = base.pose.clone();
    }
    if manifest.calibration.is_none() {
        manifest.calibration = base.calibration.clone();
    }
    if manifest.pipeline_host_inputs.is_empty() {
        manifest.pipeline_host_inputs = base.pipeline_host_inputs.clone();
    }

    // Explicit pipeline disable: allow clients to clear persisted pipeline config.
    if !manifest.pipeline_enabled {
        manifest.pipelines.clear();
        manifest.active_pipeline_id = None;
        manifest.active_pipeline_output = None;
        manifest.pipeline_layout = None;
        manifest.pipeline_wires.clear();
    } else {
        let inherits_pipeline_state = manifest.pipelines.is_empty()
            && manifest.active_pipeline_id.is_none()
            && manifest.active_pipeline_output.is_none()
            && manifest.pipeline_layout.is_none()
            && manifest.pipeline_wires.is_empty();

        if inherits_pipeline_state {
            // Only inherit pipeline state automatically from a *live* stream. Persisted pipeline/multiplex
            // layouts should not be pulled into a new stream unless the client explicitly sends them.
            if base_from_persist {
                // Leave pipelines/layout unset.
            } else {
                manifest.pipelines = base.pipelines.clone();
                manifest.active_pipeline_id = base.active_pipeline_id;
                manifest.active_pipeline_output = base.active_pipeline_output.clone();
                manifest.pipeline_layout = base.pipeline_layout.clone();
                manifest.pipeline_wires = base.pipeline_wires.clone();
            }
        }
    }

    // In single-view mode, preserve the previously-visible slot output when the incoming payload
    // leaves `output_key` empty. This prevents stale UI state from unintentionally resetting
    // RAW output selection (`undistorted` -> `raw`) during full stream re-apply.
    if manifest.pipeline_enabled
        && let Some(layout) = manifest.pipeline_layout.as_mut()
        && let Some(base_layout) = base.pipeline_layout.as_ref()
        && layout.rows == 1
        && layout.columns == 1
    {
        let mut selected_slot_idx = layout.slots.iter().position(|slot| slot.row == 0 && slot.column == 0);
        if selected_slot_idx.is_none() {
            selected_slot_idx = layout.slots.iter().position(|slot| slot.pipeline_id.is_some());
        }
        if let Some(slot_idx) = selected_slot_idx {
            let slot = &mut layout.slots[slot_idx];
            let slot_output_missing = slot.output_key.as_deref().map(str::trim).is_none_or(|value| value.is_empty());
            if slot_output_missing {
                let inherited = base_layout
                    .slots
                    .iter()
                    .find(|base_slot| base_slot.row == slot.row && base_slot.column == slot.column)
                    .or_else(|| base_layout.slots.iter().find(|base_slot| base_slot.pipeline_id == slot.pipeline_id))
                    .and_then(|base_slot| base_slot.output_key.as_deref().map(str::trim).filter(|value| !value.is_empty()).map(|value| value.to_string()))
                    .or_else(|| {
                        if base.active_pipeline_id == slot.pipeline_id {
                            base.active_pipeline_output.as_deref().map(str::trim).filter(|value| !value.is_empty()).map(|value| value.to_string())
                        } else {
                            None
                        }
                    });
                if inherited.is_some() {
                    slot.output_key = inherited;
                }
            }
        }
    }

    normalize_pipeline_manifest(manifest);

    // Preserve codec selections only when restarting the *same* capture format.
    // Carrying a decoder across formats can make the next start fail (decoder input mismatch).
    let requested_fourcc = manifest.capture.mode.format.code;
    let base_fourcc = base.capture.mode.format.code;
    let same_format = requested_fourcc == base_fourcc;
    let same_mode = manifest.capture.mode.format == base.capture.mode.format;
    if same_format {
        manifest.encoder.ensure_id(base.encoder.id().map(ToString::to_string));
        manifest.decoder.ensure_id(base.decoder.id().map(ToString::to_string));
        manifest.encoder.ensure_settings(base.encoder.settings().cloned());
        manifest.decoder.ensure_settings(base.decoder.settings().cloned());
    }
    normalize_stream_encoder_manifest(manifest);

    // Preserve capture interval unless explicitly provided.
    // Libcamera FPS should not be pinned by a persisted `interval`. Prefer the explicit FPS field
    // and allow clients to omit interval without inheriting stale values.
    if manifest.capture.backend == styx::BackendKind::Libcamera {
        if manifest.capture.target_fps.is_none() {
            let inherited = if same_mode { base.capture.target_fps } else { None };
            manifest.capture.target_fps = inherited.filter(|fps| *fps > 0 && *fps <= MAX_INHERITED_LIBCAMERA_TARGET_FPS).or(Some(DEFAULT_LIBCAMERA_TARGET_FPS));
        }
    } else if manifest.capture.target_fps.is_none() {
        manifest.capture.target_fps = base.capture.target_fps;
    }
    if manifest.capture.backend != styx::BackendKind::Libcamera && manifest.capture.interval.is_none() {
        manifest.capture.interval = base.capture.interval;
    }

    // Preserve autostart unless explicitly disabled.
    if !manifest.start_on_boot && base.start_on_boot {
        manifest.start_on_boot = true;
    }

    let preserve_existing_libcamera_controls = manifest.capture.backend == styx::BackendKind::Libcamera && manifest.capture.controls.is_empty();

    // Merge capture controls so applying stream settings doesn't clear previously-set values.
    let mut merged: std::collections::BTreeMap<u32, helios_engine::capture::CaptureControlValue> = std::collections::BTreeMap::new();

    if manifest.capture.backend != styx::BackendKind::Libcamera
        && let Some(id) = base_stream_id
        && let Ok(EngineEvent::Controls { controls, .. }) = state.engine.get_controls(id).await
    {
        for ctl in controls {
            if matches!(ctl.access, styx::core::controls::Access::ReadOnly) {
                continue;
            }
            let value = ctl.value.clone().unwrap_or_else(|| ctl.default.clone());
            merged.insert(ctl.id, value);
        }
    }

    if merged.is_empty() && (manifest.capture.backend != styx::BackendKind::Libcamera || preserve_existing_libcamera_controls) {
        for ctl in &base.capture.controls {
            merged.insert(ctl.id, ctl.value.clone());
        }
    }

    if merged.is_empty() && manifest.capture.controls.is_empty() {
        return;
    }

    for ctl in &manifest.capture.controls {
        merged.insert(ctl.id, ctl.value.clone());
    }
    // If the stream is libcamera-driven and a target FPS is present, drop FrameDurationLimits so
    // it doesn't "stick" to an old value.
    if manifest.capture.backend == styx::BackendKind::Libcamera && manifest.capture.target_fps.is_some() {
        merged.remove(&30);
    }
    manifest.capture.controls = merged.into_iter().map(|(id, value)| helios_engine::capture::ControlAssignment { id, value }).collect();

    if manifest.pose.is_none() {
        manifest.pose = Some(default_identity_rig_pose());
    }
}

async fn prepare_resolved_stream_manifest(
    state: &AppState,
    mut manifest: StreamManifest,
    requested_id: Uuid,
    camera_id: &str,
) -> Result<(StreamManifest, ResolvedStreamConfig, StreamRuntimeCapabilities), Response> {
    let runtime = match super::util::resolve_stream_runtime_capabilities(state).await {
        Ok(runtime) => runtime,
        Err(body) => return Err((StatusCode::BAD_GATEWAY, Json(body)).into_response()),
    };

    let resolved = match validate_stream_manifest_with_runtime(manifest, &runtime).await {
        Ok(validated) => {
            if !validated.warnings.is_empty() {
                tracing::info!(
                    stream_id = %requested_id,
                    warning_count = validated.warnings.len(),
                    warnings = ?validated.warnings,
                    "stream manifest sanitized during semantic validation"
                );
            }
            manifest = validated.manifest;
            validated.resolved
        }
        Err(err) => {
            tracing::warn!(
                stream_id = %requested_id,
                issue_count = err.issues.len(),
                warning_count = err.warnings.len(),
                issues = ?err.issues,
                warnings = ?err.warnings,
                "stream update rejected by semantic validator"
            );
            return Err(validation_error_response("stream manifest failed semantic validation", err.issues, err.warnings));
        }
    };

    if let Some(response) = ensure_unique_stream_identity(state, &manifest, requested_id, camera_id).await {
        return Err(response);
    }

    Ok((manifest, resolved, runtime))
}

async fn prepare_create_stream(state: &AppState, manifest: StreamManifest) -> Result<PreparedCreateStream, Response> {
    let mut manifest = manifest;
    manifest.internal = false;
    merge_stream_manifest_state(state, &mut manifest, None, ManifestMergeMode::CreateRun).await;
    let requested_id = *manifest.identity.id.get_or_insert_with(Uuid::new_v4);
    let camera_id = camera_id_for_manifest(&manifest);
    let (manifest, resolved, runtime) = prepare_resolved_stream_manifest(state, manifest, requested_id, &camera_id).await?;
    Ok(PreparedCreateStream { requested_id, manifest, resolved, runtime })
}

async fn prepare_stream_update_transaction(state: &AppState, stream_id: Uuid, manifest: StreamManifest) -> Result<PreparedStreamUpdateTransaction, Response> {
    let Some(existing) = lookup_existing_stream_target(state, stream_id).await else {
        return Err(StatusCode::NOT_FOUND.into_response());
    };

    let mut manifest = manifest;
    manifest.internal = false;
    manifest.identity.id = Some(stream_id);
    merge_stream_manifest_state(state, &mut manifest, Some(&existing.camera_id), ManifestMergeMode::UpdateExisting).await;
    manifest.identity.id = Some(stream_id);

    let (manifest, resolved, runtime) = prepare_resolved_stream_manifest(state, manifest, stream_id, &existing.camera_id).await?;
    Ok(PreparedStreamUpdateTransaction { stream_id, manifest, resolved, runtime, previous_running_manifest: existing.previous_running_manifest })
}

async fn preflight_stream_runtime_activation(state: &AppState, manifest: &StreamManifest, requested_id: Uuid) -> Option<Response> {
    if let Ok(active) = state.engine.list_streams().await
        && let Some(conflict) = active.iter().find(|s| s.manifest.internal && manifests_conflict(&s.manifest, manifest))
    {
        if sensor_bench::is_active_benchmark_stream(state, conflict.stream_id).await {
            return Some((StatusCode::CONFLICT, Json(engine_error_body(Some(helios_engine::ipc::EngineErrorCode::Busy), "device is busy (sensor benchmark is active)"))).into_response());
        }

        let _ = state.engine.stop_stream(conflict.stream_id).await;
        let _ = wait_for_stream_gone(state, conflict.stream_id, Duration::from_secs(10)).await;
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    if let Ok(active) = state.engine.list_streams().await
        && let Some(conflict) = active.iter().find(|s| !s.manifest.internal && s.stream_id != requested_id && manifests_conflict(&s.manifest, manifest))
    {
        let msg = format!("camera is already in use by stream {}", conflict.stream_id);
        return Some((StatusCode::CONFLICT, Json(engine_error_body(Some(EngineErrorCode::Busy), msg))).into_response());
    }

    None
}

async fn stop_stream_for_reconfigure(state: &AppState, stream_id: Uuid) {
    let _ = state.engine.stop_stream(stream_id).await;
    let _ = wait_for_stream_gone(state, stream_id, Duration::from_secs(10)).await;
    tokio::time::sleep(Duration::from_millis(250)).await;
}

async fn apply_stream_runtime_start(
    state: &AppState,
    requested_id: Uuid,
    mut manifest: StreamManifest,
    mut resolved: ResolvedStreamConfig,
    runtime: &StreamRuntimeCapabilities,
) -> Result<(StreamManifest, StartedStreamRun), Response> {
    let start_ms = now_ms();
    let mut output_recovery_attempted = false;

    loop {
        match state.engine.start_stream(resolved.clone()).await {
            Ok(EngineEvent::Started { stream_id, descriptor, .. }) => {
                return Ok((manifest, StartedStreamRun { stream_id, descriptor }));
            }
            Ok(EngineEvent::Nack { code, reason, retryable, .. }) => {
                if let Some(response) = maybe_engine_crash_response(state, start_ms, &manifest) {
                    return Err(response);
                }
                if !output_recovery_attempted && recover_from_missing_selected_output(&mut manifest, code, &reason) {
                    output_recovery_attempted = true;
                    tracing::warn!(stream_id = %requested_id, reason = %reason, "stream start failed due to stale selected output; cleared output selections and retrying");
                    match validate_stream_manifest_with_runtime(manifest.clone(), runtime).await {
                        Ok(validated) => {
                            manifest = validated.manifest;
                            resolved = validated.resolved;
                        }
                        Err(err) => {
                            return Err(validation_error_response("stream manifest failed semantic validation after output recovery", err.issues, err.warnings));
                        }
                    }
                    continue;
                }
                let status = if retryable { StatusCode::SERVICE_UNAVAILABLE } else { StatusCode::BAD_REQUEST };
                return Err((status, Json(engine_error_body_with_retryable(Some(code), reason, Some(retryable)))).into_response());
            }
            Ok(_) | Err(_) => match wait_for_stream_started(state, requested_id, Duration::from_secs(20)).await {
                Ok(Some(descriptor)) => {
                    return Ok((manifest, StartedStreamRun { stream_id: requested_id, descriptor }));
                }
                Ok(None) => {
                    if let Some(response) = maybe_engine_crash_response(state, start_ms, &manifest) {
                        return Err(response);
                    }
                    return Err((StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), "engine did not confirm stream start"))).into_response());
                }
                Err(err) => return Err(map_client_error(err)),
            },
        }
    }
}

async fn rollback_stream_update_runtime(state: &AppState, stream_id: Uuid, previous_manifest: StreamManifest) -> bool {
    let runtime = match super::util::resolve_stream_runtime_capabilities(state).await {
        Ok(runtime) => runtime,
        Err(body) => {
            tracing::error!(stream_id = %stream_id, error = %body.error, "stream update rollback could not load runtime capabilities");
            return false;
        }
    };

    let mut manifest = previous_manifest;
    manifest.internal = false;
    manifest.identity.id = Some(stream_id);
    let resolved = match validate_stream_manifest_with_runtime(manifest.clone(), &runtime).await {
        Ok(validated) => {
            manifest = validated.manifest;
            validated.resolved
        }
        Err(err) => {
            tracing::error!(
                stream_id = %stream_id,
                issue_count = err.issues.len(),
                warning_count = err.warnings.len(),
                issues = ?err.issues,
                warnings = ?err.warnings,
                "stream update rollback failed semantic validation"
            );
            return false;
        }
    };

    stop_stream_for_reconfigure(state, stream_id).await;
    match apply_stream_runtime_start(state, stream_id, manifest, resolved, &runtime).await {
        Ok(_) => {
            state.services.streams.invalidate_stream_list_cache().await;
            true
        }
        Err(response) => {
            tracing::error!(stream_id = %stream_id, status = %response.status(), "stream update rollback failed to restore prior runtime");
            false
        }
    }
}

pub(crate) async fn get_stream(state: AppState, id: Uuid) -> Response {
    match state.engine.list_streams().await {
        Ok(streams) => match streams.into_iter().find(|s| s.stream_id == id) {
            Some(StreamSummary { stream_id, mut descriptor, mut manifest, status, runtime }) => {
                if manifest.pose.is_none()
                    && let Some(pose) = streams_persist::pose_for_stream_id(stream_id).await
                {
                    manifest.pose = Some(pose);
                }
                let mut requested = manifest.to_requested_manifest();
                apply_effective_pipeline_layout(&mut requested);
                ensure_descriptor_has_mode(&mut descriptor, &requested);
                Json(build_stream_info(stream_id, descriptor, manifest, Some(status), Some(runtime))).into_response()
            }
            None => {
                // If the stream isn't currently running, fall back to the persisted record so the
                // UI can recover (edit settings/pipeline) without requiring the user to delete and
                // recreate the stream.
                for record in streams_persist::list_persisted_records().await {
                    let Some(mut manifest) = record.requested_manifest() else {
                        continue;
                    };
                    if manifest.internal {
                        continue;
                    }
                    let matches = manifest.identity.id == Some(id) || record.stream_id() == Some(id) || streams_persist::derived_stream_id(&record.camera_id) == id;
                    if !matches {
                        continue;
                    }
                    manifest.identity.id = Some(id);
                    apply_effective_pipeline_layout(&mut manifest);
                    let descriptor = streams_persist::descriptor_snapshot_for_record(&record).unwrap_or_else(|| descriptor_from_persisted_manifest(&manifest));
                    return Json(build_stream_info(id, descriptor, manifest.resolve(), None, None)).into_response();
                }

                StatusCode::NOT_FOUND.into_response()
            }
        },
        Err(err) => map_client_error(err),
    }
}

pub(crate) async fn list_streams(state: AppState, headers: axum::http::HeaderMap) -> Response {
    let (payload, stale, revision) = state.services.streams.get_cached_streams_snapshot_with_revision(&state).await;
    if matches_if_none_match(&headers, revision) {
        return not_modified_response(revision);
    }

    let mut response = stream_list_response(payload, stale);
    apply_revision_headers(response.headers_mut(), revision);
    response
}

pub(crate) async fn list_stream_inspect(state: AppState, headers: axum::http::HeaderMap) -> Response {
    let (payload, stale, revision) = state.services.streams.get_cached_streams_snapshot_with_revision(&state).await;
    if matches_if_none_match(&headers, revision) {
        return not_modified_response(revision);
    }

    let inspect = payload.into_iter().map(StreamInspectInfo::from).collect();
    let mut response = stream_inspect_response(inspect, stale);
    apply_revision_headers(response.headers_mut(), revision);
    response
}

pub(crate) async fn start_stream(state: AppState, manifest: StreamManifest) -> Response {
    // Serialize stream starts so identity uniqueness checks (active + persisted) remain reliable.
    let _guard = state.services.streams.stream_start_guard().await;
    let prepared = match prepare_create_stream(&state, manifest).await {
        Ok(prepared) => prepared,
        Err(response) => return response,
    };
    if let Some(response) = preflight_stream_runtime_activation(&state, &prepared.manifest, prepared.requested_id).await {
        return response;
    }

    let (manifest, started) = match apply_stream_runtime_start(&state, prepared.requested_id, prepared.manifest, prepared.resolved, &prepared.runtime).await {
        Ok(started) => started,
        Err(response) => return response,
    };

    if let Err(err) = persist_effective_stream_manifest(&state, None, started.stream_id, &manifest).await {
        let _ = state.engine.stop_stream(started.stream_id).await;
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("stream started live but failed to persist: {err}")))).into_response();
    }

    state.services.streams.invalidate_stream_list_cache().await;
    (StatusCode::OK, Json(StartStreamResponse { stream_id: started.stream_id, descriptor: started.descriptor })).into_response()
}

pub(crate) async fn update_stream(state: AppState, stream_id: Uuid, manifest: StreamManifest) -> Response {
    let _guard = state.services.streams.stream_start_guard().await;

    let prepared = match prepare_stream_update_transaction(&state, stream_id, manifest).await {
        Ok(prepared) => prepared,
        Err(response) => return response,
    };

    if prepared.previous_running_manifest.is_none() {
        match streams_persist::persist_manifest_auto_camera_id_checked(Some(prepared.stream_id), prepared.manifest.clone()).await {
            Ok(_) => {
                state.services.streams.invalidate_stream_list_cache().await;
                let descriptor = descriptor_from_persisted_manifest(&prepared.manifest);
                return (StatusCode::OK, Json(UpdateStreamResponse { stream_id: prepared.stream_id, descriptor, action: StreamUpdateAction::PersistedOnly })).into_response();
            }
            Err(err) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("failed to persist updated stream manifest: {err}")))).into_response();
            }
        }
    }

    if let Some(response) = preflight_stream_runtime_activation(&state, &prepared.manifest, prepared.stream_id).await {
        return response;
    }

    let previous_running_manifest = prepared.previous_running_manifest.clone().expect("checked above");
    stop_stream_for_reconfigure(&state, prepared.stream_id).await;

    let (manifest, started) = match apply_stream_runtime_start(&state, prepared.stream_id, prepared.manifest, prepared.resolved, &prepared.runtime).await {
        Ok(started) => started,
        Err(response) => {
            let rollback_ok = rollback_stream_update_runtime(&state, prepared.stream_id, previous_running_manifest).await;
            if !rollback_ok {
                tracing::error!(stream_id = %prepared.stream_id, "stream update failed and rollback did not restore the prior runtime");
            }
            return response;
        }
    };

    if let Err(err) = persist_effective_stream_manifest(&state, None, started.stream_id, &manifest).await {
        stop_stream_for_reconfigure(&state, started.stream_id).await;
        let rollback_ok = rollback_stream_update_runtime(&state, prepared.stream_id, previous_running_manifest).await;
        if !rollback_ok {
            tracing::error!(stream_id = %prepared.stream_id, "stream update persistence failed and rollback did not restore the prior runtime");
        }
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("updated stream started live but failed to persist: {err}")))).into_response();
    }

    state.services.streams.invalidate_stream_list_cache().await;
    (StatusCode::OK, Json(UpdateStreamResponse { stream_id: started.stream_id, descriptor: started.descriptor, action: StreamUpdateAction::Restarted })).into_response()
}

pub(crate) async fn get_stream_inspect(state: AppState, id: Uuid) -> Response {
    let (payload, _, _) = state.services.streams.get_cached_streams_snapshot_with_revision(&state).await;
    if let Some(stream) = payload.into_iter().find(|stream| stream.id == id) {
        return Json(StreamInspectInfo::from(stream)).into_response();
    }

    for record in streams_persist::list_persisted_records().await {
        let Some(mut manifest) = record.requested_manifest() else {
            continue;
        };
        if manifest.internal {
            continue;
        }
        let matches = manifest.identity.id == Some(id) || record.stream_id() == Some(id) || streams_persist::derived_stream_id(&record.camera_id) == id;
        if !matches {
            continue;
        }
        manifest.identity.id = Some(id);
        apply_effective_pipeline_layout(&mut manifest);
        let descriptor = streams_persist::descriptor_snapshot_for_record(&record).unwrap_or_else(|| descriptor_from_persisted_manifest(&manifest));
        let info = StreamInspectInfo::from(build_stream_info(id, descriptor, manifest.resolve(), None, None));
        return Json(info).into_response();
    }

    StatusCode::NOT_FOUND.into_response()
}

pub(crate) async fn delete_stream(state: AppState, id: Uuid) -> Response {
    // Unregister should be fast and resilient: remove persisted state immediately, and stop the
    // running stream on a best-effort basis (without blocking the HTTP request on engine IPC).
    if let Err(err) = streams_persist::remove_record_by_stream_id(id).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("failed to remove persisted stream record: {err}")))).into_response();
    }
    state.services.streams.invalidate_stream_list_cache().await;

    // If the stream isn't running, we're done.
    if let Ok(streams) = state.engine.list_streams_with_timeout(list_streams_timeout()).await
        && !streams.iter().any(|s| s.stream_id == id)
    {
        return StatusCode::NO_CONTENT.into_response();
    }

    // Request stop, but don't wait long enough for the UI to time out.
    match state.engine.stop_stream_with_timeout(id, Duration::from_secs(2)).await {
        Ok(EngineEvent::Stopped { .. }) => {
            state.services.streams.invalidate_stream_list_cache().await;
            StatusCode::NO_CONTENT.into_response()
        }
        Ok(EngineEvent::Nack { .. }) => StatusCode::NO_CONTENT.into_response(),
        Ok(_) => {
            state.services.streams.invalidate_stream_list_cache().await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(_) => {
            // Fire-and-forget: keep trying in the background so the stream actually stops.
            let state = state.clone();
            tokio::spawn(async move {
                let _ = state.engine.stop_stream_with_timeout(id, Duration::from_secs(20)).await;
                state.services.streams.invalidate_stream_list_cache().await;
            });
            StatusCode::NO_CONTENT.into_response()
        }
    }
}

pub(crate) async fn list_backends(state: AppState) -> Response {
    match state.engine.discover_devices().await {
        Ok(discovery) => Json(discovery.devices).into_response(),
        Err(err) => (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("device discovery failed: {err}")))).into_response(),
    }
}

pub(crate) fn codec_inventory_from_runtime(runtime: StreamRuntimeCapabilities) -> Vec<CodecInfo> {
    runtime.codecs.into_iter().map(CodecInfo::from).collect()
}

pub(crate) async fn list_codecs(state: AppState) -> Response {
    match super::util::resolve_stream_runtime_capabilities(&state).await {
        Ok(runtime) => Json(codec_inventory_from_runtime(runtime)).into_response(),
        Err(body) => (StatusCode::BAD_GATEWAY, Json(body)).into_response(),
    }
}

#[cfg(test)]
mod tests;
