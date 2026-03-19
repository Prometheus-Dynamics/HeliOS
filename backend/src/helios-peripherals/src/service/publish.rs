use chrono::Utc;

use crate::dto::{SensorInventory, SensorScope};
use crate::ipc::SensorEvent;
use tracing::warn;

use super::SensorsService;

impl SensorsService {
    pub async fn publish_snapshot(&self, command_id: Option<lib_ipc::types::CommandId>, scope: &SensorScope) {
        if self.event_bus.receiver_count() == 0 {
            return;
        }
        match self.cached_snapshot(scope).await {
            Ok(snapshot) => publish_event(self, SensorEvent::Snapshot { command_id, scope: scope.clone(), values: snapshot }),
            Err(err) => warn!(?scope, %err, "failed to publish snapshot"),
        }
    }

    pub fn publish_inventory(&self, command_id: lib_ipc::types::CommandId, inventory: SensorInventory) {
        publish_event(self, SensorEvent::Inventory { command_id, inventory });
    }

    pub fn publish_ack(&self, command_id: lib_ipc::types::CommandId) {
        publish_event(self, SensorEvent::Ack { command_id, processed_at: Utc::now() });
    }

    pub fn publish_nack(&self, command_id: lib_ipc::types::CommandId, reason: String, retryable: bool) {
        publish_event(self, SensorEvent::Nack { command_id, reason, retryable });
    }

    pub fn publish_event(&self, event: SensorEvent) {
        publish_event(self, event);
    }
}

pub(super) fn publish_event(service: &SensorsService, event: SensorEvent) {
    if service.event_bus.receiver_count() == 0 {
        return;
    }
    let _ = service.event_bus.send(event);
}
