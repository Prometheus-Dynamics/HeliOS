use crate::api_observability::{ApiCacheMetric, CacheMetricCounters};
use crate::http::AppState;
use crate::http::peripherals::{PeripheralErrors, PeripheralInventory, UsbPeripheral, derive_lighting_status, list_usb_sysfs, map_fan_status, map_sensor_inventory, safe_discover_cameras};
use crate::ipc::peripherals::SensorsConnection;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};
use tokio::task;
use tracing::warn;

#[derive(Clone)]
struct PeripheralInventoryCacheEntry {
    fetched_at: Instant,
    revision: u64,
    payload: PeripheralInventory,
}

struct CachedDiscovery {
    result: helios_engine::capture::DiscoveryResult,
    at: Instant,
    revision: u64,
}

const INVENTORY_REFRESH_MIN: Duration = Duration::from_secs(10);

pub struct HardwareReadModelState {
    camera_cache: Mutex<Option<CachedDiscovery>>,
    inventory_refresh_at: Mutex<Instant>,
    peripheral_inventory_cache: RwLock<Option<PeripheralInventoryCacheEntry>>,
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

fn peripheral_inventory_cache_ttl() -> Duration {
    static VALUE: OnceLock<Duration> = OnceLock::new();
    *VALUE.get_or_init(|| read_timeout_env("HELIOS_PERIPHERALS_CACHE_MS", 1_000, 0, 10_000))
}

fn sensor_ipc_timeout() -> Duration {
    static VALUE: OnceLock<Duration> = OnceLock::new();
    *VALUE.get_or_init(|| read_timeout_env("HELIOS_PERIPHERALS_TIMEOUT_MS", 1_500, 250, 15_000))
}

fn camera_discovery_timeout() -> Duration {
    static VALUE: OnceLock<Duration> = OnceLock::new();
    *VALUE.get_or_init(|| read_timeout_env("HELIOS_CAMERA_DISCOVERY_TIMEOUT_MS", 1_500, 250, 20_000))
}

fn sensor_refresh_timeout() -> Duration {
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

    pub async fn load_peripheral_inventory_snapshot(&self, state: &AppState) -> Result<(PeripheralInventory, u64), String> {
        let ttl = peripheral_inventory_cache_ttl();
        if ttl != Duration::from_millis(0)
            && let Some(entry) = self.peripheral_inventory_cache.read().await.clone()
            && entry.fetched_at.elapsed() < ttl
        {
            self.peripheral_inventory_stats.record_hit();
            return Ok((entry.payload, entry.revision));
        }

        self.peripheral_inventory_stats.record_miss();
        let _refresh_guard = self.peripheral_inventory_refresh_lock.lock().await;
        if ttl != Duration::from_millis(0)
            && let Some(entry) = self.peripheral_inventory_cache.read().await.clone()
            && entry.fetched_at.elapsed() < ttl
        {
            self.peripheral_inventory_stats.record_hit();
            return Ok((entry.payload, entry.revision));
        }

        let payload = self.build_peripheral_inventory(state).await?;
        let revision = self.peripheral_inventory_stats.record_refresh();
        *self.peripheral_inventory_cache.write().await = Some(PeripheralInventoryCacheEntry { fetched_at: Instant::now(), revision, payload: payload.clone() });
        Ok((payload, revision))
    }

    async fn build_peripheral_inventory(&self, state: &AppState) -> Result<PeripheralInventory, String> {
        let mut errors = PeripheralErrors { cameras: Vec::new(), i2c: Vec::new(), usb: Vec::new(), fan: Vec::new(), lighting: Vec::new() };
        let (cameras, _) = self.cached_discover_cameras_snapshot().await?;
        let usb = list_usb_sysfs();
        let (sensors, i2c, fan, lighting) = match state.ensure_sensors().await {
            Some(sensors) => {
                let (inventory_res, i2c_res, fan_res) = tokio::join!(
                    tokio::time::timeout(sensor_ipc_timeout(), sensors.inventory()),
                    tokio::time::timeout(sensor_ipc_timeout(), sensors.i2c_inventory()),
                    tokio::time::timeout(sensor_ipc_timeout(), sensors.fan_status())
                );

                let mut inventory = match inventory_res {
                    Ok(Ok(Ok(inv))) => Some(inv),
                    Ok(Ok(Err(reason))) => {
                        warn!(%reason, "sensor inventory request rejected");
                        None
                    }
                    Ok(Err(err)) => {
                        warn!(%err, "sensor inventory request failed");
                        None
                    }
                    Err(_) => {
                        state.invalidate_sensors().await;
                        warn!("sensor inventory request timed out");
                        None
                    }
                };

                if inventory.as_ref().is_none_or(|inv| inventory_needs_refresh(inv, &usb))
                    && self.allow_inventory_refresh().await
                    && let Some(refreshed) = refresh_inventory(state, sensors.clone()).await
                {
                    inventory = Some(refreshed);
                }

                let sensor_peripherals = inventory.as_ref().map(map_sensor_inventory).unwrap_or_default();
                let i2c_inv = match i2c_res {
                    Ok(Ok(Ok(inv))) => Some(inv),
                    Ok(Ok(Err(reason))) => {
                        errors.i2c.push(reason);
                        None
                    }
                    Ok(Err(err)) => {
                        errors.i2c.push(err.to_string());
                        None
                    }
                    Err(_) => {
                        state.invalidate_sensors().await;
                        errors.i2c.push("i2c inventory request timed out".into());
                        None
                    }
                };
                let fan = match fan_res {
                    Ok(Ok(Ok(status))) => Some(map_fan_status(status)),
                    Ok(Ok(Err(reason))) => {
                        errors.fan.push(reason);
                        None
                    }
                    Ok(Err(err)) => {
                        errors.fan.push(err.to_string());
                        None
                    }
                    Err(_) => {
                        state.invalidate_sensors().await;
                        errors.fan.push("fan status request timed out".into());
                        None
                    }
                };
                let lighting = inventory.as_ref().map(derive_lighting_status);
                (sensor_peripherals, i2c_inv, fan, lighting)
            }
            None => (Vec::new(), None, None, None),
        };

        errors.cameras = cameras.errors;
        Ok(PeripheralInventory { cameras: cameras.devices, sensors, errors, i2c, usb, lighting, fan })
    }

    pub async fn cached_discover_cameras_snapshot(&self) -> Result<(helios_engine::capture::DiscoveryResult, u64), String> {
        const TTL: Duration = Duration::from_secs(5);

        {
            let guard = self.camera_cache.lock().await;
            if let Some(cached) = guard.as_ref()
                && cached.at.elapsed() < TTL
            {
                self.camera_discovery_stats.record_hit();
                return Ok((cached.result.clone(), cached.revision));
            }
        }

        self.camera_discovery_stats.record_miss();
        let _refresh_guard = self.camera_refresh_lock.lock().await;
        {
            let guard = self.camera_cache.lock().await;
            if let Some(cached) = guard.as_ref()
                && cached.at.elapsed() < TTL
            {
                self.camera_discovery_stats.record_hit();
                return Ok((cached.result.clone(), cached.revision));
            }
        }

        let handle = task::spawn_blocking(safe_discover_cameras);
        let discovery = match tokio::time::timeout(camera_discovery_timeout(), handle).await {
            Ok(joined) => match joined {
                Ok(result) => result,
                Err(_) => Err("camera discovery task panicked".to_string()),
            },
            Err(_) => Err("camera discovery timed out".to_string()),
        };

        match discovery {
            Ok(result) => {
                let revision = self.camera_discovery_stats.record_refresh();
                let mut guard = self.camera_cache.lock().await;
                *guard = Some(CachedDiscovery { result: result.clone(), at: Instant::now(), revision });
                Ok((result, revision))
            }
            Err(err) => {
                let mut fallback = {
                    let guard = self.camera_cache.lock().await;
                    guard.as_ref().map(|entry| entry.result.clone()).unwrap_or_else(|| helios_engine::capture::DiscoveryResult { devices: Vec::new(), errors: Vec::new() })
                };
                if fallback.errors.iter().all(|entry| entry != &err) {
                    fallback.errors.push(err);
                    if fallback.errors.len() > 5 {
                        let drain = fallback.errors.len() - 5;
                        fallback.errors.drain(0..drain);
                    }
                }
                let revision = self.camera_discovery_stats.record_refresh();
                let mut guard = self.camera_cache.lock().await;
                *guard = Some(CachedDiscovery { result: fallback.clone(), at: Instant::now(), revision });
                Ok((fallback, revision))
            }
        }
    }

    pub async fn allow_inventory_refresh(&self) -> bool {
        let mut guard = self.inventory_refresh_at.lock().await;
        if guard.elapsed() < INVENTORY_REFRESH_MIN {
            return false;
        }
        *guard = Instant::now();
        true
    }
}

async fn refresh_inventory(state: &AppState, sensors: Arc<SensorsConnection>) -> Option<helios_peripherals::dto::SensorInventory> {
    match tokio::time::timeout(sensor_refresh_timeout(), sensors.discover(true)).await {
        Ok(Ok(Ok(inv))) => return Some(inv),
        Ok(Ok(Err(reason))) => {
            warn!(%reason, "sensor inventory refresh rejected");
            return None;
        }
        Ok(Err(err)) => {
            warn!(%err, "sensor inventory refresh failed");
        }
        Err(_) => {
            warn!("sensor inventory refresh timed out");
        }
    }

    state.invalidate_sensors().await;
    let sensors = state.ensure_sensors().await?;
    match tokio::time::timeout(sensor_refresh_timeout(), sensors.discover(true)).await {
        Ok(Ok(Ok(inv))) => Some(inv),
        Ok(Ok(Err(reason))) => {
            warn!(%reason, "sensor inventory refresh rejected after reconnect");
            None
        }
        Ok(Err(err)) => {
            warn!(%err, "sensor inventory refresh failed after reconnect");
            None
        }
        Err(_) => {
            warn!("sensor inventory refresh timed out after reconnect");
            None
        }
    }
}

fn inventory_needs_refresh(inv: &helios_peripherals::dto::SensorInventory, usb: &[UsbPeripheral]) -> bool {
    if inv.sensors.is_empty() {
        return true;
    }
    if usb_has_coral(usb) && !inventory_has_coral(inv) {
        return true;
    }
    if !inventory_has_lighting(inv) {
        return true;
    }
    false
}

fn inventory_has_coral(inv: &helios_peripherals::dto::SensorInventory) -> bool {
    inv.sensors.iter().any(|sensor| sensor.backend.eq_ignore_ascii_case("coral"))
}

fn inventory_has_lighting(inv: &helios_peripherals::dto::SensorInventory) -> bool {
    inv.sensors.iter().any(|sensor| {
        let backend = sensor.backend.to_ascii_lowercase();
        backend.contains("led") || backend.contains("lighting")
    })
}

fn usb_has_coral(devices: &[UsbPeripheral]) -> bool {
    devices.iter().any(|device| match device.kind.as_deref() {
        Some(kind) => kind.eq_ignore_ascii_case("coral"),
        None => false,
    })
}
