use crate::{BoundedU64Policy, BoundedUsizePolicy};

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

pub const HELIOS_API_STARTUP_CACHE_WARM_POLICY: ApiStartupCacheWarmPolicy = ApiStartupCacheWarmPolicy {
    initial_delay_ms: BoundedU64Policy { env_var: "HELIOS_STARTUP_CACHE_WARM_DELAY_MS", default: 1_500, min: 0, max: 30_000 },
    retry_delay_ms: BoundedU64Policy { env_var: "HELIOS_STARTUP_CACHE_WARM_RETRY_MS", default: 1_000, min: 100, max: 30_000 },
    attempts: BoundedUsizePolicy { env_var: "HELIOS_STARTUP_CACHE_WARM_ATTEMPTS", default: 4, min: 1, max: 10 },
};
