mod collector;
mod hardware;
mod log_sources;
mod processes;
mod realtime;
mod storage_metrics;
mod streams;

use crate::api_observability::{ApiCacheMetric, ApiRealtimeMetrics, CacheMetricCounters};
use crate::http::device::metrics::DeviceMetrics;
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use tokio::sync::{RwLock, broadcast};
use tokio::time::{Duration, Instant};
use tracing::warn;

use self::collector::{MetricsCacheEntry, SystemCollector, collect_device_metrics};
use self::log_sources::LogSourcesCacheEntry;
use self::realtime::{DevicesUpdatesHub, ProcessesHub, TelemetryHub};
use self::streams::{StreamMetricsHub, StreamOutputsHub};

pub use self::realtime::{DevicesUpdateReason, ProcessSample, SharedDevicesUpdate, SharedProcessesSnapshot};
pub use self::streams::{SharedStreamMetricsSnapshot, SharedStreamOutputSample, SharedStreamOutputsEvent, SharedStreamOutputsPortsSnapshot};

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
