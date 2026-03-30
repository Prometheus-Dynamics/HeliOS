use crate::api_observability::{ApiCacheMetric, ApiRealtimeMetrics, CacheMetricCounters};
use crate::http::device::metrics::{CpuCoreMetrics, DeviceHealthIssue, DeviceMetrics, DiskMetrics, ProcessMappingMetrics, ProcessMemoryMetrics, TempReading};
use crate::http::storage;
use crate::http::streams::util::list_streams_timeout;
use crate::ipc::IpcHandles;
use crate::logs;
use crate::logs::LogSource;
use crate::ws::device::{
    CpuCoreSample, CpuTelemetry, CpuThrottleStatus, DiskPartitionTelemetry, DiskTelemetry, EngineTelemetry, GpuMemorySample, GpuTelemetry, MemoryTelemetry, NetworkInterfaceSample, NetworkSample,
    PowerTelemetry, TelemetrySample,
};
use helios_engine::ipc::{EngineEvent, GraphOutputPortDescriptor, StreamState};
use helios_engine::stream::StreamMetrics;
use nix::sys::statvfs::statvfs;
use schemars::JsonSchema;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex, OnceLock, Weak};
use std::time::Duration as StdDuration;
use sysinfo::{Components, Disks, Networks, ProcessesToUpdate, System};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, RwLock, broadcast};
use tokio::task::JoinHandle;
use tokio::time::{Duration, Instant};
use tracing::{debug, warn};

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

#[derive(Debug, Clone)]
pub struct SharedStreamMetricsSnapshot {
    pub stream_id: uuid::Uuid,
    pub metrics: StreamMetrics,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone)]
pub struct SharedStreamOutputsPortsSnapshot {
    pub outputs: Vec<GraphOutputPortDescriptor>,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone)]
pub struct SharedStreamOutputSample {
    pub port: String,
    pub value: Option<serde_json::Value>,
    pub error: Option<String>,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone)]
pub enum SharedStreamOutputsEvent {
    Ports(SharedStreamOutputsPortsSnapshot),
    Sample(SharedStreamOutputSample),
}

#[derive(Clone)]
struct MetricsCacheEntry {
    fetched_at: Instant,
    revision: u64,
    body: DeviceMetrics,
}

#[derive(Clone)]
struct ProcessMetricsCacheEntry {
    fetched_at: Instant,
    metrics: Vec<ProcessMemoryMetrics>,
}

struct SystemCollector {
    sys: System,
    disks: Disks,
    components: Components,
    networks: Networks,
    net_snapshot: Option<NetSnapshot>,
    process_metrics_cache: Option<ProcessMetricsCacheEntry>,
}

impl SystemCollector {
    fn new() -> Self {
        let sys = System::new_all();
        let mut networks = Networks::new_with_refreshed_list();
        let disks = Disks::new_with_refreshed_list();
        let components = Components::new_with_refreshed_list();

        networks.refresh(false);
        let mut collector = Self { sys, disks, components, networks, net_snapshot: None, process_metrics_cache: None };
        collector.update_net_snapshot();

        // Prime CPU stats so the first incremental sample is meaningful.
        collector.sys.refresh_cpu_all();

        collector
    }

    fn collect_metrics(&mut self) -> DeviceMetrics {
        // Keep the sysinfo collector hot so incremental CPU samples stay meaningful.
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        self.sys.refresh_processes(ProcessesToUpdate::All, true);
        self.disks.refresh(false);
        self.components.refresh(false);

        let cpu_avg_pct = if self.sys.cpus().is_empty() { 0.0 } else { self.sys.global_cpu_usage() };
        let cpu_freq_mhz = self.sys.cpus().first().map(|cpu| cpu.frequency()).unwrap_or(0);
        let cpus = self.sys.cpus().iter().enumerate().map(|(idx, cpu)| CpuCoreMetrics { id: idx, name: cpu.name().to_string(), pct: cpu.cpu_usage(), freq_mhz: cpu.frequency() }).collect();
        let disks = collect_disk_metrics(&self.disks);
        let processes = self.collect_cached_process_memory_metrics();
        let temps = self.components.iter().filter_map(|component| component.temperature().map(|temperature_c| TempReading { label: component.label().to_string(), temperature_c })).collect();
        let storage_health = storage::probe_storage_health();
        let issues = storage_health.issues.into_iter().map(|issue| DeviceHealthIssue { code: issue.code.to_string(), description: issue.description }).collect::<Vec<_>>();
        let status = if issues.is_empty() { "healthy" } else { "degraded" }.to_string();

        DeviceMetrics {
            status,
            issues,
            cpu_avg_pct,
            cpu_freq_mhz,
            cpus,
            mem_total_bytes: self.sys.total_memory(),
            mem_used_bytes: self.sys.used_memory(),
            swap_total_bytes: self.sys.total_swap(),
            swap_used_bytes: self.sys.used_swap(),
            disks,
            processes,
            temps,
            api: None,
        }
    }

    fn collect_cached_process_memory_metrics(&mut self) -> Vec<ProcessMemoryMetrics> {
        let ttl = process_metrics_cache_ttl();
        if ttl > Duration::from_millis(0)
            && let Some(entry) = self.process_metrics_cache.as_ref()
            && entry.fetched_at.elapsed() < ttl
        {
            return entry.metrics.clone();
        }

        let metrics = collect_process_memory_metrics(&self.sys);
        if ttl > Duration::from_millis(0) {
            self.process_metrics_cache = Some(ProcessMetricsCacheEntry { fetched_at: Instant::now(), metrics: metrics.clone() });
        } else {
            self.process_metrics_cache = None;
        }
        metrics
    }

    fn collect_telemetry(&mut self) -> TelemetrySample {
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        self.networks.refresh(true);
        self.disks.refresh(true);
        self.components.refresh(false);

        let cpu_usage = self.sys.global_cpu_usage();
        let cores = self
            .sys
            .cpus()
            .iter()
            .enumerate()
            .map(|(idx, cpu)| CpuCoreSample { id: idx, usage_percent: cpu.cpu_usage(), frequency_mhz: Some(cpu.frequency()), label: Some(cpu.name().to_string()) })
            .collect();
        let total_mem = self.sys.total_memory();
        let used_mem = self.sys.used_memory();
        let partitions = collect_disk_partitions(&self.disks);
        let data_root = storage::data_root_path();
        let (disk_total, disk_used, disk_free) = filesystem_usage_for_path(&data_root)
            .or_else(|| {
                select_disk_for_path(&self.disks, &data_root).map(|disk| {
                    let total = disk.total_space();
                    let free = disk.available_space();
                    (total, total.saturating_sub(free), free)
                })
            })
            .unwrap_or_else(|| {
                let total = partitions.iter().map(|partition| partition.total_bytes).sum();
                let used = partitions.iter().map(|partition| partition.used_bytes).sum();
                let free = partitions.iter().map(|partition| partition.free_bytes).sum();
                (total, used, free)
            });

        TelemetrySample {
            timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
            engine: None,
            cpu: CpuTelemetry { usage_percent: cpu_usage, temperature_c: read_cpu_temperature_c(), throttle: read_cpu_throttle_status(), cores },
            memory: MemoryTelemetry { total_bytes: total_mem, used_bytes: used_mem, free_bytes: total_mem.saturating_sub(used_mem) },
            gpu: sample_gpu(),
            disk: Some(DiskTelemetry { total_bytes: disk_total, used_bytes: disk_used, free_bytes: disk_free }),
            disks: partitions,
            power: None,
            network: self.build_network_sample(),
        }
    }

    fn update_net_snapshot(&mut self) {
        let mut totals = BTreeMap::new();
        for (name, data) in self.networks.iter() {
            totals.insert(name.to_string(), (data.total_received(), data.total_transmitted()));
        }
        self.net_snapshot = Some(NetSnapshot { at: Instant::now(), totals });
    }

    fn build_network_sample(&mut self) -> Option<NetworkSample> {
        let prev = self.net_snapshot.clone();
        let mut totals = BTreeMap::new();
        let mut interfaces = Vec::new();
        let snapshot_time = prev.as_ref().map(|snap| snap.at);

        for (name, data) in self.networks.iter() {
            let rx = data.total_received();
            let tx = data.total_transmitted();
            totals.insert(name.to_string(), (rx, tx));
            let prev_vals = prev.as_ref().and_then(|snap| snap.totals.get(name)).copied();
            let (delta_rx, delta_tx, per_sec_rx, per_sec_tx) = compute_net_deltas(prev_vals, rx, tx, snapshot_time);
            interfaces.push(NetworkInterfaceSample {
                name: name.to_string(),
                rx_bytes: delta_rx,
                tx_bytes: delta_tx,
                total_rx_bytes: rx,
                total_tx_bytes: tx,
                rx_bytes_per_sec: per_sec_rx,
                tx_bytes_per_sec: per_sec_tx,
                mac: None,
            });
        }

        let total_rx = totals.values().map(|(rx, _)| *rx).sum();
        let total_tx = totals.values().map(|(_, tx)| *tx).sum();
        let prev_totals = prev.as_ref().map(|snap| {
            let rx_sum: u64 = snap.totals.values().map(|(rx, _)| *rx).sum();
            let tx_sum: u64 = snap.totals.values().map(|(_, tx)| *tx).sum();
            (rx_sum, tx_sum)
        });
        let (delta_rx, delta_tx, per_sec_rx, per_sec_tx) = compute_net_deltas(prev_totals, total_rx, total_tx, snapshot_time);

        self.update_net_snapshot();

        if total_rx == 0 && total_tx == 0 && interfaces.is_empty() {
            return None;
        }

        Some(NetworkSample { rx_bytes: delta_rx, tx_bytes: delta_tx, total_rx_bytes: total_rx, total_tx_bytes: total_tx, rx_bytes_per_sec: per_sec_rx, tx_bytes_per_sec: per_sec_tx, interfaces })
    }
}

#[derive(Clone)]
struct LogSourcesCacheEntry {
    fetched_at: Instant,
    revision: u64,
    payload: Vec<LogSource>,
}

pub struct SystemReadModelState {
    processes_hub: ProcessesHub,
    devices_updates_hub: DevicesUpdatesHub,
    telemetry_hub: TelemetryHub,
    stream_metrics_hub: StreamMetricsHub,
    stream_outputs_hub: StreamOutputsHub,
    metrics_cache: RwLock<Option<MetricsCacheEntry>>,
    metrics_refresh_lock: tokio::sync::Mutex<()>,
    metrics_stats: CacheMetricCounters,
    telemetry_collector: Arc<StdMutex<SystemCollector>>,
    metrics_collector: Arc<StdMutex<SystemCollector>>,
    log_sources_cache: RwLock<Option<LogSourcesCacheEntry>>,
    log_sources_refresh_lock: tokio::sync::Mutex<()>,
    log_sources_stats: CacheMetricCounters,
}

impl Default for SystemReadModelState {
    fn default() -> Self {
        Self {
            processes_hub: ProcessesHub::new(),
            devices_updates_hub: DevicesUpdatesHub::new(),
            telemetry_hub: TelemetryHub::new(),
            stream_metrics_hub: StreamMetricsHub::new(),
            stream_outputs_hub: StreamOutputsHub::new(),
            metrics_cache: RwLock::new(None),
            metrics_refresh_lock: tokio::sync::Mutex::new(()),
            metrics_stats: CacheMetricCounters::default(),
            telemetry_collector: Arc::new(StdMutex::new(SystemCollector::new())),
            metrics_collector: Arc::new(StdMutex::new(SystemCollector::new())),
            log_sources_cache: RwLock::new(None),
            log_sources_refresh_lock: tokio::sync::Mutex::new(()),
            log_sources_stats: CacheMetricCounters::default(),
        }
    }
}

struct ProcessesHub {
    tx: broadcast::Sender<Arc<SharedProcessesSnapshot>>,
    latest: Arc<StdMutex<Option<Arc<SharedProcessesSnapshot>>>>,
    task: Mutex<Option<JoinHandle<()>>>,
}

struct DevicesUpdatesHub {
    tx: broadcast::Sender<Arc<SharedDevicesUpdate>>,
    task: Mutex<Option<JoinHandle<()>>>,
    state: StdMutex<Option<Weak<IpcHandles>>>,
}

struct TelemetryHub {
    tx: broadcast::Sender<Arc<str>>,
    latest: Arc<StdMutex<Option<Arc<str>>>>,
    task: Mutex<Option<JoinHandle<()>>>,
    state: StdMutex<Option<Weak<IpcHandles>>>,
}

struct StreamMetricsTopic {
    tx: broadcast::Sender<Arc<SharedStreamMetricsSnapshot>>,
    latest: Arc<StdMutex<Option<Arc<SharedStreamMetricsSnapshot>>>>,
}

struct StreamMetricsHub {
    topics: Arc<StdMutex<BTreeMap<uuid::Uuid, Arc<StreamMetricsTopic>>>>,
    task: Mutex<Option<JoinHandle<()>>>,
    state: StdMutex<Option<Weak<IpcHandles>>>,
}

#[derive(Debug, Clone)]
struct StreamOutputsClientConfig {
    ports: Vec<String>,
    sample_interval: Duration,
    ports_interval: Duration,
}

struct StreamOutputsTopic {
    tx: broadcast::Sender<Arc<SharedStreamOutputsEvent>>,
    latest_ports: Arc<StdMutex<Option<Arc<SharedStreamOutputsPortsSnapshot>>>>,
    clients: Arc<Mutex<BTreeMap<uuid::Uuid, StreamOutputsClientConfig>>>,
    task: Mutex<Option<JoinHandle<()>>>,
}

struct StreamOutputsHub {
    topics: Arc<StdMutex<BTreeMap<uuid::Uuid, Arc<StreamOutputsTopic>>>>,
    state: StdMutex<Option<Weak<IpcHandles>>>,
}

impl TelemetryHub {
    fn new() -> Self {
        let (tx, _) = broadcast::channel(32);
        Self { tx, latest: Arc::new(StdMutex::new(None)), task: Mutex::new(None), state: StdMutex::new(None) }
    }

    fn set_state(&self, state: &Arc<IpcHandles>) {
        let mut guard = self.state.lock().expect("telemetry state poisoned");
        if guard.as_ref().and_then(|weak| weak.upgrade()).is_none() {
            *guard = Some(Arc::downgrade(state));
        }
    }

    async fn subscribe(&self, collector: Arc<StdMutex<SystemCollector>>) -> (broadcast::Receiver<Arc<str>>, Option<Arc<str>>) {
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

    fn subscriber_count(&self) -> u64 {
        self.tx.receiver_count() as u64
    }
}

impl StreamMetricsTopic {
    fn new() -> Self {
        let (tx, _) = broadcast::channel(32);
        Self { tx, latest: Arc::new(StdMutex::new(None)) }
    }
}

impl StreamOutputsTopic {
    fn new() -> Self {
        let (tx, _) = broadcast::channel(64);
        Self { tx, latest_ports: Arc::new(StdMutex::new(None)), clients: Arc::new(Mutex::new(BTreeMap::new())), task: Mutex::new(None) }
    }

    async fn ensure_task(&self, stream_id: uuid::Uuid, state: Option<Weak<IpcHandles>>) {
        let mut guard = self.task.lock().await;
        let needs_spawn = guard.as_ref().map(|handle| handle.is_finished()).unwrap_or(true);
        if needs_spawn {
            let tx = self.tx.clone();
            let latest_ports = self.latest_ports.clone();
            let clients = self.clients.clone();
            *guard = Some(tokio::spawn(run_stream_outputs_sampler(stream_id, tx, latest_ports, clients, state)));
        }
    }

    async fn prime_ports(&self, stream_id: uuid::Uuid, state: Option<Weak<IpcHandles>>) -> Result<Arc<SharedStreamOutputsPortsSnapshot>, String> {
        if let Some(snapshot) = self.latest_ports.lock().ok().and_then(|guard| guard.clone()) {
            return Ok(snapshot);
        }

        let Some(state) = state.and_then(|weak| weak.upgrade()) else {
            return Err("engine unavailable".into());
        };

        let snapshot = Arc::new(fetch_stream_outputs_ports(&state, stream_id).await?);
        if let Ok(mut guard) = self.latest_ports.lock()
            && guard.is_none()
        {
            *guard = Some(snapshot.clone());
        }
        let _ = self.tx.send(Arc::new(SharedStreamOutputsEvent::Ports((*snapshot).clone())));
        Ok(snapshot)
    }
}

impl StreamMetricsHub {
    fn new() -> Self {
        Self { topics: Arc::new(StdMutex::new(BTreeMap::new())), task: Mutex::new(None), state: StdMutex::new(None) }
    }

    fn set_state(&self, state: &Arc<IpcHandles>) {
        let mut guard = self.state.lock().expect("stream metrics state poisoned");
        if guard.as_ref().and_then(|weak| weak.upgrade()).is_none() {
            *guard = Some(Arc::downgrade(state));
        }
    }

    async fn subscribe(&self, stream_id: uuid::Uuid) -> Result<(broadcast::Receiver<Arc<SharedStreamMetricsSnapshot>>, Option<Arc<SharedStreamMetricsSnapshot>>), String> {
        self.ensure_task().await;
        let topic = self.topic(stream_id);
        self.prime_topic(stream_id, &topic).await?;
        let latest = topic.latest.lock().ok().and_then(|guard| guard.clone());
        Ok((topic.tx.subscribe(), latest))
    }

    async fn ensure_task(&self) {
        let mut guard = self.task.lock().await;
        let needs_spawn = guard.as_ref().map(|handle| handle.is_finished()).unwrap_or(true);
        if needs_spawn {
            let topics = self.topics.clone();
            let state = self.state.lock().expect("stream metrics state poisoned").clone();
            *guard = Some(tokio::spawn(run_stream_metrics_sampler(topics, state)));
        }
    }

    fn topic(&self, stream_id: uuid::Uuid) -> Arc<StreamMetricsTopic> {
        let mut guard = self.topics.lock().expect("stream metrics topics poisoned");
        guard.entry(stream_id).or_insert_with(|| Arc::new(StreamMetricsTopic::new())).clone()
    }

    fn stats(&self) -> (u64, u64) {
        let Ok(guard) = self.topics.lock() else {
            return (0, 0);
        };
        let topics = guard.len() as u64;
        let subscribers = guard.values().map(|topic| topic.tx.receiver_count() as u64).sum();
        (topics, subscribers)
    }

    async fn unsubscribe(&self, stream_id: uuid::Uuid) {
        let Some(topic) = self.find_topic(stream_id) else {
            return;
        };
        if topic.tx.receiver_count() > 0 {
            return;
        }
        if let Ok(mut guard) = topic.latest.lock() {
            guard.take();
        }
        if let Ok(mut topics) = self.topics.lock() {
            topics.remove(&stream_id);
        }
    }

    async fn prime_topic(&self, stream_id: uuid::Uuid, topic: &Arc<StreamMetricsTopic>) -> Result<(), String> {
        if topic.latest.lock().ok().and_then(|guard| guard.clone()).is_some() {
            return Ok(());
        }

        let Some(state) = self.state.lock().expect("stream metrics state poisoned").as_ref().and_then(|weak| weak.upgrade()) else {
            return Err("engine unavailable".into());
        };

        let snapshot = match state.engine.get_metrics(stream_id).await {
            Ok(EngineEvent::Metrics { metrics, .. }) => Arc::new(build_stream_metrics_snapshot(stream_id, metrics)),
            Ok(EngineEvent::Nack { reason, .. }) => return Err(reason),
            Ok(_) => return Err("unexpected engine response".into()),
            Err(err) => return Err(err.to_string()),
        };

        if let Ok(mut guard) = topic.latest.lock()
            && guard.is_none()
        {
            *guard = Some(snapshot.clone());
        }
        let _ = topic.tx.send(snapshot);
        Ok(())
    }

    fn find_topic(&self, stream_id: uuid::Uuid) -> Option<Arc<StreamMetricsTopic>> {
        self.topics.lock().ok()?.get(&stream_id).cloned()
    }
}

impl StreamOutputsHub {
    fn new() -> Self {
        Self { topics: Arc::new(StdMutex::new(BTreeMap::new())), state: StdMutex::new(None) }
    }

    fn set_state(&self, state: &Arc<IpcHandles>) {
        let mut guard = self.state.lock().expect("stream outputs state poisoned");
        if guard.as_ref().and_then(|weak| weak.upgrade()).is_none() {
            *guard = Some(Arc::downgrade(state));
        }
    }

    async fn subscribe(
        &self,
        stream_id: uuid::Uuid,
        sample_interval: Duration,
        ports_interval: Duration,
    ) -> Result<(uuid::Uuid, broadcast::Receiver<Arc<SharedStreamOutputsEvent>>, Arc<SharedStreamOutputsPortsSnapshot>), String> {
        let topic = self.topic(stream_id);
        let state = self.state.lock().expect("stream outputs state poisoned").clone();
        topic.ensure_task(stream_id, state.clone()).await;

        let client_id = uuid::Uuid::new_v4();
        topic.clients.lock().await.insert(client_id, StreamOutputsClientConfig { ports: Vec::new(), sample_interval, ports_interval });

        match topic.prime_ports(stream_id, state).await {
            Ok(snapshot) => Ok((client_id, topic.tx.subscribe(), snapshot)),
            Err(err) => {
                topic.clients.lock().await.remove(&client_id);
                Err(err)
            }
        }
    }

    async fn update_client(&self, stream_id: uuid::Uuid, client_id: uuid::Uuid, ports: Vec<String>, sample_interval: Duration) -> Result<(), String> {
        let Some(topic) = self.find_topic(stream_id) else {
            return Err("stream outputs subscription unavailable".into());
        };
        let mut clients = topic.clients.lock().await;
        let Some(client) = clients.get_mut(&client_id) else {
            return Err("stream outputs subscriber missing".into());
        };
        client.ports = ports;
        client.sample_interval = sample_interval;
        Ok(())
    }

    async fn unsubscribe(&self, stream_id: uuid::Uuid, client_id: uuid::Uuid) {
        let Some(topic) = self.find_topic(stream_id) else {
            return;
        };
        let idle = {
            let mut clients = topic.clients.lock().await;
            clients.remove(&client_id);
            clients.is_empty()
        };
        if !idle || topic.tx.receiver_count() > 0 {
            return;
        }

        if let Ok(mut guard) = topic.latest_ports.lock() {
            *guard = None;
        }

        if let Ok(mut topics) = self.topics.lock()
            && topics.get(&stream_id).is_some_and(|current| Arc::ptr_eq(current, &topic))
        {
            topics.remove(&stream_id);
        }
    }

    async fn current_ports(&self, stream_id: uuid::Uuid) -> Result<Arc<SharedStreamOutputsPortsSnapshot>, String> {
        let topic = self.topic(stream_id);
        let state = self.state.lock().expect("stream outputs state poisoned").clone();
        topic.ensure_task(stream_id, state.clone()).await;
        topic.prime_ports(stream_id, state).await
    }

    fn topic(&self, stream_id: uuid::Uuid) -> Arc<StreamOutputsTopic> {
        let mut guard = self.topics.lock().expect("stream outputs topics poisoned");
        guard.entry(stream_id).or_insert_with(|| Arc::new(StreamOutputsTopic::new())).clone()
    }

    fn find_topic(&self, stream_id: uuid::Uuid) -> Option<Arc<StreamOutputsTopic>> {
        self.topics.lock().ok()?.get(&stream_id).cloned()
    }

    async fn stats(&self) -> (u64, u64) {
        let topics = self.topics.lock().ok().map(|guard| guard.values().cloned().collect::<Vec<_>>()).unwrap_or_default();
        let topic_count = topics.len() as u64;
        let mut subscribers = 0_u64;
        for topic in topics {
            subscribers = subscribers.saturating_add(topic.clients.lock().await.len() as u64);
        }
        (topic_count, subscribers)
    }
}

impl DevicesUpdatesHub {
    fn new() -> Self {
        let (tx, _) = broadcast::channel(64);
        Self { tx, task: Mutex::new(None), state: StdMutex::new(None) }
    }

    fn set_state(&self, state: &Arc<IpcHandles>) {
        let mut guard = self.state.lock().expect("devices updates state poisoned");
        if guard.as_ref().and_then(|weak| weak.upgrade()).is_none() {
            *guard = Some(Arc::downgrade(state));
        }
    }

    async fn subscribe(&self) -> broadcast::Receiver<Arc<SharedDevicesUpdate>> {
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

    fn subscriber_count(&self) -> u64 {
        self.tx.receiver_count() as u64
    }
}

impl ProcessesHub {
    fn new() -> Self {
        let (tx, _) = broadcast::channel(64);
        Self { tx, latest: Arc::new(StdMutex::new(None)), task: Mutex::new(None) }
    }

    async fn subscribe(&self) -> (broadcast::Receiver<Arc<SharedProcessesSnapshot>>, Option<Arc<SharedProcessesSnapshot>>) {
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
            *guard = Some(tokio::task::spawn_blocking(move || run_processes_sampler(tx, latest)));
        }
    }

    fn subscriber_count(&self) -> u64 {
        self.tx.receiver_count() as u64
    }
}

fn read_duration_env(var: &str, default_ms: u64, min_ms: u64, max_ms: u64) -> Duration {
    let ms = std::env::var(var).ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(default_ms);
    Duration::from_millis(ms.clamp(min_ms, max_ms))
}

fn metrics_cache_ttl() -> Duration {
    static TTL: OnceLock<Duration> = OnceLock::new();
    *TTL.get_or_init(|| read_duration_env("HELIOS_DEVICE_METRICS_CACHE_MS", 750, 0, 10_000))
}

fn devices_updates_stream_poll_interval() -> Duration {
    static TTL: OnceLock<Duration> = OnceLock::new();
    *TTL.get_or_init(|| read_duration_env("HELIOS_DEVICE_UPDATES_STREAM_POLL_MS", 2_000, 250, 60_000))
}

fn process_metrics_cache_ttl() -> Duration {
    static TTL: OnceLock<Duration> = OnceLock::new();
    *TTL.get_or_init(|| read_duration_env("HELIOS_DEVICE_PROCESS_BREAKDOWN_CACHE_MS", 5_000, 0, 60_000))
}

fn metrics_refresh_timeout() -> Duration {
    static TTL: OnceLock<Duration> = OnceLock::new();
    *TTL.get_or_init(|| read_duration_env("HELIOS_DEVICE_METRICS_TIMEOUT_MS", 2_000, 250, 15_000))
}

fn log_sources_cache_ttl() -> Duration {
    static TTL: OnceLock<Duration> = OnceLock::new();
    *TTL.get_or_init(|| read_duration_env("HELIOS_LOG_SOURCES_CACHE_MS", 5_000, 0, 60_000))
}

fn log_sources_refresh_timeout() -> Duration {
    static TTL: OnceLock<Duration> = OnceLock::new();
    *TTL.get_or_init(|| read_duration_env("HELIOS_LOG_SOURCES_REFRESH_TIMEOUT_MS", 3_000, 500, 15_000))
}

fn build_log_sources() -> Vec<LogSource> {
    let mut sources = logs::default_log_sources();
    sources.extend(logs::discover_file_sources());
    sources
}

impl SystemReadModelState {
    pub fn device_metrics_cache_metrics(&self) -> ApiCacheMetric {
        self.metrics_stats.snapshot()
    }

    pub fn log_sources_cache_metrics(&self) -> ApiCacheMetric {
        self.log_sources_stats.snapshot()
    }

    pub async fn realtime_metrics(&self) -> ApiRealtimeMetrics {
        let (stream_metrics_topics, stream_metrics_subscribers) = self.stream_metrics_hub.stats();
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

    pub async fn load_device_metrics_snapshot(&self) -> Result<(DeviceMetrics, u64), String> {
        let ttl = metrics_cache_ttl();
        if ttl != Duration::from_millis(0)
            && let Some(entry) = self.metrics_cache.read().await.clone()
            && entry.fetched_at.elapsed() < ttl
        {
            self.metrics_stats.record_hit();
            return Ok((entry.body, entry.revision));
        }

        self.metrics_stats.record_miss();
        let _refresh_guard = self.metrics_refresh_lock.lock().await;
        if ttl != Duration::from_millis(0)
            && let Some(entry) = self.metrics_cache.read().await.clone()
            && entry.fetched_at.elapsed() < ttl
        {
            self.metrics_stats.record_hit();
            return Ok((entry.body, entry.revision));
        }

        let stale = self.metrics_cache.read().await.clone();
        let collector = self.metrics_collector.clone();
        let body = match tokio::time::timeout(metrics_refresh_timeout(), tokio::task::spawn_blocking(move || collect_device_metrics(&collector))).await {
            Ok(Ok(body)) => body,
            Ok(Err(err)) => {
                warn!(error = ?err, "device metrics task failed");
                if let Some(entry) = stale {
                    self.metrics_stats.record_stale_fallback();
                    return Ok((entry.body, entry.revision));
                }
                return Err(format!("metrics task failed: {err}"));
            }
            Err(_) => {
                warn!(timeout_ms = metrics_refresh_timeout().as_millis(), "device metrics task timed out");
                if let Some(entry) = stale {
                    self.metrics_stats.record_stale_fallback();
                    return Ok((entry.body, entry.revision));
                }
                return Err("metrics task timed out".into());
            }
        };

        let revision = self.metrics_stats.record_refresh();
        *self.metrics_cache.write().await = Some(MetricsCacheEntry { fetched_at: Instant::now(), revision, body: body.clone() });
        Ok((body, revision))
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

    pub fn bind_stream_metrics_state(&self, state: &crate::http::AppState) {
        self.stream_metrics_hub.set_state(state.ipc());
    }

    pub async fn subscribe_stream_metrics(&self, stream_id: uuid::Uuid) -> Result<(broadcast::Receiver<Arc<SharedStreamMetricsSnapshot>>, Option<Arc<SharedStreamMetricsSnapshot>>), String> {
        self.stream_metrics_hub.subscribe(stream_id).await
    }

    pub async fn unsubscribe_stream_metrics(&self, stream_id: uuid::Uuid) {
        self.stream_metrics_hub.unsubscribe(stream_id).await;
    }

    pub fn bind_stream_outputs_state(&self, state: &crate::http::AppState) {
        self.stream_outputs_hub.set_state(state.ipc());
    }

    pub async fn subscribe_stream_outputs(
        &self,
        stream_id: uuid::Uuid,
        sample_interval: Duration,
        ports_interval: Duration,
    ) -> Result<(uuid::Uuid, broadcast::Receiver<Arc<SharedStreamOutputsEvent>>, Arc<SharedStreamOutputsPortsSnapshot>), String> {
        self.stream_outputs_hub.subscribe(stream_id, sample_interval, ports_interval).await
    }

    pub async fn update_stream_outputs_subscription(&self, stream_id: uuid::Uuid, client_id: uuid::Uuid, ports: Vec<String>, sample_interval: Duration) -> Result<(), String> {
        self.stream_outputs_hub.update_client(stream_id, client_id, ports, sample_interval).await
    }

    pub async fn current_stream_outputs_ports(&self, stream_id: uuid::Uuid) -> Result<Arc<SharedStreamOutputsPortsSnapshot>, String> {
        self.stream_outputs_hub.current_ports(stream_id).await
    }

    pub async fn unsubscribe_stream_outputs(&self, stream_id: uuid::Uuid, client_id: uuid::Uuid) {
        self.stream_outputs_hub.unsubscribe(stream_id, client_id).await;
    }

    pub async fn load_log_sources_snapshot(&self) -> (Vec<LogSource>, u64) {
        let ttl = log_sources_cache_ttl();
        if ttl != Duration::from_millis(0)
            && let Some(entry) = self.log_sources_cache.read().await.clone()
            && entry.fetched_at.elapsed() < ttl
        {
            self.log_sources_stats.record_hit();
            return (entry.payload, entry.revision);
        }

        self.log_sources_stats.record_miss();
        let _refresh_guard = self.log_sources_refresh_lock.lock().await;
        if ttl != Duration::from_millis(0)
            && let Some(entry) = self.log_sources_cache.read().await.clone()
            && entry.fetched_at.elapsed() < ttl
        {
            self.log_sources_stats.record_hit();
            return (entry.payload, entry.revision);
        }

        let stale = self.log_sources_cache.read().await.clone();
        let base = build_log_sources();
        let sources = match tokio::time::timeout(log_sources_refresh_timeout(), logs::hydrate_systemd_statuses(base.clone())).await {
            Ok(hydrated) => hydrated,
            Err(_) => {
                warn!(timeout_ms = log_sources_refresh_timeout().as_millis(), "log source hydration timed out");
                if let Some(entry) = stale {
                    self.log_sources_stats.record_stale_fallback();
                    return (entry.payload, entry.revision);
                }
                base
            }
        };

        let revision = self.log_sources_stats.record_refresh();
        *self.log_sources_cache.write().await = Some(LogSourcesCacheEntry { fetched_at: Instant::now(), revision, payload: sources.clone() });
        (sources, revision)
    }

    pub async fn load_log_sources(&self) -> Vec<LogSource> {
        self.load_log_sources_snapshot().await.0
    }

    pub async fn resolve_log_source(&self, source_id: &str) -> Option<LogSource> {
        self.load_log_sources().await.into_iter().find(|source| source.id == source_id)
    }

    pub async fn read_log_lines(&self, source_id: &str, lines: u64) -> Result<Vec<String>, String> {
        let Some(spec) = self.resolve_log_source(source_id).await else {
            return Err("unknown log source".into());
        };
        let lines = lines.clamp(1, 10_000);

        let output = match spec.kind {
            logs::LogSourceKind::JournalSystem => Command::new("journalctl").args(["-n", &lines.to_string(), "--no-pager", "-o", "short-iso"]).output().await,
            logs::LogSourceKind::JournalUnit => {
                let unit = spec.unit.unwrap_or_default();
                if unit.trim().is_empty() {
                    return Err("invalid unit log source".into());
                }
                Command::new("journalctl").args(["-u", &unit, "-n", &lines.to_string(), "--no-pager", "-o", "short-iso"]).output().await
            }
            logs::LogSourceKind::Dmesg => Command::new("dmesg").args(["--color=never", "--ctime"]).output().await,
            logs::LogSourceKind::File => {
                let path = spec.path.unwrap_or_default();
                if path.trim().is_empty() {
                    return Err("invalid file log source".into());
                }
                Command::new("tail").arg("-n").arg(lines.to_string()).arg(PathBuf::from(path)).output().await
            }
        }
        .map_err(|err| format!("failed to fetch logs: {err}"))?;

        if !output.status.success() {
            return Err(format!("log fetch failed (status {})", output.status));
        }

        let text = String::from_utf8_lossy(&output.stdout);
        Ok(text.lines().map(|line| line.to_string()).collect())
    }

    pub fn spawn_log_download(&self, source: &LogSource, lines: Option<usize>) -> Result<(Child, String), String> {
        let filename = sanitize_log_filename(&source.label);

        let mut cmd = match source.kind {
            logs::LogSourceKind::JournalSystem => {
                let mut cmd = Command::new("journalctl");
                cmd.args(["--no-pager", "-o", "short-iso"]);
                if let Some(lines) = lines {
                    cmd.args(["-n", &lines.to_string()]);
                }
                cmd
            }
            logs::LogSourceKind::JournalUnit => {
                let unit = source.unit.clone().unwrap_or_default();
                if unit.trim().is_empty() {
                    return Err("invalid unit log source".into());
                }
                let mut cmd = Command::new("journalctl");
                cmd.args(["-u", &unit, "--no-pager", "-o", "short-iso"]);
                if let Some(lines) = lines {
                    cmd.args(["-n", &lines.to_string()]);
                }
                cmd
            }
            logs::LogSourceKind::Dmesg => {
                if let Some(lines) = lines {
                    let script = format!("dmesg --color=never --ctime 2>/dev/null | tail -n {}", lines);
                    let mut cmd = Command::new("sh");
                    cmd.args(["-c", &script]);
                    cmd
                } else {
                    let mut cmd = Command::new("dmesg");
                    cmd.args(["--color=never", "--ctime"]);
                    cmd
                }
            }
            logs::LogSourceKind::File => {
                let path = source.path.clone().unwrap_or_default();
                if path.trim().is_empty() {
                    return Err("invalid file log source".into());
                }
                let path = PathBuf::from(path);
                if let Some(lines) = lines {
                    let mut cmd = Command::new("tail");
                    cmd.arg("-n").arg(lines.to_string()).arg(path);
                    cmd
                } else {
                    let mut cmd = Command::new("cat");
                    cmd.arg(path);
                    cmd
                }
            }
        };

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::null());
        cmd.spawn().map(|child| (child, filename)).map_err(|err| format!("failed to spawn log download: {err}"))
    }

    pub fn spawn_log_stream(&self, source: &LogSource, lines: usize, follow: bool) -> Result<Child, String> {
        let follow_flag = if follow { "true" } else { "false" };
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-lc").env("LINES", lines.to_string()).env("FOLLOW", follow_flag).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);

        let script = match source.id.as_str() {
            "journal" => journal_script(None, lines, follow),
            "dmesg" => dmesg_script(lines, follow),
            _ => {
                if let Some(unit) = source.unit.as_deref() {
                    journal_script(Some(unit), lines, follow)
                } else if let Some(path) = source.path.as_deref()
                    && source.id.starts_with("file:")
                {
                    file_script(path, lines, follow)
                } else {
                    return Err("unsupported log source".into());
                }
            }
        };

        cmd.arg(script).spawn().map_err(|err| format!("failed to start log source: {err}"))
    }

    pub async fn subscribe_process_snapshots(&self) -> (broadcast::Receiver<Arc<SharedProcessesSnapshot>>, Option<Arc<SharedProcessesSnapshot>>) {
        self.processes_hub.subscribe().await
    }
}

fn collect_device_metrics(collector: &Arc<StdMutex<SystemCollector>>) -> DeviceMetrics {
    collector.lock().expect("system collector poisoned").collect_metrics()
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
                        && next_streams != last_streams {
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

async fn run_stream_metrics_sampler(topics: Arc<StdMutex<BTreeMap<uuid::Uuid, Arc<StreamMetricsTopic>>>>, state: Option<Weak<IpcHandles>>) {
    let Some(state) = state.and_then(|weak| weak.upgrade()) else {
        return;
    };

    let mut events = state.engine.subscribe_events();
    let mut saw_receiver = stream_metrics_has_receivers(&topics);

    loop {
        let has_receivers = stream_metrics_has_receivers(&topics);
        saw_receiver |= has_receivers;
        if saw_receiver && !has_receivers {
            break;
        }

        match events.recv().await {
            Ok(EngineEvent::Metrics { stream_id, metrics, .. }) | Ok(EngineEvent::MetricsUpdate { stream_id, metrics }) => {
                let Some(topic) = stream_metrics_topic(&topics, stream_id) else {
                    continue;
                };
                let snapshot = Arc::new(build_stream_metrics_snapshot(stream_id, metrics));
                if let Ok(mut guard) = topic.latest.lock() {
                    *guard = Some(snapshot.clone());
                }
                let _ = topic.tx.send(snapshot);
            }
            Ok(_) => {}
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

async fn run_stream_outputs_sampler(
    stream_id: uuid::Uuid,
    tx: broadcast::Sender<Arc<SharedStreamOutputsEvent>>,
    latest_ports: Arc<StdMutex<Option<Arc<SharedStreamOutputsPortsSnapshot>>>>,
    clients: Arc<Mutex<BTreeMap<uuid::Uuid, StreamOutputsClientConfig>>>,
    state: Option<Weak<IpcHandles>>,
) {
    let Some(state) = state.and_then(|weak| weak.upgrade()) else {
        return;
    };

    let mut ticker = tokio::time::interval(Duration::from_millis(100));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let mut last_ports_refresh = Instant::now().checked_sub(Duration::from_secs(30)).unwrap_or_else(Instant::now);
    let mut next_samples = BTreeMap::<String, Instant>::new();
    let mut saw_receiver = tx.receiver_count() > 0;

    loop {
        ticker.tick().await;

        let receiver_count = tx.receiver_count();
        let client_snapshot = clients.lock().await.clone();
        let has_activity = receiver_count > 0 || !client_snapshot.is_empty();
        saw_receiver |= has_activity;
        if saw_receiver && !has_activity {
            break;
        }

        let now = Instant::now();
        let ports_interval = aggregate_stream_outputs_ports_interval(&client_snapshot);
        let active_ports = aggregate_stream_output_ports(&client_snapshot);

        next_samples.retain(|port, _| active_ports.contains_key(port));

        let needs_ports_refresh = latest_ports.lock().ok().and_then(|guard| guard.clone()).is_none() || now.duration_since(last_ports_refresh) >= ports_interval;
        if needs_ports_refresh {
            if let Ok(snapshot) = fetch_stream_outputs_ports(&state, stream_id).await {
                let snapshot = Arc::new(snapshot);
                let changed = latest_ports.lock().ok().and_then(|guard| guard.as_ref().map(|current| current.outputs != snapshot.outputs)).unwrap_or(true);
                if let Ok(mut guard) = latest_ports.lock() {
                    *guard = Some(snapshot.clone());
                }
                if changed {
                    let _ = tx.send(Arc::new(SharedStreamOutputsEvent::Ports((*snapshot).clone())));
                }
            }
            last_ports_refresh = now;
        }

        for (port, interval) in &active_ports {
            let due = next_samples.get(port).copied().unwrap_or_else(|| now.checked_sub(*interval).unwrap_or(now));
            if now < due {
                continue;
            }
            let sample = fetch_stream_output_sample(&state, stream_id, port.clone()).await;
            let _ = tx.send(Arc::new(SharedStreamOutputsEvent::Sample(sample)));
            next_samples.insert(port.clone(), now + *interval);
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

    let sys_task = tokio::task::spawn_blocking(move || {
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
    });

    let _ = sys_task.await;
    power_task.abort();
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

fn build_stream_metrics_snapshot(stream_id: uuid::Uuid, metrics: StreamMetrics) -> SharedStreamMetricsSnapshot {
    SharedStreamMetricsSnapshot { stream_id, metrics, timestamp_ms: chrono::Utc::now().timestamp_millis().max(0) as u64 }
}

async fn fetch_stream_outputs_ports(state: &Arc<IpcHandles>, stream_id: uuid::Uuid) -> Result<SharedStreamOutputsPortsSnapshot, String> {
    match state.engine.list_graph_outputs_event(stream_id).await {
        Ok(EngineEvent::GraphOutputs { outputs, .. }) => Ok(SharedStreamOutputsPortsSnapshot { outputs, timestamp_ms: chrono::Utc::now().timestamp_millis().max(0) as u64 }),
        Ok(EngineEvent::Nack { code, reason, .. }) => Err(format!("engine rejected outputs list: {code:?}: {reason}")),
        Ok(_) => Err("unexpected engine response listing outputs".into()),
        Err(err) => Err(format!("engine error listing outputs: {err}")),
    }
}

async fn fetch_stream_output_sample(state: &Arc<IpcHandles>, stream_id: uuid::Uuid, port: String) -> SharedStreamOutputSample {
    let timestamp_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
    match state.engine.get_graph_output_sample_event(stream_id, port.clone()).await {
        Ok(EngineEvent::GraphOutputSample { value, .. }) => SharedStreamOutputSample { port, value: Some(value.0), error: None, timestamp_ms },
        Ok(EngineEvent::Nack { code, reason, .. }) => SharedStreamOutputSample { port, value: None, error: Some(format!("{code:?}: {reason}")), timestamp_ms },
        Ok(other) => SharedStreamOutputSample { port, value: None, error: Some(format!("unexpected engine response: {other:?}")), timestamp_ms },
        Err(err) => SharedStreamOutputSample { port, value: None, error: Some(err.to_string()), timestamp_ms },
    }
}

fn aggregate_stream_output_ports(clients: &BTreeMap<uuid::Uuid, StreamOutputsClientConfig>) -> BTreeMap<String, Duration> {
    let mut ports = BTreeMap::<String, Duration>::new();
    for client in clients.values() {
        for port in &client.ports {
            ports
                .entry(port.clone())
                .and_modify(|current| {
                    if client.sample_interval < *current {
                        *current = client.sample_interval;
                    }
                })
                .or_insert(client.sample_interval);
        }
    }
    ports
}

fn aggregate_stream_outputs_ports_interval(clients: &BTreeMap<uuid::Uuid, StreamOutputsClientConfig>) -> Duration {
    clients.values().map(|client| client.ports_interval).min().unwrap_or_else(|| Duration::from_secs(2))
}

fn stream_metrics_topic(topics: &Arc<StdMutex<BTreeMap<uuid::Uuid, Arc<StreamMetricsTopic>>>>, stream_id: uuid::Uuid) -> Option<Arc<StreamMetricsTopic>> {
    topics.lock().ok()?.get(&stream_id).cloned()
}

fn stream_metrics_has_receivers(topics: &Arc<StdMutex<BTreeMap<uuid::Uuid, Arc<StreamMetricsTopic>>>>) -> bool {
    topics.lock().map(|guard| guard.values().any(|topic| topic.tx.receiver_count() > 0)).unwrap_or(false)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ProcessMappingBucket {
    Executable,
    SharedLibrary,
    Heap,
    Stack,
    Anonymous,
    Device,
    Deleted,
    Other,
}

impl ProcessMappingBucket {
    fn as_str(self) -> &'static str {
        match self {
            Self::Executable => "executable",
            Self::SharedLibrary => "shared_library",
            Self::Heap => "heap",
            Self::Stack => "stack",
            Self::Anonymous => "anonymous",
            Self::Device => "device",
            Self::Deleted => "deleted",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Default, Clone)]
struct ProcessMemoryAttribution {
    tgid: Option<u32>,
    executable: Option<String>,
    executable_file_bytes: Option<u64>,
    threads: u64,
    rss_bytes: Option<u64>,
    pss_bytes: Option<u64>,
    private_dirty_bytes: u64,
    swap_bytes: u64,
    executable_pss_bytes: u64,
    shared_lib_pss_bytes: u64,
    heap_pss_bytes: u64,
    stack_pss_bytes: u64,
    anonymous_pss_bytes: u64,
    device_pss_bytes: u64,
    deleted_pss_bytes: u64,
    other_pss_bytes: u64,
    top_pss_mappings: Vec<ProcessMappingMetrics>,
}

fn collect_process_memory_metrics(sys: &System) -> Vec<ProcessMemoryMetrics> {
    let limit = process_breakdown_limit();
    let budget = process_breakdown_budget();
    let started = Instant::now();
    let mut processes: Vec<&sysinfo::Process> = sys.processes().values().collect();
    processes.sort_by(|left, right| right.memory().cmp(&left.memory()).then_with(|| left.pid().as_u32().cmp(&right.pid().as_u32())));

    let mut output = Vec::with_capacity(limit.min(processes.len()));
    for process in processes {
        if output.len() >= limit {
            break;
        }
        if budget > Duration::from_millis(0) && started.elapsed() >= budget {
            break;
        }

        let pid = process.pid().as_u32();
        let name = process.name().to_string_lossy().to_string();
        let attribution = read_process_memory_attribution(pid);
        if attribution.tgid.is_some_and(|tgid| tgid != pid) {
            continue;
        }
        output.push(ProcessMemoryMetrics {
            pid,
            name,
            executable: attribution.executable,
            executable_file_bytes: attribution.executable_file_bytes,
            threads: attribution.threads,
            rss_bytes: attribution.rss_bytes.unwrap_or_else(|| process.memory()),
            pss_bytes: attribution.pss_bytes,
            private_dirty_bytes: attribution.private_dirty_bytes,
            swap_bytes: attribution.swap_bytes,
            executable_pss_bytes: attribution.executable_pss_bytes,
            shared_lib_pss_bytes: attribution.shared_lib_pss_bytes,
            heap_pss_bytes: attribution.heap_pss_bytes,
            stack_pss_bytes: attribution.stack_pss_bytes,
            anonymous_pss_bytes: attribution.anonymous_pss_bytes,
            device_pss_bytes: attribution.device_pss_bytes,
            deleted_pss_bytes: attribution.deleted_pss_bytes,
            other_pss_bytes: attribution.other_pss_bytes,
            top_pss_mappings: attribution.top_pss_mappings,
        });
    }

    output
}

fn process_breakdown_limit() -> usize {
    static LIMIT: OnceLock<usize> = OnceLock::new();
    *LIMIT.get_or_init(|| std::env::var("HELIOS_DEVICE_PROCESS_BREAKDOWN_LIMIT").ok().and_then(|value| value.trim().parse::<usize>().ok()).unwrap_or(16).clamp(1, 128))
}

fn process_breakdown_budget() -> Duration {
    static BUDGET: OnceLock<Duration> = OnceLock::new();
    *BUDGET.get_or_init(|| read_duration_env("HELIOS_DEVICE_PROCESS_BREAKDOWN_BUDGET_MS", 400, 0, 5_000))
}

fn process_mapping_limit() -> usize {
    static LIMIT: OnceLock<usize> = OnceLock::new();
    *LIMIT.get_or_init(|| std::env::var("HELIOS_DEVICE_PROCESS_MAPPING_LIMIT").ok().and_then(|value| value.trim().parse::<usize>().ok()).unwrap_or(8).clamp(1, 64))
}

fn read_process_memory_attribution(pid: u32) -> ProcessMemoryAttribution {
    let proc_root = PathBuf::from(format!("/proc/{pid}"));
    let status = fs::read_to_string(proc_root.join("status")).ok();
    let smaps_rollup = fs::read_to_string(proc_root.join("smaps_rollup")).ok();
    let smaps = fs::read_to_string(proc_root.join("smaps")).ok();
    let executable = fs::read_link(proc_root.join("exe")).ok().map(|path| path.to_string_lossy().to_string());
    let executable_file_bytes = executable.as_ref().and_then(|path| fs::metadata(path).ok().map(|metadata| metadata.len()));

    let mut attribution = ProcessMemoryAttribution {
        tgid: status.as_deref().and_then(|text| parse_proc_key_bytes(text, "Tgid:")).and_then(|value| u32::try_from(value).ok()),
        executable,
        executable_file_bytes,
        threads: status.as_deref().and_then(|text| parse_proc_key_bytes(text, "Threads:")).unwrap_or(0),
        rss_bytes: status.as_deref().and_then(|text| parse_proc_key_bytes(text, "VmRSS:")),
        pss_bytes: smaps_rollup.as_deref().and_then(|text| parse_proc_key_bytes(text, "Pss:")),
        private_dirty_bytes: smaps_rollup.as_deref().and_then(|text| parse_proc_key_bytes(text, "Private_Dirty:")).unwrap_or(0),
        swap_bytes: status.as_deref().and_then(|text| parse_proc_key_bytes(text, "VmSwap:")).unwrap_or(0),
        ..ProcessMemoryAttribution::default()
    };

    if let Some(smaps) = smaps.as_deref() {
        merge_smaps_breakdown(&mut attribution, smaps);
    }

    attribution
}

fn merge_smaps_breakdown(attribution: &mut ProcessMemoryAttribution, smaps: &str) {
    let mut current_bucket = ProcessMappingBucket::Other;
    let mut current_label = String::from("[other]");
    let mut per_mapping = BTreeMap::<(ProcessMappingBucket, String), u64>::new();
    for line in smaps.lines() {
        if let Some(pathname) = parse_smaps_mapping_path(line) {
            current_bucket = classify_process_mapping(pathname.as_deref(), attribution.executable.as_deref());
            current_label = mapping_label(current_bucket, pathname.as_deref(), attribution.executable.as_deref());
            continue;
        }
        let Some(pss_bytes) = parse_proc_key_bytes(line, "Pss:") else {
            continue;
        };
        let key = (current_bucket, current_label.clone());
        let entry = per_mapping.entry(key).or_default();
        *entry = entry.saturating_add(pss_bytes);
        match current_bucket {
            ProcessMappingBucket::Executable => attribution.executable_pss_bytes = attribution.executable_pss_bytes.saturating_add(pss_bytes),
            ProcessMappingBucket::SharedLibrary => attribution.shared_lib_pss_bytes = attribution.shared_lib_pss_bytes.saturating_add(pss_bytes),
            ProcessMappingBucket::Heap => attribution.heap_pss_bytes = attribution.heap_pss_bytes.saturating_add(pss_bytes),
            ProcessMappingBucket::Stack => attribution.stack_pss_bytes = attribution.stack_pss_bytes.saturating_add(pss_bytes),
            ProcessMappingBucket::Anonymous => attribution.anonymous_pss_bytes = attribution.anonymous_pss_bytes.saturating_add(pss_bytes),
            ProcessMappingBucket::Device => attribution.device_pss_bytes = attribution.device_pss_bytes.saturating_add(pss_bytes),
            ProcessMappingBucket::Deleted => attribution.deleted_pss_bytes = attribution.deleted_pss_bytes.saturating_add(pss_bytes),
            ProcessMappingBucket::Other => attribution.other_pss_bytes = attribution.other_pss_bytes.saturating_add(pss_bytes),
        }
    }

    let mut top: Vec<ProcessMappingMetrics> = per_mapping.into_iter().map(|((bucket, label), pss_bytes)| ProcessMappingMetrics { bucket: bucket.as_str().to_string(), label, pss_bytes }).collect();
    top.sort_by(|left, right| right.pss_bytes.cmp(&left.pss_bytes).then_with(|| left.label.cmp(&right.label)));
    top.truncate(process_mapping_limit());
    attribution.top_pss_mappings = top;
}

fn parse_smaps_mapping_path(line: &str) -> Option<Option<String>> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let first = *fields.first()?;
    if !first.contains('-') || !first.as_bytes().first().is_some_and(u8::is_ascii_hexdigit) {
        return None;
    }

    if fields.len() <= 5 {
        return Some(None);
    }

    Some(Some(fields[5..].join(" ")))
}

fn classify_process_mapping(pathname: Option<&str>, executable: Option<&str>) -> ProcessMappingBucket {
    let Some(pathname) = pathname.map(str::trim) else {
        return ProcessMappingBucket::Anonymous;
    };

    if pathname.is_empty() {
        return ProcessMappingBucket::Anonymous;
    }
    if pathname.ends_with(" (deleted)") {
        return ProcessMappingBucket::Deleted;
    }
    if pathname == "[heap]" {
        return ProcessMappingBucket::Heap;
    }
    if pathname.starts_with("[stack") {
        return ProcessMappingBucket::Stack;
    }
    if executable.is_some_and(|candidate| pathname == candidate) {
        return ProcessMappingBucket::Executable;
    }
    if pathname.contains(".so") {
        return ProcessMappingBucket::SharedLibrary;
    }
    if pathname.starts_with("/dev/") || pathname.starts_with("/memfd:") || pathname.starts_with("memfd:") || pathname.contains("dmabuf") {
        return ProcessMappingBucket::Device;
    }
    if pathname.starts_with("[anon") {
        return ProcessMappingBucket::Anonymous;
    }
    if pathname.starts_with('[') {
        return ProcessMappingBucket::Other;
    }
    if pathname.starts_with('/') {
        return ProcessMappingBucket::Other;
    }
    ProcessMappingBucket::Anonymous
}

fn mapping_label(bucket: ProcessMappingBucket, pathname: Option<&str>, executable: Option<&str>) -> String {
    match bucket {
        ProcessMappingBucket::Anonymous => pathname.map(str::trim).filter(|value| !value.is_empty()).unwrap_or("[anonymous]").to_string(),
        ProcessMappingBucket::Heap => "[heap]".to_string(),
        ProcessMappingBucket::Stack => pathname.unwrap_or("[stack]").trim().to_string(),
        ProcessMappingBucket::Executable => executable.or(pathname).unwrap_or("[executable]").trim().to_string(),
        ProcessMappingBucket::SharedLibrary | ProcessMappingBucket::Device | ProcessMappingBucket::Deleted | ProcessMappingBucket::Other => {
            pathname.map(str::trim).filter(|value| !value.is_empty()).unwrap_or(bucket.as_str()).to_string()
        }
    }
}

fn reason_for_update_kind(kind: &str) -> DevicesUpdateReason {
    match kind {
        "streams" => DevicesUpdateReason::Streams,
        "pipelines" => DevicesUpdateReason::Pipelines,
        "localization" => DevicesUpdateReason::Localization,
        "media" => DevicesUpdateReason::Media,
        "imu" => DevicesUpdateReason::Imu,
        "device" => DevicesUpdateReason::Device,
        "settings" => DevicesUpdateReason::Settings,
        _ => DevicesUpdateReason::Api,
    }
}

fn broadcast_devices_update(tx: &broadcast::Sender<Arc<SharedDevicesUpdate>>, reasons: &BTreeSet<DevicesUpdateReason>) {
    let update = Arc::new(SharedDevicesUpdate { timestamp_ms: chrono::Utc::now().timestamp_millis().max(0) as u64, reasons: reasons.iter().copied().collect() });
    let _ = tx.send(update);
}

fn usb_fingerprint() -> String {
    let root = Path::new("/sys/bus/usb/devices");
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return String::new(),
    };

    let mut keys: Vec<String> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let vendor = fs::read_to_string(path.join("idVendor")).ok();
        let product = fs::read_to_string(path.join("idProduct")).ok();
        let (Some(vendor), Some(product)) = (vendor, product) else { continue };
        let name = entry.file_name().to_string_lossy().to_string();
        keys.push(format!("{name}:{}:{}", vendor.trim(), product.trim()));
    }
    keys.sort();
    keys.join("|")
}

async fn stream_fingerprint(state: &Arc<IpcHandles>) -> Option<String> {
    let streams = match state.engine.list_streams_with_timeout(list_streams_timeout()).await {
        Ok(streams) => streams,
        Err(err) => {
            debug!(%err, "devices updates stream list failed");
            return None;
        }
    };

    let mut keys: Vec<String> = streams.into_iter().map(|summary| format!("{}:{}", summary.stream_id, stream_state_label(summary.status.state))).collect();
    keys.sort();
    Some(keys.join("|"))
}

fn stream_state_label(state: StreamState) -> &'static str {
    match state {
        StreamState::Running => "running",
        StreamState::Disabled => "disabled",
    }
}

fn sanitize_log_filename(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return "logs.log".into();
    }
    let mut out = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            out.push(ch);
        } else if ch.is_whitespace() {
            out.push('_');
        } else {
            out.push('-');
        }
    }
    let out = out.trim_matches(&['.', '-', '_'][..]).to_string();
    if out.is_empty() { "logs.log".into() } else { format!("{out}.log") }
}

fn journal_script(unit: Option<&str>, lines: usize, follow: bool) -> String {
    let n = lines.to_string();
    let follow_flag = if follow { "-f" } else { "" };
    let unit_flag = unit.map(|u| format!("-u {u}")).unwrap_or_default();
    format!("journalctl --no-pager -o short-iso {follow_flag} -n {n} {unit_flag}")
}

fn file_script(path: &str, lines: usize, follow: bool) -> String {
    let n = lines.to_string();
    if follow { format!("tail -n {n} -F {path}") } else { format!("tail -n {n} {path}") }
}

fn dmesg_script(lines: usize, follow: bool) -> String {
    let n = lines.to_string();
    if follow {
        return format!(
            r#"
if command -v journalctl >/dev/null 2>&1; then
  journalctl -k --no-pager -o short-iso -n {n} -f
elif [ -r /dev/kmsg ]; then
  dmesg 2>/dev/null | tail -n {n}
  cat /dev/kmsg
else
  dmesg 2>/dev/null | tail -n {n}
fi
"#
        );
    }

    format!(
        r#"
dmesg 2>/dev/null | tail -n {n}
"#
    )
}

async fn sample_power_from_peripherals(state: Option<&Weak<IpcHandles>>) -> Option<PowerTelemetry> {
    use crate::http::device::power::power_status_from_snapshot;
    use helios_peripherals::dto::SensorScope;

    let handles = state?.upgrade()?;
    let sensors = handles.ensure_sensors().await?;
    let response = tokio::time::timeout(Duration::from_millis(250), sensors.sensor_snapshot(SensorScope::Device)).await.ok()?;
    let response = response.ok()?;
    let snapshot = response.ok()?;
    let status = power_status_from_snapshot(&snapshot);

    Some(PowerTelemetry { watts: status.watts.map(|value| value as f32), volts: status.volts.map(|value| value as f32), amps: status.amps.map(|value| value as f32) })
}

#[derive(Clone)]
struct NetSnapshot {
    at: Instant,
    totals: BTreeMap<String, (u64, u64)>,
}

fn compute_net_deltas(prev: Option<(u64, u64)>, rx: u64, tx: u64, snapshot_time: Option<Instant>) -> (u64, u64, f64, f64) {
    if let (Some((prev_rx, prev_tx)), Some(at)) = (prev, snapshot_time) {
        let elapsed = at.elapsed().as_secs_f64().max(0.001);
        let delta_rx = rx.saturating_sub(prev_rx);
        let delta_tx = tx.saturating_sub(prev_tx);
        let per_sec_rx = delta_rx as f64 / elapsed;
        let per_sec_tx = delta_tx as f64 / elapsed;
        (delta_rx, delta_tx, per_sec_rx, per_sec_tx)
    } else {
        (0, 0, 0.0, 0.0)
    }
}

fn sample_gpu() -> Option<GpuTelemetry> {
    let usage_percent = read_gpu_busy_percent();
    let temperature_c = read_gpu_temperature();
    let frequency_mhz = read_gpu_frequency_mhz();
    let memory = read_gpu_memory();

    if usage_percent.is_none() && temperature_c.is_none() && frequency_mhz.is_none() && memory.is_none() {
        return None;
    }

    Some(GpuTelemetry { usage_percent, temperature_c, frequency_mhz, memory })
}

fn collect_disk_partitions(disks: &Disks) -> Vec<DiskPartitionTelemetry> {
    let mounts = read_mountinfo_entries();
    let overlay_backing_key = overlay_backing_mount_key(&mounts);
    let mut grouped = BTreeMap::<String, DiskPartitionTelemetry>::new();
    let mut selection_keys = BTreeMap::<String, MountSelectionKey>::new();

    for disk in disks.iter() {
        let mount_point = disk.mount_point();
        let mount_entry = mounts.iter().find(|entry| entry.mount_point == mount_point);
        let (effective_entry, effective_mount_point) = effective_disk_mount_context(&mounts, mount_entry, mount_point, overlay_backing_key.as_deref());
        let key = effective_entry.map(disk_group_key).unwrap_or_else(|| effective_mount_point.to_string_lossy().to_string());

        let (total, used, free) =
            filesystem_usage_for_path(&effective_mount_point).unwrap_or_else(|| (disk.total_space(), disk.total_space().saturating_sub(disk.available_space()), disk.available_space()));
        let selection_key = mount_selection_key(effective_entry, &effective_mount_point);
        let telemetry = DiskPartitionTelemetry { mount: effective_mount_point.to_string_lossy().to_string(), total_bytes: total, used_bytes: used, free_bytes: free };

        if selection_keys.get(&key).is_none_or(|current| selection_key < *current) {
            selection_keys.insert(key.clone(), selection_key);
            grouped.insert(key, telemetry);
        }
    }

    grouped.into_values().collect()
}

fn select_disk_for_path<'a>(disks: &'a Disks, path: &Path) -> Option<&'a sysinfo::Disk> {
    let mut best: Option<&sysinfo::Disk> = None;
    let mut best_len = 0usize;
    for disk in disks.iter() {
        let mount = disk.mount_point();
        if path.starts_with(mount) {
            let len = mount.as_os_str().len();
            if len >= best_len {
                best = Some(disk);
                best_len = len;
            }
        }
    }
    best
}

fn collect_disk_metrics(disks: &Disks) -> Vec<DiskMetrics> {
    collect_disk_partitions(disks).into_iter().map(|partition| DiskMetrics { mount: partition.mount, total_bytes: partition.total_bytes, available_bytes: partition.free_bytes }).collect()
}

fn filesystem_usage_for_path(path: &Path) -> Option<(u64, u64, u64)> {
    let stats = statvfs(path).ok()?;
    let fragment = stats.fragment_size();
    let total = stats.blocks().saturating_mul(fragment);
    // `blocks_free` includes reserved ext4 blocks, which should not count as user-visible
    // "used" space on the dashboard. Keep `free` as unprivileged-available bytes while
    // deriving used bytes from actual occupied blocks.
    let free = stats.blocks_available().saturating_mul(fragment);
    let used = total.saturating_sub(stats.blocks_free().saturating_mul(fragment));
    Some((total, used, free))
}

#[derive(Debug, Clone)]
struct MountinfoEntry {
    root: PathBuf,
    mount_point: PathBuf,
    major_minor: String,
    fs_type: String,
    source: String,
    super_options: Vec<String>,
}

fn read_mountinfo_entries() -> Vec<MountinfoEntry> {
    let Ok(raw) = fs::read_to_string("/proc/self/mountinfo") else {
        return Vec::new();
    };

    raw.lines()
        .filter_map(|line| {
            let (pre, post) = line.split_once(" - ")?;
            let pre_fields: Vec<&str> = pre.split_whitespace().collect();
            let post_fields: Vec<&str> = post.split_whitespace().collect();
            let root = decode_mountinfo_field(pre_fields.get(3)?);
            let mount_point = decode_mountinfo_field(pre_fields.get(4)?);
            let major_minor = (*pre_fields.get(2)?).to_string();
            let fs_type = (*post_fields.first()?).to_string();
            let source = post_fields.get(1).copied().unwrap_or_default().to_string();
            let super_options = post_fields.get(2).copied().unwrap_or_default().split(',').map(str::to_string).collect();
            Some(MountinfoEntry { root, mount_point, major_minor, fs_type, source, super_options })
        })
        .collect()
}

fn decode_mountinfo_field(raw: &str) -> PathBuf {
    let mut out = String::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx] == b'\\' && idx + 3 < bytes.len() {
            let octal = &raw[idx + 1..idx + 4];
            if let Ok(value) = u8::from_str_radix(octal, 8) {
                out.push(value as char);
                idx += 4;
                continue;
            }
        }
        out.push(bytes[idx] as char);
        idx += 1;
    }
    PathBuf::from(out)
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct MountSelectionKey {
    filesystem_kind_rank: usize,
    root_rank: usize,
    depth: usize,
    mount_len: usize,
    mount: String,
}

fn overlay_backing_mount_key(mounts: &[MountinfoEntry]) -> Option<String> {
    let overlay_root = mounts.iter().find(|entry| entry.mount_point == Path::new("/") && entry.fs_type == "overlay")?;

    for prefix in ["upperdir=", "workdir="] {
        let Some(candidate_path) = overlay_mount_option_path(overlay_root, prefix) else {
            continue;
        };
        let Some(backing_mount) = mountinfo_entry_for_path(mounts, &candidate_path) else {
            continue;
        };
        if backing_mount.mount_point == Path::new("/") {
            continue;
        }
        return Some(disk_group_key(backing_mount));
    }

    infer_visible_overlay_backing_mount_key(mounts)
}

fn infer_visible_overlay_backing_mount_key(mounts: &[MountinfoEntry]) -> Option<String> {
    let mut counts = BTreeMap::<String, (u64, MountSelectionKey)>::new();

    for entry in mounts.iter().filter(|entry| is_probable_block_filesystem_mount(entry)) {
        let key = disk_group_key(entry);
        let selection = mount_selection_key(Some(entry), &entry.mount_point);
        let record = counts.entry(key).or_insert((0, selection.clone()));
        record.0 = record.0.saturating_add(1);
        if selection < record.1 {
            record.1 = selection;
        }
    }

    counts
        .into_iter()
        .max_by(|(left_key, (left_count, left_selection)), (right_key, (right_count, right_selection))| {
            left_count.cmp(right_count).then_with(|| right_selection.cmp(left_selection)).then_with(|| right_key.cmp(left_key))
        })
        .map(|(key, _)| key)
}

fn is_probable_block_filesystem_mount(entry: &MountinfoEntry) -> bool {
    if entry.mount_point == Path::new("/") || entry.fs_type == "overlay" {
        return false;
    }

    entry.source.starts_with("/dev/")
}

fn effective_disk_mount_context<'a>(
    mounts: &'a [MountinfoEntry],
    mount_entry: Option<&'a MountinfoEntry>,
    mount_point: &Path,
    overlay_backing_key: Option<&str>,
) -> (Option<&'a MountinfoEntry>, PathBuf) {
    if mount_point == Path::new("/")
        && let Some(entry) = mount_entry
        && entry.fs_type == "overlay"
        && let Some(backing_key) = overlay_backing_key
        && let Some(backing_entry) = mounts.iter().find(|candidate| disk_group_key(candidate) == backing_key)
    {
        return (Some(backing_entry), backing_entry.mount_point.clone());
    }

    (mount_entry, mount_point.to_path_buf())
}

fn disk_group_key(entry: &MountinfoEntry) -> String {
    format!("{}:{}:{}", entry.major_minor, entry.source, entry.fs_type)
}

fn mount_selection_key(entry: Option<&MountinfoEntry>, mount_point: &Path) -> MountSelectionKey {
    let filesystem_kind_rank = match entry.map(|entry| entry.fs_type.as_str()) {
        Some("overlay") => 2,
        Some(_) => 0,
        None => 1,
    };
    let root_rank = match entry {
        Some(entry) if entry.root == Path::new("/") => 0,
        Some(_) => 1,
        None => 2,
    };
    MountSelectionKey { filesystem_kind_rank, root_rank, depth: mount_point.components().count(), mount_len: mount_point.as_os_str().len(), mount: mount_point.to_string_lossy().to_string() }
}

fn mountinfo_entry_for_path<'a>(mounts: &'a [MountinfoEntry], path: &Path) -> Option<&'a MountinfoEntry> {
    mounts.iter().filter(|entry| path.starts_with(&entry.mount_point)).max_by_key(|entry| entry.mount_point.as_os_str().len())
}

fn overlay_mount_option_path(entry: &MountinfoEntry, prefix: &str) -> Option<PathBuf> {
    entry.super_options.iter().find_map(|option| option.strip_prefix(prefix).map(decode_mountinfo_field))
}

fn read_gpu_busy_percent() -> Option<f32> {
    let drm_dir = Path::new("/sys/class/drm");
    let entries = fs::read_dir(drm_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.file_name().and_then(|name| name.to_str()).map(|name| name.starts_with("card")).unwrap_or(false) {
            continue;
        }
        let device_dir = path.join("device");
        if let Some(val) = read_percent_file(&device_dir.join("gpu_busy_percent")) {
            return Some(val);
        }
        if let Ok(engine_dirs) = fs::read_dir(device_dir.join("engine")) {
            for engine in engine_dirs.flatten() {
                if let Some(val) = read_percent_file(&engine.path().join("busy_percent")) {
                    return Some(val);
                }
            }
        }
    }
    read_gpu_busy_percent_from_stats()
}

#[derive(Debug, Clone, Copy)]
struct GpuStatsSnapshot {
    timestamp: u64,
    jobs: u64,
}

#[derive(Debug, Default, Clone, Copy)]
struct GpuBusyState {
    last: Option<GpuStatsSnapshot>,
}

fn read_gpu_busy_percent_from_stats() -> Option<f32> {
    static STATE: OnceLock<StdMutex<GpuBusyState>> = OnceLock::new();
    let snapshot = read_gpu_stats_snapshot()?;
    let state = STATE.get_or_init(|| StdMutex::new(GpuBusyState::default()));
    let mut guard = state.lock().ok()?;

    let percent = match guard.last {
        None => 0.0,
        Some(prev) => {
            let dt = snapshot.timestamp.saturating_sub(prev.timestamp);
            let dj = snapshot.jobs.saturating_sub(prev.jobs);
            if dt == 0 || dj == 0 { 0.0 } else { 100.0 }
        }
    };

    guard.last = Some(snapshot);
    Some(percent)
}

fn read_gpu_stats_snapshot() -> Option<GpuStatsSnapshot> {
    let drm_dir = Path::new("/sys/class/drm");
    let entries = fs::read_dir(drm_dir).ok()?;
    for entry in entries.flatten() {
        let card_dir = entry.path();
        let file_name = card_dir.file_name().and_then(|name| name.to_str()).unwrap_or("");
        if !file_name.starts_with("card") {
            continue;
        }
        if let Some(snapshot) = parse_gpu_stats_file(&card_dir.join("device/gpu_stats")) {
            return Some(snapshot);
        }
    }
    None
}

fn parse_gpu_stats_file(path: &Path) -> Option<GpuStatsSnapshot> {
    let raw = fs::read_to_string(path).ok()?;
    let mut timestamp: Option<u64> = None;
    let mut jobs_sum: u64 = 0;
    for (idx, line) in raw.lines().enumerate() {
        if idx == 0 {
            continue;
        }
        let mut parts = line.split_whitespace();
        let queue = parts.next()?;
        let ts = parts.next()?.parse::<u64>().ok()?;
        let jobs = parts.next()?.parse::<u64>().ok()?;
        let _runtime = parts.next()?.parse::<u64>().ok()?;
        if matches!(queue, "bin" | "render" | "tfu" | "csd") {
            jobs_sum = jobs_sum.saturating_add(jobs);
        }
        timestamp = Some(timestamp.map_or(ts, |prev| prev.max(ts)));
    }
    Some(GpuStatsSnapshot { timestamp: timestamp?, jobs: jobs_sum })
}

fn read_gpu_temperature() -> Option<f32> {
    let drm_dir = Path::new("/sys/class/drm");
    for entry in fs::read_dir(drm_dir).ok()?.flatten() {
        let card_dir = entry.path();
        if !card_dir.file_name().and_then(|name| name.to_str()).map(|name| name.starts_with("card")).unwrap_or(false) {
            continue;
        }
        if let Ok(hwmon_dirs) = fs::read_dir(card_dir.join("device/hwmon")) {
            for hwmon in hwmon_dirs.flatten() {
                if let Some(raw) = read_integer_file(&hwmon.path().join("temp1_input")) {
                    return Some(raw as f32 / 1000.0);
                }
            }
        }
    }
    None
}

fn read_gpu_frequency_mhz() -> Option<u64> {
    read_gpu_frequency_debugfs_mhz().or_else(read_gpu_frequency_sysfs_mhz)
}

fn read_gpu_frequency_sysfs_mhz() -> Option<u64> {
    let entries = fs::read_dir("/sys/class/devfreq").ok()?;
    for entry in entries.flatten() {
        let raw = fs::read_to_string(entry.path().join("cur_freq")).ok()?;
        let hz = raw.trim().parse::<u64>().ok()?;
        if hz > 0 {
            return Some(hz / 1_000_000);
        }
    }
    None
}

fn read_gpu_frequency_debugfs_mhz() -> Option<u64> {
    let entries = fs::read_dir("/sys/kernel/debug/dri").ok()?;
    for entry in entries.flatten() {
        let path = entry.path().join("measure_clock");
        if !path.exists() {
            continue;
        }
        if let Some(mhz) = parse_debugfs_measure_clock_mhz(&path) {
            return Some(mhz);
        }
    }
    None
}

fn parse_debugfs_measure_clock_mhz(path: &Path) -> Option<u64> {
    let raw = fs::read_to_string(path).ok()?;
    let lower = raw.to_ascii_lowercase();
    let start = lower.find('(')?;
    let mhz_pos = lower[start..].find("mhz")? + start;
    let inner = raw.get(start + 1..mhz_pos)?.trim();
    let number = inner.split_whitespace().next()?;
    let parsed = number.parse::<f64>().ok()?;
    Some(parsed.round() as u64)
}

fn read_gpu_memory() -> Option<GpuMemorySample> {
    let drm_dir = Path::new("/sys/class/drm");
    for entry in fs::read_dir(drm_dir).ok()?.flatten() {
        let device_dir = entry.path().join("device");
        if !device_dir.exists() {
            continue;
        }
        let used = read_integer_file(&device_dir.join("mem_info_vram_used"));
        let total = read_integer_file(&device_dir.join("mem_info_vram_total"));
        if used.is_none() && total.is_none() {
            continue;
        }
        let total_bytes = total.unwrap_or(0);
        let used_bytes = used.unwrap_or(0);
        let free_bytes = total_bytes.saturating_sub(used_bytes);
        return Some(GpuMemorySample { total_bytes, used_bytes, free_bytes });
    }
    read_gpu_memory_from_cma()
}

fn read_gpu_memory_from_cma() -> Option<GpuMemorySample> {
    let meminfo = fs::read_to_string("/proc/meminfo").ok()?;
    let mut cma_total_kb: Option<u64> = None;
    let mut cma_free_kb: Option<u64> = None;
    for line in meminfo.lines() {
        if let Some(rest) = line.strip_prefix("CmaTotal:") {
            cma_total_kb = rest.split_whitespace().next().and_then(|value| value.parse::<u64>().ok());
        } else if let Some(rest) = line.strip_prefix("CmaFree:") {
            cma_free_kb = rest.split_whitespace().next().and_then(|value| value.parse::<u64>().ok());
        }
        if cma_total_kb.is_some() && cma_free_kb.is_some() {
            break;
        }
    }
    let total_kb = cma_total_kb?;
    let free_kb = cma_free_kb?;
    let total_bytes = total_kb.saturating_mul(1024);
    let free_bytes = free_kb.saturating_mul(1024);
    let used_bytes = total_bytes.saturating_sub(free_bytes);
    Some(GpuMemorySample { total_bytes, used_bytes, free_bytes })
}

fn read_cpu_temperature_c() -> Option<f32> {
    if let Ok(entries) = fs::read_dir("/sys/class/thermal") {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.file_name().and_then(|name| name.to_str()).map(|name| name.starts_with("thermal_zone")).unwrap_or(false) {
                continue;
            }
            let zone_type = fs::read_to_string(path.join("type")).ok().unwrap_or_default().to_ascii_lowercase();
            if !(zone_type.contains("cpu") || zone_type.contains("soc")) {
                continue;
            }
            let raw = fs::read_to_string(path.join("temp")).ok()?;
            let milli = raw.trim().parse::<i64>().ok()?;
            if milli <= 0 {
                continue;
            }
            return Some(milli as f32 / 1000.0);
        }
    }
    None
}

fn read_cpu_throttle_status() -> Option<CpuThrottleStatus> {
    let raw = fs::read_to_string("/sys/devices/platform/soc/firmware/get_throttled").ok()?;
    let trimmed = raw.trim().trim_start_matches("0x");
    let value = u32::from_str_radix(trimmed, 16).ok()?;
    Some(CpuThrottleStatus {
        raw: value,
        undervoltage: (value & 0x1) != 0,
        frequency_capped: (value & (1 << 1)) != 0,
        throttled: (value & (1 << 2)) != 0,
        soft_temp_limit: (value & (1 << 3)) != 0,
        undervoltage_since_boot: (value & (1 << 16)) != 0,
        frequency_capped_since_boot: (value & (1 << 17)) != 0,
        throttled_since_boot: (value & (1 << 18)) != 0,
        soft_temp_limit_since_boot: (value & (1 << 19)) != 0,
    })
}

fn read_percent_file(path: &PathBuf) -> Option<f32> {
    read_integer_file(path).map(|value| value as f32)
}

fn read_integer_file(path: &PathBuf) -> Option<u64> {
    let raw = fs::read_to_string(path).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<u64>().ok()
}

fn parse_proc_key_bytes(text: &str, key: &str) -> Option<u64> {
    text.lines().find_map(|line| {
        let trimmed = line.trim_start();
        if !trimmed.starts_with(key) {
            return None;
        }
        let value = trimmed[key.len()..].trim();
        let number = value.split_whitespace().next().and_then(|raw| raw.parse::<u64>().ok())?;
        if value.contains("kB") { Some(number.saturating_mul(1024)) } else { Some(number) }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mountinfo_entry(root: &str, mount_point: &str, major_minor: &str, fs_type: &str, source: &str, super_options: &[&str]) -> MountinfoEntry {
        MountinfoEntry {
            root: PathBuf::from(root),
            mount_point: PathBuf::from(mount_point),
            major_minor: major_minor.to_string(),
            fs_type: fs_type.to_string(),
            source: source.to_string(),
            super_options: super_options.iter().map(|value| (*value).to_string()).collect(),
        }
    }

    #[test]
    fn overlay_backing_mount_key_prefers_upperdir_backing_filesystem() {
        let mounts = vec![
            mountinfo_entry("/", "/", "0:53", "overlay", "overlay", &["rw", "lowerdir=/sysroot", "upperdir=/.overlay-data/root-a/upper", "workdir=/.overlay-data/root-a/work"]),
            mountinfo_entry("/", "/.overlay-data", "179:4", "ext4", "/dev/mmcblk0p4", &["rw"]),
            mountinfo_entry("/var/lib/helios", "/var/lib/helios", "179:4", "ext4", "/dev/mmcblk0p4", &["rw"]),
        ];

        assert_eq!(overlay_backing_mount_key(&mounts).as_deref(), Some("179:4:/dev/mmcblk0p4:ext4"));
    }

    #[test]
    fn overlay_backing_mount_key_falls_back_to_visible_block_mount_family_when_upperdir_is_hidden() {
        let mounts = vec![
            mountinfo_entry("/", "/", "0:53", "overlay", "overlay", &["rw", "lowerdir=/mnt/lower", "upperdir=/mnt/data/root-overlay/root-a/upper", "workdir=/mnt/data/root-overlay/root-a/work"]),
            mountinfo_entry("/", "/.overlay-data", "179:4", "ext4", "/dev/mmcblk0p4", &["rw"]),
            mountinfo_entry("/", "/var/lib/helios", "179:4", "ext4", "/dev/mmcblk0p4", &["rw"]),
            mountinfo_entry("/", "/run", "0:26", "tmpfs", "tmpfs", &["rw"]),
            mountinfo_entry("/", "/dev/shm", "0:24", "tmpfs", "tmpfs", &["rw"]),
        ];

        assert_eq!(overlay_backing_mount_key(&mounts).as_deref(), Some("179:4:/dev/mmcblk0p4:ext4"));
    }

    #[test]
    fn mount_selection_key_prefers_primary_mount_over_bind_mount() {
        let primary = mountinfo_entry("/", "/.overlay-data", "179:4", "ext4", "/dev/mmcblk0p4", &["rw"]);
        let bind = mountinfo_entry("/var/lib/helios", "/var/lib/helios", "179:4", "ext4", "/dev/mmcblk0p4", &["rw"]);

        assert!(mount_selection_key(Some(&primary), &primary.mount_point) < mount_selection_key(Some(&bind), &bind.mount_point));
    }

    #[test]
    fn effective_disk_mount_context_maps_overlay_root_to_backing_mount() {
        let mounts = vec![
            mountinfo_entry("/", "/", "0:53", "overlay", "overlay", &["rw", "lowerdir=/sysroot", "upperdir=/.overlay-data/root-a/upper", "workdir=/.overlay-data/root-a/work"]),
            mountinfo_entry("/", "/.overlay-data", "179:4", "ext4", "/dev/mmcblk0p4", &["rw"]),
            mountinfo_entry("/var/lib/helios", "/var/lib/helios", "179:4", "ext4", "/dev/mmcblk0p4", &["rw"]),
        ];
        let root_entry = mounts.iter().find(|entry| entry.mount_point == Path::new("/"));
        let backing_key = overlay_backing_mount_key(&mounts);

        let (effective_entry, effective_mount) = effective_disk_mount_context(&mounts, root_entry, Path::new("/"), backing_key.as_deref());

        assert_eq!(effective_mount, PathBuf::from("/.overlay-data"));
        assert_eq!(effective_entry.map(disk_group_key).as_deref(), Some("179:4:/dev/mmcblk0p4:ext4"));
    }

    #[test]
    fn parse_proc_key_bytes_handles_kib_and_plain_values() {
        let text = "Threads:\t4\nVmRSS:\t  18432 kB\nVmSwap:\t0 kB\n";
        assert_eq!(parse_proc_key_bytes(text, "Threads:"), Some(4));
        assert_eq!(parse_proc_key_bytes(text, "VmRSS:"), Some(18_874_368));
        assert_eq!(parse_proc_key_bytes(text, "VmSwap:"), Some(0));
    }

    #[test]
    fn classify_process_mapping_separates_runtime_buckets() {
        let executable = Some("/usr/bin/helios-updater");
        assert_eq!(classify_process_mapping(Some("/usr/bin/helios-updater"), executable), ProcessMappingBucket::Executable);
        assert_eq!(classify_process_mapping(Some("/usr/lib/libc.so.6"), executable), ProcessMappingBucket::SharedLibrary);
        assert_eq!(classify_process_mapping(Some("[heap]"), executable), ProcessMappingBucket::Heap);
        assert_eq!(classify_process_mapping(Some("[stack]"), executable), ProcessMappingBucket::Stack);
        assert_eq!(classify_process_mapping(None, executable), ProcessMappingBucket::Anonymous);
        assert_eq!(classify_process_mapping(Some("/dev/dma_heap/system"), executable), ProcessMappingBucket::Device);
        assert_eq!(classify_process_mapping(Some("/usr/bin/helios-updater (deleted)"), executable), ProcessMappingBucket::Deleted);
    }

    #[test]
    fn merge_smaps_breakdown_accounts_for_pss_by_bucket() {
        let mut attribution = ProcessMemoryAttribution { executable: Some("/usr/bin/helios-updater".into()), ..ProcessMemoryAttribution::default() };
        let smaps = "\
00400000-00452000 r-xp 00000000 08:01 123 /usr/bin/helios-updater\n\
Pss:                 128 kB\n\
7f000000-7f010000 r-xp 00000000 08:01 456 /usr/lib/libtokio.so\n\
Pss:                  64 kB\n\
7f010000-7f020000 rw-p 00000000 00:00 0 [heap]\n\
Pss:                  32 kB\n\
7f020000-7f030000 rw-p 00000000 00:00 0 [stack]\n\
Pss:                  16 kB\n\
7f030000-7f040000 rw-p 00000000 00:00 0\n\
Pss:                   8 kB\n\
7f040000-7f050000 rw-p 00000000 00:00 0 /dev/dma_heap/system\n\
Pss:                   4 kB\n\
7f050000-7f060000 rw-p 00000000 08:01 789 /usr/bin/old-updater (deleted)\n\
Pss:                   2 kB\n";

        merge_smaps_breakdown(&mut attribution, smaps);

        assert_eq!(attribution.executable_pss_bytes, 128 * 1024);
        assert_eq!(attribution.shared_lib_pss_bytes, 64 * 1024);
        assert_eq!(attribution.heap_pss_bytes, 32 * 1024);
        assert_eq!(attribution.stack_pss_bytes, 16 * 1024);
        assert_eq!(attribution.anonymous_pss_bytes, 8 * 1024);
        assert_eq!(attribution.device_pss_bytes, 4 * 1024);
        assert_eq!(attribution.deleted_pss_bytes, 2 * 1024);
        assert_eq!(attribution.top_pss_mappings.first().map(|mapping| mapping.bucket.as_str()), Some("executable"));
        assert_eq!(attribution.top_pss_mappings.first().map(|mapping| mapping.label.as_str()), Some("/usr/bin/helios-updater"));
        assert_eq!(attribution.top_pss_mappings.first().map(|mapping| mapping.pss_bytes), Some(128 * 1024));
    }

    #[tokio::test]
    async fn stream_outputs_unsubscribe_prunes_idle_topic() {
        let hub = StreamOutputsHub::new();
        let stream_id = uuid::Uuid::new_v4();
        let topic = hub.topic(stream_id);
        let client_id = uuid::Uuid::new_v4();

        topic.clients.lock().await.insert(client_id, StreamOutputsClientConfig { ports: Vec::new(), sample_interval: Duration::from_millis(10), ports_interval: Duration::from_millis(20) });

        let receiver = topic.tx.subscribe();
        assert!(hub.find_topic(stream_id).is_some());

        drop(receiver);
        hub.unsubscribe(stream_id, client_id).await;

        let (topic_count, subscriber_count) = hub.stats().await;
        assert_eq!(topic_count, 0);
        assert_eq!(subscriber_count, 0);
        assert!(hub.find_topic(stream_id).is_none());
    }

    #[tokio::test]
    async fn stream_metrics_unsubscribe_prunes_idle_topic() {
        let hub = StreamMetricsHub::new();
        let stream_id = uuid::Uuid::new_v4();
        let topic = hub.topic(stream_id);
        let latest = Arc::new(SharedStreamMetricsSnapshot { stream_id, metrics: StreamMetrics::default(), timestamp_ms: 1 });
        if let Ok(mut guard) = topic.latest.lock() {
            *guard = Some(latest);
        }

        let receiver = topic.tx.subscribe();
        assert!(hub.find_topic(stream_id).is_some());

        drop(receiver);
        hub.unsubscribe(stream_id).await;

        let (topic_count, subscriber_count) = hub.stats();
        assert_eq!(topic_count, 0);
        assert_eq!(subscriber_count, 0);
        assert!(hub.find_topic(stream_id).is_none());
    }
}
