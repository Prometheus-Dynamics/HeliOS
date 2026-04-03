use crate::{BoundedU64Policy, BoundedUsizePolicy};

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
