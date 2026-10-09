//! HeliOS's status on the board's status light: lemnosd's status layer
//! (`LedClient::status`), from the peripherals provider's health. lemnosd arbitrates it with
//! other clients (system states, locate and self-tests win) and drops it when HeliOS
//! disconnects, so a crashed HeliOS never leaves a stale look on the ring.

use lemnos_ipc::LedStatus;

/// The peripherals provider's health, as the status light shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderHealth {
    /// Starting, or connecting to Orion.
    Starting,
    /// Registered with Orion and publishing.
    Ready,
    /// Running without part of its inputs (a watch stopped).
    Degraded,
    /// Stopping on an error.
    Failed,
}

impl ProviderHealth {
    pub const fn led_status(self) -> LedStatus {
        match self {
            Self::Starting => LedStatus::Busy,
            Self::Ready => LedStatus::Ok,
            Self::Degraded => LedStatus::Warn,
            Self::Failed => LedStatus::Error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_health_maps_onto_the_status_layer() {
        assert_eq!(ProviderHealth::Starting.led_status(), LedStatus::Busy);
        assert_eq!(ProviderHealth::Ready.led_status(), LedStatus::Ok);
        assert_eq!(ProviderHealth::Degraded.led_status(), LedStatus::Warn);
        assert_eq!(ProviderHealth::Failed.led_status(), LedStatus::Error);
    }
}
