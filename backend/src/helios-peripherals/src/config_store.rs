use lib_schema_migration::{SyncSchemaPlan, normalize_to_current};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, env, fs, path::PathBuf};
use tracing::warn;

const DEFAULT_PATH: &str = "/var/lib/helios/sensor_configuration.json";
const DEFAULT_FILENAME: &str = "sensor_configuration.json";
const CURRENT_SENSOR_CONFIG_STORE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SensorDeviceOverride {
    pub firmware: Option<String>,
    pub last_flashed_firmware: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SensorConfigData {
    #[serde(default)]
    schema_version: u32,
    devices: BTreeMap<String, SensorDeviceOverride>,
    #[serde(default)]
    aliases: BTreeMap<String, String>,
    #[serde(default)]
    alias_overrides: BTreeMap<String, String>,
}

impl Default for SensorConfigData {
    fn default() -> Self {
        Self { schema_version: CURRENT_SENSOR_CONFIG_STORE_SCHEMA_VERSION, devices: BTreeMap::new(), aliases: BTreeMap::new(), alias_overrides: BTreeMap::new() }
    }
}

pub struct SensorConfigStore {
    path: PathBuf,
    data: SensorConfigData,
}

impl SensorConfigStore {
    pub fn load() -> Self {
        let path = resolve_store_path();
        let data = match fs::read_to_string(&path) {
            Ok(contents) => match parse_sensor_config_data(&contents) {
                Ok(data) => data,
                Err(error) => {
                    warn!(path = %path.display(), %error, "failed to load sensor config store; using defaults");
                    SensorConfigData::default()
                }
            },
            Err(_) => SensorConfigData::default(),
        };
        Self { path, data }
    }

    pub fn get_or_default(&mut self, identifier: &str) -> &mut SensorDeviceOverride {
        self.data.devices.entry(identifier.to_string()).or_default()
    }

    pub fn ensure_alias<F>(&mut self, hardware_key: &str, generator: F) -> (String, bool)
    where
        F: FnOnce() -> String,
    {
        match self.data.aliases.entry(hardware_key.to_string()) {
            std::collections::btree_map::Entry::Occupied(entry) => (entry.get().clone(), false),
            std::collections::btree_map::Entry::Vacant(entry) => {
                let alias = generator();
                entry.insert(alias.clone());
                (alias, true)
            }
        }
    }

    pub fn alias_override(&self, hardware_key: &str) -> Option<String> {
        self.data.alias_overrides.get(hardware_key).map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
    }

    /// Sets or clears a human-friendly alias override for a hardware key.
    ///
    /// Returns true when the stored value changed.
    pub fn set_alias_override(&mut self, hardware_key: &str, alias: Option<String>) -> bool {
        let key = hardware_key.trim();
        if key.is_empty() {
            return false;
        }

        match alias.map(|value| value.trim().to_string()).filter(|value| !value.is_empty()) {
            Some(value) => match self.data.alias_overrides.entry(key.to_string()) {
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    if entry.get() == &value {
                        false
                    } else {
                        entry.insert(value);
                        true
                    }
                }
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(value);
                    true
                }
            },
            None => self.data.alias_overrides.remove(key).is_some(),
        }
    }

    pub fn save(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let mut canonical = self.data.clone();
        canonical.schema_version = CURRENT_SENSOR_CONFIG_STORE_SCHEMA_VERSION;
        if let Ok(json) = serde_json::to_string_pretty(&canonical) {
            let _ = fs::write(&self.path, json);
        }
    }
}

const SENSOR_CONFIG_STORE_SCHEMA_PLAN: SyncSchemaPlan<serde_json::Value> = SyncSchemaPlan::strict("sensor config store", CURRENT_SENSOR_CONFIG_STORE_SCHEMA_VERSION);

fn parse_sensor_config_data(raw: &str) -> Result<SensorConfigData, String> {
    let value = serde_json::from_str::<serde_json::Value>(raw).map_err(|err| format!("failed to decode sensor config store: {err}"))?;
    let migrated = normalize_to_current(value, &SENSOR_CONFIG_STORE_SCHEMA_PLAN)?;
    serde_json::from_value(migrated).map_err(|err| format!("failed to parse sensor config store: {err}"))
}

fn resolve_store_path() -> PathBuf {
    let from_env = select_env(&["PERIPHERALS_CONFIG_STORE", "PERIPHERAL_CONFIG_STORE", "SENSORS_CONFIG_STORE", "SENSOR_CONFIG_STORE"]).map(PathBuf::from);

    if let Some(path) = from_env {
        return path;
    }

    if let Some(dir) = select_env(&["PERIPHERALS_STATE_DIR", "SENSORS_STATE_DIR"]) {
        let mut path = PathBuf::from(dir);
        path.push(DEFAULT_FILENAME);
        return path;
    }

    PathBuf::from(DEFAULT_PATH)
}

fn select_env(names: &[&str]) -> Option<String> {
    for name in names {
        if let Ok(value) = env::var(name) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{CURRENT_SENSOR_CONFIG_STORE_SCHEMA_VERSION, SensorConfigData, SensorConfigStore, parse_sensor_config_data};

    fn temp_path(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos();
        std::env::temp_dir().join(format!("helios-peripherals-{label}-{unique}.json"))
    }

    #[test]
    fn parse_sensor_config_data_rejects_missing_schema_version() {
        let raw = serde_json::json!({
            "devices": {
                "camera-a": {
                    "firmware": "v1.2.3"
                }
            },
            "aliases": {
                "hw-a": "Front Camera"
            }
        });

        let err = parse_sensor_config_data(&serde_json::to_string(&raw).expect("encode")).expect_err("missing schema version should fail");
        assert!(err.contains("missing required schema_version"));
    }

    #[test]
    fn parse_sensor_config_data_rejects_future_schema_version() {
        let raw = serde_json::json!({
            "schema_version": CURRENT_SENSOR_CONFIG_STORE_SCHEMA_VERSION + 1,
            "devices": {}
        });

        let err = parse_sensor_config_data(&serde_json::to_string(&raw).expect("encode")).expect_err("future schema should fail");
        assert!(err.contains("unsupported sensor config store schema_version"));
    }

    #[test]
    fn save_writes_current_schema_version() {
        let path = temp_path("sensor-config-store");
        let mut store = SensorConfigStore { path: path.clone(), data: SensorConfigData::default() };
        store.get_or_default("camera-a").firmware = Some("v9".into());
        store.save();

        let raw = std::fs::read_to_string(&path).expect("read saved file");
        let parsed = serde_json::from_str::<serde_json::Value>(&raw).expect("decode json");
        assert_eq!(parsed.get("schema_version").and_then(serde_json::Value::as_u64), Some(u64::from(CURRENT_SENSOR_CONFIG_STORE_SCHEMA_VERSION)));
        let _ = std::fs::remove_file(&path);
    }
}
