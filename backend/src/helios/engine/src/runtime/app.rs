use std::time::{SystemTime, UNIX_EPOCH};

use orion::{
    client::{ClientError, ControlPlaneEventStream, LocalExecutorEvent, LocalExecutorService, LocalNodeRuntime, LocalServiceRetryPolicy},
    control_plane::{ClientEventKind, StateSnapshot, WorkloadRecord},
};
use tokio::{
    signal,
    sync::mpsc,
    time::{Duration, interval, sleep},
};
use tracing::{info, warn};

use crate::{
    config::EngineConfig,
    execution::ResidentExecutionSet,
    model::{EngineSnapshot, ExecutionSessionState, ExecutionSessionStatus, ExecutionWorkload, GraphRef},
    plugins::{PluginLoadError, PluginLoadResult, discover_plugin_libraries, load_plugins},
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
    StateSnapshotUpdated(StateSnapshot),
    WorkloadWatchStopped(ClientError),
    StateWatchStopped(ClientError),
}

const ORION_IPC_RETRY_DELAY: Duration = Duration::from_secs(2);
const ORION_EMPTY_WATCH_RETRY_DELAY: Duration = Duration::from_millis(200);
const ENGINE_RECONCILE_INTERVAL: Duration = Duration::from_secs(1);

pub struct EngineApp {
    config: EngineConfig,
    publisher: OrionEnginePublisher,
    node_runtime: LocalNodeRuntime,
}

impl EngineApp {
    pub fn from_config(config: EngineConfig) -> Result<Self, EngineRuntimeError> {
        let publisher = OrionEnginePublisher::from_config(&config);
        let node_runtime = LocalNodeRuntime::new(config.orion_ipc_socket_path.clone(), config.orion_ipc_stream_socket_path.clone());
        Ok(Self { config, publisher, node_runtime })
    }

    pub async fn run_until_stopped(&self) -> Result<(), EngineRuntimeError> {
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
                Err(ClientError::NoMessageAvailable) => {
                    sleep(ORION_EMPTY_WATCH_RETRY_DELAY).await;
                }
                Err(error) => return Err(error.into()),
            }
        };
        let mut current_workloads = loop {
            match subscription.next_event().await {
                Ok(LocalExecutorEvent::Bootstrap(workloads)) => break workloads,
                Ok(LocalExecutorEvent::WorkloadsChanged { workloads, .. }) => break workloads,
                Err(ClientError::NoMessageAvailable) => {
                    sleep(ORION_EMPTY_WATCH_RETRY_DELAY).await;
                }
                Err(error) => return Err(error.into()),
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
                Err(ClientError::NoMessageAvailable) => {
                    sleep(ORION_EMPTY_WATCH_RETRY_DELAY).await;
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            match stream.subscribe_state(orion::core::Revision::ZERO).await {
                Ok(()) => break stream,
                Err(ClientError::NoMessageAvailable) => {
                    sleep(ORION_EMPTY_WATCH_RETRY_DELAY).await;
                }
                Err(error) => return Err(error.into()),
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
        let mut execution_sessions = ResidentExecutionSet::default();
        let mut reconcile_tick = interval(ENGINE_RECONCILE_INTERVAL);
        let mut execution_tick = interval(Duration::from_millis(self.config.execution_interval_ms.max(1)));
        self.publish_snapshot_resilient(&plugins, &mut execution_sessions, &current_workloads, current_state_snapshot.as_ref()).await?;
        info!(
            node_id = %self.config.node_id,
            plugin_count = plugins.builtins.len() + plugins.libraries.len(),
            assigned_workload_count = current_workloads.len(),
            engine_socket = %self.config.engine_socket_path.display(),
            "helios-engine started"
        );

        loop {
            tokio::select! {
                _ = signal::ctrl_c() => {
                    return Ok(());
                }
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
                            self.publish_snapshot_resilient(&plugins, &mut execution_sessions, &current_workloads, current_state_snapshot.as_ref()).await?;
                        }
                    }
                }
                _ = execution_tick.tick() => {
                    self.publish_snapshot_resilient(&plugins, &mut execution_sessions, &current_workloads, current_state_snapshot.as_ref()).await?;
                }
                maybe_event = event_rx.recv() => {
                    match maybe_event {
                        Some(RuntimeEvent::WorkloadsChanged(workloads)) => {
                            current_workloads = workloads;
                            self.publish_snapshot_resilient(&plugins, &mut execution_sessions, &current_workloads, current_state_snapshot.as_ref()).await?;
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
                            current_state_snapshot = Some(snapshot);
                            if should_publish {
                                self.publish_snapshot_resilient(&plugins, &mut execution_sessions, &current_workloads, current_state_snapshot.as_ref()).await?;
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

        let load_result = load_plugins(&plugin_paths)?;
        log_loaded_plugins(&load_result);
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
        let mut execution = execution_sessions.tick_workloads(&self.config, &plugins.registry, &plugins.host_manager, &loaded_plugins, &decoded.runnable, state_snapshot, observed_at_ms);
        execution.sessions.extend(decoded.decode_failures);
        let snapshot = EngineSnapshot {
            node_id: self.config.node_id.clone(),
            engine_socket_path: self.config.engine_socket_path.clone(),
            plugin_dirs: self.config.plugin_dirs.clone(),
            loaded_plugins,
            assigned_workloads: decoded.runnable,
            sessions: execution.sessions,
            artifacts: execution.artifacts,
        };
        self.publisher.publish_snapshot(&snapshot, workloads).await?;
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
                        && event_tx.send(RuntimeEvent::StateSnapshotUpdated(*snapshot)).is_err()
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
            abi_version = plugin.abi_version.unwrap_or_default(),
            "loaded built-in Daedalus plugin"
        );
    }

    for plugin in &plugins.libraries {
        let metadata = plugin.metadata();
        info!(
            plugin_path = %metadata.path.display(),
            plugin_name = metadata.plugin_name.as_deref().unwrap_or("unknown"),
            plugin_version = metadata.plugin_version.as_deref().unwrap_or("unknown"),
            abi_version = metadata.abi_version.unwrap_or_default(),
            "loaded external Daedalus plugin"
        );
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

fn is_retryable_no_message(error: &EngineRuntimeError) -> bool {
    matches!(error, EngineRuntimeError::Client(ClientError::NoMessageAvailable)) || matches!(error, EngineRuntimeError::Publish(EnginePublishError::Client(ClientError::NoMessageAvailable)))
}
