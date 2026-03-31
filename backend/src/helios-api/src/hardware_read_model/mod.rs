mod discovery;
mod inventory;

use crate::api_observability::{ApiCacheMetric, CacheMetricCounters};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};

pub(super) const INVENTORY_REFRESH_MIN: Duration = Duration::from_secs(10);

pub struct HardwareReadModelState {
    camera_cache: Mutex<Option<discovery::CachedDiscovery>>,
    inventory_refresh_at: Mutex<Instant>,
    peripheral_inventory_cache: RwLock<Option<inventory::PeripheralInventoryCacheEntry>>,
    peripheral_inventory_refresh_lock: Mutex<()>,
    camera_refresh_lock: Mutex<()>,
    peripheral_inventory_stats: CacheMetricCounters,
    camera_discovery_stats: CacheMetricCounters,
}

impl Default for HardwareReadModelState {
    fn default() -> Self {
        Self {
            camera_cache: Mutex::new(None),
            inventory_refresh_at: Mutex::new(Instant::now() - INVENTORY_REFRESH_MIN),
            peripheral_inventory_cache: RwLock::new(None),
            peripheral_inventory_refresh_lock: Mutex::new(()),
            camera_refresh_lock: Mutex::new(()),
            peripheral_inventory_stats: CacheMetricCounters::default(),
            camera_discovery_stats: CacheMetricCounters::default(),
        }
    }
}

fn read_timeout_env(var: &str, default_ms: u64, min_ms: u64, max_ms: u64) -> Duration {
    let ms = std::env::var(var).ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(default_ms);
    Duration::from_millis(ms.clamp(min_ms, max_ms))
}

pub(super) fn peripheral_inventory_cache_ttl() -> Duration {
    static VALUE: OnceLock<Duration> = OnceLock::new();
    *VALUE.get_or_init(|| read_timeout_env("HELIOS_PERIPHERALS_CACHE_MS", 1_000, 0, 10_000))
}

pub(super) fn sensor_ipc_timeout() -> Duration {
    static VALUE: OnceLock<Duration> = OnceLock::new();
    *VALUE.get_or_init(|| read_timeout_env("HELIOS_PERIPHERALS_TIMEOUT_MS", 1_500, 250, 15_000))
}

pub(super) fn camera_discovery_timeout() -> Duration {
    static VALUE: OnceLock<Duration> = OnceLock::new();
    *VALUE.get_or_init(|| read_timeout_env("HELIOS_CAMERA_DISCOVERY_TIMEOUT_MS", 1_500, 250, 20_000))
}

pub(super) fn sensor_refresh_timeout() -> Duration {
    static VALUE: OnceLock<Duration> = OnceLock::new();
    *VALUE.get_or_init(|| read_timeout_env("HELIOS_PERIPHERALS_REFRESH_TIMEOUT_MS", 2_500, 500, 20_000))
}

impl HardwareReadModelState {
    pub fn peripheral_inventory_cache_metrics(&self) -> ApiCacheMetric {
        self.peripheral_inventory_stats.snapshot()
    }

    pub fn camera_discovery_cache_metrics(&self) -> ApiCacheMetric {
        self.camera_discovery_stats.snapshot()
    }

    pub async fn invalidate_peripheral_inventory_cache(&self) {
        *self.peripheral_inventory_cache.write().await = None;
    }
}
