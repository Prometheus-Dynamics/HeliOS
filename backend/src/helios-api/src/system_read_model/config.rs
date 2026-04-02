use std::sync::OnceLock;

use tokio::time::Duration;

pub(super) fn read_duration_env(var: &str, default_ms: u64, min_ms: u64, max_ms: u64) -> Duration {
    let ms = std::env::var(var).ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(default_ms);
    Duration::from_millis(ms.clamp(min_ms, max_ms))
}

pub(super) fn read_size_env(var: &str, default: usize, min: usize, max: usize) -> usize {
    std::env::var(var).ok().and_then(|value| value.trim().parse::<usize>().ok()).unwrap_or(default).clamp(min, max)
}

fn api_sampler_thread_stack_bytes() -> usize {
    static VALUE: OnceLock<usize> = OnceLock::new();
    *VALUE.get_or_init(|| read_size_env("HELIOS_API_SAMPLER_THREAD_STACK_BYTES", 512 * 1024, 128 * 1024, 4 * 1024 * 1024))
}

pub(super) fn spawn_api_sampler_thread(name: &'static str, f: impl FnOnce() + Send + 'static) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new().name(name.to_string()).stack_size(api_sampler_thread_stack_bytes()).spawn(f)
}

pub(super) fn metrics_cache_ttl() -> Duration {
    static TTL: OnceLock<Duration> = OnceLock::new();
    *TTL.get_or_init(|| read_duration_env("HELIOS_DEVICE_METRICS_CACHE_MS", 750, 0, 10_000))
}

pub(super) fn devices_updates_stream_poll_interval() -> Duration {
    static TTL: OnceLock<Duration> = OnceLock::new();
    *TTL.get_or_init(|| read_duration_env("HELIOS_DEVICE_UPDATES_STREAM_POLL_MS", 2_000, 250, 60_000))
}

pub(super) fn process_metrics_cache_ttl() -> Duration {
    static TTL: OnceLock<Duration> = OnceLock::new();
    *TTL.get_or_init(|| read_duration_env("HELIOS_DEVICE_PROCESS_BREAKDOWN_CACHE_MS", 5_000, 0, 60_000))
}

pub(super) fn metrics_refresh_timeout() -> Duration {
    static TTL: OnceLock<Duration> = OnceLock::new();
    *TTL.get_or_init(|| read_duration_env("HELIOS_DEVICE_METRICS_TIMEOUT_MS", 2_000, 250, 15_000))
}
