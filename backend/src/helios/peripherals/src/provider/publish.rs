use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use orion::{
    client::{ClientError, DerivedResource, LocalNodeRuntime, LocalRuntimePublisher, ProviderResource},
    control_plane::{
        AvailabilityState, ExecutorRecord, HealthState, LeaseRecord, LeaseState, ProviderRecord, ResourceActionResult, ResourceActionStatus, ResourceCapability, ResourceOwnershipMode, ResourceRecord,
        ResourceState, TypedConfigValue,
    },
    core::{CapabilityId, ExecutorId, NodeId, ProviderId, ResourceId},
};

use crate::{
    config::PeripheralConfig,
    model::{ControlStatus, ResourceControlData, ResourceControlResult, ResourceDescriptor, ResourceKind, ResourceStatus},
    provider::streams::stream_mjpeg_socket_path,
    resources::DiscoverySnapshot,
};

const DEFAULT_PROVIDER_CLIENT_NAME: &str = "helios-peripherals";
const DEFAULT_EXECUTOR_CLIENT_NAME: &str = "helios-peripherals-executor";
const DISPLAY_NAME_LABEL: &str = "helios.display_name";
const KIND_LABEL: &str = "helios.kind";
const LABEL_PREFIX: &str = "helios.label.";
const LINK_PREFIX: &str = "helios.link.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceActionFeedback {
    pub state: ResourceState,
}

impl ResourceActionFeedback {
    pub fn applied(observed_at_ms: u64, result: &ResourceControlResult) -> Self {
        Self {
            state: ResourceState::new(observed_at_ms).with_action_result(ResourceActionResult {
                action_kind: result.control.as_ref().to_string(),
                status: match result.status {
                    ControlStatus::Applied => ResourceActionStatus::Applied,
                    ControlStatus::Read => ResourceActionStatus::Read,
                },
                data: result.data.as_ref().map(control_data_value),
                error: None,
            }),
        }
    }

    pub fn failed(observed_at_ms: u64, action_kind: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            state: ResourceState::new(observed_at_ms).with_action_result(ResourceActionResult {
                action_kind: action_kind.into(),
                status: ResourceActionStatus::Failed,
                data: None,
                error: Some(error.into()),
            }),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OrionPublishError {
    #[error(transparent)]
    Client(#[from] ClientError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrionPeripheralPublisher {
    client_name: String,
    executor_client_name: String,
    provider_id: String,
    executor_id: String,
    node_id: String,
    stream_dir: PathBuf,
    node_runtime: LocalNodeRuntime,
}

impl OrionPeripheralPublisher {
    pub fn from_config(config: &PeripheralConfig) -> Self {
        Self::new(DEFAULT_PROVIDER_CLIENT_NAME, &config.node_id)
            .with_stream_dir(config.stream_dir.clone())
            .with_node_runtime(LocalNodeRuntime::new(config.orion_ipc_socket_path.clone(), config.orion_ipc_stream_socket_path.clone()))
    }

    pub fn new(client_name: impl Into<String>, node_id: impl Into<String>) -> Self {
        let node_id = node_id.into();
        Self {
            client_name: client_name.into(),
            executor_client_name: DEFAULT_EXECUTOR_CLIENT_NAME.to_string(),
            provider_id: provider_id_for(&node_id),
            executor_id: executor_id_for(&node_id),
            node_id,
            stream_dir: std::env::temp_dir().join("helios-peripherals").join("streams"),
            node_runtime: LocalNodeRuntime::new(PathBuf::from("/run/orion/control.sock"), PathBuf::from("/run/orion/control-stream.sock")),
        }
    }

    pub fn with_stream_dir(mut self, stream_dir: impl Into<PathBuf>) -> Self {
        self.stream_dir = stream_dir.into();
        self
    }

    pub fn with_node_runtime(mut self, node_runtime: LocalNodeRuntime) -> Self {
        self.node_runtime = node_runtime;
        self
    }

    pub fn client_name(&self) -> &str {
        &self.client_name
    }

    pub fn provider_identity_record(&self) -> ProviderRecord {
        ProviderRecord::builder(ProviderId::new(self.provider_id.clone()), NodeId::new(self.node_id.clone())).build()
    }

    pub fn executor_identity_record(&self) -> ExecutorRecord {
        ExecutorRecord::builder(ExecutorId::new(self.executor_id.clone()), NodeId::new(self.node_id.clone())).runtime_type("helios.peripheral.resource_action.v1").build()
    }

    pub async fn register_identities(&self) -> Result<(), OrionPublishError> {
        self.registration_publisher().register_all().await?;
        Ok(())
    }

    pub fn provider_record(&self, snapshot: &DiscoverySnapshot) -> ProviderRecord {
        let mut builder = ProviderRecord::builder(ProviderId::new(self.provider_id.clone()), NodeId::new(self.node_id.clone()));
        for resource_type in provider_resource_types(snapshot) {
            builder = builder.resource_type(resource_type);
        }
        builder.build()
    }

    pub fn derived_channel_local_name(&self, resource: &ResourceDescriptor) -> Option<String> {
        (resource.kind == ResourceKind::CaptureDevice).then(|| format!("{}.raw", resource.id.as_str()))
    }

    pub fn derived_channel_path(&self, resource: &ResourceDescriptor) -> Option<PathBuf> {
        let local_name = self.derived_channel_local_name(resource)?;
        Some(self.stream_dir.join(format!("{}:none:{}.stream.json", resource.owner, local_name)))
    }

    pub fn derived_channel_resource_id(&self, resource: &ResourceDescriptor) -> Option<String> {
        (resource.kind == ResourceKind::CaptureDevice).then(|| format!("stream.channel.{}.raw", resource.id.as_str()))
    }

    pub fn resource_records(&self, snapshot: &DiscoverySnapshot, leases: &[LeaseRecord], feedback: &BTreeMap<String, ResourceActionFeedback>) -> Vec<ResourceRecord> {
        let lease_states = lease_state_map(leases);
        let mut records =
            snapshot.resources.iter().map(|resource| self.resource_record(resource, lease_states.get(resource.id.as_str()).copied(), feedback.get(resource.id.as_str()))).collect::<Vec<_>>();
        for resource in &snapshot.resources {
            if let Some(channel) = self.derived_channel_record(resource) {
                records.push(channel);
            }
        }
        records
    }

    pub fn resource_record(&self, resource: &ResourceDescriptor, lease_state: Option<LeaseState>, feedback: Option<&ResourceActionFeedback>) -> ResourceRecord {
        let mut builder = ProviderResource::new(resource.id.clone(), resource_type_for(resource.kind), ProviderId::new(self.provider_id.clone()))
            .ownership_mode(ownership_mode_for(resource.kind))
            .health(health_for(resource.status))
            .availability(availability_for(resource.status))
            .lease_state(lease_state.unwrap_or(LeaseState::Unleased))
            .label(format!("{DISPLAY_NAME_LABEL}={}", resource.display_name))
            .label(format!("{KIND_LABEL}={}", resource.kind.id_kind()));

        for capability in &resource.capabilities {
            let mut translated = ResourceCapability::new(CapabilityId::new(capability.name.to_string()));
            if let Some(detail) = &capability.detail {
                translated = translated.with_detail(detail.to_string());
            }
            builder = builder.capability(translated);
        }

        for (key, value) in &resource.labels {
            builder = builder.label(format!("{LABEL_PREFIX}{key}={value}"));
        }

        for endpoint in &resource.endpoints {
            builder = builder.endpoint(endpoint_value(endpoint.protocol.as_ref(), endpoint.address.as_ref()));
        }

        for link in &resource.links {
            builder = builder.label(format!("{LINK_PREFIX}{}={}", link.relation, link.target));
        }

        if let Some(feedback) = feedback {
            builder = builder.state(feedback.state.clone());
        }

        builder.build()
    }

    fn derived_channel_record(&self, resource: &ResourceDescriptor) -> Option<ResourceRecord> {
        let stream_path = self.derived_channel_path(resource)?;
        let channel_id = self.derived_channel_resource_id(resource)?;
        let frame_socket_path = crate::provider::streams::stream_socket_path(&stream_path);
        let preview_socket_path = stream_mjpeg_socket_path(&stream_path);
        Some(
            DerivedResource::new(ResourceId::new(channel_id), "stream.channel", ProviderId::new(self.provider_id.clone()))
                .source_resource(resource.id.clone())
                .ownership_mode(ResourceOwnershipMode::SharedRead)
                .health(health_for(resource.status))
                .availability(availability_for(resource.status))
                .lease_state(LeaseState::Unleased)
                .label(format!("{DISPLAY_NAME_LABEL}={} Raw Stream", resource.display_name))
                .label(format!("{KIND_LABEL}=channel"))
                .label("helios.channel.kind=raw_capture")
                .endpoint(endpoint_value("styx-frame-lease+unix", &frame_socket_path.display().to_string()))
                .endpoint(endpoint_value("shm", &stream_path.display().to_string()))
                .endpoint(endpoint_value("mjpeg+unix", &preview_socket_path.display().to_string()))
                .build(),
        )
    }

    pub async fn publish_snapshot_with_feedback(&self, snapshot: &DiscoverySnapshot, leases: &[LeaseRecord], feedback: &BTreeMap<String, ResourceActionFeedback>) -> Result<(), OrionPublishError> {
        self.snapshot_publisher(snapshot).publish_provider_resources(self.resource_records(snapshot, leases, feedback)).await?;
        Ok(())
    }

    fn registration_publisher(&self) -> LocalRuntimePublisher {
        LocalRuntimePublisher::builder(self.node_runtime.clone(), self.client_name.clone()).provider(self.provider_identity_record()).executor(self.executor_identity_record()).build()
    }

    fn snapshot_publisher(&self, snapshot: &DiscoverySnapshot) -> LocalRuntimePublisher {
        LocalRuntimePublisher::builder(self.node_runtime.clone(), self.client_name.clone()).provider(self.provider_record(snapshot)).executor(self.executor_identity_record()).build()
    }
}

fn control_data_value(data: &ResourceControlData) -> TypedConfigValue {
    match data {
        ResourceControlData::Bool(value) => TypedConfigValue::Bool(*value),
        ResourceControlData::Bytes(bytes) => TypedConfigValue::Bytes(bytes.clone()),
    }
}

fn provider_id_for(node_id: &str) -> String {
    format!("provider.peripherals.{node_id}")
}

fn executor_id_for(node_id: &str) -> String {
    format!("executor.peripherals.{node_id}")
}

fn provider_resource_types(snapshot: &DiscoverySnapshot) -> BTreeSet<&'static str> {
    snapshot.resources.iter().map(|resource| resource_type_for(resource.kind)).collect()
}

fn resource_type_for(kind: ResourceKind) -> &'static str {
    match kind {
        ResourceKind::GpioChip => "gpio.chip",
        ResourceKind::GpioLine => "gpio.line",
        ResourceKind::PwmChip => "pwm.chip",
        ResourceKind::PwmChannel => "pwm.channel",
        ResourceKind::I2cBus => "i2c.bus",
        ResourceKind::I2cDevice => "i2c.device",
        ResourceKind::SpiBus => "spi.bus",
        ResourceKind::SpiDevice => "spi.device",
        ResourceKind::UsbBus => "usb.bus",
        ResourceKind::UsbDevice => "usb.device",
        ResourceKind::UsbInterface => "usb.interface",
        ResourceKind::CaptureDevice => "camera.device",
        ResourceKind::Virtual => "virtual.resource",
        ResourceKind::Channel => "stream.channel",
    }
}

fn ownership_mode_for(kind: ResourceKind) -> ResourceOwnershipMode {
    match kind {
        ResourceKind::CaptureDevice => ResourceOwnershipMode::ExclusiveOwnerPublishesDerived,
        ResourceKind::Channel => ResourceOwnershipMode::SharedRead,
        _ => ResourceOwnershipMode::Exclusive,
    }
}

fn health_for(status: ResourceStatus) -> HealthState {
    match status {
        ResourceStatus::Available => HealthState::Healthy,
        ResourceStatus::Degraded => HealthState::Degraded,
        ResourceStatus::Missing => HealthState::Failed,
    }
}

fn availability_for(status: ResourceStatus) -> AvailabilityState {
    match status {
        ResourceStatus::Available | ResourceStatus::Degraded => AvailabilityState::Available,
        ResourceStatus::Missing => AvailabilityState::Unavailable,
    }
}

fn endpoint_value(protocol: &str, address: &str) -> String {
    format!("{protocol}://{address}")
}

fn lease_state_map(leases: &[LeaseRecord]) -> BTreeMap<&str, LeaseState> {
    leases.iter().map(|lease| (lease.resource_id.as_str(), lease.lease_state)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::NodeId,
        resources::{DiscoverySnapshot, ResourceBuilder},
    };

    #[test]
    fn provider_record_collects_distinct_resource_types_from_snapshot() {
        let owner = NodeId::new("node1");
        let gpio = ResourceBuilder::new(owner.clone(), ResourceKind::GpioLine, "gpio17", "GPIO 17").expect("gpio").build();
        let capture = ResourceBuilder::new(owner, ResourceKind::CaptureDevice, "cam0", "Camera 0").expect("camera").build();
        let snapshot = DiscoverySnapshot::new(vec![gpio, capture]);
        let publisher = OrionPeripheralPublisher::new("client", "node1");
        let provider = publisher.provider_record(&snapshot);
        assert!(provider.resource_types.iter().any(|ty| ty.as_str() == "gpio.line"));
        assert!(provider.resource_types.iter().any(|ty| ty.as_str() == "camera.device"));
    }

    #[test]
    fn capture_devices_publish_derived_stream_channel_resources() {
        let owner = NodeId::new("node1");
        let resource = ResourceBuilder::new(owner, ResourceKind::CaptureDevice, "cam0", "Front Camera").expect("camera").build();
        let publisher = OrionPeripheralPublisher::new("client", "node1");
        let derived = publisher.derived_channel_record(&resource).expect("channel");
        assert_eq!(derived.resource_type.as_str(), "stream.channel");
        assert!(derived.endpoints.iter().any(|endpoint| endpoint.starts_with("styx-frame-lease+unix://")));
        assert!(derived.endpoints.iter().any(|endpoint| endpoint.starts_with("shm://")));
    }

    #[test]
    fn resource_record_translation_preserves_hardware_facts() {
        let owner = NodeId::new("node1");
        let resource = ResourceBuilder::new(owner, ResourceKind::GpioLine, "gpio17", "GPIO 17").expect("gpio").label("gpiochip", "gpiochip0").endpoint("sysfs", "/sys/class/gpio/gpio17").build();
        let publisher = OrionPeripheralPublisher::new("client", "node1");
        let record = publisher.resource_record(&resource, Some(LeaseState::Leased), None);
        assert!(record.labels.iter().any(|label| label == "helios.label.gpiochip=gpiochip0"));
        assert!(record.endpoints.iter().any(|endpoint| endpoint == "sysfs:///sys/class/gpio/gpio17"));
    }

    #[test]
    fn lease_overlay_updates_translated_resource_lease_state() {
        let owner = NodeId::new("node1");
        let resource = ResourceBuilder::new(owner, ResourceKind::GpioLine, "gpio17", "GPIO 17").expect("gpio").build();
        let lease = LeaseRecord::builder(resource.id.clone()).lease_state(LeaseState::Leased).build();
        let publisher = OrionPeripheralPublisher::new("client", "node1");
        let records = publisher.resource_records(&DiscoverySnapshot::new(vec![resource]), &[lease], &BTreeMap::new());
        assert!(records.iter().any(|record| record.lease_state == LeaseState::Leased));
    }

    #[test]
    fn resource_action_feedback_sets_typed_resource_state() {
        let owner = NodeId::new("node1");
        let resource = ResourceBuilder::new(owner, ResourceKind::GpioLine, "gpio17", "GPIO 17").expect("gpio").build();
        let feedback = ResourceActionFeedback::failed(42, "gpio.write", "write failed");
        let publisher = OrionPeripheralPublisher::new("client", "node1");
        let record = publisher.resource_record(&resource, None, Some(&feedback));
        let result = record.state.and_then(|state| state.action_result).expect("action result");
        assert_eq!(result.action_kind, "gpio.write");
        assert_eq!(result.status, ResourceActionStatus::Failed);
    }

    #[test]
    fn missing_resources_translate_to_unavailable_failed_state() {
        let owner = NodeId::new("node1");
        let resource = ResourceBuilder::new(owner, ResourceKind::SpiDevice, "imu0", "IMU").expect("spi device").status(ResourceStatus::Missing).build();
        let publisher = OrionPeripheralPublisher::new("client", "node1");
        let record = publisher.resource_record(&resource, None, None);
        assert_eq!(record.availability, AvailabilityState::Unavailable);
        assert_eq!(record.health, HealthState::Failed);
    }
}
