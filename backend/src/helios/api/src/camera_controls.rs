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
//! deferred values, applied when its capture starts. A `null` value sets one control back to its
//! descriptor default and forgets its stored value (`Store::forget_camera_settings`); resetting
//! all of them to defaults forgets every stored value. The frame rate is the exception: it is not
//! re-applied here (see `reapply`), but with the pipelines' frame requests, which carry it
//! (`pipelines::refresh_camera_inputs`).

mod json;
#[cfg(test)]
mod tests;
mod values;

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use styx::{
    ipc::{ClientEvent, ControlClient, ControlDescriptor, ControlRefusal, ControlTarget, IpcError, StandardControl},
    prelude::{ControlKind, ControlValue, RecvOutcome},
};
use tokio::{sync::Mutex, task::JoinHandle};

use crate::{
    error::{ApiError, ApiResult},
    events::EventHub,
    store::Store,
};

pub use json::{AppliedCameraControl, CameraControl, control_event_json, value_json};
pub use values::standard_key;
use values::{MODES_FIRST, STANDARD, backend_value, default_value, standard_value};

/// How long one connection attempt to the camera service, or one control request, may take.
const CONTROL_TIMEOUT: Duration = Duration::from_secs(3);

/// Control kinds a value can be stored and reset for.
const SCALAR_KINDS: [ControlKind; 6] = [ControlKind::Bool, ControlKind::Int, ControlKind::Uint, ControlKind::Float, ControlKind::Menu, ControlKind::IntMenu];

/// The sensor's frame duration, which a running processed capture keeps (a reset cannot change it
/// under the pipelines' frame requests: the camera service refuses another rate, see `reset`).
const FRAME_DURATION_CONTROL: &str = "frame_duration_us";

/// One control change to make: what it was asked for as, the target and the value.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlChange {
    pub key: String,
    pub target: ControlTarget,
    /// The value to set; for a `forget` change, the control's default.
    pub value: ControlValue,
    /// The standard control it answers (modes are applied first).
    pub standard: Option<StandardControl>,
    /// Where its value is stored: the standard key, or the control's listed name; `None` for an
    /// action (an AF trigger), which is not kept.
    pub persist_as: Option<String>,
    /// A `null` in the request: the control goes to its default, and the value stored under
    /// `persist_as` is forgotten instead of stored.
    pub forget: bool,
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

/// What a batch of changes did: each applied in order, the values to store and the keys to
/// forget for them, and the refusal that stopped the batch.
struct Batch {
    applied: Vec<AppliedCameraControl>,
    stored: Vec<(String, serde_json::Value)>,
    forgotten: Vec<String>,
    refused: Option<ApiError>,
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
    /// (`exposure_us`, `ae`, ...), else a control's listed `name` or numeric `id`. A `null` value
    /// forgets the control (it goes to its default). Nothing is applied when any key or value is
    /// unusable.
    pub async fn resolve(&self, request: &serde_json::Map<String, serde_json::Value>) -> ApiResult<Vec<ControlChange>> {
        if request.is_empty() {
            return Err(ApiError::bad_request("name at least one control: {\"exposure_us\": 8000, ...}"));
        }
        let mut listed: Option<Vec<ControlDescriptor>> = None;
        let mut changes = Vec::with_capacity(request.len());
        for (key, value) in request {
            changes.push(self.resolve_one(key, value, &mut listed).await?);
        }
        sort_modes_first(&mut changes);
        Ok(changes)
    }

    /// One `key: value`; `listed` caches the camera's controls, listed once for the keys that
    /// need them (every backend key, and a standard key being forgotten, for its default).
    async fn resolve_one(&self, key: &str, value: &serde_json::Value, listed: &mut Option<Vec<ControlDescriptor>>) -> ApiResult<ControlChange> {
        let standard = STANDARD.iter().find(|(name, _)| *name == key).map(|(_, standard)| *standard);
        if let Some(standard) = standard
            && !value.is_null()
        {
            let persist_as = (standard != StandardControl::AfTrigger).then(|| key.to_string());
            return Ok(ControlChange { key: key.to_string(), target: ControlTarget::Standard(standard), value: standard_value(standard, value)?, standard: Some(standard), persist_as, forget: false });
        }
        let controls = self.listed_controls(listed).await?;
        let control = find_control(key, standard, controls)?;
        if value.is_null() {
            if control.standard == Some(StandardControl::AfTrigger) {
                return Err(ApiError::bad_request("af_trigger is an action: it has no value to forget"));
            }
            // Forgotten under the key it is stored by: the standard key, or the control's name.
            let persist_as = if standard.is_some() { key.to_string() } else { control.meta.name.clone() };
            return Ok(ControlChange {
                key: key.to_string(),
                target: ControlTarget::Id(control.meta.id),
                value: control.meta.default.clone(),
                standard: control.standard,
                persist_as: Some(persist_as),
                forget: true,
            });
        }
        // Kept under its own name: the value is in the backend's units, not the standard ones.
        // A player's transport (its replay controls) is a session, not a setting: not kept.
        let persist_as = (control.standard != Some(StandardControl::AfTrigger) && !crate::playback::transport::is_transport_control(&control.meta.name)).then(|| control.meta.name.clone());
        Ok(ControlChange { key: key.to_string(), target: ControlTarget::Id(control.meta.id), value: backend_value(key, value)?, standard: control.standard, persist_as, forget: false })
    }

    /// The camera's controls as the service lists them, listed once per `listed` cache.
    async fn listed_controls<'a>(&self, listed: &'a mut Option<Vec<ControlDescriptor>>) -> ApiResult<&'a [ControlDescriptor]> {
        if listed.is_none() {
            *listed = Some(self.client.controls_async().await.map_err(|error| control_error("", &[], error))?);
        }
        Ok(listed.as_deref().unwrap_or_default())
    }

    /// Apply `changes` in order and store the values now in effect, so they are applied again
    /// after a reboot; a forgotten control's stored value is removed. A refusal stops there (the
    /// changes before it stay applied, stored or forgotten, and were announced as events).
    pub async fn apply(&self, changes: Vec<ControlChange>) -> ApiResult<Vec<AppliedCameraControl>> {
        let batch = self.set_all(changes).await;
        if !batch.stored.is_empty() {
            self.store.persist_camera_settings(&self.camera_id, batch.stored).await?;
        }
        if !batch.forgotten.is_empty() {
            self.store.forget_camera_settings(&self.camera_id, batch.forgotten).await?;
        }
        match batch.refused {
            Some(error) => Err(error),
            None => Ok(batch.applied),
        }
    }

    /// Put every writable control back to its default and forget the stored values.
    pub async fn reset(&self) -> ApiResult<Vec<AppliedCameraControl>> {
        self.store.clear_camera_settings(&self.camera_id).await?;
        let descriptors = self.client.controls_async().await.map_err(|error| control_error("", &[], error))?;
        // The frame rate and duration are not reset here: the pipelines' frame requests carry the
        // rate (`refresh_pipelines` after this call), and a running capture refuses a new duration
        // ("keeps the frame rate it started with"), which would stop the reset half-way.
        let mut changes = descriptors
            .iter()
            .filter(|descriptor| descriptor.writable && descriptor.standard != Some(StandardControl::AfTrigger) && descriptor.standard != Some(StandardControl::FrameRate))
            .filter(|descriptor| descriptor.meta.name != FRAME_DURATION_CONTROL && SCALAR_KINDS.contains(&descriptor.meta.kind))
            .map(|descriptor| ControlChange {
                key: descriptor.standard.map_or_else(|| descriptor.meta.name.clone(), |standard| standard_key(standard).to_string()),
                target: ControlTarget::Id(descriptor.meta.id),
                value: default_value(&descriptor.meta),
                standard: descriptor.standard,
                persist_as: None,
                forget: false,
            })
            .collect::<Vec<_>>();
        sort_modes_first(&mut changes);
        let batch = self.set_all(changes).await;
        match batch.refused {
            Some(error) => Err(error),
            None => Ok(batch.applied),
        }
    }

    /// Apply the stored values (after the client connected). Keys the camera no longer has, or
    /// refuses, are skipped and reported.
    ///
    /// The stored frame rate is not applied here. It is a service-wide override that lasts until
    /// the service restarts, while every pipeline on the camera asks for the stored rate as an
    /// exact one with its frames (`pipelines::refresh_camera_inputs`). Restoring both could
    /// disagree (the service refuses an exact request for another rate), and the frame requests
    /// alone apply the rate from the first frame of every connection, so the control is left out.
    /// A camera with no pipeline runs at its default rate after its service restarts.
    async fn reapply(&self) -> (Vec<AppliedCameraControl>, Vec<String>) {
        let stored = match self.store.camera_settings(&self.camera_id).await {
            Ok(stored) => stored,
            Err(error) => return (Vec::new(), vec![error.message]),
        };
        let mut listed = None;
        let mut changes = Vec::with_capacity(stored.len());
        let mut errors = Vec::new();
        for (key, value) in stored.iter().filter(|(key, _)| key.as_str() != standard_key(StandardControl::FrameRate)) {
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

    /// Apply `changes` in order until one is refused.
    async fn set_all(&self, changes: Vec<ControlChange>) -> Batch {
        let mut batch = Batch { applied: Vec::with_capacity(changes.len()), stored: Vec::with_capacity(changes.len()), forgotten: Vec::new(), refused: None };
        for change in changes {
            let ControlChange { key, target, value, persist_as, forget, .. } = change;
            // The value as asked for, in the units it was asked in: applied again, it is clamped
            // again the same way.
            let asked = value_json(&value);
            match self.client.set_control_async(target, value).await {
                Ok(result) => {
                    match (forget, persist_as) {
                        (true, Some(stored_as)) => batch.forgotten.push(stored_as),
                        (false, Some(stored_as)) => batch.stored.push((stored_as, asked)),
                        _ => {}
                    }
                    batch.applied.push(AppliedCameraControl::new(key, &result));
                }
                Err(error) => {
                    batch.refused = Some(control_error(&key, &batch.applied, error));
                    break;
                }
            }
        }
        batch
    }
}

/// The listed control a request key names: the standard control it answers for a standard key,
/// else the one with that listed name, numeric id or hex id.
fn find_control<'a>(key: &str, standard: Option<StandardControl>, controls: &'a [ControlDescriptor]) -> ApiResult<&'a ControlDescriptor> {
    let found = match standard {
        Some(standard) => controls.iter().find(|control| control.standard == Some(standard)),
        None => controls.iter().find(|control| control.meta.name == key || control.meta.id.0.to_string() == key || format!("{:#x}", control.meta.id.0) == key.to_ascii_lowercase()),
    };
    found.ok_or_else(|| ApiError::bad_request(format!("the camera has no control {key:?} (GET the settings for its controls)")))
}

fn sort_modes_first(changes: &mut [ControlChange]) {
    changes.sort_by_key(|change| !change.standard.is_some_and(|standard| MODES_FIRST.contains(&standard)));
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
