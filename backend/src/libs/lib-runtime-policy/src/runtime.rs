use crate::{BoundedU64Policy, BoundedUsizePolicy};
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
