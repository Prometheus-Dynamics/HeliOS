use std::{collections::BTreeMap, sync::Arc, time::Duration as StdDuration};

use orion::{
    client::{ActionReporter, ClientError, LocalNodeRuntime, LocalProviderEvent, LocalProviderService, LocalServiceRetryPolicy},
    control_plane::{ActionRequest, ActionTarget, LeaseRecord, config_json_value},
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
        CONTROL_SET_ACTION, FAN_OVERRIDE_ACTION, FAN_RELEASE_ACTION, FanOverrideRequest, LemnosdBridge, LemnosdOptions, ProviderHealth, RawAction,
        resources::{DEVICE_ID_LABEL, PROBE_NAME},
    },
    model::{NodeId, ObservedValue, ResourceActionOutcome, ResourceDescriptor, ResourceKind},
    provider::{CameraServices, OrionPeripheralPublisher, OrionPublishError, ResourceActionFeedback},
    resources::{CaptureProbe, DiscoveryContext, DiscoveryProbe, DiscoverySnapshot, PeripheralInventoryService},
};

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
    /// Raw GPIO, PWM, I2C or SPI access, on the `lemnos.raw` resource.
    Raw(RawAction),
}

impl ResourceActionSpec {
    fn kind(&self) -> &'static str {
        match self {
            Self::FanOverride(_) => FAN_OVERRIDE_ACTION,
            Self::FanRelease => FAN_RELEASE_ACTION,
            Self::ControlSet { .. } => CONTROL_SET_ACTION,
            Self::Raw(action) => action.kind(),
        }
    }
}

/// A resource action bound to its lemnosd device (none for raw access).
#[derive(Debug, Clone, PartialEq)]
struct ResourceActionRequest {
    device: Option<String>,
    spec: ResourceActionSpec,
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

#[derive(Debug, thiserror::Error)]
pub enum PeripheralRuntimeError {
    #[error("invalid node id '{0}'")]
    InvalidNodeId(String),
    #[error(transparent)]
    Orion(#[from] OrionPublishError),
    #[error(transparent)]
    Client(#[from] ClientError),
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
    /// An action request from Orion, to decode and run on its own task.
    Action {
        request: Box<ActionRequest>,
        reporter: ActionReporter,
    },
    /// An action finished: its result is the resource's `action_result` until the next one.
    ActionFinished {
        resource_id: String,
        feedback: ResourceActionFeedback,
    },
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
            Ok(lemnosd) => {
                DiscoverySnapshot::new(snapshot.resources.into_iter().filter(|resource| !matches!(resource.kind, ResourceKind::LemnosDevice | ResourceKind::LemnosRaw)).collect()).merge(lemnosd)
            }
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
        // The state snapshot that follows the leases is the readiness point; actions no longer
        // come from the desired state (they arrive as Orion action requests below).
        let _ = next_provider_event_retrying(&mut provider_subscription).await?;
        self.publish_snapshot(&current_snapshot, &current_leases, &state).await?;
        self.set_health(ProviderHealth::Ready);

        // Action requests: one watch on the provider, each request handed to the event loop, which
        // decodes it against the current inventory and runs it on its own task.
        let action_service = LocalProviderService::new(self.node_runtime.clone(), format!("{}-actions", self.publisher.client_name()), self.publisher.provider_identity_record())
            .with_retry_policy(LocalServiceRetryPolicy::fixed_delay(PROVIDER_EVENT_RETRY_DELAY));
        let mut actions = action_service.watch_action_requests().await?;
        let reporter = actions.reporter();
        let action_tx = event_tx.clone();
        let feedback_tx = event_tx.clone();
        tokio::spawn(async move {
            loop {
                match actions.next().await {
                    Ok(request) => {
                        if action_tx.send(RuntimeEvent::Action { request: Box::new(request), reporter: reporter.clone() }).is_err() {
                            return;
                        }
                    }
                    Err(error) => {
                        let _ = action_tx.send(RuntimeEvent::ProviderWatchStopped(error));
                        return;
                    }
                }
            }
        });

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
                            self.publish_snapshot(&current_snapshot, &current_leases, &state).await?;
                        }
                        Some(RuntimeEvent::LemnosdChanged) => {
                            current_snapshot = self.with_lemnosd_resources(current_snapshot);
                            self.publish_snapshot(&current_snapshot, &current_leases, &state).await?;
                        }
                        Some(RuntimeEvent::LeaseUpdate(leases)) => {
                            current_leases = leases;
                            self.publish_snapshot(&current_snapshot, &current_leases, &state).await?;
                        }
                        Some(RuntimeEvent::Action { request, reporter }) => {
                            self.dispatch_action(*request, reporter, &current_snapshot, feedback_tx.clone());
                        }
                        Some(RuntimeEvent::ActionFinished { resource_id, feedback }) => {
                            state.resource_action_feedback.insert(resource_id, feedback);
                            self.publish_snapshot(&current_snapshot, &current_leases, &state).await?;
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

    /// Decodes an action request against the inventory and runs it on its own task: actions run
    /// concurrently, and each is answered by its id (`reject` when it cannot run at all, `succeed`
    /// or `fail` otherwise). Its result becomes the resource's `action_result`.
    fn dispatch_action(&self, request: ActionRequest, reporter: ActionReporter, inventory: &DiscoverySnapshot, feedback_tx: mpsc::UnboundedSender<RuntimeEvent>) {
        let resource = match &request.target {
            ActionTarget::Resource(id) => inventory.resources.iter().find(|resource| resource.id.as_str() == id.as_str()).cloned(),
            _ => None,
        };
        let bridge = self.lemnosd.clone();
        tokio::spawn(async move {
            let action_id = request.action_id.clone();
            let Some(resource) = resource else {
                let _ = reporter.reject(action_id, format!("no helios-peripherals resource {}", request.target.id())).await;
                return;
            };
            let kind = request.name.clone();
            let decoded = match resource_action_request_from_request(&resource, &request) {
                Ok(decoded) => decoded,
                Err(reason) => {
                    let _ = reporter.reject(action_id, reason).await;
                    return;
                }
            };
            let outcome = match bridge {
                Some(bridge) => apply_action(&bridge, &resource, &decoded).await,
                None => Err("lemnosd is not configured".to_string()),
            };
            let feedback = match outcome {
                Ok(outcome) => {
                    let mut output = BTreeMap::new();
                    if let Some(value) = outcome.value.as_ref() {
                        output.insert("value".to_string(), crate::provider::observed_value(value));
                    }
                    let _ = reporter.succeed(action_id, output).await;
                    ResourceActionFeedback::applied(now_ms(), &outcome)
                }
                Err(error) => {
                    warn!(resource_id = %resource.id, action = %kind, error = %error, "peripheral resource action failed");
                    let _ = reporter.fail(action_id, error.clone()).await;
                    ResourceActionFeedback::failed(now_ms(), kind, error)
                }
            };
            let _ = feedback_tx.send(RuntimeEvent::ActionFinished { resource_id: resource.id.as_str().to_string(), feedback });
        });
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

async fn watch_provider_updates(mut subscription: orion::client::LocalProviderSubscription, event_tx: mpsc::UnboundedSender<RuntimeEvent>) -> Result<(), ClientError> {
    loop {
        match next_provider_event_retrying(&mut subscription).await? {
            LocalProviderEvent::BootstrapLeases(leases) | LocalProviderEvent::LeasesChanged { leases, .. } => {
                if event_tx.send(RuntimeEvent::LeaseUpdate(leases)).is_err() {
                    return Ok(());
                }
            }
            // Desired-state snapshots carry no work now: actions arrive as requests.
            LocalProviderEvent::BootstrapStateSnapshot(_) | LocalProviderEvent::StateSnapshot { .. } => {}
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
        startup_wait: StdDuration::from_millis(config.lemnosd_startup_wait_ms),
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

/// Runs one decoded action on lemnosd.
async fn apply_action(bridge: &LemnosdBridge, resource: &ResourceDescriptor, request: &ResourceActionRequest) -> Result<ResourceActionOutcome, String> {
    let device = request.device.as_deref().unwrap_or_default();
    let value = match &request.spec {
        ResourceActionSpec::FanOverride(override_request) => Some(ObservedValue::F64(bridge.fan_override(device, *override_request).await?)),
        ResourceActionSpec::FanRelease => {
            bridge.fan_release(device).await?;
            None
        }
        ResourceActionSpec::ControlSet { control, value } => Some(ObservedValue::F64(bridge.set_control(device, control, *value).await?)),
        ResourceActionSpec::Raw(action) => bridge.raw(action.clone()).await?,
    };
    Ok(ResourceActionOutcome::applied(resource.id.clone(), request.spec.kind(), value))
}

/// An action request, decoded for its resource: the device (lemnosd devices only) and the spec.
/// The request's arguments are the action's `arg` fields; dotted names are nested, as they were
/// for workload configs (`ops.0.write` is `ops[0].write`).
fn resource_action_request_from_request(resource: &ResourceDescriptor, request: &ActionRequest) -> Result<ResourceActionRequest, String> {
    let kind = request.name.as_str();
    let arg = config_json_value(&request.args).map_err(|error| format!("{kind}: arguments: {error}"))?;
    let spec = resource_action_spec(kind, arg).map_err(|message| format!("{kind}: {message}"))?;
    let capability = match &spec {
        ResourceActionSpec::FanOverride(_) | ResourceActionSpec::FanRelease => FAN_OVERRIDE_ACTION,
        ResourceActionSpec::ControlSet { .. } => CONTROL_SET_ACTION,
        ResourceActionSpec::Raw(action) => action.kind(),
    };
    if !resource.has_capability(capability) {
        let reason = if resource.has_capability(FAN_OVERRIDE_ACTION) {
            "the fan's only write is fan.override"
        } else if matches!(spec, ResourceActionSpec::Raw(_)) {
            "raw GPIO, PWM, I2C and SPI actions go to the lemnos.raw resource"
        } else {
            "the resource does not offer it"
        };
        return Err(format!("{kind} on {}: {reason}", resource.id.as_str()));
    }
    let device = match resource.kind {
        ResourceKind::LemnosDevice => Some(resource.label(DEVICE_ID_LABEL).ok_or_else(|| format!("resource {} has no {DEVICE_ID_LABEL} label", resource.id.as_str()))?.to_string()),
        ResourceKind::LemnosRaw => None,
        _ => return Err(format!("resource {} has no actions", resource.id.as_str())),
    };
    Ok(ResourceActionRequest { device, spec })
}

fn resource_action_spec(kind: &str, arg: serde_json::Value) -> Result<ResourceActionSpec, String> {
    if let Some(raw) = RawAction::from_kind(kind, arg.clone()) {
        return raw.map(ResourceActionSpec::Raw);
    }
    if ![FAN_OVERRIDE_ACTION, FAN_RELEASE_ACTION, CONTROL_SET_ACTION].contains(&kind) {
        return Err(format!("unknown action kind {kind:?}"));
    }
    let arg: ResourceActionArgs = match arg {
        serde_json::Value::Null => ResourceActionArgs::default(),
        arg => serde_json::from_value(arg).map_err(|error| format!("arguments: {error}"))?,
    };
    match kind {
        FAN_OVERRIDE_ACTION => FanOverrideRequest::from_args(arg.pwm, arg.duty, arg.duration_ms).map(ResourceActionSpec::FanOverride),
        FAN_RELEASE_ACTION => Ok(ResourceActionSpec::FanRelease),
        CONTROL_SET_ACTION => Ok(ResourceActionSpec::ControlSet {
            control: arg.control.filter(|control| !control.is_empty()).ok_or_else(|| "missing control".to_string())?,
            value: arg.value.filter(|value| value.is_finite()).ok_or_else(|| "missing value".to_string())?,
        }),
        other => Err(format!("unknown action kind {other:?}")),
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
    use orion::control_plane::TypedConfigValue;

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

    fn raw_resource() -> ResourceDescriptor {
        let mut builder = ResourceBuilder::new(NodeId::new(DEFAULT_NODE_ID), ResourceKind::LemnosRaw, "io", "Raw").expect("resource");
        for action in crate::lemnosd::raw::RAW_ACTIONS {
            builder = builder.capability(action, Some("lemnosd"));
        }
        builder.build()
    }

    /// An action request as Orion delivers it: the arguments are typed, dotted names are nested.
    fn request(resource: &ResourceDescriptor, name: &str, args: &[(&str, TypedConfigValue)]) -> ActionRequest {
        args.iter().fold(ActionRequest::new("action-1", ActionTarget::Resource(resource.id.clone()), name), |request, (key, value)| request.with_arg(*key, value.clone()))
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
    fn fan_override_requests_become_timed_duty_requests() {
        let fan = fan_resource();
        let decoded =
            resource_action_request_from_request(&fan, &request(&fan, "fan.override", &[("pwm", TypedConfigValue::UInt(255)), ("duration_ms", TypedConfigValue::UInt(5_000))])).expect("request");
        assert_eq!(decoded.device.as_deref(), Some("fan"));
        assert_eq!(decoded.spec, ResourceActionSpec::FanOverride(FanOverrideRequest { duty: 1.0, duration: StdDuration::from_secs(5) }));

        let decoded = resource_action_request_from_request(&fan, &request(&fan, "fan.override", &[("duty", TypedConfigValue::F64(0.4))])).expect("request");
        assert_eq!(decoded.spec, ResourceActionSpec::FanOverride(FanOverrideRequest { duty: 0.4, duration: StdDuration::from_secs(60) }));

        let decoded = resource_action_request_from_request(&fan, &request(&fan, "fan.release", &[])).expect("request");
        assert_eq!(decoded.spec, ResourceActionSpec::FanRelease);
    }

    #[test]
    fn fan_overrides_are_bounded_to_ten_minutes() {
        let fan = fan_resource();
        assert!(resource_action_request_from_request(&fan, &request(&fan, "fan.override", &[("duty", TypedConfigValue::F64(0.5)), ("duration_ms", TypedConfigValue::UInt(600_001))])).is_err());
        assert!(resource_action_request_from_request(&fan, &request(&fan, "fan.override", &[("duty", TypedConfigValue::F64(0.5)), ("duration_ms", TypedConfigValue::UInt(600_000))])).is_ok());
    }

    #[test]
    fn the_fan_takes_no_other_write_and_other_devices_no_fan_actions() {
        let fan = fan_resource();
        let usb = usb_power_resource();
        let err = resource_action_request_from_request(&fan, &request(&fan, "control.set", &[("control", TypedConfigValue::String("level".into())), ("value", TypedConfigValue::F64(1.0))]))
            .expect_err("fan control.set");
        assert!(err.contains("fan.override"), "{err}");
        let err = resource_action_request_from_request(&usb, &request(&usb, "fan.override", &[("duty", TypedConfigValue::F64(0.5))])).expect_err("fan on usb");
        assert!(err.contains("does not offer"), "{err}");
        let decoded = resource_action_request_from_request(&usb, &request(&usb, "control.set", &[("control", TypedConfigValue::String("level".into())), ("value", TypedConfigValue::F64(0.0))]))
            .expect("control.set");
        assert_eq!(decoded.spec, ResourceActionSpec::ControlSet { control: "level".into(), value: 0.0 });
    }

    #[test]
    fn unknown_kinds_and_bad_arguments_are_refused() {
        let fan = fan_resource();
        assert!(resource_action_request_from_request(&fan, &request(&fan, "reboot", &[])).is_err());
        assert!(resource_action_request_from_request(&fan, &request(&fan, "fan.override", &[("pwm", TypedConfigValue::UInt(256))])).is_err());
        assert!(resource_action_request_from_request(&fan, &request(&fan, "fan.override", &[("unknown", TypedConfigValue::UInt(1))])).is_err());
    }

    #[test]
    fn raw_actions_go_to_the_raw_resource_only() {
        let raw = raw_resource();
        let decoded = resource_action_request_from_request(
            &raw,
            &request(&raw, "gpio.claim", &[("line", TypedConfigValue::String("aux".into())), ("direction", TypedConfigValue::String("output".into())), ("value", TypedConfigValue::Bool(true))]),
        )
        .expect("request");
        assert_eq!(decoded.device, None);
        assert!(matches!(decoded.spec, ResourceActionSpec::Raw(RawAction::GpioClaim { .. })));

        // An I2C transaction with its ops as dotted fields, the bytes as Orion's Bytes values.
        let decoded = resource_action_request_from_request(
            &raw,
            &request(
                &raw,
                "i2c.transfer",
                &[
                    ("bus", TypedConfigValue::String("i2c-1".into())),
                    ("address", TypedConfigValue::UInt(0x50)),
                    ("ops.0.write", TypedConfigValue::Bytes(vec![0x10])),
                    ("ops.1.read", TypedConfigValue::UInt(2)),
                ],
            ),
        )
        .expect("i2c request");
        assert!(matches!(decoded.spec, ResourceActionSpec::Raw(RawAction::I2cTransfer { .. })), "{decoded:?}");

        let err = resource_action_request_from_request(&fan_resource(), &request(&fan_resource(), "gpio.get", &[("claim", TypedConfigValue::String("gpio-1".into()))])).expect_err("raw on fan");
        assert!(err.contains("gpio.get"), "{err}");
    }
}
