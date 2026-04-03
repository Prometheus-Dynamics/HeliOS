use super::*;
use std::time::Duration;

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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ApiStreamsPolicy {
    pub cache_ms: BoundedU64Policy,
    pub mjpeg_poll_ms: OptionalBoundedU64Policy,
    pub preview_poll_ms: OptionalBoundedU64Policy,
    pub mjpeg_interval_ms: OptionalBoundedU64Policy,
    pub snapshot_interval_ms: BoundedU64Policy,
    pub preview_max_fps: OptionalBoundedF64Policy,
    pub preview_outage_ms: BoundedU64Policy,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedApiStreamsPolicy {
    pub cache_ms: u64,
    pub mjpeg_poll_ms: Option<u64>,
    pub preview_poll_ms: Option<u64>,
    pub mjpeg_interval_ms: Option<u64>,
    pub snapshot_interval_ms: u64,
    pub preview_max_fps: Option<f64>,
    pub preview_outage_ms: u64,
}

impl ApiStreamsPolicy {
    pub fn resolve(self) -> ResolvedApiStreamsPolicy {
        ResolvedApiStreamsPolicy {
            cache_ms: self.cache_ms.resolve(),
            mjpeg_poll_ms: self.mjpeg_poll_ms.resolve(),
            preview_poll_ms: self.preview_poll_ms.resolve(),
            mjpeg_interval_ms: self.mjpeg_interval_ms.resolve(),
            snapshot_interval_ms: self.snapshot_interval_ms.resolve(),
            preview_max_fps: self.preview_max_fps.resolve(),
            preview_outage_ms: self.preview_outage_ms.resolve(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApiHardwareReadModelPolicy {
    pub peripherals_cache_ms: BoundedU64Policy,
    pub peripherals_timeout_ms: BoundedU64Policy,
    pub camera_discovery_timeout_ms: BoundedU64Policy,
    pub peripherals_refresh_timeout_ms: BoundedU64Policy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedApiHardwareReadModelPolicy {
    pub peripherals_cache_ms: u64,
    pub peripherals_timeout_ms: u64,
    pub camera_discovery_timeout_ms: u64,
    pub peripherals_refresh_timeout_ms: u64,
}

impl ApiHardwareReadModelPolicy {
    pub fn resolve(self) -> ResolvedApiHardwareReadModelPolicy {
        ResolvedApiHardwareReadModelPolicy {
            peripherals_cache_ms: self.peripherals_cache_ms.resolve(),
            peripherals_timeout_ms: self.peripherals_timeout_ms.resolve(),
            camera_discovery_timeout_ms: self.camera_discovery_timeout_ms.resolve(),
            peripherals_refresh_timeout_ms: self.peripherals_refresh_timeout_ms.resolve(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ApiSystemReadModelPolicy {
    pub sampler_thread_stack_bytes: BoundedUsizePolicy,
    pub device_metrics_cache_ms: BoundedU64Policy,
    pub device_updates_stream_poll_ms: BoundedU64Policy,
    pub process_breakdown_cache_ms: BoundedU64Policy,
    pub device_metrics_timeout_ms: BoundedU64Policy,
    pub process_breakdown_limit: BoundedUsizePolicy,
    pub process_breakdown_budget_ms: BoundedU64Policy,
    pub process_mapping_limit: BoundedUsizePolicy,
    pub telemetry_sample_interval_ms: BoundedU64Policy,
    pub processes_sample_interval_ms: BoundedU64Policy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedApiSystemReadModelPolicy {
    pub sampler_thread_stack_bytes: usize,
    pub device_metrics_cache_ms: u64,
    pub device_updates_stream_poll_ms: u64,
    pub process_breakdown_cache_ms: u64,
    pub device_metrics_timeout_ms: u64,
    pub process_breakdown_limit: usize,
    pub process_breakdown_budget_ms: u64,
    pub process_mapping_limit: usize,
    pub telemetry_sample_interval_ms: u64,
    pub processes_sample_interval_ms: u64,
}

impl ApiSystemReadModelPolicy {
    pub fn resolve(self) -> ResolvedApiSystemReadModelPolicy {
        ResolvedApiSystemReadModelPolicy {
            sampler_thread_stack_bytes: self.sampler_thread_stack_bytes.resolve(),
            device_metrics_cache_ms: self.device_metrics_cache_ms.resolve(),
            device_updates_stream_poll_ms: self.device_updates_stream_poll_ms.resolve(),
            process_breakdown_cache_ms: self.process_breakdown_cache_ms.resolve(),
            device_metrics_timeout_ms: self.device_metrics_timeout_ms.resolve(),
            process_breakdown_limit: self.process_breakdown_limit.resolve(),
            process_breakdown_budget_ms: self.process_breakdown_budget_ms.resolve(),
            process_mapping_limit: self.process_mapping_limit.resolve(),
            telemetry_sample_interval_ms: self.telemetry_sample_interval_ms.resolve(),
            processes_sample_interval_ms: self.processes_sample_interval_ms.resolve(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeripheralsPowerPolicy {
    pub poll_interval_ms: BoundedU64Policy,
    pub idle_interval_ms: BoundedU64Policy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedPeripheralsPowerPolicy {
    pub poll_interval_ms: u64,
    pub idle_interval_ms: u64,
}

impl PeripheralsPowerPolicy {
    pub fn resolve(self) -> ResolvedPeripheralsPowerPolicy {
        ResolvedPeripheralsPowerPolicy { poll_interval_ms: self.poll_interval_ms.resolve(), idle_interval_ms: self.idle_interval_ms.resolve() }
    }
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

pub const HELIOS_ENGINE_CRASH_GUARD_POLICY: EngineCrashGuardPolicy = EngineCrashGuardPolicy {
    window_ms: BoundedU64Policy { env_var: "HELIOS_ENGINE_CRASH_GUARD_WINDOW_MS", default: 60_000, min: 5_000, max: 15 * 60_000 },
    threshold: BoundedUsizePolicy { env_var: "HELIOS_ENGINE_CRASH_GUARD_THRESHOLD", default: 3, min: 1, max: 10 },
    suppress_ms: BoundedU64Policy { env_var: "HELIOS_ENGINE_CRASH_GUARD_SUPPRESS_MS", default: 300_000, min: 10_000, max: 60 * 60_000 },
    min_downtime_ms: BoundedU64Policy { env_var: "HELIOS_ENGINE_CRASH_GUARD_MIN_DOWNTIME_MS", default: 2_000, min: 250, max: 60_000 },
    poll_ms: BoundedU64Policy { env_var: "HELIOS_ENGINE_CRASH_GUARD_POLL_MS", default: 1_000, min: 200, max: 60_000 },
};

pub const HELIOS_API_STREAMS_POLICY: ApiStreamsPolicy = ApiStreamsPolicy {
    cache_ms: BoundedU64Policy { env_var: "HELIOS_API_STREAMS_CACHE_MS", default: 750, min: 0, max: 5_000 },
    mjpeg_poll_ms: OptionalBoundedU64Policy { env_var: "HELIOS_MJPEG_POLL_MS", min: 1, max: 100 },
    preview_poll_ms: OptionalBoundedU64Policy { env_var: "HELIOS_PREVIEW_POLL_MS", min: 1, max: 100 },
    mjpeg_interval_ms: OptionalBoundedU64Policy { env_var: "HELIOS_MJPEG_INTERVAL_MS", min: 1, max: 100 },
    snapshot_interval_ms: BoundedU64Policy { env_var: "HELIOS_MJPEG_INTERVAL_MS", default: 33, min: 20, max: 500 },
    preview_max_fps: OptionalBoundedF64Policy { env_var: "HELIOS_PREVIEW_MAX_FPS", min: 1.0, max: 120.0 },
    preview_outage_ms: BoundedU64Policy { env_var: "HELIOS_PREVIEW_OUTAGE_MS", default: 15_000, min: 1_000, max: 15_000 },
};

pub const HELIOS_API_HARDWARE_READ_MODEL_POLICY: ApiHardwareReadModelPolicy = ApiHardwareReadModelPolicy {
    peripherals_cache_ms: BoundedU64Policy { env_var: "HELIOS_PERIPHERALS_CACHE_MS", default: 1_000, min: 0, max: 10_000 },
    peripherals_timeout_ms: BoundedU64Policy { env_var: "HELIOS_PERIPHERALS_TIMEOUT_MS", default: 1_500, min: 250, max: 15_000 },
    camera_discovery_timeout_ms: BoundedU64Policy { env_var: "HELIOS_CAMERA_DISCOVERY_TIMEOUT_MS", default: 1_500, min: 250, max: 20_000 },
    peripherals_refresh_timeout_ms: BoundedU64Policy { env_var: "HELIOS_PERIPHERALS_REFRESH_TIMEOUT_MS", default: 2_500, min: 500, max: 20_000 },
};

pub const HELIOS_API_SYSTEM_READ_MODEL_POLICY: ApiSystemReadModelPolicy = ApiSystemReadModelPolicy {
    sampler_thread_stack_bytes: BoundedUsizePolicy { env_var: "HELIOS_API_SAMPLER_THREAD_STACK_BYTES", default: 512 * 1024, min: 128 * 1024, max: 4 * 1024 * 1024 },
    device_metrics_cache_ms: BoundedU64Policy { env_var: "HELIOS_DEVICE_METRICS_CACHE_MS", default: 750, min: 0, max: 10_000 },
    device_updates_stream_poll_ms: BoundedU64Policy { env_var: "HELIOS_DEVICE_UPDATES_STREAM_POLL_MS", default: 2_000, min: 250, max: 60_000 },
    process_breakdown_cache_ms: BoundedU64Policy { env_var: "HELIOS_DEVICE_PROCESS_BREAKDOWN_CACHE_MS", default: 5_000, min: 0, max: 60_000 },
    device_metrics_timeout_ms: BoundedU64Policy { env_var: "HELIOS_DEVICE_METRICS_TIMEOUT_MS", default: 2_000, min: 250, max: 15_000 },
    process_breakdown_limit: BoundedUsizePolicy { env_var: "HELIOS_DEVICE_PROCESS_BREAKDOWN_LIMIT", default: 16, min: 1, max: 128 },
    process_breakdown_budget_ms: BoundedU64Policy { env_var: "HELIOS_DEVICE_PROCESS_BREAKDOWN_BUDGET_MS", default: 400, min: 0, max: 5_000 },
    process_mapping_limit: BoundedUsizePolicy { env_var: "HELIOS_DEVICE_PROCESS_MAPPING_LIMIT", default: 8, min: 1, max: 64 },
    telemetry_sample_interval_ms: BoundedU64Policy { env_var: "HELIOS_TELEMETRY_SAMPLE_INTERVAL_MS", default: 1_000, min: 250, max: 10_000 },
    processes_sample_interval_ms: BoundedU64Policy { env_var: "HELIOS_PROCESSES_SAMPLE_INTERVAL_MS", default: 1_000, min: 250, max: 10_000 },
};

pub const HELIOS_PERIPHERALS_POWER_POLICY: PeripheralsPowerPolicy = PeripheralsPowerPolicy {
    poll_interval_ms: BoundedU64Policy { env_var: "HELIOS_POWER_POLL_INTERVAL_MS", default: 100, min: 20, max: 10_000 },
    idle_interval_ms: BoundedU64Policy { env_var: "HELIOS_POWER_IDLE_INTERVAL_MS", default: 1_000, min: 100, max: 30_000 },
};
