//! heliosctl as a library: the OTA submission path, the on-disk OTA state and the device
//! security file, shared by the `heliosctl` binary and helios-api.

pub mod auth_state;
pub mod ota_state;
pub mod update;
