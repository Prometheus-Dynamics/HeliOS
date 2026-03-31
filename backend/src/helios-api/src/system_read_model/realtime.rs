use std::collections::BTreeSet;
use std::sync::{Arc, Mutex as StdMutex, Weak};
use std::time::Duration as StdDuration;

use schemars::JsonSchema;
use serde::Serialize;
use sysinfo::{ProcessesToUpdate, System};
use tokio::sync::{Mutex, broadcast, oneshot};
use tokio::task::JoinHandle;
use tokio::time::{Duration, Instant};
use tracing::{debug, warn};

use crate::ipc::IpcHandles;
use crate::ws::device::{EngineTelemetry, PowerTelemetry};

use super::{
    SystemCollector, broadcast_devices_update, devices_updates_stream_poll_interval, reason_for_update_kind, sample_power_from_peripherals, spawn_api_sampler_thread, stream_fingerprint,
    usb_fingerprint,
};

#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ProcessSample {
    pub pid: u32,
    pub name: String,
    pub cpu_percent: f32,
    pub memory_bytes: u64,
    pub virtual_memory_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cmd: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SharedProcessesSnapshot {
    pub timestamp_ms: u64,
    pub total_memory_bytes: u64,
    pub used_memory_bytes: u64,
    pub processes: Arc<[ProcessSample]>,
}

#[derive(Debug, Clone, Copy, Serialize, JsonSchema, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum DevicesUpdateReason {
    Api,
    Pipelines,
    Localization,
    Media,
    Imu,
    Device,
    Settings,
    Usb,
    Streams,
}

#[derive(Debug, Clone)]
pub struct SharedDevicesUpdate {
    pub timestamp_ms: u64,
    pub reasons: Vec<DevicesUpdateReason>,
}

pub(super) struct ProcessesHub {
    tx: broadcast::Sender<Arc<SharedProcessesSnapshot>>,
    latest: Arc<StdMutex<Option<Arc<SharedProcessesSnapshot>>>>,
    task: Mutex<Option<JoinHandle<()>>>,
}

pub(super) struct DevicesUpdatesHub {
    tx: broadcast::Sender<Arc<SharedDevicesUpdate>>,
    task: Mutex<Option<JoinHandle<()>>>,
    state: StdMutex<Option<Weak<IpcHandles>>>,
}

pub(super) struct TelemetryHub {
    tx: broadcast::Sender<Arc<str>>,
    latest: Arc<StdMutex<Option<Arc<str>>>>,
    task: Mutex<Option<JoinHandle<()>>>,
    state: StdMutex<Option<Weak<IpcHandles>>>,
}

impl TelemetryHub {
    pub(super) fn new() -> Self {
        let (tx, _) = broadcast::channel(32);
        Self { tx, latest: Arc::new(StdMutex::new(None)), task: Mutex::new(None), state: StdMutex::new(None) }
    }

    pub(super) fn set_state(&self, state: &Arc<IpcHandles>) {
        let mut guard = self.state.lock().expect("telemetry state poisoned");
        if guard.as_ref().and_then(|weak| weak.upgrade()).is_none() {
            *guard = Some(Arc::downgrade(state));
        }
    }

    pub(super) async fn subscribe(&self, collector: Arc<StdMutex<SystemCollector>>) -> (broadcast::Receiver<Arc<str>>, Option<Arc<str>>) {
        self.ensure_task(collector).await;
        let latest = self.latest.lock().ok().and_then(|guard| guard.clone());
        (self.tx.subscribe(), latest)
    }

    async fn ensure_task(&self, collector: Arc<StdMutex<SystemCollector>>) {
        let mut guard = self.task.lock().await;
        let needs_spawn = guard.as_ref().map(|handle| handle.is_finished()).unwrap_or(true);
        if needs_spawn {
            let tx = self.tx.clone();
            let latest = self.latest.clone();
            let state = self.state.lock().expect("telemetry state poisoned").clone();
            *guard = Some(tokio::spawn(run_telemetry_sampler(tx, latest, state, collector)));
        }
    }

    pub(super) fn subscriber_count(&self) -> u64 {
        self.tx.receiver_count() as u64
    }
}

impl DevicesUpdatesHub {
    pub(super) fn new() -> Self {
        let (tx, _) = broadcast::channel(64);
        Self { tx, task: Mutex::new(None), state: StdMutex::new(None) }
    }

    pub(super) fn set_state(&self, state: &Arc<IpcHandles>) {
        let mut guard = self.state.lock().expect("devices updates state poisoned");
        if guard.as_ref().and_then(|weak| weak.upgrade()).is_none() {
            *guard = Some(Arc::downgrade(state));
        }
    }

    pub(super) async fn subscribe(&self) -> broadcast::Receiver<Arc<SharedDevicesUpdate>> {
        self.ensure_task().await;
        self.tx.subscribe()
    }

    async fn ensure_task(&self) {
        let mut guard = self.task.lock().await;
        let needs_spawn = guard.as_ref().map(|handle| handle.is_finished()).unwrap_or(true);
        if needs_spawn {
            let tx = self.tx.clone();
            let state = self.state.lock().expect("devices updates state poisoned").clone();
            *guard = Some(tokio::spawn(run_devices_updates_sampler(tx, state)));
        }
    }

    pub(super) fn subscriber_count(&self) -> u64 {
        self.tx.receiver_count() as u64
    }
}

impl ProcessesHub {
    pub(super) fn new() -> Self {
        let (tx, _) = broadcast::channel(64);
        Self { tx, latest: Arc::new(StdMutex::new(None)), task: Mutex::new(None) }
    }

    pub(super) async fn subscribe(&self) -> (broadcast::Receiver<Arc<SharedProcessesSnapshot>>, Option<Arc<SharedProcessesSnapshot>>) {
        self.ensure_task().await;
        let latest = self.latest.lock().ok().and_then(|guard| guard.clone());
        (self.tx.subscribe(), latest)
    }

    async fn ensure_task(&self) {
        let mut guard = self.task.lock().await;
        let needs_spawn = guard.as_ref().map(|handle| handle.is_finished()).unwrap_or(true);
        if needs_spawn {
            let tx = self.tx.clone();
            let latest = self.latest.clone();
            *guard = Some(tokio::spawn(async move {
                let (done_tx, done_rx) = oneshot::channel();
                let fallback_tx = tx.clone();
                let fallback_latest = latest.clone();
                match spawn_api_sampler_thread("helios-api-procs", move || {
                    run_processes_sampler(tx, latest);
                    let _ = done_tx.send(());
                }) {
                    Ok(_join) => {
                        let _ = done_rx.await;
                    }
                    Err(err) => {
                        warn!(
                            error = %err,
                            "failed to spawn dedicated processes sampler thread; falling back to Tokio blocking pool"
                        );
                        let _ = tokio::task::spawn_blocking(move || run_processes_sampler(fallback_tx, fallback_latest)).await;
                    }
                }
            }));
        }
    }

    pub(super) fn subscriber_count(&self) -> u64 {
        self.tx.receiver_count() as u64
    }
}

async fn run_devices_updates_sampler(tx: broadcast::Sender<Arc<SharedDevicesUpdate>>, state: Option<Weak<IpcHandles>>) {
    const MIN_UPDATE_GAP_MS: u64 = 250;

    let Some(state) = state.and_then(|weak| weak.upgrade()) else {
        return;
    };

    let mut updates = state.subscribe_realtime_updates();
    let mut ticker = tokio::time::interval(Duration::from_millis(250));

    let mut last_usb = usb_fingerprint();
    let mut last_streams = stream_fingerprint(&state).await.unwrap_or_default();
    let streams_poll_interval = devices_updates_stream_poll_interval();
    let mut last_streams_poll = Instant::now();
    let mut pending: BTreeSet<DevicesUpdateReason> = BTreeSet::new();
    let min_gap = Duration::from_millis(MIN_UPDATE_GAP_MS);
    let mut last_sent = Instant::now().checked_sub(min_gap).unwrap_or_else(Instant::now);
    let mut saw_receiver = tx.receiver_count() > 0;

    loop {
        let receiver_count = tx.receiver_count();
        saw_receiver |= receiver_count > 0;
        if saw_receiver && receiver_count == 0 {
            break;
        }

        tokio::select! {
            _ = ticker.tick() => {
                let next_usb = usb_fingerprint();
                if next_usb != last_usb {
                    last_usb = next_usb;
                    pending.insert(DevicesUpdateReason::Usb);
                }

                if last_streams_poll.elapsed() >= streams_poll_interval {
                    last_streams_poll = Instant::now();
                    if let Some(next_streams) = stream_fingerprint(&state).await
                        && next_streams != last_streams
                    {
                        last_streams = next_streams;
                        pending.insert(DevicesUpdateReason::Streams);
                    }
                }

                if !pending.is_empty() && last_sent.elapsed() >= min_gap {
                    broadcast_devices_update(&tx, &pending);
                    pending.clear();
                    last_sent = Instant::now();
                }
            }
            update = updates.recv() => {
                match update {
                    Ok(update) => {
                        pending.insert(reason_for_update_kind(update.kind.as_str()));
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        pending.insert(DevicesUpdateReason::Api);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }

                if !pending.is_empty() && last_sent.elapsed() >= min_gap {
                    broadcast_devices_update(&tx, &pending);
                    pending.clear();
                    last_sent = Instant::now();
                }
            }
        }
    }
}

async fn run_telemetry_sampler(tx: broadcast::Sender<Arc<str>>, latest: Arc<StdMutex<Option<Arc<str>>>>, state: Option<Weak<IpcHandles>>, collector: Arc<StdMutex<SystemCollector>>) {
    let sample_interval = std::env::var("HELIOS_TELEMETRY_SAMPLE_INTERVAL_MS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .map(Duration::from_millis)
        .map(|duration| duration.clamp(Duration::from_millis(250), Duration::from_millis(10_000)))
        .unwrap_or_else(|| Duration::from_millis(1_000));

    let last_power: Arc<StdMutex<Option<PowerTelemetry>>> = Arc::new(StdMutex::new(None));

    let power_state = state.clone();
    let power_task = {
        let tx = tx.clone();
        let last_power = last_power.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(1));

            let mut saw_receiver = tx.receiver_count() > 0;
            loop {
                ticker.tick().await;

                let receiver_count = tx.receiver_count();
                saw_receiver |= receiver_count > 0;
                if saw_receiver && receiver_count == 0 {
                    break;
                }

                if let Some(sampled) = sample_power_from_peripherals(power_state.as_ref()).await
                    && let Ok(mut guard) = last_power.lock()
                {
                    *guard = Some(sampled);
                }
            }
        })
    };

    let (sys_done_tx, sys_done_rx) = oneshot::channel();
    let fallback_tx = tx.clone();
    let fallback_latest = latest.clone();
    let fallback_state = state.clone();
    let fallback_last_power = last_power.clone();
    let fallback_collector = collector.clone();
    match spawn_api_sampler_thread("helios-api-telemetry", move || {
        run_telemetry_sys_sampler(tx, latest, state, last_power, collector, sample_interval);
        let _ = sys_done_tx.send(());
    }) {
        Ok(_join) => {
            let _ = sys_done_rx.await;
        }
        Err(err) => {
            warn!(
                error = %err,
                "failed to spawn dedicated telemetry sampler thread; falling back to Tokio blocking pool"
            );
            let _ = tokio::task::spawn_blocking(move || run_telemetry_sys_sampler(fallback_tx, fallback_latest, fallback_state, fallback_last_power, fallback_collector, sample_interval)).await;
        }
    }
    power_task.abort();
}

fn run_telemetry_sys_sampler(
    tx: broadcast::Sender<Arc<str>>,
    latest: Arc<StdMutex<Option<Arc<str>>>>,
    state: Option<Weak<IpcHandles>>,
    last_power: Arc<StdMutex<Option<PowerTelemetry>>>,
    collector: Arc<StdMutex<SystemCollector>>,
    sample_interval: Duration,
) {
    let soft_budget = (sample_interval / 5).max(Duration::from_millis(50));
    let hard_budget = sample_interval + Duration::from_millis(50);

    let mut last_warn: Option<Instant> = None;
    let mut suppressed_warns: u64 = 0;

    let mut saw_receiver = tx.receiver_count() > 0;
    let mut next_tick = Instant::now();

    loop {
        let receiver_count = tx.receiver_count();
        saw_receiver |= receiver_count > 0;
        if saw_receiver && receiver_count == 0 {
            break;
        }

        let now = Instant::now();
        if now < next_tick {
            std::thread::sleep(next_tick - now);
        }
        next_tick = Instant::now() + sample_interval;

        let sampling_started = Instant::now();
        let mut sample = collector.lock().expect("system collector poisoned").collect_telemetry();
        let sampling_elapsed = sampling_started.elapsed();

        if let Some(state) = state.as_ref().and_then(|weak| weak.upgrade()) {
            sample.engine = Some(EngineTelemetry { connected: state.engine.is_connected(), last_disconnect_ms: state.engine.last_disconnect_ms() });
        }

        if let Ok(guard) = last_power.lock() {
            sample.power = guard.clone();
        }

        if sampling_elapsed >= soft_budget {
            let now = Instant::now();
            let should_warn = last_warn.map(|time| now.duration_since(time) >= Duration::from_secs(10)).unwrap_or(true);
            if should_warn {
                if sampling_elapsed >= hard_budget {
                    warn!(
                        elapsed_ms = sampling_elapsed.as_millis(),
                        soft_budget_ms = soft_budget.as_millis(),
                        interval_ms = sample_interval.as_millis(),
                        suppressed = suppressed_warns,
                        "telemetry sampling exceeded budget"
                    );
                } else {
                    debug!(
                        elapsed_ms = sampling_elapsed.as_millis(),
                        soft_budget_ms = soft_budget.as_millis(),
                        interval_ms = sample_interval.as_millis(),
                        suppressed = suppressed_warns,
                        "telemetry sampling exceeded soft budget"
                    );
                }
                last_warn = Some(now);
                suppressed_warns = 0;
            } else {
                suppressed_warns = suppressed_warns.saturating_add(1);
            }
        }

        let payload = Arc::<str>::from(serde_json::to_string(&sample).unwrap_or_default());
        if let Ok(mut guard) = latest.lock() {
            *guard = Some(payload.clone());
        }
        let _ = tx.send(payload);
    }
}

fn run_processes_sampler(tx: broadcast::Sender<Arc<SharedProcessesSnapshot>>, latest: Arc<StdMutex<Option<Arc<SharedProcessesSnapshot>>>>) {
    let sample_interval = std::env::var("HELIOS_PROCESSES_SAMPLE_INTERVAL_MS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .map(Duration::from_millis)
        .map(|duration| duration.clamp(Duration::from_millis(250), Duration::from_millis(10_000)))
        .unwrap_or_else(|| Duration::from_millis(250));

    let mut sys = System::new_all();
    sys.refresh_all();

    let mut saw_receiver = tx.receiver_count() > 0;
    let mut next_tick = StdDuration::from_millis(0);
    loop {
        let receiver_count = tx.receiver_count();
        saw_receiver |= receiver_count > 0;
        if saw_receiver && receiver_count == 0 {
            break;
        }

        if next_tick > StdDuration::from_millis(0) {
            std::thread::sleep(next_tick);
        }
        let sampling_started = std::time::Instant::now();

        sys.refresh_processes(ProcessesToUpdate::All, true);
        sys.refresh_memory();

        let snapshot = Arc::new(build_shared_process_snapshot(&sys));
        if let Ok(mut guard) = latest.lock() {
            *guard = Some(snapshot.clone());
        }
        let _ = tx.send(snapshot);

        next_tick = sample_interval.saturating_sub(sampling_started.elapsed());
    }
}

fn build_shared_process_snapshot(sys: &System) -> SharedProcessesSnapshot {
    let cpu_count = sys.cpus().len().max(1) as f32;
    let mut processes: Vec<ProcessSample> = sys
        .processes()
        .iter()
        .map(|(pid, process)| {
            let pid_u32 = pid.as_u32();
            let name = process.name().to_string_lossy().to_string();
            let cpu_percent = (process.cpu_usage() / cpu_count).max(0.0);
            let memory_bytes = process.memory();
            let virtual_memory_bytes = process.virtual_memory();
            let status = Some(format!("{:?}", process.status()));
            let cmd = process.cmd().iter().map(|part| part.to_string_lossy().to_string()).collect();
            ProcessSample { pid: pid_u32, name, cpu_percent, memory_bytes, virtual_memory_bytes, status, cmd }
        })
        .collect();

    processes.sort_by(|a, b| b.cpu_percent.total_cmp(&a.cpu_percent).then_with(|| b.memory_bytes.cmp(&a.memory_bytes)));
    processes.truncate(2_000);

    SharedProcessesSnapshot {
        timestamp_ms: chrono::Utc::now().timestamp_millis().max(0) as u64,
        total_memory_bytes: sys.total_memory(),
        used_memory_bytes: sys.used_memory(),
        processes: Arc::<[ProcessSample]>::from(processes),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reason_for_update_kind_maps_known_and_unknown_topics() {
        assert_eq!(super::reason_for_update_kind("streams"), DevicesUpdateReason::Streams);
        assert_eq!(super::reason_for_update_kind("pipelines"), DevicesUpdateReason::Pipelines);
        assert_eq!(super::reason_for_update_kind("mystery"), DevicesUpdateReason::Api);
    }

    #[tokio::test]
    async fn broadcast_devices_update_emits_sorted_reasons() {
        let (tx, mut rx) = broadcast::channel(4);
        let mut reasons = BTreeSet::new();
        reasons.insert(DevicesUpdateReason::Streams);
        reasons.insert(DevicesUpdateReason::Api);
        reasons.insert(DevicesUpdateReason::Usb);

        super::broadcast_devices_update(&tx, &reasons);

        let update = rx.recv().await.expect("update should be broadcast");
        assert_eq!(update.reasons, vec![DevicesUpdateReason::Api, DevicesUpdateReason::Usb, DevicesUpdateReason::Streams,]);
        assert!(update.timestamp_ms > 0);
    }
}
