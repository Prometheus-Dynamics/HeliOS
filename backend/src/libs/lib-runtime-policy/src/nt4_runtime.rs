use crate::{HELIOS_NT4_SETTINGS_CACHE_POLICY, HELIOS_NT4_SETTINGS_FILE_POLICY, Nt4Settings, parse_nt4_settings_file};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tracing::{debug, warn};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Nt4SettingsCacheObservabilitySnapshot {
    pub source_path: Option<String>,
    pub source_modified_ms: Option<u64>,
    pub last_loaded_bytes: Option<u64>,
    pub hits: u64,
    pub misses: u64,
    pub refreshes: u64,
    pub last_error: Option<String>,
}

#[derive(Debug)]
struct SettingsCache {
    last_check: Instant,
    source_path: Option<PathBuf>,
    modified: Option<SystemTime>,
    last_loaded_bytes: Option<u64>,
    hits: u64,
    misses: u64,
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
                hits: 0,
                misses: 0,
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
            guard.hits = guard.hits.saturating_add(1);
            return guard.value.clone();
        }
        guard.misses = guard.misses.saturating_add(1);
        guard.last_check = now;

        let path = HELIOS_NT4_SETTINGS_FILE_POLICY.resolve();
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

    fn snapshot(&self) -> Nt4SettingsCacheObservabilitySnapshot {
        let guard = self.cache.lock().expect("nt4 settings cache lock poisoned");
        Nt4SettingsCacheObservabilitySnapshot {
            source_path: guard.source_path.as_ref().map(|path| path.display().to_string()),
            source_modified_ms: guard.modified.and_then(system_time_ms),
            last_loaded_bytes: guard.last_loaded_bytes,
            hits: guard.hits,
            misses: guard.misses,
            refreshes: guard.refreshes,
            last_error: guard.last_error.clone(),
        }
    }
}

fn nt4_settings_runtime() -> &'static Nt4SettingsRuntime {
    static RUNTIME: LazyLock<Nt4SettingsRuntime> = LazyLock::new(Nt4SettingsRuntime::new);
    &RUNTIME
}

pub fn cached_nt4_settings() -> Nt4Settings {
    nt4_settings_runtime().cached_settings()
}

pub fn nt4_settings_cache_snapshot() -> Nt4SettingsCacheObservabilitySnapshot {
    nt4_settings_runtime().snapshot()
}

fn current_settings_source(path: &Path) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path.to_path_buf());
    }
    None
}

fn read_settings_file(path: &Path, max_file_bytes: usize) -> (Nt4Settings, Option<u64>, Option<String>) {
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

fn system_time_ms(value: SystemTime) -> Option<u64> {
    value.duration_since(UNIX_EPOCH).ok().map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
}

#[cfg(test)]
pub(crate) fn reset_nt4_settings_cache_for_tests() {
    *nt4_settings_runtime().cache.lock().expect("nt4 settings cache lock poisoned") = Nt4SettingsRuntime::new().cache.into_inner().expect("nt4 settings cache reset");
}

#[cfg(test)]
mod tests {
    use super::{cached_nt4_settings, nt4_settings_cache_snapshot, reset_nt4_settings_cache_for_tests};
    use crate::Nt4Settings;
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
        std::fs::write(&settings_path, vec![b'a'; 4096]).expect("write settings");

        set_env_var("HELIOS_NT4_SETTINGS_FILE", &settings_path);
        set_env_var("HELIOS_NT4_SETTINGS_MAX_FILE_BYTES", "1024");
        reset_nt4_settings_cache_for_tests();

        let expected = serde_json::to_value(Nt4Settings::default()).expect("serialize default nt4 settings");
        let actual = serde_json::to_value(cached_nt4_settings()).expect("serialize cached nt4 settings");
        assert_eq!(actual, expected);

        remove_env_var("HELIOS_NT4_SETTINGS_FILE");
        remove_env_var("HELIOS_NT4_SETTINGS_MAX_FILE_BYTES");
        reset_nt4_settings_cache_for_tests();
    }

    #[test]
    fn cache_snapshot_reports_hits_misses_refreshes_and_errors() {
        let _guard = TEST_ENV_LOCK.lock().expect("test env lock poisoned");
        let dir = tempfile::tempdir().expect("tempdir");
        let settings_path = dir.path().join("nt4.json");
        std::fs::write(&settings_path, vec![b'a'; 4096]).expect("write settings");

        set_env_var("HELIOS_NT4_SETTINGS_FILE", &settings_path);
        set_env_var("HELIOS_NT4_SETTINGS_MAX_FILE_BYTES", "1024");
        reset_nt4_settings_cache_for_tests();

        let initial = nt4_settings_cache_snapshot();
        assert_eq!(initial.hits, 0);
        assert_eq!(initial.misses, 0);
        assert_eq!(initial.refreshes, 0);
        assert!(initial.last_error.is_none());

        let _ = cached_nt4_settings();
        let after_refresh = nt4_settings_cache_snapshot();
        assert_eq!(after_refresh.hits, 0);
        assert_eq!(after_refresh.misses, 1);
        assert_eq!(after_refresh.refreshes, 1);
        assert_eq!(after_refresh.last_loaded_bytes, Some(4096));
        assert_eq!(after_refresh.source_path.as_deref(), Some(settings_path.to_string_lossy().as_ref()));
        assert!(after_refresh.source_modified_ms.is_some());
        assert_eq!(after_refresh.last_error.as_deref(), Some("nt4 settings file exceeds 1024 bytes"));

        let _ = cached_nt4_settings();
        let after_hit = nt4_settings_cache_snapshot();
        assert_eq!(after_hit.hits, 1);
        assert_eq!(after_hit.misses, 1);
        assert_eq!(after_hit.refreshes, 1);
        assert_eq!(after_hit.last_error.as_deref(), Some("nt4 settings file exceeds 1024 bytes"));

        remove_env_var("HELIOS_NT4_SETTINGS_FILE");
        remove_env_var("HELIOS_NT4_SETTINGS_MAX_FILE_BYTES");
        reset_nt4_settings_cache_for_tests();
    }
}
