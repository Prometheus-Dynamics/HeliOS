//! lemnosd, the board's hardware service (Lemnos `docs/system-service.md`): HeliOS's sensors,
//! fan and status light go through it, as a `lemnos-ipc` client named `helios`. HeliOS opens no
//! I2C bus, LED device or fan sysfs file itself.

mod bridge;
pub mod fan;
pub mod led;
pub mod resources;

#[cfg(test)]
mod tests;

pub use bridge::{LemnosdBridge, LemnosdOptions, Notify};
pub use fan::{FAN_OVERRIDE_ACTION, FAN_RELEASE_ACTION, FanOverrideRequest};
pub use led::ProviderHealth;
pub use resources::{CONTROL_SET_ACTION, LemnosdProbe, LemnosdState};
