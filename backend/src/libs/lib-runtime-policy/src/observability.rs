use crate::{BoolPolicy, BoundedU64Policy, BoundedUsizePolicy, StringPolicy};

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
pub struct EngineCrashGuardPolicy {
    pub window_ms: BoundedU64Policy,
    pub threshold: BoundedUsizePolicy,
    pub suppress_ms: BoundedU64Policy,
    pub min_downtime_ms: BoundedU64Policy,
    pub poll_ms: BoundedU64Policy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedEngineCrashGuardPolicy {
    pub window_ms: u64,
    pub threshold: usize,
    pub suppress_ms: u64,
    pub min_downtime_ms: u64,
    pub poll_ms: u64,
}

impl EngineCrashGuardPolicy {
    pub fn resolve(self) -> ResolvedEngineCrashGuardPolicy {
        ResolvedEngineCrashGuardPolicy {
            window_ms: self.window_ms.resolve(),
            threshold: self.threshold.resolve(),
            suppress_ms: self.suppress_ms.resolve(),
            min_downtime_ms: self.min_downtime_ms.resolve(),
            poll_ms: self.poll_ms.resolve(),
        }
    }
}

pub const HELIOS_LOG_FILTER_POLICY: StringPolicy = StringPolicy { env_var: "RUST_LOG", default: "info" };

pub const HELIOS_API_LOG_SOURCES_POLICY: LogSourcesPolicy = LogSourcesPolicy {
    cache_ms: BoundedU64Policy { env_var: "HELIOS_LOG_SOURCES_CACHE_MS", default: 5_000, min: 0, max: 60_000 },
    refresh_timeout_ms: BoundedU64Policy { env_var: "HELIOS_LOG_SOURCES_REFRESH_TIMEOUT_MS", default: 3_000, min: 500, max: 15_000 },
};

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

pub const HELIOS_ENGINE_CRASH_GUARD_POLICY: EngineCrashGuardPolicy = EngineCrashGuardPolicy {
    window_ms: BoundedU64Policy { env_var: "HELIOS_ENGINE_CRASH_GUARD_WINDOW_MS", default: 60_000, min: 5_000, max: 15 * 60_000 },
    threshold: BoundedUsizePolicy { env_var: "HELIOS_ENGINE_CRASH_GUARD_THRESHOLD", default: 3, min: 1, max: 10 },
    suppress_ms: BoundedU64Policy { env_var: "HELIOS_ENGINE_CRASH_GUARD_SUPPRESS_MS", default: 300_000, min: 10_000, max: 60 * 60_000 },
    min_downtime_ms: BoundedU64Policy { env_var: "HELIOS_ENGINE_CRASH_GUARD_MIN_DOWNTIME_MS", default: 2_000, min: 250, max: 60_000 },
    poll_ms: BoundedU64Policy { env_var: "HELIOS_ENGINE_CRASH_GUARD_POLL_MS", default: 1_000, min: 200, max: 60_000 },
};
