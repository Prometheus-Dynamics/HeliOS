use std::sync::Arc;

use crate::error::{Error, Result};

use super::SensorsService;

const MAX_ALIAS_LEN: usize = 64;

impl SensorsService {
    pub async fn configure_alias(self: &Arc<Self>, hardware_key: &str, alias: String) -> Result<()> {
        let hardware_key = hardware_key.trim();
        if hardware_key.is_empty() {
            return Err(Error::InvalidConfig("hardware_key is required".into()));
        }

        let alias = alias.trim().to_string();
        let alias = if alias.is_empty() { None } else { Some(alias) };

        if let Some(value) = alias.as_deref()
            && value.len() > MAX_ALIAS_LEN
        {
            return Err(Error::InvalidConfig(format!("alias must be {MAX_ALIAS_LEN} characters or fewer")));
        }

        {
            let _guard = self.command_lock.lock().await;
            let mut store = self.config_store.lock().await;
            let changed = store.set_alias_override(hardware_key, alias);
            if changed {
                store.save();
            }
        }

        // Trigger a refresh so the updated alias is applied and reflected in inventory.
        self.discover(true).await.map(|_| ())
    }
}
