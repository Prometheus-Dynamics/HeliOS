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
        std::env::var(self.env_var)
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(self.default)
            .clamp(self.min, self.max)
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
        std::env::var(self.env_var)
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(self.default)
            .clamp(self.min, self.max)
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

mod generated;

pub use generated::*;

#[cfg(test)]
mod tests {
    use super::{HELIOS_API_TOKIO_POLICY, HELIOS_ENGINE_TOKIO_POLICY, HELIOS_PERIPHERALS_TOKIO_POLICY};

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
}
