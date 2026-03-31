use crate::http::AppState;
use crate::http::peripherals::{PeripheralErrors, PeripheralInventory, UsbPeripheral, derive_lighting_status, list_usb_sysfs, map_fan_status, map_sensor_inventory};
use crate::ipc::peripherals::SensorsConnection;
use std::sync::Arc;
use std::time::Instant;
use tracing::warn;

use super::{HardwareReadModelState, INVENTORY_REFRESH_MIN, peripheral_inventory_cache_ttl, sensor_ipc_timeout, sensor_refresh_timeout};

#[derive(Clone)]
pub(super) struct PeripheralInventoryCacheEntry {
    pub(super) fetched_at: Instant,
    pub(super) revision: u64,
    pub(super) payload: PeripheralInventory,
}

impl HardwareReadModelState {
    pub async fn load_peripheral_inventory_snapshot(&self, state: &AppState) -> Result<(PeripheralInventory, u64), String> {
        let ttl = peripheral_inventory_cache_ttl();
        if ttl != std::time::Duration::from_millis(0)
            && let Some(entry) = self.peripheral_inventory_cache.read().await.clone()
            && entry.fetched_at.elapsed() < ttl
        {
            self.peripheral_inventory_stats.record_hit();
            return Ok((entry.payload, entry.revision));
        }

        self.peripheral_inventory_stats.record_miss();
        let _refresh_guard = self.peripheral_inventory_refresh_lock.lock().await;
        if ttl != std::time::Duration::from_millis(0)
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
        let (cameras, _) = self.cached_discover_cameras_snapshot(state).await?;
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
