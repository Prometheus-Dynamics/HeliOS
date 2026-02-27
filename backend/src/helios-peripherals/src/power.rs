use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use futures::future::join_all;
use lib_sensors::drivers::{
    ina226::{Ina226, Ina226Config},
    ina238::{Ina238, Ina238Config},
    ina260::Ina260,
};
use lib_sensors::power::PowerBackend;
use lib_sensors::sensor_config::SensorDeviceCfg;
use linux_embedded_hal::I2cdev;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::{MissedTickBehavior, interval};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::error::{Error, Result};
use crate::service::SensorsService;

fn power_poll_interval() -> Duration {
    let default_ms: u64 = 100;
    let ms = std::env::var("HELIOS_POWER_POLL_INTERVAL_MS").ok().and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(default_ms).clamp(20, 10_000);
    Duration::from_millis(ms)
}

struct PowerSource {
    label: String,
    bus: u32,
    address: u8,
    scale: f32,
    offset: f32,
    current_scale: f32,
    current_offset: f32,
    backend: Arc<StdMutex<Box<dyn PowerBackend + Send>>>,
}

impl PowerSource {
    fn new(cfg: &SensorDeviceCfg) -> Option<Self> {
        let path = format!("/dev/i2c-{}", cfg.bus);
        let dev = I2cdev::new(path.clone()).ok()?;
        let driver_label = cfg.driver.clone();
        let backend: Box<dyn PowerBackend + Send> = match driver_label.to_ascii_lowercase().as_str() {
            driver if driver.contains("ina238") => {
                let config = Ina238Config { address: cfg.address, shunt_resistance: cfg.shunt_resistance.unwrap_or_else(|| Ina238Config::default().shunt_resistance) };
                Box::new(Ina238::new_with_config(dev, config).ok()?)
            }
            driver if driver.contains("ina226") => {
                let config = Ina226Config {
                    address: cfg.address,
                    shunt_resistance: cfg.shunt_resistance.unwrap_or_else(|| Ina226Config::default().shunt_resistance),
                    max_current: cfg.max_current.unwrap_or_else(|| Ina226Config::default().max_current),
                };
                Box::new(Ina226::new_with_config(dev, config).ok()?)
            }
            driver if driver.contains("ina260") => Box::new(Ina260::new(dev).ok()?),
            _ => return None,
        };

        let scale = cfg.bus_scale.unwrap_or(1.0);
        let offset = cfg.bus_offset.unwrap_or(0.0);
        let current_scale = cfg.current_scale.unwrap_or(1.0);
        let current_offset = cfg.current_offset.unwrap_or(0.0);

        let label = format!("{} on i2c-{} (0x{:02X})", driver_label, cfg.bus, cfg.address);
        Some(Self { label, bus: cfg.bus, address: cfg.address, scale, offset, current_scale, current_offset, backend: Arc::new(StdMutex::new(backend)) })
    }

    async fn sample(&self) -> Result<PowerReading> {
        let backend = Arc::clone(&self.backend);
        let label = self.label.clone();
        let bus = self.bus;
        let address = self.address;
        let scale = self.scale;
        let offset = self.offset;
        let current_scale = self.current_scale;
        let current_offset = self.current_offset;

        tokio::task::spawn_blocking(move || {
            let mut guard = backend.lock().map_err(|_| Error::InvalidState("power backend mutex poisoned".into()))?;
            let volts = guard.bus_voltage()? * scale + offset;
            let amps = guard.current()? * current_scale + current_offset;
            // Prefer derived power so sign and scaling always match the post-scale V/I values.
            let watts = volts * amps;
            // Only apply sign flip to shunt voltage (avoid accidentally "calibrating" shunt volts magnitude).
            let shunt = guard.shunt_voltage().unwrap_or(0.0) * current_scale.signum();
            Ok(PowerReading { label, bus, address, volts, amps, watts, shunt })
        })
        .await?
    }
}

#[derive(Debug, Clone)]
pub struct PowerReading {
    pub label: String,
    pub bus: u32,
    pub address: u8,
    pub volts: f32,
    pub amps: f32,
    pub watts: f32,
    pub shunt: f32,
}

pub struct PowerRuntime {
    shutdown: CancellationToken,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl PowerRuntime {
    pub async fn spawn(service: &Arc<SensorsService>, shutdown: CancellationToken) -> Result<Option<Self>> {
        let config = service.config();
        let devices = lib_sensors::sensor_config::load_sensor_devices(&config.config_paths());
        let sources: Vec<_> = devices.iter().filter_map(PowerSource::new).collect();
        if sources.is_empty() {
            info!("Power runtime not started because no INA devices were configured");
            return Ok(None);
        }

        let service_handle = Arc::downgrade(service);
        let shutdown_for_task = shutdown.clone();
        let handle = tokio::spawn(async move {
            run_power_loop(service_handle, sources, shutdown_for_task).await;
        });

        Ok(Some(Self { shutdown, handle: Mutex::new(Some(handle)) }))
    }

    pub async fn stop(&self) -> Result<()> {
        self.shutdown.cancel();
        if let Some(handle) = self.handle.lock().await.take() {
            handle.await?;
        }
        Ok(())
    }
}

async fn run_power_loop(service: std::sync::Weak<SensorsService>, sources: Vec<PowerSource>, shutdown: CancellationToken) {
    let poll_interval = power_poll_interval();
    let poll_interval_ms = poll_interval.as_millis().min(u128::from(u64::MAX)) as u64;
    info!(sources = sources.len(), poll_interval_ms, "power runtime started");
    if poll_interval_ms < 100 {
        warn!(poll_interval_ms, "power polling interval is very aggressive and may increase idle CPU usage");
    }

    let mut ticker = interval(poll_interval);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = shutdown.cancelled() => break,
            _ = ticker.tick() => {
                let results = join_all(sources.iter().map(|source| {
                    let label = source.label.clone();
                    async move { (label, source.sample().await) }
                }))
                .await;

                let mut readings = Vec::with_capacity(results.len());
                let mut errors = Vec::new();
                for (label, result) in results {
                    match result {
                        Ok(sample) => readings.push(sample),
                        Err(err) => errors.push(format!("{label}: {err}")),
                    }
                }

                if let Some(service) = service.upgrade() {
                    if readings.is_empty() && !errors.is_empty() {
                        service.apply_power_error(errors.join("; ")).await;
                    } else {
                        service.apply_power_samples(readings, errors).await;
                    }
                }
            }
        }
    }
}
