use daedalus::runtime::NodeError;
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

fn settings_paths() -> (PathBuf, Option<PathBuf>) {
    // TEMP_SHIM: nt4-plugin-settings-etc-fallback
    // Keep the /etc fallback until every deployed image migrates nt4.json into /var/lib/helios.
    match std::env::var_os("HELIOS_NT4_SETTINGS_FILE") {
        Some(path) => (PathBuf::from(path), None),
        None => (PathBuf::from("/var/lib/helios/nt4.json"), Some(PathBuf::from("/etc/helios/nt4.json"))),
    }
}

fn team_file_paths() -> (PathBuf, Option<PathBuf>) {
    // TEMP_SHIM: nt4-plugin-team-file-etc-fallback
    // Keep the /etc team fallback until every deployed image migrates the team file into /var/lib/helios.
    match std::env::var_os("HELIOS_TEAM_FILE") {
        Some(path) => (PathBuf::from(path), None),
        None => (PathBuf::from("/var/lib/helios/team"), Some(PathBuf::from("/etc/helios/team"))),
    }
}

fn read_to_string_with_fallback(path: &PathBuf, fallback: Option<&PathBuf>) -> std::io::Result<String> {
    match std::fs::read_to_string(path) {
        Ok(raw) => Ok(raw),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => match fallback {
            Some(fallback) => std::fs::read_to_string(fallback),
            None => Err(err),
        },
        Err(err) => Err(err),
    }
}

pub fn read_settings_file() -> Nt4SettingsFile {
    let (path, legacy_path) = settings_paths();
    let Ok(data) = read_to_string_with_fallback(&path, legacy_path.as_ref()) else {
        return Nt4SettingsFile::default();
    };
    serde_json::from_str::<Nt4SettingsFile>(&data).unwrap_or_default()
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

    let (path, legacy_path) = settings_paths();
    let source_path = if path.is_file() { Some(path) } else { legacy_path.filter(|candidate| candidate.is_file()) };
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
    let (path, legacy_path) = team_file_paths();
    let content = read_to_string_with_fallback(&path, legacy_path.as_ref()).ok()?;
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
