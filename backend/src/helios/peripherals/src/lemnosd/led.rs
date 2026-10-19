//! HeliOS's status on the board's status light, from the peripherals provider's health. It is
//! sent as lemnosd's built-in `status.*` look in the application layer at [`LED_PRIORITY`], so
//! helios-api's LED director (higher priority) covers it while the API runs. lemnosd drops it
//! when HeliOS disconnects, so a crashed HeliOS never leaves a stale look on the ring.

use lemnos_ipc::LedStatus;

/// The priority of the peripherals LED intents. helios-api's LED director holds the ring at a
/// higher priority while it runs; this status is the fallback below it.
pub const LED_PRIORITY: u8 = 20;

/// The built-in look that shows `status` (lemnosd's `status.*`, the board's colours).
pub const fn status_look(status: LedStatus) -> &'static str {
    match status {
        LedStatus::Ok => "status.ok",
        LedStatus::Warn => "status.warn",
        LedStatus::Error => "status.error",
        LedStatus::Busy => "status.busy",
        LedStatus::Off => "status.off",
    }
}

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

    #[test]
    fn status_is_the_built_in_look_below_the_director() {
        assert_eq!(status_look(LedStatus::Busy), "status.busy");
        assert_eq!(status_look(LedStatus::Off), "status.off");
    }
}
