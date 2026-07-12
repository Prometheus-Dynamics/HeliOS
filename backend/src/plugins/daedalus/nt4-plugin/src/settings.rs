use daedalus::runtime::NodeError;
use lib_schema_migration::{SyncSchemaPlan, normalize_to_current};
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tracing::{debug, warn};

const DEFAULT_TEAM_FILE: &str = "/var/lib/helios/team";
const DEFAULT_NT4_SETTINGS_FILE: &str = "/var/lib/helios/nt4.json";
const DEFAULT_NT4_SETTINGS_REFRESH_INTERVAL_MS: u64 = 250;
const DEFAULT_NT4_SETTINGS_MAX_FILE_BYTES: usize = 64 * 1024;
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
struct StoredNt4SettingsFile {
    schema_version: u32,
    settings: Nt4Settings,
}

#[derive(Debug)]
struct SettingsCache {
    last_check: Instant,
    source_path: Option<PathBuf>,
    modified: Option<SystemTime>,
    value: Nt4Settings,
}

struct Nt4SettingsRuntime {
    cache: Mutex<SettingsCache>,
}

const NT4_SETTINGS_SCHEMA_PLAN: SyncSchemaPlan<serde_json::Value> = SyncSchemaPlan::strict("nt4 settings file", CURRENT_NT4_SETTINGS_SCHEMA_VERSION);

pub fn resolve_target(settings: &Nt4Settings) -> Result<(String, u16), NodeError> {
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

pub fn cached_settings() -> Nt4Settings {
    nt4_settings_runtime().cached_settings()
}

pub fn sanitize_segment(raw: &str, fallback: &str) -> String {
    let trimmed = raw.trim().trim_matches('/');
    let filtered: String = trimmed.chars().filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_' || *ch == '.').collect();
    if filtered.is_empty() { fallback.to_string() } else { filtered }
}

fn team_file_path() -> PathBuf {
    resolve_env_path("HELIOS_TEAM_FILE").unwrap_or_else(|| PathBuf::from(DEFAULT_TEAM_FILE))
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

fn default_subscriptions_enabled() -> bool {
    true
}

impl Nt4SettingsRuntime {
    fn new() -> Self {
        let now = Instant::now();
        Self { cache: Mutex::new(SettingsCache { last_check: now.checked_sub(Duration::from_secs(10)).unwrap_or(now), source_path: None, modified: None, value: Nt4Settings::default() }) }
    }

    fn cached_settings(&self) -> Nt4Settings {
        let mut guard = self.cache.lock().expect("nt4 settings cache lock poisoned");
        let refresh_interval = Duration::from_millis(resolve_env_u64("HELIOS_NT4_SETTINGS_REFRESH_INTERVAL_MS", DEFAULT_NT4_SETTINGS_REFRESH_INTERVAL_MS, 50, 10_000));
        let now = Instant::now();
        if now.saturating_duration_since(guard.last_check) < refresh_interval {
            return guard.value.clone();
        }
        guard.last_check = now;

        let path = resolve_env_path("HELIOS_NT4_SETTINGS_FILE").unwrap_or_else(|| PathBuf::from(DEFAULT_NT4_SETTINGS_FILE));
        let source_path = current_settings_source(&path);
        let modified = source_path.as_ref().and_then(|source| std::fs::metadata(source).ok()).and_then(|meta| meta.modified().ok());
        if modified != guard.modified || source_path != guard.source_path {
            guard.source_path = source_path;
            guard.modified = modified;
            let max_file_bytes = resolve_env_usize("HELIOS_NT4_SETTINGS_MAX_FILE_BYTES", DEFAULT_NT4_SETTINGS_MAX_FILE_BYTES, 256, 1024 * 1024);
            let (value, last_error) = read_settings_file(&path, max_file_bytes);
            guard.value = value;

            if let Some(source) = guard.source_path.as_ref() {
                if let Some(err) = last_error {
                    warn!(path = %source.display(), max_file_bytes, error = %err, "nt4 settings refresh fell back to defaults");
                } else {
                    debug!(path = %source.display(), refresh_interval_ms = refresh_interval.as_millis(), "nt4 settings cache refreshed");
                }
            }
        }

        guard.value.clone()
    }
}

fn nt4_settings_runtime() -> &'static Nt4SettingsRuntime {
    static RUNTIME: LazyLock<Nt4SettingsRuntime> = LazyLock::new(Nt4SettingsRuntime::new);
    &RUNTIME
}

fn current_settings_source(path: &Path) -> Option<PathBuf> {
    if path.is_file() { Some(path.to_path_buf()) } else { None }
}

fn read_settings_file(path: &Path, max_file_bytes: usize) -> (Nt4Settings, Option<String>) {
    let Ok(metadata) = std::fs::metadata(path) else {
        return (Nt4Settings::default(), None);
    };
    if metadata.len() > max_file_bytes as u64 {
        return (Nt4Settings::default(), Some(format!("nt4 settings file exceeds {} bytes", max_file_bytes)));
    }

    let Ok(data) = std::fs::read(path) else {
        return (Nt4Settings::default(), None);
    };
    match parse_nt4_settings_file(&data) {
        Ok((stored, _)) => (stored.settings, None),
        Err(err) => (Nt4Settings::default(), Some(err)),
    }
}

fn parse_nt4_settings_file(bytes: &[u8]) -> Result<(StoredNt4SettingsFile, bool), String> {
    let raw = serde_json::from_slice::<serde_json::Value>(bytes).map_err(|err| format!("failed to decode nt4 settings: {err}"))?;
    let migrated = normalize_to_current(raw.clone(), &NT4_SETTINGS_SCHEMA_PLAN)?;
    let parsed = serde_json::from_value::<StoredNt4SettingsFile>(migrated.clone()).map_err(|err| format!("failed to parse nt4 settings: {err}"))?;
    Ok((parsed, migrated != raw))
}

fn resolve_env_path(name: &str) -> Option<PathBuf> {
    std::env::var(name).ok().map(|value| value.trim().to_string()).filter(|value| !value.is_empty()).map(PathBuf::from)
}

fn resolve_env_u64(name: &str, default: u64, min: u64, max: u64) -> u64 {
    std::env::var(name).ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(default).clamp(min, max)
}

fn resolve_env_usize(name: &str, default: usize, min: usize, max: usize) -> usize {
    std::env::var(name).ok().and_then(|value| value.trim().parse::<usize>().ok()).unwrap_or(default).clamp(min, max)
}
