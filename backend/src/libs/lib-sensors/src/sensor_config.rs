use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use utoipa::ToSchema;

/// Default locations searched for sensor configuration.
pub const DEFAULT_SENSOR_CONFIG_PATHS: &[&str] = &["/etc/helios/sensors.toml", "/var/lib/helios/sensors.toml", "configs/presets/common.toml"];

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

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct SensorsDocCfg {
    sensors: Option<SensorsSectionCfg>,
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
            match toml::from_str::<SensorsDocCfg>(&contents) {
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
    let doc = SensorsDocCfg { sensors: Some(SensorsSectionCfg { devices: devices.to_vec() }) };
    let toml = toml::to_string_pretty(&doc).map_err(std::io::Error::other)?;
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, toml)
}
