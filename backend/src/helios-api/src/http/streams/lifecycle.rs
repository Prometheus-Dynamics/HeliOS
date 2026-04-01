use axum::{
    Json,
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use helios_engine::capture::CaptureDescriptor;
use helios_engine::ipc::{EngineErrorCode, EngineEvent, ResolvedStreamConfig, StreamManifest, StreamSummary};
use std::time::{SystemTime, UNIX_EPOCH};
use styx::codec::CodecKind;
use styx::codec::CodecRegistry;
use tokio::time::Duration;
use uuid::Uuid;

use crate::http::AppState;
use crate::http::identity_tokens;
use crate::http::revision::{apply_revision_headers, matches_if_none_match, not_modified_response};
use crate::http::streams_persist;
use crate::http::validation::validation_error_response;

use super::sensor_bench;
use super::types::{CodecInfo, CodecTunables, StartStreamResponse, StreamInfo};
use super::util::{
    apply_effective_pipeline_layout, build_stream_info, camera_id_for_manifest, default_encoder_settings_for_codec, engine_error_body, engine_error_body_with_retryable,
    list_streams_timeout, map_client_error, normalize_pipeline_manifest, normalize_stream_encoder_manifest,
};
use super::validation::validate_stream_manifest;
use super::wait::{wait_for_stream_gone, wait_for_stream_started};

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
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
    let camera_id = camera_id_override.map(str::to_string).unwrap_or_else(|| camera_id_for_manifest(&effective_manifest.to_requested_manifest()));
    match descriptor_snapshot {
        Some(descriptor_snapshot) => streams_persist::persist_resolved_config_with_descriptor_checked(&camera_id, Some(stream_id), effective_manifest, descriptor_snapshot).await,
        None => streams_persist::persist_resolved_config_checked(&camera_id, Some(stream_id), effective_manifest).await,
    }
}

pub(crate) fn descriptor_from_persisted_manifest(manifest: &StreamManifest) -> CaptureDescriptor {
    streams_persist::synthesize_descriptor_snapshot_from_manifest(manifest)
}

pub(crate) fn ensure_descriptor_has_mode(descriptor: &mut CaptureDescriptor, manifest: &StreamManifest) {
    streams_persist::ensure_descriptor_snapshot_has_mode(descriptor, manifest);
}

fn stream_list_response(payload: Vec<StreamInfo>, stale: bool) -> Response {
    let mut response = Json(payload).into_response();
    if stale {
        response.headers_mut().insert("x-helios-streams-stale", HeaderValue::from_static("1"));
    }
    response
}

fn maybe_engine_crash_response(state: &AppState, start_ms: u64, _manifest: &StreamManifest) -> Option<Response> {
    let last_disconnect = state.engine.last_disconnect_ms()?;
    if last_disconnect < start_ms {
        return None;
    }

    let reason = "engine restarted while applying stream settings".to_string();
    Some((StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), reason))).into_response())
}

fn manifests_conflict(a: &ResolvedStreamConfig, b: &StreamManifest) -> bool {
    a.capture.matches_capture_target(&b.capture)
}

fn default_identity_rig_pose() -> helios_engine::ipc::RigPose {
    helios_engine::ipc::RigPose {
        translation: helios_engine::ipc::PoseVector { x: 0.0, y: 0.0, z: 0.0 },
        rotation: helios_engine::ipc::PoseRotation { roll: 0.0, pitch: 0.0, yaw: 0.0 },
        updated_at: None,
    }
}

fn recover_from_missing_selected_output(manifest: &mut StreamManifest, code: EngineErrorCode, reason: &str) -> bool {
    if code != EngineErrorCode::InvalidState {
        return false;
    }
    let normalized = reason.to_ascii_lowercase();
    if !(normalized.contains("selected output") && normalized.contains("not found")) {
        return false;
    }

    let mut changed = false;
    if manifest.active_pipeline_output.take().is_some() {
        changed = true;
    }
    for binding in &mut manifest.pipelines {
        if binding.pipeline_output.take().is_some() {
            changed = true;
        }
    }
    if let Some(layout) = manifest.pipeline_layout.as_mut() {
        for slot in &mut layout.slots {
            if slot.output_key.take().is_some() {
                changed = true;
            }
        }
    }
    if changed {
        normalize_pipeline_manifest(manifest);
        apply_effective_pipeline_layout(manifest);
    }
    changed
}

fn stream_identity_token_set(stream_id: Uuid, manifest: &StreamManifest) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();

    // Always include the stream UUID token.
    out.insert(stream_id.to_string());

    if let Some(alias) = manifest.identity.alias.as_deref()
        && let Some(tok) = identity_tokens::normalize_token(alias)
        && !(manifest.capture.backend == styx::BackendKind::File && tok == "media-file")
    {
        out.insert(tok);
    }
    if let Some(hw) = manifest.identity.hardware_id.as_deref()
        && let Some(tok) = identity_tokens::normalize_token(hw)
        && !(manifest.capture.backend == styx::BackendKind::File && tok == "media-file")
    {
        out.insert(tok);
    }

    out
}

const DEFAULT_LIBCAMERA_TARGET_FPS: u32 = 30;
const OV9782_DEFAULT_LIBCAMERA_TARGET_FPS: u32 = 60;
const MAX_INHERITED_LIBCAMERA_TARGET_FPS: u32 = 60;
const LIBCAMERA_AE_EXPOSURE_MODE: u32 = 5;
const LIBCAMERA_SHARPNESS: u32 = 24;
const LIBCAMERA_NOISE_REDUCTION_MODE: u32 = 10002;
const OV9782_AE_EXPOSURE_SHORT: i32 = 1;
const OV9782_NOISE_REDUCTION_OFF: i32 = 0;
const OV9782_DEFAULT_SHARPNESS: f32 = 1.25;

fn token_mentions_ov9782(raw: &str) -> bool {
    raw.to_ascii_lowercase().contains("ov9782")
}

fn manifest_targets_ov9782(manifest: &StreamManifest) -> bool {
    if manifest.capture.backend != styx::BackendKind::Libcamera {
        return false;
    }
    if manifest.capture.device_keys.iter().any(|key| token_mentions_ov9782(key)) {
        return true;
    }
    if let styx::BackendHandle::Libcamera { id } = &manifest.capture.handle
        && token_mentions_ov9782(id)
    {
        return true;
    }
    if let Some(alias) = manifest.identity.alias.as_deref()
        && token_mentions_ov9782(alias)
    {
        return true;
    }
    if let Some(hw) = manifest.identity.hardware_id.as_deref()
        && token_mentions_ov9782(hw)
    {
        return true;
    }
    false
}

fn insert_manifest_control_if_missing(manifest: &mut StreamManifest, id: u32, value: helios_engine::capture::CaptureControlValue) {
    if manifest.capture.controls.iter().any(|control| control.id == id) {
        return;
    }
    manifest.capture.controls.push(helios_engine::capture::ControlAssignment { id, value });
}

fn apply_new_ov9782_defaults(manifest: &mut StreamManifest) {
    if !manifest_targets_ov9782(manifest) {
        return;
    }

    if manifest.capture.target_fps.is_none() {
        manifest.capture.target_fps = Some(OV9782_DEFAULT_LIBCAMERA_TARGET_FPS);
    }

    insert_manifest_control_if_missing(manifest, LIBCAMERA_AE_EXPOSURE_MODE, helios_engine::capture::CaptureControlValue::Int(OV9782_AE_EXPOSURE_SHORT));
    insert_manifest_control_if_missing(manifest, LIBCAMERA_NOISE_REDUCTION_MODE, helios_engine::capture::CaptureControlValue::Int(OV9782_NOISE_REDUCTION_OFF));
    insert_manifest_control_if_missing(manifest, LIBCAMERA_SHARPNESS, helios_engine::capture::CaptureControlValue::Float(OV9782_DEFAULT_SHARPNESS.max(0.0)));
    manifest.capture.enable_tdn_output = false;
}

async fn resolve_stream_owner_camera_id(state: &AppState, requested_id: Uuid) -> Option<String> {
    if let Ok(active) = state.engine.list_streams().await
        && let Some(stream) = active.into_iter().find(|stream| stream.stream_id == requested_id)
    {
        return Some(camera_id_for_manifest(&stream.manifest.to_requested_manifest()));
    }
    let persisted = streams_persist::list_persisted_records().await;
    for record in persisted {
        let Some(_) = record.requested_manifest() else {
            continue;
        };
        let record_stream_id = record.stream_id().unwrap_or_else(|| streams_persist::derived_stream_id(&record.camera_id));
        if record_stream_id == requested_id {
            return Some(record.camera_id);
        }
    }
    None
}

async fn ensure_unique_stream_identity(state: &AppState, manifest: &StreamManifest, requested_id: Uuid, camera_id: &str) -> Option<Response> {
    let requested_tokens = stream_identity_token_set(requested_id, manifest);

    if let Ok(active) = state.engine.list_streams().await {
        for stream in active {
            if stream.manifest.internal {
                continue;
            }
            let existing_camera_id = camera_id_for_manifest(&stream.manifest.to_requested_manifest());
            if stream.stream_id == requested_id {
                if existing_camera_id != camera_id {
                    return Some((StatusCode::CONFLICT, Json(engine_error_body(Some(EngineErrorCode::Conflict), "stream uuid already assigned to another camera"))).into_response());
                }
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
            if record.camera_id != camera_id {
                return Some((StatusCode::CONFLICT, Json(engine_error_body(Some(EngineErrorCode::Conflict), "stream uuid already assigned to another camera"))).into_response());
            }
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

async fn merge_stream_manifest_state(state: &AppState, manifest: &mut StreamManifest, camera_id_override: Option<&str>) {
    let camera_id = camera_id_override.map(|value| value.to_string()).unwrap_or_else(|| camera_id_for_manifest(manifest));
    let requested_id = manifest.identity.id;

    let mut base_stream_id: Option<Uuid> = None;
    let mut base_manifest: Option<StreamManifest> = None;

    if let Ok(running) = state.engine.list_streams().await {
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
    if requested_id.is_none()
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

pub(crate) async fn start_stream(state: AppState, manifest: StreamManifest) -> Response {
    // Serialize stream starts so identity uniqueness checks (active + persisted) remain reliable.
    let _guard = state.services.streams.stream_start_guard().await;

    let mut manifest = manifest;
    let start_ms = now_ms();
    // Only internal system tasks (benchmarks, etc.) may set this; ignore client values.
    manifest.internal = false;

    // IMPORTANT: preserve whether the client provided an explicit stream UUID.
    //
    // `merge_stream_manifest_state` has logic to re-use an existing running stream id when the
    // client does not specify one (e.g. UI applying settings with a partially-populated model).
    // If we eagerly `get_or_insert` here, that branch becomes unreachable and we can end up
    // attempting to start a second stream that immediately conflicts on identity tokens.
    let owner_camera_id = match manifest.identity.id {
        Some(id) => resolve_stream_owner_camera_id(&state, id).await,
        None => None,
    };
    merge_stream_manifest_state(&state, &mut manifest, owner_camera_id.as_deref()).await;

    // After merge, ensure the manifest has a concrete id (either client-provided, re-used, or new).
    let requested_id = *manifest.identity.id.get_or_insert_with(Uuid::new_v4);
    let camera_id = owner_camera_id.clone().unwrap_or_else(|| camera_id_for_manifest(&manifest));

    let mut resolved = match validate_stream_manifest(manifest).await {
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
                "stream start rejected by semantic validator"
            );
            return validation_error_response("stream manifest failed semantic validation", err.issues, err.warnings);
        }
    };
    if let Some(response) = ensure_unique_stream_identity(&state, &manifest, requested_id, &camera_id).await {
        return response;
    }

    // If an internal benchmark stream is currently using this device:
    // - If it's part of an active benchmark job, fail fast (user can cancel the benchmark).
    // - If it's stale/leaked, stop it so registration/start doesn't time out on libcamera busy.
    if let Ok(active) = state.engine.list_streams().await
        && let Some(conflict) = active.iter().find(|s| s.manifest.internal && manifests_conflict(&s.manifest, &manifest))
    {
        if sensor_bench::is_active_benchmark_stream(conflict.stream_id).await {
            return (StatusCode::CONFLICT, Json(engine_error_body(Some(helios_engine::ipc::EngineErrorCode::Busy), "device is busy (sensor benchmark is active)"))).into_response();
        }

        let _ = state.engine.stop_stream(conflict.stream_id).await;
        let _ = wait_for_stream_gone(&state, conflict.stream_id, Duration::from_secs(10)).await;
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    // If another non-internal stream already owns this camera, do not tear it down implicitly.
    // This avoids surprise stream removals when a second client tries to start a stream.
    if let Ok(active) = state.engine.list_streams().await
        && let Some(conflict) = active.iter().find(|s| !s.manifest.internal && s.stream_id != requested_id && manifests_conflict(&s.manifest, &manifest))
    {
        let msg = format!("camera is already in use by stream {}", conflict.stream_id);
        return (StatusCode::CONFLICT, Json(engine_error_body(Some(EngineErrorCode::Busy), msg))).into_response();
    }

    // If the client is "re-starting" an existing stream (same id), stop it first and wait for the
    // capture backend to fully release resources (libcamera is sensitive to rapid acquire()).
    if let Ok(active) = state.engine.list_streams().await
        && active.iter().any(|s| s.stream_id == requested_id)
    {
        let _ = state.engine.stop_stream(requested_id).await;
        let _ = wait_for_stream_gone(&state, requested_id, Duration::from_secs(10)).await;
        tokio::time::sleep(Duration::from_millis(250)).await;
    }

    let mut output_recovery_attempted = false;
    loop {
        match state.engine.start_stream(resolved.clone()).await {
            Ok(EngineEvent::Started { stream_id, descriptor, .. }) => {
                if let Err(err) = persist_effective_stream_manifest(&state, owner_camera_id.as_deref(), stream_id, &manifest).await {
                    return (StatusCode::INTERNAL_SERVER_ERROR, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("stream started live but failed to persist: {err}")))).into_response();
                }
                state.services.streams.invalidate_stream_list_cache().await;
                return (StatusCode::OK, Json(StartStreamResponse { stream_id, descriptor })).into_response();
            }
            Ok(EngineEvent::Nack { code, reason, retryable, .. }) => {
                if let Some(response) = maybe_engine_crash_response(&state, start_ms, &manifest) {
                    return response;
                }
                if !output_recovery_attempted && recover_from_missing_selected_output(&mut manifest, code, &reason) {
                    output_recovery_attempted = true;
                    tracing::warn!(stream_id = %requested_id, reason = %reason, "stream start failed due to stale selected output; cleared output selections and retrying");
                    match validate_stream_manifest(manifest).await {
                        Ok(validated) => {
                            manifest = validated.manifest;
                            resolved = validated.resolved;
                        }
                        Err(err) => {
                            return validation_error_response("stream manifest failed semantic validation after output recovery", err.issues, err.warnings);
                        }
                    }
                    continue;
                }
                let status = if retryable { StatusCode::SERVICE_UNAVAILABLE } else { StatusCode::BAD_REQUEST };
                return (status, Json(engine_error_body_with_retryable(Some(code), reason, Some(retryable)))).into_response();
            }
            Ok(_) | Err(_) => match wait_for_stream_started(&state, requested_id, Duration::from_secs(20)).await {
                Ok(Some(descriptor)) => {
                    if let Err(err) = persist_effective_stream_manifest(&state, owner_camera_id.as_deref(), requested_id, &manifest).await {
                        return (StatusCode::INTERNAL_SERVER_ERROR, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("stream started live but failed to persist: {err}"))))
                            .into_response();
                    }
                    state.services.streams.invalidate_stream_list_cache().await;
                    return (StatusCode::OK, Json(StartStreamResponse { stream_id: requested_id, descriptor })).into_response();
                }
                Ok(None) => {
                    if let Some(response) = maybe_engine_crash_response(&state, start_ms, &manifest) {
                        return response;
                    }
                    return (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), "engine did not confirm stream start"))).into_response();
                }
                Err(err) => return map_client_error(err),
            },
        }
    }
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

pub(crate) fn codec_inventory() -> Result<Vec<CodecInfo>, String> {
    CodecRegistry::list_enabled_codecs()
        .map(|entries| {
            entries
                .into_iter()
                .flat_map(|(fourcc, descs)| {
                    descs.into_iter().map(move |desc| {
                        let tunables = if desc.kind == CodecKind::Encoder {
                            default_encoder_settings_for_codec(fourcc, desc.impl_name).map(|encoder_settings| CodecTunables { encoder_settings: Some(encoder_settings) })
                        } else {
                            None
                        };
                        CodecInfo {
                            kind: desc.kind,
                            fourcc: fourcc.to_string(),
                            name: desc.name.to_string(),
                            implementation: desc.impl_name.to_string(),
                            input: desc.input.to_string(),
                            output: desc.output.to_string(),
                            tunables,
                        }
                    })
                })
                .collect()
        })
        .map_err(|err| err.to_string())
}

pub(crate) async fn list_codecs() -> Response {
    match codec_inventory() {
        Ok(codecs) => Json(codecs).into_response(),
        Err(err) => (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), err.to_string()))).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use helios_engine::capture::{BackendHandle, BackendKind, CaptureConfig, ModeId};
    use helios_engine::identity::DeviceIdentity;
    use std::collections::BTreeMap;
    use styx::prelude::{ColorSpace, FourCc, MediaFormat, Resolution};

    fn sample_ov9782_manifest() -> StreamManifest {
        let format = MediaFormat::new(FourCc::new(*b"NV12"), Resolution::new(1280, 800).unwrap(), ColorSpace::Srgb);
        StreamManifest {
            schema_version: helios_engine::ipc::CURRENT_STREAM_CONFIG_SCHEMA_VERSION,
            identity: DeviceIdentity { id: None, alias: Some("ov9782 cam".to_string()), hardware_id: None },
            capture: CaptureConfig {
                device_keys: vec!["ov9782".to_string()],
                device_identity: None,
                backend: BackendKind::Libcamera,
                handle: BackendHandle::Libcamera { id: "ov9782-main".to_string() },
                mode: ModeId { format, interval: None },
                target_fps: None,
                interval: None,
                controls: Vec::new(),
                enable_tdn_output: true,
            },
            host_buffer: 2,
            internal: false,
            pipeline_enabled: false,
            pipelines: Vec::new(),
            active_pipeline_id: None,
            active_pipeline_output: None,
            pipeline_layout: None,
            pipeline_wires: Vec::new(),
            pipeline_host_inputs: BTreeMap::new(),
            calibration: None,
            pose: None,
            encoder: helios_engine::ipc::RequestedEncoderConfig::default(),
            decoder: helios_engine::ipc::RequestedDecoderConfig::default(),
            preview_jpeg_quality: 30,
            recording_mode: helios_engine::ipc::default_recording_mode(),
            start_on_boot: false,
        }
    }

    #[test]
    fn ov9782_defaults_disable_tdn_output_when_noise_reduction_is_missing() {
        let mut manifest = sample_ov9782_manifest();

        apply_new_ov9782_defaults(&mut manifest);

        assert_eq!(manifest.capture.target_fps, Some(OV9782_DEFAULT_LIBCAMERA_TARGET_FPS));
        assert!(!manifest.capture.enable_tdn_output);
        assert_eq!(
            manifest.capture.controls.iter().find(|ctl| ctl.id == LIBCAMERA_NOISE_REDUCTION_MODE).map(|ctl| ctl.value.clone()),
            Some(helios_engine::capture::CaptureControlValue::Int(OV9782_NOISE_REDUCTION_OFF))
        );
    }

    #[test]
    fn ov9782_defaults_keep_tdn_output_disabled_even_with_nonzero_noise_reduction() {
        let mut manifest = sample_ov9782_manifest();
        manifest.capture.controls.push(helios_engine::capture::ControlAssignment { id: LIBCAMERA_NOISE_REDUCTION_MODE, value: helios_engine::capture::CaptureControlValue::Int(1) });

        apply_new_ov9782_defaults(&mut manifest);

        assert!(!manifest.capture.enable_tdn_output);
    }

    #[test]
    fn ensure_descriptor_has_mode_adds_requested_mode_to_existing_snapshot() {
        let manifest = sample_ov9782_manifest();
        let alternate_format = MediaFormat::new(FourCc::new(*b"RGB3"), Resolution::new(640, 480).unwrap(), ColorSpace::Srgb);
        let mut descriptor = helios_engine::capture::CaptureDescriptor {
            modes: vec![helios_engine::capture::CaptureMode {
                id: ModeId { format: alternate_format, interval: None },
                format: alternate_format,
                intervals: Default::default(),
                interval_stepwise: None,
            }],
            controls: Vec::new(),
        };

        ensure_descriptor_has_mode(&mut descriptor, &manifest);

        assert!(descriptor.modes.iter().any(|mode| mode.id == manifest.capture.mode));
        assert_eq!(descriptor.modes.len(), 2);
    }
}
