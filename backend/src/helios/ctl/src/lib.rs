//! heliosctl as a library: the OTA submission path and the on-disk OTA state, shared by the
//! `heliosctl` binary and helios-api (Atlas's HTTP OTA path).

pub mod ota_state;
pub mod update;
