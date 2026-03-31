mod hardware;
mod log_sources;
mod processes;
mod realtime;
mod storage_metrics;
mod streams;

use crate::api_observability::{ApiCacheMetric, ApiRealtimeMetrics, CacheMetricCounters};
use crate::http::device::metrics::{CpuCoreMetrics, DeviceHealthIssue, DeviceMetrics, ProcessMemoryMetrics, TempReading};
use crate::http::storage;
use crate::ipc::IpcHandles;
use crate::ws::device::{CpuCoreSample, CpuTelemetry, DiskTelemetry, MemoryTelemetry, NetworkInterfaceSample, NetworkSample, PowerTelemetry, TelemetrySample};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex as StdMutex, OnceLock, Weak};
use sysinfo::{Components, Disks, Networks, ProcessesToUpdate, System};
use tokio::sync::{RwLock, broadcast};
use tokio::time::{Duration, Instant};
use tracing::warn;

use self::hardware::{read_cpu_temperature_c, read_cpu_throttle_status, sample_gpu};
use self::log_sources::LogSourcesCacheEntry;
use self::processes::collect_process_memory_metrics;
use self::realtime::{DevicesUpdatesHub, ProcessesHub, TelemetryHub};
use self::storage_metrics::{collect_disk_metrics, collect_disk_partitions, filesystem_usage_for_path, select_disk_for_path};
use self::streams::{StreamMetricsHub, StreamOutputsHub};

pub use self::realtime::{DevicesUpdateReason, ProcessSample, SharedDevicesUpdate, SharedProcessesSnapshot};
pub use self::streams::{SharedStreamMetricsSnapshot, SharedStreamOutputSample, SharedStreamOutputsEvent, SharedStreamOutputsPortsSnapshot};

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

fn read_duration_env(var: &str, default_ms: u64, min_ms: u64, max_ms: u64) -> Duration {
    let ms = std::env::var(var).ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(default_ms);
    Duration::from_millis(ms.clamp(min_ms, max_ms))
}

fn read_size_env(var: &str, default: usize, min: usize, max: usize) -> usize {
    std::env::var(var).ok().and_then(|value| value.trim().parse::<usize>().ok()).unwrap_or(default).clamp(min, max)
}

fn api_sampler_thread_stack_bytes() -> usize {
    static VALUE: OnceLock<usize> = OnceLock::new();
    *VALUE.get_or_init(|| read_size_env("HELIOS_API_SAMPLER_THREAD_STACK_BYTES", 512 * 1024, 128 * 1024, 4 * 1024 * 1024))
}

fn spawn_api_sampler_thread(name: &'static str, f: impl FnOnce() + Send + 'static) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new().name(name.to_string()).stack_size(api_sampler_thread_stack_bytes()).spawn(f)
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

    pub async fn subscribe_process_snapshots(&self) -> (broadcast::Receiver<Arc<SharedProcessesSnapshot>>, Option<Arc<SharedProcessesSnapshot>>) {
        self.processes_hub.subscribe().await
    }
}

fn collect_device_metrics(collector: &Arc<StdMutex<SystemCollector>>) -> DeviceMetrics {
    collector.lock().expect("system collector poisoned").collect_metrics()
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
