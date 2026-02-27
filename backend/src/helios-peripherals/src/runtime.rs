mod session;
mod shutdown;

use std::sync::Arc;
use std::time::Instant;

use futures::FutureExt;
use tokio::fs;
use tokio::net::UnixListener;
use tokio::task::JoinSet;
use tokio::time::sleep;
use tokio::time::{Duration, timeout};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::config::SensorsConfig;
use crate::dto::{LightingColor, LightingCommand, SensorInventory};
use crate::error::{Error, Result};
use crate::service::SensorsService;
use crate::usb_proxy;

use self::session::handle_connection;
use self::shutdown::wait_for_shutdown;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeState {
    Idle,
    Starting,
    Running,
    Stopping,
}

pub struct SensorsRuntime {
    config: Arc<SensorsConfig>,
    shutdown: CancellationToken,
    state: RuntimeState,
    service: Option<Arc<SensorsService>>,
    started_at: Instant,
}

impl Default for SensorsRuntime {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
struct BootLightingConfig {
    enabled: bool,
    color: LightingColor,
    step_delay: Duration,
    hold_delay: Duration,
}

impl BootLightingConfig {
    fn from_env() -> Self {
        Self {
            enabled: env_bool("HELIOS_LED_BOOT_ENABLE", true),
            color: env_color("HELIOS_LED_BOOT_COLOR", LightingColor { r: 160, g: 0, b: 255, w: 0 }),
            step_delay: Duration::from_millis(env_u64("HELIOS_LED_BOOT_STEP_MS", 80).max(10)),
            hold_delay: Duration::from_millis(env_u64("HELIOS_LED_BOOT_HOLD_MS", 200)),
        }
    }
}

impl SensorsRuntime {
    pub fn new() -> Self {
        Self::from_config(SensorsConfig::default())
    }

    pub fn from_config(config: SensorsConfig) -> Self {
        Self { config: Arc::new(config), shutdown: CancellationToken::new(), state: RuntimeState::Idle, service: None, started_at: Instant::now() }
    }

    pub fn config(&self) -> &SensorsConfig {
        &self.config
    }

    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    pub fn request_shutdown(&self) {
        self.shutdown.cancel();
    }

    pub async fn start(&mut self) -> Result<()> {
        if matches!(self.state, RuntimeState::Starting | RuntimeState::Running) {
            return Err(Error::InvalidState("sensors runtime already running".into()));
        }

        self.config.validate()?;
        info!("sensors runtime starting");
        self.state = RuntimeState::Starting;
        self.started_at = Instant::now();

        let service = Arc::new(SensorsService::new(Arc::clone(&self.config), self.shutdown.child_token()));
        let initial_inventory = match timeout(Duration::from_secs(5), service.discover_with_options(true, true)).await {
            Ok(Ok(inv)) => inv,
            Ok(Err(err)) => {
                warn!(%err, "initial sensor discovery failed; continuing without inventory to bring IPC online");
                SensorInventory { sensors: Vec::new() }
            }
            Err(_) => {
                warn!("initial sensor discovery timed out; continuing without inventory to bring IPC online");
                SensorInventory { sensors: Vec::new() }
            }
        };
        info!(count = initial_inventory.sensors.len(), "loaded initial sensor inventory");
        self.service = Some(service.clone());
        spawn_boot_lighting_animation(service.clone(), self.shutdown.child_token());

        let (fan_res, power_res, imu_res) = tokio::join!(service.fan_status(), service.start_power(), service.start_imu());
        if let Err(err) = fan_res {
            warn!(%err, "fan controller failed to start");
        }
        if let Err(err) = power_res {
            warn!(%err, "power runtime failed to start");
        }
        if let Err(err) = imu_res {
            warn!(%err, "IMU runtime failed to start");
        }

        usb_proxy::spawn(self.shutdown.child_token());

        if let Some(parent) = self.config.socket_path().parent() {
            fs::create_dir_all(parent).await?;
        }
        if fs::metadata(self.config.socket_path()).await.is_ok() {
            fs::remove_file(self.config.socket_path()).await?;
        }

        let listener = UnixListener::bind(self.config.socket_path())?;
        self.state = RuntimeState::Running;
        info!(path = %self.config.socket_path().display(), "sensors runtime listening");

        let mut tasks = JoinSet::new();
        let shutdown_future = wait_for_shutdown(self.shutdown.clone()).fuse();
        tokio::pin!(shutdown_future);

        let shutdown_reason: &'static str = loop {
            tokio::select! {
                reason = &mut shutdown_future => break reason?,
                accept = listener.accept() => {
                    match accept {
                        Ok((stream, _addr)) => {
                            let config = Arc::clone(&self.config);
                            let shutdown = self.shutdown.child_token();
                            let service = Arc::clone(&service);
                            let started_at = self.started_at;
                            tasks.spawn(handle_connection(stream, config, shutdown, service, started_at));
                        }
                        Err(err) => {
                            error!(%err, "failed to accept sensors client");
                        }
                    }
                }
            }
        };

        self.state = RuntimeState::Stopping;
        info!(reason = shutdown_reason, "sensors runtime stopping");
        self.shutdown.cancel();

        while let Some(result) = tasks.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(err)) => warn!(%err, "sensors client session ended with error"),
                Err(join_err) => warn!(%join_err, "sensors client task panicked"),
            }
        }

        drop(listener);
        if let Err(err) = fs::remove_file(self.config.socket_path()).await
            && err.kind() != std::io::ErrorKind::NotFound
        {
            warn!(%err, "failed to remove sensors socket");
        }

        let (power_res, imu_res) = tokio::join!(service.stop_power(), service.stop_imu());
        if let Err(err) = power_res {
            warn!(%err, "failed to stop power runtime");
        }
        if let Err(err) = imu_res {
            warn!(%err, "failed to stop IMU runtime");
        }

        self.state = RuntimeState::Idle;
        self.shutdown = CancellationToken::new();
        self.service = None;
        info!("sensors runtime stopped");
        Ok(())
    }
}

fn spawn_boot_lighting_animation(service: Arc<SensorsService>, shutdown: CancellationToken) {
    let cfg = BootLightingConfig::from_env();
    if !cfg.enabled {
        return;
    }
    if !service.led_config().enabled {
        return;
    }
    let led_count = service.led_config().count as usize;
    if led_count == 0 {
        return;
    }
    let brightness = service.led_config().brightness.unwrap_or(0xFF);
    tokio::spawn(async move {
        let mut first_frame = true;
        for filled in 1..=led_count {
            if shutdown.is_cancelled() {
                return;
            }
            let mut frame = vec![LightingColor::default(); led_count];
            for led in frame.iter_mut().take(filled) {
                *led = cfg.color.clone();
            }
            let command = LightingCommand { frame: Some(frame), brightness: first_frame.then_some(brightness), animation: None };
            let _ = service.lighting_command(command).await;
            first_frame = false;
            if wait_or_cancel(cfg.step_delay, &shutdown).await {
                return;
            }
        }
        if wait_or_cancel(cfg.hold_delay, &shutdown).await {
            return;
        }

        if triple_blink(&service, led_count, &cfg.color, brightness, &shutdown).await {
            return;
        }

        let idle =
            LightingCommand { frame: None, brightness: Some(brightness), animation: Some(crate::dto::LightingAnimation::Pulse { color: cfg.color.clone(), low: 60, high: 200, period_ms: 2400 }) };
        let _ = service.lighting_command(idle).await;
    });
}

async fn triple_blink(service: &SensorsService, led_count: usize, color: &LightingColor, brightness: u8, shutdown: &CancellationToken) -> bool {
    let on_delay = Duration::from_millis(140);
    let off_delay = Duration::from_millis(120);
    let frame_on = vec![color.clone(); led_count];
    for _ in 0..3 {
        if shutdown.is_cancelled() {
            return true;
        }
        let _ = service.lighting_command(LightingCommand { frame: Some(frame_on.clone()), brightness: Some(brightness), animation: None }).await;
        if wait_or_cancel(on_delay, shutdown).await {
            return true;
        }
        let _ = service.lighting_command(LightingCommand { frame: Some(Vec::new()), brightness: Some(0), animation: None }).await;
        if wait_or_cancel(off_delay, shutdown).await {
            return true;
        }
    }
    false
}

async fn wait_or_cancel(delay: Duration, shutdown: &CancellationToken) -> bool {
    let stop = shutdown.clone();
    tokio::select! {
        _ = sleep(delay) => false,
        _ = stop.cancelled() => true,
    }
}

fn env_bool(key: &str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(value) => matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"),
        Err(_) => default,
    }
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key).ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(default)
}

fn env_color(key: &str, default: LightingColor) -> LightingColor {
    let Ok(raw) = std::env::var(key) else {
        return default;
    };
    let parts: Vec<_> = raw.split(',').map(|part| part.trim()).collect();
    if parts.len() < 3 {
        return default;
    }
    let parse = |idx| parts.get(idx).and_then(|v: &&str| v.parse::<u8>().ok());
    let Some(r) = parse(0) else {
        return default;
    };
    let Some(g) = parse(1) else {
        return default;
    };
    let Some(b) = parse(2) else {
        return default;
    };
    let w = parse(3).unwrap_or(default.w);
    LightingColor { r, g, b, w }
}
