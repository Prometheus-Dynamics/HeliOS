use super::*;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EngineStreamRuntimePolicy {
    pub default_libcamera_fps: BoundedU64Policy,
    pub encoded_channel_size: BoundedUsizePolicy,
    pub viewer_idle_timeout_ms: BoundedU64Policy,
    pub viewer_check_interval_ms: BoundedU64Policy,
    pub idle_compaction_interval_ms: BoundedU64Policy,
    pub software_encoder_fps_cap: OptionalBoundedF64Policy,
    pub capture_stall_ms: BoundedU64Policy,
    pub capture_active_stall_ms: BoundedU64Policy,
    pub capture_first_frame_stall_ms: BoundedU64Policy,
    pub encoded_consumer_stale_ms: BoundedU64Policy,
    pub metrics_stale_base_ms: BoundedU64Policy,
    pub usb_power_setup_script: StringPolicy,
    pub usb_power_recovery_settle_ms: BoundedU64Policy,
    pub software_encoder_threads: OptionalBoundedUsizePolicy,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedEngineStreamRuntimePolicy {
    pub default_libcamera_fps: u32,
    pub encoded_channel_size: usize,
    pub viewer_idle_timeout_ms: u64,
    pub viewer_check_interval_ms: u64,
    pub idle_compaction_interval_ms: u64,
    pub software_encoder_fps_cap: Option<f64>,
    pub capture_stall_ms: u64,
    pub capture_active_stall_ms: u64,
    pub capture_first_frame_stall_ms: u64,
    pub encoded_consumer_stale_ms: u64,
    pub metrics_stale_base_ms: u64,
    pub usb_power_setup_script: String,
    pub usb_power_recovery_settle_ms: u64,
    pub software_encoder_threads: Option<usize>,
}

impl EngineStreamRuntimePolicy {
    pub fn resolve(self) -> ResolvedEngineStreamRuntimePolicy {
        ResolvedEngineStreamRuntimePolicy {
            default_libcamera_fps: self.default_libcamera_fps.resolve().min(u64::from(u32::MAX)) as u32,
            encoded_channel_size: self.encoded_channel_size.resolve(),
            viewer_idle_timeout_ms: self.viewer_idle_timeout_ms.resolve(),
            viewer_check_interval_ms: self.viewer_check_interval_ms.resolve(),
            idle_compaction_interval_ms: self.idle_compaction_interval_ms.resolve(),
            software_encoder_fps_cap: self.software_encoder_fps_cap.resolve().or(Some(12.0)).filter(|value| value.is_finite() && *value > 0.0),
            capture_stall_ms: self.capture_stall_ms.resolve(),
            capture_active_stall_ms: self.capture_active_stall_ms.resolve(),
            capture_first_frame_stall_ms: self.capture_first_frame_stall_ms.resolve(),
            encoded_consumer_stale_ms: self.encoded_consumer_stale_ms.resolve(),
            metrics_stale_base_ms: self.metrics_stale_base_ms.resolve(),
            usb_power_setup_script: self.usb_power_setup_script.resolve(),
            usb_power_recovery_settle_ms: self.usb_power_recovery_settle_ms.resolve(),
            software_encoder_threads: self.software_encoder_threads.resolve(),
        }
    }
}

pub const HELIOS_ENGINE_STREAM_RUNTIME_POLICY: EngineStreamRuntimePolicy = EngineStreamRuntimePolicy {
    default_libcamera_fps: BoundedU64Policy { env_var: "HELIOS_DEFAULT_LIBCAMERA_FPS", default: 30, min: 1, max: u32::MAX as u64 },
    encoded_channel_size: BoundedUsizePolicy { env_var: "HELIOS_ENCODED_CHANNEL_SIZE", default: 8, min: 1, max: 1024 },
    viewer_idle_timeout_ms: BoundedU64Policy { env_var: "HELIOS_STREAM_VIEWER_IDLE_TIMEOUT_MS", default: 2_500, min: 250, max: 60_000 },
    viewer_check_interval_ms: BoundedU64Policy { env_var: "HELIOS_STREAM_VIEWER_CHECK_INTERVAL_MS", default: 250, min: 50, max: 5_000 },
    idle_compaction_interval_ms: BoundedU64Policy { env_var: "HELIOS_STREAM_IDLE_COMPACTION_INTERVAL_MS", default: 1_000, min: 50, max: 60_000 },
    software_encoder_fps_cap: OptionalBoundedF64Policy { env_var: "HELIOS_SOFTWARE_ENCODER_FPS_CAP", min: 0.0, max: 1_000.0 },
    capture_stall_ms: BoundedU64Policy { env_var: "HELIOS_CAPTURE_STALL_MS", default: 1_500, min: 50, max: 10_000 },
    capture_active_stall_ms: BoundedU64Policy { env_var: "HELIOS_CAPTURE_ACTIVE_STALL_MS", default: 1_000, min: 250, max: 10_000 },
    capture_first_frame_stall_ms: BoundedU64Policy { env_var: "HELIOS_CAPTURE_FIRST_FRAME_STALL_MS", default: 8_000, min: 1_000, max: 120_000 },
    encoded_consumer_stale_ms: BoundedU64Policy { env_var: "HELIOS_ENCODED_CONSUMER_STALE_MS", default: 5_000, min: 500, max: 60_000 },
    metrics_stale_base_ms: BoundedU64Policy { env_var: "HELIOS_STREAM_METRICS_STALE_MS", default: 1_500, min: 250, max: 60_000 },
    usb_power_setup_script: StringPolicy { env_var: "HELIOS_USB_POWER_SETUP_SCRIPT", default: "/usr/local/bin/helios-usb-power-setup.sh" },
    usb_power_recovery_settle_ms: BoundedU64Policy { env_var: "HELIOS_USB_POWER_RECOVERY_SETTLE_MS", default: 2_500, min: 100, max: 10_000 },
    software_encoder_threads: OptionalBoundedUsizePolicy { env_var: "HELIOS_SOFTWARE_ENCODER_THREADS", min: 1, max: usize::MAX },
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EngineCalibrationPolicy {
    pub disable_charuco: BoolPolicy,
    pub force_parity: StringPolicy,
    pub force_ordering: StringPolicy,
    pub overlay_jpeg_quality: BoundedU64Policy,
    pub overlay_save_mode: StringPolicy,
    pub min_tag_area_ratio: BoundedF64Policy,
    pub min_tag_area_median_ratio: BoundedF64Policy,
    pub min_tag_area_px: OptionalBoundedF64Policy,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedEngineCalibrationPolicy {
    pub disable_charuco: bool,
    pub force_parity: String,
    pub force_ordering: String,
    pub overlay_jpeg_quality: u8,
    pub overlay_save_mode: String,
    pub min_tag_area_ratio: f64,
    pub min_tag_area_median_ratio: f64,
    pub min_tag_area_px: Option<f64>,
}

impl EngineCalibrationPolicy {
    pub fn resolve(self) -> ResolvedEngineCalibrationPolicy {
        ResolvedEngineCalibrationPolicy {
            disable_charuco: self.disable_charuco.resolve(),
            force_parity: self.force_parity.resolve(),
            force_ordering: self.force_ordering.resolve(),
            overlay_jpeg_quality: self.overlay_jpeg_quality.resolve().min(u64::from(u8::MAX)) as u8,
            overlay_save_mode: self.overlay_save_mode.resolve(),
            min_tag_area_ratio: self.min_tag_area_ratio.resolve(),
            min_tag_area_median_ratio: self.min_tag_area_median_ratio.resolve(),
            min_tag_area_px: self.min_tag_area_px.resolve(),
        }
    }
}

pub const HELIOS_ENGINE_CALIBRATION_POLICY: EngineCalibrationPolicy = EngineCalibrationPolicy {
    disable_charuco: BoolPolicy { env_var: "HELIOS_CALIBRATION_DISABLE_CHARUCO", default: false },
    force_parity: StringPolicy { env_var: "HELIOS_CALIBRATION_FORCE_PARITY", default: "" },
    force_ordering: StringPolicy { env_var: "HELIOS_CALIBRATION_FORCE_ORDERING", default: "" },
    overlay_jpeg_quality: BoundedU64Policy { env_var: "HELIOS_CALIBRATION_OVERLAY_JPEG_QUALITY", default: 85, min: 1, max: 100 },
    overlay_save_mode: StringPolicy { env_var: "HELIOS_CALIBRATION_OVERLAY_SAVE_MODE", default: "graph" },
    min_tag_area_ratio: BoundedF64Policy { env_var: "HELIOS_CALIBRATION_MIN_TAG_AREA_RATIO", default: 0.0, min: 0.0, max: 0.1 },
    min_tag_area_median_ratio: BoundedF64Policy { env_var: "HELIOS_CALIBRATION_MIN_TAG_AREA_MEDIAN_RATIO", default: 0.0, min: 0.0, max: 3.0 },
    min_tag_area_px: OptionalBoundedF64Policy { env_var: "HELIOS_CALIBRATION_MIN_TAG_AREA_PX", min: 0.0, max: f64::MAX },
};

pub fn resolve_pipeline_template_dir() -> io::Result<PathBuf> {
    if let Ok(raw) = std::env::var("HELIOS_PIPELINE_TEMPLATE_DIR") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        let dev = cwd.join("configs").join("templates");
        if dev.is_dir() {
            return Ok(dev);
        }
    }
    Ok(PathBuf::from("/usr/share/helios/pipeline-templates"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineDaedalusEnvOverridePolicy {
    pub metrics_level: OptionalStringPolicy,
    pub force_cpu: OptionalStringPolicy,
    pub gpu_backend: OptionalStringPolicy,
    pub planner_enable_gpu: OptionalStringPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedEngineDaedalusEnvOverridePolicy {
    pub metrics_level: Option<String>,
    pub force_cpu: Option<String>,
    pub gpu_backend: Option<String>,
    pub planner_enable_gpu: Option<String>,
}

impl EngineDaedalusEnvOverridePolicy {
    pub fn resolve(self) -> ResolvedEngineDaedalusEnvOverridePolicy {
        ResolvedEngineDaedalusEnvOverridePolicy {
            metrics_level: self.metrics_level.resolve(),
            force_cpu: self.force_cpu.resolve(),
            gpu_backend: self.gpu_backend.resolve(),
            planner_enable_gpu: self.planner_enable_gpu.resolve(),
        }
    }
}

pub const HELIOS_ENGINE_DAEDALUS_ENV_OVERRIDE_POLICY: EngineDaedalusEnvOverridePolicy = EngineDaedalusEnvOverridePolicy {
    metrics_level: OptionalStringPolicy { env_var: "DAEDALUS_METRICS_LEVEL" },
    force_cpu: OptionalStringPolicy { env_var: "HELIOS_DAEDALUS_FORCE_CPU" },
    gpu_backend: OptionalStringPolicy { env_var: "HELIOS_DAEDALUS_GPU_BACKEND" },
    planner_enable_gpu: OptionalStringPolicy { env_var: "HELIOS_DAEDALUS_PLANNER_ENABLE_GPU" },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineRuntimeFlagsPolicy {
    pub cache_node_registry_snapshot: BoolPolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedEngineRuntimeFlagsPolicy {
    pub cache_node_registry_snapshot: bool,
}

impl EngineRuntimeFlagsPolicy {
    pub fn resolve(self) -> ResolvedEngineRuntimeFlagsPolicy {
        ResolvedEngineRuntimeFlagsPolicy { cache_node_registry_snapshot: self.cache_node_registry_snapshot.resolve() }
    }
}

pub const HELIOS_ENGINE_RUNTIME_FLAGS_POLICY: EngineRuntimeFlagsPolicy =
    EngineRuntimeFlagsPolicy { cache_node_registry_snapshot: BoolPolicy { env_var: "HELIOS_ENGINE_CACHE_NODE_REGISTRY", default: false } };

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineUsbPowerRecoveryPolicy {
    pub enabled: BoolPolicy,
    pub trigger_attempts: BoundedU64Policy,
    pub cooldown_ms: BoundedU64Policy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedEngineUsbPowerRecoveryPolicy {
    pub enabled: bool,
    pub trigger_attempts: u32,
    pub cooldown: Duration,
}

impl EngineUsbPowerRecoveryPolicy {
    pub fn resolve(self) -> ResolvedEngineUsbPowerRecoveryPolicy {
        ResolvedEngineUsbPowerRecoveryPolicy {
            enabled: self.enabled.resolve(),
            trigger_attempts: self.trigger_attempts.resolve().min(u64::from(u32::MAX)) as u32,
            cooldown: Duration::from_millis(self.cooldown_ms.resolve()),
        }
    }
}

pub const HELIOS_ENGINE_USB_POWER_RECOVERY_POLICY: EngineUsbPowerRecoveryPolicy = EngineUsbPowerRecoveryPolicy {
    enabled: BoolPolicy { env_var: "HELIOS_USB_POWER_RECOVERY_ENABLED", default: false },
    trigger_attempts: BoundedU64Policy { env_var: "HELIOS_USB_POWER_RECOVERY_TRIGGER_ATTEMPTS", default: 4, min: 1, max: 20 },
    cooldown_ms: BoundedU64Policy { env_var: "HELIOS_USB_POWER_RECOVERY_COOLDOWN_MS", default: 30_000, min: 1_000, max: 300_000 },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineShmemPolicy {
    pub dir: OptionalStringPolicy,
    pub capacity_bytes: BoundedUsizePolicy,
    pub read_timeout_ms: BoundedU64Policy,
    pub read_retry_ms: BoundedU64Policy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedEngineShmemPolicy {
    pub dir: Option<PathBuf>,
    pub capacity_bytes: usize,
    pub read_timeout: Duration,
    pub read_retry: Duration,
}

impl EngineShmemPolicy {
    pub fn resolve(self) -> ResolvedEngineShmemPolicy {
        ResolvedEngineShmemPolicy {
            dir: self.dir.resolve().map(PathBuf::from),
            capacity_bytes: self.capacity_bytes.resolve(),
            read_timeout: Duration::from_millis(self.read_timeout_ms.resolve()),
            read_retry: Duration::from_millis(self.read_retry_ms.resolve()),
        }
    }
}

pub const HELIOS_ENGINE_SHMEM_POLICY: EngineShmemPolicy = EngineShmemPolicy {
    dir: OptionalStringPolicy { env_var: "HELIOS_SHMEM_DIR" },
    capacity_bytes: BoundedUsizePolicy { env_var: "HELIOS_SHMEM_CAPACITY_BYTES", default: 8 * 1024 * 1024, min: 1, max: usize::MAX },
    read_timeout_ms: BoundedU64Policy { env_var: "HELIOS_SHMEM_READ_TIMEOUT_MS", default: 1_000, min: 1, max: u64::MAX },
    read_retry_ms: BoundedU64Policy { env_var: "HELIOS_SHMEM_READ_RETRY_MS", default: 25, min: 1, max: u64::MAX },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineFlamegraphPolicy {
    pub frequency: OptionalBoundedU64Policy,
    pub path: OptionalStringPolicy,
    pub dir: OptionalStringPolicy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedEngineFlamegraphPolicy {
    pub frequency: Option<i32>,
    pub path: Option<PathBuf>,
    pub dir: Option<PathBuf>,
}

impl EngineFlamegraphPolicy {
    pub fn resolve(self) -> ResolvedEngineFlamegraphPolicy {
        ResolvedEngineFlamegraphPolicy {
            frequency: self.frequency.resolve().and_then(|value| i32::try_from(value).ok()),
            path: self.path.resolve().map(PathBuf::from),
            dir: self.dir.resolve().map(PathBuf::from),
        }
    }
}

pub const HELIOS_ENGINE_FLAMEGRAPH_POLICY: EngineFlamegraphPolicy = EngineFlamegraphPolicy {
    frequency: OptionalBoundedU64Policy { env_var: "HELIOS_PPROF_FREQ", min: 1, max: i32::MAX as u64 },
    path: OptionalStringPolicy { env_var: "HELIOS_PPROF_PATH" },
    dir: OptionalStringPolicy { env_var: "HELIOS_PPROF_DIR" },
};
