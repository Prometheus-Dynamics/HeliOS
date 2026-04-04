use daedalus::runtime::NodeError;
use lib_runtime_policy::{HELIOS_NT4_SETTINGS_FILE_POLICY, HELIOS_TEAM_FILE_POLICY, Nt4Settings, parse_nt4_settings_file};
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime};

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

impl Nt4SettingsRuntime {
    fn new() -> Self {
        let now = Instant::now();
        Self { cache: Mutex::new(SettingsCache { last_check: now.checked_sub(Duration::from_secs(10)).unwrap_or(now), source_path: None, modified: None, value: Nt4Settings::default() }) }
    }

    fn cached_settings(&self) -> Nt4Settings {
        let mut guard = self.cache.lock().expect("nt4 settings cache lock poisoned");
        let now = Instant::now();
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
}

fn nt4_settings_runtime() -> &'static Nt4SettingsRuntime {
    static RUNTIME: LazyLock<Nt4SettingsRuntime> = LazyLock::new(Nt4SettingsRuntime::new);
    &RUNTIME
}

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

fn settings_path() -> PathBuf {
    HELIOS_NT4_SETTINGS_FILE_POLICY.resolve()
}

fn team_file_path() -> PathBuf {
    HELIOS_TEAM_FILE_POLICY.resolve()
}

fn parse_nt4_settings(raw: &str) -> Nt4Settings {
    parse_nt4_settings_file(raw.as_bytes()).map(|(stored, _)| stored.settings).unwrap_or_default()
}

fn current_settings_source(path: &PathBuf) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.clone());
    }
    None
}

fn read_settings_file() -> Nt4Settings {
    let path = settings_path();
    let Ok(data) = std::fs::read_to_string(&path) else {
        return Nt4Settings::default();
    };
    parse_nt4_settings(&data)
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
