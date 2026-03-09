use axum::{Json, http::StatusCode};
use std::sync::{Mutex as StdMutex, OnceLock};
use sysinfo::{Components, Disks, System};
use tokio::sync::RwLock;
use tokio::time::{Duration, Instant};
use tracing::warn;
use utoipa::ToSchema;

use super::super::error::{ApiError, ApiResult};

#[derive(Debug, Clone, ToSchema, serde::Serialize)]
pub struct DeviceMetrics {
    pub cpu_avg_pct: f32,
    pub cpu_freq_mhz: u64,
    pub cpus: Vec<CpuCoreMetrics>,
    pub mem_total_bytes: u64,
    pub mem_used_bytes: u64,
    pub swap_total_bytes: u64,
    pub swap_used_bytes: u64,
    pub disks: Vec<DiskMetrics>,
    pub temps: Vec<TempReading>,
}

#[derive(Debug, Clone, ToSchema, serde::Serialize)]
pub struct DiskMetrics {
    pub mount: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
}

#[derive(Debug, Clone, ToSchema, serde::Serialize)]
pub struct CpuCoreMetrics {
    pub id: usize,
    pub name: String,
    pub pct: f32,
    pub freq_mhz: u64,
}

#[derive(Debug, Clone, ToSchema, serde::Serialize)]
pub struct TempReading {
    pub label: String,
    pub temperature_c: f32,
}

#[derive(Clone)]
struct MetricsCacheEntry {
    fetched_at: Instant,
    body: DeviceMetrics,
}

struct MetricsCollector {
    sys: System,
    disks: Disks,
    components: Components,
}

impl MetricsCollector {
    fn new() -> Self {
        Self { sys: System::new_all(), disks: Disks::new_with_refreshed_list(), components: Components::new_with_refreshed_list() }
    }

    fn collect(&mut self) -> DeviceMetrics {
        // Reuse the collector so sysinfo can keep incremental CPU sampling state hot.
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        self.disks.refresh(false);
        self.components.refresh(false);

        let cpu_avg_pct = if self.sys.cpus().is_empty() { 0.0 } else { self.sys.global_cpu_usage() };
        let cpu_freq_mhz = self.sys.cpus().first().map(|c| c.frequency()).unwrap_or(0);
        let cpus = self.sys.cpus().iter().enumerate().map(|(idx, c)| CpuCoreMetrics { id: idx, name: c.name().to_string(), pct: c.cpu_usage(), freq_mhz: c.frequency() }).collect();
        let disks = self.disks.iter().map(|d| DiskMetrics { mount: d.mount_point().to_string_lossy().to_string(), total_bytes: d.total_space(), available_bytes: d.available_space() }).collect();
        let temps = self.components.iter().filter_map(|c| c.temperature().map(|t| TempReading { label: c.label().to_string(), temperature_c: t })).collect();

        DeviceMetrics {
            cpu_avg_pct,
            cpu_freq_mhz,
            cpus,
            mem_total_bytes: self.sys.total_memory(),
            mem_used_bytes: self.sys.used_memory(),
            swap_total_bytes: self.sys.total_swap(),
            swap_used_bytes: self.sys.used_swap(),
            disks,
            temps,
        }
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

fn metrics_refresh_timeout() -> Duration {
    static TTL: OnceLock<Duration> = OnceLock::new();
    *TTL.get_or_init(|| read_duration_env("HELIOS_DEVICE_METRICS_TIMEOUT_MS", 2_000, 250, 15_000))
}

fn metrics_cache() -> &'static RwLock<Option<MetricsCacheEntry>> {
    static CACHE: OnceLock<RwLock<Option<MetricsCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

fn metrics_refresh_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn metrics_collector() -> &'static StdMutex<MetricsCollector> {
    static COLLECTOR: OnceLock<StdMutex<MetricsCollector>> = OnceLock::new();
    COLLECTOR.get_or_init(|| StdMutex::new(MetricsCollector::new()))
}

#[utoipa::path(
    get,
    path = "/device/metrics",
    tag = "Device",
    responses((status = 200, description = "Device metrics", body = DeviceMetrics))
)]
pub async fn metrics() -> ApiResult<impl axum::response::IntoResponse> {
    let ttl = metrics_cache_ttl();
    if ttl != Duration::from_millis(0)
        && let Some(entry) = metrics_cache().read().await.clone()
        && entry.fetched_at.elapsed() < ttl
    {
        return Ok((StatusCode::OK, Json(entry.body)));
    }

    let _refresh_guard = metrics_refresh_lock().lock().await;
    if ttl != Duration::from_millis(0)
        && let Some(entry) = metrics_cache().read().await.clone()
        && entry.fetched_at.elapsed() < ttl
    {
        return Ok((StatusCode::OK, Json(entry.body)));
    }

    let stale = metrics_cache().read().await.clone();
    let body = match tokio::time::timeout(metrics_refresh_timeout(), tokio::task::spawn_blocking(collect_device_metrics)).await {
        Ok(Ok(body)) => body,
        Ok(Err(err)) => {
            warn!(error = ?err, "device metrics task failed");
            if let Some(entry) = stale {
                return Ok((StatusCode::OK, Json(entry.body)));
            }
            return Err(ApiError::internal(format!("metrics task failed: {err}")));
        }
        Err(_) => {
            warn!(timeout_ms = metrics_refresh_timeout().as_millis(), "device metrics task timed out");
            if let Some(entry) = stale {
                return Ok((StatusCode::OK, Json(entry.body)));
            }
            return Err(ApiError::internal("metrics task timed out"));
        }
    };

    *metrics_cache().write().await = Some(MetricsCacheEntry { fetched_at: Instant::now(), body: body.clone() });
    Ok((StatusCode::OK, Json(body)))
}

fn collect_device_metrics() -> DeviceMetrics {
    metrics_collector().lock().expect("device metrics collector poisoned").collect()
}
