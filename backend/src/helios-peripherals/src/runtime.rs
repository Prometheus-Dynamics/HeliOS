mod session;
mod shutdown;

use std::sync::Arc;
use std::time::Instant;

use futures::FutureExt;
use lib_sensors::led_config::{DEFAULT_ANIMATION_EVENT_STARTUP, DEFAULT_ANIMATION_EVENT_STARTUP_IDLE, LedConfig};
use serde::Deserialize;
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

const LED_ANIMATIONS_PATH: &str = "/var/lib/helios/led-animations.json";
const LEGACY_LED_ANIMATIONS_PATH: &str = "/etc/helios/led-animations.json";

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

#[derive(Debug, Clone, Deserialize, Default)]
struct StoredAnimationDoc {
    #[serde(default)]
    animations: Vec<StoredAnimationEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct StoredAnimationEntry {
    name: String,
    #[serde(default)]
    command: LightingCommand,
    #[serde(default)]
    sequence: Vec<StoredAnimationFrame>,
}

#[derive(Debug, Clone, Deserialize)]
struct StoredAnimationFrame {
    #[serde(default)]
    frame: Vec<LightingColor>,
    #[serde(default)]
    duration_ms: u32,
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
    let led_config = service.led_config().clone();
    tokio::spawn(async move {
        if run_configured_startup_animation(&service, &led_config, &shutdown).await {
            return;
        }

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

        if try_apply_default_event_command(&service, &led_config, DEFAULT_ANIMATION_EVENT_STARTUP_IDLE).await {
            return;
        }

        let idle =
            LightingCommand { frame: None, brightness: Some(brightness), animation: Some(crate::dto::LightingAnimation::Pulse { color: cfg.color.clone(), low: 60, high: 200, period_ms: 2400 }) };
        let _ = service.lighting_command(idle).await;
    });
}

async fn run_configured_startup_animation(service: &SensorsService, led_config: &LedConfig, shutdown: &CancellationToken) -> bool {
    let Some(animation_name) = led_config.animation_for_event(DEFAULT_ANIMATION_EVENT_STARTUP).map(ToOwned::to_owned) else {
        return false;
    };
    let doc = load_stored_animation_doc().await;

    if let Some(sequence) = stored_sequence_for_name(&doc, &animation_name) {
        for frame in sequence {
            if shutdown.is_cancelled() {
                return true;
            }
            let command = LightingCommand { frame: Some(frame.frame), brightness: led_config.brightness, animation: None };
            if let Err(err) = service.lighting_command(command).await {
                warn!(
                    %err,
                    event = DEFAULT_ANIMATION_EVENT_STARTUP,
                    animation = %animation_name,
                    "failed to apply configured startup sequence frame"
                );
            }
            let wait_ms = u64::from(frame.duration_ms.max(20));
            if wait_or_cancel(Duration::from_millis(wait_ms), shutdown).await {
                return true;
            }
        }
        let _ = try_apply_default_event_command(service, led_config, DEFAULT_ANIMATION_EVENT_STARTUP_IDLE).await;
        return true;
    }

    if let Some(command) = stored_command_for_name(&doc, &animation_name, led_config.brightness) {
        if let Err(err) = service.lighting_command(command).await {
            warn!(
                %err,
                event = DEFAULT_ANIMATION_EVENT_STARTUP,
                animation = %animation_name,
                "failed to apply configured startup animation command"
            );
        }
        let _ = try_apply_default_event_command(service, led_config, DEFAULT_ANIMATION_EVENT_STARTUP_IDLE).await;
        return true;
    }

    warn!(event = DEFAULT_ANIMATION_EVENT_STARTUP, animation = %animation_name, "configured default animation was not found; using built-in startup animation");
    false
}

async fn try_apply_default_event_command(service: &SensorsService, led_config: &LedConfig, event: &str) -> bool {
    let Some(animation_name) = led_config.animation_for_event(event).map(ToOwned::to_owned) else {
        return false;
    };
    let doc = load_stored_animation_doc().await;
    let Some(command) = stored_command_for_name(&doc, &animation_name, led_config.brightness) else {
        warn!(event, animation = %animation_name, "configured default animation was not found");
        return false;
    };
    if let Err(err) = service.lighting_command(command).await {
        warn!(%err, event, animation = %animation_name, "failed to apply configured default animation");
    }
    true
}

async fn load_stored_animation_doc() -> StoredAnimationDoc {
    for path in [LED_ANIMATIONS_PATH, LEGACY_LED_ANIMATIONS_PATH] {
        match fs::read_to_string(path).await {
            Ok(raw) => return serde_json::from_str::<StoredAnimationDoc>(&raw).unwrap_or_default(),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return StoredAnimationDoc::default(),
        }
    }
    StoredAnimationDoc::default()
}

fn find_stored_animation_entry<'a>(doc: &'a StoredAnimationDoc, name: &str) -> Option<&'a StoredAnimationEntry> {
    let target = name.trim();
    if target.is_empty() {
        return None;
    }
    doc.animations.iter().find(|entry| entry.name.trim().eq_ignore_ascii_case(target))
}

fn stored_command_for_name(doc: &StoredAnimationDoc, name: &str, fallback_brightness: Option<u8>) -> Option<LightingCommand> {
    let entry = find_stored_animation_entry(doc, name)?;
    if entry.command.frame.is_some() || entry.command.animation.is_some() {
        return Some(LightingCommand { frame: entry.command.frame.clone(), brightness: entry.command.brightness.or(fallback_brightness), animation: entry.command.animation.clone() });
    }
    entry.sequence.first().map(|frame| LightingCommand { frame: Some(frame.frame.clone()), brightness: entry.command.brightness.or(fallback_brightness), animation: None })
}

fn stored_sequence_for_name(doc: &StoredAnimationDoc, name: &str) -> Option<Vec<StoredAnimationFrame>> {
    let entry = find_stored_animation_entry(doc, name)?;
    if entry.sequence.is_empty() { None } else { Some(entry.sequence.clone()) }
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
