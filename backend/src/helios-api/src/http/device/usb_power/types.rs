use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq, Eq)]
pub struct UsbPowerSettings {
    pub enabled: bool,
    pub usb_a_gpio: Option<u32>,
    pub usb_c_gpio: Option<u32>,
    pub usb_a_active_high: bool,
    pub usb_c_active_high: bool,
    pub usb_a_enabled: bool,
    pub usb_c_enabled: bool,
    #[serde(default = "default_true")]
    pub tuning_enabled: bool,
    #[serde(default = "default_true")]
    pub disable_autosuspend: bool,
    #[serde(default = "default_true")]
    pub disable_usb2_lpm: bool,
    #[serde(default = "default_true")]
    pub force_power_control_on: bool,
}

impl Default for UsbPowerSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            usb_a_gpio: None,
            usb_c_gpio: None,
            usb_a_active_high: true,
            usb_c_active_high: true,
            usb_a_enabled: true,
            usb_c_enabled: true,
            tuning_enabled: true,
            disable_autosuspend: true,
            disable_usb2_lpm: true,
            force_power_control_on: true,
        }
    }
}

pub(super) fn default_true() -> bool {
    true
}
