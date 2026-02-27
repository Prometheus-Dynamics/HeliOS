use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, env, fs, path::PathBuf};

const DEFAULT_PATH: &str = "/var/lib/helios/sensor_configuration.json";
const DEFAULT_FILENAME: &str = "sensor_configuration.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SensorDeviceOverride {
    pub firmware: Option<String>,
    pub last_flashed_firmware: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct SensorConfigData {
    devices: BTreeMap<String, SensorDeviceOverride>,
    #[serde(default)]
    aliases: BTreeMap<String, String>,
    #[serde(default)]
    alias_overrides: BTreeMap<String, String>,
}

pub struct SensorConfigStore {
    path: PathBuf,
    data: SensorConfigData,
}

impl SensorConfigStore {
    pub fn load() -> Self {
        let path = resolve_store_path();
        let data = match fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
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
        if let Ok(json) = serde_json::to_string_pretty(&self.data) {
            let _ = fs::write(&self.path, json);
        }
    }
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
