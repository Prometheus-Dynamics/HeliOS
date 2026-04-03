use super::*;

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
