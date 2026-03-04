use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use chrono::Utc;
use helios_peripherals::dto::{LightingAnimation, LightingColor, LightingCommand};
use helios_updater::client::UpdaterSession;
use helios_updater::ipc::{UpdateStage, UpdateState, UpdaterCommand, UpdaterEvent};
use lib_ipc::types::CommandId;
use lib_led_animations::{LED_ANIMATIONS_PATH, command_for_animation_name, load_led_animations};
use lib_sensors::led_config::{self, DEFAULT_ANIMATION_EVENT_ENGINE_CRASH, DEFAULT_ANIMATION_EVENT_REBOOT, DEFAULT_ANIMATION_EVENT_UPDATE, DEFAULT_ANIMATION_EVENT_UPDATE_ERROR};
use tokio::time::sleep;
use tracing::{info, warn};

use crate::ipc::IpcHandles;
use crate::ipc::updater::UpdaterConnection;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdateLedMode {
    Idle = 0,
    Updating = 1,
    Error = 2,
    Rebooting = 3,
}

impl UpdateLedMode {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Updating,
            2 => Self::Error,
            3 => Self::Rebooting,
            _ => Self::Idle,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct UpdateLedState {
    mode: AtomicU8,
}

impl UpdateLedState {
    fn mode(&self) -> UpdateLedMode {
        UpdateLedMode::from_u8(self.mode.load(Ordering::Relaxed))
    }

    fn set_mode(&self, mode: UpdateLedMode) {
        self.mode.store(mode as u8, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone)]
struct UpdateLightingConfig {
    enabled: bool,
    color: LightingColor,
    speed_hz: f32,
    brightness: u8,
    error_enabled: bool,
    error_color: LightingColor,
    error_brightness: u8,
    error_flash_ms: u64,
    reboot_grace: Duration,
    retry_delay: Duration,
}

impl UpdateLightingConfig {
    fn from_env() -> Self {
        Self {
            enabled: env_bool("HELIOS_LED_UPDATE_ENABLE", true),
            color: env_color("HELIOS_LED_UPDATE_COLOR", LightingColor { r: 160, g: 0, b: 255, w: 0 }),
            speed_hz: env_f32("HELIOS_LED_UPDATE_SPEED_HZ", 10.0).max(0.5),
            brightness: env_u8("HELIOS_LED_UPDATE_BRIGHTNESS", 180),
            error_enabled: env_bool("HELIOS_LED_UPDATE_ERROR_ENABLE", true),
            error_color: env_color("HELIOS_LED_UPDATE_ERROR_COLOR", LightingColor { r: 255, g: 0, b: 0, w: 0 }),
            error_brightness: env_u8("HELIOS_LED_UPDATE_ERROR_BRIGHTNESS", 255),
            error_flash_ms: env_u64("HELIOS_LED_UPDATE_ERROR_FLASH_MS", 350).max(50),
            reboot_grace: Duration::from_secs(env_u64("HELIOS_LED_UPDATE_REBOOT_GRACE_SECS", 45).max(5)),
            retry_delay: Duration::from_secs(env_u64("HELIOS_LED_UPDATE_RETRY_SECS", 5).max(1)),
        }
    }
}

#[derive(Debug, Clone)]
struct EngineCrashLightingConfig {
    enabled: bool,
    color: LightingColor,
    brightness: u8,
    flash_ms: u64,
    poll_ms: u64,
}

impl EngineCrashLightingConfig {
    fn from_env() -> Self {
        Self {
            enabled: env_bool("HELIOS_LED_ENGINE_CRASH_ENABLE", true),
            color: env_color("HELIOS_LED_ENGINE_CRASH_COLOR", LightingColor { r: 255, g: 0, b: 0, w: 0 }),
            brightness: env_u8("HELIOS_LED_ENGINE_CRASH_BRIGHTNESS", 255),
            flash_ms: env_u64("HELIOS_LED_ENGINE_CRASH_FLASH_MS", 250).max(50),
            poll_ms: env_u64("HELIOS_LED_ENGINE_CRASH_POLL_MS", 1000).max(200),
        }
    }
}

pub(crate) fn spawn_update_led_task(handles: Arc<IpcHandles>) -> Arc<UpdateLedState> {
    let cfg = UpdateLightingConfig::from_env();
    let update_state = Arc::new(UpdateLedState::default());
    if !cfg.enabled {
        return update_state;
    }
    tokio::spawn(run_update_led_loop(handles, cfg, update_state.clone()));
    update_state
}

pub(crate) fn spawn_engine_crash_led_task(handles: Arc<IpcHandles>, update_state: Arc<UpdateLedState>) {
    let cfg = EngineCrashLightingConfig::from_env();
    if !cfg.enabled {
        return;
    }
    let update_cfg = UpdateLightingConfig::from_env();
    tokio::spawn(run_engine_crash_led_loop(handles, cfg, update_state, update_cfg));
}

async fn run_update_led_loop(handles: Arc<IpcHandles>, cfg: UpdateLightingConfig, update_state: Arc<UpdateLedState>) {
    let mut last_mode: Option<UpdateLedMode> = None;
    loop {
        let updater = { handles.updater.lock().await.clone() };
        let Some(updater) = updater else {
            sleep(cfg.retry_delay).await;
            continue;
        };

        let mut session = match updater.checkout_session().await {
            Ok(session) => session,
            Err(err) => {
                warn!(%err, "update lighting: failed to checkout updater session");
                sleep(cfg.retry_delay).await;
                continue;
            }
        };

        if let Err(err) = send_query_state(&updater, &mut session).await {
            warn!(%err, "update lighting: failed to query updater state");
        }

        loop {
            match session.next_event().await {
                Ok(Some(event)) => {
                    if let Some(mode) = update_mode_from_event(&event, cfg.reboot_grace)
                        && last_mode != Some(mode)
                    {
                        update_state.set_mode(mode);
                        if apply_update_lighting(&handles, &cfg, mode).await {
                            last_mode = Some(mode);
                        }
                    }
                }
                Ok(None) => {
                    info!("update lighting: updater session closed");
                    break;
                }
                Err(err) => {
                    warn!(%err, "update lighting: updater session error");
                    break;
                }
            }
        }

        sleep(cfg.retry_delay).await;
    }
}

async fn send_query_state(updater: &UpdaterConnection, session: &mut UpdaterSession) -> Result<(), String> {
    let command_id = CommandId::new();
    let command = UpdaterCommand::QueryState { command_id };
    session.send_command(updater.client.journal(), &command).await.map_err(|err| err.to_string())?;
    Ok(())
}

fn update_mode_from_event(event: &UpdaterEvent, reboot_grace: Duration) -> Option<UpdateLedMode> {
    match event {
        UpdaterEvent::StateSnapshot { active_update, .. } => Some(update_mode_from_state(active_update.as_ref(), reboot_grace)),
        UpdaterEvent::ApplyComplete { reboot_required, .. } => Some(if *reboot_required { UpdateLedMode::Rebooting } else { UpdateLedMode::Idle }),
        UpdaterEvent::StageProgress { .. } | UpdaterEvent::StageComplete { .. } | UpdaterEvent::ApplyScheduled { .. } => Some(UpdateLedMode::Updating),
        UpdaterEvent::RollbackTriggered { reason, .. } => Some(rollback_mode_from_reason(reason)),
        _ => None,
    }
}

fn rollback_mode_from_reason(reason: &str) -> UpdateLedMode {
    if reason.trim().eq_ignore_ascii_case("manual rollback") { UpdateLedMode::Idle } else { UpdateLedMode::Error }
}

fn update_mode_from_state(state: Option<&UpdateState>, reboot_grace: Duration) -> UpdateLedMode {
    match state.map(|s| &s.stage) {
        Some(UpdateStage::Idle) | None => UpdateLedMode::Idle,
        Some(UpdateStage::RolledBack) => {
            let has_error = state.and_then(|s| s.last_error.as_deref()).map(|reason| !reason.trim().is_empty() && !reason.trim().eq_ignore_ascii_case("manual rollback")).unwrap_or(false);
            if has_error { UpdateLedMode::Error } else { UpdateLedMode::Idle }
        }
        Some(UpdateStage::Complete) => state
            .and_then(|s| s.finished_at)
            .map(|finished_at| {
                let age = Utc::now().signed_duration_since(finished_at);
                if age.num_seconds() >= 0 && age.to_std().map(|d| d <= reboot_grace).unwrap_or(false) { UpdateLedMode::Rebooting } else { UpdateLedMode::Idle }
            })
            .unwrap_or(UpdateLedMode::Idle),
        Some(_) => UpdateLedMode::Updating,
    }
}

async fn apply_update_lighting(handles: &Arc<IpcHandles>, cfg: &UpdateLightingConfig, mode: UpdateLedMode) -> bool {
    if mode == UpdateLedMode::Idle {
        // Don't clear LEDs on idle; avoid turning off boot/idle animations.
        return true;
    }

    let fallback_brightness = match mode {
        UpdateLedMode::Updating | UpdateLedMode::Rebooting => Some(cfg.brightness),
        UpdateLedMode::Error => {
            if cfg.error_enabled {
                Some(cfg.error_brightness)
            } else {
                None
            }
        }
        UpdateLedMode::Idle => None,
    };
    if let Some(event_key) = default_event_key_for_mode(mode)
        && apply_configured_event_animation(handles, event_key, fallback_brightness).await
    {
        return true;
    }

    let Some(sensors) = handles.ensure_sensors().await else {
        warn!("update lighting: peripherals IPC unavailable");
        return false;
    };

    let command = match mode {
        UpdateLedMode::Idle => return true,
        UpdateLedMode::Updating | UpdateLedMode::Rebooting => {
            LightingCommand { frame: None, brightness: Some(cfg.brightness), animation: Some(LightingAnimation::Chase { color: cfg.color.clone(), speed_hz: cfg.speed_hz }) }
        }
        UpdateLedMode::Error => {
            if !cfg.error_enabled {
                return true;
            }
            let period_ms = (cfg.error_flash_ms.saturating_mul(2)).min(2000) as u32;
            LightingCommand { frame: None, brightness: Some(cfg.error_brightness), animation: Some(LightingAnimation::Pulse { color: cfg.error_color.clone(), low: 0, high: 255, period_ms }) }
        }
    };

    match sensors.lighting_command(command).await {
        Ok(Ok(())) => true,
        Ok(Err(reason)) => {
            warn!(%reason, "update lighting: peripheral rejected command");
            false
        }
        Err(err) => {
            warn!(%err, "update lighting: failed to send lighting command");
            false
        }
    }
}

async fn run_engine_crash_led_loop(handles: Arc<IpcHandles>, cfg: EngineCrashLightingConfig, update_state: Arc<UpdateLedState>, update_cfg: UpdateLightingConfig) {
    let mut last_seen = 0u64;
    loop {
        sleep(Duration::from_millis(cfg.poll_ms)).await;
        let Some(ts) = handles.engine.last_disconnect_ms() else {
            continue;
        };
        if ts <= last_seen {
            continue;
        }
        last_seen = ts;
        flash_engine_crash_led(&handles, &cfg).await;
        let mode = update_state.mode();
        if mode != UpdateLedMode::Idle {
            let _ = apply_update_lighting(&handles, &update_cfg, mode).await;
        }
    }
}

async fn flash_engine_crash_led(handles: &Arc<IpcHandles>, cfg: &EngineCrashLightingConfig) {
    if apply_configured_event_animation(handles, DEFAULT_ANIMATION_EVENT_ENGINE_CRASH, Some(cfg.brightness)).await {
        return;
    }

    let Some(sensors) = handles.ensure_sensors().await else {
        warn!("engine crash lighting: peripherals IPC unavailable");
        return;
    };

    let period_ms = (cfg.flash_ms.saturating_mul(2)).min(2000) as u32;
    let start = LightingCommand { frame: None, brightness: Some(cfg.brightness), animation: Some(LightingAnimation::Pulse { color: cfg.color.clone(), low: 0, high: 255, period_ms }) };
    let _ = sensors.lighting_command(start).await;
    sleep(Duration::from_millis(cfg.flash_ms)).await;
    let _ = sensors.lighting_command(LightingCommand { frame: Some(Vec::new()), brightness: None, animation: None }).await;
}

fn default_event_key_for_mode(mode: UpdateLedMode) -> Option<&'static str> {
    match mode {
        UpdateLedMode::Idle => None,
        UpdateLedMode::Updating => Some(DEFAULT_ANIMATION_EVENT_UPDATE),
        UpdateLedMode::Error => Some(DEFAULT_ANIMATION_EVENT_UPDATE_ERROR),
        UpdateLedMode::Rebooting => Some(DEFAULT_ANIMATION_EVENT_REBOOT),
    }
}

async fn apply_configured_event_animation(handles: &Arc<IpcHandles>, event_key: &str, fallback_brightness: Option<u8>) -> bool {
    let paths = led_config::default_paths();
    let Some(led_config) = led_config::load_led_config(&paths) else {
        return false;
    };
    let Some(animation_name) = led_config.animation_for_event(event_key).map(ToOwned::to_owned) else {
        return false;
    };
    let doc = load_led_animations(LED_ANIMATIONS_PATH).await;
    let Some(command) = command_for_animation_name(&doc, &animation_name, fallback_brightness) else {
        warn!(event = event_key, animation = %animation_name, "configured default animation was not found");
        return false;
    };
    let Some(sensors) = handles.ensure_sensors().await else {
        warn!(event = event_key, "peripherals IPC unavailable");
        return false;
    };
    match sensors.lighting_command(command).await {
        Ok(Ok(())) => true,
        Ok(Err(reason)) => {
            warn!(event = event_key, %reason, "peripheral rejected configured default animation");
            false
        }
        Err(err) => {
            warn!(event = event_key, %err, "failed to apply configured default animation");
            false
        }
    }
}

fn env_bool(key: &str, default: bool) -> bool {
    match std::env::var(key) {
        Ok(value) => matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"),
        Err(_) => default,
    }
}

fn env_u8(key: &str, default: u8) -> u8 {
    std::env::var(key).ok().and_then(|value| value.trim().parse::<u8>().ok()).unwrap_or(default)
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key).ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(default)
}

fn env_f32(key: &str, default: f32) -> f32 {
    std::env::var(key).ok().and_then(|value| value.trim().parse::<f32>().ok()).unwrap_or(default)
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

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn sample_state(stage: UpdateStage, last_error: Option<&str>) -> UpdateState {
        UpdateState { update_id: Uuid::new_v4(), stage, progress_percent: None, last_error: last_error.map(|value| value.to_string()), started_at: None, finished_at: None, artifacts: Vec::new() }
    }

    #[test]
    fn rollback_failure_maps_to_error_mode() {
        let event = UpdaterEvent::RollbackTriggered { update_id: Uuid::new_v4(), reason: "checksum verification failed".into() };
        assert_eq!(update_mode_from_event(&event, Duration::from_secs(45)), Some(UpdateLedMode::Error));
    }

    #[test]
    fn manual_rollback_does_not_map_to_error_mode() {
        let event = UpdaterEvent::RollbackTriggered { update_id: Uuid::new_v4(), reason: "manual rollback".into() };
        assert_eq!(update_mode_from_event(&event, Duration::from_secs(45)), Some(UpdateLedMode::Idle));
    }

    #[test]
    fn rolled_back_state_with_error_maps_to_error_mode() {
        let state = sample_state(UpdateStage::RolledBack, Some("artifact verification failed"));
        assert_eq!(update_mode_from_state(Some(&state), Duration::from_secs(45)), UpdateLedMode::Error);
    }
}
