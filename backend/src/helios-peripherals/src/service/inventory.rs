use std::sync::Arc;

use lib_sensors::sensor_config;

use crate::dto::SensorInventory;
use crate::error::Result;

use super::SensorsService;

impl SensorsService {
    pub async fn discover(self: &Arc<Self>, refresh: bool) -> Result<SensorInventory> {
        self.discover_with_options(refresh, false).await
    }

    pub async fn discover_with_options(self: &Arc<Self>, refresh: bool, restart_runtimes: bool) -> Result<SensorInventory> {
        let _guard = self.command_lock.lock().await;

        if refresh {
            let devices = sensor_config::load_sensor_devices(&self.config.config_paths());
            let mut descriptors = Vec::with_capacity(devices.len() + 4);
            descriptors.push(crate::inventory::descriptor_from_led_config(&self.lighting_config));
            if let Ok(status) = self.fan_status().await {
                descriptors.push(crate::inventory::descriptor_from_fan_status(&status));
            }
            for device in devices {
                descriptors.push(crate::inventory::descriptor_from_device(&device));
            }
            let coral_descriptors = crate::inventory::build_coral_descriptors(&self.config_store, Some(self.event_sender())).await;
            descriptors.extend(coral_descriptors);

            let mut state = self.state.write().await;
            state.inventory = SensorInventory { sensors: descriptors };

            if restart_runtimes {
                // Refresh runtimes so newly configured devices are probed and begin reporting telemetry.
                let _ = self.restart_runtimes().await;
            }
            Ok(state.inventory.clone())
        } else {
            let state = self.state.read().await;
            Ok(state.inventory.clone())
        }
    }

    pub async fn inventory(&self) -> SensorInventory {
        let state = self.state.read().await;
        state.inventory.clone()
    }
}
