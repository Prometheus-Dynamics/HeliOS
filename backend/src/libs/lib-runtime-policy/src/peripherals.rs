use crate::BoundedU64Policy;

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

pub const HELIOS_I2C_INVENTORY_POLICY: I2cInventoryPolicy = I2cInventoryPolicy {
    timeout_ms: BoundedU64Policy { env_var: "HELIOS_I2C_INVENTORY_TIMEOUT_MS", default: 5_000, min: 100, max: 30_000 },
    cache_ttl_ms: BoundedU64Policy { env_var: "HELIOS_I2C_INVENTORY_CACHE_TTL_MS", default: 2_000, min: 0, max: 60_000 },
};

pub const HELIOS_IMU_RUNTIME_POLICY: ImuRuntimePolicy = ImuRuntimePolicy { idle_interval_ms: BoundedU64Policy { env_var: "HELIOS_IMU_IDLE_INTERVAL_MS", default: 100, min: 20, max: 5_000 } };

pub const HELIOS_PERIPHERALS_POWER_POLICY: PeripheralsPowerPolicy = PeripheralsPowerPolicy {
    poll_interval_ms: BoundedU64Policy { env_var: "HELIOS_POWER_POLL_INTERVAL_MS", default: 100, min: 20, max: 10_000 },
    idle_interval_ms: BoundedU64Policy { env_var: "HELIOS_POWER_IDLE_INTERVAL_MS", default: 1_000, min: 100, max: 30_000 },
};
