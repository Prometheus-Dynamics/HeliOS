use daedalus::data::model::Value as DaedalusValue;
use daedalus::engine::{EngineConfig, GpuBackend, RuntimeMode};
use daedalus::planner::Graph;
use daedalus::runtime::{BackpressureStrategy, EdgePolicyKind, MetricsLevel};
use tracing::warn;

const PREFIX: &str = "helios.daedalus.";

const KEY_GPU_BACKEND: &str = "helios.daedalus.gpu_backend";

const KEY_PLANNER_ENABLE_GPU: &str = "helios.daedalus.planner.enable_gpu";
const KEY_PLANNER_ENABLE_LINTS: &str = "helios.daedalus.planner.enable_lints";
const KEY_PLANNER_ACTIVE_FEATURES: &str = "helios.daedalus.planner.active_features";

const KEY_RUNTIME_MODE: &str = "helios.daedalus.runtime.mode";
const KEY_RUNTIME_POOL_SIZE: &str = "helios.daedalus.runtime.pool_size";
pub(crate) const KEY_RUNTIME_DEFAULT_POLICY: &str = "helios.daedalus.runtime.default_policy";
pub(crate) const KEY_RUNTIME_BACKPRESSURE: &str = "helios.daedalus.runtime.backpressure";
const KEY_RUNTIME_LOCKFREE_QUEUES: &str = "helios.daedalus.runtime.lockfree_queues";

const ENV_FORCE_CPU: &str = "HELIOS_DAEDALUS_FORCE_CPU";
const ENV_GPU_BACKEND: &str = "HELIOS_DAEDALUS_GPU_BACKEND";
const ENV_PLANNER_ENABLE_GPU: &str = "HELIOS_DAEDALUS_PLANNER_ENABLE_GPU";
const ENV_METRICS_LEVEL: &str = "DAEDALUS_METRICS_LEVEL";

fn parse_bool(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn parse_usize(raw: &str) -> Option<usize> {
    raw.trim().parse::<usize>().ok()
}

fn split_csv(raw: &str) -> Vec<String> {
    raw.split(',').map(|item| item.trim()).filter(|item| !item.is_empty()).map(|item| item.to_string()).collect()
}

fn parse_gpu_backend(raw: &str) -> Option<GpuBackend> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "cpu" => Some(GpuBackend::Cpu),
        "mock" | "gpu-mock" => Some(GpuBackend::Mock),
        "gpu" | "device" => Some(GpuBackend::Device),
        _ => None,
    }
}

fn parse_runtime_mode(raw: &str) -> Option<RuntimeMode> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "serial" => Some(RuntimeMode::Serial),
        "parallel" => Some(RuntimeMode::Parallel),
        _ => None,
    }
}

fn parse_policy(raw: &str) -> Option<EdgePolicyKind> {
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "fifo" => Some(EdgePolicyKind::Fifo),
        "newest" | "newest_wins" => Some(EdgePolicyKind::NewestWins),
        "broadcast" => Some(EdgePolicyKind::Broadcast),
        other => other.strip_prefix("bounded:").and_then(|rest| rest.parse::<usize>().ok().map(|cap| EdgePolicyKind::Bounded { cap })),
    }
}

fn parse_backpressure(raw: &str) -> Option<BackpressureStrategy> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "none" => Some(BackpressureStrategy::None),
        "bounded" | "bounded_queues" => Some(BackpressureStrategy::BoundedQueues),
        "error" | "error_on_overflow" => Some(BackpressureStrategy::ErrorOnOverflow),
        _ => None,
    }
}

fn parse_metrics_level(raw: &str) -> Option<MetricsLevel> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "off" => Some(MetricsLevel::Off),
        "basic" => Some(MetricsLevel::Basic),
        "detailed" => Some(MetricsLevel::Detailed),
        "profile" => Some(MetricsLevel::Profile),
        _ => None,
    }
}

/// Apply Daedalus engine config overrides embedded in graph metadata.
///
/// This keeps graph documents self-contained: the UI can persist engine options per graph
/// without requiring out-of-band storage.
pub fn apply_daedalus_engine_config_overrides(cfg: &mut EngineConfig, graph: &Graph) {
    for (key, value) in &graph.metadata {
        if !key.starts_with(PREFIX) {
            continue;
        }
        let raw = match value {
            DaedalusValue::String(raw) => raw.as_ref(),
            _ => {
                warn!(key, "invalid daedalus override (expected string)");
                continue;
            }
        };
        match key.as_str() {
            KEY_GPU_BACKEND => {
                if let Some(backend) = parse_gpu_backend(raw) {
                    cfg.gpu = backend;
                } else {
                    warn!(key, raw, "invalid daedalus gpu backend override");
                }
            }
            KEY_PLANNER_ENABLE_GPU => {
                if let Some(flag) = parse_bool(raw) {
                    cfg.planner.enable_gpu = flag;
                } else {
                    warn!(key, raw, "invalid boolean override");
                }
            }
            KEY_PLANNER_ENABLE_LINTS => {
                if let Some(flag) = parse_bool(raw) {
                    cfg.planner.enable_lints = flag;
                } else {
                    warn!(key, raw, "invalid boolean override");
                }
            }
            KEY_PLANNER_ACTIVE_FEATURES => {
                cfg.planner.active_features = split_csv(raw);
            }
            KEY_RUNTIME_MODE => {
                if let Some(mode) = parse_runtime_mode(raw) {
                    cfg.runtime.mode = mode;
                } else {
                    warn!(key, raw, "invalid daedalus runtime mode override");
                }
            }
            KEY_RUNTIME_POOL_SIZE => {
                if raw.trim().is_empty() {
                    cfg.runtime.pool_size = None;
                } else if let Some(pool_size) = parse_usize(raw).filter(|v| *v > 0) {
                    cfg.runtime.pool_size = Some(pool_size);
                } else {
                    warn!(key, raw, "invalid daedalus runtime pool size override");
                }
            }
            KEY_RUNTIME_DEFAULT_POLICY => {
                if let Some(policy) = parse_policy(raw) {
                    cfg.runtime.default_policy = policy;
                } else {
                    warn!(key, raw, "invalid daedalus runtime policy override");
                }
            }
            KEY_RUNTIME_BACKPRESSURE => {
                if let Some(policy) = parse_backpressure(raw) {
                    cfg.runtime.backpressure = policy;
                } else {
                    warn!(key, raw, "invalid daedalus backpressure override");
                }
            }
            KEY_RUNTIME_LOCKFREE_QUEUES => {
                if let Some(flag) = parse_bool(raw) {
                    cfg.runtime.lockfree_queues = flag;
                } else {
                    warn!(key, raw, "invalid boolean override");
                }
            }
            _ => {
                // Historical keys removed upstream (bundles + runtime feature toggles).
                if key.starts_with("helios.daedalus.features.") || key == "helios.daedalus.bundles" {
                    warn!(key, raw, "daedalus override key is no longer supported");
                } else {
                    warn!(key, raw, "unknown daedalus config override key");
                }
            }
        }
    }
}

/// Apply environment-level Daedalus engine overrides.
///
/// These are applied after graph metadata so operators can enforce a runtime policy
/// during incident mitigation (for example, force CPU backend globally).
pub fn apply_daedalus_engine_env_overrides(cfg: &mut EngineConfig) {
    if let Ok(raw) = std::env::var(ENV_METRICS_LEVEL) {
        if let Some(level) = parse_metrics_level(&raw) {
            cfg.runtime.metrics_level = level;
        } else {
            warn!(env = ENV_METRICS_LEVEL, raw, "invalid daedalus metrics level override");
        }
    }

    if let Ok(raw) = std::env::var(ENV_FORCE_CPU) {
        match parse_bool(&raw) {
            Some(true) => {
                cfg.gpu = GpuBackend::Cpu;
                cfg.planner.enable_gpu = false;
            }
            Some(false) => {}
            None => warn!(env = ENV_FORCE_CPU, raw, "invalid boolean override"),
        }
    }

    if let Ok(raw) = std::env::var(ENV_GPU_BACKEND) {
        if let Some(backend) = parse_gpu_backend(&raw) {
            cfg.gpu = backend;
        } else {
            warn!(env = ENV_GPU_BACKEND, raw, "invalid daedalus gpu backend override");
        }
    }

    if let Ok(raw) = std::env::var(ENV_PLANNER_ENABLE_GPU) {
        if let Some(flag) = parse_bool(&raw) {
            cfg.planner.enable_gpu = flag;
        } else {
            warn!(env = ENV_PLANNER_ENABLE_GPU, raw, "invalid boolean override");
        }
    }

    if let Ok(raw) = std::env::var(ENV_FORCE_CPU) {
        if parse_bool(&raw) == Some(true) {
            // Force-cpu stays authoritative even if other env vars request GPU.
            cfg.gpu = GpuBackend::Cpu;
            cfg.planner.enable_gpu = false;
        }
    }
}
