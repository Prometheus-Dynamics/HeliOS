use orion::{
    client::{ClientError, LocalExecutorEvent, LocalExecutorService, LocalNodeRuntime, LocalServiceRetryPolicy},
    control_plane::WorkloadRecord,
};
use tokio::{
    signal,
    sync::mpsc,
    time::{Duration, MissedTickBehavior, interval, sleep},
};
use tracing::{info, warn};

use crate::{
    config::UpdaterConfig,
    executor::{ApplyOutcome, PreparedObservation, RepartitionObservation, UpdateExecutor},
    manifest::UpdateManifest,
    model::{AssignedUpdateExecution, InvocationSource, UpdatePhase, UpdateRunState},
    provider::{OrionUpdaterPublisher, UpdaterPublishError},
    service::UpdaterRuntimeService,
    slots::probe_slot_layout,
    staging::{ArtifactStageError, ArtifactStager},
    workloads::{UpdateWorkloadDecodeError, decode_assigned_workload},
};

#[derive(Debug, thiserror::Error)]
pub enum UpdaterRuntimeError {
    #[error(transparent)]
    Publish(#[from] UpdaterPublishError),
    #[error(transparent)]
    Client(#[from] ClientError),
    #[error(transparent)]
    Stage(#[from] ArtifactStageError),
    #[error(transparent)]
    Service(#[from] crate::service::InvalidNodeId),
}

enum RuntimeEvent {
    WorkloadsChanged(Vec<WorkloadRecord>),
    WorkloadWatchStopped(ClientError),
}

const ORION_IPC_RETRY_DELAY: Duration = Duration::from_secs(2);
const ACTIVE_UPDATE_MAINTENANCE_INTERVAL: Duration = Duration::from_secs(5);
const IMAGE_MANAGED_BINARIES: &[&str] = &["helios-engine", "helios-peripherals", "helios-api", "helios-updater"];

pub struct UpdaterApp {
    config: UpdaterConfig,
    service: UpdaterRuntimeService,
    publisher: OrionUpdaterPublisher,
    node_runtime: LocalNodeRuntime,
    stager: ArtifactStager,
    executor: UpdateExecutor,
}

impl UpdaterApp {
    pub fn from_config(config: UpdaterConfig) -> Result<Self, UpdaterRuntimeError> {
        let service = UpdaterRuntimeService::new(&config)?;
        let publisher = OrionUpdaterPublisher::from_config(&config);
        let stager = ArtifactStager::from_config(&config)?;
        let executor = UpdateExecutor::from_config(&config);
        let node_runtime = LocalNodeRuntime::new(config.orion_ipc_socket_path.clone(), config.orion_ipc_stream_socket_path.clone());
        Ok(Self { config, service, publisher, node_runtime, stager, executor })
    }

    pub async fn run_until_stopped(&self) -> Result<(), UpdaterRuntimeError> {
        if let Err(error) = self.executor.ensure_image_managed_bins(IMAGE_MANAGED_BINARIES) {
            warn!(error = %error, "failed to seed image-managed service binary links");
        }

        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<RuntimeEvent>();
        loop {
            match self.publisher.register_identities().await {
                Ok(()) => break,
                Err(error) => {
                    warn!(
                        node_id = %self.config.node_id,
                        error = %error,
                        retry_delay_secs = ORION_IPC_RETRY_DELAY.as_secs(),
                        "updater failed to register provider/executor identities; retrying"
                    );
                    sleep(ORION_IPC_RETRY_DELAY).await;
                }
            }
        }

        let executor_service = LocalExecutorService::new(self.node_runtime.clone(), format!("{}-watch", self.publisher.executor_client_name()), self.publisher.executor_identity_record())
            .with_retry_policy(LocalServiceRetryPolicy::fixed_delay(ORION_IPC_RETRY_DELAY));
        let subscription = executor_service.subscribe_workloads().await?;

        let watch_error_tx = event_tx.clone();
        tokio::spawn(async move {
            if let Err(error) = pump_assigned_workloads(subscription, event_tx).await {
                let _ = watch_error_tx.send(RuntimeEvent::WorkloadWatchStopped(error));
            }
        });
        let mut current_workloads = self.bootstrap().await?;
        self.publish_current_snapshot(&current_workloads).await?;
        info!(
            node_id = %self.config.node_id,
            assigned_workload_count = current_workloads.len(),
            state_dir = %self.config.updater_state_dir.display(),
            staging_dir = %self.config.updater_staging_dir.display(),
            "helios-updater started"
        );
        let mut maintenance_tick = interval(ACTIVE_UPDATE_MAINTENANCE_INTERVAL);
        maintenance_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                _ = signal::ctrl_c() => return Ok(()),
                _ = maintenance_tick.tick(), if has_assigned_update_workload(&current_workloads, &self.config.node_id) => {
                    self.publish_current_snapshot(&current_workloads).await?;
                }
                maybe_event = event_rx.recv() => {
                    match maybe_event {
                        Some(RuntimeEvent::WorkloadsChanged(workloads)) => {
                            current_workloads = workloads;
                            self.publish_current_snapshot(&current_workloads).await?;
                        }
                        Some(RuntimeEvent::WorkloadWatchStopped(error)) => {
                            warn!(node_id = %self.config.node_id, error = %error, "updater workload watch stopped");
                        }
                        None => return Ok(()),
                    }
                }
            }
        }
    }

    async fn bootstrap(&self) -> Result<Vec<WorkloadRecord>, UpdaterRuntimeError> {
        let control = self.node_runtime.control_plane(format!("{}-bootstrap", self.publisher.executor_client_name()))?;
        loop {
            match control.fetch_state_snapshot().await {
                Ok(snapshot) => {
                    let workloads = snapshot.state.desired.workloads.values().filter(|record| matches!(decode_assigned_workload(record, &self.config.node_id), Ok(_))).cloned().collect::<Vec<_>>();
                    return Ok(workloads);
                }
                Err(error) => {
                    warn!(
                        node_id = %self.config.node_id,
                        error = %error,
                        retry_delay_secs = ORION_IPC_RETRY_DELAY.as_secs(),
                        "updater bootstrap failed to fetch Orion state snapshot; retrying"
                    );
                    sleep(ORION_IPC_RETRY_DELAY).await;
                }
            }
        }
    }

    async fn publish_current_snapshot(&self, workloads: &[WorkloadRecord]) -> Result<(), UpdaterRuntimeError> {
        let assigned_workloads = workloads
            .iter()
            .filter_map(|record| match decode_assigned_workload(record, &self.config.node_id) {
                Ok(workload) => Some(workload),
                Err(UpdateWorkloadDecodeError::WrongRuntime { .. } | UpdateWorkloadDecodeError::WrongNode { .. } | UpdateWorkloadDecodeError::NotRunning { .. }) => None,
                Err(error) => {
                    warn!(
                        node_id = %self.config.node_id,
                        error = %error,
                        workload_id = %record.workload_id.as_str(),
                        "failed to decode updater workload"
                    );
                    None
                }
            })
            .collect::<Vec<_>>();

        let state_snapshot =
            if assigned_workloads.is_empty() { None } else { self.node_runtime.control_plane(format!("{}-control", self.publisher.executor_client_name()))?.fetch_state_snapshot().await.ok() };
        let (executions, run_state) = self.plan_assigned_updates(&assigned_workloads, state_snapshot.as_ref()).await;
        let snapshot = self.service.snapshot(assigned_workloads, executions, run_state);
        self.publisher.publish_snapshot(&snapshot, workloads).await?;
        if let Err(error) = self.executor.run_pending_actions() {
            warn!(
                node_id = %self.config.node_id,
                error = %error,
                "updater deferred post-publish action failed"
            );
        }
        Ok(())
    }

    async fn plan_assigned_updates(&self, workloads: &[crate::model::UpdateWorkload], state_snapshot: Option<&orion::control_plane::StateSnapshot>) -> (Vec<AssignedUpdateExecution>, UpdateRunState) {
        if workloads.is_empty() {
            return (Vec::new(), UpdateRunState::idle());
        }

        let Some(snapshot) = state_snapshot else {
            return (
                workloads.iter().cloned().map(|workload| AssignedUpdateExecution { workload, phase: UpdatePhase::Failed, status_message: "missing Orion state snapshot".into() }).collect(),
                UpdateRunState { phase: UpdatePhase::Failed, artifact_id: None, version: None, status_message: "missing Orion state snapshot".into() },
            );
        };

        let mut executions = Vec::with_capacity(workloads.len());
        let slot_layout = probe_slot_layout().map_err(|error| error.to_string());
        for workload in workloads {
            let Some(artifact) = snapshot.state.desired.artifacts.get(workload.artifact_id.as_str()) else {
                executions.push(AssignedUpdateExecution {
                    workload: workload.clone(),
                    phase: UpdatePhase::Preflight,
                    status_message: format!("waiting for artifact {} to appear in desired state", workload.artifact_id),
                });
                continue;
            };

            let slots = match slot_layout.as_ref() {
                Ok(slots) => slots,
                Err(error) => {
                    executions.push(AssignedUpdateExecution { workload: workload.clone(), phase: UpdatePhase::Failed, status_message: error.clone() });
                    continue;
                }
            };

            match UpdateManifest::from_artifact_record(artifact).and_then(|manifest| {
                self.service.plan_manifest(&manifest, InvocationSource::OrionWorkload, Some(slots)).map(|plan| (manifest, plan)).map_err(|error| crate::manifest::ManifestResolveError::InvalidLabel {
                    artifact_id: workload.artifact_id.clone(),
                    label: "plan".into(),
                    message: error.to_string(),
                })
            }) {
                Ok((manifest, plan)) => match self.executor.failed_state(&workload.workload_id) {
                    Ok(Some(failed)) => {
                        executions.push(AssignedUpdateExecution { workload: workload.clone(), phase: UpdatePhase::Failed, status_message: failed.message });
                    }
                    Ok(None) => match self.executor.observe_prepared(&workload.workload_id, slots) {
                        Ok(Some(PreparedObservation::SwitchingBoot { message })) => {
                            executions.push(AssignedUpdateExecution { workload: workload.clone(), phase: UpdatePhase::SwitchingBoot, status_message: message })
                        }
                        Ok(Some(PreparedObservation::AwaitingBootSuccess { message })) => {
                            let uptime_secs = crate::executor::boot_uptime_secs().unwrap_or_default();
                            if uptime_secs >= self.config.boot_success_timeout_secs {
                                match self.executor.rollback_prepared(&workload.workload_id, slots) {
                                    Ok(message) => executions.push(AssignedUpdateExecution {
                                        workload: workload.clone(),
                                        phase: UpdatePhase::RollingBack,
                                        status_message: format!("{}; boot_success_timeout_secs={}", message, self.config.boot_success_timeout_secs),
                                    }),
                                    Err(error) => executions.push(AssignedUpdateExecution {
                                        workload: workload.clone(),
                                        phase: UpdatePhase::Failed,
                                        status_message: format!("rollback request failed after '{message}': {error}"),
                                    }),
                                }
                            } else {
                                executions.push(AssignedUpdateExecution {
                                    workload: workload.clone(),
                                    phase: UpdatePhase::AwaitingBootSuccess,
                                    status_message: format!("{}; boot_uptime_secs={} timeout_secs={}", message, uptime_secs, self.config.boot_success_timeout_secs),
                                })
                            }
                        }
                        Ok(Some(PreparedObservation::RollingBack { message })) => {
                            executions.push(AssignedUpdateExecution { workload: workload.clone(), phase: UpdatePhase::RollingBack, status_message: message })
                        }
                        Ok(Some(PreparedObservation::Completed { message })) => match self.executor.finalize_completed(&workload.workload_id, &manifest) {
                            Ok(message) => executions.push(AssignedUpdateExecution { workload: workload.clone(), phase: UpdatePhase::Completed, status_message: message }),
                            Err(error) => executions.push(AssignedUpdateExecution {
                                workload: workload.clone(),
                                phase: UpdatePhase::Failed,
                                status_message: format!("postboot finalize failed after '{message}': {error}"),
                            }),
                        },
                        Ok(Some(PreparedObservation::RolledBack { message })) => match self.executor.finalize_rollback(&workload.workload_id, &manifest) {
                            Ok(message) => executions.push(AssignedUpdateExecution { workload: workload.clone(), phase: UpdatePhase::RollingBack, status_message: message }),
                            Err(error) => executions.push(AssignedUpdateExecution {
                                workload: workload.clone(),
                                phase: UpdatePhase::Failed,
                                status_message: format!("rollback finalize failed after '{message}': {error}"),
                            }),
                        },
                        Ok(None) => {
                            if !matches!(plan.slot_action, crate::model::SlotAction::None) {
                                match self.executor.observe_repartition(&workload.workload_id, slots) {
                                    Ok(Some(RepartitionObservation::SwitchingBoot { message })) => {
                                        executions.push(AssignedUpdateExecution {
                                            workload: workload.clone(),
                                            phase: UpdatePhase::SwitchingBoot,
                                            status_message: format!("{}; target_slot={:?} rollback_required={}", message, plan.target_slot, plan.rollback_required),
                                        });
                                    }
                                    Ok(Some(RepartitionObservation::Completed { message })) => {
                                        executions.push(AssignedUpdateExecution {
                                            workload: workload.clone(),
                                            phase: UpdatePhase::Failed,
                                            status_message: format!("{}; slot action {:?} is still required after repartition", message, plan.slot_action),
                                        });
                                    }
                                    Ok(Some(RepartitionObservation::Failed { message })) => {
                                        executions.push(AssignedUpdateExecution { workload: workload.clone(), phase: UpdatePhase::Failed, status_message: message });
                                    }
                                    Ok(None) => match self.executor.request_repartition(&workload.workload_id, &plan.slot_action, slots) {
                                        Ok(message) => {
                                            executions.push(AssignedUpdateExecution {
                                                workload: workload.clone(),
                                                phase: UpdatePhase::SwitchingBoot,
                                                status_message: format!("{}; target_slot={:?} rollback_required={}", message, plan.target_slot, plan.rollback_required),
                                            });
                                        }
                                        Err(error) => {
                                            let message = format!("repartition request failed: {error}");
                                            let _ = self.executor.record_failed(&workload.workload_id, &message);
                                            executions.push(AssignedUpdateExecution { workload: workload.clone(), phase: UpdatePhase::Failed, status_message: message });
                                        }
                                    },
                                    Err(error) => {
                                        let message = format!("repartition state failed: {error}");
                                        let _ = self.executor.record_failed(&workload.workload_id, &message);
                                        executions.push(AssignedUpdateExecution { workload: workload.clone(), phase: UpdatePhase::Failed, status_message: message })
                                    }
                                }
                                continue;
                            }

                            match self.stager.stage_manifest(&manifest).await {
                                Ok(staged) => match self.executor.apply_staged(&workload.workload_id, &manifest, slots, &staged) {
                                    Ok(ApplyOutcome::BootPrepared { message }) => executions.push(AssignedUpdateExecution {
                                        workload: workload.clone(),
                                        phase: UpdatePhase::SwitchingBoot,
                                        status_message: format!("{}; target_slot={:?} rollback_required={}", message, plan.target_slot, plan.rollback_required),
                                    }),
                                    Ok(ApplyOutcome::Completed { message }) => executions.push(AssignedUpdateExecution {
                                        workload: workload.clone(),
                                        phase: UpdatePhase::Completed,
                                        status_message: format!("{}; target_slot={:?} rollback_required={}", message, plan.target_slot, plan.rollback_required),
                                    }),
                                    Err(error) => {
                                        let message = format!("apply failed: {error}");
                                        let _ = self.executor.record_failed(&workload.workload_id, &message);
                                        executions.push(AssignedUpdateExecution { workload: workload.clone(), phase: UpdatePhase::Failed, status_message: message });
                                    }
                                },
                                Err(error) => {
                                    let message = format!("staging failed: {error}");
                                    let _ = self.executor.record_failed(&workload.workload_id, &message);
                                    executions.push(AssignedUpdateExecution { workload: workload.clone(), phase: UpdatePhase::Failed, status_message: message });
                                }
                            }
                        }
                        Err(error) => {
                            let message = format!("prepared state failed: {error}");
                            let _ = self.executor.record_failed(&workload.workload_id, &message);
                            executions.push(AssignedUpdateExecution { workload: workload.clone(), phase: UpdatePhase::Failed, status_message: message });
                        }
                    },
                    Err(error) => executions.push(AssignedUpdateExecution { workload: workload.clone(), phase: UpdatePhase::Failed, status_message: format!("failed state check failed: {error}") }),
                },
                Err(error) => {
                    let message = error.to_string();
                    let _ = self.executor.record_failed(&workload.workload_id, &message);
                    executions.push(AssignedUpdateExecution { workload: workload.clone(), phase: UpdatePhase::Failed, status_message: message });
                }
            }
        }

        let failed = executions.iter().filter(|execution| execution.phase == UpdatePhase::Failed).count();
        let rolling_back = executions.iter().filter(|execution| execution.phase == UpdatePhase::RollingBack).count();
        let awaiting_boot_success = executions.iter().filter(|execution| execution.phase == UpdatePhase::AwaitingBootSuccess).count();
        let switching = executions.iter().filter(|execution| execution.phase == UpdatePhase::SwitchingBoot).count();
        let completed = executions.iter().filter(|execution| execution.phase == UpdatePhase::Completed).count();
        let preflight = executions.iter().filter(|execution| execution.phase == UpdatePhase::Preflight).count();
        let phase = if failed > 0 {
            UpdatePhase::Failed
        } else if rolling_back > 0 {
            UpdatePhase::RollingBack
        } else if awaiting_boot_success > 0 {
            UpdatePhase::AwaitingBootSuccess
        } else if switching > 0 {
            UpdatePhase::SwitchingBoot
        } else if preflight > 0 {
            UpdatePhase::Preflight
        } else if completed > 0 {
            UpdatePhase::Completed
        } else {
            UpdatePhase::Idle
        };
        let run_state = UpdateRunState {
            phase,
            artifact_id: executions.first().map(|execution| execution.workload.artifact_id.clone()),
            version: executions.first().map(|execution| execution.workload.version.clone()),
            status_message: format!("completed={completed} awaiting_boot_success={awaiting_boot_success} switching={switching} preflight={preflight} rolling_back={rolling_back} failed={failed}"),
        };

        (executions, run_state)
    }
}

fn has_assigned_update_workload(workloads: &[WorkloadRecord], node_id: &str) -> bool {
    workloads.iter().any(|record| matches!(decode_assigned_workload(record, node_id), Ok(_)))
}

async fn pump_assigned_workloads(mut subscription: orion::client::LocalExecutorSubscription, event_tx: mpsc::UnboundedSender<RuntimeEvent>) -> Result<(), ClientError> {
    loop {
        match subscription.next_event().await? {
            LocalExecutorEvent::Bootstrap(workloads) | LocalExecutorEvent::WorkloadsChanged { workloads, .. } => {
                if event_tx.send(RuntimeEvent::WorkloadsChanged(workloads)).is_err() {
                    return Ok(());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{UpdateArtifactClass, UpdateWorkload};
    use orion::control_plane::{AppliedClusterState, ClusterStateEnvelope, DesiredClusterState, ObservedClusterState, StateSnapshot};

    #[tokio::test]
    async fn missing_artifact_is_retryable_preflight_not_failed() {
        let app = UpdaterApp::from_config(UpdaterConfig::default()).expect("app");
        let workload = UpdateWorkload {
            workload_id: "workload.update.test".into(),
            artifact_id: "artifact.update.test".into(),
            assigned_node_id: "node-local".into(),
            version: "v2026.2.0-test".into(),
            artifact_class: UpdateArtifactClass::OsImage,
        };
        let snapshot = StateSnapshot { state: ClusterStateEnvelope::new(DesiredClusterState::default(), ObservedClusterState::default(), AppliedClusterState::default()) };

        let (executions, run_state) = app.plan_assigned_updates(&[workload.clone()], Some(&snapshot)).await;

        assert_eq!(executions.len(), 1);
        assert_eq!(executions[0].phase, UpdatePhase::Preflight);
        assert!(executions[0].status_message.contains("waiting for artifact"));
        assert_eq!(run_state.phase, UpdatePhase::Preflight);
        assert!(run_state.status_message.contains("preflight=1"));
    }
}
