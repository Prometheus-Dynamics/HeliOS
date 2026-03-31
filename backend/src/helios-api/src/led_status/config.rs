use std::time::Duration;

use helios_peripherals::dto::LightingColor;

#[derive(Debug, Clone)]
pub(super) struct UpdateLightingConfig {
    pub(super) enabled: bool,
    pub(super) color: LightingColor,
    pub(super) speed_hz: f32,
    pub(super) brightness: u8,
    pub(super) error_enabled: bool,
    pub(super) error_color: LightingColor,
    pub(super) error_brightness: u8,
    pub(super) error_flash_ms: u64,
    pub(super) reboot_grace: Duration,
    pub(super) retry_delay: Duration,
}

impl UpdateLightingConfig {
    pub(super) fn from_env() -> Self {
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
pub(super) struct EngineCrashLightingConfig {
    pub(super) enabled: bool,
    pub(super) color: LightingColor,
    pub(super) brightness: u8,
    pub(super) flash_ms: u64,
    pub(super) poll_ms: u64,
}

impl EngineCrashLightingConfig {
    pub(super) fn from_env() -> Self {
        Self {
            enabled: env_bool("HELIOS_LED_ENGINE_CRASH_ENABLE", true),
            color: env_color("HELIOS_LED_ENGINE_CRASH_COLOR", LightingColor { r: 255, g: 0, b: 0, w: 0 }),
            brightness: env_u8("HELIOS_LED_ENGINE_CRASH_BRIGHTNESS", 255),
            flash_ms: env_u64("HELIOS_LED_ENGINE_CRASH_FLASH_MS", 250).max(50),
            poll_ms: env_u64("HELIOS_LED_ENGINE_CRASH_POLL_MS", 1000).max(200),
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
    let parts = raw.split(',').map(|part| part.trim()).collect::<Vec<_>>();
    if parts.len() < 3 {
        return default;
    }
    let parse = |idx| parts.get(idx).copied().and_then(|value: &str| value.parse::<u8>().ok());
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
