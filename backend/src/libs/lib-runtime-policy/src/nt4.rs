use crate::{BoundedU64Policy, filesystem::PathPolicy};
use lib_schema_migration::{SyncSchemaPlan, normalize_to_current};
use serde::{Deserialize, Serialize};

pub const HELIOS_NT4_SETTINGS_FILE_POLICY: PathPolicy = PathPolicy { env_var: "HELIOS_NT4_SETTINGS_FILE", default: "/var/lib/helios/nt4.json" };

pub const HELIOS_TEAM_FILE_POLICY: PathPolicy = PathPolicy { env_var: "HELIOS_TEAM_FILE", default: "/var/lib/helios/team" };

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Nt4SettingsCachePolicy {
    pub refresh_interval_ms: BoundedU64Policy,
    pub max_file_bytes: BoundedU64Policy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedNt4SettingsCachePolicy {
    pub refresh_interval_ms: u64,
    pub max_file_bytes: usize,
}

impl Nt4SettingsCachePolicy {
    pub fn resolve(self) -> ResolvedNt4SettingsCachePolicy {
        ResolvedNt4SettingsCachePolicy { refresh_interval_ms: self.refresh_interval_ms.resolve(), max_file_bytes: self.max_file_bytes.resolve().min(usize::MAX as u64) as usize }
    }
}

pub const HELIOS_NT4_SETTINGS_CACHE_POLICY: Nt4SettingsCachePolicy = Nt4SettingsCachePolicy {
    refresh_interval_ms: BoundedU64Policy { env_var: "HELIOS_NT4_SETTINGS_REFRESH_INTERVAL_MS", default: 250, min: 50, max: 10_000 },
    max_file_bytes: BoundedU64Policy { env_var: "HELIOS_NT4_SETTINGS_MAX_FILE_BYTES", default: 64 * 1024, min: 256, max: 1024 * 1024 },
};

const CURRENT_NT4_SETTINGS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct Nt4Settings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_subscriptions_enabled")]
    pub subscriptions_enabled: bool,
    #[serde(default)]
    pub emulate_limelight_api: bool,
    #[serde(default)]
    pub emulate_photonvision_api: bool,
    #[serde(default)]
    pub server_host: Option<String>,
    #[serde(default)]
    pub server_port: Option<u16>,
    #[serde(default)]
    pub public_api_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StoredNt4SettingsFile {
    pub schema_version: u32,
    pub settings: Nt4Settings,
}

const NT4_SETTINGS_SCHEMA_PLAN: SyncSchemaPlan<serde_json::Value> = SyncSchemaPlan::strict("nt4 settings file", CURRENT_NT4_SETTINGS_SCHEMA_VERSION);

pub fn parse_nt4_settings_file(bytes: &[u8]) -> Result<(StoredNt4SettingsFile, bool), String> {
    let raw = serde_json::from_slice::<serde_json::Value>(bytes).map_err(|err| format!("failed to decode nt4 settings: {err}"))?;
    let migrated = normalize_to_current(raw.clone(), &NT4_SETTINGS_SCHEMA_PLAN)?;
    let parsed = serde_json::from_value::<StoredNt4SettingsFile>(migrated.clone()).map_err(|err| format!("failed to parse nt4 settings: {err}"))?;
    Ok((parsed, migrated != raw))
}

pub fn encode_nt4_settings_file(mut settings: Nt4Settings) -> Result<Vec<u8>, String> {
    normalize_nt4_settings(&mut settings)?;
    serde_json::to_vec(&StoredNt4SettingsFile { schema_version: CURRENT_NT4_SETTINGS_SCHEMA_VERSION, settings }).map_err(|err| format!("failed to encode nt4 settings: {err}"))
}

pub fn normalize_nt4_settings(settings: &mut Nt4Settings) -> Result<(), String> {
    settings.server_host = settings.server_host.as_ref().map(|host| host.trim()).filter(|host| !host.is_empty()).map(str::to_string);
    settings.public_api_url = settings.public_api_url.as_ref().map(|url| url.trim()).filter(|url| !url.is_empty()).map(str::to_string);
    if matches!(settings.server_port, Some(0)) {
        return Err("server_port must be > 0".into());
    }
    Ok(())
}

fn default_subscriptions_enabled() -> bool {
    true
}
