use crate::OptionalBoundedUsizePolicy;

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

impl StyxCaptureTunablesPolicy {
    pub fn resolve(self) -> ResolvedStyxCaptureTunablesPolicy {
        ResolvedStyxCaptureTunablesPolicy { queue_depth: self.queue_depth.resolve(), pool_min: self.pool_min.resolve(), pool_bytes: self.pool_bytes.resolve(), pool_spare: self.pool_spare.resolve() }
    }
}

pub const HELIOS_STYX_CAPTURE_TUNABLES_POLICY: StyxCaptureTunablesPolicy = StyxCaptureTunablesPolicy {
    queue_depth: OptionalBoundedUsizePolicy { env_var: "HELIOS_STYX_CAPTURE_QUEUE_DEPTH", min: 1, max: 512 },
    pool_min: OptionalBoundedUsizePolicy { env_var: "HELIOS_STYX_CAPTURE_POOL_MIN", min: 1, max: 512 },
    pool_bytes: OptionalBoundedUsizePolicy { env_var: "HELIOS_STYX_CAPTURE_POOL_BYTES", min: 1, max: usize::MAX },
    pool_spare: OptionalBoundedUsizePolicy { env_var: "HELIOS_STYX_CAPTURE_POOL_SPARE", min: 0, max: 512 },
};
