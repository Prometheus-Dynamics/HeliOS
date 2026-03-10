use std::collections::BTreeSet;
use std::sync::{Arc, Mutex as StdMutex, Weak};
use std::time::Duration;

use crate::http::AppState;
use crate::http::device::imu::imu_status_from_snapshot;
use crate::http::device::lighting::LightingRuntimeStatePayload;
use crate::http::device::power::{PowerStatusPayload, power_status_from_snapshot};
use crate::http::error_history::{ErrorHistoryEntry, record_error_entry};
use crate::ipc::IpcHandles;
use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
};
use helios_peripherals::dto::SensorScope;
use helios_peripherals::ipc::{FirmwareUpdate, SensorCommand, SensorEvent};
use lib_ipc::types::CommandId;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, broadcast};
use tokio::task::JoinHandle;
use tracing::{warn, warn_span};
use uuid::Uuid;

const HUB_POLL_INTERVAL: Duration = Duration::from_millis(250);
const HUB_RETRY_DELAY: Duration = Duration::from_millis(500);

pub(crate) struct SensorEventsState {
    tx: broadcast::Sender<Arc<SharedSensorEvent>>,
    latest: Arc<StdMutex<SharedSensorLatest>>,
    task: Mutex<Option<JoinHandle<()>>>,
    state: StdMutex<Option<Weak<IpcHandles>>>,
}

impl Default for SensorEventsState {
    fn default() -> Self {
        let (tx, _) = broadcast::channel(128);
        Self { tx, latest: Arc::new(StdMutex::new(SharedSensorLatest::default())), task: Mutex::new(None), state: StdMutex::new(None) }
    }
}

impl SensorEventsState {
    pub(crate) fn bind_state(&self, state: &AppState) {
        let mut guard = self.state.lock().unwrap();
        if guard.as_ref().and_then(|weak| weak.upgrade()).is_none() {
            *guard = Some(Arc::downgrade(state.ipc()));
        }
    }

    pub(crate) async fn subscribe(&self) -> (broadcast::Receiver<Arc<SharedSensorEvent>>, SharedSensorLatest) {
        self.ensure_task().await;
        let latest = self.latest.lock().ok().map(|guard| guard.clone()).unwrap_or_default();
        (self.tx.subscribe(), latest)
    }

    async fn ensure_task(&self) {
        let mut guard = self.task.lock().await;
        let needs_spawn = guard.as_ref().map(|handle| handle.is_finished()).unwrap_or(true);
        if needs_spawn {
            let tx = self.tx.clone();
            let latest = self.latest.clone();
            let state = self.state.lock().unwrap().clone();
            *guard = Some(tokio::spawn(run_sensor_events_sampler(tx, latest, state)));
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Imu,
    Power,
    Firmware,
    Lighting,
}

impl Kind {
    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "imu" => Some(Self::Imu),
            "power" => Some(Self::Power),
            "firmware" => Some(Self::Firmware),
            "lighting" => Some(Self::Lighting),
            _ => None,
        }
    }
}

const VALID_KIND_NAMES: &[&str] = &["imu", "power", "firmware", "lighting"];

#[derive(Debug, Clone, Deserialize)]
struct ClientMessage {
    op: String,
    #[serde(default)]
    kinds: Option<Vec<String>>,
    #[serde(default)]
    interval_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub(crate) struct SnapshotPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) imu: Option<lib_sensors::dto::ImuStatusPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) power: Option<PowerStatusPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) lighting: Option<LightingRuntimeStatePayload>,
}

#[derive(Debug, Clone, Serialize)]
struct FirmwarePayload {
    firmware: FirmwareUpdate,
}

#[derive(Debug, Serialize)]
struct ErrorPayload {
    status: &'static str,
    reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    timestamp_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    operation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    trace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    retryable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remediation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reported_by: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SharedSensorLatest {
    pub(crate) snapshot: Option<Arc<SnapshotPayload>>,
    pub(crate) firmware: Option<Arc<FirmwareUpdate>>,
    pub(crate) lighting: Option<Arc<LightingRuntimeStatePayload>>,
    pub(crate) error: Option<Arc<str>>,
}

#[derive(Debug, Clone)]
pub(crate) enum SharedSensorEvent {
    Snapshot(Arc<SnapshotPayload>),
    Firmware(Arc<FirmwareUpdate>),
    Lighting(Arc<LightingRuntimeStatePayload>),
    Error(Arc<str>),
}

#[derive(Debug, Clone)]
struct WsErrorContext {
    request_id: String,
    trace_id: String,
}

pub async fn sensors_upgrade(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    state.services.hardware.bind_sensor_events_state(&state);
    ws.on_upgrade(move |socket| async move {
        let span = warn_span!("sensors_ws");
        let _guard = span.enter();
        if let Err(err) = sensors_loop(socket, state).await {
            warn!(%err, "sensors websocket terminated");
        }
    })
}

async fn sensors_loop(mut socket: WebSocket, state: AppState) -> Result<(), String> {
    state.services.hardware.bind_sensor_events_state(&state);

    let error_context = WsErrorContext { request_id: Uuid::new_v4().to_string(), trace_id: Uuid::new_v4().to_string() };
    let (mut updates, mut latest) = state.services.hardware.subscribe_sensor_events().await;
    if let Some(reason) = latest.error.as_ref() {
        send_ws_error(&mut socket, &error_context, reason.as_ref(), "connect").await;
        return Ok(());
    }

    let mut requested: BTreeSet<Kind> = BTreeSet::new();
    let mut min_interval = Duration::from_millis(100);
    let mut last_sent = tokio::time::Instant::now() - min_interval;

    loop {
        tokio::select! {
            update = updates.recv() => {
                match update {
                    Ok(update) => {
                        match update.as_ref() {
                            SharedSensorEvent::Snapshot(snapshot) => {
                                latest.snapshot = Some(snapshot.clone());
                                latest.error = None;
                                if requested.is_empty() {
                                    continue;
                                }

                                let now = tokio::time::Instant::now();
                                if now.duration_since(last_sent) < min_interval {
                                    continue;
                                }

                                if let Some(payload) = build_payload(&requested, snapshot.as_ref()) {
                                    last_sent = now;
                                    if send_json_message(&mut socket, &payload).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            SharedSensorEvent::Firmware(update) => {
                                latest.firmware = Some(update.clone());
                                latest.error = None;
                                if requested.contains(&Kind::Firmware) {
                                    let payload = FirmwarePayload { firmware: (*update.as_ref()).clone() };
                                    if send_json_message(&mut socket, &payload).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            SharedSensorEvent::Lighting(state) => {
                                latest.lighting = Some(state.clone());
                                latest.error = None;
                                if requested.contains(&Kind::Lighting) {
                                    let payload = SnapshotPayload {
                                        lighting: Some((*state.as_ref()).clone()),
                                        ..SnapshotPayload::default()
                                    };
                                    if send_json_message(&mut socket, &payload).await.is_err() {
                                        break;
                                    }
                                }
                            }
                            SharedSensorEvent::Error(reason) => {
                                send_ws_error(&mut socket, &error_context, reason.as_ref(), "stream").await;
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(Message::Text(text))) => {
                        if text.trim().eq_ignore_ascii_case("unsubscribe") {
                            requested.clear();
                            continue;
                        }
                        if let Err(err) = handle_client_message(&text, &mut requested, &mut min_interval) {
                            send_ws_error(&mut socket, &error_context, err, "client_message").await;
                            continue;
                        }

                        if send_cached_for_requested(&mut socket, &requested, &latest, &error_context).await.is_err() {
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

async fn run_sensor_events_sampler(tx: broadcast::Sender<Arc<SharedSensorEvent>>, latest: Arc<StdMutex<SharedSensorLatest>>, state: Option<Weak<IpcHandles>>) {
    let Some(state) = state.and_then(|weak| weak.upgrade()) else {
        return;
    };

    let scope = SensorScope::Device;
    let mut saw_receiver = tx.receiver_count() > 0;

    loop {
        let receiver_count = tx.receiver_count();
        saw_receiver |= receiver_count > 0;
        if saw_receiver && receiver_count == 0 {
            break;
        }

        if state.ensure_sensors().await.is_none() {
            broadcast_sensor_error(&tx, &latest, "peripherals IPC unavailable");
            tokio::time::sleep(HUB_RETRY_DELAY).await;
            continue;
        }

        let conn = match crate::ipc::peripherals::connect_sensors_stream().await {
            Ok(conn) => conn,
            Err(err) => {
                broadcast_sensor_error(&tx, &latest, err.to_string());
                tokio::time::sleep(HUB_RETRY_DELAY).await;
                continue;
            }
        };

        let mut session = conn.session;
        let subscribe = SensorCommand::Subscribe { command_id: CommandId::new(), scope: scope.clone() };
        if let Err(err) = session.send_command(conn.client.journal(), &subscribe).await {
            broadcast_sensor_error(&tx, &latest, format!("failed to subscribe: {err}"));
            tokio::time::sleep(HUB_RETRY_DELAY).await;
            continue;
        }
        clear_sensor_error(&latest);

        let mut should_retry = true;
        loop {
            let receiver_count = tx.receiver_count();
            saw_receiver |= receiver_count > 0;
            if saw_receiver && receiver_count == 0 {
                let _ = session.send_command(conn.client.journal(), &SensorCommand::Unsubscribe { command_id: CommandId::new(), scope: scope.clone() }).await;
                should_retry = false;
                break;
            }

            tokio::select! {
                event = session.next_event() => {
                    match event {
                        Ok(Some(SensorEvent::Snapshot { scope: event_scope, values, .. })) if event_scope == scope => {
                            let payload = Arc::new(SnapshotPayload {
                                imu: Some(imu_status_from_snapshot(&values)),
                                power: Some(power_status_from_snapshot(&values)),
                                lighting: None,
                            });
                            update_latest_snapshot(&latest, payload.clone());
                            let _ = tx.send(Arc::new(SharedSensorEvent::Snapshot(payload)));
                        }
                        Ok(Some(SensorEvent::FirmwareUpdate { update })) => {
                            let payload = Arc::new(update);
                            update_latest_firmware(&latest, payload.clone());
                            let _ = tx.send(Arc::new(SharedSensorEvent::Firmware(payload)));
                        }
                        Ok(Some(SensorEvent::LightingState { state, .. })) => {
                            let payload = Arc::new(LightingRuntimeStatePayload::from(state));
                            update_latest_lighting(&latest, payload.clone());
                            let _ = tx.send(Arc::new(SharedSensorEvent::Lighting(payload)));
                        }
                        Ok(Some(SensorEvent::Nack { reason, .. })) => {
                            broadcast_sensor_error(&tx, &latest, reason);
                            break;
                        }
                        Ok(Some(SensorEvent::Unsubscribed { scope: event_scope })) if event_scope == scope => {
                            broadcast_sensor_error(&tx, &latest, "sensor stream closed");
                            break;
                        }
                        Ok(None) => {
                            broadcast_sensor_error(&tx, &latest, "sensor stream closed");
                            break;
                        }
                        Err(err) => {
                            broadcast_sensor_error(&tx, &latest, err.to_string());
                            break;
                        }
                        _ => {}
                    }
                }
                _ = tokio::time::sleep(HUB_POLL_INTERVAL) => {}
            }
        }

        if !should_retry {
            break;
        }
        tokio::time::sleep(HUB_RETRY_DELAY).await;
    }
}

fn update_latest_snapshot(latest: &Arc<StdMutex<SharedSensorLatest>>, payload: Arc<SnapshotPayload>) {
    if let Ok(mut guard) = latest.lock() {
        guard.snapshot = Some(payload);
        guard.error = None;
    }
}

fn update_latest_firmware(latest: &Arc<StdMutex<SharedSensorLatest>>, payload: Arc<FirmwareUpdate>) {
    if let Ok(mut guard) = latest.lock() {
        guard.firmware = Some(payload);
        guard.error = None;
    }
}

fn update_latest_lighting(latest: &Arc<StdMutex<SharedSensorLatest>>, payload: Arc<LightingRuntimeStatePayload>) {
    if let Ok(mut guard) = latest.lock() {
        guard.lighting = Some(payload);
        guard.error = None;
    }
}

fn clear_sensor_error(latest: &Arc<StdMutex<SharedSensorLatest>>) {
    if let Ok(mut guard) = latest.lock() {
        guard.error = None;
    }
}

fn broadcast_sensor_error(tx: &broadcast::Sender<Arc<SharedSensorEvent>>, latest: &Arc<StdMutex<SharedSensorLatest>>, reason: impl Into<String>) {
    let reason = Arc::<str>::from(reason.into());
    if let Ok(mut guard) = latest.lock() {
        *guard = SharedSensorLatest { error: Some(reason.clone()), ..SharedSensorLatest::default() };
    }
    let _ = tx.send(Arc::new(SharedSensorEvent::Error(reason)));
}

fn handle_client_message(text: &str, requested: &mut BTreeSet<Kind>, min_interval: &mut Duration) -> Result<(), String> {
    let msg: ClientMessage = serde_json::from_str(text).map_err(|err| format!("invalid message: {err}"))?;
    let op = msg.op.trim().to_ascii_lowercase();

    if let Some(interval_ms) = msg.interval_ms {
        if !(50..=10_000).contains(&interval_ms) {
            return Err("interval_ms must be between 50 and 10000".into());
        }
        *min_interval = Duration::from_millis(interval_ms);
    }

    match op.as_str() {
        "subscribe" => {
            let kinds = msg.kinds.unwrap_or_default();
            if kinds.is_empty() {
                return Err("subscribe requires kinds".into());
            }
            let mut accepted = 0usize;
            let mut unknown = Vec::new();
            for kind in kinds {
                if let Some(kind) = Kind::parse(&kind) {
                    requested.insert(kind);
                    accepted += 1;
                } else {
                    unknown.push(kind);
                }
            }
            if !unknown.is_empty() {
                return Err(format!("unsupported kinds: {} (valid: {})", unknown.join(", "), VALID_KIND_NAMES.join(", ")));
            }
            if accepted == 0 {
                return Err(format!("subscribe requires at least one valid kind ({})", VALID_KIND_NAMES.join(", ")));
            }
            Ok(())
        }
        "unsubscribe" => {
            if let Some(kinds) = msg.kinds {
                let mut removed = 0usize;
                let mut unknown = Vec::new();
                for kind in kinds {
                    if let Some(kind) = Kind::parse(&kind) {
                        requested.remove(&kind);
                        removed += 1;
                    } else {
                        unknown.push(kind);
                    }
                }
                if !unknown.is_empty() {
                    return Err(format!("unsupported kinds: {} (valid: {})", unknown.join(", "), VALID_KIND_NAMES.join(", ")));
                }
                if removed == 0 {
                    return Err(format!("unsubscribe requires at least one valid kind ({})", VALID_KIND_NAMES.join(", ")));
                }
            } else {
                requested.clear();
            }
            Ok(())
        }
        other => Err(format!("unsupported op: {other}")),
    }
}

fn build_payload(requested: &BTreeSet<Kind>, values: &SnapshotPayload) -> Option<SnapshotPayload> {
    let mut payload = SnapshotPayload::default();
    for kind in requested {
        match kind {
            Kind::Imu => payload.imu = values.imu.clone(),
            Kind::Power => payload.power = values.power.clone(),
            Kind::Firmware => {}
            Kind::Lighting => payload.lighting = values.lighting.clone(),
        }
    }
    if payload.imu.is_none() && payload.power.is_none() && payload.lighting.is_none() { None } else { Some(payload) }
}

async fn send_cached_for_requested(socket: &mut WebSocket, requested: &BTreeSet<Kind>, latest: &SharedSensorLatest, error_context: &WsErrorContext) -> Result<(), ()> {
    if requested.is_empty() {
        return Ok(());
    }

    if let Some(reason) = latest.error.as_ref() {
        send_ws_error(socket, error_context, reason.as_ref(), "connect").await;
        return Err(());
    }

    if let Some(snapshot) = latest.snapshot.as_ref()
        && let Some(payload) = build_payload(requested, snapshot.as_ref())
        && send_json_message(socket, &payload).await.is_err()
    {
        return Err(());
    }

    if requested.contains(&Kind::Lighting)
        && let Some(lighting) = latest.lighting.as_ref()
    {
        let payload = SnapshotPayload { lighting: Some((*lighting.as_ref()).clone()), ..SnapshotPayload::default() };
        if send_json_message(socket, &payload).await.is_err() {
            return Err(());
        }
    }

    if requested.contains(&Kind::Firmware)
        && let Some(update) = latest.firmware.as_ref()
    {
        let payload = FirmwarePayload { firmware: (*update.as_ref()).clone() };
        if send_json_message(socket, &payload).await.is_err() {
            return Err(());
        }
    }

    Ok(())
}

async fn send_json_message<T: Serialize>(socket: &mut WebSocket, payload: &T) -> Result<(), ()> {
    let body = serde_json::to_string(payload).map_err(|_| ())?;
    socket.send(Message::Text(body.into())).await.map_err(|_| ())
}

async fn send_ws_error(socket: &mut WebSocket, context: &WsErrorContext, reason: impl Into<String>, operation: &str) {
    let reason = reason.into();
    let payload = ErrorPayload {
        status: "sensors_stream_unavailable",
        reason: reason.clone(),
        code: None,
        timestamp_ms: chrono::Utc::now().timestamp_millis().max(0) as u64,
        source: Some("helios-api/ws/sensors".to_string()),
        operation: Some(operation.to_string()),
        request_id: Some(context.request_id.clone()),
        trace_id: Some(context.trace_id.clone()),
        retryable: None,
        remediation: None,
        reported_by: Some("helios-api".to_string()),
    };
    record_error_entry(ErrorHistoryEntry {
        id: Uuid::new_v4().to_string(),
        status: None,
        code: "sensors_stream_unavailable".to_string(),
        error: reason,
        details: None,
        timestamp_ms: payload.timestamp_ms,
        source: payload.source.clone(),
        operation: payload.operation.clone(),
        request_id: payload.request_id.clone(),
        trace_id: payload.trace_id.clone(),
        retryable: payload.retryable,
        remediation: payload.remediation.clone(),
        reported_by: payload.reported_by.clone(),
        transport: Some("ws".to_string()),
    });
    match serde_json::to_string(&payload) {
        Ok(body) => {
            let _ = socket.send(Message::Text(body.into())).await;
        }
        Err(err) => {
            warn!(%err, "failed to serialize websocket error payload");
            let _ = socket.send(Message::Text(r#"{"status":"sensors_stream_unavailable","reason":"internal error"}"#.to_string().into())).await;
        }
    }
}
