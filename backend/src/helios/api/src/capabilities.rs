//! The device's capabilities: what this device is, and which features it has.
//!
//! HeliOS runs in two places. An **owner** device is the Raze image: HeliOS is the OS, the layout
//! has a board contract (identity, update writer, slots) and the board's hardware. A **guest**
//! runs on another OS (SystemCore): no board contract, so no updates, no reboot, and only the
//! hardware the host leaves to it.
//!
//! Every feature is derived from facts this process has, never from a device name: the board
//! contract, the host's os-release, the guest security switch, and the lemnosd devices Orion
//! reports (`lemnos.device` resources, by their `lemnos.class`). `GET /v1/device` reports them, the
//! routes refuse a feature the device does not have ([`require`]), and the UI shows only what is on.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::{
    ApiConfig, SharedState,
    error::{ApiError, ApiResult},
    host,
    orion::{StateView, label_map},
    routes::{resources::LEMNOS_DEVICE_TYPE, system::device_package_facts},
};

pub const PROFILE_OWNER: &str = "owner";
pub const PROFILE_GUEST: &str = "guest";

/// Features, by the name the API and the UI use.
pub mod feature {
    /// OS and HeliOS updates through the device package's writer (uploads, apply).
    pub const UPDATES: &str = "updates";
    /// Boot slots: switching them without an update.
    pub const SLOTS: &str = "slots";
    /// Rebooting the device from HeliOS.
    pub const REBOOT: &str = "reboot";
    pub const SAFE_MODE: &str = "safe_mode";
    /// The slot recovery ladder (the owner image's R1..R4).
    pub const RECOVERY_LADDER: &str = "recovery_ladder";
    /// The status ring (lemnosd's `light` device).
    pub const LEDS: &str = "leds";
    /// The fan (lemnosd's `fan` device).
    pub const FAN: &str = "fan";
    /// The IMU (lemnosd's `imu` device).
    pub const IMU: &str = "imu";
    /// The board's power monitor (lemnosd's `power-monitor` device).
    pub const POWER: &str = "power";
    /// Switchable USB port power (lemnosd's `power-switch` devices).
    pub const USB_POWER: &str = "usb_power";
    /// Sign-in and API tokens.
    pub const SECURITY: &str = "security";
    /// The owner image's USB serial console.
    pub const CONSOLE: &str = "console";

    /// Features that need lemnosd's devices, so they need Orion's resources to be read.
    pub const HARDWARE: &[&str] = &[LEDS, FAN, IMU, POWER, USB_POWER];
}

/// The host OS's os-release `ID` on SystemCore.
const SYSTEMCORE_OS_ID: &str = "limelightos_systemcore";
/// `lemnos.class` of the devices each hardware feature needs.
const LIGHT_CLASS: &str = "light";
const FAN_CLASS: &str = "fan";
const IMU_CLASS: &str = "imu";
const POWER_MONITOR_CLASS: &str = "power-monitor";
const POWER_SWITCH_CLASS: &str = "power-switch";

const OWNER_ONLY_REASON: &str = "the host owns the device on a guest; HeliOS installs as a package";
const NO_HARDWARE: &str = "lemnosd reports no such device on this device";

/// The facts the derivation reads. [`facts`] fills them from the process; tests fill them directly.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facts {
    /// The layout has a board contract (the owner profile).
    pub owner: bool,
    /// os-release `ID`, `PRETTY_NAME` and `VERSION_ID` (or `VERSION`).
    pub os_id: Option<String>,
    pub os_name: Option<String>,
    pub os_version: Option<String>,
    /// The device package's model (`BOARD_DEVICE_MODEL`), for an owner's host kind.
    pub board_model: Option<String>,
    /// The `lemnos.class` of each lemnosd device Orion reports.
    pub lemnos_classes: BTreeSet<String>,
    /// `HELIOS_API_GUEST_SECURITY`.
    pub guest_security: bool,
}

/// The host HeliOS runs on: the owner's board, SystemCore, or any other OS.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Host {
    pub kind: String,
    pub name: Option<String>,
    pub release: Option<String>,
}

/// The device's capabilities, as `GET /v1/device` reports them (flattened into the device).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Capabilities {
    pub profile: &'static str,
    pub host: Host,
    /// Every feature, on or off.
    pub features: BTreeMap<&'static str, bool>,
    /// Why each feature that is off is off.
    pub reasons: BTreeMap<&'static str, &'static str>,
}

impl Capabilities {
    pub fn has(&self, feature: &str) -> bool {
        self.features.get(feature).copied().unwrap_or(false)
    }
}

/// The capabilities for facts. Pure: everything the process reads is in `facts`.
pub fn derive(facts: &Facts) -> Capabilities {
    let lemnos = |class: &str| facts.lemnos_classes.contains(class);
    let owner = facts.owner;
    // (feature, on, why it is off)
    let table: [(&'static str, bool, &'static str); 12] = [
        (feature::UPDATES, owner, OWNER_ONLY_REASON),
        (feature::SLOTS, owner, OWNER_ONLY_REASON),
        (feature::REBOOT, owner, "a guest is not rebooted from HeliOS: the host controller is not HeliOS's to restart"),
        (feature::SAFE_MODE, owner, OWNER_ONLY_REASON),
        (feature::RECOVERY_LADDER, owner, OWNER_ONLY_REASON),
        (feature::LEDS, lemnos(LIGHT_CLASS), NO_HARDWARE),
        (feature::FAN, lemnos(FAN_CLASS), NO_HARDWARE),
        (feature::IMU, lemnos(IMU_CLASS), NO_HARDWARE),
        (feature::POWER, lemnos(POWER_MONITOR_CLASS), NO_HARDWARE),
        (feature::USB_POWER, lemnos(POWER_SWITCH_CLASS), NO_HARDWARE),
        (feature::SECURITY, owner || facts.guest_security, "a guest's host owns access; HELIOS_API_GUEST_SECURITY=on secures HeliOS here"),
        (feature::CONSOLE, owner, OWNER_ONLY_REASON),
    ];
    let mut features = BTreeMap::new();
    let mut reasons = BTreeMap::new();
    for (name, on, reason) in table {
        features.insert(name, on);
        if !on {
            reasons.insert(name, reason);
        }
    }
    Capabilities { profile: if owner { PROFILE_OWNER } else { PROFILE_GUEST }, host: host_of(facts), features, reasons }
}

fn host_of(facts: &Facts) -> Host {
    let kind = if facts.os_id.as_deref() == Some(SYSTEMCORE_OS_ID) {
        "systemcore".to_string()
    } else if facts.owner {
        facts.board_model.clone().unwrap_or_else(|| "image".into())
    } else {
        "linux".into()
    };
    Host { kind, name: facts.os_name.clone(), release: facts.os_version.clone() }
}

/// Whether this process is an owner device (its layout has a board contract).
pub fn is_owner(config: &ApiConfig) -> bool {
    config.board.is_some()
}

/// The `lemnos.class` of each lemnosd device in an Orion view.
pub fn lemnos_classes_in(view: &StateView) -> BTreeSet<String> {
    view.resources.values().filter(|record| record.resource_type.as_str() == LEMNOS_DEVICE_TYPE).filter_map(|record| label_map(&record.labels).remove("lemnos.class")).collect()
}

/// The lemnosd device classes given Orion's view (`None` when it could not be read). Unit tests have
/// no Orion: their stand-in classes win.
#[cfg_attr(not(test), allow(unused_variables))]
fn lemnos_classes_at(config: &ApiConfig, view: Option<&StateView>) -> BTreeSet<String> {
    #[cfg(test)]
    if let Some(classes) = &config.test_lemnos_classes {
        return classes.clone();
    }
    view.map(lemnos_classes_in).unwrap_or_default()
}

/// The lemnosd device classes now. Without Orion the error is returned, since the device may well
/// have the hardware.
async fn lemnos_classes(state: &SharedState) -> ApiResult<BTreeSet<String>> {
    #[cfg(test)]
    if state.config.test_lemnos_classes.is_some() {
        return Ok(lemnos_classes_at(&state.config, None));
    }
    Ok(lemnos_classes_in(&state.orion.view().await?))
}

/// The facts of this process: its layout's board, the host's os-release, and the lemnosd devices
/// it was given.
pub fn facts(config: &ApiConfig, lemnos_classes: BTreeSet<String>) -> Facts {
    let os = host::os_release(&config.layout.os_release_file());
    Facts {
        owner: is_owner(config),
        os_id: os.get("ID").cloned(),
        os_name: os.get("PRETTY_NAME").or_else(|| os.get("NAME")).cloned(),
        os_version: os.get("VERSION_ID").or_else(|| os.get("VERSION")).cloned(),
        board_model: device_package_facts(config.board.as_ref()).get("BOARD_DEVICE_MODEL").map(|model| model.to_ascii_lowercase()),
        lemnos_classes,
        guest_security: config.guest_security,
    }
}

/// The device's capabilities, given the lemnosd device classes it has.
pub fn current(config: &ApiConfig, lemnos_classes: BTreeSet<String>) -> Capabilities {
    derive(&facts(config, lemnos_classes))
}

/// The capabilities now, for `GET /v1/device`. Without Orion's view the hardware features are off.
pub fn now(config: &ApiConfig, view: Option<&StateView>) -> Capabilities {
    current(config, lemnos_classes_at(config, view))
}

/// The refusal for a feature this device does not have: 404 with the reason.
pub fn not_on_device(name: &str, reason: &str) -> ApiError {
    ApiError::not_found(format!("{name} is not on this device: {reason}"))
}

/// Refuses a request whose feature the device does not have (404 "not on this device"). The
/// hardware features read Orion's resources; when Orion is unreachable that error is returned
/// instead, since the device may well have the hardware.
pub async fn require(state: &SharedState, feature: &'static str) -> ApiResult<()> {
    let lemnos = if feature::HARDWARE.contains(&feature) { lemnos_classes(state).await? } else { BTreeSet::new() };
    let capabilities = current(&state.config, lemnos);
    if capabilities.has(feature) {
        return Ok(());
    }
    Err(not_on_device(feature, capabilities.reasons.get(feature).copied().unwrap_or(OWNER_ONLY_REASON)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lemnos(classes: &[&str]) -> BTreeSet<String> {
        classes.iter().map(|class| class.to_string()).collect()
    }

    /// The Raze image: a board contract, its os-release, and lemnosd's status ring, fan, IMU,
    /// power monitor and USB power switches.
    fn raze() -> Facts {
        Facts {
            owner: true,
            os_id: Some("helios".into()),
            os_name: Some("Helios v2026.2.0".into()),
            os_version: Some("v2026.2.0".into()),
            board_model: Some("raze".into()),
            lemnos_classes: lemnos(&["light", "fan", "imu", "power-monitor", "power-switch"]),
            guest_security: false,
        }
    }

    /// SystemCore: a guest with the host's os-release and no lemnosd devices.
    fn systemcore() -> Facts {
        Facts {
            owner: false,
            os_id: Some(SYSTEMCORE_OS_ID.into()),
            os_name: Some("limelightosr-2027.0.0-alpha18".into()),
            os_version: Some("aef8f2eb".into()),
            ..Facts::default()
        }
    }

    #[test]
    fn the_owner_image_has_its_hardware_and_its_board_features() {
        let capabilities = derive(&raze());
        assert_eq!(capabilities.profile, PROFILE_OWNER);
        assert_eq!(capabilities.host.kind, "raze");
        for name in [feature::UPDATES, feature::SLOTS, feature::REBOOT, feature::SAFE_MODE, feature::RECOVERY_LADDER, feature::SECURITY, feature::CONSOLE] {
            assert!(capabilities.has(name), "{name} on the owner image");
        }
        for name in feature::HARDWARE {
            assert!(capabilities.has(name), "{name} from lemnosd's devices");
        }
        assert!(capabilities.reasons.is_empty(), "nothing is off: {:?}", capabilities.reasons);
    }

    #[test]
    fn a_guest_has_no_updates_reboot_or_security_and_no_hardware_it_is_not_given() {
        let capabilities = derive(&systemcore());
        assert_eq!(capabilities.profile, PROFILE_GUEST);
        assert_eq!(capabilities.host.kind, "systemcore");
        for name in [feature::UPDATES, feature::SLOTS, feature::REBOOT, feature::SAFE_MODE, feature::RECOVERY_LADDER, feature::SECURITY, feature::CONSOLE] {
            assert!(!capabilities.has(name), "{name} is off on a guest");
            assert!(capabilities.reasons.contains_key(name), "{name} says why");
        }
        for name in feature::HARDWARE {
            assert!(!capabilities.has(name), "{name} is off without lemnosd's device");
        }
    }

    #[test]
    fn hardware_follows_the_lemnosd_devices_present() {
        let mut facts = systemcore();
        facts.lemnos_classes = lemnos(&["light"]);
        let capabilities = derive(&facts);
        assert!(capabilities.has(feature::LEDS), "a status ring");
        assert!(!capabilities.has(feature::FAN));
        assert!(!capabilities.has(feature::IMU));
        assert!(!capabilities.has(feature::POWER));
        assert!(!capabilities.has(feature::USB_POWER));
    }

    #[test]
    fn a_guest_can_have_security_by_its_switch() {
        let mut facts = systemcore();
        facts.guest_security = true;
        let capabilities = derive(&facts);
        assert!(capabilities.has(feature::SECURITY));
        assert!(!capabilities.has(feature::UPDATES), "the switch is for security only");
        assert!(!capabilities.reasons.contains_key(feature::SECURITY));
    }

    #[test]
    fn the_host_kind_comes_from_the_host_not_the_board_name_on_a_guest() {
        let mut other = systemcore();
        other.os_id = Some("debian".into());
        assert_eq!(derive(&other).host.kind, "linux");
        assert_eq!(derive(&raze()).host.kind, "raze");
        let mut unnamed = raze();
        unnamed.board_model = None;
        assert_eq!(derive(&unnamed).host.kind, "image");
    }

    #[test]
    fn the_reasons_are_given_only_for_what_is_off() {
        let capabilities = derive(&systemcore());
        let on_keys: Vec<&str> = capabilities.features.iter().filter(|(_, on)| **on).map(|(name, _)| *name).collect();
        assert!(on_keys.is_empty(), "a bare guest has no features on: {on_keys:?}");
        assert_eq!(capabilities.reasons.len(), capabilities.features.len());
    }
}
