use std::path::PathBuf;
use std::process::Stdio;
use std::sync::OnceLock;

use tokio::process::{Child, Command};
use tokio::time::{Duration, Instant};
use tracing::warn;

use crate::api_observability::ApiCacheMetric;
use crate::logs;
use crate::logs::LogSource;

use super::config::read_duration_env;
use super::state::SystemReadModelState;

#[derive(Clone)]
pub(super) struct LogSourcesCacheEntry {
    pub(super) fetched_at: Instant,
    pub(super) revision: u64,
    pub(super) payload: Vec<LogSource>,
}

fn log_sources_cache_ttl() -> Duration {
    static TTL: OnceLock<Duration> = OnceLock::new();
    *TTL.get_or_init(|| read_duration_env("HELIOS_LOG_SOURCES_CACHE_MS", 5_000, 0, 60_000))
}

fn log_sources_refresh_timeout() -> Duration {
    static TTL: OnceLock<Duration> = OnceLock::new();
    *TTL.get_or_init(|| read_duration_env("HELIOS_LOG_SOURCES_REFRESH_TIMEOUT_MS", 3_000, 500, 15_000))
}

fn build_log_sources() -> Vec<LogSource> {
    let mut sources = logs::default_log_sources();
    sources.extend(logs::discover_file_sources());
    sources
}

fn sanitize_log_filename(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return "logs.log".into();
    }
    let mut out = String::with_capacity(trimmed.len());
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            out.push(ch);
        } else if ch.is_whitespace() {
            out.push('_');
        } else {
            out.push('-');
        }
    }
    let out = out.trim_matches(&['.', '-', '_'][..]).to_string();
    if out.is_empty() { "logs.log".into() } else { format!("{out}.log") }
}

fn journal_script(unit: Option<&str>, lines: usize, follow: bool) -> String {
    let n = lines.to_string();
    let follow_flag = if follow { "-f" } else { "" };
    let unit_flag = unit.map(|u| format!("-u {u}")).unwrap_or_default();
    format!("journalctl --no-pager -o short-iso {follow_flag} -n {n} {unit_flag}")
}

fn file_script(path: &str, lines: usize, follow: bool) -> String {
    let n = lines.to_string();
    if follow { format!("tail -n {n} -F {path}") } else { format!("tail -n {n} {path}") }
}

fn dmesg_script(lines: usize, follow: bool) -> String {
    let n = lines.to_string();
    if follow {
        return format!(
            r#"
if command -v journalctl >/dev/null 2>&1; then
  journalctl -k --no-pager -o short-iso -n {n} -f
elif [ -r /dev/kmsg ]; then
  dmesg 2>/dev/null | tail -n {n}
  cat /dev/kmsg
else
  dmesg 2>/dev/null | tail -n {n}
fi
"#
        );
    }

    format!(
        r#"
dmesg 2>/dev/null | tail -n {n}
"#
    )
}

impl SystemReadModelState {
    pub fn log_sources_cache_metrics(&self) -> ApiCacheMetric {
        self.log_sources_stats.snapshot()
    }

    pub async fn load_log_sources_snapshot(&self) -> (Vec<LogSource>, u64) {
        let ttl = log_sources_cache_ttl();
        if ttl != Duration::from_millis(0)
            && let Some(entry) = self.log_sources_cache.read().await.clone()
            && entry.fetched_at.elapsed() < ttl
        {
            self.log_sources_stats.record_hit();
            return (entry.payload, entry.revision);
        }

        self.log_sources_stats.record_miss();
        let _refresh_guard = self.log_sources_refresh_lock.lock().await;
        if ttl != Duration::from_millis(0)
            && let Some(entry) = self.log_sources_cache.read().await.clone()
            && entry.fetched_at.elapsed() < ttl
        {
            self.log_sources_stats.record_hit();
            return (entry.payload, entry.revision);
        }

        let stale = self.log_sources_cache.read().await.clone();
        let base = build_log_sources();
        let sources = match tokio::time::timeout(log_sources_refresh_timeout(), logs::hydrate_systemd_statuses(base.clone())).await {
            Ok(hydrated) => hydrated,
            Err(_) => {
                warn!(timeout_ms = log_sources_refresh_timeout().as_millis(), "log source hydration timed out");
                if let Some(entry) = stale {
                    self.log_sources_stats.record_stale_fallback();
                    return (entry.payload, entry.revision);
                }
                base
            }
        };

        let revision = self.log_sources_stats.record_refresh();
        *self.log_sources_cache.write().await = Some(LogSourcesCacheEntry { fetched_at: Instant::now(), revision, payload: sources.clone() });
        (sources, revision)
    }

    pub async fn load_log_sources(&self) -> Vec<LogSource> {
        self.load_log_sources_snapshot().await.0
    }

    pub async fn resolve_log_source(&self, source_id: &str) -> Option<LogSource> {
        self.load_log_sources().await.into_iter().find(|source| source.id == source_id)
    }

    pub async fn read_log_lines(&self, source_id: &str, lines: u64) -> Result<Vec<String>, String> {
        let Some(spec) = self.resolve_log_source(source_id).await else {
            return Err("unknown log source".into());
        };
        let lines = lines.clamp(1, 10_000);

        let output = match spec.kind {
            logs::LogSourceKind::JournalSystem => Command::new("journalctl").args(["-n", &lines.to_string(), "--no-pager", "-o", "short-iso"]).output().await,
            logs::LogSourceKind::JournalUnit => {
                let unit = spec.unit.unwrap_or_default();
                if unit.trim().is_empty() {
                    return Err("invalid unit log source".into());
                }
                Command::new("journalctl").args(["-u", &unit, "-n", &lines.to_string(), "--no-pager", "-o", "short-iso"]).output().await
            }
            logs::LogSourceKind::Dmesg => Command::new("dmesg").args(["--color=never", "--ctime"]).output().await,
            logs::LogSourceKind::File => {
                let path = spec.path.unwrap_or_default();
                if path.trim().is_empty() {
                    return Err("invalid file log source".into());
                }
                Command::new("tail").arg("-n").arg(lines.to_string()).arg(PathBuf::from(path)).output().await
            }
        }
        .map_err(|err| format!("failed to fetch logs: {err}"))?;

        if !output.status.success() {
            return Err(format!("log fetch failed (status {})", output.status));
        }

        let text = String::from_utf8_lossy(&output.stdout);
        Ok(text.lines().map(|line| line.to_string()).collect())
    }

    pub fn spawn_log_download(&self, source: &LogSource, lines: Option<usize>) -> Result<(Child, String), String> {
        let filename = sanitize_log_filename(&source.label);

        let mut cmd = match source.kind {
            logs::LogSourceKind::JournalSystem => {
                let mut cmd = Command::new("journalctl");
                cmd.args(["--no-pager", "-o", "short-iso"]);
                if let Some(lines) = lines {
                    cmd.args(["-n", &lines.to_string()]);
                }
                cmd
            }
            logs::LogSourceKind::JournalUnit => {
                let unit = source.unit.clone().unwrap_or_default();
                if unit.trim().is_empty() {
                    return Err("invalid unit log source".into());
                }
                let mut cmd = Command::new("journalctl");
                cmd.args(["-u", &unit, "--no-pager", "-o", "short-iso"]);
                if let Some(lines) = lines {
                    cmd.args(["-n", &lines.to_string()]);
                }
                cmd
            }
            logs::LogSourceKind::Dmesg => {
                if let Some(lines) = lines {
                    let script = format!("dmesg --color=never --ctime 2>/dev/null | tail -n {}", lines);
                    let mut cmd = Command::new("sh");
                    cmd.args(["-c", &script]);
                    cmd
                } else {
                    let mut cmd = Command::new("dmesg");
                    cmd.args(["--color=never", "--ctime"]);
                    cmd
                }
            }
            logs::LogSourceKind::File => {
                let path = source.path.clone().unwrap_or_default();
                if path.trim().is_empty() {
                    return Err("invalid file log source".into());
                }
                let path = PathBuf::from(path);
                if let Some(lines) = lines {
                    let mut cmd = Command::new("tail");
                    cmd.arg("-n").arg(lines.to_string()).arg(path);
                    cmd
                } else {
                    let mut cmd = Command::new("cat");
                    cmd.arg(path);
                    cmd
                }
            }
        };

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::null());
        cmd.spawn().map(|child| (child, filename)).map_err(|err| format!("failed to spawn log download: {err}"))
    }

    pub fn spawn_log_stream(&self, source: &LogSource, lines: usize, follow: bool) -> Result<Child, String> {
        let follow_flag = if follow { "true" } else { "false" };
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-lc").env("LINES", lines.to_string()).env("FOLLOW", follow_flag).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);

        let script = match source.id.as_str() {
            "journal" => journal_script(None, lines, follow),
            "dmesg" => dmesg_script(lines, follow),
            _ => {
                if let Some(unit) = source.unit.as_deref() {
                    journal_script(Some(unit), lines, follow)
                } else if let Some(path) = source.path.as_deref()
                    && source.id.starts_with("file:")
                {
                    file_script(path, lines, follow)
                } else {
                    return Err("unsupported log source".into());
                }
            }
        };

        cmd.arg(script).spawn().map_err(|err| format!("failed to start log source: {err}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_log_filename_normalizes_labels() {
        assert_eq!(sanitize_log_filename(" helios api "), "helios_api.log");
        assert_eq!(sanitize_log_filename(""), "logs.log");
        assert_eq!(sanitize_log_filename("..."), "logs.log");
    }

    #[test]
    fn script_builders_preserve_expected_modes() {
        assert!(journal_script(Some("helios-api"), 100, true).contains("-u helios-api"));
        assert!(journal_script(None, 100, false).contains("-n 100"));
        assert_eq!(file_script("/var/log/test.log", 50, false), "tail -n 50 /var/log/test.log");
        assert!(file_script("/var/log/test.log", 50, true).contains("-F /var/log/test.log"));
        assert!(dmesg_script(25, true).contains("journalctl -k"));
        assert!(dmesg_script(25, false).contains("tail -n 25"));
    }
}
