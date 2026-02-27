use std::sync::Arc;

use crate::error::Result;
use crate::ipc::{FirmwareUpdate, FirmwareUpdateStatus, SensorEvent};
use chrono::Utc;

use super::SensorsService;

impl SensorsService {
    pub async fn configure_firmware(self: &Arc<Self>, device_id: &str, firmware: String) -> Result<()> {
        let mut store = self.config_store.lock().await;
        let (firmware_label, last_flashed) = {
            let entry = store.get_or_default(device_id);
            entry.firmware = Some(firmware);
            entry.last_error = None;
            (entry.firmware.clone().unwrap_or_default(), entry.last_flashed_firmware.clone())
        };
        store.save();

        let update = FirmwareUpdate {
            device_id: device_id.to_string(),
            firmware: firmware_label,
            status: FirmwareUpdateStatus::Queued,
            active: last_flashed,
            error: None,
            progress_pct: Some(0),
            detail: Some("Queued for flashing".to_string()),
            timestamp_ms: Utc::now().timestamp_millis().max(0) as u64,
        };
        self.publish_event(SensorEvent::FirmwareUpdate { update });

        // Trigger a refresh so the new firmware is applied and reflected in inventory.
        let service = Arc::clone(self);
        tokio::spawn(async move {
            if let Err(err) = service.discover_with_options(true, false).await {
                tracing::warn!(error = %err, "failed to refresh inventory after firmware update");
            }
        });

        Ok(())
    }
}
