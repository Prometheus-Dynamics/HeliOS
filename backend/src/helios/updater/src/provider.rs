use orion::{
    client::{ClientError, DerivedResource, LocalNodeRuntime, LocalRuntimePublisher, ProviderResource},
    control_plane::{
        AvailabilityState, ExecutorRecord, HealthState, LeaseState, ProviderRecord, ResourceConfigState, ResourceOwnershipMode, ResourceRecord, ResourceState, WorkloadObservedState, WorkloadRecord,
    },
    core::{ExecutorId, NodeId, ProviderId, ResourceId, WorkloadId},
};
use std::collections::BTreeMap;

use crate::{
    config::UpdaterConfig,
    model::{AssignedUpdateExecution, SystemUpdateExecutionResource, SystemUpdateRuntime, SystemUpdateRuntimeResource, UpdateRunState, UpdateWorkload},
};

const DEFAULT_PROVIDER_CLIENT_NAME: &str = "helios-updater-provider";
const DEFAULT_EXECUTOR_CLIENT_NAME: &str = "helios-updater-executor";

#[derive(Debug, thiserror::Error)]
pub enum UpdaterPublishError {
    #[error(transparent)]
    Client(#[from] ClientError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdaterSnapshot {
    pub node_id: String,
    pub run_state: UpdateRunState,
    pub assigned_workloads: Vec<UpdateWorkload>,
    pub executions: Vec<AssignedUpdateExecution>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrionUpdaterPublisher {
    provider_client_name: String,
    executor_client_name: String,
    provider_id: String,
    executor_id: String,
    node_id: String,
    node_runtime: LocalNodeRuntime,
}

impl OrionUpdaterPublisher {
    pub fn from_config(config: &UpdaterConfig) -> Self {
        Self {
            provider_client_name: DEFAULT_PROVIDER_CLIENT_NAME.to_string(),
            executor_client_name: DEFAULT_EXECUTOR_CLIENT_NAME.to_string(),
            provider_id: provider_id_for(&config.node_id),
            executor_id: executor_id_for(&config.node_id),
            node_id: config.node_id.clone(),
            node_runtime: LocalNodeRuntime::new(config.orion_ipc_socket_path.clone(), config.orion_ipc_stream_socket_path.clone()),
        }
    }

    pub fn provider_identity_record(&self) -> ProviderRecord {
        ProviderRecord::builder(ProviderId::new(self.provider_id.clone()), NodeId::new(self.node_id.clone()))
            .resource_type(SystemUpdateRuntimeResource::resource_type())
            .resource_type(SystemUpdateExecutionResource::resource_type())
            .build()
    }

    pub fn executor_identity_record(&self) -> ExecutorRecord {
        ExecutorRecord::builder(ExecutorId::new(self.executor_id.clone()), NodeId::new(self.node_id.clone())).runtime_type(SystemUpdateRuntime::runtime_type()).build()
    }

    pub fn executor_client_name(&self) -> &str {
        self.executor_client_name.as_str()
    }

    pub async fn register_identities(&self) -> Result<(), UpdaterPublishError> {
        self.runtime_publisher().register_all().await?;
        Ok(())
    }

    pub async fn publish_snapshot(&self, snapshot: &UpdaterSnapshot, workloads: &[WorkloadRecord]) -> Result<(), UpdaterPublishError> {
        self.runtime_publisher().publish_provider_resources(vec![self.runtime_resource(snapshot)]).await?;

        self.runtime_publisher()
            .publish_executor_snapshot(observed_workloads(workloads, &snapshot.executions), execution_resources(self.executor_id.as_str(), self.provider_id.as_str(), &snapshot.executions))
            .await?;
        Ok(())
    }

    fn runtime_publisher(&self) -> LocalRuntimePublisher {
        LocalRuntimePublisher::builder(self.node_runtime.clone(), "helios-updater").provider(self.provider_identity_record()).executor(self.executor_identity_record()).build()
    }

    fn runtime_resource(&self, snapshot: &UpdaterSnapshot) -> ResourceRecord {
        ProviderResource::new(ResourceId::new(format!("updater.runtime.{}", snapshot.node_id)), SystemUpdateRuntimeResource::resource_type(), ProviderId::new(self.provider_id.clone()))
            .ownership_mode(ResourceOwnershipMode::Exclusive)
            .health(HealthState::Healthy)
            .availability(AvailabilityState::Available)
            .lease_state(LeaseState::Unleased)
            .label(format!("helios.display_name=Helios Updater {}", snapshot.node_id))
            .label(format!("helios.updater.phase={}", snapshot.run_state.phase.as_str()))
            .label(format!("helios.updater.assigned_workloads={}", snapshot.assigned_workloads.len()))
            .build()
    }
}

fn observed_workloads(workloads: &[WorkloadRecord], executions: &[AssignedUpdateExecution]) -> Vec<WorkloadRecord> {
    let execution_by_id: BTreeMap<_, _> = executions.iter().map(|execution| (execution.workload.workload_id.as_str(), execution)).collect();

    workloads
        .iter()
        .cloned()
        .map(|mut workload| {
            if let Some(execution) = execution_by_id.get(workload.workload_id.as_str()) {
                workload.observed_state = observed_state_for_phase(execution.phase);
            }
            workload
        })
        .collect()
}

fn observed_state_for_phase(phase: crate::model::UpdatePhase) -> WorkloadObservedState {
    match phase {
        crate::model::UpdatePhase::Idle => WorkloadObservedState::Pending,
        crate::model::UpdatePhase::Preflight | crate::model::UpdatePhase::Downloading | crate::model::UpdatePhase::Staging => WorkloadObservedState::Starting,
        crate::model::UpdatePhase::SwitchingBoot | crate::model::UpdatePhase::AwaitingBootSuccess | crate::model::UpdatePhase::Finalizing | crate::model::UpdatePhase::RollingBack => {
            WorkloadObservedState::Running
        }
        crate::model::UpdatePhase::Completed => WorkloadObservedState::Completed,
        crate::model::UpdatePhase::Failed => WorkloadObservedState::Failed,
    }
}

fn execution_resources<'a>(executor_id: &'a str, provider_id: &'a str, executions: &'a [AssignedUpdateExecution]) -> impl Iterator<Item = ResourceRecord> + 'a {
    executions.iter().map(move |execution| {
        let workload = &execution.workload;
        DerivedResource::new(ResourceId::new(format!("update.execution.{}", workload.workload_id)), SystemUpdateExecutionResource::resource_type(), ProviderId::new(provider_id))
            .realized_by_executor(ExecutorId::new(executor_id))
            .realized_for_workload(WorkloadId::new(workload.workload_id.clone()))
            .source_workload(WorkloadId::new(workload.workload_id.clone()))
            .ownership_mode(ResourceOwnershipMode::Exclusive)
            .health(HealthState::Healthy)
            .availability(AvailabilityState::Available)
            .lease_state(LeaseState::Unleased)
            .label(format!("helios.update.version={}", workload.version))
            .label(format!("helios.update.artifact_class={}", workload.artifact_class.as_str()))
            .state(
                ResourceState::new(0).with_config(
                    ResourceConfigState::new()
                        .field("phase", orion::control_plane::TypedConfigValue::String(execution.phase.as_str().to_string()))
                        .field("artifact_id", orion::control_plane::TypedConfigValue::String(workload.artifact_id.clone()))
                        .field("message", orion::control_plane::TypedConfigValue::String(execution.status_message.clone())),
                ),
            )
            .build()
    })
}

fn provider_id_for(node_id: &str) -> String {
    format!("provider.updater.{node_id}")
}

fn executor_id_for(node_id: &str) -> String {
    format!("executor.updater.{node_id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{UpdateArtifactClass, UpdatePhase};

    #[test]
    fn execution_resources_declare_source_workload() {
        let workloads = vec![UpdateWorkload {
            workload_id: "update.node-local.1".into(),
            artifact_id: "artifact.os".into(),
            assigned_node_id: "node-local".into(),
            version: "2026.2.0".into(),
            artifact_class: UpdateArtifactClass::OsImage,
        }];
        let state = UpdateRunState { phase: UpdatePhase::Preflight, artifact_id: Some("artifact.os".into()), version: Some("2026.2.0".into()), status_message: "planned".into() };
        let executions = vec![AssignedUpdateExecution { workload: workloads[0].clone(), phase: state.phase, status_message: state.status_message.clone() }];

        let resource = execution_resources("executor.test", "provider.test", &executions).next().expect("execution resource should be produced");

        assert_eq!(resource.source_workload_id.as_ref().map(ToString::to_string), Some("update.node-local.1".into()));
    }

    #[test]
    fn observed_workloads_follow_execution_phase() {
        let workloads = vec![WorkloadRecord::builder("update.node-local.1", SystemUpdateRuntime::runtime_type(), "artifact.os").observed_state(WorkloadObservedState::Pending).build()];
        let execution = AssignedUpdateExecution {
            workload: UpdateWorkload {
                workload_id: "update.node-local.1".into(),
                artifact_id: "artifact.os".into(),
                assigned_node_id: "node-local".into(),
                version: "2026.2.0".into(),
                artifact_class: UpdateArtifactClass::OsImage,
            },
            phase: UpdatePhase::Completed,
            status_message: "done".into(),
        };

        let observed = observed_workloads(&workloads, &[execution]);

        assert_eq!(observed.len(), 1);
        assert_eq!(observed[0].observed_state, WorkloadObservedState::Completed);
    }
}
