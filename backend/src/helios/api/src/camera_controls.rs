//! Camera controls through each camera's Styx camera service: list, read and set them, forward
//! every control change on the camera, by any client, as an SSE `camera` event, and keep the
//! values set through the API across reboots.
//!
//! The API keeps one Styx `ControlClient` per camera (`ControlClient::options(path)
//! .reconnecting().controls_nonblocking()`): it makes no frame request, so it never joins the
//! camera's frame plan, holds no buffers and never starts or restarts the capture by connecting
//! (Styx `docs/frame-server.md`, "Control clients (no frames)"). It connects in the background
//! and comes back after the camera service restarts. Requests are awaited on Styx's reactor
//! (`controls_async`, `set_control_async`), never blocking a runtime thread.
//!
//! **Persistence:** the values set through the API (standard keys and the camera's own controls,
//! by name) are stored in the API's state directory (`Store::persist_camera_settings`, on the
//! data partition) and re-applied every time the client (re)connects to the camera service: at
//! boot, and after the camera service restarts. A camera that is not streaming takes them as
//! deferred values, applied when its capture starts. Resetting to defaults forgets them.

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use serde::Serialize;
use styx::{
    ipc::{AppliedControl, ClientEvent, ControlClient, ControlDescriptor, ControlEvent, ControlRefusal, ControlTarget, IpcError, SERVICE_FRAME_RATE, StandardControl},
    prelude::{Access, ControlId, ControlKind, ControlValue, RecvOutcome},
};
use tokio::{sync::Mutex, task::JoinHandle};

use crate::{
    error::{ApiError, ApiResult},
    events::EventHub,
    store::Store,
};

/// How long one connection attempt to the camera service, or one control request, may take.
const CONTROL_TIMEOUT: Duration = Duration::from_secs(3);

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

/// Control kinds a value can be stored and reset for.
const SCALAR_KINDS: [ControlKind; 6] = [ControlKind::Bool, ControlKind::Int, ControlKind::Uint, ControlKind::Float, ControlKind::Menu, ControlKind::IntMenu];

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
    /// A value set through the API is stored for it and re-applied after a reboot.
    pub persisted: bool,
}

impl CameraControl {
    fn new(descriptor: &ControlDescriptor, persisted: &BTreeMap<String, serde_json::Value>) -> Self {
        let meta = &descriptor.meta;
        let standard = descriptor.standard.map(standard_key);
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
            persisted: persisted.contains_key(meta.name.as_str()) || standard.is_some_and(|key| persisted.contains_key(key)),
            standard,
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
    /// Where its value is stored: the standard key, or the control's listed name; `None` for an
    /// action (an AF trigger), which is not kept.
    pub persist_as: Option<String>,
}

/// The camera clients the API keeps, one per camera.
pub struct CameraControls {
    clients: Mutex<HashMap<String, Arc<CameraClient>>>,
    store: Arc<Store>,
    events: Arc<EventHub>,
}

/// The API's client of one camera's service.
pub struct CameraClient {
    camera_id: String,
    socket: PathBuf,
    client: Arc<ControlClient>,
    store: Arc<Store>,
    /// Connected to the camera service now (the follower sets it from the client's events).
    online: Arc<AtomicBool>,
    /// Follows the camera's control changes and re-applies the stored values on (re)connect.
    follower: Option<JoinHandle<()>>,
}

impl Drop for CameraClient {
    fn drop(&mut self) {
        if let Some(follower) = self.follower.take() {
            follower.abort();
        }
    }
}

impl CameraControls {
    pub fn new(store: Arc<Store>, events: Arc<EventHub>) -> Self {
        Self { clients: Mutex::new(HashMap::new()), store, events }
    }

    /// The client of camera `camera_id`'s service at `socket`: made on first use (or when the
    /// camera moved to another socket) without waiting for the service; its control changes are
    /// followed as `camera` events and the stored values applied whenever it connects.
    pub async fn client(&self, camera_id: &str, socket: &Path) -> ApiResult<Arc<CameraClient>> {
        let mut clients = self.clients.lock().await;
        if let Some(client) = clients.get(camera_id)
            && client.socket == socket
        {
            return Ok(client.clone());
        }
        let control = ControlClient::options(socket).timeout(CONTROL_TIMEOUT).reconnecting().controls_nonblocking().map_err(|error| control_error("", &[], error))?;
        let mut client = CameraClient { camera_id: camera_id.to_string(), socket: socket.to_path_buf(), client: Arc::new(control), store: self.store.clone(), online: Arc::default(), follower: None };
        client.follower = Some(tokio::spawn(follow(client.detached(), self.events.clone())));
        let client = Arc::new(client);
        clients.insert(camera_id.to_string(), client.clone());
        Ok(client)
    }

    /// Whether the API's client of camera `camera_id` is connected to its camera service (from
    /// Styx's connection events); `None` when the API keeps no client for it.
    pub async fn online(&self, camera_id: &str) -> Option<bool> {
        self.clients.lock().await.get(camera_id).map(|client| client.online())
    }
}

impl CameraClient {
    /// The same camera client without the follower (for the follower itself).
    fn detached(&self) -> Self {
        Self { camera_id: self.camera_id.clone(), socket: self.socket.clone(), client: self.client.clone(), store: self.store.clone(), online: self.online.clone(), follower: None }
    }

    /// Whether the client is connected to the camera service: set on Styx's `Connected`,
    /// cleared on `Disconnected`.
    pub fn online(&self) -> bool {
        self.online.load(Ordering::Acquire)
    }

    /// The camera's controls, with their values now and whether a stored value is kept for each.
    pub async fn controls(&self) -> ApiResult<Vec<CameraControl>> {
        let persisted = self.store.camera_settings(&self.camera_id).await?;
        self.controls_with(&persisted).await
    }

    async fn controls_with(&self, persisted: &BTreeMap<String, serde_json::Value>) -> ApiResult<Vec<CameraControl>> {
        let descriptors = self.client.controls_async().await.map_err(|error| control_error("", &[], error))?;
        Ok(descriptors.iter().map(|descriptor| CameraControl::new(descriptor, persisted)).collect())
    }

    /// The values stored for this camera.
    pub async fn persisted(&self) -> ApiResult<BTreeMap<String, serde_json::Value>> {
        self.store.camera_settings(&self.camera_id).await
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
            changes.push(self.resolve_one(key, value, &mut listed).await?);
        }
        sort_modes_first(&mut changes);
        Ok(changes)
    }

    /// One `key: value`; `listed` caches the camera's controls for keys that are not standard.
    async fn resolve_one(&self, key: &str, value: &serde_json::Value, listed: &mut Option<Vec<CameraControl>>) -> ApiResult<ControlChange> {
        if let Some((_, standard)) = STANDARD.iter().find(|(name, _)| *name == key) {
            let persist_as = (*standard != StandardControl::AfTrigger).then(|| key.to_string());
            return Ok(ControlChange { key: key.to_string(), target: ControlTarget::Standard(*standard), value: standard_value(*standard, value)?, persist_as });
        }
        if listed.is_none() {
            *listed = Some(self.controls_with(&BTreeMap::new()).await?);
        }
        let controls = listed.as_deref().unwrap_or_default();
        let control = controls
            .iter()
            .find(|control| control.name == key || control.id.to_string() == key || format!("{:#x}", control.id) == key.to_ascii_lowercase())
            .ok_or_else(|| ApiError::bad_request(format!("the camera has no control {key:?} (GET the settings for its controls)")))?;
        // Kept under its own name: the value is in the backend's units, not the standard ones.
        let persist_as = (control.standard != Some("af_trigger")).then(|| control.name.clone());
        Ok(ControlChange { key: key.to_string(), target: ControlTarget::Id(ControlId(control.id)), value: backend_value(key, value)?, persist_as })
    }

    /// Apply `changes` in order and store the values now in effect, so they are applied again
    /// after a reboot. A refusal stops there (the changes before it stay applied and stored,
    /// and were announced as events).
    pub async fn apply(&self, changes: Vec<ControlChange>) -> ApiResult<Vec<AppliedCameraControl>> {
        let (applied, stored, refused) = self.set_all(changes).await;
        if !stored.is_empty() {
            self.store.persist_camera_settings(&self.camera_id, stored).await?;
        }
        match refused {
            Some(error) => Err(error),
            None => Ok(applied),
        }
    }

    /// Put every writable control back to its default and forget the stored values.
    pub async fn reset(&self) -> ApiResult<Vec<AppliedCameraControl>> {
        self.store.clear_camera_settings(&self.camera_id).await?;
        let descriptors = self.client.controls_async().await.map_err(|error| control_error("", &[], error))?;
        let mut defaults =
            descriptors.iter().filter(|descriptor| descriptor.writable && descriptor.standard != Some(StandardControl::AfTrigger) && SCALAR_KINDS.contains(&descriptor.meta.kind)).collect::<Vec<_>>();
        // Modes first; by id, as defaults are in the backend's units.
        defaults.sort_by_key(|descriptor| !descriptor.standard.is_some_and(|standard| MODES_FIRST.contains(&standard)));
        let changes = defaults
            .into_iter()
            .map(|descriptor| {
                let key = descriptor.standard.map_or_else(|| descriptor.meta.name.clone(), |standard| standard_key(standard).to_string());
                ControlChange { key, target: ControlTarget::Id(descriptor.meta.id), value: descriptor.meta.default.clone(), persist_as: None }
            })
            .collect::<Vec<_>>();
        let (applied, _, refused) = self.set_all(changes).await;
        match refused {
            Some(error) => Err(error),
            None => Ok(applied),
        }
    }

    /// Apply the stored values (after the client connected). Keys the camera no longer has, or
    /// refuses, are skipped and reported.
    async fn reapply(&self) -> (Vec<AppliedCameraControl>, Vec<String>) {
        let stored = match self.store.camera_settings(&self.camera_id).await {
            Ok(stored) => stored,
            Err(error) => return (Vec::new(), vec![error.message]),
        };
        let mut listed = None;
        let mut changes = Vec::with_capacity(stored.len());
        let mut errors = Vec::new();
        for (key, value) in &stored {
            match self.resolve_one(key, value, &mut listed).await {
                Ok(change) => changes.push(change),
                Err(error) => errors.push(format!("{key}: {}", error.message)),
            }
        }
        sort_modes_first(&mut changes);
        let mut applied = Vec::with_capacity(changes.len());
        for change in changes {
            match self.client.set_control_async(change.target, change.value).await {
                Ok(result) => applied.push(AppliedCameraControl::new(change.key, &result)),
                Err(error) => errors.push(format!("{}: {error}", change.key)),
            }
        }
        (applied, errors)
    }

    /// Apply `changes` in order until one is refused: what was applied, the values to store for
    /// it, and the refusal.
    async fn set_all(&self, changes: Vec<ControlChange>) -> (Vec<AppliedCameraControl>, Vec<(String, serde_json::Value)>, Option<ApiError>) {
        let mut applied = Vec::with_capacity(changes.len());
        let mut stored = Vec::with_capacity(changes.len());
        for change in changes {
            // The value as asked for, in the units it was asked in: applied again, it is
            // clamped again the same way.
            let asked = value_json(&change.value);
            match self.client.set_control_async(change.target, change.value).await {
                Ok(result) => {
                    if let Some(key) = change.persist_as {
                        stored.push((key, asked));
                    }
                    applied.push(AppliedCameraControl::new(change.key, &result));
                }
                Err(error) => {
                    let refused = control_error(&change.key, &applied, error);
                    return (applied, stored, Some(refused));
                }
            }
        }
        (applied, stored, None)
    }
}

fn sort_modes_first(changes: &mut [ControlChange]) {
    changes.sort_by_key(|change| !matches!(change.target, ControlTarget::Standard(standard) if MODES_FIRST.contains(&standard)));
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

/// Follow the client's Styx `ClientEvent`s (each connection change exactly once, in order with
/// the control changes): publish control changes as `camera` events, apply the stored values on
/// every `Connected` (the camera service appeared or restarted), and mark the camera offline on
/// `Disconnected`. Runs until the client is dropped.
async fn follow(camera: CameraClient, hub: Arc<EventHub>) {
    loop {
        match camera.client.next_client_event().await {
            RecvOutcome::Data(ClientEvent::Data(event)) => hub.publish("camera", control_event_json(&camera.camera_id, &event)),
            RecvOutcome::Data(ClientEvent::Connected { reconnects }) => {
                camera.online.store(true, Ordering::Release);
                hub.publish("camera", serde_json::json!({ "id": camera.camera_id, "change": "online", "reconnects": reconnects }));
                let (applied, errors) = camera.reapply().await;
                if !errors.is_empty() {
                    tracing::warn!(camera = %camera.camera_id, ?errors, "stored camera settings not applied");
                }
                if !applied.is_empty() || !errors.is_empty() {
                    tracing::info!(camera = %camera.camera_id, restored = applied.len(), "stored camera settings applied");
                    hub.publish("camera", serde_json::json!({ "id": camera.camera_id, "change": "restored", "applied": applied, "errors": errors }));
                }
            }
            RecvOutcome::Data(ClientEvent::Disconnected { error }) => {
                camera.online.store(false, Ordering::Release);
                tracing::info!(camera = %camera.camera_id, %error, "camera service connection lost");
                hub.publish("camera", serde_json::json!({ "id": camera.camera_id, "change": "offline", "error": error.to_string() }));
            }
            // The client gave up (it reconnects, so only when its descriptors failed).
            _ => {
                camera.online.store(false, Ordering::Release);
                tracing::warn!(camera = %camera.camera_id, error = ?camera.client.last_error(), "camera control client closed");
                return;
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

    fn controls_for(dir: &Path, hub: &Arc<EventHub>) -> CameraControls {
        CameraControls::new(Arc::new(Store::new(dir.join("state"))), hub.clone())
    }

    fn current(controls: &[CameraControl], name: &str) -> Option<serde_json::Value> {
        controls.iter().find(|control| control.name == name).and_then(|control| control.current.clone())
    }

    /// Wait until the camera's controls read `exposure_time_us` = `exposure` and `sharpness` =
    /// `sharpness` (re-applied by the follower once it reconnected).
    async fn wait_for_values(client: &CameraClient, exposure: u32, sharpness: i32) {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Ok(controls) = client.controls().await
                    && current(&controls, "exposure_time_us") == Some(serde_json::json!(exposure))
                    && current(&controls, "sharpness") == Some(serde_json::json!(sharpness))
                {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("exposure {exposure} and sharpness {sharpness} were not applied"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn controls_are_listed_applied_and_announced() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("cam.sock");
        let service = camera_service(&socket);
        let hub = Arc::new(EventHub::new(16));
        let mut events = hub.subscribe();
        let cameras = controls_for(dir.path(), &hub);
        let client = cameras.client("cam0", &socket).await.expect("camera client");
        assert!(Arc::ptr_eq(&client, &cameras.client("cam0", &socket).await.expect("again")), "one client per camera");

        let controls = client.controls().await.expect("controls");
        let exposure = controls.iter().find(|control| control.standard == Some("exposure_us")).expect("exposure listed");
        assert!(exposure.writable && exposure.kind == "uint" && exposure.max == serde_json::json!(33_000) && !exposure.persisted, "{exposure:?}");
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

        // A control client: never one of the camera's frame clients, so it never joined (or
        // changed) the capture plan and holds no buffers.
        assert_eq!(service.stats().clients, 0);
    }

    /// Values set through the API are stored and applied again when the camera service comes
    /// back (a restart, a reboot); a reset puts the defaults back and forgets them.
    #[tokio::test(flavor = "multi_thread")]
    async fn settings_persist_and_are_reapplied_when_the_camera_service_returns() {
        let dir = tempfile::tempdir().expect("tempdir");
        let socket = dir.path().join("cam.sock");
        let hub = Arc::new(EventHub::new(64));
        let service = camera_service(&socket);
        let cameras = controls_for(dir.path(), &hub);
        let client = cameras.client("cam0", &socket).await.expect("camera client");

        let request = serde_json::json!({ "exposure_us": 5000, "sharpness": 4 });
        let changes = client.resolve(request.as_object().expect("object")).await.expect("resolve");
        client.apply(changes).await.expect("apply");
        let stored = client.persisted().await.expect("stored");
        assert_eq!(stored, BTreeMap::from([("exposure_us".to_string(), serde_json::json!(5000)), ("sharpness".to_string(), serde_json::json!(4))]));
        // Actions are not stored.
        let trigger = client.resolve(serde_json::json!({ "af_trigger": "start" }).as_object().expect("object")).await.expect("resolve");
        assert_eq!(trigger[0].persist_as, None);
        let controls = client.controls().await.expect("controls");
        assert!(controls.iter().any(|control| control.name == "exposure_time_us" && control.persisted));
        assert!(controls.iter().any(|control| control.name == "sharpness" && control.persisted));
        assert!(controls.iter().any(|control| control.name == "gain" && !control.persisted));

        // The follower handled the first Connected event.
        tokio::time::timeout(Duration::from_secs(5), async {
            while !client.online() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("camera online");

        // The camera service restarts with its defaults; the API's client sees the connection go
        // (offline), reconnects (online) and applies the stored values, in that order.
        let mut events = hub.subscribe();
        drop(service);
        let _ = std::fs::remove_file(&socket);
        let service = camera_service(&socket);
        wait_for_values(&client, 5000, 4).await;
        let (changes, restored) = tokio::time::timeout(Duration::from_secs(5), async {
            let mut changes = Vec::new();
            loop {
                let event = events.recv().await.expect("event");
                let change = event.data["change"].as_str().unwrap_or_default().to_string();
                if ["offline", "online", "restored"].contains(&change.as_str()) {
                    changes.push(change);
                }
                if event.data["change"] == "restored" {
                    return (changes, event);
                }
            }
        })
        .await
        .expect("restored event");
        assert_eq!(changes, ["offline", "online", "restored"]);
        assert_eq!(restored.data["id"], "cam0");
        assert!(client.online());

        // After a reboot: a new API (a new client, the same state directory) and a new service.
        drop(cameras);
        drop(client);
        drop(service);
        let _ = std::fs::remove_file(&socket);
        let _service = camera_service(&socket);
        let cameras = controls_for(dir.path(), &hub);
        let client = cameras.client("cam0", &socket).await.expect("camera client");
        wait_for_values(&client, 5000, 4).await;

        // Reset: defaults back, nothing stored.
        client.reset().await.expect("reset");
        assert!(client.persisted().await.expect("stored").is_empty());
        let controls = client.controls().await.expect("controls");
        assert_eq!(current(&controls, "exposure_time_us"), Some(serde_json::json!(10)));
        assert_eq!(current(&controls, "sharpness"), Some(serde_json::json!(0)));
        assert!(controls.iter().all(|control| !control.persisted));
    }
}
