use std::collections::{BTreeMap, BTreeSet};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use lib_sensors::model::SensorReading;

use crate::dto::{I2cInventory, LightingCommand, LightingRuntimeState, SensorInventory, SensorKind, SensorScope, SensorSnapshot};

use super::SensorsService;

#[derive(Debug, Default)]
pub(super) struct SensorsState {
    pub(super) inventory: SensorInventory,
    pub(super) scopes: BTreeSet<SensorScope>,
    pub(super) scope_subscribers: BTreeMap<SensorScope, usize>,
    pub(super) readings: BTreeMap<SensorScope, BTreeMap<SensorKind, SensorReading>>,
    pub(super) serialized_readings: BTreeMap<SensorScope, SensorSnapshot>,
    pub(super) i2c_inventory: Option<I2cInventory>,
    pub(super) i2c_inventory_updated_at: Option<Instant>,
    pub(super) lighting_state: LightingRuntimeState,
}

impl SensorsState {
    pub(super) fn with_device_scope(initial_lighting_state: LightingRuntimeState) -> Self {
        let mut state = Self { lighting_state: initial_lighting_state, ..Self::default() };
        state.scopes.insert(SensorScope::Device);
        state.readings.insert(SensorScope::Device, BTreeMap::new());
        state.serialized_readings.insert(SensorScope::Device, BTreeMap::new());
        state
    }
}

impl SensorsService {
    pub(super) async fn update_lighting_state(&self, command: LightingCommand) -> LightingRuntimeState {
        let mut guard = self.state.write().await;
        guard.lighting_state = LightingRuntimeState { command, updated_at_ms: now_ms() };
        guard.lighting_state.clone()
    }

    pub async fn lighting_state(&self) -> LightingRuntimeState {
        self.state.read().await.lighting_state.clone()
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().min(u128::from(u64::MAX)) as u64
}
