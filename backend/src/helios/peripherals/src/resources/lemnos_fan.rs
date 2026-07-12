use std::borrow::Cow;

use lemnos::core::{
    CoreError, CustomInteractionResponse, DeviceDescriptor, DeviceId, DeviceKind, DeviceLifecycleState, DeviceStateSnapshot, InteractionRequest, InteractionResponse, InterfaceKind, OperationRecord,
    OperationStatus, PwmConfiguration, PwmPolarity, PwmRequest, PwmResponse, StandardResponse, Value, ValueMap,
};
use lemnos::driver::{
    BoundDevice, CustomInteraction, Driver, DriverBindContext, DriverError, DriverManifest, DriverMatch, DriverPriority, DriverResult, LinuxClassDeviceIo, MatchCondition, MatchRule, interaction_name,
};

pub const FAN_READ_INTERACTION: &str = "fan.read";
pub const FAN_SET_PWM_INTERACTION: &str = "fan.set_pwm";
pub const FAN_SET_MODE_INTERACTION: &str = "fan.set_mode";

pub struct HeliosLinuxHwmonFanDriver;

impl HeliosLinuxHwmonFanDriver {
    const DRIVER_ID: &str = "helios.linux.hwmon-fan";
}

impl Driver for HeliosLinuxHwmonFanDriver {
    fn id(&self) -> &str {
        Self::DRIVER_ID
    }

    fn interface(&self) -> InterfaceKind {
        InterfaceKind::Pwm
    }

    fn manifest_ref(&self) -> Cow<'static, DriverManifest> {
        Cow::Owned(
            DriverManifest::new(self.id(), "HeliOS Linux hwmon fan driver", vec![InterfaceKind::Pwm])
                .with_priority(DriverPriority::Preferred)
                .with_kind(DeviceKind::Unspecified(InterfaceKind::Pwm))
                .with_custom_interaction(FAN_READ_INTERACTION, "Read fan hwmon state")
                .with_custom_interaction(FAN_SET_PWM_INTERACTION, "Set raw pwm1 value from 0 to 255")
                .with_custom_interaction(FAN_SET_MODE_INTERACTION, "Set pwm1_enable mode from 0 to 3")
                .with_rule(MatchRule::new(200).described("Linux hwmon fan control device").require(MatchCondition::PropertyEq { key: "linux.subsystem".into(), value: Value::from("hwmon") }))
                .with_tag("linux")
                .with_tag("fan")
                .with_tag("hwmon"),
        )
    }

    fn matches(&self, device: &DeviceDescriptor) -> DriverMatch {
        self.manifest_ref().match_device(device).into()
    }

    fn bind(&self, device: &DeviceDescriptor, _context: &DriverBindContext<'_>) -> DriverResult<Box<dyn BoundDevice>> {
        let io = LinuxClassDeviceIo::from_device(self.id(), device)?;
        let interactions =
            [(FAN_READ_INTERACTION, "Read fan hwmon state"), (FAN_SET_PWM_INTERACTION, "Set raw pwm1 value from 0 to 255"), (FAN_SET_MODE_INTERACTION, "Set pwm1_enable mode from 0 to 3")]
                .into_iter()
                .map(|(id, summary)| {
                    CustomInteraction::new(id, summary).map_err(|source| DriverError::BindFailed { driver_id: self.id().to_string(), device_id: device.id.clone(), reason: source.to_string() })
                })
                .collect::<DriverResult<Vec<_>>>()?;

        let mut bound = HeliosLinuxHwmonFanBoundDevice { driver_id: self.id().to_string(), device: device.clone(), io, interactions };
        bound.read_sample()?;
        Ok(Box::new(bound))
    }
}

struct HeliosLinuxHwmonFanBoundDevice {
    driver_id: String,
    device: DeviceDescriptor,
    io: LinuxClassDeviceIo,
    interactions: Vec<CustomInteraction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LinuxHwmonFanSample {
    pwm: u64,
    pwm_mode: u64,
    rpm: Option<u64>,
}

impl LinuxHwmonFanSample {
    fn into_value(self, name: &str) -> Value {
        let mut map = ValueMap::new();
        map.insert("fan_name".into(), Value::from(name));
        map.insert("pwm".into(), Value::from(self.pwm));
        map.insert("pwm_mode".into(), Value::from(self.pwm_mode));
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

impl HeliosLinuxHwmonFanBoundDevice {
    fn read_sample(&mut self) -> DriverResult<LinuxHwmonFanSample> {
        Ok(LinuxHwmonFanSample { pwm: self.io.read_u64("pwm1")?, pwm_mode: self.io.read_u64("pwm1_enable")?, rpm: self.io.read_optional_u64("fan1_input")? })
    }

    fn set_pwm(&mut self, pwm: u64) -> DriverResult<LinuxHwmonFanSample> {
        if pwm > 255 {
            return Err(self.invalid_request(FAN_SET_PWM_INTERACTION, "PWM must be between 0 and 255"));
        }
        self.io.write_u64("pwm1", pwm)?;
        self.read_sample()
    }

    fn set_mode(&mut self, mode: u64) -> DriverResult<LinuxHwmonFanSample> {
        if !matches!(mode, 0..=3) {
            return Err(self.invalid_request(FAN_SET_MODE_INTERACTION, "pwm1_enable must be between 0 and 3"));
        }
        self.io.write_u64("pwm1_enable", mode)?;
        self.read_sample()
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
                .with_config("fan_name", self.fan_name().to_string()),
        )
    }

    fn response_for(&self, interaction_id: lemnos::core::InteractionId, sample: LinuxHwmonFanSample) -> InteractionResponse {
        InteractionResponse::Custom(CustomInteractionResponse::new(interaction_id).with_output(sample.into_value(self.fan_name())))
    }

    fn standard_response(&mut self, response: PwmResponse) -> DriverResult<InteractionResponse> {
        Ok(InteractionResponse::Standard(StandardResponse::Pwm(response)))
    }

    fn configuration_from_sample(sample: &LinuxHwmonFanSample) -> PwmConfiguration {
        PwmConfiguration { period_ns: 255, duty_cycle_ns: sample.pwm, enabled: sample.pwm_mode != 0, polarity: PwmPolarity::Normal }
    }

    fn raw_pwm_from_config(&self, configuration: &PwmConfiguration) -> DriverResult<u64> {
        if configuration.period_ns == 0 {
            return Err(self.invalid_request("pwm.configure", "period_ns must be greater than 0"));
        }
        Ok(configuration.duty_cycle_ns.saturating_mul(255).checked_div(configuration.period_ns).unwrap_or(0).min(255))
    }

    fn invalid_request(&self, request: &'static str, reason: impl Into<String>) -> DriverError {
        DriverError::InvalidRequest { driver_id: self.driver_id.clone(), device_id: self.device.id.clone(), source: CoreError::InvalidRequest { request, reason: reason.into() } }
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
        let output = sample.clone().into_value(self.fan_name());
        Ok(Some(self.state_from_sample(&sample).with_last_operation(OperationRecord::new(FAN_READ_INTERACTION, OperationStatus::Succeeded).with_output(output))))
    }

    fn execute(&mut self, request: &InteractionRequest) -> DriverResult<InteractionResponse> {
        match request {
            InteractionRequest::Standard(lemnos::core::StandardRequest::Pwm(PwmRequest::Enable { enabled })) => {
                self.set_mode(u64::from(*enabled))?;
                self.standard_response(PwmResponse::Applied)
            }
            InteractionRequest::Standard(lemnos::core::StandardRequest::Pwm(PwmRequest::Configure(configuration))) => {
                let pwm = self.raw_pwm_from_config(configuration)?;
                self.set_mode(u64::from(configuration.enabled))?;
                self.set_pwm(pwm)?;
                self.standard_response(PwmResponse::Applied)
            }
            InteractionRequest::Standard(lemnos::core::StandardRequest::Pwm(PwmRequest::SetDutyCycle { duty_cycle_ns })) => {
                self.set_pwm((*duty_cycle_ns).min(255))?;
                self.standard_response(PwmResponse::Applied)
            }
            InteractionRequest::Standard(lemnos::core::StandardRequest::Pwm(PwmRequest::SetPeriod { .. })) => self.standard_response(PwmResponse::Applied),
            InteractionRequest::Standard(lemnos::core::StandardRequest::Pwm(PwmRequest::GetConfiguration)) => {
                let sample = self.read_sample()?;
                self.standard_response(PwmResponse::Configuration(Self::configuration_from_sample(&sample)))
            }
            InteractionRequest::Custom(request) if request.id.as_str() == FAN_READ_INTERACTION => {
                let sample = self.read_sample()?;
                Ok(self.response_for(self.interactions[0].id.clone(), sample))
            }
            InteractionRequest::Custom(request) if request.id.as_str() == FAN_SET_PWM_INTERACTION => {
                let pwm = require_u64_input(&self.driver_id, &self.device.id, request.input.as_ref(), FAN_SET_PWM_INTERACTION)?;
                let sample = self.set_pwm(pwm)?;
                Ok(self.response_for(self.interactions[1].id.clone(), sample))
            }
            InteractionRequest::Custom(request) if request.id.as_str() == FAN_SET_MODE_INTERACTION => {
                let mode = require_u64_input(&self.driver_id, &self.device.id, request.input.as_ref(), FAN_SET_MODE_INTERACTION)?;
                let sample = self.set_mode(mode)?;
                Ok(self.response_for(self.interactions[2].id.clone(), sample))
            }
            _ => Err(DriverError::UnsupportedAction { driver_id: self.driver_id.clone(), device_id: self.device.id.clone(), action: interaction_name(request).into_owned() }),
        }
    }
}

fn require_u64_input(driver_id: &str, device_id: &DeviceId, input: Option<&Value>, interaction: &'static str) -> DriverResult<u64> {
    input.and_then(Value::as_u64).ok_or_else(|| DriverError::InvalidRequest {
        driver_id: driver_id.to_string(),
        device_id: device_id.clone(),
        source: CoreError::InvalidRequest { request: interaction, reason: "expected a u64 custom input".into() },
    })
}
