use std::{sync::Arc, time::Duration};

use helios_peripherals::dto::{LightingAnimation, LightingCommand};
use lib_led_animations::{LED_ANIMATIONS_PATH, command_for_animation_name, load_led_animations};
use lib_sensors::led_config::{self, DEFAULT_ANIMATION_EVENT_ENGINE_CRASH, DEFAULT_ANIMATION_EVENT_REBOOT, DEFAULT_ANIMATION_EVENT_UPDATE, DEFAULT_ANIMATION_EVENT_UPDATE_ERROR};
use tokio::time::sleep;
use tracing::warn;

use crate::ipc::IpcHandles;

use super::config::{EngineCrashLightingConfig, UpdateLightingConfig};
use super::state::UpdateLedMode;

pub(super) async fn apply_update_lighting(handles: &Arc<IpcHandles>, cfg: &UpdateLightingConfig, mode: UpdateLedMode) -> bool {
    if mode == UpdateLedMode::Idle {
        return true;
    }

    let fallback_brightness = match mode {
        UpdateLedMode::Updating | UpdateLedMode::Rebooting => Some(cfg.brightness),
        UpdateLedMode::Error => cfg.error_enabled.then_some(cfg.error_brightness),
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

pub(super) async fn flash_engine_crash_led(handles: &Arc<IpcHandles>, cfg: &EngineCrashLightingConfig) {
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
        warn!(
            event = event_key,
            animation = %animation_name,
            "configured default animation was not found"
        );
        return false;
    };
    let Some(sensors) = handles.ensure_sensors().await else {
        warn!(event = event_key, "peripherals IPC unavailable");
        return false;
    };
    match sensors.lighting_command(command).await {
        Ok(Ok(())) => true,
        Ok(Err(reason)) => {
            warn!(
                event = event_key,
                %reason,
                "peripheral rejected configured default animation"
            );
            false
        }
        Err(err) => {
            warn!(
                event = event_key,
                %err,
                "failed to apply configured default animation"
            );
            false
        }
    }
}
