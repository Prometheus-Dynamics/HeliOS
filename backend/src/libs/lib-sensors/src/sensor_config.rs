use std::{
    fs,
    path::{Path, PathBuf},
};

use lib_schema_migration::{SyncSchemaPlan, normalize_to_current};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use utoipa::ToSchema;

/// Default locations searched for sensor configuration.
pub const CANONICAL_SENSOR_CONFIG_PATH: &str = "/var/lib/helios/sensors.toml";
pub const DEFAULT_SENSOR_CONFIG_PATHS: &[&str] = &[CANONICAL_SENSOR_CONFIG_PATH, "configs/presets/common.toml"];
pub const CURRENT_SENSOR_CONFIG_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
pub struct SensorDeviceCfg {
    pub driver: String,
    pub bus: u32,
    pub address: u8,
    #[serde(default)]
    pub shunt_resistance: Option<f32>,
    #[serde(default)]
    pub max_current: Option<f32>,
    #[serde(default)]
    pub bus_scale: Option<f32>,
    #[serde(default)]
    pub bus_offset: Option<f32>,
    #[serde(default)]
    pub current_scale: Option<f32>,
    #[serde(default)]
    pub current_offset: Option<f32>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct SensorsSectionCfg {
    #[serde(default)]
    devices: Vec<SensorDeviceCfg>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct SensorsDocCfg {
    schema_version: u32,
    sensors: Option<SensorsSectionCfg>,
}

impl Default for SensorsDocCfg {
    fn default() -> Self {
        Self { schema_version: CURRENT_SENSOR_CONFIG_SCHEMA_VERSION, sensors: None }
    }
}

/// Returns the default sensor configuration search paths.
#[must_use]
pub fn default_paths() -> Vec<PathBuf> {
    DEFAULT_SENSOR_CONFIG_PATHS.iter().map(PathBuf::from).collect()
}

/// Preferred writable location for runtime sensor overrides.
#[must_use]
pub fn writable_path() -> PathBuf {
    // Prefer a mutable location over the read-only defaults.
    PathBuf::from("/var/lib/helios/sensors.toml")
}

/// Loads the first available sensor configuration from the provided search paths.
#[must_use]
pub fn load_sensor_devices<P>(paths: &[P]) -> Vec<SensorDeviceCfg>
where
    P: AsRef<Path>,
{
    for path in paths {
        if let Ok(contents) = fs::read_to_string(path) {
            match parse_sensor_config_doc(&contents) {
                Ok(doc) => {
                    if let Some(section) = doc.sensors {
                        debug!(path = %path.as_ref().display(), count = section.devices.len(), "loaded sensor configuration");
                        return section.devices;
                    }
                }
                Err(err) => {
                    warn!(path = %path.as_ref().display(), error = %err, "failed to parse sensor configuration; continuing to next path");
                }
            }
        }
    }

    let fallback = default_devices();
    if !fallback.is_empty() {
        info!(count = fallback.len(), "using built-in sensor configuration fallback");
    }
    fallback
}

/// Built-in sensor definitions used when no external configuration is present.
#[must_use]
pub fn default_devices() -> Vec<SensorDeviceCfg> {
    Vec::new()
}

/// Persists sensor device configuration to a TOML file.
pub fn save_sensor_devices<P>(path: P, devices: &[SensorDeviceCfg]) -> std::io::Result<()>
where
    P: AsRef<Path>,
{
    let doc = SensorsDocCfg { schema_version: CURRENT_SENSOR_CONFIG_SCHEMA_VERSION, sensors: Some(SensorsSectionCfg { devices: devices.to_vec() }) };
    let toml = toml::to_string_pretty(&doc).map_err(std::io::Error::other)?;
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, toml)
}

const SENSOR_CONFIG_DOC_SCHEMA_PLAN: SyncSchemaPlan<toml::Value> = SyncSchemaPlan::strict("sensor configuration document", CURRENT_SENSOR_CONFIG_SCHEMA_VERSION);

fn parse_sensor_config_doc(raw: &str) -> Result<SensorsDocCfg, String> {
    let value = toml::from_str::<toml::Value>(raw).map_err(|err| err.to_string())?;
    let migrated = normalize_to_current(value, &SENSOR_CONFIG_DOC_SCHEMA_PLAN)?;
    migrated.try_into().map_err(|err: toml::de::Error| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::{CURRENT_SENSOR_CONFIG_SCHEMA_VERSION, parse_sensor_config_doc};

    #[test]
    fn parse_sensor_config_doc_rejects_missing_schema_version() {
        let raw = r#"
            [sensors]
            devices = [
              { driver = "ina226", bus = 1, address = 64 }
            ]
        "#;

        let err = parse_sensor_config_doc(raw).expect_err("missing schema version should fail");
        assert!(err.contains("missing required schema_version"));
    }

    #[test]
    fn parse_sensor_config_doc_rejects_future_schema_version() {
        let raw = format!("schema_version = {}\n[sensors]\ndevices = []\n", CURRENT_SENSOR_CONFIG_SCHEMA_VERSION + 1);
        let err = parse_sensor_config_doc(&raw).expect_err("future schema should fail");
        assert!(err.contains("unsupported sensor configuration document schema_version"));
    }
}
