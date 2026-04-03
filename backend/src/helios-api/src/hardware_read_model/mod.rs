mod discovery;
mod inventory;

use crate::api_observability::{ApiCacheMetric, CacheMetricCounters};
use lib_runtime_policy::HELIOS_API_HARDWARE_READ_MODEL_POLICY;
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

pub(super) fn peripheral_inventory_cache_ttl() -> Duration {
    static VALUE: OnceLock<Duration> = OnceLock::new();
    *VALUE.get_or_init(|| Duration::from_millis(HELIOS_API_HARDWARE_READ_MODEL_POLICY.resolve().peripherals_cache_ms))
}

pub(super) fn sensor_ipc_timeout() -> Duration {
    static VALUE: OnceLock<Duration> = OnceLock::new();
    *VALUE.get_or_init(|| Duration::from_millis(HELIOS_API_HARDWARE_READ_MODEL_POLICY.resolve().peripherals_timeout_ms))
}

pub(super) fn camera_discovery_timeout() -> Duration {
    static VALUE: OnceLock<Duration> = OnceLock::new();
    *VALUE.get_or_init(|| Duration::from_millis(HELIOS_API_HARDWARE_READ_MODEL_POLICY.resolve().camera_discovery_timeout_ms))
}

pub(super) fn sensor_refresh_timeout() -> Duration {
    static VALUE: OnceLock<Duration> = OnceLock::new();
    *VALUE.get_or_init(|| Duration::from_millis(HELIOS_API_HARDWARE_READ_MODEL_POLICY.resolve().peripherals_refresh_timeout_ms))
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
