//! Camera controls through each camera's Styx camera service: list, read and set them
//! (`FrameClient::controls`, `set_control`), and forward every control change on the camera, by
//! any client, as an SSE `camera` event (`FrameClient::control_events`).
//!
//! Styx serves controls to a camera's frame clients only, so the API keeps one client per camera
//! service, opened on first use and kept: it asks for the luma frames the engine asks for by
//! default (so joining does not make the camera service plan another capture) and drops each
//! frame as it arrives, so it never holds camera buffers. Control requests are blocking IPC and
//! run on Tokio's blocking pool.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use serde::Serialize;
use styx::{
    ipc::{AppliedControl, ControlDescriptor, ControlEvent, ControlEvents, ControlRefusal, ControlTarget, FrameClient, IpcError, SERVICE_FRAME_RATE, StandardControl},
    prelude::{Access, ControlId, ControlKind, ControlValue, Frames, RecvOutcome},
};
use tokio::{io::unix::AsyncFd, sync::Mutex};

use crate::{
    error::{ApiError, ApiResult},
    events::EventHub,
};

/// How long opening the camera service, or one control request, may take.
const CONTROL_TIMEOUT: Duration = Duration::from_secs(3);
/// Between attempts to follow a camera service's control changes again after it went away.
const RESUBSCRIBE_EVERY: Duration = Duration::from_secs(2);

/// The API's names for Styx's standard controls (in Styx's units), and how values are sent.
const STANDARD: [(&str, StandardControl); 12] = [
    ("exposure_us", StandardControl::ExposureUs),
    ("gain", StandardControl::Gain),
    ("ae", StandardControl::AeEnable),
    ("ev", StandardControl::ExposureValue),
    ("fps", StandardControl::FrameRate),
    ("awb", StandardControl::AwbEnable),
    ("colour_temperature", StandardControl::ColourTemperature),
    ("red_gain", StandardControl::RedGain),
    ("blue_gain", StandardControl::BlueGain),
    ("af_mode", StandardControl::AfMode),
    ("af_trigger", StandardControl::AfTrigger),
    ("lens_position", StandardControl::LensPosition),
];

/// Modes are applied before the values they gate (`{"ae": false, "exposure_us": 8000}`).
const MODES_FIRST: [StandardControl; 3] = [StandardControl::AeEnable, StandardControl::AwbEnable, StandardControl::AfMode];

pub fn standard_key(control: StandardControl) -> &'static str {
    STANDARD.iter().find(|(_, standard)| *standard == control).map_or("", |(key, _)| key)
}

/// A camera control as the API lists it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CameraControl {
    /// The backend's control id (Styx's `SERVICE_FRAME_RATE` for a frame rate the service sets
    /// by restarting the capture).
    pub id: u32,
    pub name: String,
    /// `bool`, `int`, `uint`, `float`, `menu`, `int_menu`, `rectangle`, `none` or `unknown`.
    pub kind: &'static str,
    pub read_only: bool,
    pub min: serde_json::Value,
    pub max: serde_json::Value,
    pub default: serde_json::Value,
    pub step: Option<serde_json::Value>,
    pub menu: Option<Vec<String>>,
    /// The value now; `null` when it cannot be read.
    pub current: Option<serde_json::Value>,
    /// The standard control it answers (`exposure_us`, `gain`, `ae`, ...), in Styx's units.
    pub standard: Option<&'static str>,
    /// The camera service lets the API change it.
    pub writable: bool,
}

impl From<&ControlDescriptor> for CameraControl {
    fn from(descriptor: &ControlDescriptor) -> Self {
        let meta = &descriptor.meta;
        Self {
            id: meta.id.0,
            name: meta.name.clone(),
            kind: kind_name(meta.kind),
            read_only: meta.access == Access::ReadOnly,
            min: value_json(&meta.min),
            max: value_json(&meta.max),
            default: value_json(&meta.default),
            step: meta.step.as_ref().map(value_json),
            menu: meta.menu.clone(),
            current: descriptor.current.as_ref().map(value_json),
            standard: descriptor.standard.map(standard_key),
            writable: descriptor.writable,
        }
    }
}

/// What a control change did.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AppliedCameraControl {
    /// The key it was asked for under.
    pub control: String,
    pub id: u32,
    pub requested: serde_json::Value,
    /// The value in effect now.
    pub value: serde_json::Value,
    /// The value asked for was out of range (or between steps) and `value` is the clamped one.
    pub clamped: bool,
    /// The camera is not streaming: the value applies when it starts.
    pub deferred: bool,
    /// The capture was restarted for every client to apply it (a frame rate).
    pub restarted: bool,
    /// Frame-exact cameras: the sensor sequence of the first frame using it.
    pub frame: Option<u64>,
}

impl AppliedCameraControl {
    fn new(control: String, applied: &AppliedControl) -> Self {
        Self {
            control,
            id: applied.id.0,
            requested: value_json(&applied.requested),
            value: value_json(&applied.value),
            clamped: applied.clamped,
            deferred: applied.deferred,
            restarted: applied.restarted,
            frame: applied.frame,
        }
    }
}

/// One control change to make: what it was asked for as, the target and the value.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlChange {
    pub key: String,
    pub target: ControlTarget,
    pub value: ControlValue,
}

/// The camera clients the API keeps, one per camera service socket.
#[derive(Default)]
pub struct CameraControls {
    clients: Mutex<HashMap<PathBuf, Arc<ControlClient>>>,
}

/// The API's client of one camera service.
pub struct ControlClient {
    client: Arc<FrameClient>,
}

impl CameraControls {
    /// The client of the camera service at `socket` (opened, and its control changes followed as
    /// `camera` events for `camera_id`, on first use).
    pub async fn client(&self, camera_id: &str, socket: &Path, events: Arc<EventHub>) -> ApiResult<Arc<ControlClient>> {
        let mut clients = self.clients.lock().await;
        if let Some(client) = clients.get(socket) {
            return Ok(client.clone());
        }
        let path = socket.to_path_buf();
        let (client, subscription) = blocking(move || {
            let client = FrameClient::options(&path).timeout(CONTROL_TIMEOUT).reconnecting().request(&Frames::gray().latest())?;
            let subscription = client.control_events()?;
            Ok((Arc::new(client), subscription))
        })
        .await?;
        tokio::spawn(follow(camera_id.to_string(), client.clone(), subscription, events));
        let client = Arc::new(ControlClient { client });
        clients.insert(socket.to_path_buf(), client.clone());
        Ok(client)
    }
}

impl ControlClient {
    /// The camera's controls, with their values now.
    pub async fn controls(&self) -> ApiResult<Vec<CameraControl>> {
        let client = self.client.clone();
        let descriptors = blocking(move || client.controls()).await?;
        Ok(descriptors.iter().map(CameraControl::from).collect())
    }

    /// Resolve `{"key": value, ...}` into control changes, modes first: standard keys
    /// (`exposure_us`, `ae`, ...), else a control's listed `name` or numeric `id`. Nothing is
    /// applied when any key or value is unusable.
    pub async fn resolve(&self, request: &serde_json::Map<String, serde_json::Value>) -> ApiResult<Vec<ControlChange>> {
        if request.is_empty() {
            return Err(ApiError::bad_request("name at least one control: {\"exposure_us\": 8000, ...}"));
        }
        let mut listed: Option<Vec<CameraControl>> = None;
        let mut changes = Vec::with_capacity(request.len());
        for (key, value) in request {
            let change = match STANDARD.iter().find(|(name, _)| name == key) {
                Some((_, standard)) => ControlChange { key: key.clone(), target: ControlTarget::Standard(*standard), value: standard_value(*standard, value)? },
                None => {
                    if listed.is_none() {
                        listed = Some(self.controls().await?);
                    }
                    let controls = listed.as_deref().unwrap_or_default();
                    let control = controls
                        .iter()
                        .find(|control| control.name == *key || control.id.to_string() == *key || format!("{:#x}", control.id) == key.to_ascii_lowercase())
                        .ok_or_else(|| ApiError::bad_request(format!("the camera has no control {key:?} (GET the settings for its controls)")))?;
                    ControlChange { key: key.clone(), target: ControlTarget::Id(ControlId(control.id)), value: backend_value(key, value)? }
                }
            };
            changes.push(change);
        }
        changes.sort_by_key(|change| !matches!(change.target, ControlTarget::Standard(standard) if MODES_FIRST.contains(&standard)));
        Ok(changes)
    }

    /// Apply `changes` in order. A refusal stops there (the changes before it stay applied, and
    /// were announced as events).
    pub async fn apply(&self, changes: Vec<ControlChange>) -> ApiResult<Vec<AppliedCameraControl>> {
        let client = self.client.clone();
        blocking(move || {
            let mut applied = Vec::with_capacity(changes.len());
            for change in changes {
                match client.set_control(change.target, change.value) {
                    Ok(result) => applied.push(AppliedCameraControl::new(change.key, &result)),
                    Err(error) => return Ok(Err(control_error(&change.key, &applied, error))),
                }
            }
            Ok(Ok(applied))
        })
        .await?
    }
}

/// Run blocking camera service IPC off the async runtime.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Result<T, IpcError> + Send + 'static) -> ApiResult<T> {
    match tokio::task::spawn_blocking(work).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(control_error("", &[], error)),
        Err(error) => Err(ApiError::internal(error.to_string())),
    }
}

fn control_error(key: &str, applied: &[AppliedCameraControl], error: IpcError) -> ApiError {
    let subject = if key.is_empty() { String::new() } else { format!("{key}: ") };
    let before = if applied.is_empty() { String::new() } else { format!(" (applied before it: {})", applied.iter().map(|change| change.control.as_str()).collect::<Vec<_>>().join(", ")) };
    let message = format!("{subject}{error}{before}");
    match error {
        IpcError::ControlRefused(ControlRefusal::Unsupported(_) | ControlRefusal::ReadOnly(_) | ControlRefusal::Invalid(_)) => ApiError::unprocessable(message),
        IpcError::ControlRefused(ControlRefusal::NotPermitted(_)) => ApiError::forbidden(message),
        _ => ApiError::backend(format!("camera service: {message}")),
    }
}

/// Follow the camera's control changes and publish them as `camera` events; drop the frames the
/// client gets as they arrive. Runs as long as the API.
async fn follow(camera_id: String, client: Arc<FrameClient>, subscription: ControlEvents, hub: Arc<EventHub>) {
    let mut events = AsyncFd::new(subscription).map_err(|error| tracing::warn!(camera = %camera_id, %error, "cannot follow camera control changes")).ok();
    let mut resubscribe = tokio::time::interval(RESUBSCRIBE_EVERY);
    loop {
        tokio::select! {
            // Dropped at once: the API never holds the camera's buffers.
            frame = client.next() => {
                if matches!(frame, RecvOutcome::Closed) {
                    tokio::time::sleep(RESUBSCRIBE_EVERY).await;
                }
            }
            ready = async { events.as_ref().expect("guarded").readable().await }, if events.is_some() => {
                let Ok(mut guard) = ready else {
                    events = None;
                    continue;
                };
                loop {
                    match guard.get_inner().try_recv() {
                        RecvOutcome::Data(event) => hub.publish("camera", control_event_json(&camera_id, &event)),
                        RecvOutcome::Empty => {
                            guard.clear_ready();
                            break;
                        }
                        RecvOutcome::Closed => {
                            drop(guard);
                            events = None;
                            break;
                        }
                    }
                }
            }
            _ = resubscribe.tick(), if events.is_none() => {
                let client = client.clone();
                if let Ok(Ok(subscription)) = tokio::task::spawn_blocking(move || client.control_events()).await {
                    events = AsyncFd::new(subscription).ok();
                }
            }
        }
    }
}

pub fn control_event_json(camera_id: &str, event: &ControlEvent) -> serde_json::Value {
    serde_json::json!({
        "id": camera_id,
        "change": "control",
        "control": {
            "id": event.id.0,
            "standard": event.standard.map(standard_key),
            "frame_rate_restart": event.id == SERVICE_FRAME_RATE,
            "value": value_json(&event.value),
            "frame": event.frame,
            "by": event.by,
        },
    })
}

fn kind_name(kind: ControlKind) -> &'static str {
    match kind {
        ControlKind::None => "none",
        ControlKind::Bool => "bool",
        ControlKind::Rectangle => "rectangle",
        ControlKind::Int => "int",
        ControlKind::Uint => "uint",
        ControlKind::Float => "float",
        ControlKind::Menu => "menu",
        ControlKind::IntMenu => "int_menu",
        ControlKind::Unknown => "unknown",
    }
}

pub fn value_json(value: &ControlValue) -> serde_json::Value {
    let rect = |r: &styx::prelude::ControlRect| serde_json::json!({ "x": r.x, "y": r.y, "width": r.width, "height": r.height });
    match value {
        ControlValue::None => serde_json::Value::Null,
        ControlValue::Bool(value) => serde_json::json!(value),
        ControlValue::Int(value) => serde_json::json!(value),
        ControlValue::Uint(value) => serde_json::json!(value),
        // Through the shortest decimal of the f32, so 0.1 stays 0.1 rather than 0.10000000149.
        ControlValue::Float(value) => value.to_string().parse::<f64>().map_or(serde_json::Value::Null, |value| serde_json::json!(value)),
        ControlValue::Rect(r) => rect(r),
        ControlValue::Rects(rects) => rects.iter().map(rect).collect(),
    }
}

fn number(key: &str, value: &serde_json::Value) -> ApiResult<f64> {
    value.as_f64().filter(|value| value.is_finite()).ok_or_else(|| ApiError::bad_request(format!("{key} takes a number")))
}

/// A standard control's value in its type: exposure and colour temperature as whole numbers,
/// gains, EV, fps and lens position as numbers, AE and AWB as booleans, AF mode
/// (`manual`/`auto`/`continuous` or 0-2) and AF trigger (`start`/`cancel` or 0/1).
pub fn standard_value(control: StandardControl, value: &serde_json::Value) -> ApiResult<ControlValue> {
    let key = standard_key(control);
    let choice = |names: &[&str]| -> ApiResult<ControlValue> {
        if let Some(name) = value.as_str() {
            return names.iter().position(|n| *n == name).map(|index| ControlValue::Int(index as i32)).ok_or_else(|| ApiError::bad_request(format!("{key} is one of {}", names.join(", "))));
        }
        let index = number(key, value)?;
        if index.fract() == 0.0 && (0.0..names.len() as f64).contains(&index) {
            Ok(ControlValue::Int(index as i32))
        } else {
            Err(ApiError::bad_request(format!("{key} is one of {}", names.join(", "))))
        }
    };
    Ok(match control {
        StandardControl::AeEnable | StandardControl::AwbEnable => ControlValue::Bool(value.as_bool().ok_or_else(|| ApiError::bad_request(format!("{key} takes true or false")))?),
        StandardControl::ExposureUs | StandardControl::ColourTemperature => {
            let n = number(key, value)?;
            if n < 0.0 || n > f64::from(u32::MAX) {
                return Err(ApiError::bad_request(format!("{key} takes a whole number from 0")));
            }
            ControlValue::Uint(n.round() as u32)
        }
        StandardControl::AfMode => choice(&["manual", "auto", "continuous"])?,
        StandardControl::AfTrigger => choice(&["start", "cancel"])?,
        StandardControl::Gain | StandardControl::ExposureValue | StandardControl::FrameRate | StandardControl::RedGain | StandardControl::BlueGain | StandardControl::LensPosition => {
            ControlValue::Float(number(key, value)? as f32)
        }
    })
}

/// A backend control's value: booleans as booleans, whole numbers as integers, others as floats
/// (the camera service takes numbers of any type for the control's type and clamps them).
pub fn backend_value(key: &str, value: &serde_json::Value) -> ApiResult<ControlValue> {
    if let Some(flag) = value.as_bool() {
        return Ok(ControlValue::Bool(flag));
    }
    if let Some(int) = value.as_i64() {
        return Ok(match i32::try_from(int) {
            Ok(int) => ControlValue::Int(int),
            Err(_) => ControlValue::Uint(u32::try_from(int).map_err(|_| ApiError::bad_request(format!("{key} is out of range")))?),
        });
    }
    Ok(ControlValue::Float(number(key, value)? as f32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_values_take_their_types() {
        assert_eq!(standard_value(StandardControl::ExposureUs, &serde_json::json!(8000)).unwrap(), ControlValue::Uint(8000));
        assert_eq!(standard_value(StandardControl::Gain, &serde_json::json!(2.5)).unwrap(), ControlValue::Float(2.5));
        assert_eq!(standard_value(StandardControl::AeEnable, &serde_json::json!(false)).unwrap(), ControlValue::Bool(false));
        assert_eq!(standard_value(StandardControl::AfMode, &serde_json::json!("continuous")).unwrap(), ControlValue::Int(2));
        assert_eq!(standard_value(StandardControl::AfTrigger, &serde_json::json!(1)).unwrap(), ControlValue::Int(1));
        assert!(standard_value(StandardControl::ExposureUs, &serde_json::json!(-1)).is_err());
        assert!(standard_value(StandardControl::AeEnable, &serde_json::json!(1)).is_err());
        assert!(standard_value(StandardControl::AfMode, &serde_json::json!("sideways")).is_err());
        assert_eq!(backend_value("sharpness", &serde_json::json!(4)).unwrap(), ControlValue::Int(4));
        assert_eq!(backend_value("sharpness", &serde_json::json!(4.5)).unwrap(), ControlValue::Float(4.5));
        assert_eq!(value_json(&ControlValue::Float(0.1)), serde_json::json!(0.1));
    }

    /// A virtual camera with exposure, gain, AE, sharpness and a read-only temperature, served by
    /// a Styx camera service like helios-peripherals serves the OV9782.
    fn camera_service(socket: &Path) -> styx::ipc::CameraServiceHandle {
        use styx::prelude::{ColorSpace, ControlMeta, ControlMetadata, FourCc, Interval, MediaFormat, Mode, Resolution};
        let control = |id: u32, name: &str, kind: ControlKind, min: ControlValue, max: ControlValue| ControlMeta {
            id: ControlId(id),
            name: name.into(),
            kind,
            access: Access::ReadWrite,
            default: min.clone(),
            min,
            max,
            step: None,
            menu: None,
            metadata: ControlMetadata::default(),
        };
        let mut temperature = control(0x200, "sensor_temperature", ControlKind::Int, ControlValue::Int(40), ControlValue::Int(40));
        temperature.access = Access::ReadOnly;
        let mode = Mode::with_interval(MediaFormat::new(FourCc::GREY, Resolution::new(320, 200).expect("resolution"), ColorSpace::Srgb), Interval::from_fps(30).expect("interval"));
        let device = styx::capture_api::make_virtual_device_with_controls(
            "cam",
            [mode],
            vec![
                control(0xF400_0001, "exposure_time_us", ControlKind::Uint, ControlValue::Uint(10), ControlValue::Uint(33_000)),
                control(0xF400_0002, "gain", ControlKind::Float, ControlValue::Float(1.0), ControlValue::Float(16.0)),
                control(0xF400_0005, "ae_enable", ControlKind::Bool, ControlValue::Bool(false), ControlValue::Bool(true)),
                control(0x100, "sharpness", ControlKind::Int, ControlValue::Int(0), ControlValue::Int(10)),
                temperature,
            ],
        );
        styx::ipc::CameraService::new(device).keep_streaming().serve(socket).expect("serve camera")
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn controls_are_listed_applied_and_announced() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("cam.sock");
        let _service = camera_service(&socket);
        let hub = Arc::new(EventHub::new(16));
        let mut events = hub.subscribe();
        let cameras = CameraControls::default();
        let client = cameras.client("cam0", &socket, hub.clone()).await.expect("camera client");
        assert!(Arc::ptr_eq(&client, &cameras.client("cam0", &socket, hub.clone()).await.expect("again")), "one client per camera service");

        let controls = client.controls().await.expect("controls");
        let exposure = controls.iter().find(|control| control.standard == Some("exposure_us")).expect("exposure listed");
        assert!(exposure.writable && exposure.kind == "uint" && exposure.max == serde_json::json!(33_000), "{exposure:?}");
        assert!(controls.iter().any(|control| control.name == "sensor_temperature" && control.read_only));

        // Modes first; out-of-range values are clamped; backend controls by name.
        let request = serde_json::json!({ "exposure_us": 100_000, "sharpness": 4, "ae": false });
        let changes = client.resolve(request.as_object().expect("object")).await.expect("resolve");
        assert_eq!(changes[0].key, "ae");
        let applied = client.apply(changes).await.expect("apply");
        let exposure = applied.iter().find(|change| change.control == "exposure_us").expect("exposure applied");
        assert!(exposure.clamped && exposure.value == serde_json::json!(33_000), "{exposure:?}");
        assert!(applied.iter().any(|change| change.control == "sharpness" && change.value == serde_json::json!(4)));

        // Every change reaches the SSE `camera` events.
        let event = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = events.recv().await.expect("event");
                if event.data["control"]["standard"] == "exposure_us" {
                    return event;
                }
            }
        })
        .await
        .expect("exposure event");
        assert_eq!(event.kind, "camera");
        assert_eq!(event.data["id"], "cam0");
        assert_eq!(event.data["change"], "control");
        assert_eq!(event.data["control"]["value"], 33_000);

        // Unknown keys apply nothing; read-only controls are refused.
        let unknown = client.resolve(serde_json::json!({ "gain": 2, "nope": 1 }).as_object().expect("object")).await.expect_err("unknown control");
        assert_eq!(unknown.code, crate::error::ErrorCode::BadRequest);
        let changes = client.resolve(serde_json::json!({ "sensor_temperature": 41 }).as_object().expect("object")).await.expect("resolve");
        let refused = client.apply(changes).await.expect_err("read only");
        assert_eq!(refused.code, crate::error::ErrorCode::Unprocessable, "{refused:?}");
    }
}
