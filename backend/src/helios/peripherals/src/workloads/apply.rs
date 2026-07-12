use crate::model::{GpioControl, I2cControl, PwmControl, ResourceControlData, ResourceControlRequest, ResourceControlResult, ResourceDescriptor, ResourceId, ResourceKind, SpiControl};
use std::collections::BTreeMap;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ResourceControlError {
    #[error("resource '{0}' is unknown")]
    UnknownResource(ResourceId),
    #[error("resource '{resource_id}' is claimed by '{held_by}', requested owner '{owner}'")]
    ClaimConflict { resource_id: ResourceId, owner: String, held_by: String },
    #[error("resource '{resource_id}' is claimed; an owner is required")]
    ClaimRequired { resource_id: ResourceId },
    #[error("resource '{resource_id}' lease generation is required for owner '{owner}'")]
    LeaseGenerationRequired { resource_id: ResourceId, owner: String },
    #[error("resource '{resource_id}' lease generation mismatch for owner '{owner}': requested {requested}, current {current}")]
    LeaseGenerationConflict { resource_id: ResourceId, owner: String, requested: u64, current: u64 },
    #[error("resource '{resource_id}' does not support {requested} control; expected kinds: {supported:?}")]
    UnsupportedControl { resource_id: ResourceId, requested: &'static str, supported: Vec<ResourceKind> },
    #[error("resource '{resource_id}' control failed: {message}")]
    OperationFailed { resource_id: ResourceId, message: String },
}

pub trait ResourceController: Send + Sync {
    fn kind(&self) -> &'static str;
    fn supported_kinds(&self) -> &'static [ResourceKind];
    fn apply(&self, resources: &[ResourceDescriptor], resource: &ResourceDescriptor, request: &ResourceControlRequest) -> Result<Option<ResourceControlResult>, ResourceControlError>;
}

#[derive(Default)]
pub struct PeripheralManager {
    controllers: Vec<Box<dyn ResourceController>>,
}

impl PeripheralManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_controller(&mut self, controller: Box<dyn ResourceController>) -> &mut Self {
        self.controllers.push(controller);
        self
    }

    pub fn apply(&self, resources: &[ResourceDescriptor], request: &ResourceControlRequest) -> Result<ResourceControlResult, ResourceControlError> {
        let resource = resources.iter().find(|resource| resource.id == *request.resource_id()).ok_or_else(|| ResourceControlError::UnknownResource(request.resource_id().clone()))?;
        let mut supported = Vec::new();

        for controller in &self.controllers {
            supported.extend_from_slice(controller.supported_kinds());
            if let Some(result) = controller.apply(resources, resource, request)? {
                return Ok(result);
            }
        }

        supported.sort_by_key(|kind| kind.id_kind());
        supported.dedup();
        Err(ResourceControlError::UnsupportedControl { resource_id: request.resource_id().clone(), requested: request.control_kind(), supported })
    }
}

#[derive(Debug, Default)]
pub struct MemoryControlSink {
    values: BTreeMap<(ResourceId, String), ResourceControlData>,
}

impl MemoryControlSink {
    pub fn value(&self, resource_id: &ResourceId, control: &str) -> Option<&ResourceControlData> {
        self.values.get(&(resource_id.clone(), control.to_string()))
    }
}

impl ResourceController for std::sync::Mutex<MemoryControlSink> {
    fn kind(&self) -> &'static str {
        "memory"
    }

    fn supported_kinds(&self) -> &'static [ResourceKind] {
        const ALL: &[ResourceKind] = &[ResourceKind::GpioLine, ResourceKind::PwmChannel, ResourceKind::I2cDevice, ResourceKind::SpiDevice];
        ALL
    }

    fn apply(&self, _resources: &[ResourceDescriptor], resource: &ResourceDescriptor, request: &ResourceControlRequest) -> Result<Option<ResourceControlResult>, ResourceControlError> {
        let mut sink = self.lock().expect("control sink poisoned");
        match request {
            ResourceControlRequest::Gpio(request) => {
                let result = match &request.control {
                    GpioControl::Read => ResourceControlResult::read(resource.id.clone(), "gpio.read", ResourceControlData::Bool(false)),
                    GpioControl::Write { high } => {
                        sink.values.insert((resource.id.clone(), "gpio".into()), ResourceControlData::Bool(*high));
                        ResourceControlResult::applied(resource.id.clone(), "gpio.write")
                    }
                    GpioControl::ConfigureDirection { .. } => ResourceControlResult::applied(resource.id.clone(), "gpio.configure_direction"),
                };
                Ok(Some(result))
            }
            ResourceControlRequest::Pwm(request) => {
                let (value, control) = match &request.control {
                    PwmControl::Enable { enabled } => (Some(ResourceControlData::Bool(*enabled)), "pwm.enable"),
                    PwmControl::SetPeriodNs { period_ns } => (Some(ResourceControlData::Bytes(period_ns.to_le_bytes().to_vec())), "pwm.set_period_ns"),
                    PwmControl::SetDutyCycleNs { duty_cycle_ns } => (Some(ResourceControlData::Bytes(duty_cycle_ns.to_le_bytes().to_vec())), "pwm.set_duty_cycle_ns"),
                    PwmControl::Configure { enabled, .. } => (Some(ResourceControlData::Bool(*enabled)), "pwm.configure"),
                };
                if let Some(value) = value {
                    sink.values.insert((resource.id.clone(), "pwm".into()), value);
                }
                Ok(Some(ResourceControlResult::applied(resource.id.clone(), control)))
            }
            ResourceControlRequest::I2c(request) => {
                let result = match &request.control {
                    I2cControl::Read { len } => ResourceControlResult::read(resource.id.clone(), "i2c.read", ResourceControlData::Bytes(vec![0; *len])),
                    I2cControl::Write { bytes } => {
                        sink.values.insert((resource.id.clone(), "i2c".into()), ResourceControlData::Bytes(bytes.clone()));
                        ResourceControlResult::applied(resource.id.clone(), "i2c.write")
                    }
                    I2cControl::WriteRead { write, read_len } => {
                        sink.values.insert((resource.id.clone(), "i2c".into()), ResourceControlData::Bytes(write.clone()));
                        ResourceControlResult::read(resource.id.clone(), "i2c.write_read", ResourceControlData::Bytes(vec![0; *read_len]))
                    }
                };
                Ok(Some(result))
            }
            ResourceControlRequest::Spi(request) => {
                let result = match &request.control {
                    SpiControl::Transfer { bytes } => ResourceControlResult::read(resource.id.clone(), "spi.transfer", ResourceControlData::Bytes(bytes.clone())),
                    SpiControl::Write { bytes } => {
                        sink.values.insert((resource.id.clone(), "spi".into()), ResourceControlData::Bytes(bytes.clone()));
                        ResourceControlResult::applied(resource.id.clone(), "spi.write")
                    }
                };
                Ok(Some(result))
            }
        }
    }
}

impl ResourceController for std::sync::Arc<std::sync::Mutex<MemoryControlSink>> {
    fn kind(&self) -> &'static str {
        "memory"
    }
    fn supported_kinds(&self) -> &'static [ResourceKind] {
        const ALL: &[ResourceKind] = &[ResourceKind::GpioLine, ResourceKind::PwmChannel, ResourceKind::I2cDevice, ResourceKind::SpiDevice];
        ALL
    }
    fn apply(&self, resources: &[ResourceDescriptor], resource: &ResourceDescriptor, request: &ResourceControlRequest) -> Result<Option<ResourceControlResult>, ResourceControlError> {
        self.as_ref().apply(resources, resource, request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ControlStatus, GpioControlRequest, NodeId, PwmControlRequest};
    use crate::resources::ResourceBuilder;
    use std::sync::Mutex;

    #[test]
    fn manager_applies_supported_typed_control_requests() {
        let owner = NodeId::new("node1");
        let resource = ResourceBuilder::new(owner, ResourceKind::PwmChannel, "pwm0", "PWM 0").expect("resource").capability("fan", None::<String>).build();
        let request =
            ResourceControlRequest::Pwm(PwmControlRequest { resource_id: resource.id.clone(), owner: None, lease_generation: None, control: PwmControl::SetDutyCycleNs { duty_cycle_ns: 500_000 } });
        let sink = Mutex::new(MemoryControlSink::default());
        let mut manager = PeripheralManager::new();
        manager.register_controller(Box::new(sink));
        let result = manager.apply(std::slice::from_ref(&resource), &request).expect("control should apply");
        assert_eq!(result.status, ControlStatus::Applied);
    }

    #[test]
    fn manager_rejects_unknown_resources() {
        let owner = NodeId::new("node1");
        let resource = ResourceBuilder::new(owner, ResourceKind::PwmChannel, "pwm0", "PWM 0").expect("resource").build();
        let request =
            ResourceControlRequest::Pwm(PwmControlRequest { resource_id: resource.id.clone(), owner: None, lease_generation: None, control: PwmControl::SetDutyCycleNs { duty_cycle_ns: 500_000 } });
        let manager = PeripheralManager::new();
        let error = manager.apply(&[], &request).expect_err("missing resource should fail");
        assert!(matches!(error, ResourceControlError::UnknownResource(resource_id) if resource_id == *request.resource_id()));
    }

    #[test]
    fn memory_sink_returns_read_data_for_gpio() {
        let owner = NodeId::new("node1");
        let resource = ResourceBuilder::new(owner, ResourceKind::GpioLine, "gpio0", "GPIO 0").expect("resource").build();
        let request = ResourceControlRequest::Gpio(GpioControlRequest { resource_id: resource.id.clone(), owner: None, lease_generation: None, control: GpioControl::Read });
        let sink = Mutex::new(MemoryControlSink::default());
        let mut manager = PeripheralManager::new();
        manager.register_controller(Box::new(sink));
        let result = manager.apply(std::slice::from_ref(&resource), &request).expect("control should apply");
        assert_eq!(result.status, ControlStatus::Read);
        assert_eq!(result.data, Some(ResourceControlData::Bool(false)));
    }
}
