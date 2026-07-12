use orion::{
    client::{ClientError, DerivedResource, LocalNodeRuntime, LocalRuntimePublisher, ProviderResource},
    control_plane::{AvailabilityState, ExecutorRecord, HealthState, LeaseState, ProviderRecord, ResourceConfigState, ResourceOwnershipMode, ResourceRecord, ResourceState, WorkloadRecord},
    core::{ExecutorId, NodeId, ProviderId, ResourceId, ResourceType, WorkloadId},
};
use std::collections::BTreeMap;

use crate::{
    config::EngineConfig,
    model::{EngineExecutionRuntime, EngineRuntimeResource, EngineSnapshot, ExecutionArtifactRecord, ExecutionArtifactResource, ExecutionSessionResource, ExecutionSessionState, GraphRef},
};

const DEFAULT_PROVIDER_CLIENT_NAME: &str = "helios-engine-provider";
const DEFAULT_EXECUTOR_CLIENT_NAME: &str = "helios-engine-executor";

#[derive(Debug, thiserror::Error)]
pub enum EnginePublishError {
    #[error(transparent)]
    Client(#[from] ClientError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrionEnginePublisher {
    provider_client_name: String,
    executor_client_name: String,
    provider_id: String,
    executor_id: String,
    node_id: String,
    node_runtime: LocalNodeRuntime,
}

impl OrionEnginePublisher {
    pub fn from_config(config: &EngineConfig) -> Self {
        Self {
            provider_client_name: DEFAULT_PROVIDER_CLIENT_NAME.to_string(),
            executor_client_name: DEFAULT_EXECUTOR_CLIENT_NAME.to_string(),
            provider_id: provider_id_for(&config.node_id),
            executor_id: executor_id_for(&config.node_id),
            node_id: config.node_id.clone(),
            node_runtime: LocalNodeRuntime::new(config.orion_ipc_socket_path.clone(), config.orion_ipc_stream_socket_path.clone()),
        }
    }

    pub fn provider_client_name(&self) -> &str {
        self.provider_client_name.as_str()
    }

    pub fn executor_client_name(&self) -> &str {
        self.executor_client_name.as_str()
    }

    pub fn provider_identity_record(&self) -> ProviderRecord {
        ProviderRecord::builder(ProviderId::new(self.provider_id.clone()), NodeId::new(self.node_id.clone()))
            .resource_type(EngineRuntimeResource::resource_type())
            .resource_type(ExecutionSessionResource::resource_type())
            .resource_type(ExecutionArtifactResource::resource_type())
            .build()
    }

    pub fn executor_identity_record(&self) -> ExecutorRecord {
        ExecutorRecord::builder(ExecutorId::new(self.executor_id.clone()), NodeId::new(self.node_id.clone())).runtime_type(EngineExecutionRuntime::runtime_type()).build()
    }

    pub async fn register_identities(&self) -> Result<(), EnginePublishError> {
        self.runtime_publisher().register_all().await?;
        Ok(())
    }

    pub async fn publish_snapshot(&self, snapshot: &EngineSnapshot, workloads: &[WorkloadRecord]) -> Result<(), EnginePublishError> {
        let provider_resources = std::iter::once(self.runtime_resource(snapshot))
            .chain(session_resources(self.executor_id.as_str(), self.provider_id.as_str(), &snapshot.sessions))
            .chain(artifact_resources(self.executor_id.as_str(), self.provider_id.as_str(), &snapshot.artifacts))
            .collect::<Vec<_>>();
        self.runtime_publisher().publish_provider_resources(provider_resources).await?;
        self.runtime_publisher().publish_executor_snapshot(observed_workloads(workloads, &snapshot.sessions), std::iter::empty::<ResourceRecord>()).await?;
        Ok(())
    }

    fn runtime_publisher(&self) -> LocalRuntimePublisher {
        LocalRuntimePublisher::builder(self.node_runtime.clone(), "helios-engine").provider(self.provider_identity_record()).executor(self.executor_identity_record()).build()
    }

    fn runtime_resource(&self, snapshot: &EngineSnapshot) -> ResourceRecord {
        let mut builder = ProviderResource::new(ResourceId::new(format!("engine.runtime.{}", snapshot.node_id)), EngineRuntimeResource::resource_type(), ProviderId::new(self.provider_id.clone()))
            .ownership_mode(ResourceOwnershipMode::Exclusive)
            .health(HealthState::Healthy)
            .availability(AvailabilityState::Available)
            .lease_state(LeaseState::Unleased)
            .label(format!("helios.display_name=Helios Engine {}", snapshot.node_id))
            .label(format!("helios.engine.plugin_count={}", snapshot.loaded_plugins.len()))
            .label(format!("helios.engine.assigned_workloads={}", snapshot.assigned_workloads.len()))
            .endpoint(format!("unix://{}", snapshot.engine_socket_path.display()));

        for plugin in &snapshot.loaded_plugins {
            if let Some(name) = &plugin.plugin_name {
                builder = builder.label(format!("helios.plugin.loaded={name}"));
            }
        }

        builder.build()
    }
}

fn observed_workloads(workloads: &[WorkloadRecord], sessions: &[ExecutionSessionState]) -> Vec<WorkloadRecord> {
    let session_by_workload: BTreeMap<_, _> = sessions.iter().map(|session| (session.workload_id.as_str(), session)).collect();

    workloads
        .iter()
        .cloned()
        .map(|mut workload| {
            if let Some(session) = session_by_workload.get(workload.workload_id.as_str()) {
                workload.observed_state = session.status.to_workload_observed_state();
            }
            workload
        })
        .collect()
}

fn session_resources<'a>(executor_id: &'a str, provider_id: &'a str, sessions: &'a [ExecutionSessionState]) -> impl Iterator<Item = ResourceRecord> + 'a {
    sessions.iter().map(move |session| {
        let mut state = ResourceConfigState::new().field("status", orion::control_plane::TypedConfigValue::String(session.status.as_str().into()));

        match &session.graph_ref {
            GraphRef::ArtifactId(value) => state = state.field("graph.artifact_id", orion::control_plane::TypedConfigValue::String(value.clone())),
            GraphRef::ResourceId(value) => state = state.field("graph.resource_id", orion::control_plane::TypedConfigValue::String(value.clone())),
            GraphRef::InlineSpec(value) => state = state.field("graph.inline", orion::control_plane::TypedConfigValue::String(value.clone())),
        }

        for plugin in &session.plugin_requirements {
            state = state.field(format!("plugin.{}", plugin.plugin_name), orion::control_plane::TypedConfigValue::String(plugin.version.clone().unwrap_or_else(|| "present".into())));
        }
        if let Some(message) = &session.message {
            state = state.field("message", orion::control_plane::TypedConfigValue::String(message.clone()));
        }

        DerivedResource::new(ResourceId::new(format!("engine.session.{}", session.session_id)), ExecutionSessionResource::resource_type(), ProviderId::new(provider_id))
            .realized_by_executor(ExecutorId::new(executor_id))
            .realized_for_workload(WorkloadId::new(session.workload_id.clone()))
            .source_workload(WorkloadId::new(session.workload_id.clone()))
            .ownership_mode(ResourceOwnershipMode::Exclusive)
            .health(HealthState::Healthy)
            .availability(AvailabilityState::Available)
            .lease_state(LeaseState::Unleased)
            .label(format!("helios.session.status={}", session.status.as_str()))
            .state(ResourceState::new(session.observed_at_ms).with_config(state))
            .build()
    })
}

fn artifact_resources<'a>(executor_id: &'a str, provider_id: &'a str, artifacts: &'a [ExecutionArtifactRecord]) -> impl Iterator<Item = ResourceRecord> + 'a {
    artifacts.iter().map(move |artifact| {
        let mut config = ResourceConfigState::new()
            .field("kind", orion::control_plane::TypedConfigValue::String(artifact.kind.clone()))
            .field("artifact_id", orion::control_plane::TypedConfigValue::String(artifact.artifact_id.clone()));
        if let Some(message) = &artifact.message {
            config = config.field("message", orion::control_plane::TypedConfigValue::String(message.clone()));
        }
        let resource_type = if artifact.kind.starts_with("stream.channel:") { ResourceType::new("stream.channel") } else { ExecutionArtifactResource::resource_type() };
        let mut builder = DerivedResource::new(ResourceId::new(format!("engine.artifact.{}", artifact.artifact_id)), resource_type, ProviderId::new(provider_id))
            .realized_by_executor(ExecutorId::new(executor_id))
            .realized_for_workload(WorkloadId::new(artifact.workload_id.clone()))
            .source_workload(WorkloadId::new(artifact.workload_id.clone()))
            .ownership_mode(ResourceOwnershipMode::SharedRead)
            .health(HealthState::Healthy)
            .availability(AvailabilityState::Available)
            .lease_state(LeaseState::Unleased)
            .label(format!("helios.artifact.kind={}", artifact.kind))
            .state(ResourceState::new(artifact.observed_at_ms).with_config(config));
        for endpoint in &artifact.endpoints {
            builder = builder.endpoint(endpoint.clone());
        }
        builder.build()
    })
}

fn provider_id_for(node_id: &str) -> String {
    format!("provider.engine.{node_id}")
}

fn executor_id_for(node_id: &str) -> String {
    format!("executor.engine.{node_id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ExecutionSessionStatus;
    use orion::control_plane::WorkloadObservedState;

    #[test]
    fn session_resources_declare_source_workload() {
        let sessions = vec![ExecutionSessionState {
            workload_id: "workload.test".into(),
            session_id: "session.workload.test".into(),
            status: ExecutionSessionStatus::Running,
            observed_at_ms: 123,
            graph_ref: GraphRef::InlineSpec("{\"nodes\":[]}".into()),
            bindings: Vec::new(),
            plugin_requirements: Vec::new(),
            message: None,
        }];

        let resource = session_resources("executor.test", "provider.test", &sessions).next().expect("session resource should be produced");

        assert_eq!(resource.realized_for_workload_id.as_ref().map(ToString::to_string), Some("workload.test".into()));
        assert_eq!(resource.source_workload_id.as_ref().map(ToString::to_string), Some("workload.test".into()));
    }

    #[test]
    fn artifact_resources_declare_source_workload() {
        let artifacts = vec![ExecutionArtifactRecord {
            workload_id: "workload.test".into(),
            session_id: "session.workload.test".into(),
            artifact_id: "artifact.test".into(),
            kind: "telemetry".into(),
            observed_at_ms: 456,
            message: Some("{\"value\":42}".into()),
            endpoints: Vec::new(),
        }];

        let resource = artifact_resources("executor.test", "provider.test", &artifacts).next().expect("artifact resource should be produced");

        assert_eq!(resource.realized_for_workload_id.as_ref().map(ToString::to_string), Some("workload.test".into()));
        assert_eq!(resource.source_workload_id.as_ref().map(ToString::to_string), Some("workload.test".into()));
        assert_eq!(resource.state.as_ref().and_then(|state| state.config.as_ref()).and_then(|config| config.payload.get("message")).and_then(|value| value.as_str()), Some("{\"value\":42}"));
    }

    #[test]
    fn frame_artifacts_publish_as_stream_channel_resources() {
        let artifacts = vec![ExecutionArtifactRecord {
            workload_id: "workload.stream".into(),
            session_id: "session.workload.stream".into(),
            artifact_id: "artifact.stream".into(),
            kind: "stream.channel:frame".into(),
            observed_at_ms: 456,
            message: None,
            endpoints: vec!["styx-frame-lease+unix:///tmp/frame.sock".into()],
        }];

        let resource = artifact_resources("executor.test", "provider.test", &artifacts).next().expect("stream resource should be produced");

        assert_eq!(resource.resource_type.as_str(), "stream.channel");
        assert!(resource.endpoints.iter().any(|endpoint| endpoint.starts_with("styx-frame-lease+unix://")));
    }

    #[test]
    fn observed_workloads_overlay_session_status() {
        let workloads = vec![WorkloadRecord::builder("workload.demo", EngineExecutionRuntime::runtime_type(), "artifact.demo").observed_state(WorkloadObservedState::Pending).build()];
        let sessions = vec![ExecutionSessionState {
            workload_id: "workload.demo".into(),
            session_id: "session.workload.demo".into(),
            status: ExecutionSessionStatus::Succeeded,
            observed_at_ms: 42,
            graph_ref: GraphRef::InlineSpec("{\"nodes\":[]}".into()),
            bindings: Vec::new(),
            plugin_requirements: Vec::new(),
            message: Some("ok".into()),
        }];

        let observed = observed_workloads(&workloads, &sessions);
        assert_eq!(observed[0].observed_state, WorkloadObservedState::Completed);
    }
}
