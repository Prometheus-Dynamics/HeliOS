use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration as StdDuration,
};

use orion::{
    client::{ClientError, LocalNodeRuntime, LocalProviderEvent, LocalProviderService, LocalServiceRetryPolicy},
    control_plane::{DesiredState, LeaseRecord, StateSnapshot, WorkloadRecord, deserialize_config},
    core::Revision,
};
use serde::Deserialize;
use styx::watch::{CompositeWatcher, LinuxVideoFsWatcher, WatchRuntime};
use tokio::{
    signal,
    sync::{Notify, mpsc},
    task::JoinHandle,
    time::{Duration, sleep},
};
use tracing::warn;

use crate::{
    config::{LEMNOSD_CLIENT_NAME, PeripheralConfig},
    lemnosd::{
        CONTROL_SET_ACTION, FAN_OVERRIDE_ACTION, FAN_RELEASE_ACTION, FanOverrideRequest, LemnosdBridge, LemnosdOptions, ProviderHealth,
        resources::{DEVICE_ID_LABEL, PROBE_NAME},
    },
    model::{NodeId, ResourceActionOutcome, ResourceDescriptor, ResourceKind},
    provider::{CameraServices, OrionPeripheralPublisher, OrionPublishError, ResourceActionFeedback},
    resources::{CaptureProbe, DiscoveryContext, DiscoveryProbe, DiscoverySnapshot, PeripheralInventoryService},
};

const PERIPHERAL_RESOURCE_ACTION_RUNTIME_TYPE: &str = "helios.peripheral.resource_action.v1";
const PROVIDER_EVENT_RETRY_DELAY: Duration = Duration::from_millis(200);

/// A decoded resource action. Every action goes to lemnosd.
#[derive(Debug, Clone, PartialEq)]
enum ResourceActionSpec {
    /// The fan's only write: a duty for a limited time, then back to the kernel's governor.
    FanOverride(FanOverrideRequest),
    /// Ends the fan override now.
    FanRelease,
    /// A control write on a device other than a fan, under lemnosd's write policy.
    ControlSet { control: String, value: f64 },
}

impl ResourceActionSpec {
    fn kind(&self) -> &'static str {
        match self {
            Self::FanOverride(_) => FAN_OVERRIDE_ACTION,
            Self::FanRelease => FAN_RELEASE_ACTION,
            Self::ControlSet { .. } => CONTROL_SET_ACTION,
        }
    }
}

/// A resource action bound to its lemnosd device.
#[derive(Debug, Clone, PartialEq)]
struct ResourceActionRequest {
    device: String,
    spec: ResourceActionSpec,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
enum ResourceActionKind {
    #[serde(rename = "fan.override")]
    FanOverride,
    #[serde(rename = "fan.release")]
    FanRelease,
    #[serde(rename = "control.set")]
    ControlSet,
}

#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ResourceActionArgs {
    /// `fan.override`: 0-255.
    pwm: Option<u64>,
    /// `fan.override`: 0.0-1.0.
    duty: Option<f64>,
    /// `fan.override`: 1000-600000 (default 60000).
    duration_ms: Option<u64>,
    /// `control.set`.
    control: Option<String>,
    /// `control.set`: in the control's unit.
    value: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceActionTag {
    kind: ResourceActionKind,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceActionConfig {
    action: ResourceActionTag,
    #[serde(default)]
    arg: ResourceActionArgs,
}

#[derive(Debug, thiserror::Error)]
pub enum PeripheralRuntimeError {
    #[error("invalid node id '{0}'")]
    InvalidNodeId(String),
    #[error(transparent)]
    Orion(#[from] OrionPublishError),
    #[error(transparent)]
    Client(#[from] ClientError),
    #[error("invalid peripheral resource action workload '{workload_id}': {message}")]
    InvalidResourceActionWorkload { workload_id: String, message: String },
}

#[derive(Clone, Default)]
pub struct RefreshHandle {
    notify: Arc<Notify>,
}

impl RefreshHandle {
    pub fn request_refresh(&self) {
        self.notify.notify_one();
    }
}

enum RuntimeEvent {
    RefreshRequested,
    /// lemnosd's devices, statuses, readings or the fan override changed.
    LemnosdChanged,
    LeaseUpdate(Vec<LeaseRecord>),
    DesiredStateSnapshot(Box<StateSnapshot>),
    ProviderWatchStopped(ClientError),
    RefreshWatchStopped(String),
}

const ORION_IPC_RETRY_DELAY: Duration = Duration::from_secs(2);

pub struct PeripheralRuntime {
    config: PeripheralConfig,
    inventory: Arc<PeripheralInventoryService>,
    publisher: OrionPeripheralPublisher,
    refresh_handle: RefreshHandle,
    node_runtime: LocalNodeRuntime,
    lemnosd: Option<Arc<LemnosdBridge>>,
}

#[derive(Default)]
struct RuntimeState {
    resource_action_feedback: BTreeMap<String, ResourceActionFeedback>,
    /// Action workloads already run: each runs once, however often the desired state is seen.
    applied_actions: BTreeSet<String>,
    camera_services: CameraServices,
}

impl PeripheralRuntime {
    pub fn from_config(config: PeripheralConfig) -> Result<Self, PeripheralRuntimeError> {
        let (inventory, lemnosd) = build_runtime_components(&config)?;
        Ok(Self {
            node_runtime: LocalNodeRuntime::new(config.orion_ipc_socket_path.clone(), config.orion_ipc_stream_socket_path.clone()),
            publisher: OrionPeripheralPublisher::from_config(&config),
            refresh_handle: RefreshHandle::default(),
            config,
            inventory: Arc::new(inventory),
            lemnosd: Some(Arc::new(lemnosd)),
        })
    }

    /// A runtime without lemnosd (tests): resource actions fail.
    pub fn from_parts(config: PeripheralConfig, inventory: Arc<PeripheralInventoryService>) -> Self {
        let node_runtime = LocalNodeRuntime::new(config.orion_ipc_socket_path.clone(), config.orion_ipc_stream_socket_path.clone());
        let publisher = OrionPeripheralPublisher::from_config(&config);
        Self { config, inventory, publisher, refresh_handle: RefreshHandle::default(), node_runtime, lemnosd: None }
    }

    pub async fn refresh_inventory(&self, leases: &[LeaseRecord]) -> Result<DiscoverySnapshot, PeripheralRuntimeError> {
        self.refresh_inventory_with_state(leases, &RuntimeState::default()).await
    }

    async fn refresh_inventory_with_state(&self, leases: &[LeaseRecord], state: &RuntimeState) -> Result<DiscoverySnapshot, PeripheralRuntimeError> {
        let observed_at_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
        let report = self.inventory.refresh_report(observed_at_ms);
        for probe in &report.probe_reports {
            match &probe.error {
                Some(error) => warn!(probe = %probe.probe, error = %error, "peripheral inventory probe failed"),
                None => tracing::debug!(probe = %probe.probe, discovered_resources = probe.discovered_resources, "peripheral inventory probe completed"),
            }
        }
        self.publish_snapshot(&report.snapshot, leases, state).await?;
        Ok(report.snapshot)
    }

    /// `snapshot` with its lemnosd resources rebuilt from the bridge's state (no camera probe).
    fn with_lemnosd_resources(&self, snapshot: DiscoverySnapshot) -> DiscoverySnapshot {
        let Some(bridge) = &self.lemnosd else { return snapshot };
        let context = DiscoveryContext::new(NodeId::new(self.config.node_id.clone()), chrono::Utc::now().timestamp_millis().max(0) as u64);
        match bridge.probe().discover(&context) {
            Ok(lemnosd) => DiscoverySnapshot::new(snapshot.resources.into_iter().filter(|resource| resource.kind != ResourceKind::LemnosDevice).collect()).merge(lemnosd),
            Err(error) => {
                warn!(probe = PROBE_NAME, error = %error, "lemnosd resources could not be built");
                snapshot
            }
        }
    }

    async fn publish_snapshot(&self, snapshot: &DiscoverySnapshot, leases: &[LeaseRecord], state: &RuntimeState) -> Result<(), PeripheralRuntimeError> {
        self.publisher.publish_snapshot_with_feedback(snapshot, leases, &state.resource_action_feedback).await?;
        Ok(())
    }

    fn set_health(&self, health: ProviderHealth) {
        if let Some(bridge) = &self.lemnosd {
            bridge.set_status(health.led_status());
        }
    }

    pub async fn run_until_stopped(&self) -> Result<(), PeripheralRuntimeError> {
        let result = self.run().await;
        if let Err(error) = &result {
            warn!(node_id = %self.config.node_id, error = %error, "peripherals runtime failed");
            self.set_health(ProviderHealth::Failed);
        }
        if let Some(bridge) = self.lemnosd.clone() {
            // Hands a running fan override back to the governor before the process exits.
            let _ = tokio::task::spawn_blocking(move || bridge.stop()).await;
        }
        result
    }

    async fn run(&self) -> Result<(), PeripheralRuntimeError> {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<RuntimeEvent>();
        let refresh_notify = self.refresh_handle.notify.clone();
        let refresh_event_tx = event_tx.clone();

        self.set_health(ProviderHealth::Starting);
        if let Some(bridge) = &self.lemnosd {
            let lemnosd_tx = event_tx.clone();
            if let Err(error) = bridge.start(Box::new(move || {
                let _ = lemnosd_tx.send(RuntimeEvent::LemnosdChanged);
            })) {
                warn!(error = %error, "failed to start the lemnosd connection; no sensors, fan or status light");
            }
        }

        loop {
            match self.publisher.register_identities().await {
                Ok(()) => break,
                Err(error) => {
                    warn!(
                        node_id = %self.config.node_id,
                        error = %error,
                        retry_delay_secs = ORION_IPC_RETRY_DELAY.as_secs(),
                        "peripherals failed to register provider/executor identities; retrying"
                    );
                    sleep(ORION_IPC_RETRY_DELAY).await;
                }
            }
        }

        tokio::spawn(async move {
            loop {
                refresh_notify.notified().await;
                if refresh_event_tx.send(RuntimeEvent::RefreshRequested).is_err() {
                    break;
                }
            }
        });

        let _styx_watch = self.spawn_styx_watch_task(event_tx.clone());

        let provider_service = LocalProviderService::new(self.node_runtime.clone(), format!("{}-watch", self.publisher.client_name()), self.publisher.provider_identity_record())
            .with_retry_policy(LocalServiceRetryPolicy::fixed_delay(PROVIDER_EVENT_RETRY_DELAY));
        let mut provider_subscription = provider_service.subscribe(Revision::ZERO).await?;
        let mut current_leases = match next_provider_event_retrying(&mut provider_subscription).await? {
            LocalProviderEvent::BootstrapLeases(leases) => leases,
            LocalProviderEvent::LeasesChanged { leases, .. } => leases,
            LocalProviderEvent::BootstrapStateSnapshot(_) | LocalProviderEvent::StateSnapshot { .. } => Vec::new(),
        };
        let mut state = RuntimeState::default();
        let mut current_snapshot = self.refresh_inventory_with_state(&current_leases, &state).await?;
        state.camera_services.sync(&self.config, &current_snapshot.resources);
        let mut current_state_snapshot = match next_provider_event_retrying(&mut provider_subscription).await? {
            LocalProviderEvent::BootstrapStateSnapshot(snapshot) | LocalProviderEvent::StateSnapshot { snapshot, .. } => {
                self.reconcile_controls(&current_snapshot, &snapshot, &mut state).await;
                self.publish_snapshot(&current_snapshot, &current_leases, &state).await?;
                Some(snapshot)
            }
            LocalProviderEvent::BootstrapLeases(_) | LocalProviderEvent::LeasesChanged { .. } => None,
        };
        self.set_health(ProviderHealth::Ready);

        let provider_watch_error_tx = event_tx.clone();
        tokio::spawn(async move {
            if let Err(error) = watch_provider_updates(provider_subscription, event_tx).await {
                let _ = provider_watch_error_tx.send(RuntimeEvent::ProviderWatchStopped(error));
            }
        });

        loop {
            tokio::select! {
                _ = shutdown_requested() => return Ok(()),
                maybe_event = event_rx.recv() => {
                    match maybe_event {
                        Some(RuntimeEvent::RefreshRequested) => {
                            current_snapshot = self.refresh_inventory_with_state(&current_leases, &state).await?;
                            state.camera_services.sync(&self.config, &current_snapshot.resources);
                            if let Some(snapshot) = current_state_snapshot.as_ref() {
                                self.reconcile_controls(&current_snapshot, snapshot, &mut state).await;
                                self.publish_snapshot(&current_snapshot, &current_leases, &state).await?;
                            }
                        }
                        Some(RuntimeEvent::LemnosdChanged) => {
                            current_snapshot = self.with_lemnosd_resources(current_snapshot);
                            self.publish_snapshot(&current_snapshot, &current_leases, &state).await?;
                        }
                        Some(RuntimeEvent::LeaseUpdate(leases)) => {
                            current_leases = leases;
                            self.publish_snapshot(&current_snapshot, &current_leases, &state).await?;
                            if let Some(snapshot) = current_state_snapshot.as_ref() {
                                self.reconcile_controls(&current_snapshot, snapshot, &mut state).await;
                                self.publish_snapshot(&current_snapshot, &current_leases, &state).await?;
                            }
                        }
                        Some(RuntimeEvent::DesiredStateSnapshot(snapshot)) => {
                            self.reconcile_controls(&current_snapshot, &snapshot, &mut state).await;
                            self.publish_snapshot(&current_snapshot, &current_leases, &state).await?;
                            current_state_snapshot = Some(*snapshot);
                        }
                        Some(RuntimeEvent::ProviderWatchStopped(error)) => {
                            warn!(node_id = %self.config.node_id, error = %error, "provider watch stopped");
                            self.set_health(ProviderHealth::Degraded);
                        }
                        Some(RuntimeEvent::RefreshWatchStopped(error)) => {
                            warn!(node_id = %self.config.node_id, error = %error, "refresh watch stopped");
                            self.set_health(ProviderHealth::Degraded);
                        }
                        None => return Ok(()),
                    }
                }
            }
        }
    }

    /// Runs the local resource-action workloads the desired state holds, each once. A rejected
    /// workload (no lease, bad arguments) is looked at again on the next change, since its lease
    /// may arrive later.
    async fn reconcile_controls(&self, inventory: &DiscoverySnapshot, snapshot: &StateSnapshot, state: &mut RuntimeState) {
        let resources = &inventory.resources;
        let leases = &snapshot.state.desired.leases;
        let local_workloads = snapshot.state.desired.workloads.values().filter(|workload| is_local_peripheral_resource_action_workload(workload, &self.config.node_id)).collect::<Vec<_>>();
        let active_resource_ids = local_workloads.iter().filter_map(|workload| bound_resource(resources, workload)).map(|resource| resource.id.as_str().to_string()).collect::<BTreeSet<_>>();

        state.resource_action_feedback.retain(|resource_id, _| resources.iter().any(|resource| resource.id.as_str() == resource_id) && active_resource_ids.contains(resource_id));
        state.applied_actions.retain(|workload_id| local_workloads.iter().any(|workload| workload.workload_id.as_str() == workload_id));

        for workload in local_workloads {
            if state.applied_actions.contains(workload.workload_id.as_str()) {
                continue;
            }
            let Some(resource) = bound_resource(resources, workload) else {
                warn!(
                    workload_id = %workload.workload_id,
                    node_id = %self.config.node_id,
                    "peripheral resource action workload is missing a bound resource; ignoring"
                );
                continue;
            };
            let lease = leases.values().find(|lease| lease.resource_id.as_str() == resource.id.as_str());
            let request = match resource_action_request_from_workload(resource, workload, lease) {
                Ok(request) => request,
                Err(error) => {
                    let feedback = ResourceActionFeedback::failed(now_ms(), workload_action_kind(workload), error.to_string());
                    state.resource_action_feedback.insert(resource.id.as_str().to_string(), feedback);
                    warn!(
                        workload_id = %workload.workload_id,
                        resource_id = %resource.id,
                        error = %error,
                        "peripheral resource action workload was rejected"
                    );
                    continue;
                }
            };
            state.applied_actions.insert(workload.workload_id.as_str().to_string());
            let feedback = match self.apply_action(resource, &request).await {
                Ok(outcome) => ResourceActionFeedback::applied(now_ms(), &outcome),
                Err(error) => {
                    warn!(workload_id = %workload.workload_id, resource_id = %resource.id, error = %error, "peripheral resource action failed");
                    ResourceActionFeedback::failed(now_ms(), request.spec.kind(), error)
                }
            };
            state.resource_action_feedback.insert(resource.id.as_str().to_string(), feedback);
        }
    }

    async fn apply_action(&self, resource: &ResourceDescriptor, request: &ResourceActionRequest) -> Result<ResourceActionOutcome, String> {
        let bridge = self.lemnosd.as_ref().ok_or("lemnosd is not configured")?;
        let value = match &request.spec {
            ResourceActionSpec::FanOverride(override_request) => Some(bridge.fan_override(&request.device, *override_request).await?),
            ResourceActionSpec::FanRelease => {
                bridge.fan_release(&request.device).await?;
                None
            }
            ResourceActionSpec::ControlSet { control, value } => Some(bridge.set_control(&request.device, control, *value).await?),
        };
        Ok(ResourceActionOutcome::applied(resource.id.clone(), request.spec.kind(), value))
    }

    fn spawn_styx_watch_task(&self, event_tx: mpsc::UnboundedSender<RuntimeEvent>) -> Option<JoinHandle<()>> {
        if !self.config.enable_linux_probes {
            return None;
        }
        let node_id = self.config.node_id.clone();
        Some(tokio::spawn(async move {
            let mut watcher = CompositeWatcher::new();
            match LinuxVideoFsWatcher::new() {
                Ok(video_fs) => watcher.push(video_fs),
                Err(error) => {
                    let _ = event_tx.send(RuntimeEvent::RefreshWatchStopped(format!("styx watcher init failed on {node_id}: {error}")));
                    return;
                }
            }
            let mut runtime = WatchRuntime::new();
            loop {
                match runtime.poll_watcher_and_refresh(&mut watcher) {
                    Ok(Some(_)) => {
                        if event_tx.send(RuntimeEvent::RefreshRequested).is_err() {
                            return;
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        let _ = event_tx.send(RuntimeEvent::RefreshWatchStopped(format!("styx watcher on {node_id}: {error}")));
                        return;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        }))
    }
}

fn now_ms() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

fn workload_action_kind(workload: &WorkloadRecord) -> String {
    workload.config.as_ref().and_then(|config| config.string("action.kind").map(str::to_string)).unwrap_or_else(|| "resource.action".to_string())
}

async fn watch_provider_updates(mut subscription: orion::client::LocalProviderSubscription, event_tx: mpsc::UnboundedSender<RuntimeEvent>) -> Result<(), ClientError> {
    loop {
        match next_provider_event_retrying(&mut subscription).await? {
            LocalProviderEvent::BootstrapLeases(leases) | LocalProviderEvent::LeasesChanged { leases, .. } => {
                if event_tx.send(RuntimeEvent::LeaseUpdate(leases)).is_err() {
                    return Ok(());
                }
            }
            LocalProviderEvent::BootstrapStateSnapshot(snapshot) | LocalProviderEvent::StateSnapshot { snapshot, .. } => {
                if event_tx.send(RuntimeEvent::DesiredStateSnapshot(Box::new(snapshot))).is_err() {
                    return Ok(());
                }
            }
        }
    }
}

async fn next_provider_event_retrying(subscription: &mut orion::client::LocalProviderSubscription) -> Result<LocalProviderEvent, ClientError> {
    loop {
        match subscription.next_event().await {
            Ok(event) => return Ok(event),
            Err(ClientError::NoMessageAvailable) => {
                sleep(PROVIDER_EVENT_RETRY_DELAY).await;
            }
            Err(error) => return Err(error),
        }
    }
}

fn build_runtime_components(config: &PeripheralConfig) -> Result<(PeripheralInventoryService, LemnosdBridge), PeripheralRuntimeError> {
    let local_node_id = NodeId::try_new(config.node_id.clone()).map_err(|_| PeripheralRuntimeError::InvalidNodeId(config.node_id.clone()))?;
    let lemnosd = LemnosdBridge::new(LemnosdOptions {
        socket: config.lemnosd_socket_path.clone(),
        client: LEMNOSD_CLIENT_NAME.to_string(),
        reading_interval: StdDuration::from_millis(config.lemnosd_reading_interval_ms),
        fan_marker: Some(config.lemnosd_fan_marker_path()),
    });
    let mut service = PeripheralInventoryService::new(local_node_id);
    service.register_probe(Arc::new(lemnosd.probe()));
    if config.enable_linux_probes {
        service.register_probe(Arc::new(CaptureProbe));
    }
    Ok((service, lemnosd))
}

pub fn build_inventory_service(config: &PeripheralConfig) -> Result<PeripheralInventoryService, PeripheralRuntimeError> {
    let (service, _) = build_runtime_components(config)?;
    Ok(service)
}

fn is_local_peripheral_resource_action_workload(workload: &WorkloadRecord, node_id: &str) -> bool {
    workload.runtime_type.as_str() == PERIPHERAL_RESOURCE_ACTION_RUNTIME_TYPE
        && workload.desired_state == DesiredState::Running
        && workload.assigned_node_id.as_ref().is_some_and(|assigned| assigned.as_str() == node_id)
}

fn bound_resource<'a>(resources: &'a [ResourceDescriptor], workload: &WorkloadRecord) -> Option<&'a ResourceDescriptor> {
    let binding = workload.resource_bindings.first()?;
    resources.iter().find(|resource| resource.id.as_str() == binding.resource_id.as_str())
}

fn invalid(workload: &WorkloadRecord, message: impl Into<String>) -> PeripheralRuntimeError {
    PeripheralRuntimeError::InvalidResourceActionWorkload { workload_id: workload.workload_id.as_str().to_string(), message: message.into() }
}

fn resource_action_request_from_workload(resource: &ResourceDescriptor, workload: &WorkloadRecord, lease: Option<&LeaseRecord>) -> Result<ResourceActionRequest, PeripheralRuntimeError> {
    validate_control_lease(resource, workload, lease)?;
    let spec = resource_action_spec_from_workload(workload)?;
    if resource.kind != ResourceKind::LemnosDevice {
        return Err(invalid(workload, format!("resource {} has no actions", resource.id.as_str())));
    }
    let device = resource.label(DEVICE_ID_LABEL).ok_or_else(|| invalid(workload, format!("resource {} has no {DEVICE_ID_LABEL} label", resource.id.as_str())))?;
    let capability = match spec {
        ResourceActionSpec::FanOverride(_) | ResourceActionSpec::FanRelease => FAN_OVERRIDE_ACTION,
        ResourceActionSpec::ControlSet { .. } => CONTROL_SET_ACTION,
    };
    if !resource.has_capability(capability) {
        let reason = if resource.has_capability(FAN_OVERRIDE_ACTION) { "the fan's only write is fan.override" } else { "the device does not offer it" };
        return Err(invalid(workload, format!("{} on {}: {reason}", spec.kind(), resource.id.as_str())));
    }
    Ok(ResourceActionRequest { device: device.to_string(), spec })
}

fn validate_control_lease(resource: &ResourceDescriptor, workload: &WorkloadRecord, lease: Option<&LeaseRecord>) -> Result<(), PeripheralRuntimeError> {
    match lease {
        Some(lease) if lease.holder_workload_id.as_ref().is_some_and(|holder| holder.as_str() == workload.workload_id.as_str()) => Ok(()),
        Some(lease) if lease.holder_workload_id.is_some() => {
            Err(invalid(workload, format!("resource {} is leased to {}", resource.id.as_str(), lease.holder_workload_id.as_ref().map(|holder| holder.as_str()).unwrap_or("unknown"))))
        }
        Some(_) => Err(invalid(workload, format!("resource {} lease holder mismatch", resource.id.as_str()))),
        None => Err(invalid(workload, format!("resource {} is not leased", resource.id.as_str()))),
    }
}

fn resource_action_spec_from_workload(workload: &WorkloadRecord) -> Result<ResourceActionSpec, PeripheralRuntimeError> {
    let config = workload.config.as_ref().ok_or_else(|| invalid(workload, "missing config payload"))?;
    let decoded: ResourceActionConfig = deserialize_config(&config.payload).map_err(|error| invalid(workload, format!("config decode failed: {error}")))?;
    let arg = decoded.arg;
    match decoded.action.kind {
        ResourceActionKind::FanOverride => FanOverrideRequest::from_args(arg.pwm, arg.duty, arg.duration_ms).map(ResourceActionSpec::FanOverride).map_err(|message| invalid(workload, message)),
        ResourceActionKind::FanRelease => Ok(ResourceActionSpec::FanRelease),
        ResourceActionKind::ControlSet => Ok(ResourceActionSpec::ControlSet {
            control: arg.control.filter(|control| !control.is_empty()).ok_or_else(|| invalid(workload, "missing arg.control"))?,
            value: arg.value.filter(|value| value.is_finite()).ok_or_else(|| invalid(workload, "missing arg.value"))?,
        }),
    }
}

/// Resolves on SIGINT or SIGTERM (what `systemctl stop` sends), so shutdown runs the same
/// cleanup either way.
async fn shutdown_requested() {
    let mut terminate = match signal::unix::signal(signal::unix::SignalKind::terminate()) {
        Ok(stream) => stream,
        Err(error) => {
            tracing::warn!(error = %error, "cannot listen for SIGTERM; only SIGINT stops the service");
            let _ = signal::ctrl_c().await;
            return;
        }
    };
    tokio::select! {
        _ = signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::DEFAULT_NODE_ID, resources::ResourceBuilder};
    use orion::control_plane::{AppliedClusterState, ClusterStateEnvelope, DesiredClusterState, LeaseState, ObservedClusterState, TypedConfigValue, WorkloadConfig};

    fn fan_resource() -> ResourceDescriptor {
        ResourceBuilder::new(NodeId::new(DEFAULT_NODE_ID), ResourceKind::LemnosDevice, "fan", "Fan")
            .expect("resource")
            .label(DEVICE_ID_LABEL, "fan")
            .capability(FAN_OVERRIDE_ACTION, Some("lemnosd"))
            .capability(FAN_RELEASE_ACTION, Some("lemnosd"))
            .build()
    }

    fn usb_power_resource() -> ResourceDescriptor {
        ResourceBuilder::new(NodeId::new(DEFAULT_NODE_ID), ResourceKind::LemnosDevice, "usb-a-power", "USB-A power")
            .expect("resource")
            .label(DEVICE_ID_LABEL, "usb-a-power")
            .capability(CONTROL_SET_ACTION, Some("lemnosd"))
            .build()
    }

    fn action_workload(id: &str, resource: &ResourceDescriptor, fields: &[(&str, TypedConfigValue)]) -> WorkloadRecord {
        let config = fields.iter().fold(WorkloadConfig::new(PERIPHERAL_RESOURCE_ACTION_RUNTIME_TYPE), |config, (key, value)| config.field(*key, value.clone()));
        WorkloadRecord::builder(orion::core::WorkloadId::new(id), orion::core::RuntimeType::new(PERIPHERAL_RESOURCE_ACTION_RUNTIME_TYPE), orion::core::ArtifactId::new("artifact.control"))
            .desired_state(DesiredState::Running)
            .assigned_to(orion::core::NodeId::new(DEFAULT_NODE_ID))
            .config(config)
            .bind_resource(resource.id.clone(), orion::core::NodeId::new(DEFAULT_NODE_ID))
            .build()
    }

    fn lease_for(resource: &ResourceDescriptor, workload: &WorkloadRecord) -> LeaseRecord {
        LeaseRecord::builder(resource.id.clone()).lease_state(LeaseState::Leased).holder_node(orion::core::NodeId::new(DEFAULT_NODE_ID)).holder_workload(workload.workload_id.clone()).build()
    }

    fn kind(value: &str) -> (&'static str, TypedConfigValue) {
        ("action.kind", TypedConfigValue::String(value.into()))
    }

    #[test]
    fn build_inventory_service_rejects_invalid_node_id() {
        let config = PeripheralConfig { node_id: "".into(), ..PeripheralConfig::default() };
        match build_inventory_service(&config) {
            Err(PeripheralRuntimeError::InvalidNodeId(_)) => {}
            Err(other) => panic!("unexpected error: {other}"),
            Ok(_) => panic!("invalid node id should fail"),
        }
    }

    #[test]
    fn build_inventory_service_registers_lemnosd_and_camera_probes() {
        let service = build_inventory_service(&PeripheralConfig::default()).expect("service");
        assert_eq!(service.probe_names(), vec!["lemnosd", "styx-capture"]);
        let service = build_inventory_service(&PeripheralConfig { enable_linux_probes: false, ..PeripheralConfig::default() }).expect("service");
        assert_eq!(service.probe_names(), vec!["lemnosd"]);
        assert!(service.refresh_report(1).snapshot.resources.is_empty(), "no lemnosd devices before it connects");
    }

    #[tokio::test]
    async fn refresh_handle_notifies_waiters() {
        let handle = RefreshHandle::default();
        let notify = handle.notify.clone();
        handle.request_refresh();
        tokio::time::timeout(std::time::Duration::from_millis(50), notify.notified()).await.expect("refresh handle should wake waiters");
    }

    #[test]
    fn is_local_peripheral_resource_action_workload_filters_by_runtime_state_and_node() {
        let workload = action_workload("workload.fan.override", &fan_resource(), &[kind("fan.release")]);
        assert!(is_local_peripheral_resource_action_workload(&workload, DEFAULT_NODE_ID));
        assert!(!is_local_peripheral_resource_action_workload(&workload, "other-node"));
    }

    #[test]
    fn fan_override_workloads_become_timed_duty_requests() {
        let fan = fan_resource();
        let workload = action_workload("workload.fan.override", &fan, &[kind("fan.override"), ("arg.pwm", TypedConfigValue::UInt(255)), ("arg.duration_ms", TypedConfigValue::UInt(5_000))]);
        let request = resource_action_request_from_workload(&fan, &workload, Some(&lease_for(&fan, &workload))).expect("request");
        assert_eq!(request.device, "fan");
        assert_eq!(request.spec, ResourceActionSpec::FanOverride(FanOverrideRequest { duty: 1.0, duration: StdDuration::from_secs(5) }));

        let workload = action_workload("workload.fan.duty", &fan, &[kind("fan.override"), ("arg.duty", TypedConfigValue::F64(0.4))]);
        let request = resource_action_request_from_workload(&fan, &workload, Some(&lease_for(&fan, &workload))).expect("request");
        assert_eq!(request.spec, ResourceActionSpec::FanOverride(FanOverrideRequest { duty: 0.4, duration: StdDuration::from_secs(60) }));

        let workload = action_workload("workload.fan.release", &fan, &[kind("fan.release")]);
        assert_eq!(resource_action_request_from_workload(&fan, &workload, Some(&lease_for(&fan, &workload))).expect("request").spec, ResourceActionSpec::FanRelease);
    }

    #[test]
    fn fan_overrides_are_bounded_to_ten_minutes() {
        let fan = fan_resource();
        let workload = action_workload("workload.fan.long", &fan, &[kind("fan.override"), ("arg.pwm", TypedConfigValue::UInt(200)), ("arg.duration_ms", TypedConfigValue::UInt(600_001))]);
        let error = resource_action_request_from_workload(&fan, &workload, Some(&lease_for(&fan, &workload))).expect_err("too long");
        assert!(matches!(error, PeripheralRuntimeError::InvalidResourceActionWorkload { ref message, .. } if message.contains("duration_ms")));
    }

    #[test]
    fn the_fan_takes_no_other_write_and_other_devices_no_fan_actions() {
        let fan = fan_resource();
        let workload = action_workload("workload.fan.set", &fan, &[kind("control.set"), ("arg.control", TypedConfigValue::String("duty".into())), ("arg.value", TypedConfigValue::F64(0.2))]);
        let error = resource_action_request_from_workload(&fan, &workload, Some(&lease_for(&fan, &workload))).expect_err("raw fan write");
        assert!(matches!(error, PeripheralRuntimeError::InvalidResourceActionWorkload { ref message, .. } if message.contains("only write is fan.override")));

        let usb = usb_power_resource();
        let workload = action_workload("workload.usb.override", &usb, &[kind("fan.override"), ("arg.pwm", TypedConfigValue::UInt(10))]);
        assert!(resource_action_request_from_workload(&usb, &workload, Some(&lease_for(&usb, &workload))).is_err());

        let workload = action_workload("workload.usb.set", &usb, &[kind("control.set"), ("arg.control", TypedConfigValue::String("level".into())), ("arg.value", TypedConfigValue::UInt(0))]);
        let request = resource_action_request_from_workload(&usb, &workload, Some(&lease_for(&usb, &workload))).expect("request");
        assert_eq!(request, ResourceActionRequest { device: "usb-a-power".into(), spec: ResourceActionSpec::ControlSet { control: "level".into(), value: 0.0 } });
    }

    #[test]
    fn resource_action_request_from_workload_requires_matching_lease_holder() {
        let fan = fan_resource();
        let workload = action_workload("workload.fan.release", &fan, &[kind("fan.release")]);
        let lease =
            LeaseRecord::builder(fan.id.clone()).lease_state(LeaseState::Leased).holder_node(orion::core::NodeId::new(DEFAULT_NODE_ID)).holder_workload(orion::core::WorkloadId::new("other")).build();
        let error = resource_action_request_from_workload(&fan, &workload, Some(&lease)).expect_err("lease mismatch");
        assert!(matches!(error, PeripheralRuntimeError::InvalidResourceActionWorkload { ref message, .. } if message.contains("leased to other")));
        assert!(resource_action_request_from_workload(&fan, &workload, None).is_err());
    }

    #[test]
    fn resource_action_request_from_workload_rejects_bad_and_unknown_args() {
        let fan = fan_resource();
        let workload = action_workload("workload.fan.bad", &fan, &[kind("fan.override"), ("arg.pwm", TypedConfigValue::String("bad".into()))]);
        assert!(resource_action_request_from_workload(&fan, &workload, Some(&lease_for(&fan, &workload))).is_err());

        let workload = action_workload("workload.fan.strict", &fan, &[kind("fan.override"), ("arg.pwm", TypedConfigValue::UInt(10)), ("arg.extra", TypedConfigValue::String("bad".into()))]);
        let error = resource_action_request_from_workload(&fan, &workload, Some(&lease_for(&fan, &workload))).expect_err("unknown fields should fail");
        assert!(matches!(
            error,
            PeripheralRuntimeError::InvalidResourceActionWorkload { ref message, .. }
                if message.contains("arg.extra") && message.contains("unknown field")
        ));

        let workload = action_workload("workload.gpio.write", &fan, &[kind("gpio.write"), ("arg.high", TypedConfigValue::Bool(true))]);
        assert!(resource_action_request_from_workload(&fan, &workload, Some(&lease_for(&fan, &workload))).is_err(), "raw GPIO/PWM/I2C/SPI actions are gone");
    }

    fn snapshot_with(workloads: Vec<WorkloadRecord>, leases: Vec<LeaseRecord>) -> StateSnapshot {
        let mut desired = DesiredClusterState::default();
        for workload in workloads {
            desired.workloads.insert(workload.workload_id.clone(), workload);
        }
        for lease in leases {
            desired.leases.insert(lease.resource_id.clone(), lease);
        }
        StateSnapshot { state: ClusterStateEnvelope::new(desired, ObservedClusterState::default(), AppliedClusterState::default()) }
    }

    fn test_runtime() -> PeripheralRuntime {
        PeripheralRuntime::from_parts(PeripheralConfig::default(), Arc::new(PeripheralInventoryService::new(NodeId::new(DEFAULT_NODE_ID))))
    }

    #[tokio::test]
    async fn reconcile_controls_records_failed_feedback_for_unleased_resource_action() {
        let fan = fan_resource();
        let inventory = DiscoverySnapshot::new(vec![fan.clone()]);
        let workload = action_workload("workload.fan.release", &fan, &[kind("fan.release")]);
        let snapshot = snapshot_with(vec![workload], Vec::new());
        let runtime = test_runtime();
        let mut state = RuntimeState::default();

        runtime.reconcile_controls(&inventory, &snapshot, &mut state).await;

        let feedback = state.resource_action_feedback.get(fan.id.as_str()).expect("failed feedback");
        let action = feedback.state.action_result.as_ref().expect("action result");
        assert_eq!(action.action_kind, "fan.release");
        assert_eq!(action.status, orion::control_plane::ResourceActionStatus::Failed);
        assert!(action.error.as_ref().is_some_and(|error| error.contains("not leased")));
        assert!(state.applied_actions.is_empty(), "a rejected workload is looked at again once its lease arrives");
    }

    #[tokio::test]
    async fn reconcile_controls_runs_each_action_workload_once() {
        let fan = fan_resource();
        let inventory = DiscoverySnapshot::new(vec![fan.clone()]);
        let workload = action_workload("workload.fan.override", &fan, &[kind("fan.override"), ("arg.pwm", TypedConfigValue::UInt(128))]);
        let lease = lease_for(&fan, &workload);
        let snapshot = snapshot_with(vec![workload], vec![lease]);
        let runtime = test_runtime();
        let mut state = RuntimeState::default();

        runtime.reconcile_controls(&inventory, &snapshot, &mut state).await;
        let action = state.resource_action_feedback.get(fan.id.as_str()).and_then(|feedback| feedback.state.action_result.clone()).expect("action result");
        assert_eq!(action.action_kind, "fan.override");
        assert!(action.error.as_ref().is_some_and(|error| error.contains("lemnosd")), "no lemnosd in this runtime: {action:?}");
        assert!(state.applied_actions.contains("workload.fan.override"));

        // Seen again (a lease update, a refresh): not run again.
        let observed_at = state.resource_action_feedback.get(fan.id.as_str()).expect("feedback").state.observed_at_ms;
        runtime.reconcile_controls(&inventory, &snapshot, &mut state).await;
        assert_eq!(state.resource_action_feedback.get(fan.id.as_str()).expect("feedback").state.observed_at_ms, observed_at);

        // Removed from the desired state: forgotten.
        runtime.reconcile_controls(&inventory, &snapshot_with(Vec::new(), Vec::new()), &mut state).await;
        assert!(state.applied_actions.is_empty());
        assert!(state.resource_action_feedback.is_empty());
    }
}
