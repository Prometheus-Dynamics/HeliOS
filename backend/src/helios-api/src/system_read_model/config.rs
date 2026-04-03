use std::sync::OnceLock;

use lib_runtime_policy::HELIOS_API_SYSTEM_READ_MODEL_POLICY;
use tokio::time::Duration;

fn api_sampler_thread_stack_bytes() -> usize {
    static VALUE: OnceLock<usize> = OnceLock::new();
    *VALUE.get_or_init(|| HELIOS_API_SYSTEM_READ_MODEL_POLICY.resolve().sampler_thread_stack_bytes)
}

pub(super) fn spawn_api_sampler_thread(name: &'static str, f: impl FnOnce() + Send + 'static) -> std::io::Result<std::thread::JoinHandle<()>> {
    std::thread::Builder::new().name(name.to_string()).stack_size(api_sampler_thread_stack_bytes()).spawn(f)
}

pub(super) fn metrics_cache_ttl() -> Duration {
    static TTL: OnceLock<Duration> = OnceLock::new();
    *TTL.get_or_init(|| Duration::from_millis(HELIOS_API_SYSTEM_READ_MODEL_POLICY.resolve().device_metrics_cache_ms))
}

pub(super) fn devices_updates_stream_poll_interval() -> Duration {
    static TTL: OnceLock<Duration> = OnceLock::new();
    *TTL.get_or_init(|| Duration::from_millis(HELIOS_API_SYSTEM_READ_MODEL_POLICY.resolve().device_updates_stream_poll_ms))
}

pub(super) fn process_metrics_cache_ttl() -> Duration {
    static TTL: OnceLock<Duration> = OnceLock::new();
    *TTL.get_or_init(|| Duration::from_millis(HELIOS_API_SYSTEM_READ_MODEL_POLICY.resolve().process_breakdown_cache_ms))
}

pub(super) fn metrics_refresh_timeout() -> Duration {
    static TTL: OnceLock<Duration> = OnceLock::new();
    *TTL.get_or_init(|| Duration::from_millis(HELIOS_API_SYSTEM_READ_MODEL_POLICY.resolve().device_metrics_timeout_ms))
}
