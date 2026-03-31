use std::{sync::OnceLock, time::Duration};

pub(super) const ENGINE_SOCKET: &str = "/run/helios/engine.sock";
pub(super) const DEV_ENGINE_SOCKET: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/target/dev/run/engine.sock");
pub(super) const ENGINE_RESPONSE_TIMEOUT: Duration = Duration::from_secs(10);
pub(super) const ENGINE_PIPELINE_LAYOUT_TIMEOUT: Duration = Duration::from_secs(30);
pub(super) const ENGINE_START_STREAM_TIMEOUT: Duration = Duration::from_secs(60);
pub(super) const ENGINE_STOP_STREAM_TIMEOUT: Duration = Duration::from_secs(30);
pub(super) const ENGINE_STOP_RECORDING_TIMEOUT: Duration = Duration::from_secs(180);
pub(super) const ENGINE_CAPTURE_SHADOW_TIMEOUT: Duration = Duration::from_secs(300);
pub(super) const ENGINE_RECONNECT_INITIAL: Duration = Duration::from_millis(200);
pub(super) const ENGINE_RECONNECT_MAX: Duration = Duration::from_secs(5);

const TIMEOUT_SCALE_PPM_BASE: u64 = 1_000_000;

pub(super) fn calibration_solve_timeout() -> Duration {
    const DEFAULT_SECS: u64 = 300;
    const MIN_SECS: u64 = 30;
    const MAX_SECS: u64 = 1800;
    let secs = std::env::var("HELIOS_CALIBRATION_SOLVE_TIMEOUT_SECS").ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(DEFAULT_SECS).clamp(MIN_SECS, MAX_SECS);
    Duration::from_secs(secs)
}

pub(super) fn localization_solve_timeout() -> Duration {
    const DEFAULT_MS: u64 = 30_000;
    const MIN_MS: u64 = 1_000;
    const MAX_MS: u64 = 300_000;
    read_timeout_env("HELIOS_ENGINE_LOCALIZATION_SOLVE_TIMEOUT_MS", DEFAULT_MS, MIN_MS, MAX_MS)
}

fn read_timeout_env(var: &str, default_ms: u64, min_ms: u64, max_ms: u64) -> Duration {
    let ms = std::env::var(var).ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(default_ms);
    Duration::from_millis(ms.clamp(min_ms, max_ms))
}

fn read_usize_env(var: &str, default_value: usize, min_value: usize, max_value: usize) -> usize {
    let value = std::env::var(var).ok().and_then(|raw| raw.trim().parse::<usize>().ok()).unwrap_or(default_value);
    value.clamp(min_value, max_value)
}

pub(super) fn engine_request_send_timeout() -> Duration {
    static VALUE: OnceLock<Duration> = OnceLock::new();
    *VALUE.get_or_init(|| read_timeout_env("HELIOS_ENGINE_REQUEST_SEND_TIMEOUT_MS", 1_000, 50, 10_000))
}

pub(super) fn engine_command_send_timeout() -> Duration {
    static VALUE: OnceLock<Duration> = OnceLock::new();
    *VALUE.get_or_init(|| read_timeout_env("HELIOS_ENGINE_COMMAND_SEND_TIMEOUT_MS", 2_000, 100, 15_000))
}

pub(super) fn engine_request_queue_size(stream_count: usize) -> usize {
    let base = read_usize_env("HELIOS_ENGINE_REQUEST_QUEUE", 256, 32, 8_192);
    let per_stream = read_usize_env("HELIOS_ENGINE_REQUEST_QUEUE_PER_STREAM", 8, 0, 256);
    let max = read_usize_env("HELIOS_ENGINE_REQUEST_QUEUE_MAX", 2_048, base, 8_192);
    let extra = stream_count.saturating_mul(per_stream);
    base.saturating_add(extra).min(max)
}

fn timeout_scale_per_stream_ppm() -> u64 {
    static VALUE: OnceLock<u64> = OnceLock::new();
    *VALUE.get_or_init(|| std::env::var("HELIOS_ENGINE_TIMEOUT_PER_STREAM_PPM").ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(50_000))
}

fn timeout_scale_max_ppm() -> u64 {
    static VALUE: OnceLock<u64> = OnceLock::new();
    *VALUE.get_or_init(|| std::env::var("HELIOS_ENGINE_TIMEOUT_SCALE_MAX_PPM").ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(2_000_000))
}

pub(super) fn timeout_scale_for_streams(stream_count: usize) -> u64 {
    let per_stream = timeout_scale_per_stream_ppm();
    let max = timeout_scale_max_ppm().max(TIMEOUT_SCALE_PPM_BASE);
    let scale = TIMEOUT_SCALE_PPM_BASE.saturating_add(per_stream.saturating_mul(stream_count as u64));
    scale.min(max)
}

pub(super) fn scale_timeout(base: Duration, scale_ppm: u64) -> Duration {
    if base.is_zero() || scale_ppm == TIMEOUT_SCALE_PPM_BASE {
        return base;
    }
    let base_ms = base.as_millis().min(u128::from(u64::MAX)) as u64;
    let scaled = base_ms.saturating_mul(scale_ppm).saturating_div(TIMEOUT_SCALE_PPM_BASE);
    Duration::from_millis(scaled.max(1))
}
