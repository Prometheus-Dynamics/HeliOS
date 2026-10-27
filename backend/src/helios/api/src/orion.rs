//! Orion, the device's state store, over its local IPC socket. Everything the engine, the
//! peripherals service and the updater publish is read from here, and pipeline/update/peripheral
//! requests are written here as desired state.

use std::{collections::BTreeMap, collections::BTreeSet, future::Future, path::PathBuf, sync::Arc, time::Duration};

use orion::{
    Revision,
    client::{ClientError, ControlPlaneEventStream, LocalControlPlaneClient},
    control_plane::{
        ActionRequest, ActionResult, ArtifactRecord, ClusterStateEnvelope, DesiredClusterState, DesiredStateMutation, LeaseRecord, MutationBatch, NodeObservabilitySnapshot, NodeRecord,
        ObservedClusterState, ResourceRecord, StateSnapshot, StatusEntry, StatusQuery, WorkloadObservedState, WorkloadRecord,
    },
    core::ResourceId,
};
use orion_client::ActionCaller;
use tokio::sync::Mutex;

use crate::error::{ApiError, ApiResult};

const CLIENT_NAME: &str = "helios-api";
const EVENTS_CLIENT_NAME: &str = "helios-api-events";
const ACTIONS_CLIENT_NAME: &str = "helios-api-actions";
const LIVE_STREAMS_CLIENT_NAME: &str = "helios-api-live";

/// Anything that runs an Orion action and waits for its result: [`Orion`] in the server, a fake in
/// the tests of the action routes.
pub trait ActionCall: Sync {
    fn call_action(&self, request: ActionRequest, timeout: Duration) -> impl Future<Output = ApiResult<ActionResult>> + Send;
}

#[derive(Debug, Clone)]
pub struct Orion {
    socket: PathBuf,
    stream_socket: PathBuf,
    /// One control-plane stream for every action, shared by all callers (concurrent calls do not
    /// wait for each other); connected on first use and again after it drops.
    actions: Arc<Mutex<Option<ActionCaller>>>,
}

impl Orion {
    pub fn new(socket: PathBuf, stream_socket: PathBuf) -> Self {
        Self { socket, stream_socket, actions: Arc::new(Mutex::new(None)) }
    }

    /// Runs `request` and waits up to `timeout` for its final result (Orion `ActionCaller`): the
    /// latest, still running result when the wait runs out.
    pub async fn call_action(&self, request: ActionRequest, timeout: Duration) -> ApiResult<ActionResult> {
        let caller = {
            let mut slot = self.actions.lock().await;
            match slot.as_ref() {
                Some(caller) => caller.clone(),
                None => {
                    let caller = ActionCaller::connect_at(&self.stream_socket, ACTIONS_CLIENT_NAME).await.map_err(|error| unreachable(&self.stream_socket, error))?;
                    *slot = Some(caller.clone());
                    caller
                }
            }
        };
        match caller.call(request, timeout).await {
            Ok(result) => Ok(result),
            Err(error) => {
                // The stream is gone (orion-node restarted, or it gave up the call): connect again next time.
                *self.actions.lock().await = None;
                Err(unreachable(&self.stream_socket, error))
            }
        }
    }

    /// Orion's peer discovery: the robot's cluster and the peers discovery found (`docs/discovery.md`).
    pub async fn discovery(&self) -> ApiResult<orion::control_plane::DiscoverySnapshot> {
        self.raw_client()?.query_discovery().await.map_err(|error| unreachable(&self.socket, error))
    }

    pub fn stream_socket(&self) -> &PathBuf {
        &self.stream_socket
    }

    fn client(&self) -> ApiResult<LocalControlPlaneClient> {
        LocalControlPlaneClient::connect_at(&self.socket, CLIENT_NAME).map_err(|error| unreachable(&self.socket, error))
    }

    pub fn raw_client(&self) -> ApiResult<LocalControlPlaneClient> {
        self.client()
    }

    pub async fn snapshot(&self) -> ApiResult<StateSnapshot> {
        self.client()?.fetch_state_snapshot().await.map_err(|error| unreachable(&self.socket, error))
    }

    pub async fn view(&self) -> ApiResult<StateView> {
        Ok(StateView::from_snapshot(self.snapshot().await?))
    }

    /// An event stream on the node's stream socket, subscribed to desired and observed state
    /// changes (`subscribe_state_and_observed`: a full snapshot each, starting with a bootstrap
    /// snapshot) and to the node's whole status lane: this node's `host.*` metrics (Orion
    /// `docs/host-facts.md`, "Following host metrics without polling") and the Lemnos bridge's
    /// device readings (`resource/lemnos.*`, pushed in 50 ms batches), so neither is polled.
    ///
    /// The local address is fixed: reconnecting under it (or after a helios-api restart) resumes
    /// the node's session for this client, which first delivers the events queued while it was
    /// away; the subscriptions here then start again with new bootstraps.
    pub async fn state_and_host_events(&self) -> ApiResult<ControlPlaneEventStream> {
        let fail = |error| unreachable(&self.stream_socket, error);
        let mut events = ControlPlaneEventStream::connect_at(&self.stream_socket, EVENTS_CLIENT_NAME).await.map_err(fail)?;
        events.subscribe_state_and_observed(Revision::ZERO).await.map_err(fail)?;
        events.subscribe_status(StatusQuery::all()).await.map_err(fail)?;
        Ok(events)
    }

    /// An event stream on the node's stream socket for live streams only (`live_streams`): the
    /// caller subscribes to the stream it wants, nothing else is sent.
    pub async fn live_stream_events(&self) -> ApiResult<ControlPlaneEventStream> {
        ControlPlaneEventStream::connect_at(&self.stream_socket, LIVE_STREAMS_CLIENT_NAME).await.map_err(|error| unreachable(&self.stream_socket, error))
    }

    /// Every live status entry of the node (its status lane: the Lemnos bridge's device readings
    /// are the entries of `resource/lemnos.<board>.<device>`).
    pub async fn status_entries(&self) -> ApiResult<Vec<StatusEntry>> {
        self.client()?.query_status(StatusQuery::all()).await.map_err(|error| unreachable(&self.socket, error))
    }

    pub async fn observability(&self) -> ApiResult<NodeObservabilitySnapshot> {
        self.client()?.query_observability().await.map_err(|error| unreachable(&self.socket, error))
    }

    /// Apply `mutations` on top of the current desired revision. A concurrent writer that moves
    /// the revision in between gets one retry.
    pub async fn apply(&self, mutations: Vec<DesiredStateMutation>) -> ApiResult<()> {
        let client = self.client()?;
        let mut last_error = None;
        for _ in 0..2 {
            let revision = client.fetch_state_snapshot().await.map_err(|error| unreachable(&self.socket, error))?.state.desired.revision;
            match client.apply_mutations(MutationBatch { base_revision: revision, mutations: mutations.clone(), stamps: Vec::new() }).await {
                Ok(()) => return Ok(()),
                Err(ClientError::Rejected(reason)) => last_error = Some(reason),
                Err(error) => return Err(unreachable(&self.socket, error)),
            }
        }
        Err(ApiError::conflict(format!("Orion rejected the change: {}", last_error.unwrap_or_default())))
    }
}

impl ActionCall for Orion {
    fn call_action(&self, request: ActionRequest, timeout: Duration) -> impl Future<Output = ApiResult<ActionResult>> + Send {
        Orion::call_action(self, request, timeout)
    }
}

fn unreachable(socket: &std::path::Path, error: ClientError) -> ApiError {
    match error {
        ClientError::Rejected(reason) => ApiError::conflict(format!("Orion rejected the request: {reason}")),
        other => ApiError::backend(format!("Orion is not reachable at {}: {other}", socket.display())),
    }
}

/// Desired and observed state merged into what the API reports: observed records (published by
/// providers and executors) win over desired ones.
#[derive(Debug, Clone, Default)]
pub struct StateView {
    pub desired_revision: u64,
    pub nodes: BTreeMap<String, NodeRecord>,
    pub artifacts: BTreeMap<String, ArtifactRecord>,
    pub workloads: BTreeMap<String, WorkloadRecord>,
    pub resources: BTreeMap<String, ResourceRecord>,
    pub leases: BTreeMap<String, LeaseRecord>,
}

impl StateView {
    pub fn from_snapshot(snapshot: StateSnapshot) -> Self {
        let state = snapshot.state;
        let retired = retired_resources(&state.desired, &state.observed);
        let mut view = Self { desired_revision: state.desired.revision.get(), ..Self::default() };
        for (id, node) in state.desired.nodes {
            view.nodes.insert(id.to_string(), node);
        }
        for (id, node) in state.observed.nodes {
            view.nodes.insert(id.to_string(), node);
        }
        for (id, artifact) in state.desired.artifacts {
            view.artifacts.insert(id.to_string(), artifact);
        }
        for (id, workload) in state.desired.workloads {
            view.workloads.insert(id.to_string(), workload);
        }
        for (id, observed) in state.observed.workloads {
            match view.workloads.get_mut(id.as_str()) {
                Some(desired) => desired.observed_state = observed.observed_state,
                None => {
                    view.workloads.insert(id.to_string(), observed);
                }
            }
        }
        for (id, resource) in state.desired.resources {
            view.resources.insert(id.to_string(), resource);
        }
        for (id, resource) in state.observed.resources {
            if !retired.contains(&id) {
                view.resources.insert(id.to_string(), resource);
            }
        }
        for (id, lease) in state.desired.leases {
            view.leases.insert(id.to_string(), lease);
        }
        for (id, lease) in state.observed.leases {
            if !retired.contains(&id) {
                view.leases.insert(id.to_string(), lease);
            }
        }
        view
    }

    pub fn workload_observed(&self, id: &str) -> Option<WorkloadObservedState> {
        self.workloads.get(id).map(|workload| workload.observed_state)
    }
}

/// The observed resources (and leases) Orion no longer keeps: those no desired record backs, with a
/// provider or executor that is gone from the desired state. Orion's own prune rule
/// (`prune_observed_for_desired`) drops them at its next start, but a removed provider never reports
/// them gone, so they stay observed until then; the old node id's cameras after the node-id migration
/// are one case. Only the view hides them: no record that a live provider or executor backs is touched.
pub fn retired_resources(desired: &DesiredClusterState, observed: &ObservedClusterState) -> BTreeSet<ResourceId> {
    observed
        .resources
        .values()
        .filter(|resource| {
            !desired.resources.contains_key(&resource.resource_id)
                && !desired.providers.contains_key(&resource.provider_id)
                && !resource.realized_by_executor_id.as_ref().is_some_and(|executor| desired.executors.contains_key(executor))
        })
        .map(|resource| resource.resource_id.clone())
        .collect()
}

/// `key=value` labels (Orion's label form) as a map.
pub fn label_map(labels: &[String]) -> BTreeMap<String, String> {
    labels.iter().filter_map(|label| label.split_once('=')).map(|(key, value)| (key.to_string(), value.to_string())).collect()
}

/// All values of a repeated `key=value` label.
pub fn label_values<'a>(labels: &'a [String], key: &str) -> Vec<&'a str> {
    labels.iter().filter_map(|label| label.split_once('=')).filter(|(k, _)| *k == key).map(|(_, value)| value).collect()
}

/// An Orion enum in the API's snake_case form (`HealthState::Healthy` -> `healthy`).
pub fn enum_name(value: impl std::fmt::Debug) -> String {
    let debug = format!("{value:?}");
    let mut out = String::with_capacity(debug.len() + 4);
    for (index, ch) in debug.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

pub fn config_value_json(value: &orion::control_plane::TypedConfigValue) -> serde_json::Value {
    use orion::control_plane::TypedConfigValue;
    match value {
        TypedConfigValue::Bool(value) => serde_json::Value::Bool(*value),
        TypedConfigValue::Int(value) => serde_json::Value::from(*value),
        TypedConfigValue::UInt(value) => serde_json::Value::from(*value),
        // JSON has no NaN or infinity: those become null.
        TypedConfigValue::F64(value) => serde_json::Number::from_f64(*value).map_or(serde_json::Value::Null, serde_json::Value::Number),
        TypedConfigValue::String(value) => serde_json::Value::String(value.clone()),
        TypedConfigValue::Bytes(bytes) => serde_json::Value::Array(bytes.iter().map(|byte| serde_json::Value::from(*byte)).collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orion::control_plane::HealthState;

    #[test]
    fn enum_names_are_snake_case() {
        assert_eq!(enum_name(HealthState::Healthy), "healthy");
        assert_eq!(enum_name(WorkloadObservedState::Running), "running");
        assert_eq!(enum_name(orion::control_plane::ResourceOwnershipMode::SharedRead), "shared_read");
    }

    fn snapshot(desired: DesiredClusterState, observed: ObservedClusterState) -> StateSnapshot {
        StateSnapshot { state: ClusterStateEnvelope { desired, observed, applied: Default::default() } }
    }

    #[test]
    fn observed_resources_of_a_gone_provider_are_hidden() {
        use orion::{
            control_plane::ProviderRecord,
            core::{NodeId, ProviderId},
        };
        let mut desired = DesiredClusterState::default();
        desired.put_provider(ProviderRecord::builder(ProviderId::new("provider.peripherals.systemcore-3c99"), NodeId::new("systemcore-3c99")).build());
        desired.put_resource(ResourceRecord::builder(ResourceId::new("capture_device_systemcore-3c99_camera-3c99"), "camera.device", ProviderId::new("provider.peripherals.systemcore-3c99")).build());
        let mut observed = ObservedClusterState::default();
        for (resource, provider) in [
            ("capture_device_systemcore-3c99_camera-3c99", "provider.peripherals.systemcore-3c99"),
            // The old id's providers were removed from the desired state; their observed records stayed.
            ("capture_device_node-local_video0", "provider.peripherals.node-local"),
            ("lemnos_raw_node-local_io", "provider.peripherals.node-local"),
        ] {
            observed.put_resource(ResourceRecord::builder(ResourceId::new(resource), "camera.device", ProviderId::new(provider)).build());
        }
        let retired = retired_resources(&desired, &observed);
        assert_eq!(retired.iter().map(ResourceId::as_str).collect::<Vec<_>>(), ["capture_device_node-local_video0", "lemnos_raw_node-local_io"]);

        let view = StateView::from_snapshot(snapshot(desired, observed));
        assert!(view.resources.contains_key("capture_device_systemcore-3c99_camera-3c99"), "a live provider's record stays");
        assert!(!view.resources.contains_key("capture_device_node-local_video0"));
        assert!(!view.resources.contains_key("lemnos_raw_node-local_io"));
    }

    #[test]
    fn a_record_backed_by_a_desired_resource_or_executor_stays() {
        use orion::{
            control_plane::ExecutorRecord,
            core::{ExecutorId, NodeId, ProviderId},
        };
        let mut desired = DesiredClusterState::default();
        desired.put_executor(ExecutorRecord::builder(ExecutorId::new("executor.engine.node-local"), NodeId::new("node-local")).build());
        desired.put_resource(ResourceRecord::builder(ResourceId::new("camera.front"), "camera.device", ProviderId::new("provider.gone")).build());
        let mut observed = ObservedClusterState::default();
        // Realized by a desired executor: its provider need not be in the desired state.
        observed.put_resource(ResourceRecord::builder(ResourceId::new("engine.runtime.node-local"), "execution.runtime", ProviderId::new("provider.engine.node-local")).realized_by_executor(ExecutorId::new("executor.engine.node-local")).build());
        // Backed by a desired resource: kept whatever its provider.
        observed.put_resource(ResourceRecord::builder(ResourceId::new("camera.front"), "camera.device", ProviderId::new("provider.gone")).build());
        assert!(retired_resources(&desired, &observed).is_empty());
    }

    #[test]
    fn labels_split_on_first_equals() {
        let labels = vec!["helios.display_name=Front cam".to_string(), "helios.plugin.loaded=eidos".to_string(), "helios.plugin.loaded=styx=x".to_string()];
        assert_eq!(label_map(&labels).get("helios.display_name").map(String::as_str), Some("Front cam"));
        assert_eq!(label_values(&labels, "helios.plugin.loaded"), vec!["eidos", "styx=x"]);
    }
}
