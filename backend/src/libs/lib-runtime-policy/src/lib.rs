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
pub struct OptionalBoundedU64Policy {
    pub env_var: &'static str,
    pub min: u64,
    pub max: u64,
}

impl OptionalBoundedU64Policy {
    pub fn resolve(self) -> Option<u64> {
        std::env::var(self.env_var).ok().and_then(|value| value.trim().parse::<u64>().ok()).map(|value| value.clamp(self.min, self.max))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OptionalBoundedF64Policy {
    pub env_var: &'static str,
    pub min: f64,
    pub max: f64,
}

impl OptionalBoundedF64Policy {
    pub fn resolve(self) -> Option<f64> {
        std::env::var(self.env_var).ok().and_then(|value| value.trim().parse::<f64>().ok()).filter(|value| value.is_finite()).map(|value| value.clamp(self.min, self.max))
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineExecutorBusyPolicy {
    Drop,
    Block,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineGraphPolicy {
    pub pool_size: OptionalBoundedUsizePolicy,
    pub runtime_queue_cap: BoundedUsizePolicy,
    pub dedicated_executor: BoolPolicy,
    pub executor_busy: StringPolicy,
    pub executor_busy_timeout_ms: OptionalBoundedU64Policy,
    pub auto_target_roi: BoolPolicy,
    pub host_outputs_in_graph: BoolPolicy,
    pub demand_driven: BoolPolicy,
    pub host_output_debug: BoolPolicy,
    pub perf_counters: BoolPolicy,
    pub pprof_enabled: BoolPolicy,
    pub background_trim_interval_ms: BoundedU64Policy,
    pub active_trim_interval_ms: BoundedU64Policy,
    pub host_output_sample_ttl_ms: BoundedU64Policy,
    pub pprof_frames: BoundedU64Policy,
    pub pprof_duration_ms: OptionalBoundedU64Policy,
    pub pprof_duration_secs: OptionalBoundedU64Policy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedEngineGraphPolicy {
    pub pool_size: Option<usize>,
    pub runtime_queue_cap: usize,
    pub dedicated_executor: bool,
    pub executor_busy: EngineExecutorBusyPolicy,
    pub executor_busy_timeout_ms: Option<u64>,
    pub auto_target_roi: bool,
    pub host_outputs_in_graph: bool,
    pub demand_driven: bool,
    pub host_output_debug: bool,
    pub perf_counters: bool,
    pub pprof_enabled: bool,
    pub background_trim_interval_ms: u64,
    pub active_trim_interval_ms: u64,
    pub host_output_sample_ttl_ms: u64,
    pub pprof_frames: u64,
    pub pprof_duration_ms: Option<u64>,
}

impl EngineGraphPolicy {
    pub fn resolve(self) -> ResolvedEngineGraphPolicy {
        let executor_busy = match self.executor_busy.resolve().trim().to_ascii_lowercase().as_str() {
            "block" | "1" | "true" => EngineExecutorBusyPolicy::Block,
            _ => EngineExecutorBusyPolicy::Drop,
        };
        let pprof_duration_ms = self.pprof_duration_ms.resolve().or_else(|| self.pprof_duration_secs.resolve().map(|secs| secs.saturating_mul(1000)));
        ResolvedEngineGraphPolicy {
            pool_size: self.pool_size.resolve(),
            runtime_queue_cap: self.runtime_queue_cap.resolve(),
            dedicated_executor: self.dedicated_executor.resolve(),
            executor_busy,
            executor_busy_timeout_ms: self.executor_busy_timeout_ms.resolve(),
            auto_target_roi: self.auto_target_roi.resolve(),
            host_outputs_in_graph: self.host_outputs_in_graph.resolve(),
            demand_driven: self.demand_driven.resolve(),
            host_output_debug: self.host_output_debug.resolve(),
            perf_counters: self.perf_counters.resolve(),
            pprof_enabled: self.pprof_enabled.resolve(),
            background_trim_interval_ms: self.background_trim_interval_ms.resolve(),
            active_trim_interval_ms: self.active_trim_interval_ms.resolve(),
            host_output_sample_ttl_ms: self.host_output_sample_ttl_ms.resolve(),
            pprof_frames: self.pprof_frames.resolve(),
            pprof_duration_ms,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineRecordingPolicy {
    pub stream_command_queue_size: BoundedUsizePolicy,
    pub recording_frame_queue_size: BoundedUsizePolicy,
    pub stream_worker_stack_bytes: BoundedUsizePolicy,
    pub recording_worker_stack_bytes: BoundedUsizePolicy,
    pub recording_stop_grace_ms: BoundedU64Policy,
    pub shadow_window_ms: BoundedU64Policy,
    pub shadow_segment_ms: BoundedU64Policy,
    pub shadow_flush_interval_ms: BoundedU64Policy,
    pub shadow_writer_buffer_bytes: BoundedUsizePolicy,
    pub shadow_config_scan_interval_ms: BoundedU64Policy,
    pub keep_raw_on_record_fail: BoolPolicy,
    pub shadow_recorder_enabled: BoolPolicy,
    pub recording_encoded_passthrough: BoolPolicy,
    pub recording_shadow_start_stop: BoolPolicy,
    pub rewrite_encoded_frame_timestamps_to_wall: BoolPolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedEngineRecordingPolicy {
    pub stream_command_queue_size: usize,
    pub recording_frame_queue_size: usize,
    pub stream_worker_stack_bytes: usize,
    pub recording_worker_stack_bytes: usize,
    pub recording_stop_grace_ms: u64,
    pub shadow_window_ms: u64,
    pub shadow_segment_ms: u64,
    pub shadow_flush_interval_ms: u64,
    pub shadow_writer_buffer_bytes: usize,
    pub shadow_config_scan_interval_ms: u64,
    pub keep_raw_on_record_fail: bool,
    pub shadow_recorder_enabled: bool,
    pub recording_encoded_passthrough: bool,
    pub recording_shadow_start_stop: bool,
    pub rewrite_encoded_frame_timestamps_to_wall: bool,
}

impl EngineRecordingPolicy {
    pub fn resolve(self) -> ResolvedEngineRecordingPolicy {
        let shadow_window_ms = self.shadow_window_ms.resolve();
        let shadow_segment_ms = self.shadow_segment_ms.resolve().min(shadow_window_ms);
        ResolvedEngineRecordingPolicy {
            stream_command_queue_size: self.stream_command_queue_size.resolve(),
            recording_frame_queue_size: self.recording_frame_queue_size.resolve(),
            stream_worker_stack_bytes: self.stream_worker_stack_bytes.resolve(),
            recording_worker_stack_bytes: self.recording_worker_stack_bytes.resolve(),
            recording_stop_grace_ms: self.recording_stop_grace_ms.resolve(),
            shadow_window_ms,
            shadow_segment_ms,
            shadow_flush_interval_ms: self.shadow_flush_interval_ms.resolve(),
            shadow_writer_buffer_bytes: self.shadow_writer_buffer_bytes.resolve(),
            shadow_config_scan_interval_ms: self.shadow_config_scan_interval_ms.resolve(),
            keep_raw_on_record_fail: self.keep_raw_on_record_fail.resolve(),
            shadow_recorder_enabled: self.shadow_recorder_enabled.resolve(),
            recording_encoded_passthrough: self.recording_encoded_passthrough.resolve(),
            recording_shadow_start_stop: self.recording_shadow_start_stop.resolve(),
            rewrite_encoded_frame_timestamps_to_wall: self.rewrite_encoded_frame_timestamps_to_wall.resolve(),
        }
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

pub const HELIOS_STYX_CAPTURE_TUNABLES_POLICY: StyxCaptureTunablesPolicy = StyxCaptureTunablesPolicy {
    queue_depth: OptionalBoundedUsizePolicy { env_var: "HELIOS_STYX_CAPTURE_QUEUE_DEPTH", min: 1, max: 512 },
    pool_min: OptionalBoundedUsizePolicy { env_var: "HELIOS_STYX_CAPTURE_POOL_MIN", min: 1, max: 512 },
    pool_bytes: OptionalBoundedUsizePolicy { env_var: "HELIOS_STYX_CAPTURE_POOL_BYTES", min: 1, max: usize::MAX },
    pool_spare: OptionalBoundedUsizePolicy { env_var: "HELIOS_STYX_CAPTURE_POOL_SPARE", min: 0, max: 512 },
};

pub const HELIOS_ENGINE_GRAPH_POLICY: EngineGraphPolicy = EngineGraphPolicy {
    pool_size: OptionalBoundedUsizePolicy { env_var: "HELIOS_DAEDALUS_POOL_SIZE", min: 1, max: usize::MAX },
    runtime_queue_cap: BoundedUsizePolicy { env_var: "HELIOS_DAEDALUS_RUNTIME_QUEUE_CAP", default: 4, min: 1, max: 1024 },
    dedicated_executor: BoolPolicy { env_var: "HELIOS_DAEDALUS_DEDICATED_EXECUTOR", default: false },
    executor_busy: StringPolicy { env_var: "HELIOS_DAEDALUS_EXECUTOR_BUSY", default: "drop" },
    executor_busy_timeout_ms: OptionalBoundedU64Policy { env_var: "HELIOS_DAEDALUS_EXECUTOR_BUSY_TIMEOUT_MS", min: 1, max: u64::MAX },
    auto_target_roi: BoolPolicy { env_var: "HELIOS_DAEDALUS_AUTO_TARGET_ROI", default: true },
    host_outputs_in_graph: BoolPolicy { env_var: "HELIOS_DAEDALUS_HOST_OUTPUTS_IN_GRAPH", default: false },
    demand_driven: BoolPolicy { env_var: "HELIOS_DAEDALUS_DEMAND_DRIVEN", default: false },
    host_output_debug: BoolPolicy { env_var: "HELIOS_HOST_OUTPUT_DEBUG", default: false },
    perf_counters: BoolPolicy { env_var: "HELIOS_PERF_COUNTERS", default: false },
    pprof_enabled: BoolPolicy { env_var: "HELIOS_PPROF", default: false },
    background_trim_interval_ms: BoundedU64Policy { env_var: "HELIOS_GRAPH_BACKGROUND_TRIM_INTERVAL_MS", default: 5_000, min: 0, max: u64::MAX },
    active_trim_interval_ms: BoundedU64Policy { env_var: "HELIOS_GRAPH_ACTIVE_TRIM_INTERVAL_MS", default: 0, min: 0, max: u64::MAX },
    host_output_sample_ttl_ms: BoundedU64Policy { env_var: "HELIOS_HOST_OUTPUT_SAMPLE_TTL_MS", default: 500, min: 50, max: 5_000 },
    pprof_frames: BoundedU64Policy { env_var: "HELIOS_PPROF_FRAMES", default: 1, min: 1, max: u64::MAX },
    pprof_duration_ms: OptionalBoundedU64Policy { env_var: "HELIOS_PPROF_DURATION_MS", min: 1, max: u64::MAX },
    pprof_duration_secs: OptionalBoundedU64Policy { env_var: "HELIOS_PPROF_DURATION_SECS", min: 1, max: u64::MAX },
};

pub const HELIOS_ENGINE_RECORDING_POLICY: EngineRecordingPolicy = EngineRecordingPolicy {
    stream_command_queue_size: BoundedUsizePolicy { env_var: "HELIOS_STREAM_COMMAND_QUEUE_SIZE", default: 64, min: 8, max: 512 },
    recording_frame_queue_size: BoundedUsizePolicy { env_var: "HELIOS_RECORDING_FRAME_QUEUE_SIZE", default: 48, min: 1, max: 256 },
    stream_worker_stack_bytes: BoundedUsizePolicy { env_var: "HELIOS_ENGINE_STREAM_THREAD_STACK_BYTES", default: 2 * 1024 * 1024, min: 256 * 1024, max: 8 * 1024 * 1024 },
    recording_worker_stack_bytes: BoundedUsizePolicy { env_var: "HELIOS_ENGINE_RECORDING_THREAD_STACK_BYTES", default: 1024 * 1024, min: 256 * 1024, max: 8 * 1024 * 1024 },
    recording_stop_grace_ms: BoundedU64Policy { env_var: "HELIOS_RECORDING_STOP_GRACE_MS", default: 0, min: 0, max: 2_000 },
    shadow_window_ms: BoundedU64Policy { env_var: "HELIOS_SHADOW_WINDOW_MS", default: 120_000, min: 5_000, max: 600_000 },
    shadow_segment_ms: BoundedU64Policy { env_var: "HELIOS_SHADOW_SEGMENT_MS", default: 2_000, min: 250, max: 10_000 },
    shadow_flush_interval_ms: BoundedU64Policy { env_var: "HELIOS_SHADOW_FLUSH_MS", default: 1_000, min: 100, max: 5_000 },
    shadow_writer_buffer_bytes: BoundedUsizePolicy { env_var: "HELIOS_SHADOW_WRITER_BYTES", default: 1 << 20, min: 64 << 10, max: 8 << 20 },
    shadow_config_scan_interval_ms: BoundedU64Policy { env_var: "HELIOS_SHADOW_CONFIG_SCAN_MS", default: 1_000, min: 100, max: 10_000 },
    keep_raw_on_record_fail: BoolPolicy { env_var: "HELIOS_KEEP_RAW_ON_RECORD_FAIL", default: false },
    shadow_recorder_enabled: BoolPolicy { env_var: "HELIOS_ENABLE_SHADOW_RECORDER", default: true },
    recording_encoded_passthrough: BoolPolicy { env_var: "HELIOS_RECORDING_USE_ENCODED_PASSTHROUGH", default: false },
    recording_shadow_start_stop: BoolPolicy { env_var: "HELIOS_RECORDING_USE_SHADOW_START_STOP", default: false },
    rewrite_encoded_frame_timestamps_to_wall: BoolPolicy { env_var: "HELIOS_RECORDING_REWRITE_FRAME_TS_TO_WALL", default: false },
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
        EngineExecutorBusyPolicy, HELIOS_API_HARDWARE_READ_MODEL_POLICY, HELIOS_API_LOG_SOURCES_POLICY, HELIOS_API_STARTUP_CACHE_WARM_POLICY, HELIOS_API_STREAMS_POLICY,
        HELIOS_API_SYSTEM_READ_MODEL_POLICY, HELIOS_API_TOKIO_POLICY, HELIOS_ENGINE_CRASH_GUARD_POLICY, HELIOS_ENGINE_GRAPH_POLICY, HELIOS_ENGINE_RECORDING_POLICY, HELIOS_ENGINE_TOKIO_POLICY,
        HELIOS_I2C_INVENTORY_POLICY, HELIOS_IMU_RUNTIME_POLICY, HELIOS_LOG_FILTER_POLICY, HELIOS_PERIPHERALS_POWER_POLICY, HELIOS_PERIPHERALS_TOKIO_POLICY, HELIOS_RESOURCE_GUARD_POLICY,
        HELIOS_STYX_CAPTURE_TUNABLES_POLICY, PlatformFamily, classify_platform_family,
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
    fn engine_crash_guard_policy_defaults_match_expected_values() {
        let resolved = HELIOS_ENGINE_CRASH_GUARD_POLICY.resolve();
        assert_eq!(resolved.window_ms, 60_000);
        assert_eq!(resolved.threshold, 3);
        assert_eq!(resolved.suppress_ms, 300_000);
        assert_eq!(resolved.min_downtime_ms, 2_000);
        assert_eq!(resolved.poll_ms, 1_000);
    }

    #[test]
    fn api_streams_policy_defaults_match_expected_values() {
        let resolved = HELIOS_API_STREAMS_POLICY.resolve();
        assert_eq!(resolved.cache_ms, 750);
        assert_eq!(resolved.mjpeg_poll_ms, None);
        assert_eq!(resolved.snapshot_interval_ms, 33);
        assert_eq!(resolved.preview_max_fps, None);
        assert_eq!(resolved.preview_outage_ms, 15_000);
    }

    #[test]
    fn api_hardware_read_model_policy_defaults_match_expected_values() {
        let resolved = HELIOS_API_HARDWARE_READ_MODEL_POLICY.resolve();
        assert_eq!(resolved.peripherals_cache_ms, 1_000);
        assert_eq!(resolved.peripherals_timeout_ms, 1_500);
        assert_eq!(resolved.camera_discovery_timeout_ms, 1_500);
        assert_eq!(resolved.peripherals_refresh_timeout_ms, 2_500);
    }

    #[test]
    fn api_system_read_model_policy_defaults_match_expected_values() {
        let resolved = HELIOS_API_SYSTEM_READ_MODEL_POLICY.resolve();
        assert_eq!(resolved.sampler_thread_stack_bytes, 512 * 1024);
        assert_eq!(resolved.device_metrics_cache_ms, 750);
        assert_eq!(resolved.process_breakdown_limit, 16);
        assert_eq!(resolved.processes_sample_interval_ms, 1_000);
    }

    #[test]
    fn peripherals_power_policy_defaults_match_expected_values() {
        let resolved = HELIOS_PERIPHERALS_POWER_POLICY.resolve();
        assert_eq!(resolved.poll_interval_ms, 100);
        assert_eq!(resolved.idle_interval_ms, 1_000);
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
    fn engine_graph_policy_defaults_match_expected_values() {
        let resolved = HELIOS_ENGINE_GRAPH_POLICY.resolve();
        assert_eq!(resolved.pool_size, None);
        assert_eq!(resolved.runtime_queue_cap, 4);
        assert!(!resolved.dedicated_executor);
        assert_eq!(resolved.executor_busy, EngineExecutorBusyPolicy::Drop);
        assert_eq!(resolved.executor_busy_timeout_ms, None);
        assert!(resolved.auto_target_roi);
        assert!(!resolved.host_outputs_in_graph);
        assert!(!resolved.demand_driven);
        assert!(!resolved.host_output_debug);
        assert!(!resolved.perf_counters);
        assert!(!resolved.pprof_enabled);
        assert_eq!(resolved.background_trim_interval_ms, 5_000);
        assert_eq!(resolved.active_trim_interval_ms, 0);
        assert_eq!(resolved.host_output_sample_ttl_ms, 500);
        assert_eq!(resolved.pprof_frames, 1);
        assert_eq!(resolved.pprof_duration_ms, None);
    }

    #[test]
    fn engine_recording_policy_defaults_match_expected_values() {
        let resolved = HELIOS_ENGINE_RECORDING_POLICY.resolve();
        assert_eq!(resolved.stream_command_queue_size, 64);
        assert_eq!(resolved.recording_frame_queue_size, 48);
        assert_eq!(resolved.stream_worker_stack_bytes, 2 * 1024 * 1024);
        assert_eq!(resolved.recording_worker_stack_bytes, 1024 * 1024);
        assert_eq!(resolved.recording_stop_grace_ms, 0);
        assert_eq!(resolved.shadow_window_ms, 120_000);
        assert_eq!(resolved.shadow_segment_ms, 2_000);
        assert_eq!(resolved.shadow_flush_interval_ms, 1_000);
        assert_eq!(resolved.shadow_writer_buffer_bytes, 1 << 20);
        assert_eq!(resolved.shadow_config_scan_interval_ms, 1_000);
        assert!(!resolved.keep_raw_on_record_fail);
        assert!(resolved.shadow_recorder_enabled);
        assert!(!resolved.recording_encoded_passthrough);
        assert!(!resolved.recording_shadow_start_stop);
        assert!(!resolved.rewrite_encoded_frame_timestamps_to_wall);
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
