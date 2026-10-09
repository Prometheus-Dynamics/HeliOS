//! Cameras: the `camera.device` resources helios-peripherals publishes, with live facts from
//! each camera's Styx camera service (its sources, clients, frame rate, latency and current 3A
//! exposure/gain), their controls (`camera_controls`) and the robot mount the API stores.

use std::{collections::BTreeMap, path::PathBuf, time::Duration};

use axum::{
    Json,
    body::Body,
    extract::{
        Path, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use futures_util::StreamExt;
use serde::Serialize;
use styx::preview::{MJPEG_CONTENT_TYPE, ws_message};

use crate::{
    SharedState,
    camera_context::{self, CameraCalibration},
    camera_controls::{AppliedCameraControl, CameraClient, CameraControl},
    error::{ApiError, ApiResult},
    orion::{StateView, enum_name, label_map},
    store::CameraMount,
};

use super::{check_id, pipelines};

pub const CAMERA_RESOURCE_TYPE: &str = "camera.device";
const STYX_ENDPOINT_PREFIX: &str = "styx-frames+unix://";
const LIVE_TIMEOUT: Duration = Duration::from_secs(3);
/// How often the camera settings keeper looks for cameras in Orion.
const CAMERA_SCAN_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CameraSource {
    pub name: String,
    pub keys: Vec<String>,
    pub in_use: bool,
}

/// One running capture in the camera service.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CaptureLive {
    pub name: String,
    pub backend: String,
    pub mode: String,
    pub fps_configured: Option<f64>,
    pub fps_measured: Option<f64>,
    pub frames_delivered: u64,
    pub drops: u64,
    pub latency_p50_ms: Option<f64>,
    pub latency_p95_ms: Option<f64>,
    pub cpu_per_frame_us: Option<f64>,
    pub exposure_us: Option<f64>,
    pub analogue_gain: Option<f64>,
    pub digital_gain: Option<f64>,
    pub ae_state: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CameraLive {
    pub sources: Vec<CameraSource>,
    pub clients: usize,
    pub frames_sent: u64,
    pub frames_skipped: u64,
    pub restarts: u64,
    pub captures: Vec<CaptureLive>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Camera {
    pub id: String,
    pub name: String,
    pub node_id: String,
    pub provider: String,
    pub health: String,
    pub availability: String,
    pub backend: Option<String>,
    pub labels: BTreeMap<String, String>,
    /// The Styx camera service socket (`styx-frames+unix://...`), when it serves frames.
    pub frames_endpoint: Option<String>,
    /// Pipelines whose bindings use this camera.
    pub used_by: Vec<String>,
    pub mount: Option<CameraMount>,
    /// Live facts from the camera service; `null` with `live_error` when it did not answer.
    pub live: Option<CameraLive>,
    pub live_error: Option<String>,
    /// Whether the API's control client is connected to the camera service now (Styx's
    /// connection events); `null` when the API keeps no client for the camera.
    pub service_online: Option<bool>,
    /// Whether settings can be changed through the API: the camera has a Styx camera service
    /// (which controls are writable is in its settings).
    pub settings_writable: bool,
    /// Whether `GET /v1/cameras/{id}/preview` can serve it: the camera has a Styx camera service.
    pub preview_available: bool,
}

pub fn frames_socket(endpoints: &[String]) -> Option<PathBuf> {
    endpoints.iter().find_map(|endpoint| endpoint.strip_prefix(STYX_ENDPOINT_PREFIX)).map(PathBuf::from)
}

pub fn camera_records(view: &StateView) -> impl Iterator<Item = &orion::control_plane::ResourceRecord> {
    view.resources.values().filter(|r| r.resource_type.as_str() == CAMERA_RESOURCE_TYPE)
}

fn base_camera(record: &orion::control_plane::ResourceRecord, view: &StateView, node_id: &str, mounts: &BTreeMap<String, CameraMount>) -> Camera {
    let labels = label_map(&record.labels);
    let id = record.resource_id.to_string();
    let frames_endpoint = record.endpoints.iter().find(|e| e.starts_with(STYX_ENDPOINT_PREFIX)).cloned();
    Camera {
        name: labels.get("helios.display_name").cloned().unwrap_or_else(|| id.clone()),
        node_id: node_id.to_string(),
        provider: record.provider_id.to_string(),
        health: enum_name(record.health),
        availability: enum_name(record.availability),
        backend: labels.get("helios.label.styx.backend").cloned(),
        labels: labels.iter().filter_map(|(k, v)| k.strip_prefix("helios.label.").map(|k| (k.to_string(), v.clone()))).collect(),
        preview_available: frames_endpoint.is_some(),
        settings_writable: frames_endpoint.is_some(),
        used_by: pipelines::pipelines_using(view, &id),
        mount: mounts.get(&id).copied(),
        frames_endpoint,
        live: None,
        live_error: None,
        service_online: None,
        id,
    }
}

/// Ask a camera service for its sources and metrics (blocking IPC, so off the async runtime).
pub async fn live_facts(socket: PathBuf) -> Result<CameraLive, String> {
    let task = tokio::task::spawn_blocking(move || -> Result<CameraLive, String> {
        let sources = styx::ipc::FrameClient::options(&socket).timeout(LIVE_TIMEOUT).cameras().map_err(|error| error.to_string())?;
        let metrics = styx::ipc::FrameClient::service_metrics(&socket).map_err(|error| error.to_string())?;
        Ok(CameraLive {
            sources: sources.into_iter().map(|s| CameraSource { name: s.name, keys: s.keys, in_use: s.in_use }).collect(),
            clients: metrics.clients,
            frames_sent: metrics.sent,
            frames_skipped: metrics.skipped,
            restarts: metrics.restarts,
            captures: metrics
                .snapshot
                .cameras
                .into_iter()
                .map(|c| CaptureLive {
                    name: c.name,
                    backend: c.backend,
                    mode: c.mode,
                    fps_configured: c.fps.configured,
                    fps_measured: c.fps.measured,
                    frames_delivered: c.frames.delivered,
                    drops: c.drops.total,
                    latency_p50_ms: c.latency.sensor_to_delivery.p50_ms,
                    latency_p95_ms: c.latency.sensor_to_delivery.p95_ms,
                    cpu_per_frame_us: c.cpu.per_frame_us,
                    exposure_us: c.aaa.as_ref().and_then(|a| a.exposure_us),
                    analogue_gain: c.aaa.as_ref().and_then(|a| a.analogue_gain),
                    digital_gain: c.aaa.as_ref().and_then(|a| a.digital_gain),
                    ae_state: c.aaa.and_then(|a| a.ae_state),
                })
                .collect(),
        })
    });
    match tokio::time::timeout(LIVE_TIMEOUT * 3, task).await {
        Ok(Ok(result)) => result,
        Ok(Err(error)) => Err(error.to_string()),
        Err(_) => Err("camera service did not answer in time".into()),
    }
}

async fn with_live(state: &SharedState, mut camera: Camera, record: &orion::control_plane::ResourceRecord) -> Camera {
    camera.service_online = state.cameras.online(&camera.id).await;
    match frames_socket(&record.endpoints) {
        Some(socket) => match live_facts(socket).await {
            Ok(live) => camera.live = Some(live),
            Err(error) => camera.live_error = Some(error),
        },
        None => camera.live_error = Some("the camera has no Styx frames endpoint".into()),
    }
    camera
}

pub async fn list(State(state): State<SharedState>) -> ApiResult<Json<Vec<Camera>>> {
    let view = state.orion.view().await?;
    let mounts = state.store.mounts().await?;
    let futures = camera_records(&view).map(|record| with_live(&state, base_camera(record, &view, &state.config.node_id, &mounts), record));
    Ok(Json(futures_util::future::join_all(futures).await))
}

async fn find(state: &SharedState, id: &str) -> ApiResult<(StateView, orion::control_plane::ResourceRecord)> {
    check_id(id)?;
    let view = state.orion.view().await?;
    let record = camera_records(&view).find(|r| r.resource_id.as_str() == id).cloned().ok_or_else(|| ApiError::not_found(format!("no camera {id}")))?;
    Ok((view, record))
}

pub async fn get_one(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<Json<Camera>> {
    let (view, record) = find(&state, &id).await?;
    let mounts = state.store.mounts().await?;
    Ok(Json(with_live(&state, base_camera(&record, &view, &state.config.node_id, &mounts), &record).await))
}

/// A camera's settings: its controls as the camera service lists them (range, default, value
/// now, the standard control each answers, whether the API may change it) and the capture's
/// mode and measured 3A from its live metrics.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CameraSettings {
    /// At least one control can be changed.
    pub writable: bool,
    pub controls: Vec<CameraControl>,
    pub mode: Option<String>,
    pub fps: Option<f64>,
    pub exposure_us: Option<f64>,
    pub analogue_gain: Option<f64>,
    pub digital_gain: Option<f64>,
    pub ae_state: Option<String>,
    /// Why `mode`..`ae_state` are missing (the camera service's metrics did not answer).
    pub live_error: Option<String>,
    /// The values set through the API that are kept across reboots and applied again whenever
    /// the camera service starts (standard keys, or the camera's control names).
    pub persisted: BTreeMap<String, serde_json::Value>,
}

/// What `PATCH` (or `DELETE`) `/v1/cameras/{id}/settings` did, in the order applied, and the
/// values kept for the camera now.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SettingsApplied {
    pub applied: Vec<AppliedCameraControl>,
    pub persisted: BTreeMap<String, serde_json::Value>,
}

/// The API's client of the camera's service, and its socket.
async fn control_client(state: &SharedState, id: &str) -> ApiResult<(std::sync::Arc<CameraClient>, PathBuf)> {
    let (_, record) = find(state, id).await?;
    let socket = frames_socket(&record.endpoints).ok_or_else(|| ApiError::backend(format!("{id} has no Styx frames endpoint")))?;
    Ok((state.cameras.client(id, &socket).await?, socket))
}

/// Keep a camera control client for every camera with a Styx camera service, so the values
/// stored for a camera are applied as soon as its service appears (at boot, after it
/// restarted), not only once someone opens its settings. Checks Orion's cameras every few
/// seconds; a client is made without waiting for its service.
pub fn spawn_camera_settings_keeper(state: SharedState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(CAMERA_SCAN_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            let Ok(view) = state.orion.view().await else {
                continue;
            };
            state.previews.retain(|id| camera_records(&view).any(|record| record.resource_id.as_str() == id && frames_socket(&record.endpoints).is_some())).await;
            for record in camera_records(&view) {
                if let Some(socket) = frames_socket(&record.endpoints)
                    && let Err(error) = state.cameras.client(record.resource_id.as_str(), &socket).await
                {
                    tracing::warn!(camera = %record.resource_id, error = %error.message, "camera control client");
                }
            }
        }
    })
}

pub async fn get_settings(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<Json<CameraSettings>> {
    let (client, socket) = control_client(&state, &id).await?;
    let (controls, live) = tokio::join!(client.controls(), live_facts(socket));
    let controls = controls?;
    let (capture, live_error) = match &live {
        Ok(live) => (live.captures.first(), None),
        Err(error) => (None, Some(error.clone())),
    };
    Ok(Json(CameraSettings {
        writable: controls.iter().any(|control| control.writable),
        mode: capture.map(|c| c.mode.clone()),
        fps: capture.and_then(|c| c.fps_configured),
        exposure_us: capture.and_then(|c| c.exposure_us),
        analogue_gain: capture.and_then(|c| c.analogue_gain),
        digital_gain: capture.and_then(|c| c.digital_gain),
        ae_state: capture.and_then(|c| c.ae_state.clone()),
        live_error,
        controls,
        persisted: client.persisted().await?,
    }))
}

/// Change camera controls: `{"ae": false, "exposure_us": 8000, "gain": 4}` (standard keys, or
/// a control's listed `name` or `id`). Modes go first; a refusal stops there.
pub async fn set_settings(State(state): State<SharedState>, Path(id): Path<String>, Json(request): Json<serde_json::Value>) -> ApiResult<Json<SettingsApplied>> {
    let serde_json::Value::Object(request) = request else {
        return Err(ApiError::bad_request("the body is an object of controls: {\"exposure_us\": 8000, ...}"));
    };
    let (client, _) = control_client(&state, &id).await?;
    let changes = client.resolve(&request).await?;
    let applied = client.apply(changes).await?;
    Ok(Json(SettingsApplied { applied, persisted: client.persisted().await? }))
}

/// Reset to defaults: every writable control back to its default, and the stored values
/// forgotten (the camera starts with its defaults after a reboot).
pub async fn reset_settings(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<Json<SettingsApplied>> {
    let (client, _) = control_client(&state, &id).await?;
    let applied = client.reset().await?;
    Ok(Json(SettingsApplied { applied, persisted: client.persisted().await? }))
}

pub async fn get_mount(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<Json<Option<CameraMount>>> {
    check_id(&id)?;
    Ok(Json(state.store.mounts().await?.get(&id).copied()))
}

pub async fn put_mount(State(state): State<SharedState>, Path(id): Path<String>, Json(mount): Json<CameraMount>) -> ApiResult<Json<CameraMount>> {
    let _ = find(&state, &id).await?;
    mount.validate()?;
    state.store.set_mount(&id, Some(mount)).await?;
    state.events.publish("camera", serde_json::json!({ "id": id, "change": "mount", "mount": mount }));
    refresh_pipelines(&state, &id).await;
    Ok(Json(mount))
}

pub async fn delete_mount(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    check_id(&id)?;
    state.store.set_mount(&id, None).await?;
    state.events.publish("camera", serde_json::json!({ "id": id, "change": "mount", "mount": null }));
    refresh_pipelines(&state, &id).await;
    Ok(StatusCode::NO_CONTENT)
}

/// The camera's preview (`camera_preview`), made on its first viewer.
async fn camera_preview(state: &SharedState, id: &str) -> ApiResult<std::sync::Arc<styx::preview::Preview>> {
    let (_, record) = find(state, id).await?;
    let socket = frames_socket(&record.endpoints).ok_or_else(|| ApiError::backend(format!("{id} has no Styx frames endpoint")))?;
    state.previews.get(id, &socket).await
}

/// MJPEG (`multipart/x-mixed-replace`, for an `<img>`): the newest preview JPEG each time the
/// client is ready for one; a slow client skips frames. The body stays open while the client
/// reads it, also while the camera service is down (frames resume when it is back).
pub async fn preview(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<Response> {
    let preview = camera_preview(&state, &id).await?;
    let body = Body::from_stream(preview.mjpeg().fallible());
    Ok(([(header::CONTENT_TYPE, MJPEG_CONTENT_TYPE), (header::CACHE_CONTROL, "no-cache, no-store"), (header::HeaderName::from_static("x-accel-buffering"), "no")], body).into_response())
}

/// WebSocket: one binary message per preview frame, Styx's `SPV1` layout (a 32-byte header
/// with the sequence, capture timestamp and size, then the JPEG; Styx `docs/preview.md`).
pub async fn preview_ws(State(state): State<SharedState>, Path(id): Path<String>, upgrade: WebSocketUpgrade) -> ApiResult<Response> {
    let preview = camera_preview(&state, &id).await?;
    Ok(upgrade.on_upgrade(move |socket| send_preview(socket, preview)))
}

async fn send_preview(mut socket: WebSocket, preview: std::sync::Arc<styx::preview::Preview>) {
    let mut viewer = preview.subscribe();
    loop {
        tokio::select! {
            frame = viewer.next() => {
                let Some(frame) = frame else { break };
                if socket.send(Message::Binary(ws_message(&frame))).await.is_err() {
                    break;
                }
            }
            // The client closing (or anything it sends) is noticed between frames too.
            message = socket.next() => match message {
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                Some(Ok(_)) => {}
            },
        }
    }
}

/// Hand a camera's new calibration or mount to the pipelines using it (no new revision). A
/// pipeline that could not be updated now gets it with its next deploy.
async fn refresh_pipelines(state: &SharedState, camera: &str) {
    match super::pipelines::refresh_camera_context(state, camera).await {
        Ok(updated) if !updated.is_empty() => tracing::info!(camera, pipelines = ?updated, "camera context updated"),
        Ok(_) => {}
        Err(error) => tracing::warn!(camera, error = %error.message, "camera context not handed to its pipelines"),
    }
}

/// A camera's calibrations, and which one each pipeline using the camera gets.
#[derive(Debug, Serialize)]
pub struct CameraCalibrations {
    pub calibrations: Vec<CameraCalibration>,
    /// `[{pipeline, input, calibration: {status, width, height, model, scaled_from?} | {status: "uncalibrated", reason}}]`
    pub pipelines: Vec<serde_json::Value>,
}

async fn calibrations_of(state: &SharedState, id: &str) -> ApiResult<CameraCalibrations> {
    let calibrations = state.store.calibrations().await?.remove(id).unwrap_or_default();
    let pipelines = match state.orion.view().await {
        Ok(view) => view
            .workloads
            .values()
            .filter(|w| w.runtime_type.as_str() == pipelines::ENGINE_RUNTIME)
            .flat_map(|w| {
                pipelines::decode_bindings(w).into_iter().filter(|(_, b)| b.resource_id == id).map(|(input, binding)| {
                    let matched = camera_context::match_calibration(&calibrations, binding.output_width.zip(binding.output_height));
                    serde_json::json!({ "pipeline": pipelines::pipeline_id(w.workload_id.as_str()), "input": input, "calibration": pipelines::calibration_note(&matched) })
                })
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    Ok(CameraCalibrations { calibrations, pipelines })
}

/// `GET /v1/cameras/{id}/calibration`.
pub async fn get_calibration(State(state): State<SharedState>, Path(id): Path<String>) -> ApiResult<Json<CameraCalibrations>> {
    check_id(&id)?;
    Ok(Json(calibrations_of(&state, &id).await?))
}

/// `PUT /v1/cameras/{id}/calibration`: keep a calibration (replacing the one of the same image
/// size) on the data partition and hand it to the pipelines using the camera.
pub async fn put_calibration(State(state): State<SharedState>, Path(id): Path<String>, Json(mut calibration): Json<CameraCalibration>) -> ApiResult<Json<CameraCalibrations>> {
    let _ = find(&state, &id).await?;
    calibration.validate()?;
    calibration.saved_at_ms = crate::host::now_ms();
    state.store.put_calibration(&id, calibration.clone()).await?;
    state.events.publish("camera", serde_json::json!({ "id": id, "change": "calibration", "calibration": calibration }));
    refresh_pipelines(&state, &id).await;
    Ok(Json(calibrations_of(&state, &id).await?))
}

#[derive(Debug, serde::Deserialize)]
pub struct CalibrationSize {
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// `DELETE /v1/cameras/{id}/calibration[?width=&height=]`: forget one calibration, or all.
pub async fn delete_calibration(State(state): State<SharedState>, Path(id): Path<String>, axum::extract::Query(size): axum::extract::Query<CalibrationSize>) -> ApiResult<StatusCode> {
    check_id(&id)?;
    let size = match (size.width, size.height) {
        (Some(width), Some(height)) => Some((width, height)),
        (None, None) => None,
        _ => return Err(ApiError::bad_request("give both width and height, or neither to forget every calibration")),
    };
    state.store.delete_calibration(&id, size).await?;
    state.events.publish("camera", serde_json::json!({ "id": id, "change": "calibration", "calibration": null, "size": size }));
    refresh_pipelines(&state, &id).await;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /v1/cameras/{id}/calibration/capture`: not on the device yet.
pub async fn capture_calibration(Path(id): Path<String>) -> ApiResult<ApiError> {
    check_id(&id)?;
    Ok(ApiError::not_available(
        "calibration capture is not available on the device yet; upload a calibration with PUT /v1/cameras/{id}/calibration",
        "a capture and solve flow (board detection on the camera's frames, an Eidos calibration solve) behind this endpoint",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_socket_from_endpoints() {
        let endpoints = vec!["dev:///dev/video0".to_string(), "styx-frames+unix:///run/helios/streams/cam.styx.sock".to_string()];
        assert_eq!(frames_socket(&endpoints), Some(PathBuf::from("/run/helios/streams/cam.styx.sock")));
        assert_eq!(frames_socket(&endpoints[..1]), None);
    }
}
