use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use lib_schema_migration::{SyncSchemaPlan, migrate_to_current};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Default locations searched for LED configuration.
/// Prefer the on-device file, then fall back to the shared preset shipped with the repo.
pub const CANONICAL_LED_CONFIG_PATH: &str = "/var/lib/helios/leds.toml";
pub const DEFAULT_LED_CONFIG_PATHS: &[&str] = &[CANONICAL_LED_CONFIG_PATH, "configs/presets/common.toml"];
pub const CURRENT_LED_CONFIG_SCHEMA_VERSION: u32 = 1;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LedDoc {
    schema_version: u32,
    #[serde(default)]
    leds: Option<LedConfig>,
}

impl Default for LedDoc {
    fn default() -> Self {
        Self { schema_version: CURRENT_LED_CONFIG_SCHEMA_VERSION, leds: None }
    }
}

/// Returns the default LED configuration search paths.
#[must_use]
pub fn default_paths() -> Vec<PathBuf> {
    DEFAULT_LED_CONFIG_PATHS.iter().map(PathBuf::from).collect()
}

/// Preferred writable location for runtime LED overrides.
#[must_use]
pub fn writable_path() -> PathBuf {
    PathBuf::from("/var/lib/helios/leds.toml")
}

/// Loads LED configuration from the first readable document in the provided paths.
#[must_use]
pub fn load_led_config<P>(paths: &[P]) -> Option<LedConfig>
where
    P: AsRef<Path>,
{
    for path in paths {
        if let Ok(contents) = fs::read_to_string(path)
            && let Ok(doc) = parse_led_config_doc(&contents)
            && let Some(leds) = doc.leds
        {
            return Some(leds);
        }
    }
    None
}

const LED_CONFIG_DOC_SCHEMA_PLAN: SyncSchemaPlan<toml::Value> =
    SyncSchemaPlan { document_name: "LED configuration document", legacy_version: CURRENT_LED_CONFIG_SCHEMA_VERSION, current_version: CURRENT_LED_CONFIG_SCHEMA_VERSION, migrations: &[] };

fn parse_led_config_doc(raw: &str) -> Result<LedDoc, String> {
    let value = toml::from_str::<toml::Value>(raw).map_err(|err| err.to_string())?;
    let migrated = migrate_to_current(value, &LED_CONFIG_DOC_SCHEMA_PLAN)?;
    migrated.try_into().map_err(|err: toml::de::Error| err.to_string())
}

pub fn encode_led_config_doc(config: &LedConfig) -> Result<String, String> {
    let doc = LedDoc { schema_version: CURRENT_LED_CONFIG_SCHEMA_VERSION, leds: Some(config.clone()) };
    toml::to_string_pretty(&doc).map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::{CURRENT_LED_CONFIG_SCHEMA_VERSION, LedConfig, encode_led_config_doc, parse_led_config_doc};

    #[test]
    fn parse_led_config_doc_rejects_missing_schema_version() {
        let raw = r#"
            [leds]
            enabled = true
            gpio = 13
            count = 16
            color_order = "rgbw"
            frequency_hz = 800000
            use_pwm = true
            protocol = "sk6812-ec20"
        "#;

        let err = parse_led_config_doc(raw).expect_err("missing schema version should fail");
        assert!(err.contains("missing required schema_version"));
    }

    #[test]
    fn parse_led_config_doc_rejects_future_schema_version() {
        let raw = format!("schema_version = {}\n[leds]\nenabled = true\n", CURRENT_LED_CONFIG_SCHEMA_VERSION + 1);
        let err = parse_led_config_doc(&raw).expect_err("future schema should fail");
        assert!(err.contains("unsupported LED configuration document schema_version"));
    }

    #[test]
    fn encode_led_config_doc_writes_current_schema_version() {
        let raw = encode_led_config_doc(&LedConfig::default()).expect("encode");
        let parsed = parse_led_config_doc(&raw).expect("parse");
        assert_eq!(parsed.schema_version, CURRENT_LED_CONFIG_SCHEMA_VERSION);
        assert!(parsed.leds.is_some());
    }
}
