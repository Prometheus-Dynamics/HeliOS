//! Orion, the device's state store, over its local IPC socket. Everything the engine, the
//! peripherals service and the updater publish is read from here, and pipeline/update/peripheral
//! requests are written here as desired state.

use std::{collections::BTreeMap, future::Future, path::PathBuf, sync::Arc, time::Duration};

use orion::{
    Revision,
    client::{ClientError, ControlPlaneEventStream, LocalControlPlaneClient},
    control_plane::{
        ActionRequest, ActionResult, ArtifactRecord, DesiredStateMutation, LeaseRecord, MutationBatch, NodeObservabilitySnapshot, NodeRecord, ResourceRecord, StateSnapshot, StatusEntry, StatusQuery,
        WorkloadObservedState, WorkloadRecord,
    },
};
use orion_client::ActionCaller;
use tokio::sync::Mutex;

use crate::error::{ApiError, ApiResult};

const CLIENT_NAME: &str = "helios-api";
const EVENTS_CLIENT_NAME: &str = "helios-api-events";
const ACTIONS_CLIENT_NAME: &str = "helios-api-actions";
const LIVE_STREAMS_CLIENT_NAME: &str = "helios-api-live";
const STATE_CLIENT_NAME: &str = "helios-api-state";
/// How long the state follower waits before it subscribes again after its stream ended.
const FOLLOW_RETRY: Duration = Duration::from_secs(1);

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
    /// The newest state, shared by every request ([`Orion::view`]).
    views: Arc<Views>,
}

/// The node's state as one shared [`StateView`]. [`Orion::follow_state`] keeps it current from
/// Orion's state subscription, so a request takes it without asking Orion or copying anything: a
/// snapshot read per request was the API's whole heap (twenty UI polls at once decoded twenty
/// copies of the node's state; glibc kept the high-water mark, 23-34 MB a boot).
#[derive(Debug, Default)]
struct Views {
    latest: std::sync::Mutex<Latest>,
    /// One snapshot read at a time: requests that find the view stale share it.
    fetch: Mutex<()>,
}

#[derive(Debug, Default)]
struct Latest {
    view: Option<Arc<StateView>>,
    /// The follower's subscription is live: views it delivers are current.
    following: bool,
    /// Changes this process made to the state (mutations, actions), and how many of them the
    /// newest read view has: while it lacks one, the next request reads a view.
    changes: u64,
    read_changes: u64,
    /// Bumped by each view stored.
    generation: u64,
}

impl Latest {
    fn current(&self) -> Option<Arc<StateView>> {
        self.view.clone().filter(|_| self.following && self.read_changes == self.changes)
    }

    /// Keep `view` unless a newer desired revision is already held (a subscription snapshot that
    /// was on its way while a read got a newer one).
    fn store(&mut self, view: Arc<StateView>) {
        if self.view.as_ref().is_some_and(|held| held.desired_revision > view.desired_revision) {
            return;
        }
        self.view = Some(view);
        self.generation += 1;
    }
}

impl Views {
    fn latest(&self) -> std::sync::MutexGuard<'_, Latest> {
        self.latest.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Orion {
    pub fn new(socket: PathBuf, stream_socket: PathBuf) -> Self {
        Self { socket, stream_socket, actions: Arc::new(Mutex::new(None)), views: Arc::default() }
    }

    /// Follow the node's state (desired and observed) for [`Orion::view`] until the task is
    /// dropped: one subscription, a new view per change. Without it each view is read from Orion.
    pub fn follow_state(&self) -> tokio::task::JoinHandle<()> {
        let orion = self.clone();
        tokio::spawn(async move {
            loop {
                if let Err(error) = orion.follow_once().await {
                    tracing::debug!(error = %error, "Orion state subscription ended; subscribing again");
                }
                orion.views.latest().following = false;
                tokio::time::sleep(FOLLOW_RETRY).await;
            }
        })
    }

    async fn follow_once(&self) -> Result<(), String> {
        let mut events = ControlPlaneEventStream::connect_at(&self.stream_socket, STATE_CLIENT_NAME).await.map_err(|error| error.to_string())?;
        events.subscribe_state_and_observed(Revision::ZERO).await.map_err(|error| error.to_string())?;
        loop {
            let batch = events.next_events().await.map_err(|error| error.to_string())?;
            // Only the newest snapshot of a batch is decoded.
            let Some(snapshot) = batch.into_iter().rev().find_map(|event| match event.event {
                orion::control_plane::ClientEventKind::StateSnapshot(snapshot) => Some(snapshot),
                _ => None,
            }) else {
                continue;
            };
            let view = Arc::new(StateView::from_snapshot(*snapshot));
            let mut latest = self.views.latest();
            latest.store(view);
            latest.following = true;
        }
    }

    /// The state changed here (a mutation or an action): the next view is read, not followed.
    fn changed(&self) {
        self.views.latest().changes += 1;
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
        let result = caller.call(request, timeout).await;
        self.changed();
        match result {
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

    /// The node's state: the followed view while it is current, else one read (concurrent
    /// requests share a read that started after they asked).
    pub async fn view(&self) -> ApiResult<Arc<StateView>> {
        let asked = {
            let latest = self.views.latest();
            if let Some(view) = latest.current() {
                return Ok(view);
            }
            latest.generation
        };
        let _fetch = self.views.fetch.lock().await;
        let changes = {
            let latest = self.views.latest();
            if latest.generation > asked
                && latest.read_changes == latest.changes
                && let Some(view) = &latest.view
            {
                return Ok(view.clone());
            }
            latest.changes
        };
        let view = Arc::new(StateView::from_snapshot(self.snapshot().await?));
        let mut latest = self.views.latest();
        latest.read_changes = latest.read_changes.max(changes);
        latest.store(view.clone());
        Ok(view)
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
        // The held view's revision first; a rejection (another writer moved it) reads it.
        let mut revision = self.views.latest().current().map(|view| Revision::new(view.desired_revision));
        for _ in 0..2 {
            let base_revision = match revision.take() {
                Some(revision) => revision,
                None => client.fetch_state_snapshot().await.map_err(|error| unreachable(&self.socket, error))?.state.desired.revision,
            };
            let applied = client.apply_mutations(MutationBatch { base_revision, mutations: mutations.clone(), stamps: Vec::new() }).await;
            self.changed();
            match applied {
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
    /// Inline pipeline graphs parsed from this view, by workload id ([`StateView::graph`]).
    graphs: Graphs,
    pub nodes: BTreeMap<String, NodeRecord>,
    pub artifacts: BTreeMap<String, ArtifactRecord>,
    pub workloads: BTreeMap<String, WorkloadRecord>,
    pub resources: BTreeMap<String, ResourceRecord>,
    pub leases: BTreeMap<String, LeaseRecord>,
}

impl StateView {
    pub fn from_snapshot(snapshot: StateSnapshot) -> Self {
        let state = snapshot.state;
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
            view.resources.insert(id.to_string(), resource);
        }
        for (id, lease) in state.desired.leases {
            view.leases.insert(id.to_string(), lease);
        }
        for (id, lease) in state.observed.leases {
            view.leases.insert(id.to_string(), lease);
        }
        view
    }

    /// `text`, the inline graph of workload `id`, parsed once for this view: every request that
    /// shows the pipeline shares the tree (each one parsed ~100 KB of graph before).
    pub fn graph(&self, id: &str, text: &str) -> Result<Arc<serde_json::Value>, serde_json::Error> {
        let mut graphs = self.graphs.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((held, graph)) = graphs.get(id)
            && held.as_str() == text
        {
            return Ok(graph.clone());
        }
        let graph = Arc::new(serde_json::from_str::<serde_json::Value>(text)?);
        graphs.insert(id.to_string(), (text.to_string(), graph.clone()));
        Ok(graph)
    }

    pub fn workload_observed(&self, id: &str) -> Option<WorkloadObservedState> {
        self.workloads.get(id).map(|workload| workload.observed_state)
    }
}

/// The parsed graphs of a view. A copy of a view starts without them.
#[derive(Debug, Default)]
struct Graphs(std::sync::Mutex<BTreeMap<String, (String, Arc<serde_json::Value>)>>);

impl Clone for Graphs {
    fn clone(&self) -> Self {
        Self::default()
    }
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

    #[test]
    fn labels_split_on_first_equals() {
        let labels = vec!["helios.display_name=Front cam".to_string(), "helios.plugin.loaded=eidos".to_string(), "helios.plugin.loaded=styx=x".to_string()];
        assert_eq!(label_map(&labels).get("helios.display_name").map(String::as_str), Some("Front cam"));
        assert_eq!(label_values(&labels, "helios.plugin.loaded"), vec!["eidos", "styx=x"]);
    }

    fn view_at(revision: u64) -> Arc<StateView> {
        Arc::new(StateView { desired_revision: revision, ..StateView::default() })
    }

    #[test]
    fn a_followed_view_is_shared_until_this_process_changes_the_state() {
        let mut latest = Latest::default();
        latest.store(view_at(4));
        assert!(latest.current().is_none(), "nothing is current before the subscription is live");
        latest.following = true;
        let shared = latest.current().expect("followed");
        assert!(Arc::ptr_eq(&shared, &latest.current().expect("followed")), "requests share one view");

        // A change made here: the followed view may not have it yet, so it is read again.
        latest.changes += 1;
        assert!(latest.current().is_none());
        latest.store(view_at(5));
        assert!(latest.current().is_none(), "a followed view does not stand in for a read");
        latest.read_changes = latest.changes;
        assert_eq!(latest.current().map(|view| view.desired_revision), Some(5));

        // The subscription ending: views are read again until it is back.
        latest.following = false;
        assert!(latest.current().is_none());
    }

    #[test]
    fn an_older_snapshot_does_not_replace_a_newer_view() {
        let mut latest = Latest { following: true, ..Latest::default() };
        latest.store(view_at(7));
        let generation = latest.generation;
        latest.store(view_at(6));
        assert_eq!(latest.current().map(|view| view.desired_revision), Some(7));
        assert_eq!(latest.generation, generation);
        latest.store(view_at(7));
        assert_eq!(latest.generation, generation + 1, "the same revision with new observed state is kept");
    }
}
