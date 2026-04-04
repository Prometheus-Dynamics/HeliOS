use daedalus::runtime::NodeError;
use lib_runtime_policy::{HELIOS_NT4_SETTINGS_CACHE_POLICY, HELIOS_NT4_SETTINGS_FILE_POLICY, HELIOS_TEAM_FILE_POLICY, Nt4Settings, parse_nt4_settings_file};
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tracing::{debug, warn};

#[derive(Debug)]
struct SettingsCache {
    last_check: Instant,
    source_path: Option<PathBuf>,
    modified: Option<SystemTime>,
    last_loaded_bytes: Option<u64>,
    cache_hits: u64,
    cache_misses: u64,
    refreshes: u64,
    last_error: Option<String>,
    value: Nt4Settings,
}

struct Nt4SettingsRuntime {
    cache: Mutex<SettingsCache>,
}

impl Nt4SettingsRuntime {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            cache: Mutex::new(SettingsCache {
                last_check: now.checked_sub(Duration::from_secs(10)).unwrap_or(now),
                source_path: None,
                modified: None,
                last_loaded_bytes: None,
                cache_hits: 0,
                cache_misses: 0,
                refreshes: 0,
                last_error: None,
                value: Nt4Settings::default(),
            }),
        }
    }

    fn cached_settings(&self) -> Nt4Settings {
        let mut guard = self.cache.lock().expect("nt4 settings cache lock poisoned");
        let policy = HELIOS_NT4_SETTINGS_CACHE_POLICY.resolve();
        let now = Instant::now();
        if now.saturating_duration_since(guard.last_check) < Duration::from_millis(policy.refresh_interval_ms) {
            guard.cache_hits = guard.cache_hits.saturating_add(1);
            return guard.value.clone();
        }
        guard.cache_misses = guard.cache_misses.saturating_add(1);
        guard.last_check = now;

        let path = settings_path();
        let source_path = current_settings_source(&path);
        let modified = source_path.as_ref().and_then(|source| std::fs::metadata(source).ok()).and_then(|meta| meta.modified().ok());
        if modified != guard.modified || source_path != guard.source_path {
            guard.source_path = source_path;
            guard.modified = modified;
            guard.refreshes = guard.refreshes.saturating_add(1);
            let (value, loaded_bytes, last_error) = read_settings_file(&path, policy.max_file_bytes);
            guard.last_loaded_bytes = loaded_bytes;
            guard.last_error = last_error.clone();
            guard.value = value;
            if let Some(source) = guard.source_path.as_ref() {
                if let Some(err) = last_error {
                    warn!(path = %source.display(), max_file_bytes = policy.max_file_bytes, error = %err, "nt4 settings refresh fell back to defaults");
                } else {
                    debug!(path = %source.display(), loaded_bytes = guard.last_loaded_bytes, refresh_interval_ms = policy.refresh_interval_ms, "nt4 settings cache refreshed");
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

fn current_settings_source(path: &PathBuf) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.clone());
    }
    None
}

fn read_settings_file(path: &PathBuf, max_file_bytes: usize) -> (Nt4Settings, Option<u64>, Option<String>) {
    let Ok(metadata) = std::fs::metadata(path) else {
        return (Nt4Settings::default(), None, None);
    };
    if metadata.len() > max_file_bytes as u64 {
        return (Nt4Settings::default(), Some(metadata.len()), Some(format!("nt4 settings file exceeds {} bytes", max_file_bytes)));
    }

    let Ok(data) = std::fs::read(path) else {
        return (Nt4Settings::default(), None, None);
    };
    let loaded_bytes = Some(data.len() as u64);
    match parse_nt4_settings_file(&data) {
        Ok((stored, _)) => (stored.settings, loaded_bytes, None),
        Err(err) => (Nt4Settings::default(), loaded_bytes, Some(err)),
    }
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

#[cfg(test)]
fn reset_cache_for_tests() {
    *nt4_settings_runtime().cache.lock().expect("nt4 settings cache lock poisoned") = Nt4SettingsRuntime::new().cache.into_inner().expect("nt4 settings cache reset");
}

#[cfg(test)]
mod tests {
    use super::{cached_settings, reset_cache_for_tests};
    use std::sync::{LazyLock, Mutex};

    static TEST_ENV_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    #[allow(unsafe_code)]
    fn set_env_var<K: AsRef<std::ffi::OsStr>, V: AsRef<std::ffi::OsStr>>(key: K, value: V) {
        unsafe {
            std::env::set_var(key, value);
        }
    }

    #[allow(unsafe_code)]
    fn remove_env_var<K: AsRef<std::ffi::OsStr>>(key: K) {
        unsafe {
            std::env::remove_var(key);
        }
    }

    #[test]
    fn oversized_settings_file_falls_back_to_defaults() {
        let _guard = TEST_ENV_LOCK.lock().expect("test env lock poisoned");
        let dir = tempfile::tempdir().expect("tempdir");
        let settings_path = dir.path().join("nt4.json");
        let team_path = dir.path().join("team.txt");
        std::fs::write(&settings_path, vec![b'a'; 4096]).expect("write settings");
        std::fs::write(&team_path, b"").expect("write team file");

        set_env_var("HELIOS_NT4_SETTINGS_FILE", &settings_path);
        set_env_var("HELIOS_TEAM_FILE", &team_path);
        set_env_var("HELIOS_NT4_SETTINGS_MAX_FILE_BYTES", "1024");
        reset_cache_for_tests();

        let settings = cached_settings();
        let expected = serde_json::to_value(lib_runtime_policy::Nt4Settings::default()).expect("serialize default nt4 settings");
        let actual = serde_json::to_value(settings).expect("serialize cached nt4 settings");
        assert_eq!(actual, expected);

        remove_env_var("HELIOS_NT4_SETTINGS_FILE");
        remove_env_var("HELIOS_TEAM_FILE");
        remove_env_var("HELIOS_NT4_SETTINGS_MAX_FILE_BYTES");
        reset_cache_for_tests();
    }
}
