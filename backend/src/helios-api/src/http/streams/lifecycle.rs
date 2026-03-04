use axum::{
    Json,
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use helios_engine::capture::CaptureDescriptor;
use helios_engine::ipc::{EngineErrorCode, EngineEvent, StreamManifest, StreamSummary};
use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Instant;
use std::time::{SystemTime, UNIX_EPOCH};
use styx::capture::prelude::Mode as CaptureMode;
use styx::codec::CodecKind;
use styx::codec::CodecRegistry;
use tokio::time::Duration;
use uuid::Uuid;

use crate::http::AppState;
use crate::http::identity_tokens;
use crate::http::streams_persist;
use crate::http::validation::validation_error_response;

use super::sensor_bench;
use super::types::{CodecInfo, CodecTunables, StartStreamResponse, StreamInfo};
use super::util::{
    apply_effective_pipeline_layout, camera_id_for_manifest, default_ffmpeg_settings_descriptor, engine_error_body, list_streams_timeout, map_client_error, normalize_pipeline_manifest,
};
use super::validation::validate_stream_manifest;
use super::wait::{wait_for_stream_gone, wait_for_stream_started};

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn descriptor_from_persisted_manifest(manifest: &StreamManifest) -> CaptureDescriptor {
    // Persisted records may exist even when the stream isn't currently running.
    // Synthesize a minimal descriptor so the UI can still render format/resolution and
    // allow the user to re-apply/start the stream without first selecting a backend.
    let mode_id = manifest.capture.mode.clone();
    let format = mode_id.format;
    // Preserve the selected interval (if present) so the UI can show FPS options even when
    // the stream is stopped and only the persisted manifest is available.
    let intervals = mode_id.interval.into_iter().collect();
    let mode = CaptureMode { id: mode_id, format, intervals, interval_stepwise: None };
    CaptureDescriptor { modes: vec![mode], controls: Vec::new() }
}

fn ensure_descriptor_has_mode(descriptor: &mut CaptureDescriptor, manifest: &StreamManifest) {
    if !descriptor.modes.is_empty() {
        return;
    }
    let mode_id = manifest.capture.mode.clone();
    let format = mode_id.format;
    let intervals = mode_id.interval.into_iter().collect();
    descriptor.modes.push(CaptureMode { id: mode_id, format, intervals, interval_stepwise: None });
}

#[derive(Clone)]
struct StreamListCacheEntry {
    fetched_at: Instant,
    payload: Vec<StreamInfo>,
}

fn stream_list_cache() -> &'static tokio::sync::RwLock<Option<StreamListCacheEntry>> {
    static CACHE: OnceLock<tokio::sync::RwLock<Option<StreamListCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| tokio::sync::RwLock::new(None))
}

fn stream_start_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn stream_list_cache_ttl() -> Duration {
    const DEFAULT_MS: u64 = 750;
    const MIN_MS: u64 = 0;
    const MAX_MS: u64 = 5_000;

    let ms = std::env::var("HELIOS_API_STREAMS_CACHE_MS").ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(DEFAULT_MS);

    Duration::from_millis(ms.clamp(MIN_MS, MAX_MS))
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

fn manifests_conflict(a: &StreamManifest, b: &StreamManifest) -> bool {
    if a.capture.device_keys.is_empty() || b.capture.device_keys.is_empty() {
        return false;
    }
    a.capture.device_keys.iter().any(|key| b.capture.device_keys.iter().any(|other| other == key))
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

fn is_legacy_media_file_token(raw: &str) -> bool {
    identity_tokens::normalize_token(raw).as_deref() == Some("media-file")
}

fn sanitize_file_stream_identity(manifest: &mut StreamManifest) {
    if manifest.capture.backend != styx::BackendKind::File {
        return;
    }

    manifest.capture.device_keys.retain(|key| !is_legacy_media_file_token(key));

    let alias_missing = manifest.identity.alias.as_deref().map(str::trim).is_none_or(|value| value.is_empty());
    if alias_missing {
        let fallback = manifest.identity.id.map(|id| format!("media-replay-{id}")).unwrap_or_else(|| format!("media-replay-{}", Uuid::new_v4()));
        manifest.identity.alias = Some(fallback);
    }

    if manifest.identity.hardware_id.as_deref().is_some_and(is_legacy_media_file_token) {
        manifest.identity.hardware_id = manifest.identity.alias.clone().or_else(|| manifest.identity.id.map(|id| id.to_string()));
    }
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
const OV9782_NOISE_REDUCTION_FAST: i32 = 1;
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
    insert_manifest_control_if_missing(manifest, LIBCAMERA_NOISE_REDUCTION_MODE, helios_engine::capture::CaptureControlValue::Int(OV9782_NOISE_REDUCTION_FAST));
    insert_manifest_control_if_missing(manifest, LIBCAMERA_SHARPNESS, helios_engine::capture::CaptureControlValue::Float(OV9782_DEFAULT_SHARPNESS.max(0.0)));
    manifest.capture.enable_tdn_output = true;
}

async fn resolve_stream_owner_camera_id(state: &AppState, requested_id: Uuid) -> Option<String> {
    if let Ok(active) = state.engine.list_streams().await
        && let Some(stream) = active.into_iter().find(|stream| stream.stream_id == requested_id)
    {
        return Some(camera_id_for_manifest(&stream.manifest));
    }
    let persisted = streams_persist::list_persisted_records().await;
    for record in persisted {
        let Some(record_manifest) = record.manifest else {
            continue;
        };
        let record_stream_id = record_manifest.identity.id.or(record.last_stream_id).unwrap_or_else(|| streams_persist::derived_stream_id(&record.camera_id));
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
            let existing_camera_id = camera_id_for_manifest(&stream.manifest);
            if stream.stream_id == requested_id {
                if existing_camera_id != camera_id {
                    return Some((StatusCode::CONFLICT, Json(engine_error_body(Some(EngineErrorCode::Conflict), "stream uuid already assigned to another camera"))).into_response());
                }
                continue;
            }

            let existing_tokens = stream_identity_token_set(stream.stream_id, &stream.manifest);

            if let Some(tok) = requested_tokens.intersection(&existing_tokens).next().cloned() {
                let msg = format!("stream identity token already in use: token=\"{tok}\" conflicts with running stream id=\"{}\" (camera=\"{}\")", stream.stream_id, existing_camera_id);
                return Some((StatusCode::CONFLICT, Json(engine_error_body(Some(EngineErrorCode::Conflict), msg))).into_response());
            }
        }
    }

    let persisted = streams_persist::list_persisted_records().await;
    for record in persisted {
        let Some(record_manifest) = record.manifest else {
            continue;
        };
        if record_manifest.internal {
            continue;
        }
        let record_stream_id = record_manifest.identity.id.or(record.last_stream_id).unwrap_or_else(|| streams_persist::derived_stream_id(&record.camera_id));
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
            base_manifest = Some(existing.manifest.clone());
        }

        if base_manifest.is_none()
            && let Some(existing) = running.iter().find(|s| !s.manifest.internal && manifests_conflict(&s.manifest, manifest))
        {
            base_stream_id = Some(existing.stream_id);
            base_manifest = Some(existing.manifest.clone());
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
    if manifest.pipeline_enabled == Some(false) {
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
    if manifest.pipeline_enabled != Some(false)
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

    // Honor explicit codec enable/disable toggles.
    //
    // We need dedicated boolean toggles because JSON `null` / absent fields are indistinguishable
    // for `Option<T>`. Without these flags, a client cannot reliably disable codecs while
    // restarting the same capture format: `null` gets treated as "not provided" and we inherit
    // persisted codec selections.
    if manifest.encoder_enabled == Some(false) {
        manifest.encoder_id = None;
        manifest.encoder_settings = None;
    }
    if manifest.decoder_enabled == Some(false) {
        manifest.decoder_id = None;
        manifest.decoder_settings = None;
    }

    // Preserve codec selections only when restarting the *same* capture format.
    // Carrying a decoder across formats can make the next start fail (decoder input mismatch).
    let requested_fourcc = manifest.capture.mode.format.code;
    let base_fourcc = base.capture.mode.format.code;
    let same_format = requested_fourcc == base_fourcc;
    let same_mode = manifest.capture.mode.format == base.capture.mode.format;
    if same_format {
        if manifest.encoder_enabled != Some(false) && manifest.encoder_id.is_none() {
            manifest.encoder_id = base.encoder_id.clone();
        }
        if manifest.decoder_enabled != Some(false) && manifest.decoder_id.is_none() {
            manifest.decoder_id = base.decoder_id.clone();
        }
        if manifest.encoder_enabled != Some(false) && manifest.encoder_settings.is_none() {
            manifest.encoder_settings = base.encoder_settings.clone();
        }
        if manifest.decoder_enabled != Some(false) && manifest.decoder_settings.is_none() {
            manifest.decoder_settings = base.decoder_settings.clone();
        }
    }

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

    // Merge capture controls so applying stream settings doesn't clear previously-set values.
    let mut merged: std::collections::BTreeMap<u32, helios_engine::capture::CaptureControlValue> = std::collections::BTreeMap::new();

    if let Some(id) = base_stream_id
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

    if merged.is_empty() {
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

    if manifest.capture.backend == styx::BackendKind::Libcamera {
        let tdn_value = manifest.capture.controls.iter().find(|ctl| ctl.id == LIBCAMERA_NOISE_REDUCTION_MODE).map(|ctl| match &ctl.value {
            helios_engine::capture::CaptureControlValue::Int(v) => *v != 0,
            helios_engine::capture::CaptureControlValue::Uint(v) => *v != 0,
            helios_engine::capture::CaptureControlValue::Float(v) => *v != 0.0,
            helios_engine::capture::CaptureControlValue::Bool(v) => *v,
            helios_engine::capture::CaptureControlValue::None => false,
        });
        if let Some(enabled) = tdn_value {
            manifest.capture.enable_tdn_output = enabled;
        }
    }

    if manifest.pose.is_none() {
        manifest.pose = Some(default_identity_rig_pose());
    }
}

pub(crate) async fn get_stream(state: AppState, id: Uuid) -> Response {
    match state.engine.list_streams().await {
        Ok(streams) => match streams.into_iter().find(|s| s.stream_id == id) {
            Some(StreamSummary { stream_id, mut descriptor, mut manifest, status }) => {
                if manifest.pose.is_none()
                    && let Some(pose) = streams_persist::pose_for_stream_id(stream_id).await
                {
                    manifest.pose = Some(pose);
                }
                apply_effective_pipeline_layout(&mut manifest);
                ensure_descriptor_has_mode(&mut descriptor, &manifest);
                Json(StreamInfo { id: stream_id, descriptor, manifest, status: Some(status) }).into_response()
            }
            None => {
                // If the stream isn't currently running, fall back to the persisted record so the
                // UI can recover (edit settings/pipeline) without requiring the user to delete and
                // recreate the stream.
                for record in streams_persist::list_persisted_records().await {
                    let Some(mut manifest) = record.manifest else {
                        continue;
                    };
                    if manifest.internal {
                        continue;
                    }
                    let matches = manifest.identity.id == Some(id) || record.last_stream_id == Some(id) || streams_persist::derived_stream_id(&record.camera_id) == id;
                    if !matches {
                        continue;
                    }
                    manifest.identity.id = Some(id);
                    apply_effective_pipeline_layout(&mut manifest);
                    let descriptor = descriptor_from_persisted_manifest(&manifest);
                    return Json(StreamInfo { id, descriptor, manifest, status: None }).into_response();
                }

                StatusCode::NOT_FOUND.into_response()
            }
        },
        Err(err) => map_client_error(err),
    }
}

pub(crate) async fn list_streams(state: AppState) -> Response {
    // Avoid blocking the HTTP handler if the engine IPC stalls.
    let ttl = stream_list_cache_ttl();
    if ttl != Duration::from_millis(0)
        && let Some(entry) = stream_list_cache().read().await.clone()
        && entry.fetched_at.elapsed() < ttl
    {
        return stream_list_response(entry.payload, false);
    }

    let mut stale = false;
    let mut out: Vec<StreamInfo> = match state.engine.list_streams_with_timeout(list_streams_timeout()).await {
        Ok(streams) => streams
            .into_iter()
            .filter(|s| !s.manifest.internal)
            .map(|StreamSummary { stream_id, mut descriptor, mut manifest, status }| {
                normalize_pipeline_manifest(&mut manifest);
                apply_effective_pipeline_layout(&mut manifest);
                ensure_descriptor_has_mode(&mut descriptor, &manifest);
                StreamInfo { id: stream_id, descriptor, manifest, status: Some(status) }
            })
            .collect(),
        Err(err) => {
            tracing::warn!(error = %err, "engine list_streams timed out");
            stale = true;
            if let Some(entry) = stream_list_cache().read().await.clone() {
                return stream_list_response(entry.payload, true);
            }
            Vec::new()
        }
    };

    let mut seen_ids: std::collections::BTreeSet<Uuid> = out.iter().map(|s| s.id).collect();
    let mut persisted_ids: std::collections::BTreeSet<Uuid> = std::collections::BTreeSet::new();
    let persisted = streams_persist::list_persisted_records().await;
    let mut pose_by_stream: HashMap<Uuid, _> = HashMap::new();
    for record in &persisted {
        let Some(manifest) = record.manifest.as_ref() else {
            continue;
        };
        if manifest.internal {
            continue;
        }
        let stream_id = manifest.identity.id.or(record.last_stream_id).unwrap_or_else(|| streams_persist::derived_stream_id(&record.camera_id));
        if let Some(pose) = manifest.pose.clone() {
            pose_by_stream.insert(stream_id, pose);
        }
    }
    for stream in &mut out {
        if stream.manifest.pose.is_none()
            && let Some(pose) = pose_by_stream.get(&stream.id).cloned()
        {
            stream.manifest.pose = Some(pose);
        }
    }
    for record in persisted {
        let Some(mut manifest) = record.manifest else {
            continue;
        };
        if manifest.internal {
            continue;
        }
        let stream_id = manifest.identity.id.or(record.last_stream_id).unwrap_or_else(|| streams_persist::derived_stream_id(&record.camera_id));
        if !persisted_ids.insert(stream_id) {
            tracing::warn!(camera_id = %record.camera_id, stream_id = %stream_id, "duplicate persisted stream id; keeping first record");
            continue;
        }
        if seen_ids.contains(&stream_id) {
            tracing::debug!(camera_id = %record.camera_id, stream_id = %stream_id, "persisted stream already running; skipping");
            continue;
        }
        manifest.identity.id = Some(stream_id);
        normalize_pipeline_manifest(&mut manifest);
        apply_effective_pipeline_layout(&mut manifest);
        let descriptor = descriptor_from_persisted_manifest(&manifest);
        out.push(StreamInfo { id: stream_id, descriptor, manifest, status: None });
        seen_ids.insert(stream_id);
    }

    *stream_list_cache().write().await = Some(StreamListCacheEntry { fetched_at: Instant::now(), payload: out.clone() });
    stream_list_response(out, stale)
}

pub(crate) async fn start_stream(state: AppState, manifest: StreamManifest) -> Response {
    // Serialize stream starts so identity uniqueness checks (active + persisted) remain reliable.
    let _guard = stream_start_lock().lock().await;

    let mut manifest = manifest;
    let start_ms = now_ms();
    // Only internal system tasks (benchmarks, etc.) may set this; ignore client values.
    manifest.internal = false;
    sanitize_file_stream_identity(&mut manifest);

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

    // Global feature gate: force shadow recorder off even if a client/persisted manifest requests it.
    if !crate::features::shadow_recorder_enabled() {
        manifest.shadow_recorder_enabled = false;
    }

    match validate_stream_manifest(manifest).await {
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
    }
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

    // Libcamera can return EBUSY briefly after a previous stream stops (buffers still unwinding).
    // Treat that as transient and retry rather than surfacing random start failures to the user.
    let mut attempts = 0u32;
    let mut output_recovery_attempted = false;
    loop {
        attempts += 1;
        match state.engine.start_stream(manifest.clone()).await {
            Ok(EngineEvent::Started { stream_id, descriptor, .. }) => {
                let persist_id = owner_camera_id.clone().unwrap_or_else(|| camera_id_for_manifest(&manifest));
                streams_persist::persist_manifest(&persist_id, Some(stream_id), manifest.clone()).await;
                return (StatusCode::OK, Json(StartStreamResponse { stream_id, descriptor })).into_response();
            }
            Ok(EngineEvent::Nack { code, reason, .. }) => {
                if let Some(response) = maybe_engine_crash_response(&state, start_ms, &manifest) {
                    return response;
                }
                if !output_recovery_attempted && recover_from_missing_selected_output(&mut manifest, code, &reason) {
                    output_recovery_attempted = true;
                    tracing::warn!(stream_id = %requested_id, reason = %reason, "stream start failed due to stale selected output; cleared output selections and retrying");
                    continue;
                }
                let is_transient_busy =
                    matches!(code, helios_engine::ipc::EngineErrorCode::InvalidState) && (reason.contains("Device or resource busy") || reason.contains("resource busy") || reason.contains("EBUSY"));
                if is_transient_busy && attempts < 10 {
                    tokio::time::sleep(Duration::from_millis(50 * attempts as u64)).await;
                    continue;
                }
                return (StatusCode::BAD_REQUEST, Json(engine_error_body(Some(code), reason))).into_response();
            }
            Ok(_) | Err(_) => match wait_for_stream_started(&state, requested_id, Duration::from_secs(20)).await {
                Ok(Some(descriptor)) => {
                    let persist_id = owner_camera_id.clone().unwrap_or_else(|| camera_id_for_manifest(&manifest));
                    streams_persist::persist_manifest(&persist_id, Some(requested_id), manifest.clone()).await;
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
    let _ = streams_persist::remove_record_by_stream_id(id).await;

    // If the stream isn't running, we're done.
    if let Ok(streams) = state.engine.list_streams_with_timeout(list_streams_timeout()).await
        && !streams.iter().any(|s| s.stream_id == id)
    {
        return StatusCode::NO_CONTENT.into_response();
    }

    // Request stop, but don't wait long enough for the UI to time out.
    match state.engine.stop_stream_with_timeout(id, Duration::from_secs(2)).await {
        Ok(EngineEvent::Stopped { .. }) => StatusCode::NO_CONTENT.into_response(),
        Ok(EngineEvent::Nack { .. }) => StatusCode::NO_CONTENT.into_response(),
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => {
            // Fire-and-forget: keep trying in the background so the stream actually stops.
            let state = state.clone();
            tokio::spawn(async move {
                let _ = state.engine.stop_stream_with_timeout(id, Duration::from_secs(20)).await;
            });
            StatusCode::NO_CONTENT.into_response()
        }
    }
}

pub(crate) async fn list_backends() -> Response {
    match tokio::task::spawn_blocking(helios_engine::capture::discover_devices).await {
        Ok(devices) => Json(devices).into_response(),
        Err(err) => (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), format!("device discovery failed: {err}")))).into_response(),
    }
}

pub(crate) async fn list_codecs() -> Response {
    match CodecRegistry::list_enabled_codecs() {
        Ok(entries) => {
            let codecs: Vec<CodecInfo> = entries
                .into_iter()
                .flat_map(|(fourcc, descs)| {
                    descs.into_iter().map(move |desc| {
                        let tunables = if desc.kind == CodecKind::Encoder && desc.impl_name.eq_ignore_ascii_case("ffmpeg") {
                            Some(CodecTunables { encoder_settings: Some(default_ffmpeg_settings_descriptor()) })
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
                .collect();
            Json(codecs).into_response()
        }
        Err(err) => (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), err.to_string()))).into_response(),
    }
}
