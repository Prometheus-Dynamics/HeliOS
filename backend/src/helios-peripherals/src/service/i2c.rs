use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::time::timeout;
use tracing::warn;

use crate::dto::I2cInventory;
use crate::error::{Error, Result};

use super::SensorsService;

const I2C_INVENTORY_TIMEOUT: Duration = Duration::from_secs(5);
const I2C_INVENTORY_CACHE_TTL: Duration = Duration::from_secs(2);

impl SensorsService {
    pub async fn i2c_inventory(self: &Arc<Self>) -> Result<I2cInventory> {
        {
            let state = self.state.read().await;
            if let Some(inventory) = state.i2c_inventory.as_ref()
                && state.i2c_inventory_updated_at.is_some_and(|ts| ts.elapsed() < I2C_INVENTORY_CACHE_TTL)
            {
                return Ok(inventory.clone());
            }
        }

        let config_paths = self.config.config_paths();
        match timeout(I2C_INVENTORY_TIMEOUT, tokio::task::spawn_blocking(move || crate::inventory::read_i2c_inventory(config_paths))).await {
            Ok(result) => {
                let inventory = match result {
                    Ok(inventory) => inventory?,
                    Err(err) => return Err(Error::InvalidState(format!("i2c inventory task failed: {err}"))),
                };
                let mut state = self.state.write().await;
                state.i2c_inventory = Some(inventory.clone());
                state.i2c_inventory_updated_at = Some(Instant::now());
                Ok(inventory)
            }
            Err(_) => {
                warn!("i2c inventory: probe timed out; returning cached snapshot");
                let state = self.state.read().await;
                Ok(state.i2c_inventory.clone().unwrap_or_default())
            }
        }
    }
}
