use std::path::Path;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundedUsizePolicy {
    pub env_var: &'static str,
    pub default: usize,
    pub min: usize,
    pub max: usize,
}

impl BoundedUsizePolicy {
    pub fn resolve(self) -> usize {
        std::env::var(self.env_var).ok().and_then(|value| value.trim().parse::<usize>().ok()).unwrap_or(self.default).clamp(self.min, self.max)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoundedU64Policy {
    pub env_var: &'static str,
    pub default: u64,
    pub min: u64,
    pub max: u64,
}

impl BoundedU64Policy {
    pub fn resolve(self) -> u64 {
        std::env::var(self.env_var).ok().and_then(|value| value.trim().parse::<u64>().ok()).unwrap_or(self.default).clamp(self.min, self.max)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptionalBoundedUsizePolicy {
    pub env_var: &'static str,
    pub min: usize,
    pub max: usize,
}

impl OptionalBoundedUsizePolicy {
    pub fn resolve(self) -> Option<usize> {
        std::env::var(self.env_var).ok().and_then(|value| value.trim().parse::<usize>().ok()).map(|value| value.clamp(self.min, self.max))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoolPolicy {
    pub env_var: &'static str,
    pub default: bool,
}

impl BoolPolicy {
    pub fn resolve(self) -> bool {
        match std::env::var(self.env_var) {
            Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "on" => true,
                "0" | "false" | "no" | "off" => false,
                _ => self.default,
            },
            Err(_) => self.default,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StringPolicy {
    pub env_var: &'static str,
    pub default: &'static str,
}

impl StringPolicy {
    pub fn resolve(self) -> String {
        std::env::var(self.env_var).ok().map(|value| value.trim().to_string()).filter(|value| !value.is_empty()).unwrap_or_else(|| self.default.to_string())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TokioRuntimePolicy {
    pub worker_threads: BoundedUsizePolicy,
    pub max_blocking_threads: BoundedUsizePolicy,
    pub thread_stack_bytes: Option<BoundedUsizePolicy>,
    pub blocking_keep_alive_ms: Option<BoundedU64Policy>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedTokioRuntimePolicy {
    pub worker_threads: usize,
    pub max_blocking_threads: usize,
    pub thread_stack_bytes: Option<usize>,
    pub blocking_keep_alive: Option<Duration>,
}

impl TokioRuntimePolicy {
    pub fn resolve(self) -> ResolvedTokioRuntimePolicy {
        ResolvedTokioRuntimePolicy {
            worker_threads: self.worker_threads.resolve(),
            max_blocking_threads: self.max_blocking_threads.resolve(),
            thread_stack_bytes: self.thread_stack_bytes.map(BoundedUsizePolicy::resolve),
            blocking_keep_alive: self.blocking_keep_alive_ms.map(|policy| Duration::from_millis(policy.resolve())),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApiStartupCacheWarmPolicy {
    pub initial_delay_ms: BoundedU64Policy,
    pub retry_delay_ms: BoundedU64Policy,
    pub attempts: BoundedUsizePolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedApiStartupCacheWarmPolicy {
    pub initial_delay_ms: u64,
    pub retry_delay_ms: u64,
    pub attempts: usize,
}

impl ApiStartupCacheWarmPolicy {
    pub fn resolve(self) -> ResolvedApiStartupCacheWarmPolicy {
        ResolvedApiStartupCacheWarmPolicy { initial_delay_ms: self.initial_delay_ms.resolve(), retry_delay_ms: self.retry_delay_ms.resolve(), attempts: self.attempts.resolve() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogSourcesPolicy {
    pub cache_ms: BoundedU64Policy,
    pub refresh_timeout_ms: BoundedU64Policy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedLogSourcesPolicy {
    pub cache_ms: u64,
    pub refresh_timeout_ms: u64,
}

impl LogSourcesPolicy {
    pub fn resolve(self) -> ResolvedLogSourcesPolicy {
        ResolvedLogSourcesPolicy { cache_ms: self.cache_ms.resolve(), refresh_timeout_ms: self.refresh_timeout_ms.resolve() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct I2cInventoryPolicy {
    pub timeout_ms: BoundedU64Policy,
    pub cache_ttl_ms: BoundedU64Policy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedI2cInventoryPolicy {
    pub timeout_ms: u64,
    pub cache_ttl_ms: u64,
}

impl I2cInventoryPolicy {
    pub fn resolve(self) -> ResolvedI2cInventoryPolicy {
        ResolvedI2cInventoryPolicy { timeout_ms: self.timeout_ms.resolve(), cache_ttl_ms: self.cache_ttl_ms.resolve() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImuRuntimePolicy {
    pub idle_interval_ms: BoundedU64Policy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedImuRuntimePolicy {
    pub idle_interval_ms: u64,
}

impl ImuRuntimePolicy {
    pub fn resolve(self) -> ResolvedImuRuntimePolicy {
        ResolvedImuRuntimePolicy { idle_interval_ms: self.idle_interval_ms.resolve() }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceGuardPolicy {
    pub enabled: BoolPolicy,
    pub poll_ms: BoundedU64Policy,
    pub mem_low_kb: BoundedU64Policy,
    pub mem_recover_kb: BoundedU64Policy,
    pub cooldown_ms: BoundedU64Policy,
    pub metrics_top_n: BoundedUsizePolicy,
    pub metrics_timeout_ms: BoundedU64Policy,
    pub allow_stop_fallback: BoolPolicy,
    pub stop_timeout_ms: BoundedU64Policy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedResourceGuardPolicy {
    pub enabled: bool,
    pub poll_ms: u64,
    pub mem_low_kb: u64,
    pub mem_recover_kb: u64,
    pub cooldown_ms: u64,
    pub metrics_top_n: usize,
    pub metrics_timeout_ms: u64,
    pub allow_stop_fallback: bool,
    pub stop_timeout_ms: u64,
}

impl ResourceGuardPolicy {
    pub fn resolve(self) -> ResolvedResourceGuardPolicy {
        let mem_low_kb = self.mem_low_kb.resolve();
        let mem_recover_floor = mem_low_kb.saturating_add(64 * 1024);
        let resolved_mem_recover_kb = self.mem_recover_kb.resolve().max(mem_recover_floor);
        ResolvedResourceGuardPolicy {
            enabled: self.enabled.resolve(),
            poll_ms: self.poll_ms.resolve(),
            mem_low_kb,
            mem_recover_kb: resolved_mem_recover_kb,
            cooldown_ms: self.cooldown_ms.resolve(),
            metrics_top_n: self.metrics_top_n.resolve(),
            metrics_timeout_ms: self.metrics_timeout_ms.resolve(),
            allow_stop_fallback: self.allow_stop_fallback.resolve(),
            stop_timeout_ms: self.stop_timeout_ms.resolve(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StyxCaptureTunablesPolicy {
    pub queue_depth: OptionalBoundedUsizePolicy,
    pub pool_min: OptionalBoundedUsizePolicy,
    pub pool_bytes: OptionalBoundedUsizePolicy,
    pub pool_spare: OptionalBoundedUsizePolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedStyxCaptureTunablesPolicy {
    pub queue_depth: Option<usize>,
    pub pool_min: Option<usize>,
    pub pool_bytes: Option<usize>,
    pub pool_spare: Option<usize>,
}

impl ResolvedStyxCaptureTunablesPolicy {
    pub fn any_overridden(&self) -> bool {
        self.queue_depth.is_some() || self.pool_min.is_some() || self.pool_bytes.is_some() || self.pool_spare.is_some()
    }
}

impl StyxCaptureTunablesPolicy {
    pub fn resolve(self) -> ResolvedStyxCaptureTunablesPolicy {
        ResolvedStyxCaptureTunablesPolicy { queue_depth: self.queue_depth.resolve(), pool_min: self.pool_min.resolve(), pool_bytes: self.pool_bytes.resolve(), pool_spare: self.pool_spare.resolve() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlatformFamily {
    RaspberryPi,
    GenericLinux,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlatformIdentity {
    pub family: PlatformFamily,
    pub model: Option<String>,
    pub architecture: String,
}

pub const HELIOS_LOG_FILTER_POLICY: StringPolicy = StringPolicy { env_var: "RUST_LOG", default: "info" };

pub const HELIOS_API_STARTUP_CACHE_WARM_POLICY: ApiStartupCacheWarmPolicy = ApiStartupCacheWarmPolicy {
    initial_delay_ms: BoundedU64Policy { env_var: "HELIOS_STARTUP_CACHE_WARM_DELAY_MS", default: 1_500, min: 0, max: 30_000 },
    retry_delay_ms: BoundedU64Policy { env_var: "HELIOS_STARTUP_CACHE_WARM_RETRY_MS", default: 1_000, min: 100, max: 30_000 },
    attempts: BoundedUsizePolicy { env_var: "HELIOS_STARTUP_CACHE_WARM_ATTEMPTS", default: 4, min: 1, max: 10 },
};

pub const HELIOS_API_LOG_SOURCES_POLICY: LogSourcesPolicy = LogSourcesPolicy {
    cache_ms: BoundedU64Policy { env_var: "HELIOS_LOG_SOURCES_CACHE_MS", default: 5_000, min: 0, max: 60_000 },
    refresh_timeout_ms: BoundedU64Policy { env_var: "HELIOS_LOG_SOURCES_REFRESH_TIMEOUT_MS", default: 3_000, min: 500, max: 15_000 },
};

pub const HELIOS_I2C_INVENTORY_POLICY: I2cInventoryPolicy = I2cInventoryPolicy {
    timeout_ms: BoundedU64Policy { env_var: "HELIOS_I2C_INVENTORY_TIMEOUT_MS", default: 5_000, min: 100, max: 30_000 },
    cache_ttl_ms: BoundedU64Policy { env_var: "HELIOS_I2C_INVENTORY_CACHE_TTL_MS", default: 2_000, min: 0, max: 60_000 },
};

pub const HELIOS_IMU_RUNTIME_POLICY: ImuRuntimePolicy = ImuRuntimePolicy { idle_interval_ms: BoundedU64Policy { env_var: "HELIOS_IMU_IDLE_INTERVAL_MS", default: 100, min: 20, max: 5_000 } };

pub const HELIOS_RESOURCE_GUARD_POLICY: ResourceGuardPolicy = ResourceGuardPolicy {
    enabled: BoolPolicy { env_var: "HELIOS_RESOURCE_GUARD_ENABLED", default: true },
    poll_ms: BoundedU64Policy { env_var: "HELIOS_RESOURCE_GUARD_POLL_MS", default: 1_500, min: 250, max: 60_000 },
    mem_low_kb: BoundedU64Policy { env_var: "HELIOS_RESOURCE_GUARD_MEM_LOW_KB", default: 700_000, min: 64 * 1024, max: u64::MAX },
    mem_recover_kb: BoundedU64Policy { env_var: "HELIOS_RESOURCE_GUARD_MEM_RECOVER_KB", default: 1_000_000, min: 64 * 1024, max: u64::MAX },
    cooldown_ms: BoundedU64Policy { env_var: "HELIOS_RESOURCE_GUARD_COOLDOWN_MS", default: 5_000, min: 500, max: 60_000 },
    metrics_top_n: BoundedUsizePolicy { env_var: "HELIOS_RESOURCE_GUARD_METRICS_TOP_N", default: 6, min: 1, max: 24 },
    metrics_timeout_ms: BoundedU64Policy { env_var: "HELIOS_RESOURCE_GUARD_METRICS_TIMEOUT_MS", default: 300, min: 50, max: 60_000 },
    allow_stop_fallback: BoolPolicy { env_var: "HELIOS_RESOURCE_GUARD_ALLOW_STOP_FALLBACK", default: false },
    stop_timeout_ms: BoundedU64Policy { env_var: "HELIOS_RESOURCE_GUARD_STOP_TIMEOUT_MS", default: 4_000, min: 500, max: 60_000 },
};

pub const HELIOS_STYX_CAPTURE_TUNABLES_POLICY: StyxCaptureTunablesPolicy = StyxCaptureTunablesPolicy {
    queue_depth: OptionalBoundedUsizePolicy { env_var: "HELIOS_STYX_CAPTURE_QUEUE_DEPTH", min: 1, max: 512 },
    pool_min: OptionalBoundedUsizePolicy { env_var: "HELIOS_STYX_CAPTURE_POOL_MIN", min: 1, max: 512 },
    pool_bytes: OptionalBoundedUsizePolicy { env_var: "HELIOS_STYX_CAPTURE_POOL_BYTES", min: 1, max: usize::MAX },
    pool_spare: OptionalBoundedUsizePolicy { env_var: "HELIOS_STYX_CAPTURE_POOL_SPARE", min: 0, max: 512 },
};

pub fn detect_platform_identity() -> PlatformIdentity {
    let model = read_first_non_empty(&["/sys/firmware/devicetree/base/model", "/proc/device-tree/model"]);
    let architecture = std::env::consts::ARCH.to_string();
    PlatformIdentity { family: classify_platform_family(model.as_deref(), &architecture), model, architecture }
}

pub fn classify_platform_family(model: Option<&str>, architecture: &str) -> PlatformFamily {
    let normalized_model = model.unwrap_or_default().trim().to_ascii_lowercase();
    if normalized_model.contains("raspberry pi") {
        return PlatformFamily::RaspberryPi;
    }
    match architecture {
        "aarch64" | "arm" | "armv7" | "armv7l" | "armv6" | "armv6l" => PlatformFamily::GenericLinux,
        "" => PlatformFamily::Unknown,
        _ => PlatformFamily::GenericLinux,
    }
}

fn read_first_non_empty(paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|path| {
        let path = Path::new(path);
        std::fs::read_to_string(path).ok().map(|value| value.trim_matches(char::from(0)).trim().to_string()).filter(|value| !value.is_empty())
    })
}

mod generated;

pub use generated::*;

#[cfg(test)]
mod tests {
    use super::{
        HELIOS_API_LOG_SOURCES_POLICY, HELIOS_API_STARTUP_CACHE_WARM_POLICY, HELIOS_API_TOKIO_POLICY, HELIOS_ENGINE_TOKIO_POLICY, HELIOS_I2C_INVENTORY_POLICY, HELIOS_IMU_RUNTIME_POLICY,
        HELIOS_LOG_FILTER_POLICY, HELIOS_PERIPHERALS_TOKIO_POLICY, HELIOS_RESOURCE_GUARD_POLICY, HELIOS_STYX_CAPTURE_TUNABLES_POLICY, PlatformFamily, classify_platform_family,
    };

    #[test]
    fn api_runtime_policy_defaults_match_expected_values() {
        let resolved = HELIOS_API_TOKIO_POLICY.resolve();
        assert_eq!(resolved.worker_threads, 2);
        assert_eq!(resolved.max_blocking_threads, 4);
        assert_eq!(resolved.thread_stack_bytes, Some(1_048_576));
        assert_eq!(resolved.blocking_keep_alive.map(|value| value.as_millis()), Some(500));
    }

    #[test]
    fn engine_runtime_policy_defaults_match_expected_values() {
        let resolved = HELIOS_ENGINE_TOKIO_POLICY.resolve();
        assert_eq!(resolved.worker_threads, 4);
        assert_eq!(resolved.max_blocking_threads, 4);
        assert_eq!(resolved.thread_stack_bytes, Some(2_097_152));
        assert_eq!(resolved.blocking_keep_alive.map(|value| value.as_millis()), Some(3_000));
    }

    #[test]
    fn peripherals_runtime_policy_keeps_optional_stack_and_keepalive_unset() {
        let resolved = HELIOS_PERIPHERALS_TOKIO_POLICY.resolve();
        assert_eq!(resolved.worker_threads, 1);
        assert_eq!(resolved.max_blocking_threads, 1);
        assert_eq!(resolved.thread_stack_bytes, None);
        assert_eq!(resolved.blocking_keep_alive, None);
    }

    #[test]
    fn api_startup_cache_warm_policy_defaults_match_expected_values() {
        let resolved = HELIOS_API_STARTUP_CACHE_WARM_POLICY.resolve();
        assert_eq!(resolved.initial_delay_ms, 1_500);
        assert_eq!(resolved.retry_delay_ms, 1_000);
        assert_eq!(resolved.attempts, 4);
    }

    #[test]
    fn log_sources_policy_defaults_match_expected_values() {
        let resolved = HELIOS_API_LOG_SOURCES_POLICY.resolve();
        assert_eq!(resolved.cache_ms, 5_000);
        assert_eq!(resolved.refresh_timeout_ms, 3_000);
    }

    #[test]
    fn i2c_inventory_policy_defaults_match_expected_values() {
        let resolved = HELIOS_I2C_INVENTORY_POLICY.resolve();
        assert_eq!(resolved.timeout_ms, 5_000);
        assert_eq!(resolved.cache_ttl_ms, 2_000);
    }

    #[test]
    fn imu_runtime_policy_defaults_match_expected_values() {
        let resolved = HELIOS_IMU_RUNTIME_POLICY.resolve();
        assert_eq!(resolved.idle_interval_ms, 100);
    }

    #[test]
    fn resource_guard_policy_defaults_match_expected_values() {
        let resolved = HELIOS_RESOURCE_GUARD_POLICY.resolve();
        assert!(resolved.enabled);
        assert_eq!(resolved.poll_ms, 1_500);
        assert_eq!(resolved.mem_low_kb, 700_000);
        assert_eq!(resolved.mem_recover_kb, 1_000_000);
        assert_eq!(resolved.cooldown_ms, 5_000);
        assert_eq!(resolved.metrics_top_n, 6);
        assert_eq!(resolved.metrics_timeout_ms, 300);
        assert!(!resolved.allow_stop_fallback);
        assert_eq!(resolved.stop_timeout_ms, 4_000);
    }

    #[test]
    fn styx_capture_policy_is_optional_by_default() {
        let resolved = HELIOS_STYX_CAPTURE_TUNABLES_POLICY.resolve();
        assert_eq!(resolved.queue_depth, None);
        assert_eq!(resolved.pool_min, None);
        assert_eq!(resolved.pool_bytes, None);
        assert_eq!(resolved.pool_spare, None);
        assert!(!resolved.any_overridden());
    }

    #[test]
    fn log_filter_policy_defaults_to_info() {
        assert_eq!(HELIOS_LOG_FILTER_POLICY.default, "info");
    }

    #[test]
    fn classify_platform_family_detects_raspberry_pi() {
        assert_eq!(classify_platform_family(Some("Raspberry Pi Compute Module 5 Rev 1.0"), "aarch64"), PlatformFamily::RaspberryPi);
    }

    #[test]
    fn classify_platform_family_falls_back_to_generic_linux_for_aarch64() {
        assert_eq!(classify_platform_family(None, "aarch64"), PlatformFamily::GenericLinux);
    }
}
