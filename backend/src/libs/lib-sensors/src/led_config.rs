use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Default locations searched for LED configuration.
/// Prefer the on-device file, then fall back to the shared preset shipped with the repo.
pub const DEFAULT_LED_CONFIG_PATHS: &[&str] = &["/etc/helios/leds.toml", "configs/presets/common.toml"];

fn default_enabled() -> bool {
    true
}

fn default_gpio() -> u8 {
    13
}

fn default_count() -> u16 {
    16
}

fn default_color_order() -> String {
    "rgbw".to_string()
}

fn default_frequency_hz() -> u32 {
    800_000
}

fn default_brightness() -> Option<u8> {
    Some(128)
}

fn default_protocol() -> String {
    "sk6812-ec20".to_string()
}

fn default_use_pwm() -> bool {
    true
}

pub const DEFAULT_ANIMATION_EVENT_STARTUP: &str = "startup";
pub const DEFAULT_ANIMATION_EVENT_STARTUP_IDLE: &str = "startup_idle";
pub const DEFAULT_ANIMATION_EVENT_REBOOT: &str = "reboot";
pub const DEFAULT_ANIMATION_EVENT_UPDATE: &str = "update";
pub const DEFAULT_ANIMATION_EVENT_UPDATE_ERROR: &str = "update_error";
pub const DEFAULT_ANIMATION_EVENT_ENGINE_CRASH: &str = "engine_crash";

/// Declarative configuration for a single addressable LED chain.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ToSchema)]
pub struct LedConfig {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_gpio")]
    pub gpio: u8,
    #[serde(default = "default_count")]
    pub count: u16,
    #[serde(default = "default_color_order")]
    pub color_order: String,
    #[serde(default = "default_frequency_hz")]
    pub frequency_hz: u32,
    #[serde(default = "default_use_pwm")]
    pub use_pwm: bool,
    #[serde(default = "default_brightness")]
    pub brightness: Option<u8>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default = "default_protocol")]
    pub protocol: String,
    #[serde(default)]
    pub default_animations: BTreeMap<String, String>,
}

impl Default for LedConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            gpio: default_gpio(),
            count: default_count(),
            color_order: default_color_order(),
            frequency_hz: default_frequency_hz(),
            use_pwm: default_use_pwm(),
            brightness: default_brightness(),
            label: None,
            protocol: default_protocol(),
            default_animations: BTreeMap::new(),
        }
    }
}

impl LedConfig {
    #[must_use]
    pub fn animation_for_event(&self, event: &str) -> Option<&str> {
        let key = event.trim();
        if key.is_empty() {
            return None;
        }
        if let Some(value) = self.default_animations.get(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
        self.default_animations.iter().find_map(|(candidate_key, value)| {
            if candidate_key.trim().eq_ignore_ascii_case(key) {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed);
                }
            }
            None
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct LedDoc {
    #[serde(default)]
    leds: Option<LedConfig>,
}

/// Returns the default LED configuration search paths.
#[must_use]
pub fn default_paths() -> Vec<PathBuf> {
    DEFAULT_LED_CONFIG_PATHS.iter().map(PathBuf::from).collect()
}

/// Loads LED configuration from the first readable document in the provided paths.
#[must_use]
pub fn load_led_config<P>(paths: &[P]) -> Option<LedConfig>
where
    P: AsRef<Path>,
{
    for path in paths {
        if let Ok(contents) = fs::read_to_string(path)
            && let Ok(doc) = toml::from_str::<LedDoc>(&contents)
            && let Some(leds) = doc.leds
        {
            return Some(leds);
        }
    }
    None
}
