use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use helios_engine::capture::{CaptureControlInfo, CaptureControlValue, ControlAssignment};
use helios_engine::ipc::StreamManifest;
use helios_engine::ipc::{EngineErrorCode, EngineEvent};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};
use tokio::time::{sleep, timeout};
use uuid::Uuid;

use crate::http::AppState;

use super::lifecycle;
use super::util::camera_id_for_manifest;
use super::util::{engine_error_body, map_client_error, update_persisted_manifest_by_stream_id};
use crate::http::streams_persist;

const NOISE_REDUCTION_MODE: u32 = 10002;
const CONTROL_CACHE_TTL: Duration = Duration::from_secs(30);
const CONTROL_PERSIST_DEBOUNCE: Duration = Duration::from_millis(250);
const CONTROL_APPLY_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone)]
struct ControlCacheEntry {
    fetched_at: Instant,
    controls: Vec<CaptureControlInfo>,
}

static CONTROL_CACHE: Lazy<RwLock<HashMap<Uuid, ControlCacheEntry>>> = Lazy::new(|| RwLock::new(HashMap::new()));

#[derive(Clone)]
struct ControlPersistEntry {
    version: u64,
    manifest: StreamManifest,
    running: bool,
}

static CONTROL_PERSIST: Lazy<tokio::sync::Mutex<HashMap<Uuid, ControlPersistEntry>>> = Lazy::new(|| tokio::sync::Mutex::new(HashMap::new()));

fn cache_controls(stream_id: Uuid, controls: &[CaptureControlInfo]) {
    let Ok(mut guard) = CONTROL_CACHE.write() else {
        return;
    };
    guard.insert(stream_id, ControlCacheEntry { fetched_at: Instant::now(), controls: controls.to_vec() });
}

fn cached_controls(stream_id: Uuid) -> Option<Vec<CaptureControlInfo>> {
    let Ok(guard) = CONTROL_CACHE.read() else {
        return None;
    };
    let entry = guard.get(&stream_id)?;
    if entry.fetched_at.elapsed() > CONTROL_CACHE_TTL {
        return None;
    }
    Some(entry.controls.clone())
}

async fn queue_control_persist(stream_id: Uuid, manifest: StreamManifest) {
    let (spawn_worker, version) = {
        let mut guard = CONTROL_PERSIST.lock().await;
        let entry = guard.entry(stream_id).or_insert(ControlPersistEntry { version: 0, manifest: manifest.clone(), running: false });
        entry.version = entry.version.saturating_add(1);
        entry.manifest = manifest;
        let spawn_worker = if !entry.running {
            entry.running = true;
            true
        } else {
            false
        };
        (spawn_worker, entry.version)
    };

    if !spawn_worker {
        return;
    }

    tokio::spawn(async move {
        let mut last_version = version;
        loop {
            sleep(CONTROL_PERSIST_DEBOUNCE).await;
            let maybe_manifest = {
                let mut guard = CONTROL_PERSIST.lock().await;
                let Some(entry) = guard.get_mut(&stream_id) else {
                    return;
                };
                if entry.version == last_version {
                    entry.running = false;
                    Some(entry.manifest.clone())
                } else {
                    last_version = entry.version;
                    None
                }
            };

            if let Some(manifest) = maybe_manifest {
                streams_persist::persist_manifest(&camera_id_for_manifest(&manifest), Some(stream_id), manifest).await;
                return;
            }
        }
    });
}

fn numeric_bounds(value: &CaptureControlValue) -> Option<(f64, f64)> {
    match value {
        CaptureControlValue::Int(v) => Some((*v as f64, *v as f64)),
        CaptureControlValue::Uint(v) => Some((*v as f64, *v as f64)),
        CaptureControlValue::Float(v) => Some((*v as f64, *v as f64)),
        CaptureControlValue::Bool(v) => Some((if *v { 1.0 } else { 0.0 }, if *v { 1.0 } else { 0.0 })),
        CaptureControlValue::None => None,
    }
}

fn uint_like(value: &CaptureControlValue) -> Option<u64> {
    match value {
        CaptureControlValue::Uint(v) => Some(*v as u64),
        CaptureControlValue::Int(v) if *v >= 0 => Some(*v as u64),
        _ => None,
    }
}

fn replay_frame_range_adjustment(controls: &[CaptureControlInfo], manifest: &StreamManifest, control_id: u32, value: &CaptureControlValue) -> Option<(u32, CaptureControlValue)> {
    let target = controls.iter().find(|control| control.id == control_id)?;
    let name = target.name.trim();
    let new_value = uint_like(value)?;

    let (prefix, kind) = if let Some(prefix) = name.strip_suffix(".start_frame") {
        (prefix, "start")
    } else if let Some(prefix) = name.strip_suffix(".stop_frame") {
        (prefix, "stop")
    } else {
        return None;
    };

    let sibling_name = if kind == "start" { format!("{prefix}.stop_frame") } else { format!("{prefix}.start_frame") };
    let sibling = controls.iter().find(|control| control.name == sibling_name)?;
    let sibling_current = manifest.capture.controls.iter().find(|assignment| assignment.id == sibling.id).and_then(|assignment| uint_like(&assignment.value));
    let sibling_default = uint_like(&sibling.default);
    let sibling_value = sibling_current.or(sibling_default)?;

    if kind == "start" {
        if new_value > sibling_value {
            return Some((sibling.id, CaptureControlValue::Uint(new_value as u32)));
        }
        return None;
    }

    if new_value < sibling_value {
        return Some((sibling.id, CaptureControlValue::Uint(new_value as u32)));
    }
    None
}

fn validate_control_value(meta: &CaptureControlInfo, value: &CaptureControlValue) -> Result<(), String> {
    // Allow explicit "none" as a best-effort reset/clear operation.
    if matches!(value, CaptureControlValue::None) {
        return Ok(());
    }

    // Ensure the variant matches the control kind.
    match (meta.kind, value) {
        (styx::core::controls::ControlKind::Bool, CaptureControlValue::Bool(_))
        | (styx::core::controls::ControlKind::Int, CaptureControlValue::Int(_))
        | (styx::core::controls::ControlKind::Uint, CaptureControlValue::Uint(_))
        | (styx::core::controls::ControlKind::Float, CaptureControlValue::Float(_))
        // Menus are represented as integers (index).
        | (styx::core::controls::ControlKind::Menu, CaptureControlValue::Uint(_))
        | (styx::core::controls::ControlKind::IntMenu, CaptureControlValue::Int(_)) => {}
        _ => return Err("value type mismatch for control kind".into()),
    }

    // Validate numeric ranges where possible.
    let Some((vmin, vmax)) = numeric_bounds(value) else {
        return Ok(());
    };
    if !vmin.is_finite() || !vmax.is_finite() {
        return Err("value is not finite".into());
    }
    let min = numeric_bounds(&meta.min).map(|x| x.0);
    let max = numeric_bounds(&meta.max).map(|x| x.1);
    if let (Some(min), Some(max)) = (min, max)
        && (vmin < min || vmax > max)
    {
        return Err(format!("value out of range (min={min}, max={max})"));
    }

    // For menus, also ensure the index is inside the menu array length (when provided).
    if matches!(meta.kind, styx::core::controls::ControlKind::Menu | styx::core::controls::ControlKind::IntMenu)
        && let Some(menu) = meta.menu.as_ref()
    {
        let idx = match value {
            CaptureControlValue::Uint(v) => *v as usize,
            CaptureControlValue::Int(v) if *v >= 0 => *v as usize,
            _ => return Err("menu index must be non-negative".into()),
        };
        if idx >= menu.len() {
            return Err(format!("menu index out of range (0..{})", menu.len().saturating_sub(1)));
        }
    }

    Ok(())
}

pub(crate) async fn get_controls(state: AppState, id: Uuid) -> Response {
    let mut result = state.engine.get_controls(id).await;
    if result.is_err() {
        result = state.engine.get_controls(id).await;
    }
    match result {
        Ok(EngineEvent::Controls { controls, .. }) => {
            cache_controls(id, &controls);
            Json::<Vec<CaptureControlInfo>>(controls).into_response()
        }
        Ok(EngineEvent::Nack { code, reason, .. }) => (StatusCode::BAD_REQUEST, Json(engine_error_body(Some(code), reason))).into_response(),
        Ok(_) => (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), "unexpected engine response"))).into_response(),
        Err(err) => map_client_error(err),
    }
}

async fn apply_engine_control(state: &AppState, stream_id: Uuid, control_id: u32, value: CaptureControlValue) -> Result<EngineEvent, Response> {
    match timeout(CONTROL_APPLY_TIMEOUT, state.engine.set_control(stream_id, control_id, value)).await {
        Ok(Ok(event)) => Ok(event),
        Ok(Err(err)) => Err(map_client_error(err)),
        Err(_) => {
            Err((StatusCode::GATEWAY_TIMEOUT, Json(engine_error_body(Some(EngineErrorCode::Timeout), format!("control apply timed out after {}ms", CONTROL_APPLY_TIMEOUT.as_millis()))))
                .into_response())
        }
    }
}

pub(crate) async fn set_control(state: AppState, id: Uuid, control_id: u32, value: CaptureControlValue) -> Response {
    // Validate against live control metadata to prevent invalid/out-of-range values from
    // crashing libcamera (or the stack) when clients send raw numbers.
    let mut controls = cached_controls(id);
    if controls.is_none()
        && let Ok(EngineEvent::Controls { controls: fetched, .. }) = state.engine.get_controls(id).await
    {
        cache_controls(id, &fetched);
        controls = Some(fetched);
    }
    if let Some(ref controls) = controls
        && let Some(meta) = controls.iter().find(|c| c.id == control_id)
        && let Err(msg) = validate_control_value(meta, &value)
    {
        return (StatusCode::BAD_REQUEST, Json(engine_error_body(Some(EngineErrorCode::InvalidInput), msg))).into_response();
    }

    let stream_manifest = state.engine.list_streams().await.ok().and_then(|streams| streams.into_iter().find(|stream| stream.stream_id == id).map(|stream| stream.manifest));
    let is_file_backend = stream_manifest.as_ref().is_some_and(|manifest| manifest.capture.backend == styx::BackendKind::File);
    let adjusted_pair = match (&controls, &stream_manifest) {
        (Some(controls), Some(manifest)) => replay_frame_range_adjustment(controls, manifest, control_id, &value),
        _ => None,
    };
    // Libcamera PiSP TDN/noise-reduction controls are not reliably safe to toggle live: some
    // combinations require a dedicated TDN output stream at configure-time, and applying them
    // via set_control can lead to freezes or hard crashes inside libcamera.
    //
    // Treat any NoiseReductionMode change as "requires restart" on libcamera so we reconfigure
    // capture with the correct TDN output policy and controls applied atomically.
    if control_id == NOISE_REDUCTION_MODE
        && stream_manifest.as_ref().is_some_and(|manifest| manifest.capture.backend == styx::BackendKind::Libcamera)
        && let Some(mut manifest) = stream_manifest.clone()
    {
        apply_control_to_manifest(&mut manifest, control_id, value.clone());
        streams_persist::persist_manifest(&camera_id_for_manifest(&manifest), Some(id), manifest.clone()).await;
        let _ = state.engine.stop_stream(id).await;
        return lifecycle::start_stream(state, manifest).await;
    }

    // For replay start/stop range corrections, apply the sibling first so we never place the
    // backend in a transient invalid range (start > stop or stop < start).
    // This must run for file replay too; otherwise an invalid pair can be persisted and wedge
    // playback after restart.
    let mut controls_to_apply: Vec<(u32, CaptureControlValue)> = Vec::new();
    if let Some((paired_control_id, paired_value)) = adjusted_pair.clone() {
        controls_to_apply.push((paired_control_id, paired_value));
    }
    controls_to_apply.push((control_id, value.clone()));

    let mut apply_err: Option<Response> = None;
    for (apply_id, apply_value) in &controls_to_apply {
        match apply_engine_control(&state, id, *apply_id, apply_value.clone()).await {
            Ok(EngineEvent::Ack { .. }) => {}
            Ok(EngineEvent::Nack { code, reason, .. }) => {
                apply_err = Some((StatusCode::BAD_REQUEST, Json(engine_error_body(Some(code), reason))).into_response());
                break;
            }
            Ok(_) => {
                apply_err = Some((StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), "unexpected engine response"))).into_response());
                break;
            }
            Err(response) => {
                if is_file_backend
                    && response.status() == StatusCode::GATEWAY_TIMEOUT
                    && let Some(mut manifest) = stream_manifest.clone()
                {
                    // File replay controls can hang while seeking/decoder reconfigures. Recover by
                    // restarting the stream with the intended control set so frame output doesn't wedge.
                    for (restart_control_id, restart_control_value) in &controls_to_apply {
                        apply_control_to_manifest(&mut manifest, *restart_control_id, restart_control_value.clone());
                    }
                    streams_persist::persist_manifest(&camera_id_for_manifest(&manifest), Some(id), manifest.clone()).await;
                    let _ = state.engine.stop_stream(id).await;
                    let restart = lifecycle::start_stream(state.clone(), manifest).await;
                    if restart.status().is_success() {
                        return StatusCode::NO_CONTENT.into_response();
                    }
                    return restart;
                }
                apply_err = Some(response);
                break;
            }
        }
    }

    if let Some(err) = apply_err {
        return err;
    }

    // File-backend controls are sanitized/applied in-engine as one coherent set. Persist exactly
    // what the engine now holds to avoid writing stale or transiently-invalid frame ranges.
    if is_file_backend && let Some(manifest) = state.engine.list_streams().await.ok().and_then(|streams| streams.into_iter().find(|stream| stream.stream_id == id).map(|stream| stream.manifest)) {
        queue_control_persist(id, manifest).await;
        return StatusCode::NO_CONTENT.into_response();
    }

    if let Some(mut manifest) = stream_manifest {
        if let Some((paired_control_id, paired_value)) = adjusted_pair {
            apply_control_to_manifest(&mut manifest, paired_control_id, paired_value);
        }
        apply_control_to_manifest(&mut manifest, control_id, value.clone());
        queue_control_persist(id, manifest).await;
    } else {
        let _ = update_persisted_manifest_by_stream_id(id, |manifest| {
            if let Some((paired_control_id, paired_value)) = adjusted_pair.clone() {
                apply_control_to_manifest(manifest, paired_control_id, paired_value);
            }
            apply_control_to_manifest(manifest, control_id, value.clone());
        })
        .await;
    }
    StatusCode::NO_CONTENT.into_response()
}

fn apply_control_to_manifest(manifest: &mut StreamManifest, control_id: u32, value: CaptureControlValue) {
    manifest.capture.controls.retain(|ctl| ctl.id != control_id);
    let enable_tdn_output = control_id == NOISE_REDUCTION_MODE && !matches!(value, CaptureControlValue::Int(0) | CaptureControlValue::None);
    if !matches!(value, CaptureControlValue::None) {
        manifest.capture.controls.push(ControlAssignment { id: control_id, value });
    }
    if control_id == NOISE_REDUCTION_MODE {
        manifest.capture.enable_tdn_output = enable_tdn_output;
    }
}

pub(crate) async fn get_metrics(state: AppState, id: Uuid) -> Response {
    let mut result = state.engine.get_metrics(id).await;
    if result.is_err() {
        result = state.engine.get_metrics(id).await;
    }
    match result {
        Ok(EngineEvent::Metrics { metrics, .. }) => Json(metrics).into_response(),
        Ok(EngineEvent::Nack { code, reason, .. }) => (StatusCode::BAD_REQUEST, Json(engine_error_body(Some(code), reason))).into_response(),
        Ok(_) => (StatusCode::BAD_GATEWAY, Json(engine_error_body(Some(EngineErrorCode::Internal), "unexpected engine response"))).into_response(),
        Err(err) => map_client_error(err),
    }
}
