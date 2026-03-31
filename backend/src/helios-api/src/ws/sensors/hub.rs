use crate::http::AppState;
use crate::http::device::imu::imu_status_from_snapshot;
use crate::http::device::lighting::LightingRuntimeStatePayload;
use crate::http::device::power::{PowerStatusPayload, power_status_from_snapshot};
use crate::ipc::IpcHandles;
use helios_peripherals::dto::SensorScope;
use helios_peripherals::ipc::{FirmwareUpdate, SensorCommand, SensorEvent};
use lib_ipc::types::CommandId;
use serde::Serialize;
use std::sync::{Arc, Mutex as StdMutex, Weak};
use std::time::Duration;
use tokio::sync::{Mutex, broadcast};
use tokio::task::JoinHandle;

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
        Self {
            tx,
            latest: Arc::new(StdMutex::new(SharedSensorLatest::default())),
            task: Mutex::new(None),
            state: StdMutex::new(None),
        }
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
        let latest = self
            .latest
            .lock()
            .ok()
            .map(|guard| guard.clone())
            .unwrap_or_default();
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

#[derive(Debug, Clone, Serialize, Default)]
pub(crate) struct SnapshotPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) imu: Option<lib_sensors::dto::ImuStatusPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) power: Option<PowerStatusPayload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) lighting: Option<LightingRuntimeStatePayload>,
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

async fn run_sensor_events_sampler(
    tx: broadcast::Sender<Arc<SharedSensorEvent>>,
    latest: Arc<StdMutex<SharedSensorLatest>>,
    state: Option<Weak<IpcHandles>>,
) {
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
        let subscribe = SensorCommand::Subscribe {
            command_id: CommandId::new(),
            scope: scope.clone(),
        };
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
                let _ = session
                    .send_command(
                        conn.client.journal(),
                        &SensorCommand::Unsubscribe {
                            command_id: CommandId::new(),
                            scope: scope.clone(),
                        },
                    )
                    .await;
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

fn broadcast_sensor_error(
    tx: &broadcast::Sender<Arc<SharedSensorEvent>>,
    latest: &Arc<StdMutex<SharedSensorLatest>>,
    reason: impl Into<String>,
) {
    let reason = Arc::<str>::from(reason.into());
    if let Ok(mut guard) = latest.lock() {
        *guard = SharedSensorLatest {
            error: Some(reason.clone()),
            ..SharedSensorLatest::default()
        };
    }
    let _ = tx.send(Arc::new(SharedSensorEvent::Error(reason)));
}
