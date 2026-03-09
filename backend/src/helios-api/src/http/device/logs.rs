use super::super::error::{ApiError, ApiResult};
use crate::logs;
use crate::logs::LogSource;
use axum::body::Body;
use axum::http::{HeaderMap, StatusCode, header};
use axum::{
    Json,
    extract::Query,
    response::{IntoResponse, Response},
};
use std::path::PathBuf;
use std::sync::OnceLock;
use tokio::process::{Child, Command};
use tokio::sync::RwLock;
use tokio::time::{Duration, Instant};
use tokio_util::io::ReaderStream;
use tracing::warn;

#[derive(Clone)]
struct LogSourcesCacheEntry {
    fetched_at: Instant,
    payload: Vec<LogSource>,
}

fn log_sources_cache() -> &'static RwLock<Option<LogSourcesCacheEntry>> {
    static CACHE: OnceLock<RwLock<Option<LogSourcesCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

fn log_sources_refresh_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn read_duration_env(var: &str, default_ms: u64, min_ms: u64, max_ms: u64) -> Duration {
    let ms = std::env::var(var).ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(default_ms);
    Duration::from_millis(ms.clamp(min_ms, max_ms))
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

async fn load_log_sources() -> Vec<LogSource> {
    let ttl = log_sources_cache_ttl();
    if ttl != Duration::from_millis(0)
        && let Some(entry) = log_sources_cache().read().await.clone()
        && entry.fetched_at.elapsed() < ttl
    {
        return entry.payload;
    }

    let _refresh_guard = log_sources_refresh_lock().lock().await;
    if ttl != Duration::from_millis(0)
        && let Some(entry) = log_sources_cache().read().await.clone()
        && entry.fetched_at.elapsed() < ttl
    {
        return entry.payload;
    }

    let stale = log_sources_cache().read().await.clone().map(|entry| entry.payload);
    let base = build_log_sources();
    let sources = match tokio::time::timeout(log_sources_refresh_timeout(), logs::hydrate_systemd_statuses(base.clone())).await {
        Ok(hydrated) => hydrated,
        Err(_) => {
            warn!(timeout_ms = log_sources_refresh_timeout().as_millis(), "log source hydration timed out");
            stale.unwrap_or(base)
        }
    };

    *log_sources_cache().write().await = Some(LogSourcesCacheEntry { fetched_at: Instant::now(), payload: sources.clone() });
    sources
}

#[utoipa::path(
    get,
    path = "/device/logs",
    tag = "Device",
    responses((status = 200, description = "Logs placeholder", body = [String]))
)]
pub async fn logs(Query(params): Query<LogParams>) -> ApiResult<Json<Vec<String>>> {
    let source = params.source.unwrap_or_else(|| "unit:helios-engine.service".to_string());
    let lines = params.lines.unwrap_or(250).clamp(1, 10_000);

    let sources = load_log_sources().await;
    let Some(spec) = sources.into_iter().find(|s| s.id == source) else {
        return Err(ApiError::bad_request("unknown log source"));
    };

    let output = match spec.kind {
        logs::LogSourceKind::JournalSystem => Command::new("journalctl").args(["-n", &lines.to_string(), "--no-pager", "-o", "short-iso"]).output().await,
        logs::LogSourceKind::JournalUnit => {
            let unit = spec.unit.unwrap_or_default();
            if unit.trim().is_empty() {
                return Err(ApiError::bad_request("invalid unit log source"));
            }
            Command::new("journalctl").args(["-u", &unit, "-n", &lines.to_string(), "--no-pager", "-o", "short-iso"]).output().await
        }
        logs::LogSourceKind::Dmesg => Command::new("dmesg").args(["--color=never", "--ctime"]).output().await,
        logs::LogSourceKind::File => {
            let path = spec.path.unwrap_or_default();
            if path.trim().is_empty() {
                return Err(ApiError::bad_request("invalid file log source"));
            }
            let path = PathBuf::from(path);
            Command::new("tail").arg("-n").arg(lines.to_string()).arg(path).output().await
        }
    }
    .map_err(|err| ApiError::internal(format!("failed to fetch logs: {err}")))?;

    if !output.status.success() {
        return Err(ApiError::internal(format!("log fetch failed (status {})", output.status)));
    }

    let text = String::from_utf8_lossy(&output.stdout);
    Ok(Json(text.lines().map(|s| s.to_string()).collect()))
}

#[utoipa::path(
    get,
    path = "/device/logs/download",
    operation_id = "device_logs_download",
    tag = "Device",
    params(
        ("source" = Option<String>, Query, description = "Log source ID"),
        ("lines" = Option<u64>, Query, description = "Optional line limit; omit for full output")
    ),
    responses((status = 200, description = "Log download", content_type = "text/plain"))
)]
pub async fn download(Query(params): Query<LogParams>) -> Response {
    let source = params.source.unwrap_or_else(|| "unit:helios-engine.service".to_string());
    let lines = params.lines.map(|value| value.clamp(1, 200_000) as usize);

    let sources = load_log_sources().await;
    let Some(spec) = sources.into_iter().find(|s| s.id == source) else {
        return (StatusCode::BAD_REQUEST, "unknown log source").into_response();
    };

    let (mut child, suggested_filename) = match spawn_log_download(&spec, lines) {
        Ok(value) => value,
        Err(err) => return (StatusCode::BAD_GATEWAY, err).into_response(),
    };

    let Some(stdout) = child.stdout.take() else {
        return (StatusCode::BAD_GATEWAY, "log download unavailable").into_response();
    };

    tokio::spawn(async move {
        let _ = child.wait().await;
    });

    let stream = ReaderStream::new(stdout);
    let body = Body::from_stream(stream);
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, "text/plain; charset=utf-8".parse().unwrap());
    headers.insert(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{suggested_filename}\"").parse().unwrap());
    (headers, body).into_response()
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct LogParams {
    pub source: Option<String>,
    pub lines: Option<u64>,
}

fn sanitize_filename(value: &str) -> String {
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

fn spawn_log_download(source: &LogSource, lines: Option<usize>) -> Result<(Child, String), String> {
    let filename = sanitize_filename(&source.label);

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

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::null());
    cmd.spawn().map(|child| (child, filename)).map_err(|err| format!("failed to spawn log download: {err}"))
}

#[utoipa::path(
    get,
    path = "/device/logs/sources",
    tag = "Device",
    responses((status = 200, description = "Available log sources", body = [LogSource]))
)]
pub async fn sources() -> ApiResult<Json<Vec<LogSource>>> {
    Ok(Json(load_log_sources().await))
}

#[utoipa::path(
    get,
    path = "/device/console",
    tag = "Device",
    responses((status = 200, description = "Console placeholder", body = [String]))
)]
pub async fn console() -> ApiResult<Json<Vec<String>>> {
    Ok(Json(Vec::new()))
}
