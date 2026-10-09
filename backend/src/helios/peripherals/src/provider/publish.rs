use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use orion::{
    client::{ClientError, LocalNodeRuntime, LocalRuntimePublisher, ProviderResource},
    control_plane::{
        AvailabilityState, CustomEndpointScheme, ExecutorRecord, HealthState, LeaseRecord, LeaseState, ProviderRecord, ResourceActionResult, ResourceActionStatus, ResourceCapability,
        ResourceConfigState, ResourceOwnershipMode, ResourceRecord, ResourceState, TypedConfigValue,
    },
    core::{CapabilityId, ExecutorId, NodeId, ProviderId},
};

use crate::{
    config::PeripheralConfig,
    model::{ObservedValue, ResourceActionOutcome, ResourceDescriptor, ResourceKind, ResourceObservation, ResourceStatus},
    provider::camera_service::{StyxFramesEndpoint, camera_service_socket_path, serves_camera_frames},
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
    pub fn applied(observed_at_ms: u64, outcome: &ResourceActionOutcome) -> Self {
        Self {
            state: ResourceState::new(observed_at_ms).with_action_result(ResourceActionResult {
                action_kind: outcome.action_kind.as_ref().to_string(),
                status: ResourceActionStatus::Applied,
                data: outcome.value.map(TypedConfigValue::F64),
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

    pub fn resource_records(&self, snapshot: &DiscoverySnapshot, leases: &[LeaseRecord], feedback: &BTreeMap<String, ResourceActionFeedback>) -> Vec<ResourceRecord> {
        let lease_states = lease_state_map(leases);
        snapshot.resources.iter().map(|resource| self.resource_record(resource, lease_states.get(resource.id.as_str()).copied(), feedback.get(resource.id.as_str()))).collect()
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

        if serves_camera_frames(resource) {
            builder = builder.endpoint(StyxFramesEndpoint::endpoint_string(camera_service_socket_path(&self.stream_dir, resource).display()));
        }

        for link in &resource.links {
            builder = builder.label(format!("{LINK_PREFIX}{}={}", link.relation, link.target));
        }

        if let Some(state) = resource_state(resource.observation.as_ref(), feedback) {
            builder = builder.state(state);
        }

        builder.build()
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

/// The resource's state: its observation (readings) as config values, and the last action's
/// result.
fn resource_state(observation: Option<&ResourceObservation>, feedback: Option<&ResourceActionFeedback>) -> Option<ResourceState> {
    let mut state = match (observation, feedback) {
        (None, None) => return None,
        (_, Some(feedback)) => feedback.state.clone(),
        (Some(observation), None) => ResourceState::new(observation.observed_at_ms),
    };
    if let Some(observation) = observation {
        state.observed_at_ms = state.observed_at_ms.max(observation.observed_at_ms);
        let payload = observation.values.iter().map(|(key, value)| (key.clone(), observed_value(value))).collect();
        state = state.with_config(ResourceConfigState { payload });
    }
    Some(state)
}

fn observed_value(value: &ObservedValue) -> TypedConfigValue {
    match value {
        ObservedValue::Bool(value) => TypedConfigValue::Bool(*value),
        ObservedValue::UInt(value) => TypedConfigValue::UInt(*value),
        ObservedValue::F64(value) => TypedConfigValue::F64(*value),
        ObservedValue::String(value) => TypedConfigValue::String(value.clone()),
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
        ResourceKind::CaptureDevice => "camera.device",
        ResourceKind::LemnosDevice => "lemnos.device",
        ResourceKind::Virtual => "virtual.resource",
    }
}

fn ownership_mode_for(kind: ResourceKind) -> ResourceOwnershipMode {
    match kind {
        ResourceKind::CaptureDevice => ResourceOwnershipMode::SharedRead,
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
        let imu = ResourceBuilder::new(owner.clone(), ResourceKind::LemnosDevice, "imu", "IMU").expect("imu").build();
        let capture = ResourceBuilder::new(owner, ResourceKind::CaptureDevice, "cam0", "Camera 0").expect("camera").build();
        let snapshot = DiscoverySnapshot::new(vec![imu, capture]);
        let publisher = OrionPeripheralPublisher::new("client", "node1");
        let provider = publisher.provider_record(&snapshot);
        assert!(provider.resource_types.iter().any(|ty| ty.as_str() == "lemnos.device"));
        assert!(provider.resource_types.iter().any(|ty| ty.as_str() == "camera.device"));
    }

    #[test]
    fn camera_resources_advertise_their_styx_frames_endpoint() {
        let owner = NodeId::new("node1");
        let camera = ResourceBuilder::new(owner.clone(), ResourceKind::CaptureDevice, "cam0", "Front Camera").expect("camera").build();
        let imu = ResourceBuilder::new(owner, ResourceKind::LemnosDevice, "imu", "IMU").expect("imu").build();
        let publisher = OrionPeripheralPublisher::new("client", "node1").with_stream_dir("/run/helios/streams");
        let records = publisher.resource_records(&DiscoverySnapshot::new(vec![camera, imu]), &[], &BTreeMap::new());
        assert_eq!(records.len(), 2, "cameras no longer publish a derived stream channel");

        let camera = records.iter().find(|record| record.resource_type.as_str() == "camera.device").expect("camera record");
        assert!(camera.endpoints.iter().any(|endpoint| endpoint == "styx-frames+unix:///run/helios/streams/capture_device_node1_cam0.styx.sock"));
        let endpoint = camera.endpoint::<StyxFramesEndpoint>().expect("typed styx frames endpoint");
        assert_eq!(endpoint.socket_path, PathBuf::from("/run/helios/streams/capture_device_node1_cam0.styx.sock"));

        let imu = records.iter().find(|record| record.resource_type.as_str() == "lemnos.device").expect("lemnos record");
        assert!(imu.endpoints.iter().all(|endpoint| !endpoint.starts_with("styx-frames+unix://")));
    }

    #[test]
    fn resource_record_translation_preserves_hardware_facts() {
        let owner = NodeId::new("node1");
        let resource = ResourceBuilder::new(owner, ResourceKind::LemnosDevice, "imu", "IMU").expect("imu").label("lemnos.class", "imu").endpoint("lemnos+unix", "/run/lemnos/lemnosd.sock").build();
        let publisher = OrionPeripheralPublisher::new("client", "node1");
        let record = publisher.resource_record(&resource, Some(LeaseState::Leased), None);
        assert!(record.labels.iter().any(|label| label == "helios.label.lemnos.class=imu"));
        assert!(record.endpoints.iter().any(|endpoint| endpoint == "lemnos+unix:///run/lemnos/lemnosd.sock"));
    }

    #[test]
    fn lease_overlay_updates_translated_resource_lease_state() {
        let owner = NodeId::new("node1");
        let resource = ResourceBuilder::new(owner, ResourceKind::LemnosDevice, "imu", "IMU").expect("imu").build();
        let lease = LeaseRecord::builder(resource.id.clone()).lease_state(LeaseState::Leased).build();
        let publisher = OrionPeripheralPublisher::new("client", "node1");
        let records = publisher.resource_records(&DiscoverySnapshot::new(vec![resource]), &[lease], &BTreeMap::new());
        assert!(records.iter().any(|record| record.lease_state == LeaseState::Leased));
    }

    #[test]
    fn resource_action_feedback_sets_typed_resource_state() {
        let owner = NodeId::new("node1");
        let resource = ResourceBuilder::new(owner, ResourceKind::LemnosDevice, "imu", "IMU").expect("imu").build();
        let feedback = ResourceActionFeedback::failed(42, "fan.override", "lemnosd refused: not allowed");
        let publisher = OrionPeripheralPublisher::new("client", "node1");
        let record = publisher.resource_record(&resource, None, Some(&feedback));
        let result = record.state.and_then(|state| state.action_result).expect("action result");
        assert_eq!(result.action_kind, "fan.override");
        assert_eq!(result.status, ResourceActionStatus::Failed);
    }

    #[test]
    fn missing_resources_translate_to_unavailable_failed_state() {
        let owner = NodeId::new("node1");
        let resource = ResourceBuilder::new(owner, ResourceKind::LemnosDevice, "imu", "IMU").expect("imu").status(ResourceStatus::Missing).build();
        let publisher = OrionPeripheralPublisher::new("client", "node1");
        let record = publisher.resource_record(&resource, None, None);
        assert_eq!(record.availability, AvailabilityState::Unavailable);
        assert_eq!(record.health, HealthState::Failed);
    }

    #[test]
    fn observations_become_typed_resource_state_next_to_the_action_result() {
        let owner = NodeId::new("node1");
        let mut values = BTreeMap::new();
        values.insert("reading.duty".to_string(), ObservedValue::F64(0.7));
        values.insert("fan.override.active".to_string(), ObservedValue::Bool(true));
        let resource = ResourceBuilder::new(owner, ResourceKind::LemnosDevice, "fan", "Fan").expect("fan").observation(ResourceObservation { observed_at_ms: 100, values }).build();
        let publisher = OrionPeripheralPublisher::new("client", "node1");

        let state = publisher.resource_record(&resource, None, None).state.expect("state");
        assert_eq!(state.observed_at_ms, 100);
        let config = state.config.expect("config");
        assert_eq!(config.payload.get("reading.duty"), Some(&TypedConfigValue::F64(0.7)));
        assert_eq!(config.payload.get("fan.override.active"), Some(&TypedConfigValue::Bool(true)));

        let outcome = ResourceActionOutcome::applied(resource.id.clone(), "fan.override", Some(1.0));
        let feedback = ResourceActionFeedback::applied(50, &outcome);
        let state = publisher.resource_record(&resource, None, Some(&feedback)).state.expect("state");
        assert_eq!(state.observed_at_ms, 100);
        let action = state.action_result.expect("action result");
        assert_eq!(action.status, ResourceActionStatus::Applied);
        assert_eq!(action.data, Some(TypedConfigValue::F64(1.0)));
        assert!(state.config.is_some_and(|config| config.payload.contains_key("reading.duty")));
    }
}
