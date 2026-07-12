use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use orion::{
    client::{ClientError, LocalNodeRuntime, LocalProviderEvent, LocalProviderService, LocalServiceRetryPolicy},
    control_plane::{DesiredState, LeaseRecord, StateSnapshot, WorkloadRecord, deserialize_config},
    core::Revision,
};
use serde::Deserialize;
use styx::watch::{CompositeWatcher, LinuxVideoFsWatcher, WatchRuntime};
use styx::{
    BackendHandle, BackendKind, ProbedBackend, ProbedDevice,
    core::buffer::FrameLease,
    prelude::{CaptureHandle, CaptureRequest, CaptureStartPolicy, RecvOutcome, StyxConfig},
    probe_all_with_errors,
};
use tokio::{
    signal,
    sync::{Notify, mpsc},
    task::JoinHandle,
    time::{Duration, sleep, timeout},
};
use tracing::{info, warn};

use crate::{
    config::PeripheralConfig,
    model::{
        GpioControl, GpioControlRequest, GpioDirection, I2cControl, I2cControlRequest, NodeId, PwmControl, PwmControlRequest, ResourceControlRequest, ResourceDescriptor, SpiControl, SpiControlRequest,
    },
    provider::streams::PeripheralStreamWriter,
    provider::{OrionPeripheralPublisher, OrionPublishError, ResourceActionFeedback},
    resources::{DiscoverySnapshot, LemnosPeripheralStack, PeripheralInventoryService},
    workloads::{PeripheralManager, ResourceControlError},
};

const PERIPHERAL_RESOURCE_ACTION_RUNTIME_TYPE: &str = "helios.peripheral.resource_action.v1";
const PROVIDER_EVENT_RETRY_DELAY: Duration = Duration::from_millis(200);
const DEFAULT_CAPTURE_QUEUE_DEPTH: usize = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
enum ResourceActionSpec {
    GpioRead,
    GpioWrite { high: bool },
    GpioConfigureDirection { direction: GpioDirection, initial_high: Option<bool> },
    PwmEnable { enabled: bool },
    PwmSetPeriodNs { period_ns: u64 },
    PwmSetDutyCycleNs { duty_cycle_ns: u64 },
    PwmConfigure { period_ns: u64, duty_cycle_ns: u64, enabled: bool },
    I2cRead { len: usize },
    I2cWrite { bytes: Vec<u8> },
    I2cWriteRead { write: Vec<u8>, read_len: usize },
    SpiTransfer { bytes: Vec<u8> },
    SpiWrite { bytes: Vec<u8> },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum GpioDirectionConfig {
    Input,
    Output,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
enum ResourceActionKind {
    #[serde(rename = "gpio.read")]
    GpioRead,
    #[serde(rename = "gpio.write")]
    GpioWrite,
    #[serde(rename = "gpio.configure_direction")]
    GpioConfigureDirection,
    #[serde(rename = "pwm.enable")]
    PwmEnable,
    #[serde(rename = "pwm.set_period_ns")]
    PwmSetPeriodNs,
    #[serde(rename = "pwm.set_duty_cycle_ns")]
    PwmSetDutyCycleNs,
    #[serde(rename = "pwm.configure")]
    PwmConfigure,
    #[serde(rename = "i2c.read")]
    I2cRead,
    #[serde(rename = "i2c.write")]
    I2cWrite,
    #[serde(rename = "i2c.write_read")]
    I2cWriteRead,
    #[serde(rename = "spi.transfer")]
    SpiTransfer,
    #[serde(rename = "spi.write")]
    SpiWrite,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ResourceActionArgs {
    high: Option<bool>,
    direction: Option<GpioDirectionConfig>,
    initial_high: Option<bool>,
    enabled: Option<bool>,
    period_ns: Option<u64>,
    duty_cycle_ns: Option<u64>,
    len: Option<u64>,
    bytes: Option<Vec<u8>>,
    write: Option<Vec<u8>>,
    read_len: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceActionTag {
    kind: ResourceActionKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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
    #[error("failed to initialize Lemnos runtime: {0}")]
    LemnosInit(String),
    #[error(transparent)]
    Orion(#[from] OrionPublishError),
    #[error(transparent)]
    Client(#[from] ClientError),
    #[error("invalid peripheral resource action workload '{workload_id}': {message}")]
    InvalidResourceActionWorkload { workload_id: String, message: String },
    #[error(transparent)]
    Control(#[from] ResourceControlError),
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
    LeaseUpdate(Vec<LeaseRecord>),
    DesiredStateSnapshot(StateSnapshot),
    ProviderWatchStopped(ClientError),
    RefreshWatchStopped(String),
}

const ORION_IPC_RETRY_DELAY: Duration = Duration::from_secs(2);

pub struct PeripheralRuntime {
    config: PeripheralConfig,
    inventory: Arc<PeripheralInventoryService>,
    publisher: OrionPeripheralPublisher,
    refresh_handle: RefreshHandle,
    manager: PeripheralManager,
    node_runtime: LocalNodeRuntime,
    lemnos: Option<LemnosPeripheralStack>,
}

#[derive(Default)]
struct RuntimeState {
    resource_action_feedback: BTreeMap<String, ResourceActionFeedback>,
}

impl PeripheralRuntime {
    pub fn from_config(config: PeripheralConfig) -> Result<Self, PeripheralRuntimeError> {
        let (inventory, manager, lemnos) = build_runtime_components(&config)?;
        Ok(Self {
            node_runtime: LocalNodeRuntime::new(config.orion_ipc_socket_path.clone(), config.orion_ipc_stream_socket_path.clone()),
            publisher: OrionPeripheralPublisher::from_config(&config),
            refresh_handle: RefreshHandle::default(),
            config,
            inventory: Arc::new(inventory),
            manager,
            lemnos: Some(lemnos),
        })
    }

    pub fn from_parts(config: PeripheralConfig, inventory: Arc<PeripheralInventoryService>, manager: PeripheralManager) -> Self {
        let node_runtime = LocalNodeRuntime::new(config.orion_ipc_socket_path.clone(), config.orion_ipc_stream_socket_path.clone());
        let publisher = OrionPeripheralPublisher::from_config(&config);
        Self { config, inventory, publisher, refresh_handle: RefreshHandle::default(), manager, node_runtime, lemnos: None }
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
                None => warn!(probe = %probe.probe, discovered_resources = probe.discovered_resources, "peripheral inventory probe completed"),
            }
        }
        self.publish_snapshot(&report.snapshot, leases, state).await?;
        Ok(report.snapshot)
    }

    async fn publish_snapshot(&self, snapshot: &DiscoverySnapshot, leases: &[LeaseRecord], state: &RuntimeState) -> Result<(), PeripheralRuntimeError> {
        self.publisher.publish_snapshot_with_feedback(snapshot, leases, &state.resource_action_feedback).await?;
        Ok(())
    }

    pub async fn run_until_stopped(&self) -> Result<(), PeripheralRuntimeError> {
        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<RuntimeEvent>();
        let refresh_notify = self.refresh_handle.notify.clone();
        let refresh_event_tx = event_tx.clone();

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

        let _lemnos_hotplug = self.spawn_lemnos_hotplug_task(event_tx.clone());
        let _styx_watch = self.spawn_styx_watch_task(event_tx.clone());
        let _config_refresh_watchers = spawn_config_refresh_watchers(&self.config, event_tx.clone());

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
        let mut capture_stream_publishers = BTreeMap::<String, JoinHandle<()>>::new();
        self.sync_capture_stream_publishers(&current_snapshot, &mut capture_stream_publishers);
        let mut current_state_snapshot = match next_provider_event_retrying(&mut provider_subscription).await? {
            LocalProviderEvent::BootstrapStateSnapshot(snapshot) | LocalProviderEvent::StateSnapshot { snapshot, .. } => {
                self.reconcile_controls(&current_snapshot, &snapshot, &mut state)?;
                self.publish_snapshot(&current_snapshot, &current_leases, &state).await?;
                Some(snapshot)
            }
            LocalProviderEvent::BootstrapLeases(_) | LocalProviderEvent::LeasesChanged { .. } => None,
        };

        let provider_watch_error_tx = event_tx.clone();
        tokio::spawn(async move {
            if let Err(error) = watch_provider_updates(provider_subscription, event_tx).await {
                let _ = provider_watch_error_tx.send(RuntimeEvent::ProviderWatchStopped(error));
            }
        });

        loop {
            tokio::select! {
                _ = signal::ctrl_c() => {
                    abort_capture_stream_publishers(&mut capture_stream_publishers);
                    return Ok(());
                }
                maybe_event = event_rx.recv() => {
                    match maybe_event {
                        Some(RuntimeEvent::RefreshRequested) => {
                            current_snapshot = self.refresh_inventory_with_state(&current_leases, &state).await?;
                            self.sync_capture_stream_publishers(&current_snapshot, &mut capture_stream_publishers);
                            if let Some(snapshot) = current_state_snapshot.as_ref() {
                                self.reconcile_controls(&current_snapshot, snapshot, &mut state)?;
                                self.publish_snapshot(&current_snapshot, &current_leases, &state).await?;
                            }
                        }
                        Some(RuntimeEvent::LeaseUpdate(leases)) => {
                            current_leases = leases;
                            self.publish_snapshot(&current_snapshot, &current_leases, &state).await?;
                            if let Some(snapshot) = current_state_snapshot.as_ref() {
                                self.reconcile_controls(&current_snapshot, snapshot, &mut state)?;
                                self.publish_snapshot(&current_snapshot, &current_leases, &state).await?;
                            }
                        }
                        Some(RuntimeEvent::DesiredStateSnapshot(snapshot)) => {
                            self.reconcile_controls(&current_snapshot, &snapshot, &mut state)?;
                            self.publish_snapshot(&current_snapshot, &current_leases, &state).await?;
                            current_state_snapshot = Some(snapshot);
                        }
                        Some(RuntimeEvent::ProviderWatchStopped(error)) => warn!(node_id = %self.config.node_id, error = %error, "provider watch stopped"),
                        Some(RuntimeEvent::RefreshWatchStopped(error)) => warn!(node_id = %self.config.node_id, error = %error, "refresh watch stopped"),
                        None => return Ok(()),
                    }
                }
            }
        }
    }

    fn reconcile_controls(&self, inventory: &DiscoverySnapshot, snapshot: &StateSnapshot, state: &mut RuntimeState) -> Result<(), PeripheralRuntimeError> {
        let resources = &inventory.resources;
        let leases = &snapshot.state.desired.leases;
        let active_resource_action_resource_ids = snapshot
            .state
            .desired
            .workloads
            .values()
            .filter(|workload| is_local_peripheral_resource_action_workload(workload, &self.config.node_id))
            .filter_map(|workload| bound_resource(resources, workload))
            .map(|resource| resource.id.as_str().to_string())
            .collect::<BTreeSet<_>>();

        state.resource_action_feedback.retain(|resource_id, _| resources.iter().any(|resource| resource.id.as_str() == resource_id) && active_resource_action_resource_ids.contains(resource_id));

        for workload in snapshot.state.desired.workloads.values() {
            if !is_local_peripheral_resource_action_workload(workload, &self.config.node_id) {
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
                    let observed_at_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
                    let feedback = ResourceActionFeedback::failed(observed_at_ms, workload_action_kind(workload), error.to_string());
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
            let observed_at_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
            let feedback = match self.manager.apply(resources, &request) {
                Ok(result) => ResourceActionFeedback::applied(observed_at_ms, &result),
                Err(error) => {
                    let feedback = ResourceActionFeedback::failed(observed_at_ms, request.action_kind(), error.to_string());
                    state.resource_action_feedback.insert(resource.id.as_str().to_string(), feedback);
                    continue;
                }
            };
            state.resource_action_feedback.insert(resource.id.as_str().to_string(), feedback);
        }
        Ok(())
    }

    fn sync_capture_stream_publishers(&self, snapshot: &DiscoverySnapshot, publishers: &mut BTreeMap<String, JoinHandle<()>>) {
        let wanted = snapshot.resources.iter().filter(|resource| should_publish_capture_channel(resource)).map(|resource| resource.id.as_str().to_string()).collect::<BTreeSet<_>>();
        publishers.retain(|resource_id, handle| {
            if wanted.contains(resource_id) {
                true
            } else {
                handle.abort();
                false
            }
        });
        for resource in snapshot.resources.iter().filter(|resource| should_publish_capture_channel(resource)) {
            if publishers.contains_key(resource.id.as_str()) {
                continue;
            }
            let Some(stream_path) = self.publisher.derived_channel_path(resource) else {
                continue;
            };
            publishers.insert(resource.id.as_str().to_string(), spawn_capture_stream_publisher(self.config.node_id.clone(), resource.clone(), stream_path));
        }
    }

    fn spawn_lemnos_hotplug_task(&self, event_tx: mpsc::UnboundedSender<RuntimeEvent>) -> Option<JoinHandle<()>> {
        let lemnos = self.lemnos.clone()?;
        let node_id = self.config.node_id.clone();
        Some(tokio::spawn(async move {
            let mut watcher = match lemnos.hotplug_watcher() {
                Ok(watcher) => watcher,
                Err(error) => {
                    let _ = event_tx.send(RuntimeEvent::RefreshWatchStopped(format!("lemnos hotplug watcher init failed: {error}")));
                    return;
                }
            };
            loop {
                let observed_at_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
                match lemnos.poll_hotplug_refresh(&mut watcher, observed_at_ms) {
                    Ok(true) => {
                        if event_tx.send(RuntimeEvent::RefreshRequested).is_err() {
                            return;
                        }
                    }
                    Ok(false) => {}
                    Err(error) => {
                        let _ = event_tx.send(RuntimeEvent::RefreshWatchStopped(format!("lemnos hotplug watcher on {node_id}: {error}")));
                        return;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        }))
    }

    fn spawn_styx_watch_task(&self, event_tx: mpsc::UnboundedSender<RuntimeEvent>) -> Option<JoinHandle<()>> {
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
                if event_tx.send(RuntimeEvent::DesiredStateSnapshot(snapshot)).is_err() {
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

const CAPTURE_SESSION_RETRY_DELAY: Duration = Duration::from_millis(200);
const CAPTURE_SESSION_FIRST_FRAME_TIMEOUT: Duration = Duration::from_millis(1500);
const CAPTURE_STYX_METRICS_INTERVAL_FRAMES: u64 = 30;

fn spawn_capture_stream_publisher(node_id: String, resource: ResourceDescriptor, stream_path: PathBuf) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut writer = match PeripheralStreamWriter::create_capture_channel(&stream_path, &node_id, &resource) {
            Ok(writer) => writer,
            Err(error) => {
                warn!(resource_id = resource.id.as_str(), path = %stream_path.display(), error = %error, "failed to create capture stream publisher");
                return;
            }
        };
        let mut sequence = 0_u64;
        let mut published_frame = false;
        loop {
            match start_capture_stream_session(&resource).await {
                Ok((handle, first_frame)) => {
                    if let Err(error) = publish_captured_frame(&mut writer, &resource, &stream_path, &mut sequence, &first_frame) {
                        warn!(resource_id = resource.id.as_str(), error = %error, "failed to publish first capture frame");
                    } else {
                        published_frame = true;
                        log_capture_styx_metrics(&handle, &resource, sequence);
                    }
                    drop(first_frame);

                    let handle = handle;
                    loop {
                        match handle.recv_async().await {
                            RecvOutcome::Data(frame) => {
                                if let Err(error) = publish_captured_frame(&mut writer, &resource, &stream_path, &mut sequence, &frame) {
                                    warn!(resource_id = resource.id.as_str(), error = %error, "failed to publish capture stream frame");
                                } else {
                                    published_frame = true;
                                    log_capture_styx_metrics(&handle, &resource, sequence);
                                }
                            }
                            RecvOutcome::Empty => continue,
                            RecvOutcome::Closed => {
                                warn!(resource_id = resource.id.as_str(), "capture stream closed; restarting session");
                                break;
                            }
                        }
                    }
                }
                Err(error) => {
                    sequence += 1;
                    let produced_at_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
                    warn!(resource_id = resource.id.as_str(), error = %error, "failed to start persistent capture session");
                    if !published_frame && let Err(error) = writer.publish_capture_heartbeat(&resource, sequence, produced_at_ms) {
                        warn!(resource_id = resource.id.as_str(), path = %stream_path.display(), error = %error, "failed to publish capture stream heartbeat");
                    }
                }
            }
            sleep(CAPTURE_SESSION_RETRY_DELAY).await;
        }
    })
}

fn log_capture_styx_metrics(handle: &CaptureHandle, resource: &ResourceDescriptor, sequence: u64) {
    if sequence == 0 || sequence % CAPTURE_STYX_METRICS_INTERVAL_FRAMES != 0 {
        return;
    }

    let health = handle.health_report();
    let memory = handle.memory_stats();
    let runtime_memory = handle.runtime_memory_report();
    let capture = handle.metrics().snapshot();
    let external_current_buffers = memory.external_backings.iter().map(|stats| stats.current_buffers).sum::<u64>();
    let external_current_bytes = memory.external_backings.iter().map(|stats| stats.current_bytes).sum::<u64>();
    let external_peak_buffers = memory.external_backings.iter().map(|stats| stats.peak_buffers).sum::<u64>();
    let external_peak_bytes = memory.external_backings.iter().map(|stats| stats.peak_bytes).sum::<u64>();

    warn!(
        resource_id = resource.id.as_str(),
        sequence,
        backend = ?handle.backend(),
        mode = ?handle.mode(),
        interval = ?handle.interval(),
        capture_samples = capture.samples,
        capture_total_samples = capture.total_samples,
        capture_fps = capture.fps,
        capture_wait_last_ms = capture.last_millis,
        capture_wait_avg_ms = capture.avg_millis,
        capture_wait_p50_ms = health.capture_wait_p50_ms,
        capture_wait_p95_ms = health.capture_wait_p95_ms,
        capture_queue_depth = health.capture_queue_depth,
        capture_queue_capacity = health.capture_queue_capacity,
        capture_backpressure_count = health.capture_backpressure_count,
        capture_drop_count = health.drop_count,
        capture_async_send_waits = health.capture_async_send_waits,
        capture_async_recv_waits = health.capture_async_recv_waits,
        capture_async_send_wakes = health.capture_async_send_wakes,
        capture_async_recv_wakes = health.capture_async_recv_wakes,
        external_current_buffers,
        external_current_mib = bytes_to_mib(external_current_bytes),
        external_peak_buffers,
        external_peak_mib = bytes_to_mib(external_peak_bytes),
        external_backings = ?memory.external_backings,
        capture_queue_memory = ?memory.capture_queue,
        transform_pool = ?memory.transform_pool,
        shared_decode_pool = ?memory.shared_decode_pool,
        shared_encode_pool = ?memory.shared_encode_pool,
        process_pss_mib = runtime_memory.process.pss_bytes.map(bytes_to_mib),
        process_rss_mib = runtime_memory.process.rss_bytes.map(bytes_to_mib),
        process_private_clean_mib = runtime_memory.process.private_clean_bytes.map(bytes_to_mib),
        process_private_dirty_mib = runtime_memory.process.private_dirty_bytes.map(bytes_to_mib),
        process_dmabuf_fd_count = runtime_memory.fds.dmabuf.fd_count,
        process_dmabuf_unique_buffers = runtime_memory.fds.dmabuf.unique_buffers,
        process_dmabuf_total_mib = bytes_to_mib(runtime_memory.fds.dmabuf.total_bytes),
        process_dmabuf_exporters = ?runtime_memory.fds.dmabuf.exporters,
        fd_total = runtime_memory.fds.total,
        fd_classes = ?runtime_memory.fds.classes,
        kernel_dmabuf_total_buffers = runtime_memory.kernel_dmabuf.total_buffers,
        kernel_dmabuf_total_mib = runtime_memory.kernel_dmabuf.total_bytes.map(bytes_to_mib),
        kernel_dmabuf_exporters = ?runtime_memory.kernel_dmabuf.exporters,
        memory_mappings = ?runtime_memory.mappings,
        unexplained_pss_mib = runtime_memory.unexplained_pss_bytes.map(bytes_to_mib),
        memory_warnings = ?runtime_memory.warnings,
        drop_reasons = ?health.drop_reasons,
        recent_stage_errors = ?health.recent_stage_errors,
        capture_retries = ?health.capture_retries,
        "styx capture health"
    );
}

fn bytes_to_mib(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0
}

fn publish_captured_frame(writer: &mut PeripheralStreamWriter, resource: &ResourceDescriptor, stream_path: &Path, sequence: &mut u64, frame: &FrameLease) -> Result<(), String> {
    *sequence += 1;
    writer.publish_capture_frame(frame).map_err(|error| format!("failed to publish frame for resource '{}' at '{}': {error}", resource.id.as_str(), stream_path.display()))
}

async fn start_capture_stream_session(resource: &ResourceDescriptor) -> Result<(CaptureHandle, FrameLease), String> {
    let device = find_capture_device(resource)?;
    let backend = select_capture_backend(&device, resource)?;
    let capture_queue_depth = capture_queue_depth();
    let capture_max_width = capture_optional_u32("HELIOS_CAPTURE_MAX_WIDTH");
    let capture_max_height = capture_optional_u32("HELIOS_CAPTURE_MAX_HEIGHT");
    let capture_config = StyxConfig::new().capture_queue_depth(capture_queue_depth);
    let mut mode_candidates = backend.descriptor.modes.clone();
    let unfiltered_mode_candidates = mode_candidates.clone();
    mode_candidates.retain(|mode| {
        let resolution = mode.id.format.resolution;
        capture_max_width.is_none_or(|max_width| resolution.width.get() <= max_width) && capture_max_height.is_none_or(|max_height| resolution.height.get() <= max_height)
    });
    if mode_candidates.is_empty() {
        warn!(resource_id = resource.id.as_str(), capture_max_width, capture_max_height, "capture mode filter matched no modes; falling back to all advertised modes");
        mode_candidates = unfiltered_mode_candidates;
    }
    mode_candidates.sort_by_key(|mode| {
        let resolution = mode.id.format.resolution;
        (capture_mode_rank(mode), std::cmp::Reverse(u64::from(resolution.width.get()) * u64::from(resolution.height.get())))
    });

    let mut last_error = String::from("no decodable capture mode succeeded");
    for mode in mode_candidates {
        let handle = match CaptureRequest::new(&device).backend(backend.kind).mode(mode.id.clone()).config(capture_config.clone()).start_with_policy(CaptureStartPolicy::resilient()) {
            Ok(handle) => handle,
            Err(error) => {
                last_error = error.to_string();
                continue;
            }
        };
        let first_frame = match timeout(CAPTURE_SESSION_FIRST_FRAME_TIMEOUT, handle.recv_async()).await {
            Ok(RecvOutcome::Data(frame)) => frame,
            Ok(RecvOutcome::Empty) => {
                handle.stop();
                last_error = format!("capture mode {:?} produced no frame", mode.id.format.code);
                continue;
            }
            Ok(RecvOutcome::Closed) => {
                handle.stop();
                last_error = format!("capture mode {:?} closed before first frame", mode.id.format.code);
                continue;
            }
            Err(_) => {
                handle.stop();
                last_error = format!("capture mode {:?} timed out waiting for first frame", mode.id.format.code);
                continue;
            }
        };
        info!(
            resource_id = resource.id.as_str(),
            backend = ?backend.kind,
            format = %mode.id.format.code,
            width = mode.id.format.resolution.width.get(),
            height = mode.id.format.resolution.height.get(),
            fps = mode.id.interval.as_ref().map(|interval| interval.fps()),
            capture_queue_depth,
            capture_max_width,
            capture_max_height,
            "capture stream started"
        );
        return Ok((handle, first_frame));
    }

    Err(last_error)
}

fn capture_queue_depth() -> usize {
    std::env::var("HELIOS_CAPTURE_QUEUE_DEPTH").ok().and_then(|value| value.parse::<usize>().ok()).filter(|depth| *depth > 0).unwrap_or(DEFAULT_CAPTURE_QUEUE_DEPTH)
}

fn capture_optional_u32(name: &str) -> Option<u32> {
    std::env::var(name).ok().and_then(|value| value.parse::<u32>().ok()).filter(|value| *value > 0)
}

fn capture_mode_rank(mode: &styx::prelude::Mode) -> u8 {
    match mode.id.format.code.to_string().as_str() {
        "NV12" => 0,
        "RGB3" | "RGB4" | "BGR3" | "BGR4" | "YUYV" | "UYVY" | "NV21" | "YU12" | "YV12" | "GREY" => 1,
        code if code.contains("RAW") || code.contains("SRG") || code.contains("SBG") || code.contains("BYR") => 2,
        _ => 3,
    }
}

fn find_capture_device(resource: &ResourceDescriptor) -> Result<ProbedDevice, String> {
    let mut probe = probe_all_with_errors();
    for error in probe.errors.drain(..) {
        warn!(resource_id = resource.id.as_str(), error = %error, "styx capture probe error during frame capture");
    }
    for device in styx::prelude::probe_libcamera() {
        let device_id = device.id.clone();
        let already_present = probe.devices.iter().any(|existing| existing.backends.iter().any(|backend| matches!(&backend.handle, BackendHandle::Libcamera { id } if id == &device_id)));
        if already_present {
            continue;
        }
        probe.devices.push(styx::ProbedDevice {
            identity: styx::DeviceIdentity { display: device_id.clone(), keys: vec![device_id.clone()] },
            backends: vec![styx::ProbedBackend {
                kind: styx::BackendKind::Libcamera,
                handle: BackendHandle::Libcamera { id: device_id },
                descriptor: device.descriptor,
                properties: device.properties,
            }],
        });
    }

    probe
        .devices
        .into_iter()
        .find(|device| device.identity.display == resource.display_name.as_ref() || device.backends.iter().any(|backend| backend_matches_resource(backend, resource)))
        .ok_or_else(|| format!("no Styx capture device matched '{}'", resource.display_name))
}

fn select_capture_backend<'a>(device: &'a ProbedDevice, resource: &ResourceDescriptor) -> Result<&'a ProbedBackend, String> {
    let preferred_kind = match resource.label("styx.backend") {
        Some("libcamera") => Some(BackendKind::Libcamera),
        Some("v4l2") => Some(BackendKind::V4l2),
        _ => None,
    };
    device
        .backends
        .iter()
        .find(|backend| preferred_kind.is_none_or(|kind| backend.kind == kind) && backend_matches_resource(backend, resource))
        .or_else(|| device.backends.iter().find(|backend| preferred_kind.is_none_or(|kind| backend.kind == kind)))
        .ok_or_else(|| format!("no Styx backend matched resource '{}'", resource.id.as_str()))
}

fn backend_matches_resource(backend: &ProbedBackend, resource: &ResourceDescriptor) -> bool {
    let dev_endpoint = resource.endpoint("dev");
    match (&backend.handle, dev_endpoint) {
        (BackendHandle::V4l2 { path }, Some(dev)) => path == dev,
        (BackendHandle::Libcamera { id }, _) => id == resource.display_name.as_ref(),
        _ => backend.properties.iter().any(|(key, value)| (key == "devnode" || key == "device") && Some(value.as_str()) == dev_endpoint),
    }
}

fn abort_capture_stream_publishers(publishers: &mut BTreeMap<String, JoinHandle<()>>) {
    for handle in publishers.values() {
        handle.abort();
    }
    publishers.clear();
}

fn should_publish_capture_channel(resource: &ResourceDescriptor) -> bool {
    resource.kind == crate::model::ResourceKind::CaptureDevice && resource.label("capture_role").unwrap_or("camera_stream") == "camera_stream"
}

fn spawn_config_refresh_watchers(config: &PeripheralConfig, event_tx: mpsc::UnboundedSender<RuntimeEvent>) -> Vec<RecommendedWatcher> {
    let mut watchers = Vec::new();
    for path in config_refresh_watch_paths(config) {
        match create_refresh_watcher(path.clone(), event_tx.clone()) {
            Ok(watcher) => watchers.push(watcher),
            Err(error) => warn!(path = %path.display(), error = %error, "failed to create peripheral refresh watcher"),
        }
    }
    watchers
}

fn create_refresh_watcher(path: PathBuf, event_tx: mpsc::UnboundedSender<RuntimeEvent>) -> notify::Result<RecommendedWatcher> {
    let callback_path = path.clone();
    let callback_tx = event_tx.clone();
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| match result {
        Ok(event) if should_trigger_refresh(&event) => {
            let _ = callback_tx.send(RuntimeEvent::RefreshRequested);
        }
        Ok(_) => {}
        Err(error) => {
            let _ = callback_tx.send(RuntimeEvent::RefreshWatchStopped(format!("{}: {error}", callback_path.display())));
        }
    })?;
    watcher.watch(&path, RecursiveMode::NonRecursive)?;
    Ok(watcher)
}

fn should_trigger_refresh(event: &notify::Event) -> bool {
    use notify::event::{CreateKind, EventKind, ModifyKind, RemoveKind, RenameMode};
    matches!(
        event.kind,
        EventKind::Create(CreateKind::Any | CreateKind::File | CreateKind::Folder)
            | EventKind::Modify(ModifyKind::Any | ModifyKind::Data(_) | ModifyKind::Metadata(_) | ModifyKind::Name(RenameMode::Any | RenameMode::Both | RenameMode::From | RenameMode::To))
            | EventKind::Remove(RemoveKind::Any | RemoveKind::File | RemoveKind::Folder)
    )
}

fn config_refresh_watch_paths(config: &PeripheralConfig) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for sensor_path in &config.sensor_config_paths {
        if let Some(path) = watchable_sensor_config_path(sensor_path)
            && !paths.contains(&path)
        {
            paths.push(path);
        }
    }
    paths
}

fn watchable_sensor_config_path(path: &Path) -> Option<PathBuf> {
    if path.exists() {
        return Some(path.to_path_buf());
    }
    path.parent().filter(|parent| parent.exists()).map(Path::to_path_buf)
}

fn build_runtime_components(config: &PeripheralConfig) -> Result<(PeripheralInventoryService, PeripheralManager, LemnosPeripheralStack), PeripheralRuntimeError> {
    let local_node_id = NodeId::try_new(config.node_id.clone()).map_err(|_| PeripheralRuntimeError::InvalidNodeId(config.node_id.clone()))?;
    let lemnos = LemnosPeripheralStack::new(local_node_id.clone()).map_err(PeripheralRuntimeError::LemnosInit)?;
    let mut service = PeripheralInventoryService::new(local_node_id);
    if config.enable_linux_probes {
        lemnos.register_into_inventory(&mut service);
    }
    let manager = lemnos.build_manager();
    Ok((service, manager, lemnos))
}

pub fn build_inventory_service(config: &PeripheralConfig) -> Result<PeripheralInventoryService, PeripheralRuntimeError> {
    let (service, _, _) = build_runtime_components(config)?;
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

fn resource_action_request_from_workload(resource: &ResourceDescriptor, workload: &WorkloadRecord, lease: Option<&LeaseRecord>) -> Result<ResourceControlRequest, PeripheralRuntimeError> {
    validate_control_lease(resource, workload, lease)?;
    let spec = resource_action_spec_from_workload(workload)?;
    match resource.kind {
        crate::model::ResourceKind::GpioLine => gpio_request(resource, workload, &spec),
        crate::model::ResourceKind::PwmChannel => pwm_request(resource, workload, &spec),
        crate::model::ResourceKind::I2cDevice => i2c_request(resource, workload, &spec),
        crate::model::ResourceKind::SpiDevice => spi_request(resource, workload, &spec),
        _ => Err(PeripheralRuntimeError::InvalidResourceActionWorkload { workload_id: workload.workload_id.as_str().to_string(), message: "unsupported resource kind".into() }),
    }
}

fn validate_control_lease(resource: &ResourceDescriptor, workload: &WorkloadRecord, lease: Option<&LeaseRecord>) -> Result<(), PeripheralRuntimeError> {
    let workload_id = workload.workload_id.as_str().to_string();
    match lease {
        Some(lease) if lease.holder_workload_id.as_ref().is_some_and(|holder| holder.as_str() == workload.workload_id.as_str()) => Ok(()),
        Some(lease) if lease.holder_workload_id.is_some() => Err(PeripheralRuntimeError::InvalidResourceActionWorkload {
            workload_id,
            message: format!("resource {} is leased to {}", resource.id.as_str(), lease.holder_workload_id.as_ref().map(|holder| holder.as_str()).unwrap_or("unknown")),
        }),
        Some(_) => Err(PeripheralRuntimeError::InvalidResourceActionWorkload { workload_id, message: format!("resource {} lease holder mismatch", resource.id.as_str()) }),
        None => Err(PeripheralRuntimeError::InvalidResourceActionWorkload { workload_id, message: format!("resource {} is not leased", resource.id.as_str()) }),
    }
}

fn resource_action_spec_from_workload(workload: &WorkloadRecord) -> Result<ResourceActionSpec, PeripheralRuntimeError> {
    let config = workload
        .config
        .as_ref()
        .ok_or_else(|| PeripheralRuntimeError::InvalidResourceActionWorkload { workload_id: workload.workload_id.as_str().to_string(), message: "missing config payload".into() })?;
    let workload_id = workload.workload_id.as_str().to_string();
    let decoded: ResourceActionConfig = deserialize_config(&config.payload)
        .map_err(|error| PeripheralRuntimeError::InvalidResourceActionWorkload { workload_id: workload_id.clone(), message: format!("config decode failed: {error}") })?;

    match decoded.action.kind {
        ResourceActionKind::GpioRead => Ok(ResourceActionSpec::GpioRead),
        ResourceActionKind::GpioWrite => Ok(ResourceActionSpec::GpioWrite {
            high: decoded.arg.high.ok_or_else(|| PeripheralRuntimeError::InvalidResourceActionWorkload { workload_id: workload_id.clone(), message: "missing arg.high".into() })?,
        }),
        ResourceActionKind::GpioConfigureDirection => Ok(ResourceActionSpec::GpioConfigureDirection {
            direction: match decoded.arg.direction.ok_or_else(|| PeripheralRuntimeError::InvalidResourceActionWorkload { workload_id: workload_id.clone(), message: "missing arg.direction".into() })? {
                GpioDirectionConfig::Input => GpioDirection::Input,
                GpioDirectionConfig::Output => GpioDirection::Output,
            },
            initial_high: decoded.arg.initial_high,
        }),
        ResourceActionKind::PwmEnable => Ok(ResourceActionSpec::PwmEnable {
            enabled: decoded.arg.enabled.ok_or_else(|| PeripheralRuntimeError::InvalidResourceActionWorkload { workload_id: workload_id.clone(), message: "missing arg.enabled".into() })?,
        }),
        ResourceActionKind::PwmSetPeriodNs => Ok(ResourceActionSpec::PwmSetPeriodNs {
            period_ns: decoded.arg.period_ns.ok_or_else(|| PeripheralRuntimeError::InvalidResourceActionWorkload { workload_id: workload_id.clone(), message: "missing arg.period_ns".into() })?,
        }),
        ResourceActionKind::PwmSetDutyCycleNs => Ok(ResourceActionSpec::PwmSetDutyCycleNs {
            duty_cycle_ns: decoded
                .arg
                .duty_cycle_ns
                .ok_or_else(|| PeripheralRuntimeError::InvalidResourceActionWorkload { workload_id: workload_id.clone(), message: "missing arg.duty_cycle_ns".into() })?,
        }),
        ResourceActionKind::PwmConfigure => Ok(ResourceActionSpec::PwmConfigure {
            period_ns: decoded.arg.period_ns.ok_or_else(|| PeripheralRuntimeError::InvalidResourceActionWorkload { workload_id: workload_id.clone(), message: "missing arg.period_ns".into() })?,
            duty_cycle_ns: decoded
                .arg
                .duty_cycle_ns
                .ok_or_else(|| PeripheralRuntimeError::InvalidResourceActionWorkload { workload_id: workload_id.clone(), message: "missing arg.duty_cycle_ns".into() })?,
            enabled: decoded.arg.enabled.ok_or_else(|| PeripheralRuntimeError::InvalidResourceActionWorkload { workload_id: workload_id.clone(), message: "missing arg.enabled".into() })?,
        }),
        ResourceActionKind::I2cRead => Ok(ResourceActionSpec::I2cRead {
            len: decoded.arg.len.ok_or_else(|| PeripheralRuntimeError::InvalidResourceActionWorkload { workload_id: workload_id.clone(), message: "missing arg.len".into() })? as usize,
        }),
        ResourceActionKind::I2cWrite => Ok(ResourceActionSpec::I2cWrite {
            bytes: decoded.arg.bytes.ok_or_else(|| PeripheralRuntimeError::InvalidResourceActionWorkload { workload_id: workload_id.clone(), message: "missing arg.bytes".into() })?,
        }),
        ResourceActionKind::I2cWriteRead => Ok(ResourceActionSpec::I2cWriteRead {
            write: decoded.arg.write.ok_or_else(|| PeripheralRuntimeError::InvalidResourceActionWorkload { workload_id: workload_id.clone(), message: "missing arg.write".into() })?,
            read_len: decoded.arg.read_len.ok_or_else(|| PeripheralRuntimeError::InvalidResourceActionWorkload { workload_id: workload_id.clone(), message: "missing arg.read_len".into() })? as usize,
        }),
        ResourceActionKind::SpiTransfer => Ok(ResourceActionSpec::SpiTransfer {
            bytes: decoded.arg.bytes.ok_or_else(|| PeripheralRuntimeError::InvalidResourceActionWorkload { workload_id: workload_id.clone(), message: "missing arg.bytes".into() })?,
        }),
        ResourceActionKind::SpiWrite => {
            Ok(ResourceActionSpec::SpiWrite { bytes: decoded.arg.bytes.ok_or_else(|| PeripheralRuntimeError::InvalidResourceActionWorkload { workload_id, message: "missing arg.bytes".into() })? })
        }
    }
}

fn gpio_request(resource: &ResourceDescriptor, workload: &WorkloadRecord, spec: &ResourceActionSpec) -> Result<ResourceControlRequest, PeripheralRuntimeError> {
    let owner = Some(workload.workload_id.as_str().to_string());
    let lease_generation = Some(0);
    match spec {
        ResourceActionSpec::GpioRead => Ok(ResourceControlRequest::Gpio(GpioControlRequest { resource_id: resource.id.clone(), owner, lease_generation, control: GpioControl::Read })),
        ResourceActionSpec::GpioWrite { high } => {
            Ok(ResourceControlRequest::Gpio(GpioControlRequest { resource_id: resource.id.clone(), owner, lease_generation, control: GpioControl::Write { high: *high } }))
        }
        ResourceActionSpec::GpioConfigureDirection { direction, initial_high } => Ok(ResourceControlRequest::Gpio(GpioControlRequest {
            resource_id: resource.id.clone(),
            owner,
            lease_generation,
            control: GpioControl::ConfigureDirection { direction: *direction, initial_high: *initial_high },
        })),
        _ => Err(PeripheralRuntimeError::InvalidResourceActionWorkload { workload_id: workload.workload_id.as_str().to_string(), message: "action kind does not match GPIO resource".into() }),
    }
}

fn pwm_request(resource: &ResourceDescriptor, workload: &WorkloadRecord, spec: &ResourceActionSpec) -> Result<ResourceControlRequest, PeripheralRuntimeError> {
    let owner = Some(workload.workload_id.as_str().to_string());
    let lease_generation = Some(0);
    let control = match spec {
        ResourceActionSpec::PwmEnable { enabled } => PwmControl::Enable { enabled: *enabled },
        ResourceActionSpec::PwmSetPeriodNs { period_ns } => PwmControl::SetPeriodNs { period_ns: *period_ns },
        ResourceActionSpec::PwmSetDutyCycleNs { duty_cycle_ns } => PwmControl::SetDutyCycleNs { duty_cycle_ns: *duty_cycle_ns },
        ResourceActionSpec::PwmConfigure { period_ns, duty_cycle_ns, enabled } => PwmControl::Configure { period_ns: *period_ns, duty_cycle_ns: *duty_cycle_ns, enabled: *enabled },
        _ => return Err(PeripheralRuntimeError::InvalidResourceActionWorkload { workload_id: workload.workload_id.as_str().to_string(), message: "action kind does not match PWM resource".into() }),
    };
    Ok(ResourceControlRequest::Pwm(PwmControlRequest { resource_id: resource.id.clone(), owner, lease_generation, control }))
}

fn i2c_request(resource: &ResourceDescriptor, workload: &WorkloadRecord, spec: &ResourceActionSpec) -> Result<ResourceControlRequest, PeripheralRuntimeError> {
    let owner = Some(workload.workload_id.as_str().to_string());
    let lease_generation = Some(0);
    let control = match spec {
        ResourceActionSpec::I2cRead { len } => I2cControl::Read { len: *len },
        ResourceActionSpec::I2cWrite { bytes } => I2cControl::Write { bytes: bytes.clone() },
        ResourceActionSpec::I2cWriteRead { write, read_len } => I2cControl::WriteRead { write: write.clone(), read_len: *read_len },
        _ => return Err(PeripheralRuntimeError::InvalidResourceActionWorkload { workload_id: workload.workload_id.as_str().to_string(), message: "action kind does not match I2C resource".into() }),
    };
    Ok(ResourceControlRequest::I2c(I2cControlRequest { resource_id: resource.id.clone(), owner, lease_generation, control }))
}

fn spi_request(resource: &ResourceDescriptor, workload: &WorkloadRecord, spec: &ResourceActionSpec) -> Result<ResourceControlRequest, PeripheralRuntimeError> {
    let owner = Some(workload.workload_id.as_str().to_string());
    let lease_generation = Some(0);
    let control = match spec {
        ResourceActionSpec::SpiTransfer { bytes } => SpiControl::Transfer { bytes: bytes.clone() },
        ResourceActionSpec::SpiWrite { bytes } => SpiControl::Write { bytes: bytes.clone() },
        _ => return Err(PeripheralRuntimeError::InvalidResourceActionWorkload { workload_id: workload.workload_id.as_str().to_string(), message: "action kind does not match SPI resource".into() }),
    };
    Ok(ResourceControlRequest::Spi(SpiControlRequest { resource_id: resource.id.clone(), owner, lease_generation, control }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::DEFAULT_NODE_ID,
        model::ResourceKind,
        resources::{PeripheralInventoryService, ResourceBuilder},
        workloads::MemoryControlSink,
    };
    use orion::control_plane::{AppliedClusterState, ClusterStateEnvelope, DesiredClusterState, LeaseState, ObservedClusterState, TypedConfigValue, WorkloadConfig};
    use std::sync::{Arc, Mutex};

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
    fn build_inventory_service_registers_linux_probes_when_enabled() {
        let service = build_inventory_service(&PeripheralConfig::default()).expect("service");
        assert_eq!(service.probe_names(), vec!["lemnos-linux", "styx-capture"]);
    }

    #[test]
    fn build_inventory_service_skips_linux_probes_when_disabled() {
        let service = build_inventory_service(&PeripheralConfig { enable_linux_probes: false, ..PeripheralConfig::default() }).expect("service");
        assert!(service.probe_names().is_empty());
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
        let workload = WorkloadRecord::builder(
            orion::core::WorkloadId::new("workload.gpio.write"),
            orion::core::RuntimeType::new(PERIPHERAL_RESOURCE_ACTION_RUNTIME_TYPE),
            orion::core::ArtifactId::new("artifact.control"),
        )
        .desired_state(DesiredState::Running)
        .assigned_to(orion::core::NodeId::new(DEFAULT_NODE_ID))
        .build();
        assert!(is_local_peripheral_resource_action_workload(&workload, DEFAULT_NODE_ID));
    }

    #[test]
    fn resource_action_request_from_workload_builds_gpio_write_request() {
        let owner = NodeId::new(DEFAULT_NODE_ID);
        let resource = ResourceBuilder::new(owner, ResourceKind::GpioLine, "gpio17", "GPIO 17").expect("resource").build();
        let workload = WorkloadRecord::builder(
            orion::core::WorkloadId::new("workload.gpio.write"),
            orion::core::RuntimeType::new(PERIPHERAL_RESOURCE_ACTION_RUNTIME_TYPE),
            orion::core::ArtifactId::new("artifact.control"),
        )
        .desired_state(DesiredState::Running)
        .assigned_to(orion::core::NodeId::new(DEFAULT_NODE_ID))
        .config(WorkloadConfig::new(PERIPHERAL_RESOURCE_ACTION_RUNTIME_TYPE).field("action.kind", TypedConfigValue::String("gpio.write".into())).field("arg.high", TypedConfigValue::Bool(true)))
        .bind_resource(resource.id.clone(), orion::core::NodeId::new(DEFAULT_NODE_ID))
        .build();
        let lease =
            LeaseRecord::builder(resource.id.clone()).lease_state(LeaseState::Leased).holder_node(orion::core::NodeId::new(DEFAULT_NODE_ID)).holder_workload(workload.workload_id.clone()).build();
        let request = resource_action_request_from_workload(&resource, &workload, Some(&lease)).expect("request");
        assert!(matches!(request, ResourceControlRequest::Gpio(GpioControlRequest { control: GpioControl::Write { high: true }, .. })));
    }

    #[test]
    fn resource_action_request_from_workload_requires_matching_lease_holder() {
        let owner = NodeId::new(DEFAULT_NODE_ID);
        let resource = ResourceBuilder::new(owner, ResourceKind::GpioLine, "gpio17", "GPIO 17").expect("resource").build();
        let workload = WorkloadRecord::builder(
            orion::core::WorkloadId::new("workload.gpio.write"),
            orion::core::RuntimeType::new(PERIPHERAL_RESOURCE_ACTION_RUNTIME_TYPE),
            orion::core::ArtifactId::new("artifact.control"),
        )
        .desired_state(DesiredState::Running)
        .assigned_to(orion::core::NodeId::new(DEFAULT_NODE_ID))
        .config(WorkloadConfig::new(PERIPHERAL_RESOURCE_ACTION_RUNTIME_TYPE).field("action.kind", TypedConfigValue::String("gpio.write".into())).field("arg.high", TypedConfigValue::Bool(true)))
        .bind_resource(resource.id.clone(), orion::core::NodeId::new(DEFAULT_NODE_ID))
        .build();
        let lease = LeaseRecord::builder(resource.id.clone())
            .lease_state(LeaseState::Leased)
            .holder_node(orion::core::NodeId::new(DEFAULT_NODE_ID))
            .holder_workload(orion::core::WorkloadId::new("other"))
            .build();
        let error = resource_action_request_from_workload(&resource, &workload, Some(&lease)).expect_err("lease mismatch");
        assert!(matches!(error, PeripheralRuntimeError::InvalidResourceActionWorkload { .. }));
    }

    #[test]
    fn resource_action_request_from_workload_requires_structured_args_object() {
        let owner = NodeId::new(DEFAULT_NODE_ID);
        let resource = ResourceBuilder::new(owner, ResourceKind::GpioLine, "gpio17", "GPIO 17").expect("resource").build();
        let workload = WorkloadRecord::builder(
            orion::core::WorkloadId::new("workload.gpio.write"),
            orion::core::RuntimeType::new(PERIPHERAL_RESOURCE_ACTION_RUNTIME_TYPE),
            orion::core::ArtifactId::new("artifact.control"),
        )
        .desired_state(DesiredState::Running)
        .assigned_to(orion::core::NodeId::new(DEFAULT_NODE_ID))
        .config(
            WorkloadConfig::new(PERIPHERAL_RESOURCE_ACTION_RUNTIME_TYPE).field("action.kind", TypedConfigValue::String("gpio.write".into())).field("arg.high", TypedConfigValue::String("bad".into())),
        )
        .bind_resource(resource.id.clone(), orion::core::NodeId::new(DEFAULT_NODE_ID))
        .build();
        let lease =
            LeaseRecord::builder(resource.id.clone()).lease_state(LeaseState::Leased).holder_node(orion::core::NodeId::new(DEFAULT_NODE_ID)).holder_workload(workload.workload_id.clone()).build();
        let error = resource_action_request_from_workload(&resource, &workload, Some(&lease)).expect_err("bad payload");
        assert!(matches!(error, PeripheralRuntimeError::InvalidResourceActionWorkload { .. }));
    }

    #[test]
    fn resource_action_request_from_workload_rejects_unknown_fields() {
        let owner = NodeId::new(DEFAULT_NODE_ID);
        let resource = ResourceBuilder::new(owner, ResourceKind::GpioLine, "gpio17", "GPIO 17").expect("resource").build();
        let workload = WorkloadRecord::builder(
            orion::core::WorkloadId::new("workload.gpio.strict"),
            orion::core::RuntimeType::new(PERIPHERAL_RESOURCE_ACTION_RUNTIME_TYPE),
            orion::core::ArtifactId::new("artifact.control"),
        )
        .desired_state(DesiredState::Running)
        .assigned_to(orion::core::NodeId::new(DEFAULT_NODE_ID))
        .config(
            WorkloadConfig::new(PERIPHERAL_RESOURCE_ACTION_RUNTIME_TYPE)
                .field("action.kind", TypedConfigValue::String("gpio.write".into()))
                .field("arg.high", TypedConfigValue::Bool(true))
                .field("arg.extra", TypedConfigValue::String("bad".into())),
        )
        .bind_resource(resource.id.clone(), orion::core::NodeId::new(DEFAULT_NODE_ID))
        .build();
        let lease =
            LeaseRecord::builder(resource.id.clone()).lease_state(LeaseState::Leased).holder_node(orion::core::NodeId::new(DEFAULT_NODE_ID)).holder_workload(workload.workload_id.clone()).build();

        let error = resource_action_request_from_workload(&resource, &workload, Some(&lease)).expect_err("unknown fields should fail");
        assert!(matches!(
            error,
            PeripheralRuntimeError::InvalidResourceActionWorkload { ref message, .. }
                if message.contains("arg.extra") && message.contains("unknown field")
        ));
    }

    #[test]
    fn config_refresh_watch_paths_include_sensor_config_parent_when_file_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = PeripheralConfig { sensor_config_paths: vec![dir.path().join("missing.toml")], ..PeripheralConfig::default() };
        let paths = config_refresh_watch_paths(&config);
        assert_eq!(paths, vec![dir.path().to_path_buf()]);
    }

    #[test]
    fn watchable_sensor_config_path_prefers_existing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("sensor.toml");
        std::fs::write(&file, b"sensor = true").expect("write");
        assert_eq!(watchable_sensor_config_path(&file), Some(file));
    }

    #[test]
    fn watchable_sensor_config_path_falls_back_to_existing_parent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("sensor.toml");
        assert_eq!(watchable_sensor_config_path(&file), Some(dir.path().to_path_buf()));
    }

    #[test]
    fn reconcile_controls_records_failed_feedback_for_unleased_resource_action() {
        let owner = NodeId::new(DEFAULT_NODE_ID);
        let resource = ResourceBuilder::new(owner, ResourceKind::GpioLine, "gpio17", "GPIO 17").expect("resource").build();
        let inventory = DiscoverySnapshot::new(vec![resource.clone()]);
        let workload = WorkloadRecord::builder(
            orion::core::WorkloadId::new("workload.gpio.read"),
            orion::core::RuntimeType::new(PERIPHERAL_RESOURCE_ACTION_RUNTIME_TYPE),
            orion::core::ArtifactId::new("artifact.control"),
        )
        .desired_state(DesiredState::Running)
        .assigned_to(orion::core::NodeId::new(DEFAULT_NODE_ID))
        .config(WorkloadConfig::new(PERIPHERAL_RESOURCE_ACTION_RUNTIME_TYPE).field("action.kind", TypedConfigValue::String("gpio.read".into())))
        .bind_resource(resource.id.clone(), orion::core::NodeId::new(DEFAULT_NODE_ID))
        .build();
        let mut desired = DesiredClusterState::default();
        desired.workloads.insert(workload.workload_id.clone(), workload);
        let snapshot = StateSnapshot { state: ClusterStateEnvelope::new(desired, ObservedClusterState::default(), AppliedClusterState::default()) };

        let config = PeripheralConfig::default();
        let mut manager = PeripheralManager::new();
        manager.register_controller(Box::new(Mutex::new(MemoryControlSink::default())));
        let runtime = PeripheralRuntime::from_parts(config, Arc::new(PeripheralInventoryService::new(NodeId::new(DEFAULT_NODE_ID))), manager);
        let mut state = RuntimeState::default();

        runtime.reconcile_controls(&inventory, &snapshot, &mut state).expect("invalid workload should not crash runtime");

        let feedback = state.resource_action_feedback.get(resource.id.as_str()).expect("failed feedback");
        let action = feedback.state.action_result.as_ref().expect("action result");
        assert_eq!(action.action_kind, "gpio.read");
        assert_eq!(action.status, orion::control_plane::ResourceActionStatus::Failed);
        assert!(action.error.as_ref().is_some_and(|error| error.contains("not leased")));
    }
}
