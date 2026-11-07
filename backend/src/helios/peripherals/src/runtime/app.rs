use std::{collections::BTreeMap, sync::Arc};

use helios_shutdown::{STAGE_TIMEOUT, Shutdown};
use orion::{
    client::{ClientError, LocalNodeRuntime, LocalProviderEvent, LocalProviderService, LocalServiceRetryPolicy},
    control_plane::LeaseRecord,
    core::Revision,
};
use styx::watch::{CompositeWatcher, LinuxVideoFsWatcher, WatchRuntime};
use tokio::{
    sync::{Notify, mpsc},
    task::JoinHandle,
    time::{Duration, sleep},
};
use tracing::warn;

use crate::{
    config::PeripheralConfig,
    model::NodeId,
    provider::{CameraServices, OrionPeripheralPublisher, OrionPublishError},
    resources::{CaptureProbe, DiscoverySnapshot, PeripheralInventoryService},
};

const PROVIDER_EVENT_RETRY_DELAY: Duration = Duration::from_millis(200);

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
    LeaseUpdate(Vec<LeaseRecord>),
    ProviderWatchStopped(ClientError),
    RefreshWatchStopped(String),
    /// Publish the current snapshot again: the last publish failed (Orion slow or away).
    Republish,
}

const ORION_IPC_RETRY_DELAY: Duration = Duration::from_secs(2);

pub struct PeripheralRuntime {
    config: PeripheralConfig,
    inventory: Arc<PeripheralInventoryService>,
    publisher: OrionPeripheralPublisher,
    refresh_handle: RefreshHandle,
    node_runtime: LocalNodeRuntime,
}

#[derive(Default)]
struct RuntimeState {
    camera_services: CameraServices,
}

impl PeripheralRuntime {
    pub fn from_config(config: PeripheralConfig) -> Result<Self, PeripheralRuntimeError> {
        let inventory = build_inventory_service(&config)?;
        Ok(Self {
            node_runtime: LocalNodeRuntime::new(config.orion_ipc_socket_path.clone(), config.orion_ipc_stream_socket_path.clone()),
            publisher: OrionPeripheralPublisher::from_config(&config),
            refresh_handle: RefreshHandle::default(),
            config,
            inventory: Arc::new(inventory),
        })
    }

    /// A runtime over a given inventory (tests).
    pub fn from_parts(config: PeripheralConfig, inventory: Arc<PeripheralInventoryService>) -> Self {
        let node_runtime = LocalNodeRuntime::new(config.orion_ipc_socket_path.clone(), config.orion_ipc_stream_socket_path.clone());
        let publisher = OrionPeripheralPublisher::from_config(&config);
        Self { config, inventory, publisher, refresh_handle: RefreshHandle::default(), node_runtime }
    }

    pub async fn refresh_inventory(&self, leases: &[LeaseRecord]) -> Result<DiscoverySnapshot, PeripheralRuntimeError> {
        self.refresh_inventory_with_state(leases, &RuntimeState::default()).await
    }

    async fn refresh_inventory_with_state(&self, leases: &[LeaseRecord], state: &RuntimeState) -> Result<DiscoverySnapshot, PeripheralRuntimeError> {
        let snapshot = self.probe_inventory();
        self.publish_snapshot(&snapshot, leases, state).await?;
        Ok(snapshot)
    }

    /// Probe the inventory (cameras), reporting each probe's failure.
    fn probe_inventory(&self) -> DiscoverySnapshot {
        let observed_at_ms = chrono::Utc::now().timestamp_millis().max(0) as u64;
        let report = self.inventory.refresh_report(observed_at_ms);
        for probe in &report.probe_reports {
            match &probe.error {
                Some(error) => warn!(probe = %probe.probe, error = %error, "peripheral inventory probe failed"),
                None => tracing::debug!(probe = %probe.probe, discovered_resources = probe.discovered_resources, "peripheral inventory probe completed"),
            }
        }
        report.snapshot
    }

    async fn publish_snapshot(&self, snapshot: &DiscoverySnapshot, leases: &[LeaseRecord], state: &RuntimeState) -> Result<(), PeripheralRuntimeError> {
        let _ = state;
        self.publisher.publish_snapshot_with_feedback(snapshot, leases, &BTreeMap::new()).await?;
        Ok(())
    }

    /// Publish from the event loop. A failed publish (Orion slow to answer, or restarting) never
    /// ends the runtime: it is reported, the provider shows degraded, and the snapshot is
    /// published again after `ORION_IPC_RETRY_DELAY` (the latest one then, not this one).
    async fn publish_or_retry(&self, snapshot: &DiscoverySnapshot, leases: &[LeaseRecord], state: &RuntimeState, retry: &mpsc::UnboundedSender<RuntimeEvent>) -> bool {
        match self.publish_snapshot(snapshot, leases, state).await {
            Ok(()) => true,
            Err(error) => {
                warn!(node_id = %self.config.node_id, error = %error, retry_delay_secs = ORION_IPC_RETRY_DELAY.as_secs(), "snapshot publish failed; retrying");
                let retry = retry.clone();
                tokio::spawn(async move {
                    sleep(ORION_IPC_RETRY_DELAY).await;
                    let _ = retry.send(RuntimeEvent::Republish);
                });
                false
            }
        }
    }

    /// Runs until SIGINT or SIGTERM (or a failure), then stops: the runtime's loops end, and the
    /// camera services are torn down within `STAGE_TIMEOUT` (see `helios_shutdown`).
    pub async fn run_until_stopped(&self) -> Result<(), PeripheralRuntimeError> {
        let shutdown = Shutdown::new("helios-peripherals");
        shutdown.listen();
        let stop = shutdown.token();
        // Owned here, not by `run`, so the teardown below runs whether `run` ends or is cut short.
        let mut state = RuntimeState::default();
        let result = tokio::select! {
            result = self.run(&mut state) => result,
            _ = stop.cancelled() => Ok(()),
        };
        if let Err(error) = &result {
            warn!(node_id = %self.config.node_id, error = %error, "peripherals runtime failed");
        }
        // Dropping a handle stops its camera service, which joins its capture threads.
        shutdown.stage("camera services", STAGE_TIMEOUT, move || drop(state));
        shutdown.finish();
        result
    }

    async fn run(&self, state: &mut RuntimeState) -> Result<(), PeripheralRuntimeError> {
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

        let _styx_watch = self.spawn_styx_watch_task(event_tx.clone());

        let provider_service = LocalProviderService::new(self.node_runtime.clone(), format!("{}-watch", self.publisher.client_name()), self.publisher.provider_identity_record())
            .with_retry_policy(LocalServiceRetryPolicy::fixed_delay(PROVIDER_EVENT_RETRY_DELAY));
        let mut provider_subscription = provider_service.subscribe(Revision::ZERO).await?;
        let mut current_leases = match next_provider_event_retrying(&mut provider_subscription).await? {
            LocalProviderEvent::BootstrapLeases(leases) => leases,
            LocalProviderEvent::LeasesChanged { leases, .. } => leases,
            LocalProviderEvent::BootstrapStateSnapshot(_) | LocalProviderEvent::StateSnapshot { .. } => Vec::new(),
        };
        let mut current_snapshot = self.refresh_inventory_with_state(&current_leases, state).await?;
        state.camera_services.sync(&self.config, &current_snapshot.resources);
        // The state snapshot that follows the leases is the readiness point.
        let _ = next_provider_event_retrying(&mut provider_subscription).await?;
        self.publish_snapshot(&current_snapshot, &current_leases, state).await?;

        let provider_watch_error_tx = event_tx.clone();
        let retry_tx = event_tx.clone();
        tokio::spawn(async move {
            if let Err(error) = watch_provider_updates(provider_subscription, event_tx).await {
                let _ = provider_watch_error_tx.send(RuntimeEvent::ProviderWatchStopped(error));
            }
        });

        loop {
            tokio::select! {
                maybe_event = event_rx.recv() => {
                    match maybe_event {
                        Some(RuntimeEvent::RefreshRequested) => {
                            current_snapshot = self.probe_inventory();
                            state.camera_services.sync(&self.config, &current_snapshot.resources);
                            self.publish_or_retry(&current_snapshot, &current_leases, state, &retry_tx).await;
                        }
                        Some(RuntimeEvent::LeaseUpdate(leases)) => {
                            current_leases = leases;
                            self.publish_or_retry(&current_snapshot, &current_leases, state, &retry_tx).await;
                        }
                        Some(RuntimeEvent::Republish) => {
                            self.publish_or_retry(&current_snapshot, &current_leases, state, &retry_tx).await;
                        }
                        Some(RuntimeEvent::ProviderWatchStopped(error)) => {
                            warn!(node_id = %self.config.node_id, error = %error, "provider watch stopped");
                        }
                        Some(RuntimeEvent::RefreshWatchStopped(error)) => {
                            warn!(node_id = %self.config.node_id, error = %error, "refresh watch stopped");
                        }
                        None => return Ok(()),
                    }
                }
            }
        }
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

pub fn build_inventory_service(config: &PeripheralConfig) -> Result<PeripheralInventoryService, PeripheralRuntimeError> {
    let local_node_id = NodeId::try_new(config.node_id.clone()).map_err(|_| PeripheralRuntimeError::InvalidNodeId(config.node_id.clone()))?;
    let mut service = PeripheralInventoryService::new(local_node_id);
    if config.enable_linux_probes {
        service.register_probe(Arc::new(CaptureProbe));
    }
    Ok(service)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn build_inventory_service_registers_the_camera_probe() {
        let service = build_inventory_service(&PeripheralConfig::default()).expect("service");
        assert_eq!(service.probe_names(), vec!["styx-capture"]);
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
}
