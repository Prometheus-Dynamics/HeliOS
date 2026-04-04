use std::path::PathBuf;

use super::super::config::{DEV_PERIPHERALS_SOCKET, resolve_peripherals_socket_candidates};
use super::support::env_lock;

#[test]
fn peripherals_candidates_prefer_canonical_socket_override() {
    let _lock = env_lock();
    unsafe { std::env::set_var("HELIOS_PERIPHERALS_SOCKET", "/tmp/policy-peripherals.sock") };
    let candidates = resolve_peripherals_socket_candidates();
    assert_eq!(candidates, vec![PathBuf::from("/tmp/policy-peripherals.sock"), PathBuf::from(DEV_PERIPHERALS_SOCKET), PathBuf::from("/run/helios/peripherals.sock")]);
    unsafe { std::env::remove_var("HELIOS_PERIPHERALS_SOCKET") };
}

#[test]
fn peripherals_candidates_ignore_removed_socket_aliases() {
    let _lock = env_lock();
    unsafe {
        std::env::remove_var("HELIOS_PERIPHERALS_SOCKET");
        std::env::set_var("PERIPHERALS_SOCKET", "/tmp/legacy-peripherals.sock");
        std::env::set_var("SENSORS_SOCKET", "/tmp/legacy-sensors.sock");
        std::env::set_var("SENSOR_SOCKET", "/tmp/legacy-sensor.sock");
    }
    let candidates = resolve_peripherals_socket_candidates();
    assert_eq!(candidates, vec![PathBuf::from(DEV_PERIPHERALS_SOCKET), PathBuf::from("/run/helios/peripherals.sock")]);
    unsafe {
        std::env::remove_var("PERIPHERALS_SOCKET");
        std::env::remove_var("SENSORS_SOCKET");
        std::env::remove_var("SENSOR_SOCKET");
    }
}
