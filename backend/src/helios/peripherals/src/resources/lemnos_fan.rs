//! The Raze fan (and other hwmon fans) as a Lemnos device: **read-only by default**.
//!
//! The kernel drives the fan: the device package's `raze-fan` overlay hands it to the thermal
//! governor (`pwm1_enable=2`, automatic) with the package's 70 % floor. Raw `pwm1`/`pwm1_enable`
//! writes would fight that governor and bypass the floor, so this driver only reads, and the
//! one way to set the speed is an explicit, time-limited manual override:
//!
//! - `fan.read`: pwm, mode and rpm.
//! - `fan.override` `{ "pwm": 0..=255, "duration_ms": 1000..=600000 }` (default 60 s): manual
//!   mode at that duty cycle. It always ends in automatic mode (`pwm1_enable=2`): on
//!   `fan.release`, when `duration_ms` runs out (a timer thread, independent of polling), when
//!   the device is dropped (helios-peripherals stops), and on the next bind if a crash left the
//!   fan in manual mode. helios-peripherals.service also restores it on stop (`ExecStopPost`).
//! - `fan.release`: back to automatic now.
//!
//! Standard PWM requests other than `GetConfiguration` are refused. The driver binds only
//! hwmon fan devices (a `pwm1` control and Lemnos's fan capability), not every hwmon device.

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use lemnos::core::{
    CapabilityId, CoreError, CustomInteractionResponse, DeviceDescriptor, DeviceKind, DeviceLifecycleState, DeviceStateSnapshot, InteractionRequest, InteractionResponse, InterfaceKind,
    OperationRecord, OperationStatus, PwmConfiguration, PwmPolarity, PwmRequest, PwmResponse, StandardResponse, Value, ValueMap,
};
use lemnos::driver::{
    BoundDevice, CustomInteraction, Driver, DriverBindContext, DriverError, DriverManifest, DriverMatch, DriverPriority, DriverResult, LinuxClassDeviceIo, MatchCondition, MatchRule, interaction_name,
};

pub const FAN_READ_INTERACTION: &str = "fan.read";
pub const FAN_OVERRIDE_INTERACTION: &str = "fan.override";
pub const FAN_RELEASE_INTERACTION: &str = "fan.release";

/// `pwm1_enable`: 1 is manual, 2 is automatic (the kernel's thermal control).
const MODE_MANUAL: u64 = 1;
const MODE_AUTOMATIC: u64 = 2;
const DEFAULT_OVERRIDE_MS: u64 = 60_000;
const MIN_OVERRIDE_MS: u64 = 1_000;
const MAX_OVERRIDE_MS: u64 = 600_000;
/// Lemnos's hwmon probe gives fan devices (those with a `pwm1` control) this capability.
const FAN_CAPABILITY: &str = "fan.set_pwm";

const INTERACTIONS: [(&str, &str); 3] = [
    (FAN_READ_INTERACTION, "Read fan hwmon state"),
    (FAN_OVERRIDE_INTERACTION, "Manual fan speed for a limited time: {pwm: 0-255, duration_ms: 1000-600000}; always returns to automatic"),
    (FAN_RELEASE_INTERACTION, "End a manual override: back to automatic (kernel thermal control)"),
];

pub struct HeliosLinuxHwmonFanDriver;

impl HeliosLinuxHwmonFanDriver {
    const DRIVER_ID: &str = "helios.linux.hwmon-fan";
}

fn fan_rule() -> MatchRule {
    let rule = MatchRule::new(300)
        .described("Linux hwmon fan control device")
        .require(MatchCondition::PropertyEq { key: "linux.subsystem".into(), value: Value::from("hwmon") })
        .require(MatchCondition::Kind(DeviceKind::Unspecified(InterfaceKind::Pwm)));
    match CapabilityId::new(FAN_CAPABILITY) {
        Ok(capability) => rule.require(MatchCondition::Capability(capability)),
        Err(_) => rule,
    }
}

impl Driver for HeliosLinuxHwmonFanDriver {
    fn id(&self) -> &str {
        Self::DRIVER_ID
    }

    fn interface(&self) -> InterfaceKind {
        InterfaceKind::Pwm
    }

    fn manifest_ref(&self) -> Cow<'static, DriverManifest> {
        let manifest = DriverManifest::new(self.id(), "HeliOS Linux hwmon fan driver (read-only, timed manual override)", vec![InterfaceKind::Pwm])
            // Exact: wins over Lemnos's generic hwmon fan driver, which allows raw writes.
            .with_priority(DriverPriority::Exact)
            .with_kind(DeviceKind::Unspecified(InterfaceKind::Pwm));
        let manifest = INTERACTIONS.iter().fold(manifest, |manifest, (id, summary)| manifest.with_custom_interaction(*id, *summary));
        Cow::Owned(manifest.with_rule(fan_rule()).with_tag("linux").with_tag("fan").with_tag("hwmon"))
    }

    fn matches(&self, device: &DeviceDescriptor) -> DriverMatch {
        self.manifest_ref().match_device(device).into()
    }

    fn bind(&self, device: &DeviceDescriptor, _context: &DriverBindContext<'_>) -> DriverResult<Box<dyn BoundDevice>> {
        let io = LinuxClassDeviceIo::from_device(self.id(), device)?;
        let interactions = INTERACTIONS
            .iter()
            .map(|(id, summary)| {
                CustomInteraction::new(*id, *summary).map_err(|source| DriverError::BindFailed { driver_id: self.id().to_string(), device_id: device.id.clone(), reason: source.to_string() })
            })
            .collect::<DriverResult<Vec<_>>>()?;

        let mut bound =
            HeliosLinuxHwmonFanBoundDevice { driver_id: self.id().to_string(), device: device.clone(), io, interactions, override_generation: Arc::new(AtomicU64::new(0)), overriding: false };
        // A manual mode left behind (a crash during an override) goes back to automatic.
        if bound.read_sample()?.pwm_mode == MODE_MANUAL {
            bound.io.write_u64("pwm1_enable", MODE_AUTOMATIC)?;
        }
        Ok(Box::new(bound))
    }
}

struct HeliosLinuxHwmonFanBoundDevice {
    driver_id: String,
    device: DeviceDescriptor,
    io: LinuxClassDeviceIo,
    interactions: Vec<CustomInteraction>,
    /// Bumped by every override and release; a timer only restores its own override.
    override_generation: Arc<AtomicU64>,
    overriding: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LinuxHwmonFanSample {
    pwm: u64,
    pwm_mode: u64,
    rpm: Option<u64>,
}

impl LinuxHwmonFanSample {
    fn into_value(self, name: &str, overriding: bool) -> Value {
        let mut map = ValueMap::new();
        map.insert("fan_name".into(), Value::from(name));
        map.insert("pwm".into(), Value::from(self.pwm));
        map.insert("pwm_mode".into(), Value::from(self.pwm_mode));
        map.insert("manual_override".into(), Value::from(overriding));
        if let Some(rpm) = self.rpm {
            map.insert("rpm".into(), Value::from(rpm));
        }
        Value::from(map)
    }

    fn apply_telemetry(&self, state: DeviceStateSnapshot) -> DeviceStateSnapshot {
        let mut state = state.with_telemetry("pwm", self.pwm).with_telemetry("pwm_mode", self.pwm_mode);
        if let Some(rpm) = self.rpm {
            state = state.with_telemetry("rpm", rpm);
        }
        state
    }
}

/// The override request: `{pwm, duration_ms?}`, or a bare number for `pwm`.
fn parse_override(input: Option<&Value>) -> Result<(u64, u64), String> {
    let (pwm, duration) = match input {
        Some(value) if value.as_u64().is_some() => (value.as_u64(), None),
        Some(value) => match value.as_map() {
            Some(map) => (map.get("pwm").and_then(Value::as_u64), map.get("duration_ms").map(|d| d.as_u64().ok_or("duration_ms must be a whole number of milliseconds"))),
            None => return Err("expected {\"pwm\": 0-255, \"duration_ms\": 1000-600000}".into()),
        },
        None => return Err("expected {\"pwm\": 0-255, \"duration_ms\": 1000-600000}".into()),
    };
    let pwm = pwm.ok_or("pwm (0-255) is required")?;
    if pwm > 255 {
        return Err("pwm must be between 0 and 255".into());
    }
    let duration = duration.transpose()?.unwrap_or(DEFAULT_OVERRIDE_MS);
    if !(MIN_OVERRIDE_MS..=MAX_OVERRIDE_MS).contains(&duration) {
        return Err(format!("duration_ms must be between {MIN_OVERRIDE_MS} and {MAX_OVERRIDE_MS}"));
    }
    Ok((pwm, duration))
}

impl HeliosLinuxHwmonFanBoundDevice {
    fn read_sample(&mut self) -> DriverResult<LinuxHwmonFanSample> {
        Ok(LinuxHwmonFanSample { pwm: self.io.read_u64("pwm1")?, pwm_mode: self.io.read_u64("pwm1_enable")?, rpm: self.io.read_optional_u64("fan1_input")? })
    }

    fn mode_path(&self) -> PathBuf {
        self.io.root().join("pwm1_enable")
    }

    /// Manual mode at `pwm` for `duration_ms`, then automatic again (timer thread).
    fn start_override(&mut self, pwm: u64, duration_ms: u64) -> DriverResult<LinuxHwmonFanSample> {
        let generation = self.override_generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.io.write_u64("pwm1_enable", MODE_MANUAL)?;
        if let Err(error) = self.io.write_u64("pwm1", pwm) {
            let _ = self.io.write_u64("pwm1_enable", MODE_AUTOMATIC);
            return Err(error);
        }
        self.overriding = true;
        let current = Arc::clone(&self.override_generation);
        let mode_path = self.mode_path();
        let spawned = std::thread::Builder::new().name("fan-override".into()).spawn(move || {
            std::thread::sleep(Duration::from_millis(duration_ms));
            if current.load(Ordering::SeqCst) == generation {
                let _ = std::fs::write(&mode_path, MODE_AUTOMATIC.to_string());
            }
        });
        if spawned.is_err() {
            // Without the timer the override could outlive its duration: refuse it.
            self.release()?;
            return Err(self.invalid_request(FAN_OVERRIDE_INTERACTION, "could not start the override timer"));
        }
        self.read_sample()
    }

    fn release(&mut self) -> DriverResult<LinuxHwmonFanSample> {
        self.override_generation.fetch_add(1, Ordering::SeqCst);
        self.overriding = false;
        self.io.write_u64("pwm1_enable", MODE_AUTOMATIC)?;
        self.read_sample()
    }

    /// The timer restores automatic mode on its own; notice it.
    fn refresh_override(&mut self, sample: &LinuxHwmonFanSample) {
        if self.overriding && sample.pwm_mode != MODE_MANUAL {
            self.overriding = false;
        }
    }

    fn fan_name(&self) -> &str {
        self.device.properties.get("hwmon.name").and_then(Value::as_str).or(self.device.display_name.as_deref()).unwrap_or_else(|| self.device.id.as_str())
    }

    fn state_from_sample(&self, sample: &LinuxHwmonFanSample) -> DeviceStateSnapshot {
        sample.apply_telemetry(
            DeviceStateSnapshot::new(self.device.id.clone())
                .with_lifecycle(DeviceLifecycleState::Idle)
                .with_config("label", self.fan_name().to_string())
                .with_config("linux.class_root", self.io.root().display().to_string())
                .with_config("fan_name", self.fan_name().to_string())
                .with_config("manual_override", self.overriding),
        )
    }

    fn response_for(&self, index: usize, sample: LinuxHwmonFanSample) -> InteractionResponse {
        InteractionResponse::Custom(CustomInteractionResponse::new(self.interactions[index].id.clone()).with_output(sample.into_value(self.fan_name(), self.overriding)))
    }

    fn configuration_from_sample(sample: &LinuxHwmonFanSample) -> PwmConfiguration {
        PwmConfiguration { period_ns: 255, duty_cycle_ns: sample.pwm, enabled: sample.pwm_mode != 0, polarity: PwmPolarity::Normal }
    }

    fn invalid_request(&self, request: &'static str, reason: impl Into<String>) -> DriverError {
        DriverError::InvalidRequest { driver_id: self.driver_id.clone(), device_id: self.device.id.clone(), source: CoreError::InvalidRequest { request, reason: reason.into() } }
    }

    fn read_only(&self, request: &InteractionRequest) -> DriverError {
        DriverError::UnsupportedAction {
            driver_id: self.driver_id.clone(),
            device_id: self.device.id.clone(),
            action: format!("{} (the fan is read-only: the kernel's thermal control drives it; use {FAN_OVERRIDE_INTERACTION} for a timed manual speed)", interaction_name(request)),
        }
    }
}

impl Drop for HeliosLinuxHwmonFanBoundDevice {
    fn drop(&mut self) {
        if self.overriding {
            self.override_generation.fetch_add(1, Ordering::SeqCst);
            let _ = self.io.write_u64("pwm1_enable", MODE_AUTOMATIC);
        }
    }
}

impl BoundDevice for HeliosLinuxHwmonFanBoundDevice {
    fn device(&self) -> &DeviceDescriptor {
        &self.device
    }

    fn driver_id(&self) -> &str {
        &self.driver_id
    }

    fn custom_interactions(&self) -> &[CustomInteraction] {
        &self.interactions
    }

    fn state(&mut self) -> DriverResult<Option<DeviceStateSnapshot>> {
        let sample = self.read_sample()?;
        self.refresh_override(&sample);
        let output = sample.clone().into_value(self.fan_name(), self.overriding);
        Ok(Some(self.state_from_sample(&sample).with_last_operation(OperationRecord::new(FAN_READ_INTERACTION, OperationStatus::Succeeded).with_output(output))))
    }

    fn execute(&mut self, request: &InteractionRequest) -> DriverResult<InteractionResponse> {
        match request {
            InteractionRequest::Standard(lemnos::core::StandardRequest::Pwm(PwmRequest::GetConfiguration)) => {
                let sample = self.read_sample()?;
                Ok(InteractionResponse::Standard(StandardResponse::Pwm(PwmResponse::Configuration(Self::configuration_from_sample(&sample)))))
            }
            InteractionRequest::Custom(custom) if custom.id.as_str() == FAN_READ_INTERACTION => {
                let sample = self.read_sample()?;
                self.refresh_override(&sample);
                Ok(self.response_for(0, sample))
            }
            InteractionRequest::Custom(custom) if custom.id.as_str() == FAN_OVERRIDE_INTERACTION => {
                let (pwm, duration_ms) = parse_override(custom.input.as_ref()).map_err(|reason| self.invalid_request(FAN_OVERRIDE_INTERACTION, reason))?;
                let sample = self.start_override(pwm, duration_ms)?;
                Ok(self.response_for(1, sample))
            }
            InteractionRequest::Custom(custom) if custom.id.as_str() == FAN_RELEASE_INTERACTION => {
                let sample = self.release()?;
                Ok(self.response_for(2, sample))
            }
            _ => Err(self.read_only(request)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(entries: &[(&str, Value)]) -> Value {
        let mut map = ValueMap::new();
        for (key, value) in entries {
            map.insert((*key).into(), value.clone());
        }
        Value::from(map)
    }

    #[test]
    fn override_requests_are_bounded() {
        assert_eq!(parse_override(Some(&map(&[("pwm", Value::from(200u64))]))), Ok((200, DEFAULT_OVERRIDE_MS)));
        assert_eq!(parse_override(Some(&map(&[("pwm", Value::from(10u64)), ("duration_ms", Value::from(5_000u64))]))), Ok((10, 5_000)));
        assert_eq!(parse_override(Some(&Value::from(128u64))), Ok((128, DEFAULT_OVERRIDE_MS)));
        assert!(parse_override(Some(&map(&[("pwm", Value::from(256u64))]))).is_err());
        assert!(parse_override(Some(&map(&[("pwm", Value::from(10u64)), ("duration_ms", Value::from(MAX_OVERRIDE_MS + 1))]))).is_err());
        assert!(parse_override(Some(&map(&[("pwm", Value::from(10u64)), ("duration_ms", Value::from(10u64))]))).is_err());
        assert!(parse_override(None).is_err());
    }

    #[test]
    fn manifest_matches_only_fan_hwmon_devices() {
        let manifest = HeliosLinuxHwmonFanDriver.manifest_ref().into_owned();
        assert_eq!(manifest.priority, DriverPriority::Exact);
        let rule = &manifest.rules[0];
        assert!(rule.all_of.iter().any(|c| matches!(c, MatchCondition::Capability(id) if id.as_str() == FAN_CAPABILITY)));
        assert!(rule.all_of.iter().any(|c| matches!(c, MatchCondition::Kind(DeviceKind::Unspecified(InterfaceKind::Pwm)))));
    }
}
