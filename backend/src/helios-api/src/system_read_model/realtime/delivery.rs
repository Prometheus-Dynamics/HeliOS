use std::sync::{Arc, Mutex as StdMutex, Weak};

use tokio::sync::{Mutex, broadcast, oneshot};
use tokio::task::JoinHandle;
use tracing::warn;

use crate::api_observability::{ApiRealtimeDiagnostics, ApiRealtimeMetrics, RuntimeBroadcastCounters};
use crate::ipc::IpcHandles;
use crate::system_read_model::hub::{bind_sync_state, clone_sync_latest, ensure_task};

use super::super::collector::SystemCollector;
use super::super::state::SystemReadModelState;
use super::models::{SharedDevicesUpdate, SharedProcessesSnapshot};
use super::samplers::{run_devices_updates_sampler, run_processes_sampler, run_telemetry_sampler};

pub(in crate::system_read_model) struct ProcessesHub {
    tx: broadcast::Sender<Arc<SharedProcessesSnapshot>>,
    latest: Arc<StdMutex<Option<Arc<SharedProcessesSnapshot>>>>,
    task: Mutex<Option<JoinHandle<()>>>,
    metrics: Arc<RuntimeBroadcastCounters>,
}

pub(in crate::system_read_model) struct DevicesUpdatesHub {
    tx: broadcast::Sender<Arc<SharedDevicesUpdate>>,
    task: Mutex<Option<JoinHandle<()>>>,
    state: StdMutex<Option<Weak<IpcHandles>>>,
    metrics: Arc<RuntimeBroadcastCounters>,
}

pub(in crate::system_read_model) struct TelemetryHub {
    tx: broadcast::Sender<Arc<str>>,
    latest: Arc<StdMutex<Option<Arc<str>>>>,
    task: Mutex<Option<JoinHandle<()>>>,
    state: StdMutex<Option<Weak<IpcHandles>>>,
    metrics: Arc<RuntimeBroadcastCounters>,
}

impl TelemetryHub {
    pub(in crate::system_read_model) fn new() -> Self {
        let (tx, _) = broadcast::channel(32);
        Self { tx, latest: Arc::new(StdMutex::new(None)), task: Mutex::new(None), state: StdMutex::new(None), metrics: Arc::new(RuntimeBroadcastCounters::default()) }
    }

    pub(in crate::system_read_model) fn set_state(&self, state: &Arc<IpcHandles>) {
        bind_sync_state(&self.state, state, "telemetry state poisoned");
    }

    pub(in crate::system_read_model) async fn subscribe(&self, collector: Arc<StdMutex<SystemCollector>>) -> (broadcast::Receiver<Arc<str>>, Option<Arc<str>>) {
        self.ensure_task(collector).await;
        let latest = clone_sync_latest(&self.latest, "telemetry latest poisoned");
        (self.tx.subscribe(), latest)
    }

    async fn ensure_task(&self, collector: Arc<StdMutex<SystemCollector>>) {
        ensure_task(&self.task, || {
            let tx = self.tx.clone();
            let latest = self.latest.clone();
            let state = self.state.lock().expect("telemetry state poisoned").clone();
            let metrics = self.metrics.clone();
            tokio::spawn(run_telemetry_sampler(tx, latest, state, collector, metrics))
        })
        .await;
    }

    pub(in crate::system_read_model) fn subscriber_count(&self) -> u64 {
        self.tx.receiver_count() as u64
    }

    pub(in crate::system_read_model) fn snapshot(&self) -> crate::api_observability::RuntimeBroadcastSnapshot {
        self.metrics.snapshot(self.subscriber_count())
    }
}

impl DevicesUpdatesHub {
    pub(in crate::system_read_model) fn new() -> Self {
        let (tx, _) = broadcast::channel(64);
        Self { tx, task: Mutex::new(None), state: StdMutex::new(None), metrics: Arc::new(RuntimeBroadcastCounters::default()) }
    }

    pub(in crate::system_read_model) fn set_state(&self, state: &Arc<IpcHandles>) {
        bind_sync_state(&self.state, state, "devices updates state poisoned");
    }

    pub(in crate::system_read_model) async fn subscribe(&self) -> broadcast::Receiver<Arc<SharedDevicesUpdate>> {
        self.ensure_task().await;
        self.tx.subscribe()
    }

    async fn ensure_task(&self) {
        ensure_task(&self.task, || {
            let tx = self.tx.clone();
            let state = self.state.lock().expect("devices updates state poisoned").clone();
            let metrics = self.metrics.clone();
            tokio::spawn(run_devices_updates_sampler(tx, state, metrics))
        })
        .await;
    }

    pub(in crate::system_read_model) fn subscriber_count(&self) -> u64 {
        self.tx.receiver_count() as u64
    }

    pub(in crate::system_read_model) fn snapshot(&self) -> crate::api_observability::RuntimeBroadcastSnapshot {
        self.metrics.snapshot(self.subscriber_count())
    }
}

impl ProcessesHub {
    pub(in crate::system_read_model) fn new() -> Self {
        let (tx, _) = broadcast::channel(64);
        Self { tx, latest: Arc::new(StdMutex::new(None)), task: Mutex::new(None), metrics: Arc::new(RuntimeBroadcastCounters::default()) }
    }

    pub(in crate::system_read_model) async fn subscribe(&self) -> (broadcast::Receiver<Arc<SharedProcessesSnapshot>>, Option<Arc<SharedProcessesSnapshot>>) {
        self.ensure_task().await;
        let latest = clone_sync_latest(&self.latest, "process latest poisoned");
        (self.tx.subscribe(), latest)
    }

    async fn ensure_task(&self) {
        ensure_task(&self.task, || {
            let tx = self.tx.clone();
            let latest = self.latest.clone();
            let metrics = self.metrics.clone();
            tokio::spawn(async move {
                let (done_tx, done_rx) = oneshot::channel();
                let fallback_tx = tx.clone();
                let fallback_latest = latest.clone();
                let fallback_metrics = metrics.clone();
                match super::super::config::spawn_api_sampler_thread("helios-api-procs", move || {
                    run_processes_sampler(tx, latest, metrics);
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
                        let _ = tokio::task::spawn_blocking(move || run_processes_sampler(fallback_tx, fallback_latest, fallback_metrics)).await;
                    }
                }
            })
        })
        .await;
    }

    pub(in crate::system_read_model) fn subscriber_count(&self) -> u64 {
        self.tx.receiver_count() as u64
    }

    pub(in crate::system_read_model) fn snapshot(&self) -> crate::api_observability::RuntimeBroadcastSnapshot {
        self.metrics.snapshot(self.subscriber_count())
    }
}

impl SystemReadModelState {
    pub async fn realtime_metrics(&self) -> ApiRealtimeMetrics {
        let (stream_metrics_topics, stream_metrics_subscribers) = self.stream_metrics_hub.stats().await;
        let (stream_outputs_topics, stream_outputs_subscribers) = self.stream_outputs_hub.stats().await;
        ApiRealtimeMetrics {
            telemetry_subscribers: self.telemetry_hub.subscriber_count(),
            process_subscribers: self.processes_hub.subscriber_count(),
            device_update_subscribers: self.devices_updates_hub.subscriber_count(),
            stream_metrics_topics,
            stream_metrics_subscribers,
            stream_outputs_topics,
            stream_outputs_subscribers,
        }
    }

    pub async fn realtime_diagnostics(&self) -> ApiRealtimeDiagnostics {
        ApiRealtimeDiagnostics {
            telemetry: self.telemetry_hub.snapshot(),
            processes: self.processes_hub.snapshot(),
            device_updates: self.devices_updates_hub.snapshot(),
            stream_metrics: self.stream_metrics_hub.snapshot().await,
            stream_outputs: self.stream_outputs_hub.snapshot().await,
        }
    }

    pub fn bind_telemetry_state(&self, state: &crate::http::AppState) {
        self.telemetry_hub.set_state(state.ipc());
    }

    pub async fn subscribe_telemetry_payloads(&self) -> (broadcast::Receiver<Arc<str>>, Option<Arc<str>>) {
        self.telemetry_hub.subscribe(self.telemetry_collector.clone()).await
    }

    pub fn bind_devices_updates_state(&self, state: &crate::http::AppState) {
        self.devices_updates_hub.set_state(state.ipc());
    }

    pub async fn subscribe_devices_updates(&self) -> broadcast::Receiver<Arc<SharedDevicesUpdate>> {
        self.devices_updates_hub.subscribe().await
    }

    pub async fn subscribe_process_snapshots(&self) -> (broadcast::Receiver<Arc<SharedProcessesSnapshot>>, Option<Arc<SharedProcessesSnapshot>>) {
        self.processes_hub.subscribe().await
    }
}
