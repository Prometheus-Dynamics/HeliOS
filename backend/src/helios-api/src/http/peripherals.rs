mod inventory;
mod sensors;
mod support;
mod types;

use axum::{
    Router,
    routing::{get, post},
};

use super::AppState;

pub use types::*;

pub(crate) use inventory::{
    __path_fan_status, __path_led_status, __path_list_cameras, __path_list_i2c, __path_list_peripherals, __path_list_usb, fan_status, led_status, list_cameras, list_i2c, list_peripherals, list_usb,
    scan_i2c,
};
pub(crate) use sensors::{configure_sensor_alias, configure_sensor_firmware, derive_lighting_status, list_sensors, map_sensor_inventory};
pub(crate) use support::{list_usb_sysfs, map_fan_status};

pub(super) async fn invalidate_peripheral_inventory_cache(state: &AppState) {
    state.services.hardware.invalidate_peripheral_inventory_cache().await;
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_peripherals))
        .route("/cameras", get(list_cameras))
        .route("/i2c", get(list_i2c))
        .route("/i2c/scan", post(scan_i2c))
        .route("/usb", get(list_usb))
        .route("/fan", get(fan_status))
        .route("/leds", get(led_status))
        .route("/sensors", get(list_sensors))
        .route("/sensors/firmware", post(configure_sensor_firmware))
        .route("/sensors/alias", post(configure_sensor_alias))
}
