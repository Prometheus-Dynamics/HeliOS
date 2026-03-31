use super::{
    UsbPowerSettings,
    support::{parse_bool, parse_env_file, render_settings_for_tests},
};

#[test]
fn parse_env_file_ignores_comments_and_blank_lines() {
    let env = parse_env_file(
        "
        # comment
        USB_POWER_ENABLED=1

        USB_POWER_USB_A_GPIO=23
        malformed
        ",
    );
    assert_eq!(env.get("USB_POWER_ENABLED").map(String::as_str), Some("1"));
    assert_eq!(env.get("USB_POWER_USB_A_GPIO").map(String::as_str), Some("23"));
    assert!(!env.contains_key("malformed"));
}

#[test]
fn parse_bool_accepts_common_truthy_and_falsey_variants() {
    assert!(parse_bool(Some("true"), false));
    assert!(parse_bool(Some("YES"), false));
    assert!(!parse_bool(Some("off"), true));
    assert!(!parse_bool(None, false));
}

#[test]
fn render_settings_serializes_expected_flags() {
    let rendered = render_settings_for_tests(&UsbPowerSettings {
        enabled: true,
        usb_a_gpio: Some(23),
        usb_c_gpio: None,
        usb_a_active_high: false,
        usb_c_active_high: true,
        usb_a_enabled: false,
        usb_c_enabled: true,
        tuning_enabled: false,
        disable_autosuspend: true,
        disable_usb2_lpm: false,
        force_power_control_on: true,
    });

    assert!(rendered.contains("USB_POWER_ENABLED=1"));
    assert!(rendered.contains("USB_POWER_USB_A_GPIO=23"));
    assert!(!rendered.contains("USB_POWER_USB_C_GPIO="));
    assert!(rendered.contains("USB_POWER_USB_A_ACTIVE_HIGH=0"));
    assert!(rendered.contains("USB_POWER_DISABLE_USB2_LPM=0"));
}
