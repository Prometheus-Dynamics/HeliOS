use lib_sensors::led_config;

use crate::config::SensorsConfig;

pub(crate) fn resolve_led_config(config: &SensorsConfig) -> led_config::LedConfig {
    let mut led_paths = led_config::default_paths();
    led_paths.extend(config.config_paths());
    let mut leds = led_config::load_led_config(&led_paths).unwrap_or_default();
    // Force LEDs to be treated as available so they are always exposed as a peripheral.
    leds.enabled = true;
    leds
}
