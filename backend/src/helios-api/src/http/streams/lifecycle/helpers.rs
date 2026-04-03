use super::*;

pub(super) fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub(crate) fn descriptor_from_persisted_manifest(manifest: &StreamManifest) -> CaptureDescriptor {
    streams_persist::synthesize_descriptor_snapshot_from_manifest(manifest)
}

pub(crate) fn ensure_descriptor_has_mode(descriptor: &mut CaptureDescriptor, manifest: &StreamManifest) {
    streams_persist::ensure_descriptor_snapshot_has_mode(descriptor, manifest);
}

pub(super) fn stream_list_response(payload: Vec<StreamInfo>, stale: bool) -> Response {
    let mut response = Json(payload).into_response();
    if stale {
        response.headers_mut().insert("x-helios-streams-stale", HeaderValue::from_static("1"));
    }
    response
}

pub(super) fn stream_inspect_response(payload: Vec<StreamInspectInfo>, stale: bool) -> Response {
    let mut response = Json(payload).into_response();
    if stale {
        response.headers_mut().insert("x-helios-streams-stale", HeaderValue::from_static("1"));
    }
    response
}

pub(super) fn maybe_engine_crash_response(state: &AppState, start_ms: u64, _manifest: &StreamManifest) -> Option<Response> {
    let last_disconnect = state.engine.last_disconnect_ms()?;
    if last_disconnect < start_ms {
        return None;
    }

    let reason = "engine restarted while applying stream settings".to_string();
    Some((StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), reason))).into_response())
}

pub(super) fn manifests_conflict(a: &ResolvedStreamConfig, b: &StreamManifest) -> bool {
    a.capture.matches_capture_target(&b.capture)
}

pub(super) fn default_identity_rig_pose() -> helios_engine::ipc::RigPose {
    helios_engine::ipc::RigPose {
        translation: helios_engine::ipc::PoseVector { x: 0.0, y: 0.0, z: 0.0 },
        rotation: helios_engine::ipc::PoseRotation { roll: 0.0, pitch: 0.0, yaw: 0.0 },
        updated_at: None,
    }
}

pub(super) fn recover_from_missing_selected_output(manifest: &mut StreamManifest, code: EngineErrorCode, reason: &str) -> bool {
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

pub(super) fn stream_identity_token_set(stream_id: Uuid, manifest: &StreamManifest) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();

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

pub(super) const DEFAULT_LIBCAMERA_TARGET_FPS: u32 = 30;
pub(super) const OV9782_DEFAULT_LIBCAMERA_TARGET_FPS: u32 = 60;
pub(super) const MAX_INHERITED_LIBCAMERA_TARGET_FPS: u32 = 60;
pub(super) const LIBCAMERA_AE_EXPOSURE_MODE: u32 = 5;
pub(super) const LIBCAMERA_SHARPNESS: u32 = 24;
pub(super) const LIBCAMERA_NOISE_REDUCTION_MODE: u32 = 10002;
pub(super) const OV9782_AE_EXPOSURE_SHORT: i32 = 1;
pub(super) const OV9782_NOISE_REDUCTION_OFF: i32 = 0;
pub(super) const OV9782_DEFAULT_SHARPNESS: f32 = 1.25;

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

pub(super) fn apply_new_ov9782_defaults(manifest: &mut StreamManifest) {
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
