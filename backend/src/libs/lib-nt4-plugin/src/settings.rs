use daedalus::runtime::NodeError;
use lib_schema_migration::{SyncSchemaPlan, normalize_to_current};
use serde::Deserialize;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct Nt4SettingsFile {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_subscriptions_enabled")]
    pub subscriptions_enabled: bool,
    #[allow(dead_code)]
    #[serde(default)]
    pub emulate_limelight_api: bool,
    #[allow(dead_code)]
    #[serde(default)]
    pub emulate_photonvision_api: bool,
    #[serde(default)]
    pub server_host: Option<String>,
    #[serde(default)]
    pub server_port: Option<u16>,
}

const CURRENT_NT4_SETTINGS_SCHEMA_VERSION: u32 = 1;
const CANONICAL_NT4_SETTINGS_PATH: &str = "/var/lib/helios/nt4.json";
const CANONICAL_TEAM_FILE_PATH: &str = "/var/lib/helios/team";

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
struct StoredNt4SettingsFile {
    schema_version: u32,
    #[serde(default)]
    settings: Nt4SettingsFile,
}

const NT4_SETTINGS_SCHEMA_PLAN: SyncSchemaPlan<serde_json::Value> = SyncSchemaPlan::strict("nt4 settings file", CURRENT_NT4_SETTINGS_SCHEMA_VERSION);

pub fn resolve_target(settings: &Nt4SettingsFile) -> Result<(String, u16), NodeError> {
    if !settings.enabled {
        return Err(NodeError::InvalidInput("nt4 is disabled (enable it in device settings)".into()));
    }

    let host = settings.server_host.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).or_else(default_nt4_server_host_from_team_file);
    let Some(host) = host else {
        return Err(NodeError::InvalidInput("no nt4 server host available (set team number or override the host in device settings)".into()));
    };
    let port = settings.server_port.unwrap_or(5810);
    if port == 0 {
        return Err(NodeError::InvalidInput("server_port must be between 1 and 65535".into()));
    }
    Ok((host, port))
}

pub fn resolve_topic(hostname: &str, stream_alias: &str, pipeline_alias: &str, topic_suffix: &str) -> String {
    let hostname = sanitize_segment(hostname, "helios");
    let stream = sanitize_segment(stream_alias, "stream");
    let pipeline = sanitize_segment(pipeline_alias, "pipeline");
    let suffix = sanitize_segment(topic_suffix, "value");
    format!("/{hostname}/streams/{stream}/pipelines/{pipeline}/{suffix}")
}

pub fn sanitize_segment(raw: &str, fallback: &str) -> String {
    let trimmed = raw.trim().trim_matches('/');
    let filtered: String = trimmed.chars().filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_' || *ch == '.').collect();
    if filtered.is_empty() { fallback.to_string() } else { filtered }
}

fn settings_path() -> PathBuf {
    match std::env::var_os("HELIOS_NT4_SETTINGS_FILE") {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(CANONICAL_NT4_SETTINGS_PATH),
    }
}

fn team_file_path() -> PathBuf {
    match std::env::var_os("HELIOS_TEAM_FILE") {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from(CANONICAL_TEAM_FILE_PATH),
    }
}

fn parse_nt4_settings(raw: &str) -> Nt4SettingsFile {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Nt4SettingsFile::default();
    };
    let Ok(migrated) = normalize_to_current(value, &NT4_SETTINGS_SCHEMA_PLAN) else {
        return Nt4SettingsFile::default();
    };
    let Ok(stored) = serde_json::from_value::<StoredNt4SettingsFile>(migrated) else {
        return Nt4SettingsFile::default();
    };
    stored.settings
}

fn current_settings_source(path: &PathBuf) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.clone());
    }
    None
}

pub fn read_settings_file() -> Nt4SettingsFile {
    let path = settings_path();
    let Ok(data) = std::fs::read_to_string(&path) else {
        return Nt4SettingsFile::default();
    };
    parse_nt4_settings(&data)
}

#[derive(Debug)]
struct SettingsCache {
    last_check: Instant,
    source_path: Option<PathBuf>,
    modified: Option<SystemTime>,
    value: Nt4SettingsFile,
}

static SETTINGS_CACHE: once_cell::sync::Lazy<Mutex<SettingsCache>> = once_cell::sync::Lazy::new(|| {
    let now = Instant::now();
    Mutex::new(SettingsCache { last_check: now.checked_sub(Duration::from_secs(10)).unwrap_or(now), source_path: None, modified: None, value: Nt4SettingsFile::default() })
});

pub fn cached_settings() -> Nt4SettingsFile {
    let mut guard = SETTINGS_CACHE.lock().expect("nt4 settings cache lock poisoned");
    let now = Instant::now();
    // Avoid stat'ing/reading the settings file for every node execution.
    if now.saturating_duration_since(guard.last_check) < Duration::from_millis(250) {
        return guard.value.clone();
    }
    guard.last_check = now;

    let path = settings_path();
    let source_path = current_settings_source(&path);
    let modified = source_path.as_ref().and_then(|source| std::fs::metadata(source).ok()).and_then(|meta| meta.modified().ok());
    if modified != guard.modified || source_path != guard.source_path {
        guard.source_path = source_path;
        guard.modified = modified;
        guard.value = read_settings_file();
    }
    guard.value.clone()
}

fn default_subscriptions_enabled() -> bool {
    true
}

fn default_nt4_server_host_from_team_file() -> Option<String> {
    let path = team_file_path();
    let content = std::fs::read_to_string(&path).ok()?;
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }
    let team: u32 = trimmed.parse().ok()?;
    team_number_to_rio_ip(team).map(|ip| ip.to_string())
}

fn team_number_to_rio_ip(team: u32) -> Option<Ipv4Addr> {
    if team == 0 || team > 25_599 {
        return None;
    }
    let a = (team / 100) as u8;
    let b = (team % 100) as u8;
    Some(Ipv4Addr::new(10, a, b, 2))
}
