use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use lib_sensors::led_config;
use lib_sensors::model::SensorReading;
use lib_sensors::model::json_from_sensor_reading;
use tokio::sync::{Mutex, RwLock, broadcast};
use tokio_util::sync::CancellationToken;

use crate::ai::AiModelManager;
use crate::config::SensorsConfig;
use crate::config_store::SensorConfigStore;
use crate::dto::{JsonData, LightingCommand, LightingRuntimeState, SensorKind, SensorSnapshot};
use crate::fan::FanController;
use crate::ipc::SensorEvent;
use crate::lighting::{LightingController, resolve_led_device_path};
use crate::orchestrator::RuntimeOrchestrator;

mod ai;
mod alias;
mod firmware;
mod i2c;
mod inventory;
mod publish;
mod readings;
mod runtimes;
mod scopes;
mod state;

use state::SensorsState;

const EVENT_BUS_CAPACITY: usize = 256;

pub struct SensorsService {
    config: Arc<SensorsConfig>,
    state: RwLock<SensorsState>,
    command_lock: Mutex<()>,
    config_store: Arc<Mutex<SensorConfigStore>>,
    event_bus: broadcast::Sender<SensorEvent>,
    ai: AiModelManager,
    lighting_config: led_config::LedConfig,
    shutdown: CancellationToken,
    runtimes: RuntimeOrchestrator,
}

impl SensorsService {
    pub fn config(&self) -> Arc<SensorsConfig> {
        Arc::clone(&self.config)
    }

    pub fn led_config(&self) -> &led_config::LedConfig {
        &self.lighting_config
    }

    pub fn new(config: Arc<SensorsConfig>, shutdown: CancellationToken) -> Self {
        let (event_bus, _rx) = broadcast::channel(EVENT_BUS_CAPACITY);
        let config_store = Arc::new(Mutex::new(SensorConfigStore::load()));
        let ai = AiModelManager::with_default_path();
        let lighting_config = crate::inventory::resolve_led_config(&config);
        let lighting = build_lighting_controller(&lighting_config);
        let fan = Arc::new(FanController::new());
        let initial_lighting = LightingRuntimeState { command: LightingCommand { frame: None, brightness: lighting_config.brightness, animation: None }, updated_at_ms: now_ms() };

        Self {
            config,
            state: RwLock::new(SensorsState::with_device_scope(initial_lighting)),
            command_lock: Mutex::new(()),
            config_store,
            event_bus,
            ai,
            lighting_config,
            shutdown,
            runtimes: RuntimeOrchestrator::new(fan, lighting),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<SensorEvent> {
        self.event_bus.subscribe()
    }

    pub(crate) fn event_sender(&self) -> broadcast::Sender<SensorEvent> {
        self.event_bus.clone()
    }
}

fn build_lighting_controller(config: &led_config::LedConfig) -> Arc<LightingController> {
    let device_path = resolve_led_device_path();
    Arc::new(LightingController::new(device_path, config.count, &config.color_order))
}

fn serialize_readings(readings: &BTreeMap<SensorKind, SensorReading>) -> SensorSnapshot {
    readings
        .iter()
        .map(|(kind, reading)| {
            let json = json_from_sensor_reading(reading);
            (kind.clone(), JsonData::from_value(&json))
        })
        .collect()
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().min(u128::from(u64::MAX)) as u64
}
