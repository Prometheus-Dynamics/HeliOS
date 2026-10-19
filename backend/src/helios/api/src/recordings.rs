//! Session recordings in helios-api: the camera's frames, the host's channels and the start-time
//! facts (`helios-recorder` does the recording). This module builds a recording's request from
//! the API's state, feeds the host channels from the event stream while one runs, publishes its
//! status as `recording` events, and maps the recorder's errors.

use std::{path::Path, time::Duration};

use helios_recorder::{
    Attachment, LemnosSource, RecorderError, RecorderSettings, RecordingInfo, RecordingRequest, RecordingStatus, frames,
    host::{self, HostSender, TOPIC_EVENTS},
    info::{ATTACHMENT_FIELD, ATTACHMENT_PIPELINE_PREFIX},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{sync::broadcast, task::JoinHandle};

use crate::{
    API_VERSION, SharedState, VERSION,
    error::{ApiError, ApiResult},
    events::ApiEvent,
    field_layouts, host as api_host,
    orion::StateView,
    routes::{
        cameras::{self, frames_socket},
        pipelines,
        system::device_package_facts,
    },
};

/// `POST /v1/recordings`: the camera, and the settings to change for this recording only.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StartRecording {
    /// The camera (its resource id, as `GET /v1/cameras` lists it).
    pub camera: String,
    /// Frames per second at most for this recording (default: the stored setting, else the
    /// camera's rate).
    #[serde(default)]
    pub max_fps: Option<f64>,
    /// lz4 chunks on or off for this recording (default: the stored setting).
    #[serde(default)]
    pub compression: Option<bool>,
}

/// The stored settings with this start's overrides applied.
pub fn effective_settings(stored: &RecorderSettings, request: &StartRecording) -> RecorderSettings {
    let mut settings = stored.clone();
    if let Some(fps) = request.max_fps {
        settings.max_fps = Some(fps);
    }
    if let Some(compression) = request.compression {
        settings.compression = compression;
    }
    settings
}

/// The recording's start-time facts: the session, the camera (calibration, mount, mode, rate,
/// controls), the versions, the selected field layout and the pipelines using the camera.
/// Facts that cannot be read now are recorded as `null` (or an error note), never skipped.
pub async fn recording_info(state: &SharedState, camera: &str, socket: &Path, view: &StateView) -> RecordingInfo {
    let calibrations = state.store.calibrations().await.ok().and_then(|mut all| all.remove(camera));
    let mount = state.store.mounts().await.ok().and_then(|mut all| all.remove(camera));
    let (mode, fps) = match cameras::live_facts(socket.to_path_buf()).await {
        Ok(live) => live.captures.first().map(|c| (json!(c.mode), json!(c.fps_configured))).unwrap_or((Value::Null, Value::Null)),
        Err(error) => (json!({ "error": error }), Value::Null),
    };
    let controls = match state.cameras.client(camera, socket).await {
        Ok(client) => match client.controls().await {
            Ok(controls) => json!(controls),
            Err(error) => json!({ "error": error.message }),
        },
        Err(error) => json!({ "error": error.message }),
    };
    let persisted = state.store.camera_settings(camera).await.unwrap_or_default();

    let session = map([("board", json!(state.config.node_id)), ("camera", json!(camera)), ("api_version", json!(API_VERSION))]);
    let camera_facts = map([("calibration", json!(calibrations)), ("mount", json!(mount)), ("mode", mode), ("fps", fps), ("controls", controls), ("stored_settings", json!(persisted))]);
    let package = device_package_facts();
    let os = api_host::os_release();
    let versions = map([
        ("helios_api", json!(VERSION)),
        ("os", json!(os.get("PRETTY_NAME"))),
        ("kernel", json!(std::fs::read_to_string("/proc/sys/kernel/osrelease").ok().map(|s| s.trim().to_string()))),
        ("device_package", json!(package.get("BOARD_PACKAGE_VERSION"))),
        ("device_package_commit", json!(package.get("BOARD_PACKAGE_COMMIT"))),
    ]);

    let field = match field_layouts::selected_id(state).await {
        Ok(id) => field_layouts::layout(state, &id).await.ok().and_then(|layout| attachment(ATTACHMENT_FIELD, &layout)),
        Err(_) => None,
    };
    RecordingInfo { session: to_strings(session), camera: to_strings(camera_facts), versions: to_strings(versions), field, pipelines: pipeline_attachments(view, camera) }
}

/// The pipelines whose bindings use `camera`, each with its graph document as an attachment.
fn pipeline_attachments(view: &StateView, camera: &str) -> Vec<Attachment> {
    pipelines::pipelines_using(view, camera)
        .into_iter()
        .filter_map(|id| {
            let workload = view.workloads.values().find(|w| w.runtime_type.as_str() == pipelines::ENGINE_RUNTIME && pipelines::pipeline_id(w.workload_id.as_str()) == id)?;
            let graph = pipelines::pipeline_dto(view, workload).graph?;
            attachment(&format!("{ATTACHMENT_PIPELINE_PREFIX}{id}"), &graph)
        })
        .collect()
}

fn attachment(name: &str, value: &impl serde::Serialize) -> Option<Attachment> {
    let data = serde_json::to_vec_pretty(value).ok()?;
    Some(Attachment { name: name.to_string(), media_type: "application/json".into(), data })
}

fn map<const N: usize>(entries: [(&str, Value); N]) -> serde_json::Map<String, Value> {
    entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// Facts as the metadata records hold them: each value as its JSON text.
fn to_strings(facts: serde_json::Map<String, Value>) -> std::collections::BTreeMap<String, String> {
    facts.into_iter().map(|(k, v)| (k, v.to_string())).collect()
}

/// Everything `recorder.start` needs for `camera`: its frames socket, the board and the commit.
pub async fn request_for(state: &SharedState, camera: &str, settings: RecorderSettings) -> ApiResult<RecordingRequest> {
    let (view, record) = cameras::find(state, camera).await?;
    let socket = frames_socket(&record.endpoints).ok_or_else(|| ApiError::backend(format!("{camera} has no Styx frames endpoint")))?;
    let info = recording_info(state, camera, &socket, &view).await;
    Ok(RecordingRequest {
        camera_id: camera.to_string(),
        frames_socket: socket,
        frames: frames::luma_request(),
        board: state.config.node_id.clone(),
        commit: device_package_facts().get("BOARD_PACKAGE_COMMIT").cloned().unwrap_or_default(),
        settings: settings.clone(),
        info,
        lemnos: Some(LemnosSource {
            socket: state.config.lemnos_socket.clone(),
            client_name: "helios-recorder".into(),
            imu_period_ms: settings.imu_period_ms,
            mag_period_ms: settings.mag_period_ms,
            power_period_ms: settings.power_period_ms,
        }),
    })
}

/// How often a running recording's status is published as a `recording` event.
const STATUS_INTERVAL: Duration = Duration::from_secs(1);

/// While recording `id`: forwards pipeline outputs and the API's events to the host channels,
/// and publishes its status once a second. Ends with the recording.
pub fn spawn_session_tasks(state: SharedState, host: HostSender, id: String) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut events = state.events.subscribe();
        let mut tick = tokio::time::interval(STATUS_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                event = events.recv() => match event {
                    Ok(event) => forward(&host, &event),
                    Err(broadcast::error::RecvError::Lagged(missed)) => tracing::warn!(missed, "recording: events missed"),
                    Err(broadcast::error::RecvError::Closed) => break,
                },
                _ = tick.tick() => match state.recorder.status() {
                    Some(status) if status.id == id => {
                        publish(&state, &status);
                        if !status.state.is_active() {
                            break;
                        }
                    }
                    _ => break,
                },
            }
        }
    })
}

/// One API event as a host message: an output to its port's channel, anything else (but the
/// status and metrics, which are high-rate and not recorded) to `/helios/events`.
fn forward(host: &HostSender, event: &ApiEvent) {
    match event.kind.as_str() {
        "output" => {
            let Some(output) = event.data.get("value") else { return };
            let port = output["port"].as_str().unwrap_or_default();
            let pipeline = output["pipeline"].as_str().unwrap_or_default();
            let observed = output["observed_at_ms"].as_u64().unwrap_or_default();
            host.send(&host::output_topic(port), host::output_json(pipeline, port, &output["value"], observed));
        }
        "recording" | "metrics" | "hello" | "lagged" => {}
        kind => host.send(TOPIC_EVENTS, host::event_json(kind, event.at_ms, &event.data)),
    }
}

/// Publishes a recording's status as a `recording` event.
pub fn publish(state: &SharedState, status: &RecordingStatus) {
    if let Ok(data) = serde_json::to_value(status) {
        state.events.publish("recording", data);
    }
}

/// The API's error for a recorder error.
pub fn api_error(error: RecorderError) -> ApiError {
    use helios_recorder::storage::StorageError;
    match error {
        RecorderError::Busy(id) => ApiError::conflict(format!("recording {id} is still running; stop it first")),
        RecorderError::Storage(StorageError::ReserveNotMet { free, reserve }) => {
            ApiError::insufficient_storage(format!("{free} bytes free, the reserve is {reserve}; free space or lower the reserve"))
        }
        RecorderError::Storage(StorageError::NotWritable(dir)) => ApiError::unprocessable(format!("recordings directory {} is not writable", dir.display())),
        RecorderError::Storage(error) => ApiError::internal(error.to_string()),
        RecorderError::Camera(message) => ApiError::backend(format!("camera service: {message}")),
        RecorderError::Settings(message) => ApiError::unprocessable(message),
        RecorderError::NotFound(message) => ApiError::not_found(message),
        RecorderError::Mcap(message) | RecorderError::Io(message) => ApiError::internal(message),
    }
}

/// A recording id from the path must be a stored or active one: letters, digits, `.`, `_`, `-`.
pub fn check_recording_id(id: &str) -> ApiResult<()> {
    if helios_recorder::naming::is_valid_id(id) { Ok(()) } else { Err(ApiError::bad_request(format!("invalid recording id {id:?}"))) }
}
