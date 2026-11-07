use helios_shutdown::{STAGE_TIMEOUT, Shutdown};
use orion::{
    client::{ClientError, ControlPlaneEventStream, LocalExecutorEvent, LocalExecutorService, LocalNodeRuntime, LocalServiceRetryPolicy},
    control_plane::{ClientEventKind, StateSnapshot, WorkloadRecord},
    transport::ipc::IpcTransportError,
};
use tokio::{
    sync::mpsc,
    time::{Duration, Instant, interval, sleep, sleep_until},
};
use tracing::{info, warn};

use crate::{
    config::EngineConfig,
    execution::{ExecutionPlugins, GraphSettings, ResidentExecutionSet},
    model::{EngineSnapshot, ExecutionSessionState, ExecutionSessionStatus, ExecutionWorkload, GraphRef, now_ms},
    plugins::{PluginLoadError, PluginLoadResult, discover_plugin_libraries, load_plugins_as},
    provider::{EnginePublishError, OrionEnginePublisher},
    workloads::{WorkloadDecodeError, decode_assigned_workload},
};

#[derive(Debug, thiserror::Error)]
pub enum EngineRuntimeError {
    #[error(transparent)]
    PluginLoad(#[from] PluginLoadError),
    #[error(transparent)]
    Publish(#[from] EnginePublishError),
    #[error(transparent)]
    Client(#[from] ClientError),
}

enum RuntimeEvent {
    WorkloadsChanged(Vec<WorkloadRecord>),
    StateSnapshotUpdated(Box<StateSnapshot>),
    WorkloadWatchStopped(ClientError),
    StateWatchStopped(ClientError),
}

const ORION_IPC_RETRY_DELAY: Duration = Duration::from_secs(2);
const ORION_EMPTY_WATCH_RETRY_DELAY: Duration = Duration::from_millis(200);
/// A full state read as a safety net under the state watch (which pushes every change).
const ENGINE_RECONCILE_INTERVAL: Duration = Duration::from_secs(5);
/// How long after a due publication a tick is waited for before publishing anyway.
const PUBLISH_FALLBACK_DELAY: Duration = Duration::from_millis(100);

pub struct EngineApp {
    config: EngineConfig,
    publisher: OrionEnginePublisher,
    node_runtime: LocalNodeRuntime,
    /// The NetworkTables publisher's connection, once it runs.
    nt4_status: std::sync::Mutex<Option<std::sync::Arc<std::sync::Mutex<crate::nt4::Nt4Status>>>>,
}

impl EngineApp {
    pub fn from_config(config: EngineConfig) -> Result<Self, EngineRuntimeError> {
        let publisher = OrionEnginePublisher::from_config(&config);
        let node_runtime = LocalNodeRuntime::new(config.orion_ipc_socket_path.clone(), config.orion_ipc_stream_socket_path.clone());
        Ok(Self { config, publisher, node_runtime, nt4_status: std::sync::Mutex::default() })
    }

    /// Runs until SIGINT or SIGTERM (or a failure), then stops the graphs: their threads are asked
    /// to stop and waited for at most `STAGE_TIMEOUT` (see `helios_shutdown`).
    pub async fn run_until_stopped(&self) -> Result<(), EngineRuntimeError> {
        let shutdown = Shutdown::new("helios-engine");
        shutdown.listen();
        let stop = shutdown.token();
        // Owned here, not by `run`, so the graphs are stopped below whether `run` ends or is cut short.
        let mut sessions: Option<ResidentExecutionSet> = None;
        let result = tokio::select! {
            result = self.run(&mut sessions) => result,
            _ = stop.cancelled() => Ok(()),
        };
        if let Some(sessions) = sessions.take() {
            // Dropping the set stops each graph and joins its thread.
            shutdown.stage("graph threads", STAGE_TIMEOUT, move || drop(sessions));
        }
        shutdown.finish();
        result
    }

    async fn run(&self, sessions: &mut Option<ResidentExecutionSet>) -> Result<(), EngineRuntimeError> {
        let plugins = self.bootstrap().await?;
        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<RuntimeEvent>();
        loop {
            match self.publisher.register_identities().await {
                Ok(()) => break,
                Err(error) => {
                    warn!(
                        node_id = %self.config.node_id,
                        error = %error,
                        retry_delay_secs = ORION_IPC_RETRY_DELAY.as_secs(),
                        "engine failed to register provider/executor identities; retrying"
                    );
                    sleep(ORION_IPC_RETRY_DELAY).await;
                }
            }
        }

        let service = LocalExecutorService::new(self.node_runtime.clone(), format!("{}-watch", self.publisher.executor_client_name()), self.publisher.executor_identity_record())
            .with_retry_policy(LocalServiceRetryPolicy::fixed_delay(ORION_IPC_RETRY_DELAY));
        let mut subscription = loop {
            match service.subscribe_workloads().await {
                Ok(subscription) => break subscription,
                Err(error) => self.wait_for_orion(error).await?,
            }
        };
        let mut current_workloads = loop {
            match subscription.next_event().await {
                Ok(LocalExecutorEvent::Bootstrap(workloads)) => break workloads,
                Ok(LocalExecutorEvent::WorkloadsChanged { workloads, .. }) => break workloads,
                Err(error) => self.wait_for_orion(error).await?,
            }
        };
        let watch_error_tx = event_tx.clone();
        let workload_event_tx = event_tx.clone();
        tokio::spawn(async move {
            if let Err(error) = watch_assigned_workloads(subscription, workload_event_tx).await {
                let _ = watch_error_tx.send(RuntimeEvent::WorkloadWatchStopped(error));
            }
        });

        let state_watch = loop {
            let mut stream = match ControlPlaneEventStream::connect_at(&self.config.orion_ipc_stream_socket_path, format!("{}-state-watch", self.publisher.executor_client_name())).await {
                Ok(stream) => stream,
                Err(error) => {
                    self.wait_for_orion(error).await?;
                    continue;
                }
            };
            match stream.subscribe_state(orion::core::Revision::ZERO).await {
                Ok(()) => break stream,
                Err(error) => self.wait_for_orion(error).await?,
            }
        };
        let state_watch_error_tx = event_tx.clone();
        let state_event_tx = event_tx.clone();
        tokio::spawn(async move {
            if let Err(error) = watch_state_snapshots(state_watch, state_event_tx).await {
                let _ = state_watch_error_tx.send(RuntimeEvent::StateWatchStopped(error));
            }
        });

        let mut current_state_snapshot = self.node_runtime.control_plane(format!("{}-control", self.publisher.executor_client_name()))?.fetch_state_snapshot().await.ok();
        let (nt4, nt4_status) = crate::nt4::spawn(self.config.nt4_settings_path.clone(), self.config.controller_facts_path.clone(), format!("helios-{}", self.config.node_id));
        *self.nt4_status.lock().unwrap_or_else(|poison| poison.into_inner()) = Some(nt4_status);
        let execution_sessions = sessions.insert(
            ResidentExecutionSet::new(GraphSettings { metrics_level: self.config.metrics_level })
                .with_nt4(nt4)
                .with_live(crate::live::spawn(self.config.live_socket_path.clone()))
                .with_plan_dir(self.config.plan_dir.clone()),
        );
        // Graphs run on their own threads when their input arrives; they signal here when they
        // produced something, and what they produced is published at most once per interval.
        let outputs_ready = execution_sessions.outputs_ready();
        let publish_interval = Duration::from_millis(self.config.publish_interval_ms.max(1));
        let mut publish_due: Option<Instant> = None;
        let mut last_publish = Instant::now();
        let mut reconcile_tick = interval(ENGINE_RECONCILE_INTERVAL);
        self.publish_snapshot_resilient(&plugins, execution_sessions, &current_workloads, current_state_snapshot.as_ref()).await?;
        info!(
            node_id = %self.config.node_id,
            plugin_count = plugins.builtins.len() + plugins.libraries.len(),
            assigned_workload_count = current_workloads.len(),
            engine_socket = %self.config.engine_socket_path.display(),
            "helios-engine started"
        );

        loop {
            tokio::select! {
                _ = reconcile_tick.tick() => {
                    if let Ok(snapshot) = self
                        .node_runtime
                        .control_plane(format!("{}-control", self.publisher.executor_client_name()))?
                        .fetch_state_snapshot()
                        .await
                    {
                        let snapshot_workloads = snapshot
                            .state
                            .desired
                            .workloads
                            .values()
                            .cloned()
                            .collect::<Vec<_>>();
                        let decoded = decode_execution_workloads(&snapshot_workloads, &self.config.node_id, now_ms());
                        let workloads_changed = workload_records_changed(&current_workloads, &snapshot_workloads);
                        let should_publish = workloads_changed
                            || relevant_binding_state_changed(
                                current_state_snapshot.as_ref(),
                                &snapshot,
                                &decoded.runnable,
                            );
                        current_workloads = snapshot_workloads;
                        current_state_snapshot = Some(snapshot);
                        if should_publish {
                            self.publish_snapshot_resilient(&plugins, execution_sessions, &current_workloads, current_state_snapshot.as_ref()).await?;
                        }
                    }
                }
                // Published right after a graph tick, once the interval has passed: the publish
                // (and Orion's and helios-api's handling of it) then falls between frames instead
                // of on the camera's ISP job, which it slows by contending for memory.
                _ = outputs_ready.notified() => {
                    if Instant::now() >= last_publish + publish_interval {
                        publish_due = None;
                        last_publish = Instant::now();
                        self.publish_snapshot_resilient(&plugins, execution_sessions, &current_workloads, current_state_snapshot.as_ref()).await?;
                    } else if publish_due.is_none() {
                        publish_due = Some(last_publish + publish_interval + PUBLISH_FALLBACK_DELAY);
                    }
                }
                // No tick came after the interval (the graph stopped or runs slower than it).
                _ = sleep_until(publish_due.unwrap_or_else(Instant::now)), if publish_due.is_some() => {
                    publish_due = None;
                    last_publish = Instant::now();
                    self.publish_snapshot_resilient(&plugins, execution_sessions, &current_workloads, current_state_snapshot.as_ref()).await?;
                }
                maybe_event = event_rx.recv() => {
                    match maybe_event {
                        Some(RuntimeEvent::WorkloadsChanged(workloads)) => {
                            current_workloads = workloads;
                            self.publish_snapshot_resilient(&plugins, execution_sessions, &current_workloads, current_state_snapshot.as_ref()).await?;
                        }
                        Some(RuntimeEvent::StateSnapshotUpdated(snapshot)) => {
                            let snapshot_workloads = snapshot
                                .state
                                .desired
                                .workloads
                                .values()
                                .cloned()
                                .collect::<Vec<_>>();
                            let workloads_changed = workload_records_changed(&current_workloads, &snapshot_workloads);
                            let should_publish = workloads_changed || relevant_binding_state_changed(
                                current_state_snapshot.as_ref(),
                                &snapshot,
                                &decode_execution_workloads(&snapshot_workloads, &self.config.node_id, now_ms()).runnable,
                            );
                            current_workloads = snapshot_workloads;
                            current_state_snapshot = Some(*snapshot);
                            if should_publish {
                                self.publish_snapshot_resilient(&plugins, execution_sessions, &current_workloads, current_state_snapshot.as_ref()).await?;
                            }
                        }
                        Some(RuntimeEvent::WorkloadWatchStopped(error)) => {
                            warn!(node_id = %self.config.node_id, error = %error, "engine workload watch stopped");
                        }
                        Some(RuntimeEvent::StateWatchStopped(error)) => {
                            warn!(node_id = %self.config.node_id, error = %error, "engine state watch stopped");
                        }
                        None => return Ok(()),
                    }
                }
            }
        }
    }

    async fn bootstrap(&self) -> Result<PluginLoadResult, EngineRuntimeError> {
        let plugin_paths = discover_plugin_libraries(&self.config.plugin_dirs);
        if plugin_paths.is_empty() {
            warn!("no Daedalus plugins discovered in configured plugin dirs");
        }

        let mut load_result = load_plugins_as(&plugin_paths, self.config.plugin_install)?;
        log_loaded_plugins(&load_result);
        match crate::plugins::catalog::write_catalog(&load_result.registry, &self.config.catalog_path) {
            Ok(path) => load_result.catalog_path = Some(path),
            Err(error) => warn!(path = %self.config.catalog_path.display(), %error, "could not write the node catalog"),
        }
        Ok(load_result)
    }

    async fn publish_current_snapshot(
        &self,
        plugins: &PluginLoadResult,
        execution_sessions: &mut ResidentExecutionSet,
        workloads: &[WorkloadRecord],
        state_snapshot: Option<&StateSnapshot>,
    ) -> Result<(), EngineRuntimeError> {
        let observed_at_ms = now_ms();
        let decoded = decode_execution_workloads(workloads, &self.config.node_id, observed_at_ms);
        let fetched_snapshot;
        let state_snapshot = match state_snapshot {
            Some(snapshot) => Some(snapshot),
            None => {
                fetched_snapshot = self.node_runtime.control_plane(format!("{}-control", self.publisher.executor_client_name()))?.fetch_state_snapshot().await.ok();
                fetched_snapshot.as_ref()
            }
        };
        let loaded_plugins = plugins.builtins.iter().cloned().chain(plugins.libraries.iter().map(|plugin| plugin.metadata().clone())).collect::<Vec<_>>();
        let mut execution = execution_sessions.sync_workloads(&ExecutionPlugins { registry: &plugins.registry, loaded_plugins: &loaded_plugins }, &decoded.runnable, state_snapshot, observed_at_ms);
        execution.sessions.extend(decoded.decode_failures);
        let snapshot = EngineSnapshot {
            node_id: self.config.node_id.clone(),
            engine_socket_path: self.config.engine_socket_path.clone(),
            catalog_path: plugins.catalog_path.clone(),
            nt4: self.nt4_status.lock().unwrap_or_else(|poison| poison.into_inner()).as_ref().map(|status| status.lock().unwrap_or_else(|poison| poison.into_inner()).clone()),
            plugin_dirs: self.config.plugin_dirs.clone(),
            loaded_plugins,
            assigned_workloads: decoded.runnable,
            sessions: execution.sessions,
            artifacts: execution.artifacts,
        };
        self.publisher.publish_snapshot(&snapshot, workloads).await?;
        Ok(())
    }

    /// Waits out a startup call Orion could not answer yet, or returns the error when retrying
    /// cannot help.
    async fn wait_for_orion(&self, error: ClientError) -> Result<(), ClientError> {
        if matches!(error, ClientError::NoMessageAvailable) {
            sleep(ORION_EMPTY_WATCH_RETRY_DELAY).await;
            return Ok(());
        }
        if !orion_unavailable(&error) {
            return Err(error);
        }
        warn!(node_id = %self.config.node_id, error = %error, retry_delay_secs = ORION_IPC_RETRY_DELAY.as_secs(), "orion-node is not answering yet; retrying");
        sleep(ORION_IPC_RETRY_DELAY).await;
        Ok(())
    }

    async fn publish_snapshot_resilient(
        &self,
        plugins: &PluginLoadResult,
        execution_sessions: &mut ResidentExecutionSet,
        workloads: &[WorkloadRecord],
        state_snapshot: Option<&StateSnapshot>,
    ) -> Result<(), EngineRuntimeError> {
        match self.publish_current_snapshot(plugins, execution_sessions, workloads, state_snapshot).await {
            Ok(()) => Ok(()),
            Err(error) if is_retryable_no_message(&error) => {
                warn!(node_id = %self.config.node_id, error = %error, "engine publish hit transient no-message condition; will retry on next event");
                Ok(())
            }
            Err(error) if publish_lost_orion(&error) => {
                warn!(node_id = %self.config.node_id, error = %error, "orion-node is not answering; skipping this publish");
                Ok(())
            }
            Err(error) => Err(error),
        }
    }
}

async fn watch_assigned_workloads(mut subscription: orion::client::LocalExecutorSubscription, event_tx: mpsc::UnboundedSender<RuntimeEvent>) -> Result<(), ClientError> {
    loop {
        match subscription.next_event().await {
            Ok(LocalExecutorEvent::Bootstrap(_)) => {}
            Ok(LocalExecutorEvent::WorkloadsChanged { workloads, .. }) => {
                if event_tx.send(RuntimeEvent::WorkloadsChanged(workloads)).is_err() {
                    return Ok(());
                }
            }
            Err(ClientError::NoMessageAvailable) => {
                sleep(ORION_EMPTY_WATCH_RETRY_DELAY).await;
            }
            Err(error) => return Err(error),
        }
    }
}

async fn watch_state_snapshots(mut subscription: ControlPlaneEventStream, event_tx: mpsc::UnboundedSender<RuntimeEvent>) -> Result<(), ClientError> {
    loop {
        match subscription.next_events().await {
            Ok(events) => {
                for event in events {
                    if let ClientEventKind::StateSnapshot(snapshot) = event.event
                        && event_tx.send(RuntimeEvent::StateSnapshotUpdated(snapshot)).is_err()
                    {
                        return Ok(());
                    }
                }
            }
            Err(ClientError::NoMessageAvailable) => {
                sleep(ORION_EMPTY_WATCH_RETRY_DELAY).await;
            }
            Err(error) => return Err(error),
        }
    }
}

#[derive(Debug, Default)]
struct DecodedExecutionWorkloads {
    runnable: Vec<ExecutionWorkload>,
    decode_failures: Vec<ExecutionSessionState>,
}

fn decode_execution_workloads(workloads: &[WorkloadRecord], node_id: &str, observed_at_ms: u64) -> DecodedExecutionWorkloads {
    let mut decoded = DecodedExecutionWorkloads::default();
    for record in workloads {
        match decode_assigned_workload(record, node_id) {
            Ok(workload) => decoded.runnable.push(workload),
            Err(WorkloadDecodeError::WrongRuntime { .. } | WorkloadDecodeError::WrongNode { .. } | WorkloadDecodeError::NotRunning { .. }) => {}
            Err(error) => {
                warn!(node_id = %node_id, error = %error, workload_id = %record.workload_id.as_str(), "failed to decode engine workload");
                decoded.decode_failures.push(decode_failure_session(record, error, observed_at_ms));
            }
        }
    }
    decoded
}

fn decode_failure_session(record: &WorkloadRecord, error: WorkloadDecodeError, observed_at_ms: u64) -> ExecutionSessionState {
    let workload_id = record.workload_id.as_str().to_string();
    ExecutionSessionState {
        workload_id: workload_id.clone(),
        session_id: format!("session.{workload_id}"),
        status: ExecutionSessionStatus::Failed,
        observed_at_ms,
        graph_ref: GraphRef::ArtifactId(record.artifact_id.as_str().to_string()),
        bindings: Vec::new(),
        plugin_requirements: Vec::new(),
        message: Some(error.to_string()),
    }
}

fn relevant_binding_state_changed(previous: Option<&StateSnapshot>, current: &StateSnapshot, workloads: &[crate::model::ExecutionWorkload]) -> bool {
    let relevant_ids = workloads.iter().flat_map(|workload| workload.bindings.iter().map(|binding| binding.resource_id.as_str().to_string())).collect::<std::collections::BTreeSet<_>>();
    if relevant_ids.is_empty() {
        return false;
    }
    let current_resources = &current.state.observed.resources;
    match previous {
        None => true,
        Some(previous) => relevant_ids.into_iter().any(|resource_id| previous.state.observed.resources.get(resource_id.as_str()) != current_resources.get(resource_id.as_str())),
    }
}

fn workload_records_changed(previous: &[WorkloadRecord], current: &[WorkloadRecord]) -> bool {
    previous != current
}

fn log_loaded_plugins(plugins: &PluginLoadResult) {
    for plugin in &plugins.builtins {
        info!(
            plugin_path = %plugin.path.display(),
            plugin_name = plugin.plugin_name.as_deref().unwrap_or("unknown"),
            plugin_version = plugin.plugin_version.as_deref().unwrap_or("builtin"),
            daedalus_version = plugin.daedalus_version.as_deref().unwrap_or("builtin"),
            "loaded built-in Daedalus plugin"
        );
    }

    for plugin in &plugins.libraries {
        let metadata = plugin.metadata();
        info!(
            plugin_path = %metadata.path.display(),
            plugin_name = metadata.plugin_name.as_deref().unwrap_or("unknown"),
            plugin_version = metadata.plugin_version.as_deref().unwrap_or("unknown"),
            daedalus_version = metadata.daedalus_version.as_deref().unwrap_or("unknown"),
            "loaded external Daedalus plugin"
        );
    }
}

/// Whether this is orion-node not being there (still starting, or restarting), not a refusal.
fn orion_unavailable(error: &ClientError) -> bool {
    matches!(
        error,
        ClientError::SendFailed
            | ClientError::Ipc(IpcTransportError::ConnectFailed(_) | IpcTransportError::ConnectionRefused(_) | IpcTransportError::ReadFailed(_) | IpcTransportError::WriteFailed(_))
    )
}

fn is_retryable_no_message(error: &EngineRuntimeError) -> bool {
    publish_client_error(error).is_some_and(|error| matches!(error, ClientError::NoMessageAvailable))
}

/// A publish that found no orion-node: the next one tries again, as it does after a restart of it.
fn publish_lost_orion(error: &EngineRuntimeError) -> bool {
    publish_client_error(error).is_some_and(orion_unavailable)
}

fn publish_client_error(error: &EngineRuntimeError) -> Option<&ClientError> {
    match error {
        EngineRuntimeError::Client(error) | EngineRuntimeError::Publish(EnginePublishError::Client(error)) => Some(error),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_orion_node_is_waited_for_and_a_refusal_is_not() {
        assert!(orion_unavailable(&ClientError::SendFailed));
        assert!(orion_unavailable(&ClientError::Ipc(IpcTransportError::ConnectFailed("no socket".into()))));
        assert!(orion_unavailable(&ClientError::Ipc(IpcTransportError::ConnectionRefused("refused".into()))));
        assert!(!orion_unavailable(&ClientError::Rejected("no".into())));
        assert!(!orion_unavailable(&ClientError::ProtocolMismatch { local: 1, remote: 2 }));
        assert!(!orion_unavailable(&ClientError::Ipc(IpcTransportError::MissingPreamble)));
    }

    #[test]
    fn a_publish_that_finds_no_orion_node_does_not_stop_the_engine() {
        let gone = ClientError::Ipc(IpcTransportError::ConnectFailed("no socket".into()));
        assert!(publish_lost_orion(&EngineRuntimeError::Publish(EnginePublishError::Client(gone))));
        let refused = ClientError::Rejected("no".into());
        assert!(!publish_lost_orion(&EngineRuntimeError::Publish(EnginePublishError::Client(refused))));
        assert!(is_retryable_no_message(&EngineRuntimeError::Client(ClientError::NoMessageAvailable)));
    }
}
