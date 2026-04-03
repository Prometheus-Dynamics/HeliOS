use crate::{BoundedU64Policy, BoundedUsizePolicy};
use std::path::{Path, PathBuf};
use std::time::Duration;

const TIMEOUT_SCALE_PPM_BASE: u64 = 1_000_000;
pub const HELIOS_ENGINE_DEFAULT_SOCKET: &str = "/run/helios/engine.sock";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineSocketPolicy {
    pub env_vars: &'static [&'static str],
    pub default: &'static str,
}

impl EngineSocketPolicy {
    pub fn resolve(self) -> PathBuf {
        for env_var in self.env_vars {
            let Ok(raw) = std::env::var(env_var) else {
                continue;
            };
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                return PathBuf::from(trimmed);
            }
        }
        PathBuf::from(self.default)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineIpcPolicy {
    pub socket: EngineSocketPolicy,
    pub event_buffer: BoundedUsizePolicy,
    pub metrics_broadcast_ms: BoundedU64Policy,
    pub calibration_solve_timeout_secs: BoundedU64Policy,
    pub localization_solve_timeout_ms: BoundedU64Policy,
    pub request_send_timeout_ms: BoundedU64Policy,
    pub command_send_timeout_ms: BoundedU64Policy,
    pub request_queue_base: BoundedUsizePolicy,
    pub request_queue_per_stream: BoundedUsizePolicy,
    pub request_queue_max: BoundedUsizePolicy,
    pub timeout_scale_per_stream_ppm: BoundedU64Policy,
    pub timeout_scale_max_ppm: BoundedU64Policy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedEngineIpcPolicy {
    pub socket: PathBuf,
    pub event_buffer: usize,
    pub metrics_broadcast_interval: Duration,
    pub calibration_solve_timeout: Duration,
    pub localization_solve_timeout: Duration,
    pub request_send_timeout: Duration,
    pub command_send_timeout: Duration,
    pub request_queue_base: usize,
    pub request_queue_per_stream: usize,
    pub request_queue_max: usize,
    pub timeout_scale_per_stream_ppm: u64,
    pub timeout_scale_max_ppm: u64,
}

impl EngineIpcPolicy {
    pub fn resolve(self) -> ResolvedEngineIpcPolicy {
        let request_queue_base = self.request_queue_base.resolve();
        ResolvedEngineIpcPolicy {
            socket: self.socket.resolve(),
            event_buffer: self.event_buffer.resolve(),
            metrics_broadcast_interval: Duration::from_millis(self.metrics_broadcast_ms.resolve()),
            calibration_solve_timeout: Duration::from_secs(self.calibration_solve_timeout_secs.resolve()),
            localization_solve_timeout: Duration::from_millis(self.localization_solve_timeout_ms.resolve()),
            request_send_timeout: Duration::from_millis(self.request_send_timeout_ms.resolve()),
            command_send_timeout: Duration::from_millis(self.command_send_timeout_ms.resolve()),
            request_queue_per_stream: self.request_queue_per_stream.resolve(),
            request_queue_max: self.request_queue_max.resolve().max(request_queue_base),
            request_queue_base,
            timeout_scale_per_stream_ppm: self.timeout_scale_per_stream_ppm.resolve(),
            timeout_scale_max_ppm: self.timeout_scale_max_ppm.resolve().max(TIMEOUT_SCALE_PPM_BASE),
        }
    }
}

impl ResolvedEngineIpcPolicy {
    pub fn client_socket_candidates(&self, manifest_dir: &Path) -> Vec<PathBuf> {
        let mut paths = vec![self.socket.clone(), workspace_dev_socket(manifest_dir), PathBuf::from(HELIOS_ENGINE_DEFAULT_SOCKET)];
        paths.dedup();
        paths
    }

    pub fn request_queue_size(&self, stream_count: usize) -> usize {
        let extra = stream_count.saturating_mul(self.request_queue_per_stream);
        self.request_queue_base.saturating_add(extra).min(self.request_queue_max)
    }

    pub fn timeout_scale_for_streams(&self, stream_count: usize) -> u64 {
        let scale = TIMEOUT_SCALE_PPM_BASE.saturating_add(self.timeout_scale_per_stream_ppm.saturating_mul(stream_count as u64));
        scale.min(self.timeout_scale_max_ppm)
    }
}

pub fn scale_timeout(base: Duration, scale_ppm: u64) -> Duration {
    if base.is_zero() || scale_ppm == TIMEOUT_SCALE_PPM_BASE {
        return base;
    }
    let base_ms = base.as_millis().min(u128::from(u64::MAX)) as u64;
    let scaled = base_ms.saturating_mul(scale_ppm).saturating_div(TIMEOUT_SCALE_PPM_BASE);
    Duration::from_millis(scaled.max(1))
}

fn workspace_dev_socket(manifest_dir: &Path) -> PathBuf {
    manifest_dir.join("../..").join("target/dev/run/engine.sock")
}

pub const HELIOS_ENGINE_RESPONSE_TIMEOUT: Duration = Duration::from_secs(10);
pub const HELIOS_ENGINE_PIPELINE_LAYOUT_TIMEOUT: Duration = Duration::from_secs(30);
pub const HELIOS_ENGINE_START_STREAM_TIMEOUT: Duration = Duration::from_secs(60);
pub const HELIOS_ENGINE_STOP_STREAM_TIMEOUT: Duration = Duration::from_secs(30);
pub const HELIOS_ENGINE_STOP_RECORDING_TIMEOUT: Duration = Duration::from_secs(180);
pub const HELIOS_ENGINE_CAPTURE_SHADOW_TIMEOUT: Duration = Duration::from_secs(300);
pub const HELIOS_ENGINE_RECONNECT_INITIAL: Duration = Duration::from_millis(200);
pub const HELIOS_ENGINE_RECONNECT_MAX: Duration = Duration::from_secs(5);

pub const HELIOS_ENGINE_IPC_POLICY: EngineIpcPolicy = EngineIpcPolicy {
    socket: EngineSocketPolicy { env_vars: &["HELIOS_ENGINE_SOCKET", "ENGINE_SOCKET"], default: HELIOS_ENGINE_DEFAULT_SOCKET },
    event_buffer: BoundedUsizePolicy { env_var: "HELIOS_ENGINE_IPC_EVENT_BUFFER", default: 4096, min: 128, max: 16_384 },
    metrics_broadcast_ms: BoundedU64Policy { env_var: "HELIOS_ENGINE_METRICS_BROADCAST_MS", default: 5_000, min: 250, max: 10_000 },
    calibration_solve_timeout_secs: BoundedU64Policy { env_var: "HELIOS_CALIBRATION_SOLVE_TIMEOUT_SECS", default: 300, min: 30, max: 1800 },
    localization_solve_timeout_ms: BoundedU64Policy { env_var: "HELIOS_ENGINE_LOCALIZATION_SOLVE_TIMEOUT_MS", default: 30_000, min: 1_000, max: 300_000 },
    request_send_timeout_ms: BoundedU64Policy { env_var: "HELIOS_ENGINE_REQUEST_SEND_TIMEOUT_MS", default: 1_000, min: 50, max: 10_000 },
    command_send_timeout_ms: BoundedU64Policy { env_var: "HELIOS_ENGINE_COMMAND_SEND_TIMEOUT_MS", default: 2_000, min: 100, max: 15_000 },
    request_queue_base: BoundedUsizePolicy { env_var: "HELIOS_ENGINE_REQUEST_QUEUE", default: 256, min: 32, max: 8_192 },
    request_queue_per_stream: BoundedUsizePolicy { env_var: "HELIOS_ENGINE_REQUEST_QUEUE_PER_STREAM", default: 8, min: 0, max: 256 },
    request_queue_max: BoundedUsizePolicy { env_var: "HELIOS_ENGINE_REQUEST_QUEUE_MAX", default: 2_048, min: 32, max: 8_192 },
    timeout_scale_per_stream_ppm: BoundedU64Policy { env_var: "HELIOS_ENGINE_TIMEOUT_PER_STREAM_PPM", default: 50_000, min: 0, max: 10_000_000 },
    timeout_scale_max_ppm: BoundedU64Policy { env_var: "HELIOS_ENGINE_TIMEOUT_SCALE_MAX_PPM", default: 2_000_000, min: TIMEOUT_SCALE_PPM_BASE, max: 10_000_000 },
};
